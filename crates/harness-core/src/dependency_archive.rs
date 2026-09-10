//! Bounded archive readers for explicit dependency staging/auditing.
//! Readers never extract paths or execute package installation scripts.
#![cfg(windows)]
use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::bufread::GzDecoder;
use sha2::{Digest, Sha256, Sha384, Sha512};
use std::{
    collections::BTreeSet,
    io::{self, Cursor, Read},
};

const COMPRESSED_LIMIT: usize = 128 * 1024 * 1024;
const EXPANDED_LIMIT: u64 = 512 * 1024 * 1024;
const FILE_LIMIT: u64 = 128 * 1024 * 1024;
// Codebase Memory 0.10.8 contains a 296,140,288-byte native executable.
const ZIP_FILE_LIMIT: u64 = EXPANDED_LIMIT;
const ENTRY_LIMIT: usize = 16_384;
const NAME_LIMIT: usize = 4096;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "dependency archive is invalid or exceeds its limits",
    )
}

/// Integrity metadata is supplied by an independently identified official
/// release. This check alone does not establish that metadata's provenance.
pub fn verify_sri(bytes: &[u8], integrity: &str) -> io::Result<()> {
    if bytes.len() > COMPRESSED_LIMIT || integrity.len() > 4096 {
        return Err(invalid());
    }
    let mut expected = Vec::new();
    let mut strength = 0;
    for token in integrity.split_ascii_whitespace() {
        let (algorithm, encoded) = token.split_once('-').ok_or_else(invalid)?;
        let bits = match algorithm {
            "sha256" => 256,
            "sha384" => 384,
            "sha512" => 512,
            _ => return Err(invalid()),
        };
        let decoded = STANDARD.decode(encoded).map_err(|_| invalid())?;
        if decoded.len() != bits / 8 {
            return Err(invalid());
        }
        if bits > strength {
            expected.clear();
            strength = bits;
        }
        if bits == strength {
            expected.push(decoded);
        }
    }
    let actual = match strength {
        256 => Sha256::digest(bytes).to_vec(),
        384 => Sha384::digest(bytes).to_vec(),
        512 => Sha512::digest(bytes).to_vec(),
        _ => return Err(invalid()),
    };
    if expected.contains(&actual) {
        Ok(())
    } else {
        Err(invalid())
    }
}

/// Reject Windows path aliases that can escape an intended regular-file tree.
/// Returned names use `/`; no caller should bypass its own publication guards.
fn name(raw: &[u8], directory: bool, npm: bool) -> io::Result<String> {
    let raw = std::str::from_utf8(raw).map_err(|_| invalid())?;
    if raw.is_empty()
        || raw.len() > NAME_LIMIT
        || raw.contains('\\')
        || raw.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    let raw = if directory {
        raw.strip_suffix('/').unwrap_or(raw)
    } else {
        raw
    };
    for component in raw.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with('.')
            // Conservative interoperability admission, not an NTFS alias claim.
            || component.ends_with(char::is_whitespace)
            || component.contains([':', '*', '?', '"', '<', '>', '|'])
        {
            return Err(invalid());
        }
        let base = component.split('.').next().unwrap().to_ascii_uppercase();
        if matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            base.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        }) {
            return Err(invalid());
        }
    }
    if npm {
        if directory && raw == "package" {
            return Ok(String::new());
        }
        return raw
            .strip_prefix("package/")
            .map(str::to_owned)
            .ok_or_else(invalid);
    }
    Ok(raw.to_owned())
}

#[derive(Debug, PartialEq, Eq)]
pub struct Summary {
    pub files: usize,
    pub bytes: u64,
}

struct Budget {
    entries: usize,
    file_limit: u64,
    summary: Summary,
    names: BTreeSet<String>,
}
impl Budget {
    fn new(file_limit: u64) -> Self {
        Self {
            entries: 0,
            file_limit,
            summary: Summary { files: 0, bytes: 0 },
            names: BTreeSet::new(),
        }
    }
    fn entry(&mut self, path: &str, size: u64, directory: bool) -> io::Result<()> {
        self.entries += 1;
        if self.entries > ENTRY_LIMIT || size > self.file_limit || (directory && size != 0) {
            return Err(invalid());
        }
        // Conservatively reject Unicode case lookalikes too; this is an
        // archive admission rule, not a claim about NTFS object identity.
        // Publication still needs exclusive creation and path/handle guards.
        if !path.is_empty() && !self.names.insert(path.to_uppercase().to_lowercase()) {
            return Err(invalid());
        }
        if !directory {
            self.summary.files += 1;
            self.summary.bytes = self.summary.bytes.checked_add(size).ok_or_else(invalid)?;
            if self.summary.bytes > EXPANDED_LIMIT {
                return Err(invalid());
            }
        }
        Ok(())
    }
    fn finish(self) -> io::Result<Summary> {
        if self.summary.files == 0 {
            Err(invalid())
        } else {
            Ok(self.summary)
        }
    }
}

struct Bounded<R> {
    reader: R,
    remaining: u64,
    failed: bool,
}
impl<R: Read> Read for Bounded<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.failed {
            return Err(invalid());
        }
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return match self.reader.read(&mut [0; 1]) {
                Ok(0) => Ok(0),
                _ => {
                    self.failed = true;
                    Err(invalid())
                }
            };
        }
        let length = buffer.len().min(self.remaining as usize);
        let count = match self.reader.read(&mut buffer[..length]) {
            Ok(count) => count,
            Err(error) => {
                self.failed = true;
                return Err(error);
            }
        };
        self.remaining -= count as u64;
        Ok(count)
    }
}
fn tar_reader(bytes: &[u8]) -> tar::Archive<Bounded<GzDecoder<&[u8]>>> {
    tar::Archive::new(Bounded {
        reader: GzDecoder::new(bytes),
        remaining: EXPANDED_LIMIT,
        failed: false,
    })
}

fn finish_tar(mut reader: Bounded<GzDecoder<&[u8]>>) -> io::Result<()> {
    // tar stops at its first zero block. Only zero padding may follow it.
    // Draining also validates the gzip checksum and expanded-byte bound.
    let mut buffer = [0; 16 * 1024];
    loop {
        let size = reader.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        if buffer[..size].iter().any(|byte| *byte != 0) {
            return Err(invalid());
        }
    }
    // bufread::GzDecoder leaves the next member/trailing input unconsumed.
    if !reader.reader.get_ref().is_empty() {
        return Err(invalid());
    }
    Ok(())
}

/// Callbacks receive validated regular-file names and must use an owned staging
/// scope. The whole archive remains tentative until this method returns Ok;
/// a later payload/CRC failure can invalidate prior callbacks' partial work.
pub fn visit_npm_tar(
    bytes: &[u8],
    visit: impl FnMut(&str, u64, &mut dyn Read) -> io::Result<()>,
) -> io::Result<Summary> {
    visit_npm_tar_with_directories(bytes, |_| Ok(()), visit)
}

/// Staging also preserves explicitly listed empty directories. Both callbacks
/// have the same tentative lifetime and validated names as the file-only reader.
pub(crate) fn visit_npm_tar_with_directories(
    bytes: &[u8],
    mut directory_visit: impl FnMut(&str) -> io::Result<()>,
    mut visit: impl FnMut(&str, u64, &mut dyn Read) -> io::Result<()>,
) -> io::Result<Summary> {
    if bytes.len() > COMPRESSED_LIMIT {
        return Err(invalid());
    }
    // The raw preflight bounds GNU/PAX extension payloads before tar's logical
    // iterator collects those payloads in memory. No callbacks run in this pass.
    let mut preflight = tar_reader(bytes);
    let mut count = 0;
    for entry in preflight.entries()?.raw(true) {
        let entry = entry?;
        count += 1;
        if count > ENTRY_LIMIT * 3 {
            return Err(invalid());
        }
        let kind = entry.header().entry_type();
        let limit = if kind.is_gnu_longname() {
            NAME_LIMIT as u64
        } else if kind.is_pax_local_extensions() {
            64 * 1024
        } else if kind.is_file() || kind.is_dir() {
            FILE_LIMIT
        } else {
            return Err(invalid());
        };
        if entry.size() > limit {
            return Err(invalid());
        }
    }
    finish_tar(preflight.into_inner())?;
    let mut archive = tar_reader(bytes);
    let mut budget = Budget::new(FILE_LIMIT);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let directory = entry.header().entry_type().is_dir();
        if !directory && !entry.header().entry_type().is_file() {
            return Err(invalid());
        }
        let path = name(&entry.path_bytes(), directory, true)?;
        let size = entry.size();
        budget.entry(&path, size, directory)?;
        if directory {
            directory_visit(&path)?;
        } else {
            let mut bounded = Bounded {
                reader: &mut entry,
                remaining: size,
                failed: false,
            };
            visit(&path, size, &mut bounded)?;
            // Force full payload consumption even when a caller skips a file.
            io::copy(&mut bounded, &mut io::sink())?;
            if bounded.remaining != 0 {
                return Err(invalid());
            }
        }
    }
    finish_tar(archive.into_inner())?;
    budget.finish()
}

fn zip_preflight(bytes: &[u8]) -> io::Result<usize> {
    if bytes.len() > COMPRESSED_LIMIT || bytes.len() < 22 || !bytes.starts_with(b"PK\x03\x04") {
        return Err(invalid());
    }
    let start = bytes.len().saturating_sub(65_557);
    let end = (start..=bytes.len() - 22)
        .rev()
        .find(|offset| {
            bytes[*offset..*offset + 4] == *b"PK\x05\x06"
                && *offset
                    + 22
                    + u16::from_le_bytes([bytes[*offset + 20], bytes[*offset + 21]]) as usize
                    == bytes.len()
        })
        .ok_or_else(invalid)?;
    let u16_at = |offset| u16::from_le_bytes([bytes[end + offset], bytes[end + offset + 1]]);
    let u32_at =
        |offset| u32::from_le_bytes(bytes[end + offset..end + offset + 4].try_into().unwrap());
    let count = u16_at(10) as usize;
    let directory_size = u32_at(12) as usize;
    let directory_start = u32_at(16) as usize;
    // Current bounded native archives need neither ZIP64 nor multi-disk/SFX.
    // Check before ZipArchive allocates an index from untrusted entry counts.
    if u16_at(4) != 0
        || u16_at(6) != 0
        || u16_at(8) as usize != count
        || count == 0
        || count > ENTRY_LIMIT
        || directory_size > 16 * 1024 * 1024
        || directory_start.checked_add(directory_size) != Some(end)
        || directory_size < count * 46
        || (end >= 20 && bytes[end - 20..end - 16] == *b"PK\x06\x07")
    {
        return Err(invalid());
    }
    // Count exact raw records: ZipArchive deduplicates names while building its
    // index and may otherwise accept a count that hides additional records.
    let mut position = directory_start;
    for _ in 0..count {
        if position + 46 > end || bytes[position..position + 4] != *b"PK\x01\x02" {
            return Err(invalid());
        }
        let length = |offset| {
            u16::from_le_bytes([bytes[position + offset], bytes[position + offset + 1]]) as usize
        };
        position += 46 + length(28) + length(30) + length(32);
        if position > end {
            return Err(invalid());
        }
    }
    if position != end {
        return Err(invalid());
    }
    Ok(count)
}

pub fn visit_zip(
    bytes: &[u8],
    mut visit: impl FnMut(&str, u64, &mut dyn Read) -> io::Result<()>,
) -> io::Result<Summary> {
    let count = zip_preflight(bytes)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid())?;
    if archive.len() != count || archive.has_overlapping_files().map_err(|_| invalid())? {
        return Err(invalid());
    }
    let mut budget = Budget::new(ZIP_FILE_LIMIT);
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| invalid())?;
        let directory = file.is_dir();
        if file.encrypted()
            || file.unix_mode().is_some_and(|mode| {
                let kind = mode & 0o170000;
                kind != 0 && kind != if directory { 0o040000 } else { 0o100000 }
            })
        {
            return Err(invalid());
        }
        let path = name(file.name_raw(), directory, false)?;
        let size = file.size();
        budget.entry(&path, size, directory)?;
        if !directory {
            let mut bounded = Bounded {
                reader: &mut file,
                remaining: size,
                failed: false,
            };
            visit(&path, size, &mut bounded)?;
            io::copy(&mut bounded, &mut io::sink())?;
            if bounded.remaining != 0 {
                return Err(invalid());
            }
        }
    }
    budget.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn gzip(raw: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(raw).unwrap();
        encoder.finish().unwrap()
    }
    fn tar(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, kind, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(body.len() as u64);
            header.set_entry_type(*kind);
            // Raw header assembly makes even traversal names real parser input.
            assert!(path.len() <= 100);
            header.as_mut_bytes()[..path.len()].copy_from_slice(path.as_bytes());
            header.set_cksum();
            builder.append(&header, *body).unwrap();
        }
        gzip(&builder.into_inner().unwrap())
    }
    fn zip(entries: &[(&str, &[u8])], method: zip::CompressionMethod) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, bytes) in entries {
            writer
                .start_file(
                    *path,
                    SimpleFileOptions::default().compression_method(method),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    fn end_offset(bytes: &[u8]) -> usize {
        bytes
            .windows(4)
            .rposition(|part| part == b"PK\x05\x06")
            .unwrap()
    }

    #[test]
    fn strongest_sri_digest_controls_acceptance_and_malformed_input_is_private() {
        let bytes = b"known owned archive bytes";
        let strong = format!("sha512-{}", STANDARD.encode(Sha512::digest(bytes)));
        let weak = format!("sha256-{}", STANDARD.encode(Sha256::digest(bytes)));
        verify_sri(bytes, &strong).unwrap();
        verify_sri(bytes, &format!("{weak} {strong}")).unwrap();
        let bad_strong = format!("sha512-{}", STANDARD.encode([0; 64]));
        assert!(verify_sri(bytes, &format!("{weak} {bad_strong}")).is_err());
        assert!(verify_sri(bytes, &format!("{bad_strong} {strong}")).is_ok());
        for bad in [
            "",
            "sha1-AAAA",
            "sha512-PRIVATE_DIGEST",
            "sha512-Zg==",
            "PRIVATE_DIGEST",
        ] {
            let error = verify_sri(bytes, bad).unwrap_err().to_string();
            assert!(!error.contains("PRIVATE_DIGEST"));
        }
        assert!(verify_sri(b"changed bytes", &strong).is_err());
    }

    #[test]
    fn paths_reject_escape_ads_devices_and_potential_case_collisions() {
        for bad in [
            "../x",
            "/absolute",
            "C:/absolute",
            "a/../x",
            "a//x",
            "a/./x",
            "a\\x",
            "name:stream",
            "a/NUL.txt",
            "COM1",
            "COM¹.txt",
            "CONIN$",
            "CONOUT$",
            "a/file.",
            "a/file ",
            "a/",
            "a\0b",
        ] {
            assert!(name(bad.as_bytes(), false, false).is_err(), "{bad:?}");
        }
        assert_eq!(
            name(b"package/a/file.py", false, true).unwrap(),
            "a/file.py"
        );
        assert!(name(b"not-package/a", false, true).is_err());
        let mut budget = Budget::new(FILE_LIMIT);
        budget.entry("S.py", 1, false).unwrap();
        assert!(budget.entry("ſ.py", 1, false).is_err());
        let mut budget = Budget::new(FILE_LIMIT);
        budget.entry("Dir/File.py", 1, false).unwrap();
        assert!(budget.entry("dir/file.py", 1, false).is_err());
    }

    #[test]
    fn npm_tar_visits_files_without_extracting_and_supports_bounded_long_names() {
        let archive = tar(&[
            ("package/", tar::EntryType::Directory, b""),
            ("package/package.json", tar::EntryType::Regular, b"{}"),
            (
                "package/bin/tool",
                tar::EntryType::Regular,
                b"native fixture bytes",
            ),
        ]);
        let mut files = Vec::new();
        let summary = visit_npm_tar(&archive, |path, size, reader| {
            let mut data = Vec::new();
            reader.read_to_end(&mut data)?;
            assert_eq!(data.len() as u64, size);
            files.push((path.to_owned(), data));
            Ok(())
        })
        .unwrap();
        assert_eq!(summary.files, 2);
        assert_eq!(files[0], ("package.json".to_owned(), b"{}".to_vec()));
        let mut builder = tar::Builder::new(Vec::new());
        let long = format!("package/{}file.py", "folder/".repeat(20));
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, &long, b"x".as_slice())
            .unwrap();
        let archive = gzip(&builder.into_inner().unwrap());
        visit_npm_tar(&archive, |path, _, _| {
            assert_eq!(path, long.strip_prefix("package/").unwrap());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn tar_preflight_refuses_links_extensions_and_declared_oversize_before_callbacks() {
        for kind in [
            tar::EntryType::Symlink,
            tar::EntryType::Link,
            tar::EntryType::GNUSparse,
        ] {
            let archive = tar(&[("package/entry", kind, b"")]);
            assert!(visit_npm_tar(&archive, |_, _, _| panic!("link admitted")).is_err());
        }
        let oversized = vec![b'a'; NAME_LIMIT + 1];
        let archive = tar(&[("././@LongLink", tar::EntryType::GNULongName, &oversized)]);
        assert!(visit_npm_tar(&archive, |_, _, _| panic!("large extension admitted")).is_err());
        let mut header = tar::Header::new_gnu();
        header.set_path("package/large").unwrap();
        header.set_size(FILE_LIMIT + 1);
        header.set_mode(0o644);
        header.set_cksum();
        let archive = gzip(header.as_bytes());
        assert!(visit_npm_tar(&archive, |_, _, _| panic!("large file admitted")).is_err());
    }

    #[test]
    fn tar_rejects_traversal_duplicates_and_corrupted_gzip_trailer() {
        for path in [
            "../outside",
            "package/../outside",
            "package/NUL",
            "package/file:stream",
        ] {
            assert!(
                visit_npm_tar(
                    &tar(&[(path, tar::EntryType::Regular, b"x")]),
                    |_, _, _| panic!("bad path admitted")
                )
                .is_err()
            );
        }
        let duplicate = tar(&[
            ("package/A.py", tar::EntryType::Regular, b"a"),
            ("package/a.py", tar::EntryType::Regular, b"b"),
        ]);
        assert!(visit_npm_tar(&duplicate, |_, _, _| Ok(())).is_err());
        let mut corrupt = tar(&[("package/a", tar::EntryType::Regular, b"a")]);
        let trailer = corrupt.len() - 8;
        corrupt[trailer] ^= 1;
        assert!(visit_npm_tar(&corrupt, |_, _, _| Ok(())).is_err());
    }

    #[test]
    fn zip_reads_stored_and_deflated_files_and_drains_skipped_payloads() {
        for method in [
            zip::CompressionMethod::Stored,
            zip::CompressionMethod::Deflated,
        ] {
            let archive = zip(
                &[
                    ("dir/tool.exe", b"native fixture bytes"),
                    ("LICENSE", b"fixture license"),
                ],
                method,
            );
            let summary = visit_zip(&archive, |path, size, reader| {
                if path == "LICENSE" {
                    let mut data = Vec::new();
                    reader.read_to_end(&mut data)?;
                    assert_eq!(data.len() as u64, size);
                }
                Ok(())
            })
            .unwrap();
            assert_eq!(
                summary,
                Summary {
                    files: 2,
                    bytes: 35
                }
            );
        }
    }

    #[test]
    fn tar_rejects_nonzero_content_after_its_first_end_block() {
        let first = tar(&[("package/a", tar::EntryType::Regular, b"a")]);
        let second = tar(&[("package/b", tar::EntryType::Regular, b"b")]);
        let mut first_raw = Vec::new();
        GzDecoder::new(first.as_slice())
            .read_to_end(&mut first_raw)
            .unwrap();
        let mut second_raw = Vec::new();
        GzDecoder::new(second.as_slice())
            .read_to_end(&mut second_raw)
            .unwrap();
        // A regular header and one payload block, then an early tar terminator.
        first_raw.truncate(1024 + 512);
        first_raw.extend(second_raw);
        assert!(visit_npm_tar(&gzip(&first_raw), |_, _, _| Ok(())).is_err());
    }

    #[test]
    fn tar_rejects_a_second_gzip_member() {
        let mut archive = tar(&[("package/a", tar::EntryType::Regular, b"a")]);
        archive.extend(gzip(&[0; 1024]));
        assert!(visit_npm_tar(&archive, |_, _, _| Ok(())).is_err());
    }

    #[test]
    fn tar_rejects_trailing_compressed_input() {
        let mut archive = tar(&[("package/a", tar::EntryType::Regular, b"a")]);
        archive.extend_from_slice(b"unvalidated input");
        assert!(visit_npm_tar(&archive, |_, _, _| Ok(())).is_err());
    }

    #[test]
    fn zip_rejects_duplicate_raw_central_directory_names() {
        let mut archive = zip(
            &[("a", b"first"), ("b", b"second")],
            zip::CompressionMethod::Stored,
        );
        let local = archive
            .windows(4)
            .rposition(|bytes| bytes == b"PK\x03\x04")
            .unwrap();
        let central = archive
            .windows(4)
            .rposition(|bytes| bytes == b"PK\x01\x02")
            .unwrap();
        assert_eq!(archive[local + 30], b'b');
        assert_eq!(archive[central + 46], b'b');
        archive[local + 30] = b'a';
        archive[central + 46] = b'a';
        assert_eq!(
            zip::ZipArchive::new(Cursor::new(&archive)).unwrap().len(),
            1
        );
        assert!(visit_zip(&archive, |_, _, _| Ok(())).is_err());
    }

    #[test]
    fn zip_rejects_a_count_hiding_central_directory_records() {
        let mut archive = zip(
            &[("a", b"first"), ("b", b"second")],
            zip::CompressionMethod::Stored,
        );
        let end = end_offset(&archive);
        archive[end + 8..end + 12].copy_from_slice(&[1, 0, 1, 0]);
        assert!(visit_zip(&archive, |_, _, _| Ok(())).is_err());
    }

    #[test]
    fn paths_conservatively_reject_trailing_unicode_whitespace() {
        for path in ["a/file\u{a0}", "COM1\u{2000}", "a\u{3000}/file"] {
            assert!(name(path.as_bytes(), false, false).is_err(), "{path:?}");
        }
        // This admission rule does not assert Unicode normalization on NTFS.
        assert_eq!(name("a/é.txt".as_bytes(), false, false).unwrap(), "a/é.txt");
    }

    #[test]
    fn zip_preflight_bounds_index_allocation_and_rejects_multidisk_or_trailing_data() {
        let original = zip(&[("a", b"x")], zip::CompressionMethod::Stored);
        let end = end_offset(&original);
        let mut excessive = original.clone();
        excessive[end + 8..end + 12].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
        assert!(visit_zip(&excessive, |_, _, _| panic!("index admitted")).is_err());
        let mut multidisk = original.clone();
        multidisk[end + 4] = 1;
        assert!(visit_zip(&multidisk, |_, _, _| panic!("multidisk admitted")).is_err());
        let mut trailing = original;
        trailing.extend_from_slice(b"unowned trailer");
        assert!(visit_zip(&trailing, |_, _, _| panic!("trailer admitted")).is_err());
    }

    #[test]
    fn zip_rejects_escape_links_aliases_and_crc_failure() {
        let mut traversal = zip(&[("aaaa", b"x")], zip::CompressionMethod::Stored);
        for offset in 0..traversal.len() - 3 {
            if traversal[offset..offset + 4] == *b"aaaa" {
                traversal[offset..offset + 4].copy_from_slice(b"../x");
            }
        }
        assert!(visit_zip(&traversal, |_, _, _| panic!("traversal admitted")).is_err());
        let duplicate = zip(
            &[("S.py", b"a"), ("ſ.py", b"b")],
            zip::CompressionMethod::Stored,
        );
        assert!(visit_zip(&duplicate, |_, _, _| Ok(())).is_err());
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_symlink("link", "outside", SimpleFileOptions::default())
            .unwrap();
        let link = writer.finish().unwrap().into_inner();
        assert!(visit_zip(&link, |_, _, _| panic!("symlink admitted")).is_err());
        let mut corrupt = zip(&[("a", b"known payload")], zip::CompressionMethod::Stored);
        let offset = corrupt
            .windows(13)
            .position(|part| part == b"known payload")
            .unwrap();
        corrupt[offset] ^= 1;
        assert!(visit_zip(&corrupt, |_, _, _| Ok(())).is_err());
        assert!(
            visit_zip(&corrupt, |_, _, reader| {
                let _ = reader.read_to_end(&mut Vec::new());
                Ok(())
            })
            .is_err(),
            "a callback cannot erase a parser/CRC failure"
        );
    }

    #[test]
    fn expanded_stream_bound_is_an_error_instead_of_a_successful_truncated_read() {
        let mut bounded = Bounded {
            reader: Cursor::new(b"12345"),
            remaining: 4,
            failed: false,
        };
        assert!(bounded.read_to_end(&mut Vec::new()).is_err());
        assert!(
            bounded.read(&mut [0; 1]).is_err(),
            "failed limits remain failed"
        );
        let mut budget = Budget::new(FILE_LIMIT);
        assert!(budget.entry("huge", FILE_LIMIT + 1, false).is_err());
        let mut budget = Budget::new(ZIP_FILE_LIMIT);
        budget.entry("native.exe", 296_140_288, false).unwrap();
        assert!(budget.entry("overflow", EXPANDED_LIMIT, false).is_err());
        let mut budget = Budget::new(ZIP_FILE_LIMIT);
        assert!(budget.entry("huge", ZIP_FILE_LIMIT + 1, false).is_err());
    }
}
