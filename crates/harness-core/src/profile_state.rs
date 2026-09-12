//! Preserve legacy machine settings before retiring the shared native profile.
#![cfg(windows)]
use crate::config_file::ConfigSnapshot;
use std::{
    fs,
    io::{self, Write},
    path::Path,
};

/// Explicit local preparation survives later connection rollback/disconnection.
/// Local values win conflicts; both original documents stay in private recovery.
pub fn migrate(shared: &Path, home: &Path) -> io::Result<usize> {
    let legacy = home.join("harness.config.toml");
    match fs::symlink_metadata(&legacy) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
        Ok(metadata) if !metadata.file_type().is_symlink() => return Ok(0),
        Ok(_) => {}
    }
    let stored = fs::read_link(&legacy)?;
    if !same_recorded_target(&stored, shared)? {
        // A relocated checkout leaves a dangling recorded target. Compare the
        // stored name without following it, then read the new shared file.
        match fs::symlink_metadata(&stored) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(_) => {
                return Err(io::Error::other(
                    "Legacy profile belongs to another source; preserve it and resolve ownership.",
                ));
            }
        }
    }
    let shared_bytes = fs::read(shared)?;
    let mut incoming = parse(&shared_bytes)?;
    for key in [
        "approval_policy",
        "sandbox_mode",
        "approvals_reviewer",
        "developer_instructions",
        "agents",
        "features",
    ] {
        incoming.remove(key);
    }
    let path = home.join("config.toml");
    let snapshot = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
        Ok(_) => Some(ConfigSnapshot::read(&path)?),
    };
    let before = snapshot.as_ref().map_or(&[][..], ConfigSnapshot::contents);
    let mut local = parse(before)?;
    let conflicts = merge(&mut local, incoming);
    let after = toml::to_string(&local)
        .map_err(|_| io::Error::other("Cannot encode native configuration."))?;
    let recovery = home.join("harness/private-profile-migration");
    crate::inventory::ordinary_parents(&recovery.join("probe"))?;
    fs::create_dir_all(&recovery)?;
    backup(&recovery, "shared", &shared_bytes)?;
    backup(&recovery, "base", before)?;
    if let Some(snapshot) = snapshot {
        if parse(snapshot.contents())? != local {
            snapshot.replace(after.as_bytes())?;
        }
    } else {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)?;
        file.write_all(after.as_bytes())?;
        file.sync_all()?;
    }
    Ok(conflicts)
}

fn backup(root: &Path, kind: &str, bytes: &[u8]) -> io::Result<()> {
    let path = root.join(format!(
        "{kind}-{}.toml",
        crate::build_identity::hash_bytes(bytes)
    ));
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(bytes)?;
            file.sync_all()
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && fs::read(path)? == bytes => {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn merge(local: &mut toml::Table, incoming: toml::Table) -> usize {
    let mut conflicts = 0;
    for (key, value) in incoming {
        match (local.get_mut(&key), value) {
            (Some(toml::Value::Table(current)), toml::Value::Table(next)) => {
                conflicts += merge(current, next)
            }
            (Some(current), next) if *current != next => conflicts += 1,
            (None, next) => {
                local.insert(key, next);
            }
            _ => {}
        }
    }
    conflicts
}

fn parse(bytes: &[u8]) -> io::Result<toml::Table> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.trim_start_matches('\u{feff}').parse().ok())
        .ok_or_else(|| {
            io::Error::other("Invalid native configuration TOML; existing data preserved.")
        })
}

fn same_recorded_target(left: &Path, right: &Path) -> io::Result<bool> {
    fn name(path: &Path) -> io::Result<String> {
        let text = path
            .to_str()
            .ok_or_else(|| io::Error::other("profile path is not UTF-8"))?;
        let text = text.strip_prefix(r"\\?\").unwrap_or(text);
        Ok(std::path::absolute(Path::new(text))?
            .to_string_lossy()
            .to_ascii_lowercase())
    }
    Ok(name(left)? == name(right)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn relocated_dangling_profile_uses_new_shared_without_opening_old_target() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        fs::create_dir_all(home.join("harness")).unwrap();
        let old = root.path().join(r"old\global\profile.toml");
        let shared = root.path().join(r"new\global\profile.toml");
        fs::create_dir_all(shared.parent().unwrap()).unwrap();
        fs::write(&shared, "model = 'gpt-6-astra'\nextra = 1\n").unwrap();
        std::os::windows::fs::symlink_file(&old, home.join("harness.config.toml")).unwrap();
        assert!(!old.exists());
        assert_eq!(migrate(&shared, &home).unwrap(), 0);
        assert!(
            fs::read_to_string(home.join("config.toml"))
                .unwrap()
                .contains("extra = 1")
        );
        assert_eq!(
            fs::read_link(home.join("harness.config.toml")).unwrap(),
            old
        );
        assert!(!old.exists());
    }

    #[test]
    fn live_foreign_profile_target_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        fs::create_dir_all(home.join("harness")).unwrap();
        let foreign = root.path().join("foreign-profile.toml");
        let shared = root.path().join(r"new\global\profile.toml");
        fs::create_dir_all(shared.parent().unwrap()).unwrap();
        fs::write(&foreign, "secret = 1\n").unwrap();
        fs::write(&shared, "model = 'gpt-6-astra'\n").unwrap();
        std::os::windows::fs::symlink_file(&foreign, home.join("harness.config.toml")).unwrap();
        assert!(migrate(&shared, &home).is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"secret = 1\n");
        assert_eq!(
            fs::read_link(home.join("harness.config.toml")).unwrap(),
            foreign
        );
        assert!(!home.join("config.toml").exists());
    }
}
