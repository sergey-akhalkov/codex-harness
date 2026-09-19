use crate::CategoryReport;
use crate::drive_root;
use crate::mft::{self, MftIndex};
use crate::reclaim::default_categories;
use serde::Serialize;
use std::fs;
use std::io::{self, Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Auto,
    Mft,
    Walk,
}

impl Method {
    pub fn parse(value: &str) -> io::Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "mft" => Ok(Self::Mft),
            "walk" => Ok(Self::Walk),
            _ => Err(Error::new(
                ErrorKind::InvalidInput,
                "method must be auto, mft or walk",
            )),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct VolumeInfo {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct UsageEntry {
    pub path: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SnapshotReport {
    pub drive: char,
    pub method: String,
    pub elapsed_ms: u128,
    pub volume: VolumeInfo,
    pub file_records: u64,
    pub top_level: Vec<UsageEntry>,
    pub top_directories: Vec<UsageEntry>,
    pub top_files: Vec<UsageEntry>,
    pub reclaimable_bytes: u64,
    pub reclaimable: Vec<crate::CategoryReport>,
}

pub struct SnapshotOptions {
    pub drive: char,
    pub root: Option<PathBuf>,
    pub top: usize,
    pub method: Method,
}

pub fn snapshot(options: SnapshotOptions) -> io::Result<SnapshotReport> {
    let started = Instant::now();
    let drive = options.drive.to_ascii_uppercase();
    let volume = volume_info(drive)?;
    if let Some(root) = &options.root {
        return walk_snapshot(drive, root, options.top, started, volume);
    }
    if options.method == Method::Walk {
        return walk_snapshot(drive, &drive_root(drive), options.top, started, volume);
    }
    match scan_mft(drive, options.top, volume.clone()) {
        Ok(report) => Ok(with_elapsed(report, started)),
        Err(err) if options.method == Method::Mft => Err(err),
        Err(_) => walk_snapshot(drive, &drive_root(drive), options.top, started, volume),
    }
}

fn with_elapsed(mut report: SnapshotReport, started: Instant) -> SnapshotReport {
    report.elapsed_ms = started.elapsed().as_millis();
    report
}

fn walk_snapshot(
    drive: char,
    root: &Path,
    top: usize,
    started: Instant,
    volume: VolumeInfo,
) -> io::Result<SnapshotReport> {
    let (dirs, files, records) = walk_usage(root, top)?;
    Ok(SnapshotReport {
        drive,
        method: "walk".into(),
        elapsed_ms: started.elapsed().as_millis(),
        volume,
        file_records: records,
        top_level: Vec::new(),
        top_directories: dirs,
        top_files: files,
        reclaimable_bytes: 0,
        reclaimable: Vec::new(),
    })
}

fn scan_mft(drive: char, top: usize, volume: VolumeInfo) -> io::Result<SnapshotReport> {
    let index: MftIndex = mft::read_volume(drive)?;
    let (top_level, dirs, files) = index.ranked(top);
    let reclaimable: Vec<CategoryReport> = default_categories(drive)
        .into_iter()
        .map(|category| {
            let bytes = index.subtree_of_path(&category.path).unwrap_or(0);
            CategoryReport {
                id: category.id.to_string(),
                description: category.description.to_string(),
                path: category.path.to_string_lossy().into_owned(),
                candidate_files: 0,
                candidate_bytes: bytes,
                deleted_files: 0,
                freed_bytes: 0,
                skipped_files: 0,
            }
        })
        .collect();
    Ok(SnapshotReport {
        drive,
        method: "mft".into(),
        elapsed_ms: 0,
        volume,
        file_records: index.records_in_use,
        top_level: to_entries(top_level),
        top_directories: to_entries(dirs),
        top_files: to_entries(files),
        reclaimable_bytes: reclaimable.iter().map(|item| item.candidate_bytes).sum(),
        reclaimable,
    })
}

fn to_entries(items: Vec<(PathBuf, u64)>) -> Vec<UsageEntry> {
    items
        .into_iter()
        .map(|(path, bytes)| UsageEntry {
            path: path.to_string_lossy().into_owned(),
            bytes,
        })
        .collect()
}

fn walk_usage(root: &Path, top: usize) -> io::Result<(Vec<UsageEntry>, Vec<UsageEntry>, u64)> {
    let mut dir_sizes: Vec<(PathBuf, u64)> = Vec::new();
    let mut files: Vec<(PathBuf, u64)> = Vec::new();
    let mut records = 0u64;
    let total = walk_dir(root, &mut dir_sizes, &mut files, &mut records)?;
    dir_sizes.push((root.to_path_buf(), total));
    dir_sizes.sort_by_key(|item| std::cmp::Reverse(item.1));
    files.sort_by_key(|item| std::cmp::Reverse(item.1));
    Ok((
        to_entries(
            dir_sizes
                .into_iter()
                .filter(|(path, _)| path != root)
                .take(top)
                .collect(),
        ),
        to_entries(files.into_iter().take(top).collect()),
        records,
    ))
}

fn walk_dir(
    path: &Path,
    dirs: &mut Vec<(PathBuf, u64)>,
    files: &mut Vec<(PathBuf, u64)>,
    records: &mut u64,
) -> io::Result<u64> {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return Ok(0),
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        let child = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if is_reparse(&meta) {
            continue;
        }
        *records += 1;
        if meta.is_dir() {
            let size = walk_dir(&child, dirs, files, records)?;
            dirs.push((child, size));
            total = total.saturating_add(size);
        } else if meta.is_file() {
            let size = file_size(&meta);
            files.push((child, size));
            total = total.saturating_add(size);
        }
    }
    Ok(total)
}

fn is_reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn file_size(meta: &fs::Metadata) -> u64 {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_size()
    }
    #[cfg(not(windows))]
    {
        meta.len()
    }
}

fn volume_info(drive: char) -> io::Result<VolumeInfo> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let mut wide: Vec<u16> = drive_root(drive).to_string_lossy().encode_utf16().collect();
        wide.push(0);
        let mut free = 0u64;
        let mut total = 0u64;
        let mut caller = 0u64;
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut caller, &mut total, &mut free) };
        if ok == 0 {
            return Err(Error::last_os_error());
        }
        Ok(VolumeInfo {
            total_bytes: total,
            free_bytes: free,
            used_bytes: total.saturating_sub(free),
        })
    }
    #[cfg(not(windows))]
    {
        let _ = drive;
        Err(Error::new(
            ErrorKind::Unsupported,
            "volume queries require Windows",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    #[test]
    fn walk_reports_largest_fixture_file() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("small")).unwrap();
        fs::create_dir(root.path().join("large")).unwrap();
        fs::write(root.path().join("small/a.txt"), [0u8; 32]).unwrap();
        let mut big = fs::File::create(root.path().join("large/b.bin")).unwrap();
        big.write_all(&vec![0u8; 4096]).unwrap();
        drop(big);
        let (dirs, files, records) = walk_usage(root.path(), 5).unwrap();
        assert!(records >= 2);
        assert_eq!(
            files[0].path,
            root.path().join("large").join("b.bin").to_string_lossy()
        );
        assert!(dirs.iter().any(|entry| entry.path.ends_with("large")));
    }
}
