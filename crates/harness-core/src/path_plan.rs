//! PATH text planning and read-only command precedence for the native installer.
//! No environment, registry, file or process mutation occurs here.
#![cfg(windows)]

use std::{fs, io, path::Path};
use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn bounded(value: &str) -> io::Result<()> {
    if value.contains('\0') || value.encode_utf16().count() > 32767 {
        return Err(invalid("PATH input is invalid or exceeds its bound"));
    }
    Ok(())
}

fn expand(value: &str) -> io::Result<String> {
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
    bounded(value)?;
    let input: Vec<_> = value.encode_utf16().chain(Some(0)).collect();
    let size = unsafe { ExpandEnvironmentStringsW(input.as_ptr(), std::ptr::null_mut(), 0) };
    if size == 0 {
        return Err(io::Error::last_os_error());
    }
    if size > 32768 {
        return Err(invalid("expanded PATH input exceeds its bound"));
    }
    let mut output = vec![0u16; size as usize];
    let written = unsafe { ExpandEnvironmentStringsW(input.as_ptr(), output.as_mut_ptr(), size) };
    if written == 0 {
        return Err(io::Error::last_os_error());
    }
    if written > size {
        return Err(invalid("PATH environment changed during expansion"));
    }
    output.truncate(written as usize - 1);
    String::from_utf16(&output).map_err(|_| invalid("expanded PATH input is not valid Unicode"))
}

fn name(value: &str) -> io::Result<String> {
    let expanded = expand(value.trim_matches('"'))?;
    if expanded.contains(';') || !Path::new(&expanded).is_absolute() {
        return Err(invalid(
            "PATH entry cannot be resolved as one absolute directory",
        ));
    }
    let full = std::path::absolute(&expanded)?;
    Ok(full
        .to_str()
        .ok_or_else(|| invalid("PATH entry is not valid Unicode"))?
        .trim_end_matches('\\')
        .to_owned())
}

fn same(left: &str, right: &str) -> io::Result<bool> {
    let left: Vec<_> = left.encode_utf16().collect();
    let right: Vec<_> = right.encode_utf16().collect();
    match unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        )
    } {
        0 => Err(io::Error::last_os_error()),
        result => Ok(result == CSTR_EQUAL),
    }
}

fn bin_name(bin: &Path) -> io::Result<String> {
    let text = bin
        .to_str()
        .ok_or_else(|| invalid("command directory is not valid Unicode"))?;
    if text.contains('%') || text.contains('"') || text.contains(';') {
        return Err(invalid(
            "command directory must be an explicit absolute path",
        ));
    }
    name(text)
}

/// Preserve the original string exactly when its current expansion already
/// contains the bin. `bool` records whether the installer added an entry.
pub fn prepend(value: Option<&str>, bin: &Path) -> io::Result<(String, bool)> {
    let value = value.unwrap_or("");
    bounded(value)?;
    let expected = bin_name(bin)?;
    for entry in value.split(';').filter(|s| !s.is_empty()) {
        if let Ok(actual) = name(entry)
            && same(&actual, &expected)?
        {
            return Ok((value.to_owned(), false));
        }
    }
    let result = format!("{};{value}", bin.display());
    bounded(&result)?;
    Ok((result, true))
}

/// Remove matching entries while retaining the text/order of all others,
/// including empty entries. The caller must establish that the entry was added.
pub fn remove(value: &str, bin: &Path) -> io::Result<String> {
    bounded(value)?;
    let expected = bin_name(bin)?;
    let mut kept = Vec::new();
    for entry in value.split(';') {
        let matches = if entry.is_empty() {
            false
        } else {
            match name(entry) {
                Ok(actual) => same(&actual, &expected)?,
                Err(_) => false,
            }
        };
        if !matches {
            kept.push(entry);
        }
    }
    Ok(kept.join(";"))
}

/// Inspect the explicit effective PATH in its search order. This does not
/// certify aliases, shell functions or a shell's current-directory search.
pub fn check_precedence(effective: &str, bin: &Path) -> io::Result<()> {
    let expected = bin_name(bin)?;
    let effective = expand(effective)?;
    for entry in effective.split(';').filter(|s| !s.is_empty()) {
        let directory = name(entry)?;
        if same(&directory, &expected)? {
            return Ok(());
        }
        // Avoid unbounded network filesystem access during a local preflight.
        if directory.starts_with("\\\\") {
            return Err(invalid("remote PATH precedence requires an explicit check"));
        }
        for extension in ["ps1", "exe", "cmd", "bat", "com"] {
            match fs::metadata(Path::new(&directory).join(format!("codex.{extension}"))) {
                Ok(meta) if meta.is_file() => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "another codex command precedes the managed directory",
                    ));
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
    }
    Err(invalid(
        "command directory is absent from the effective PATH",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_planning_preserves_unrelated_and_empty_entries_exactly() {
        let bin = Path::new("C:\\Owned Юникод\\bin");
        let before = ";C:\\foreign;;";
        let (after, added) = prepend(Some(before), bin).unwrap();
        assert!(added);
        assert_eq!(remove(&after, bin).unwrap(), before);
        let existing = ";\"c:/owned Юникод/bin/\";;C:\\foreign;";
        assert_eq!(
            prepend(Some(existing), bin).unwrap(),
            (existing.into(), false)
        );
        assert_eq!(remove(existing, bin).unwrap(), ";;C:\\foreign;");
        assert!(prepend(None, bin).unwrap().1);
        assert_eq!(
            prepend(
                Some("%SystemRoot%;C:\\foreign"),
                Path::new(&std::env::var("SystemRoot").unwrap())
            )
            .unwrap(),
            ("%SystemRoot%;C:\\foreign".into(), false)
        );
        assert!(prepend(Some("bad\0path"), bin).is_err());
        assert!(prepend(Some(&"x".repeat(32767)), bin).is_err());
        assert!(prepend(None, Path::new("relative")).is_err());
    }

    #[test]
    fn actual_inert_commands_block_only_when_before_the_managed_bin() {
        let root = tempfile::tempdir().unwrap();
        let other = root.path().join("other");
        let bin = root.path().join("future bin Юникод");
        fs::create_dir(&other).unwrap();
        for extension in ["ps1", "exe", "cmd", "bat", "com"] {
            let command = other.join(format!("codex.{extension}"));
            fs::write(&command, b"inert test data, never executable").unwrap();
            assert_eq!(
                check_precedence(&format!("{};{}", other.display(), bin.display()), &bin)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::AlreadyExists
            );
            check_precedence(&format!("{};{}", bin.display(), other.display()), &bin).unwrap();
            assert_eq!(
                fs::read(&command).unwrap(),
                b"inert test data, never executable"
            );
            fs::remove_file(command).unwrap();
        }
        check_precedence(
            &format!("{};;\"{}\";", other.display(), bin.display()),
            &bin,
        )
        .unwrap();
        // Read-only precedence follows external directory links, unlike the
        // stricter mutation-parent checks used by installation publication.
        let linked = root.path().join("linked external tools");
        std::os::windows::fs::symlink_dir(&other, &linked).unwrap();
        fs::write(other.join("CoDeX.EXE"), b"inert data").unwrap();
        assert!(
            check_precedence(&format!("{};{}", linked.display(), bin.display()), &bin).is_err()
        );
        fs::remove_file(other.join("CoDeX.EXE")).unwrap();
        fs::create_dir(other.join("codex.exe")).unwrap();
        check_precedence(&format!("{};{}", linked.display(), bin.display()), &bin).unwrap();
        assert!(check_precedence(&other.to_string_lossy(), &bin).is_err());
        assert!(check_precedence(&format!("relative;{}", bin.display()), &bin).is_err());
        assert!(!bin.exists());
    }
}
