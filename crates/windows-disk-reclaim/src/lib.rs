//! Fast NTFS usage snapshot and allowlist-only reclaim for Windows system drives.
//!
//! Deletion is never ranked by size. The only mutating path removes files under
//! resolved, canonical allowlist roots after age and reparse checks.

mod mft;
mod reclaim;
mod snapshot;

pub use reclaim::{
    CategoryReport, CategoryRoot, ReclaimOptions, ReclaimReport, default_categories, reclaim,
};
pub use snapshot::{Method, SnapshotOptions, SnapshotReport, UsageEntry, VolumeInfo, snapshot};

use std::path::{Path, PathBuf};

pub fn bytes_human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(crate) fn win_text(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let stripped = raw
        .strip_prefix(r"\\?\")
        .unwrap_or(raw.as_ref())
        .replace('/', "\\");
    stripped.trim_end_matches('\\').to_lowercase()
}

pub(crate) fn is_under(path: &Path, root: &Path) -> bool {
    let path_text = win_text(path);
    let root_text = win_text(root);
    path_text == root_text || path_text.starts_with(&format!("{root_text}\\"))
}

pub(crate) fn drive_letter(path: &Path) -> Option<char> {
    let text = win_text(path);
    let mut chars = text.chars();
    let letter = chars.next()?;
    if chars.next() == Some(':') && letter.is_ascii_alphabetic() {
        Some(letter.to_ascii_uppercase())
    } else {
        None
    }
}

pub(crate) fn drive_root(drive: char) -> PathBuf {
    PathBuf::from(format!("{}:\\", drive.to_ascii_uppercase()))
}

pub(crate) fn denied_file_name(name: &str) -> bool {
    const DENY: &[&str] = &[
        "pagefile.sys",
        "hiberfil.sys",
        "swapfile.sys",
        "ntuser.dat",
        "ntuser.dat.log",
        "bootmgr",
        "bootnxt",
        "bcd",
    ];
    let lower = name.to_ascii_lowercase();
    DENY.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn under_check_is_case_insensitive_and_requires_separator() {
        let root = Path::new(r"C:\Temp");
        assert!(is_under(Path::new(r"c:\temp\a.txt"), root));
        assert!(is_under(Path::new(r"\\?\C:\Temp"), root));
        assert!(!is_under(Path::new(r"C:\Temp2\a.txt"), root));
        assert!(!is_under(Path::new(r"C:\Windows"), root));
    }

    #[test]
    fn deny_list_covers_virtual_memory_files() {
        assert!(denied_file_name("Pagefile.sys"));
        assert!(!denied_file_name("setup.tmp"));
    }
}
