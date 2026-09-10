// Private unit module; kept below tests/ so Cargo does not treat it as an integration crate.
use super::*;
use std::{fs, path::PathBuf};

fn fixture() -> PathBuf {
    let root = tempfile::Builder::new()
        .prefix("harness-atomic-cleanup-")
        .tempdir()
        .unwrap()
        .keep();
    fs::create_dir(root.join("state")).unwrap();
    for name in ["owner", "journal", "applied", "undone"] {
        fs::write(root.join("state").join(name), name.as_bytes()).unwrap();
    }
    root
}

fn records(root: &Path) -> Vec<FileGuard> {
    ["journal", "applied", "undone"]
        .into_iter()
        .map(|name| {
            FileGuard::open_regular(&root.join("state").join(name), name.as_bytes()).unwrap()
        })
        .collect()
}

fn assert_state(root: &Path, removed: bool) {
    assert_eq!(fs::read(root.join("state/owner")).unwrap(), b"owner");
    for name in ["journal", "applied", "undone"] {
        let path = root.join("state").join(name);
        if removed {
            assert!(!path.exists());
        } else {
            assert_eq!(fs::read(path).unwrap(), name.as_bytes());
        }
    }
}

#[test]
fn sibling_cleanup_is_atomic_at_every_failure_boundary() {
    for (phase, at) in [
        ("released", 0),
        ("staged", 0),
        ("staged", 1),
        ("before-commit", 0),
        ("committed", 0),
    ] {
        let root = fixture();
        let owner = FileGuard::open_regular(&root.join("state/owner"), b"owner").unwrap();
        let guards = records(&root);
        let identities = guards
            .iter()
            .map(|guard| guard.object_identity().unwrap())
            .collect::<Vec<_>>();
        assert!(
            owner
                .remove_siblings(guards, |current, index| {
                    if current == phase && index == at {
                        Err(io::Error::other("owned cleanup stop"))
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        drop(owner);
        assert_state(&root, phase == "committed");
        if phase != "committed" {
            let current = records(&root);
            assert_eq!(
                current
                    .iter()
                    .map(|guard| guard.object_identity().unwrap())
                    .collect::<Vec<_>>(),
                identities
            );
        }
    }
    let root = fixture();
    let owner = FileGuard::open_regular(&root.join("state/owner"), b"owner").unwrap();
    owner
        .remove_siblings(records(&root), |_, _| Ok(()))
        .unwrap();
    drop(owner);
    assert_state(&root, true);
}

#[test]
fn sibling_cleanup_preserves_foreign_replacement_and_a_changed_later_record() {
    for case in ["same-bytes-new-id", "same-id-new-bytes", "later-record"] {
        let root = fixture();
        let owner = FileGuard::open_regular(&root.join("state/owner"), b"owner").unwrap();
        let result = owner.remove_siblings(records(&root), |phase, index| {
            if case == "same-bytes-new-id" && phase == "released" {
                fs::rename(root.join("state/journal"), root.join("saved"))?;
                fs::write(root.join("state/journal"), b"journal")?;
            } else if case == "same-id-new-bytes" && phase == "released" {
                fs::write(root.join("state/journal"), b"foreign bytes")?;
            } else if case == "later-record" && phase == "staged" && index == 0 {
                fs::write(root.join("state/undone"), b"foreign bytes")?;
            }
            Ok(())
        });
        assert!(result.is_err());
        drop(owner);
        assert_eq!(fs::read(root.join("state/applied")).unwrap(), b"applied");
        if case == "same-bytes-new-id" {
            assert_eq!(fs::read(root.join("saved")).unwrap(), b"journal");
        }
        assert_eq!(
            fs::read(root.join("state/journal")).unwrap(),
            if case == "same-id-new-bytes" {
                b"foreign bytes".as_slice()
            } else {
                b"journal".as_slice()
            }
        );
        assert_eq!(
            fs::read(root.join("state/undone")).unwrap(),
            if case == "later-record" {
                b"foreign bytes".as_slice()
            } else {
                b"undone".as_slice()
            }
        );
    }
}

#[test]
fn sibling_cleanup_requires_the_retained_parent_and_regular_records() {
    let root = fixture();
    let owner = FileGuard::open_regular(&root.join("state/owner"), b"owner").unwrap();
    fs::write(root.join("foreign"), b"foreign").unwrap();
    let foreign = FileGuard::open_regular(&root.join("foreign"), b"foreign").unwrap();
    assert!(owner.remove_siblings(vec![foreign], |_, _| Ok(())).is_err());
    std::os::windows::fs::symlink_file(root.join("foreign"), root.join("state/link")).unwrap();
    let link = FileGuard::capture_link(&root.join("state/link")).unwrap();
    assert!(owner.remove_siblings(vec![link], |_, _| Ok(())).is_err());
    owner
        .remove_siblings(records(&root), |phase, _| {
            if phase == "released" {
                assert!(fs::rename(root.join("state"), root.join("moved-state")).is_err());
            }
            Ok(())
        })
        .unwrap();
    drop(owner);
    assert_state(&root, true);
    assert_eq!(fs::read(root.join("foreign")).unwrap(), b"foreign");
    assert!(root.join("state/link").is_symlink());
    fs::rename(root.join("state"), root.join("moved-state")).unwrap();
    fs::rename(root.join("moved-state"), root.join("state")).unwrap();
}

#[test]
#[ignore = "owned atomic-cleanup subprocess fixture"]
fn cleanup_process_fixture() {
    let root = PathBuf::from(std::env::var_os("HARNESS_ATOMIC_CLEANUP_ROOT").unwrap());
    let requested = std::env::var("HARNESS_ATOMIC_CLEANUP_PHASE").unwrap();
    let owner = FileGuard::open_regular(&root.join("state/owner"), b"owner").unwrap();
    owner
        .remove_siblings(records(&root), |phase, index| {
            if phase == requested && index == 0 {
                fs::write(root.join("ready"), phase)?;
                std::thread::sleep(std::time::Duration::from_secs(20));
                std::process::exit(79);
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn killed_sibling_cleanup_has_only_whole_before_or_after_state() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for phase in ["staged", "before-commit", "committed"] {
        let root = fixture();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "registration_native::atomic_cleanup_tests::cleanup_process_fixture",
                "--nocapture",
            ])
            .env("HARNESS_ATOMIC_CLEANUP_ROOT", &root)
            .env("HARNESS_ATOMIC_CLEANUP_PHASE", phase)
            .stdin(Stdio::null())
            .stdout(fs::File::create(root.join("child.stdout")).unwrap())
            .stderr(fs::File::create(root.join("child.stderr")).unwrap())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        while !root.join("ready").exists() && Instant::now() < until {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let reached = fs::read(root.join("ready")).ok().as_deref() == Some(phase.as_bytes());
        let _ = child.kill();
        let exit = child.wait().unwrap();
        assert!(reached, "boundary {phase}: {}", root.display());
        assert!(!exit.success());
        assert_ne!(exit.code(), Some(79));
        assert_state(&root, phase == "committed");
    }
}
