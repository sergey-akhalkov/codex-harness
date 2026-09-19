use crate::{denied_file_name, drive_letter, drive_root, is_under};
use serde::Serialize;
use std::fs;
use std::io::{self, Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug)]
pub struct CategoryRoot {
    pub id: &'static str,
    pub description: &'static str,
    pub min_age_hours: u64,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct CategoryReport {
    pub id: String,
    pub description: String,
    pub path: String,
    pub candidate_files: u64,
    pub candidate_bytes: u64,
    pub deleted_files: u64,
    pub freed_bytes: u64,
    pub skipped_files: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReclaimReport {
    pub drive: char,
    pub apply: bool,
    pub elapsed_ms: u128,
    pub candidate_files: u64,
    pub candidate_bytes: u64,
    pub deleted_files: u64,
    pub freed_bytes: u64,
    pub skipped_files: u64,
    pub recycle_emptied: bool,
    pub categories: Vec<CategoryReport>,
}

pub struct ReclaimOptions {
    pub drive: char,
    pub apply: bool,
    pub min_age_hours: u64,
    pub empty_recycle: bool,
    pub categories: Vec<CategoryRoot>,
}

pub fn default_categories(drive: char) -> Vec<CategoryRoot> {
    let drive = drive.to_ascii_uppercase();
    let mut categories = Vec::new();
    if let Some(temp) = std::env::temp_dir()
        .canonicalize()
        .ok()
        .filter(|path| on_drive(path, drive))
    {
        categories.push(CategoryRoot {
            id: "user-temp",
            description: "user temporary files older than the age threshold",
            min_age_hours: 48,
            path: temp,
        });
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        push_existing(
            &mut categories,
            drive,
            "thumbnails",
            "Explorer thumbnail cache",
            0,
            local.join(r"Microsoft\Windows\Explorer"),
        );
        push_existing(
            &mut categories,
            drive,
            "wer-queue",
            "Windows Error Reporting queue",
            24,
            local.join(r"Microsoft\Windows\WER\ReportQueue"),
        );
        push_existing(
            &mut categories,
            drive,
            "internet-cache",
            "Temporary Internet Files / INetCache",
            48,
            local.join(r"Microsoft\Windows\INetCache"),
        );
        push_existing(
            &mut categories,
            drive,
            "shader-cache",
            "DirectX shader cache",
            48,
            local.join("D3DSCache"),
        );
        push_existing(
            &mut categories,
            drive,
            "crash-dumps-user",
            "user crash dumps",
            168,
            local.join("CrashDumps"),
        );
        if let Ok(temp) = local.join("Temp").canonicalize()
            && on_drive(&temp, drive)
            && !categories
                .iter()
                .any(|item| is_under(&temp, &item.path) || is_under(&item.path, &temp))
        {
            categories.push(CategoryRoot {
                id: "user-temp-local",
                description: "LOCALAPPDATA temporary files older than the age threshold",
                min_age_hours: 48,
                path: temp,
            });
        }
    }
    let windows = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into()));
    push_existing(
        &mut categories,
        drive,
        "windows-temp",
        "Windows temporary files",
        48,
        windows.join("Temp"),
    );
    push_existing(
        &mut categories,
        drive,
        "minidumps",
        "Windows minidump files",
        168,
        windows.join("Minidump"),
    );
    push_existing(
        &mut categories,
        drive,
        "delivery-optimization",
        "Delivery Optimization cache",
        48,
        drive_root(drive).join(r"Windows\ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache"),
    );
    categories
}

fn push_existing(
    categories: &mut Vec<CategoryRoot>,
    drive: char,
    id: &'static str,
    description: &'static str,
    min_age_hours: u64,
    path: PathBuf,
) {
    let Ok(path) = path.canonicalize() else {
        return;
    };
    if on_drive(&path, drive) {
        categories.push(CategoryRoot {
            id,
            description,
            min_age_hours,
            path,
        });
    }
}

fn on_drive(path: &Path, drive: char) -> bool {
    drive_letter(path) == Some(drive.to_ascii_uppercase())
}

pub fn reclaim(options: ReclaimOptions) -> io::Result<ReclaimReport> {
    let started = Instant::now();
    let drive = options.drive.to_ascii_uppercase();
    if !drive.is_ascii_alphabetic() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "drive must be a letter",
        ));
    }
    let mut categories = Vec::new();
    for root in options.categories {
        if !on_drive(&root.path, drive) {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!("{} is not on drive {drive}", root.path.display()),
            ));
        }
        let min_age = if root.min_age_hours == 0 {
            Duration::from_secs(0)
        } else {
            Duration::from_secs(options.min_age_hours.max(root.min_age_hours) * 3600)
        };
        let mut report = CategoryReport {
            id: root.id.to_string(),
            description: root.description.to_string(),
            path: root.path.to_string_lossy().into_owned(),
            candidate_files: 0,
            candidate_bytes: 0,
            deleted_files: 0,
            freed_bytes: 0,
            skipped_files: 0,
        };
        scan(
            &root.path,
            &root.path,
            min_age,
            options.apply,
            0,
            &mut report,
        );
        categories.push(report);
    }
    let mut recycle_emptied = false;
    if options.empty_recycle && options.apply {
        recycle_emptied = empty_recycle(drive)?;
    }
    let report = ReclaimReport {
        drive,
        apply: options.apply,
        elapsed_ms: started.elapsed().as_millis(),
        candidate_files: categories.iter().map(|item| item.candidate_files).sum(),
        candidate_bytes: categories.iter().map(|item| item.candidate_bytes).sum(),
        deleted_files: categories.iter().map(|item| item.deleted_files).sum(),
        freed_bytes: categories.iter().map(|item| item.freed_bytes).sum(),
        skipped_files: categories.iter().map(|item| item.skipped_files).sum(),
        recycle_emptied,
        categories,
    };
    Ok(report)
}

fn scan(
    root: &Path,
    current: &Path,
    min_age: Duration,
    apply: bool,
    depth: usize,
    report: &mut CategoryReport,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if is_reparse(&meta) {
            report.skipped_files += 1;
            continue;
        }
        if meta.is_dir() {
            scan(root, &path, min_age, apply, depth + 1, report);
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            report.skipped_files += 1;
            continue;
        };
        if denied_file_name(name) {
            report.skipped_files += 1;
            continue;
        }
        let Ok(canon) = fs::canonicalize(&path) else {
            report.skipped_files += 1;
            continue;
        };
        if !is_under(&canon, root) {
            report.skipped_files += 1;
            continue;
        }
        if min_age > Duration::ZERO
            && let Ok(modified) = meta.modified()
            && let Ok(age) = SystemTime::now().duration_since(modified)
            && age < min_age
        {
            continue;
        }
        let size = meta.len();
        report.candidate_files += 1;
        report.candidate_bytes = report.candidate_bytes.saturating_add(size);
        if !apply {
            continue;
        }
        if clear_readonly(&canon).is_err() {
            report.skipped_files += 1;
            continue;
        }
        match fs::remove_file(&canon) {
            Ok(()) => {
                report.deleted_files += 1;
                report.freed_bytes = report.freed_bytes.saturating_add(size);
            }
            Err(_) => report.skipped_files += 1,
        }
    }
}

fn is_reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    let mut permissions = meta.permissions();
    if permissions.readonly() {
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

fn empty_recycle(drive: char) -> io::Result<bool> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize,
        };
        use windows_sys::Win32::UI::Shell::{
            SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHEmptyRecycleBinW,
        };
        let mut wide: Vec<u16> = drive_root(drive).to_string_lossy().encode_utf16().collect();
        wide.push(0);
        unsafe {
            let _ = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
            let status = SHEmptyRecycleBinW(
                std::ptr::null_mut(),
                wide.as_ptr(),
                SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
            );
            CoUninitialize();
            if status < 0 {
                Err(Error::other(format!(
                    "SHEmptyRecycleBin failed with HRESULT {status:#x}"
                )))
            } else {
                Ok(true)
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = drive;
        Err(Error::new(
            ErrorKind::Unsupported,
            "emptying Recycle Bin requires Windows",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::time::{Duration, SystemTime};

    fn old_file(path: &Path, bytes: usize) {
        let mut file = fs::File::create(path).unwrap();
        file.write_all(&vec![7u8; bytes]).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(60 * 60 * 72))
            .unwrap();
    }

    #[test]
    fn dry_run_does_not_delete_and_apply_stays_inside_allowlist() {
        let sandbox = tempfile::tempdir().unwrap();
        let safe = sandbox.path().join("safe-temp");
        let secrets = sandbox.path().join("documents");
        fs::create_dir_all(safe.join("nested")).unwrap();
        fs::create_dir(&secrets).unwrap();
        old_file(&safe.join("nested/stale.tmp"), 1024);
        old_file(&secrets.join("keep.txt"), 2048);
        fs::write(safe.join("recent.tmp"), b"new").unwrap();

        let drive = drive_letter(&safe.canonicalize().unwrap()).expect("fixture drive");
        let categories = vec![CategoryRoot {
            id: "user-temp",
            description: "fixture temp",
            min_age_hours: 48,
            path: safe.canonicalize().unwrap(),
        }];
        let dry = reclaim(ReclaimOptions {
            drive,
            apply: false,
            min_age_hours: 48,
            empty_recycle: false,
            categories: categories.clone(),
        })
        .unwrap();
        assert_eq!(dry.candidate_files, 1);
        assert!(dry.candidate_bytes >= 1024);
        assert_eq!(dry.deleted_files, 0);
        assert!(safe.join("nested/stale.tmp").exists());
        assert!(secrets.join("keep.txt").exists());

        let applied = reclaim(ReclaimOptions {
            drive,
            apply: true,
            min_age_hours: 48,
            empty_recycle: false,
            categories,
        })
        .unwrap();
        assert_eq!(applied.deleted_files, 1);
        assert!(!safe.join("nested/stale.tmp").exists());
        assert!(safe.join("recent.tmp").exists());
        assert!(secrets.join("keep.txt").exists());
    }

    #[test]
    fn refuses_category_off_the_requested_drive() {
        let sandbox = tempfile::tempdir().unwrap();
        let path = sandbox.path().canonicalize().unwrap();
        let drive = drive_letter(&path).unwrap();
        let other = if drive == 'C' { 'D' } else { 'C' };
        let err = reclaim(ReclaimOptions {
            drive: other,
            apply: false,
            min_age_hours: 48,
            empty_recycle: false,
            categories: vec![CategoryRoot {
                id: "user-temp",
                description: "fixture",
                min_age_hours: 48,
                path,
            }],
        })
        .unwrap_err();
        assert!(err.to_string().contains("is not on drive"));
    }
}
