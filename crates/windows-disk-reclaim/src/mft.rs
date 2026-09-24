//! Sequential NTFS $MFT reader. Opens the volume, follows $MFT data runs, and
//! reconstructs unique allocated sizes without walking the directory tree.

use crate::drive_root;
use std::io::{self, Error, ErrorKind};
use std::mem::size_of;
use std::path::PathBuf;

const FSCTL_GET_NTFS_VOLUME_DATA: u32 = 0x0009_0064;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_END: u32 = 0xFFFF_FFFF;
const NS_DOS: u8 = 2;
const ROOT_INDEX: u64 = 5;
const FILE_ATTR_REPARSE: u32 = 0x400;

type Ranked = Vec<(PathBuf, u64)>;

#[repr(C)]
#[derive(Clone, Copy)]
struct NtfsVolumeData {
    volume_serial: i64,
    number_sectors: i64,
    total_clusters: i64,
    free_clusters: i64,
    total_reserved: i64,
    bytes_per_sector: u32,
    bytes_per_cluster: u32,
    bytes_per_file_record: u32,
    clusters_per_file_record: u32,
    mft_valid_data_length: i64,
    mft_start_lcn: i64,
    mft2_start_lcn: i64,
    mft_zone_start: i64,
    mft_zone_end: i64,
}

#[derive(Clone, Debug, Default)]
pub struct MftNode {
    pub allocated: u64,
    pub is_dir: bool,
    pub is_reparse: bool,
    pub names: Vec<(u64, String)>,
}

#[derive(Debug)]
pub struct MftIndex {
    pub drive: char,
    pub nodes: Vec<Option<MftNode>>,
    pub records_in_use: u64,
    subtree: Vec<u64>,
    children: Vec<Vec<u64>>,
}

impl MftIndex {
    pub fn path_of(&self, index: u64) -> Option<PathBuf> {
        let mut parts = Vec::new();
        let mut current = index;
        for _ in 0..4096 {
            if current == ROOT_INDEX {
                break;
            }
            let node = self.nodes.get(current as usize)?.as_ref()?;
            let (parent, name) = node.names.first()?;
            if parent == &current {
                break;
            }
            parts.push(name.clone());
            current = *parent;
        }
        parts.reverse();
        let mut path = drive_root(self.drive);
        for part in parts {
            path.push(part);
        }
        Some(path)
    }

    fn finish(&mut self) {
        let len = self.nodes.len();
        self.children = vec![Vec::new(); len];
        self.subtree = vec![0u64; len];
        let mut attached = vec![false; len];
        for (id, slot) in self.nodes.iter().enumerate() {
            let Some(node) = slot else {
                continue;
            };
            let Some((parent, _)) = node.names.first() else {
                continue;
            };
            let parent = *parent as usize;
            if parent < len && parent != id && !attached[id] {
                self.children[parent].push(id as u64);
                attached[id] = true;
            }
        }
        let mut state = vec![0u8; len];
        rec(
            ROOT_INDEX as usize,
            &self.nodes,
            &self.children,
            &mut self.subtree,
            &mut state,
        );
    }

    pub fn ranked(&self, top: usize) -> (Ranked, Ranked, Ranked) {
        let mut top_level = Vec::new();
        if let Some(kids) = self.children.get(ROOT_INDEX as usize) {
            for child in kids {
                let Some(node) = self
                    .nodes
                    .get(*child as usize)
                    .and_then(|slot| slot.as_ref())
                else {
                    continue;
                };
                if skip_rank(node) {
                    continue;
                }
                if let Some(path) = self.path_of(*child) {
                    top_level.push((
                        path,
                        self.subtree.get(*child as usize).copied().unwrap_or(0),
                    ));
                }
            }
        }
        top_level.sort_by_key(|item| std::cmp::Reverse(item.1));
        top_level.truncate(top);

        let mut dir_sizes = Vec::new();
        let mut file_sizes = Vec::new();
        for (id, slot) in self.nodes.iter().enumerate() {
            let Some(node) = slot else {
                continue;
            };
            if skip_rank(node) {
                continue;
            }
            if node.is_dir {
                if id as u64 != ROOT_INDEX {
                    dir_sizes.push((id as u64, self.subtree.get(id).copied().unwrap_or(0)));
                }
            } else if !node.is_reparse {
                file_sizes.push((id as u64, node.allocated));
            }
        }
        dir_sizes.sort_by_key(|item| std::cmp::Reverse(item.1));
        file_sizes.sort_by_key(|item| std::cmp::Reverse(item.1));
        let dirs = dir_sizes
            .into_iter()
            .filter_map(|(id, size)| Some((self.path_of(id)?, size)))
            .take(top)
            .collect();
        let files = file_sizes
            .into_iter()
            .filter_map(|(id, size)| Some((self.path_of(id)?, size)))
            .take(top)
            .collect();
        (top_level, dirs, files)
    }

    pub fn subtree_of_path(&self, path: &std::path::Path) -> Option<u64> {
        let text = crate::win_text(path);
        let rest = text.get(2..)?.trim_start_matches('\\');
        let mut id = ROOT_INDEX;
        if !rest.is_empty() {
            for part in rest.split('\\') {
                id = self.child_named(id, part)?;
            }
        }
        self.subtree.get(id as usize).copied()
    }

    fn child_named(&self, parent: u64, name: &str) -> Option<u64> {
        let kids = self.children.get(parent as usize)?;
        kids.iter().copied().find(|id| {
            self.nodes
                .get(*id as usize)
                .and_then(|slot| slot.as_ref())
                .is_some_and(|node| {
                    node.names
                        .iter()
                        .any(|(_, candidate)| candidate.eq_ignore_ascii_case(name))
                })
        })
    }
}

pub fn read_volume(drive: char) -> io::Result<MftIndex> {
    #[cfg(windows)]
    {
        windows_read(drive)
    }
    #[cfg(not(windows))]
    {
        let _ = drive;
        Err(Error::new(
            ErrorKind::Unsupported,
            "NTFS MFT snapshot requires Windows",
        ))
    }
}

#[cfg(windows)]
fn windows_read(drive: char) -> io::Result<MftIndex> {
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    struct Owned(HANDLE);
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    let letter = drive.to_ascii_uppercase();
    let mut wide: Vec<u16> = format!(r"\\.\{letter}:").encode_utf16().collect();
    wide.push(0);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_SEQUENTIAL_SCAN,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(Error::last_os_error());
    }
    let owned = Owned(handle);

    let mut volume = NtfsVolumeData {
        volume_serial: 0,
        number_sectors: 0,
        total_clusters: 0,
        free_clusters: 0,
        total_reserved: 0,
        bytes_per_sector: 0,
        bytes_per_cluster: 0,
        bytes_per_file_record: 0,
        clusters_per_file_record: 0,
        mft_valid_data_length: 0,
        mft_start_lcn: 0,
        mft2_start_lcn: 0,
        mft_zone_start: 0,
        mft_zone_end: 0,
    };
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            owned.0,
            FSCTL_GET_NTFS_VOLUME_DATA,
            std::ptr::null(),
            0,
            &mut volume as *mut NtfsVolumeData as *mut _,
            size_of::<NtfsVolumeData>() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(Error::last_os_error());
    }
    if volume.bytes_per_cluster == 0 || volume.bytes_per_file_record == 0 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "NTFS volume data is incomplete",
        ));
    }
    let record_size = volume.bytes_per_file_record as usize;
    let cluster = volume.bytes_per_cluster as u64;
    let mft_offset = volume.mft_start_lcn as u64 * cluster;
    let mut first = vec![0u8; record_size];
    read_at(owned.0, mft_offset, &mut first)?;
    apply_usa(&mut first, volume.bytes_per_sector as usize);
    let runs = mft_runs(&first)
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "unable to parse $MFT data runs"))?;

    let valid = volume.mft_valid_data_length.max(0) as u64;
    let mut remaining = valid;
    let mut leftover = Vec::new();
    let mut nodes: Vec<Option<MftNode>> = Vec::new();
    let mut in_use = 0u64;
    let mut index = 0u64;
    let mut chunk = vec![0u8; 8 * 1024 * 1024];

    for run in runs {
        if remaining == 0 {
            break;
        }
        if run.sparse {
            let skip = (run.clusters * cluster).min(remaining);
            remaining -= skip;
            continue;
        }
        let mut run_bytes = run.clusters * cluster;
        let mut disk = run.lcn * cluster;
        while run_bytes > 0 && remaining > 0 {
            let want = chunk.len().min(run_bytes as usize).min(remaining as usize);
            read_at(owned.0, disk, &mut chunk[..want])?;
            leftover.extend_from_slice(&chunk[..want]);
            let mut offset = 0;
            while offset + record_size <= leftover.len() {
                apply_usa(
                    &mut leftover[offset..offset + record_size],
                    volume.bytes_per_sector as usize,
                );
                if let Some(node) = parse_record(&leftover[offset..offset + record_size])
                    && !node.names.is_empty()
                {
                    let slot = index as usize;
                    if nodes.len() <= slot {
                        nodes.resize_with(slot + 1, || None);
                    }
                    nodes[slot] = Some(node);
                    in_use += 1;
                }
                index += 1;
                offset += record_size;
            }
            leftover.copy_within(offset.., 0);
            leftover.truncate(leftover.len() - offset);
            disk += want as u64;
            run_bytes -= want as u64;
            remaining -= want as u64;
        }
    }

    let mut index = MftIndex {
        drive: letter,
        nodes,
        records_in_use: in_use,
        subtree: Vec::new(),
        children: Vec::new(),
    };
    index.finish();
    Ok(index)
}

#[cfg(windows)]
fn read_at(
    handle: windows_sys::Win32::Foundation::HANDLE,
    offset: u64,
    buf: &mut [u8],
) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{ReadFile, SetFilePointerEx};
    let mut got = 0u32;
    let mut filled = 0usize;
    while filled < buf.len() {
        let pos = offset + filled as u64;
        let ok_seek = unsafe { SetFilePointerEx(handle, pos as i64, std::ptr::null_mut(), 0) };
        if ok_seek == 0 {
            return Err(Error::last_os_error());
        }
        let ok_read = unsafe {
            ReadFile(
                handle,
                buf[filled..].as_mut_ptr() as *mut _,
                (buf.len() - filled) as u32,
                &mut got,
                std::ptr::null_mut(),
            )
        };
        if ok_read == 0 {
            return Err(Error::last_os_error());
        }
        if got == 0 {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "volume read ended before $MFT was complete",
            ));
        }
        filled += got as usize;
    }
    Ok(())
}

struct DataRun {
    lcn: u64,
    clusters: u64,
    sparse: bool,
}

fn apply_usa(record: &mut [u8], bytes_per_sector: usize) {
    if record.len() < 8 || bytes_per_sector < 2 {
        return;
    }
    let usa_offset = u16::from_le_bytes(record[4..6].try_into().unwrap()) as usize;
    let usa_count = u16::from_le_bytes(record[6..8].try_into().unwrap()) as usize;
    if usa_count < 2 || usa_offset + usa_count * 2 > record.len() {
        return;
    }
    for i in 1..usa_count {
        let sector_end = i.saturating_mul(bytes_per_sector);
        let src = usa_offset + i * 2;
        if sector_end < 2 || sector_end > record.len() || src + 1 >= record.len() {
            return;
        }
        record[sector_end - 2] = record[src];
        record[sector_end - 1] = record[src + 1];
    }
}

fn mft_runs(record: &[u8]) -> Option<Vec<DataRun>> {
    let mut offset = u16::from_le_bytes(record.get(20..22)?.try_into().ok()?) as usize;
    let used = u32::from_le_bytes(record.get(24..28)?.try_into().ok()?) as usize;
    while offset + 8 <= record.len().min(used) {
        let kind = u32::from_le_bytes(record[offset..offset + 4].try_into().ok()?);
        if kind == ATTR_END {
            break;
        }
        let size = u32::from_le_bytes(record[offset + 4..offset + 8].try_into().ok()?) as usize;
        if size < 8 || offset + size > record.len() {
            break;
        }
        let non_resident = record[offset + 8];
        let name_len = record[offset + 9];
        if kind == ATTR_DATA && non_resident == 1 && name_len == 0 {
            let pairs_offset =
                u16::from_le_bytes(record[offset + 32..offset + 34].try_into().ok()?) as usize;
            return parse_runs(record.get(offset + pairs_offset..offset + size)?);
        }
        offset += size;
    }
    None
}

fn parse_runs(mut pairs: &[u8]) -> Option<Vec<DataRun>> {
    let mut runs = Vec::new();
    let mut lcn: i64 = 0;
    while let Some((&header, rest)) = pairs.split_first() {
        if header == 0 {
            break;
        }
        let len_size = (header & 0x0F) as usize;
        let off_size = ((header >> 4) & 0x0F) as usize;
        if len_size == 0 || rest.len() < len_size + off_size {
            return None;
        }
        let clusters = read_unsigned(&rest[..len_size])?;
        pairs = &rest[len_size..];
        if off_size == 0 {
            runs.push(DataRun {
                lcn: 0,
                clusters,
                sparse: true,
            });
            continue;
        }
        let delta = read_signed(&pairs[..off_size])?;
        pairs = &pairs[off_size..];
        lcn += delta;
        if lcn < 0 {
            return None;
        }
        runs.push(DataRun {
            lcn: lcn as u64,
            clusters,
            sparse: false,
        });
    }
    if runs.is_empty() { None } else { Some(runs) }
}

fn read_unsigned(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let mut buf = [0u8; 8];
    buf[..bytes.len()].copy_from_slice(bytes);
    Some(u64::from_le_bytes(buf))
}

fn read_signed(bytes: &[u8]) -> Option<i64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let mut buf = [0u8; 8];
    buf[..bytes.len()].copy_from_slice(bytes);
    if bytes.last()? & 0x80 != 0 {
        for slot in buf.iter_mut().skip(bytes.len()) {
            *slot = 0xFF;
        }
    }
    Some(i64::from_le_bytes(buf))
}

pub fn parse_record(record: &[u8]) -> Option<MftNode> {
    if record.len() < 48 || &record[0..4] != b"FILE" {
        return None;
    }
    let flags = u16::from_le_bytes(record[22..24].try_into().ok()?);
    if flags & 0x01 == 0 {
        return None;
    }
    let base = u64::from_le_bytes(record[32..40].try_into().ok()?);
    if base & 0x0000_FFFF_FFFF_FFFF != 0 {
        return None;
    }
    let mut offset = u16::from_le_bytes(record[20..22].try_into().ok()?) as usize;
    let used = u32::from_le_bytes(record[24..28].try_into().ok()?) as usize;
    let mut node = MftNode {
        allocated: 0,
        is_dir: flags & 0x02 != 0,
        is_reparse: false,
        names: Vec::new(),
    };
    while offset + 8 <= record.len().min(used) {
        let kind = u32::from_le_bytes(record[offset..offset + 4].try_into().ok()?);
        if kind == ATTR_END {
            break;
        }
        let size = u32::from_le_bytes(record[offset + 4..offset + 8].try_into().ok()?) as usize;
        if size < 16 || offset + size > record.len() {
            break;
        }
        let non_resident = record[offset + 8];
        let name_len = record[offset + 9];
        match kind {
            ATTR_FILE_NAME if non_resident == 0 => {
                if let Some((parent, name, namespace, reparse)) =
                    parse_file_name(&record[offset..offset + size])
                {
                    node.is_reparse |= reparse;
                    if namespace == NS_DOS {
                        if node.names.is_empty() {
                            node.names.push((parent, name));
                        }
                    } else {
                        node.names.insert(0, (parent, name));
                    }
                }
            }
            ATTR_DATA => {
                if let Some(allocated) =
                    data_allocated(&record[offset..offset + size], name_len, non_resident)
                {
                    node.allocated = node.allocated.saturating_add(allocated);
                }
            }
            _ => {}
        }
        offset += size;
    }
    if node.names.is_empty() {
        None
    } else {
        Some(node)
    }
}

fn parse_file_name(attr: &[u8]) -> Option<(u64, String, u8, bool)> {
    if attr.len() < 24 {
        return None;
    }
    let value_len = u32::from_le_bytes(attr[16..20].try_into().ok()?) as usize;
    let value_off = u16::from_le_bytes(attr[20..22].try_into().ok()?) as usize;
    let value = attr.get(value_off..value_off + value_len)?;
    if value.len() < 66 {
        return None;
    }
    let parent = u64::from_le_bytes(value[0..8].try_into().ok()?) & 0x0000_FFFF_FFFF_FFFF;
    let flags = u32::from_le_bytes(value[56..60].try_into().ok()?);
    let name_len = value[64] as usize;
    let namespace = value[65];
    let name_bytes = value.get(66..66 + name_len * 2)?;
    let units: Vec<u16> = name_bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|chunk| u16::from_le_bytes(*chunk))
        .collect();
    let name = String::from_utf16_lossy(&units);
    if name.is_empty() {
        return None;
    }
    Some((parent, name, namespace, flags & FILE_ATTR_REPARSE != 0))
}

fn data_allocated(attr: &[u8], name_len: u8, non_resident: u8) -> Option<u64> {
    let _ = name_len;
    if non_resident == 0 {
        if attr.len() < 20 {
            return None;
        }
        Some(u32::from_le_bytes(attr[16..20].try_into().ok()?) as u64)
    } else {
        if attr.len() < 56 {
            return None;
        }
        Some(u64::from_le_bytes(attr[40..48].try_into().ok()?))
    }
}

fn rec(
    id: usize,
    nodes: &[Option<MftNode>],
    children: &[Vec<u64>],
    sizes: &mut [u64],
    state: &mut [u8],
) -> u64 {
    if id >= state.len() {
        return 0;
    }
    if state[id] == 2 {
        return sizes[id];
    }
    if state[id] == 1 {
        return 0;
    }
    state[id] = 1;
    let mut total = 0u64;
    if let Some(Some(node)) = nodes.get(id)
        && !node.is_reparse
    {
        if !skip_rank(node) {
            total = node.allocated;
        }
        if let Some(kids) = children.get(id) {
            for child in kids {
                total = total.saturating_add(rec(*child as usize, nodes, children, sizes, state));
            }
        }
    }
    sizes[id] = total;
    state[id] = 2;
    total
}

fn skip_rank(node: &MftNode) -> bool {
    node.names.iter().any(|(parent, name)| {
        *parent == ROOT_INDEX && name.starts_with('$') && !name.eq_ignore_ascii_case("$Recycle.Bin")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_u16(buf: &mut [u8], at: usize, value: u16) {
        buf[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn write_u32(buf: &mut [u8], at: usize, value: u32) {
        buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    fn write_u64(buf: &mut [u8], at: usize, value: u64) {
        buf[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn parses_resident_file_record() {
        let mut record = vec![0u8; 1024];
        record[0..4].copy_from_slice(b"FILE");
        write_u16(&mut record, 20, 0x30);
        write_u16(&mut record, 22, 0x01);
        write_u32(&mut record, 24, 256);
        write_u32(&mut record, 28, 1024);

        let attr = 0x30;
        write_u32(&mut record, attr, ATTR_FILE_NAME);
        write_u32(&mut record, attr + 4, 96);
        record[attr + 8] = 0;
        write_u32(&mut record, attr + 16, 70);
        write_u16(&mut record, attr + 20, 24);
        let value = attr + 24;
        write_u64(&mut record, value, 5);
        record[value + 64] = 1;
        record[value + 65] = 1;
        write_u16(&mut record, value + 66, u16::from(b'a'));

        let data = attr + 96;
        write_u32(&mut record, data, ATTR_DATA);
        write_u32(&mut record, data + 4, 24);
        record[data + 8] = 0;
        write_u32(&mut record, data + 16, 4096);
        write_u32(&mut record, data + 24, ATTR_END);

        let node = parse_record(&record).unwrap();
        assert_eq!(node.names[0].0, 5);
        assert_eq!(node.names[0].1, "a");
        assert_eq!(node.allocated, 4096);
        assert!(!node.is_dir);
    }

    #[test]
    fn signed_run_delta_sign_extends() {
        assert_eq!(read_signed(&[0xFF]).unwrap(), -1);
        assert_eq!(read_unsigned(&[0x10]).unwrap(), 16);
    }
}
