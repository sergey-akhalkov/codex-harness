// Included inside the crate-private bridge: no public test-only API is needed.
use super::*;
use crate::inventory::Connection;
use std::{
    cell::Cell,
    os::windows::fs::{symlink_dir, symlink_file},
};

// Fixture format only; the bridge deliberately defines no installation schema.
#[derive(Serialize, Deserialize)]
struct Stored {
    links: Vec<MetadataLink>,
    identity: LinkIdentity,
}

fn encode_view(view: &MetadataView) -> io::Result<Vec<u8>> {
    Ok(serde_json::to_vec(&Stored {
        links: view.links.clone(),
        identity: view.identity.clone(),
    })?)
}

fn fixture() -> PathBuf {
    let root = tempfile::Builder::new()
        .prefix("harness-metadata-")
        .tempdir()
        .unwrap()
        .keep();
    println!("metadata evidence: {}", root.display());
    fs::write(root.join("source-file"), b"source file").unwrap();
    fs::write(root.join("next-file"), b"next file").unwrap();
    fs::create_dir(root.join("source-dir")).unwrap();
    fs::write(root.join("source-dir/keep"), b"directory source").unwrap();
    fs::create_dir(root.join("next-dir")).unwrap();
    fs::write(root.join("config"), b"before").unwrap();
    symlink_file(root.join("source-file"), root.join("reuse-file")).unwrap();
    symlink_dir(root.join("source-dir"), root.join("reuse-dir")).unwrap();
    symlink_file(root.join("source-file"), root.join("replace")).unwrap();
    symlink_dir(root.join("source-dir"), root.join("remove")).unwrap();
    root
}

fn link(root: &Path, destination: &str, source: &str) -> Link {
    Link {
        kind: "fixture".into(),
        name: destination.into(),
        source: root.join(source),
        destination: root.join(destination),
        connection: Connection::Missing,
    }
}

fn links(root: &Path) -> Vec<Link> {
    vec![
        link(root, "reuse-file", "source-file"),
        link(root, "reuse-dir", "source-dir"),
        link(root, "fresh-file", "source-file"),
        link(root, "fresh-dir", "source-dir"),
    ]
}

fn changes(root: &Path) -> Vec<LinkChange> {
    vec![
        LinkChange::replace(
            &root.join("replace"),
            &root.join("source-file"),
            &root.join("next-dir"),
        )
        .unwrap(),
        LinkChange::remove(&root.join("remove"), &root.join("source-dir")).unwrap(),
    ]
}

fn live(path: &Path) -> MetadataLink {
    let guard = FileGuard::capture_link(path).unwrap();
    guarded(path, &guard).unwrap()
}

fn no_temporary(root: &Path, reg: &Registration) {
    assert!(!reg.journal_path().exists());
    assert!(!reg.state.join(COMPLETION).exists());
    assert!(!reg.state.join(COMMIT).exists());
    for entry in fs::read_dir(root).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".codex-harness-")
        );
    }
}

fn original(root: &Path, reg: &Registration) {
    no_temporary(root, reg);
    for path in ["fresh-file", "fresh-dir", "created"] {
        assert!(fs::symlink_metadata(root.join(path)).is_err(), "{path}");
    }
    assert_eq!(fs::read(root.join("config")).unwrap(), b"before");
    assert_eq!(
        fs::read_link(root.join("replace")).unwrap(),
        root.join("source-file")
    );
    assert_eq!(
        fs::read_link(root.join("remove")).unwrap(),
        root.join("source-dir")
    );
    assert_eq!(fs::read(root.join("source-file")).unwrap(), b"source file");
    assert_eq!(
        fs::read(root.join("source-dir/keep")).unwrap(),
        b"directory source"
    );
}

fn read_stored(root: &Path) -> Stored {
    let (guard, bytes) = FileGuard::read_regular(&root.join("metadata")).unwrap();
    let stored: Stored = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(stored.identity, guard.object_identity().unwrap());
    for expected in &stored.links {
        assert_eq!(&live(&expected.path), expected);
    }
    stored
}

#[test]
fn metadata_actual_apply_finish_reload_reuse_update_preserves_recorded_ids() {
    let root = fixture();
    let reg = Registration::open(&root.join("state")).unwrap();
    let previous =
        ["reuse-file", "reuse-dir", "replace", "remove"].map(|name| live(&root.join(name)));
    // Exercise the reserved metadata identity through actual NTFS name tunneling.
    fs::write(root.join("metadata"), b"recent occupant").unwrap();
    fs::remove_file(root.join("metadata")).unwrap();
    reg.apply_with_metadata(
        &links(&root),
        &[ConfigSnapshot::read(&root.join("config"))
            .unwrap()
            .plan_replace(b"after")
            .unwrap()],
        &[ConfigCreation::new(&root.join("created"), b"created").unwrap()],
        &changes(&root),
        MetadataDestination::absent(&root.join("metadata")).unwrap(),
        |view| {
            assert_eq!(view.path, root.join("metadata"));
            assert_eq!(view.links.len(), 5);
            assert_eq!(view.previous.len(), previous.len());
            for old in &previous {
                assert!(view.previous.contains(old));
            }
            for name in ["metadata", "fresh-file", "fresh-dir", "created"] {
                assert!(fs::symlink_metadata(root.join(name)).is_err(), "{name}");
            }
            no_temporary(&root, &reg);
            for old in &previous {
                assert!(fs::rename(&old.path, root.join("unexpected-rename")).is_err());
            }
            encode_view(view)
        },
    )
    .unwrap();
    let stored = read_stored(&root);
    assert_eq!(fs::read(root.join("config")).unwrap(), b"after");
    assert_eq!(fs::read(root.join("created")).unwrap(), b"created");
    let journal: Journal = serde_json::from_slice(&fs::read(reg.journal_path()).unwrap()).unwrap();
    assert_eq!(journal.schema, SCHEMA);
    let witness = journal
        .creations
        .iter()
        .find(|item| item.path == root.join("metadata"))
        .unwrap();
    assert_eq!(witness.published_identity(), &stored.identity);
    assert_eq!(
        witness.published_bytes(),
        fs::read(root.join("metadata")).unwrap()
    );
    assert!(reg.finish(&root.join("metadata")).unwrap().committed);
    drop(reg);

    let reg = Registration::open(&root.join("state")).unwrap();
    let stored = read_stored(&root);
    let snapshot = ConfigSnapshot::read(&root.join("metadata")).unwrap();
    let kept = stored
        .links
        .iter()
        .filter(|item| ![root.join("fresh-file"), root.join("fresh-dir")].contains(&item.path))
        .map(|item| Link {
            kind: "fixture".into(),
            name: "reused".into(),
            source: item.target.clone(),
            destination: item.path.clone(),
            connection: Connection::Missing,
        })
        .collect::<Vec<_>>();
    let next = [
        LinkChange::replace(
            &root.join("fresh-file"),
            &root.join("source-file"),
            &root.join("next-file"),
        )
        .unwrap(),
        LinkChange::remove(&root.join("fresh-dir"), &root.join("source-dir")).unwrap(),
    ];
    reg.apply_with_metadata(
        &kept,
        &[],
        &[],
        &next,
        MetadataDestination::existing(&snapshot).unwrap(),
        |view| {
            assert_eq!(view.identity, stored.identity);
            assert_eq!(view.previous.len(), stored.links.len());
            for old in &stored.links {
                assert!(view.previous.contains(old));
            }
            assert!(fs::write(root.join("metadata"), b"unexpected write").is_err());
            assert!(fs::rename(root.join("metadata"), root.join("unexpected-metadata")).is_err());
            assert!(!reg.journal_path().exists());
            encode_view(view)
        },
    )
    .unwrap();
    assert!(reg.finish(&root.join("metadata")).unwrap().committed);
    let final_stored = read_stored(&root);
    assert_eq!(final_stored.links.len(), 4);
    assert_eq!(final_stored.identity, stored.identity);
    for old in &kept {
        assert!(final_stored.links.contains(&live(&old.destination)));
    }
    no_temporary(&root, &reg);
    assert!(!reg.finish(&root.join("metadata")).unwrap().committed);
}

#[test]
fn metadata_uncommitted_disconnect_restores_both_metadata_modes_and_link_ids() {
    for existing in [false, true] {
        let root = fixture();
        let reg = Registration::open(&root.join("state")).unwrap();
        let old = [live(&root.join("replace")), live(&root.join("remove"))];
        let destination = if existing {
            fs::write(root.join("metadata"), b"old metadata").unwrap();
            MetadataDestination::existing(&ConfigSnapshot::read(&root.join("metadata")).unwrap())
                .unwrap()
        } else {
            MetadataDestination::absent(&root.join("metadata")).unwrap()
        };
        reg.apply_with_metadata(
            &links(&root),
            &[ConfigSnapshot::read(&root.join("config"))
                .unwrap()
                .plan_replace(b"after")
                .unwrap()],
            &[ConfigCreation::new(&root.join("created"), b"created").unwrap()],
            &changes(&root),
            destination,
            encode_view,
        )
        .unwrap();
        let bytes = fs::read(reg.journal_path()).unwrap();
        assert!(reg.recover().is_err());
        assert_eq!(fs::read(reg.journal_path()).unwrap(), bytes);
        assert!(!reg.disconnect().unwrap().committed);
        original(&root, &reg);
        for expected in old {
            assert_eq!(live(&expected.path), expected);
        }
        if existing {
            assert_eq!(fs::read(root.join("metadata")).unwrap(), b"old metadata");
        } else {
            assert!(!root.join("metadata").exists());
        }
    }
}

#[test]
fn metadata_reserved_conflicts_and_snapshot_rebinding_never_invoke_builder() {
    for case in [
        "link-overlap",
        "state-overlap",
        "config-overlap",
        "rebound",
        "hardlink",
        "reparse",
    ] {
        let root = fixture();
        let reg = Registration::open(&root.join("state")).unwrap();
        fs::write(root.join("metadata"), b"private original").unwrap();
        let snapshot = ConfigSnapshot::read(&root.join("metadata")).unwrap();
        let mut configs = Vec::new();
        let destination = match case {
            "link-overlap" => MetadataDestination::absent(&root.join("fresh-file")).unwrap(),
            "state-overlap" => MetadataDestination::absent(&reg.state.join("reserved")).unwrap(),
            "config-overlap" => {
                configs.push(snapshot.plan_replace(b"other").unwrap());
                MetadataDestination::existing(&snapshot).unwrap()
            }
            "rebound" => {
                fs::rename(root.join("metadata"), root.join("retained-metadata")).unwrap();
                fs::write(root.join("metadata"), snapshot.contents()).unwrap();
                MetadataDestination::existing(&snapshot).unwrap()
            }
            "hardlink" => {
                fs::hard_link(root.join("metadata"), root.join("retained-metadata")).unwrap();
                MetadataDestination::existing(&snapshot).unwrap()
            }
            "reparse" => {
                fs::rename(root.join("metadata"), root.join("retained-metadata")).unwrap();
                symlink_file(root.join("retained-metadata"), root.join("metadata")).unwrap();
                MetadataDestination::existing(&snapshot).unwrap()
            }
            _ => unreachable!(),
        };
        let called = Cell::new(false);
        assert!(
            reg.apply_with_metadata(
                &links(&root),
                &configs,
                &[],
                &changes(&root),
                destination,
                |_| {
                    called.set(true);
                    Ok(Vec::new())
                }
            )
            .is_err(),
            "{case}"
        );
        assert!(!called.get(), "{case}");
        original(&root, &reg);
        assert_eq!(
            fs::read(root.join("metadata")).unwrap(),
            b"private original"
        );
    }
}

#[test]
fn metadata_foreign_same_target_ids_can_be_rejected_from_guarded_prior_view() {
    for changed in ["reuse-file", "replace"] {
        let root = fixture();
        let reg = Registration::open(&root.join("state")).unwrap();
        let owned = live(&root.join(changed));
        fs::rename(root.join(changed), root.join("retained-owned-link")).unwrap();
        symlink_file(root.join("source-file"), root.join(changed)).unwrap();
        let foreign = live(&root.join(changed));
        assert_ne!(foreign.identity, owned.identity);
        let called = Cell::new(false);
        let result = reg.apply_with_metadata(
            &links(&root),
            &[],
            &[],
            &changes(&root),
            MetadataDestination::absent(&root.join("metadata")).unwrap(),
            |view| {
                called.set(true);
                assert!(view.previous.contains(&foreign));
                assert!(!view.previous.contains(&owned));
                Err(io::Error::other("PRIVATE_OWNERSHIP_DETAILS"))
            },
        );
        assert!(called.get());
        assert!(
            !result
                .unwrap_err()
                .to_string()
                .contains("PRIVATE_OWNERSHIP_DETAILS")
        );
        original(&root, &reg);
        assert_eq!(live(&root.join(changed)), foreign);
        assert!(!root.join("metadata").exists());
        assert!(fs::symlink_metadata(root.join("retained-owned-link")).is_ok());
    }
}

#[test]
fn metadata_reused_alias_retains_actual_stored_target_without_resolving_it() {
    let root = fixture();
    fs::create_dir(root.join("source-parent")).unwrap();
    fs::write(root.join("source-parent/file"), b"alias source").unwrap();
    symlink_dir(root.join("source-parent"), root.join("source-alias")).unwrap();
    symlink_file(root.join("source-alias/file"), root.join("alias-link")).unwrap();
    let expected = live(&root.join("alias-link"));
    let reg = Registration::open(&root.join("state")).unwrap();
    reg.apply_with_metadata(
        &[link(&root, "alias-link", "source-parent/file")],
        &[],
        &[],
        &[],
        MetadataDestination::absent(&root.join("metadata")).unwrap(),
        |view| {
            assert_eq!(view.links, vec![expected.clone()]);
            assert_eq!(view.previous, vec![expected.clone()]);
            encode_view(view)
        },
    )
    .unwrap();
    assert!(reg.finish(&root.join("metadata")).unwrap().committed);
    assert_eq!(read_stored(&root).links, vec![expected]);
}

#[test]
fn metadata_builder_error_and_both_size_bounds_publish_nothing() {
    for case in ["error", "byte-bound", "encoded-bound"] {
        for existing in [false, true] {
            let root = fixture();
            let reg = Registration::open(&root.join("state")).unwrap();
            let destination = if existing {
                fs::write(root.join("metadata"), b"old metadata").unwrap();
                MetadataDestination::existing(
                    &ConfigSnapshot::read(&root.join("metadata")).unwrap(),
                )
                .unwrap()
            } else {
                MetadataDestination::absent(&root.join("metadata")).unwrap()
            };
            let called = Cell::new(false);
            let result = reg.apply_with_metadata(
                &links(&root),
                &[],
                &[],
                &changes(&root),
                destination,
                |_| {
                    called.set(true);
                    no_temporary(&root, &reg);
                    match case {
                        "error" => Err(io::Error::other("PRIVATE_BUILDER_BYTES")),
                        "byte-bound" => Ok(vec![255; MAX_JOURNAL as usize + 1]),
                        "encoded-bound" => Ok(vec![255; MAX_JOURNAL as usize / 4]),
                        _ => unreachable!(),
                    }
                },
            );
            assert!(called.get());
            assert!(
                !result
                    .unwrap_err()
                    .to_string()
                    .contains("PRIVATE_BUILDER_BYTES")
            );
            original(&root, &reg);
            if existing {
                assert_eq!(fs::read(root.join("metadata")).unwrap(), b"old metadata");
            } else {
                assert!(!root.join("metadata").exists());
            }
        }
    }
}

#[test]
fn metadata_foreign_arrival_during_preparation_is_preserved_without_intent() {
    for path in ["metadata", "fresh-file", "created", "config"] {
        let root = fixture();
        let reg = Registration::open(&root.join("state")).unwrap();
        let config = ConfigSnapshot::read(&root.join("config"))
            .unwrap()
            .plan_replace(b"after")
            .unwrap();
        let result = reg.apply_with_metadata(
            &links(&root),
            &[config],
            &[ConfigCreation::new(&root.join("created"), b"created").unwrap()],
            &changes(&root),
            MetadataDestination::absent(&root.join("metadata")).unwrap(),
            |_| {
                fs::write(root.join(path), b"foreign arrival").unwrap();
                Ok(b"candidate metadata".to_vec())
            },
        );
        assert!(result.is_err(), "{path}");
        no_temporary(&root, &reg);
        assert_eq!(fs::read(root.join(path)).unwrap(), b"foreign arrival");
        for absent in ["metadata", "fresh-file", "fresh-dir", "created"] {
            if path != absent {
                assert!(fs::symlink_metadata(root.join(absent)).is_err());
            }
        }
        assert_eq!(
            fs::read_link(root.join("replace")).unwrap(),
            root.join("source-file")
        );
        assert_eq!(
            fs::read_link(root.join("remove")).unwrap(),
            root.join("source-dir")
        );
    }
}

#[test]
fn metadata_pending_journal_rejects_rebuilding_without_invoking_builder() {
    let root = fixture();
    let reg = Registration::open(&root.join("state")).unwrap();
    reg.apply_with_metadata(
        &links(&root),
        &[],
        &[],
        &[],
        MetadataDestination::absent(&root.join("metadata")).unwrap(),
        encode_view,
    )
    .unwrap();
    let journal = fs::read(reg.journal_path()).unwrap();
    let completion = fs::read(reg.state.join(COMPLETION)).unwrap();
    let bytes = fs::read(root.join("metadata")).unwrap();
    let called = Cell::new(false);
    assert!(
        reg.apply_with_metadata(
            &links(&root),
            &[],
            &[],
            &[],
            MetadataDestination::existing(&ConfigSnapshot::read(&root.join("metadata")).unwrap())
                .unwrap(),
            |_| {
                called.set(true);
                Ok(Vec::new())
            }
        )
        .is_err()
    );
    assert!(!called.get());
    assert_eq!(fs::read(reg.journal_path()).unwrap(), journal);
    assert_eq!(fs::read(reg.state.join(COMPLETION)).unwrap(), completion);
    assert_eq!(fs::read(root.join("metadata")).unwrap(), bytes);
    assert!(reg.finish(&root.join("metadata")).unwrap().committed);
}

#[test]
fn metadata_same_target_replacement_after_apply_cannot_commit_stale_ownership() {
    let root = fixture();
    let reg = Registration::open(&root.join("state")).unwrap();
    reg.apply_with_metadata(
        &[link(&root, "reuse-file", "source-file")],
        &[],
        &[],
        &[],
        MetadataDestination::absent(&root.join("metadata")).unwrap(),
        encode_view,
    )
    .unwrap();
    let metadata = fs::read(root.join("metadata")).unwrap();
    let journal = fs::read(reg.journal_path()).unwrap();
    let completion = fs::read(reg.state.join(COMPLETION)).unwrap();
    let owned = live(&root.join("reuse-file"));
    fs::rename(root.join("reuse-file"), root.join("retained-owned-link")).unwrap();
    symlink_file(root.join("source-file"), root.join("reuse-file")).unwrap();
    let foreign = live(&root.join("reuse-file"));
    assert_ne!(owned.identity, foreign.identity);
    // Retain the original failure oracle before any post-fix assertions.
    assert!(
        reg.finish(&root.join("metadata")).is_err(),
        "finish committed metadata with a stale reused ID: {}",
        root.display()
    );
    assert!(reg.disconnect().is_err());
    assert!(
        reg.apply_with_files(
            &[link(&root, "reuse-file", "source-file")],
            &[],
            &[ConfigCreation::new(&root.join("metadata"), &metadata).unwrap()]
        )
        .is_err()
    );
    assert_eq!(fs::read(root.join("metadata")).unwrap(), metadata);
    assert_eq!(fs::read(reg.journal_path()).unwrap(), journal);
    assert_eq!(fs::read(reg.state.join(COMPLETION)).unwrap(), completion);
    assert!(!reg.state.join(COMMIT).exists());
    assert_eq!(live(&root.join("reuse-file")), foreign);
    assert!(fs::symlink_metadata(root.join("retained-owned-link")).is_ok());
}

#[test]
#[ignore = "exact owned child of metadata_process_death_before_intent_leaves_original_state"]
fn metadata_process_fixture() {
    let root = PathBuf::from(std::env::var_os("HARNESS_METADATA_ROOT").unwrap());
    let reg = Registration::open(&root.join("state")).unwrap();
    reg.apply_with_metadata(
        &links(&root),
        &[],
        &[],
        &changes(&root),
        MetadataDestination::absent(&root.join("metadata")).unwrap(),
        |view| {
            fs::write(root.join("paused.tmp"), encode_view(view)?).unwrap();
            fs::rename(root.join("paused.tmp"), root.join("paused")).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(20));
            std::process::exit(79);
        },
    )
    .unwrap();
}

#[test]
fn metadata_process_death_before_intent_leaves_original_state() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let root = fixture();
    let old = [
        live(&root.join("replace")),
        live(&root.join("remove")),
        live(&root.join("reuse-file")),
    ];
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "registration::metadata::tests::metadata_process_fixture",
            "--nocapture",
        ])
        .env("HARNESS_METADATA_ROOT", &root)
        .stdin(Stdio::null())
        .stdout(fs::File::create(root.join("child.stdout.txt")).unwrap())
        .stderr(fs::File::create(root.join("child.stderr.txt")).unwrap())
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while !root.join("paused").exists() && Instant::now() < until {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let reached = fs::read(root.join("paused"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        .is_some();
    // Exact retained child handle: no PID/name search or Rust Drop/unwind oracle.
    let _ = child.kill();
    let status = child.wait().unwrap();
    assert!(
        reached,
        "preparation boundary not reached: {}",
        root.display()
    );
    assert!(!status.success());
    assert_ne!(status.code(), Some(79));
    let reg = Registration::open(&root.join("state")).unwrap();
    original(&root, &reg);
    assert!(!root.join("metadata").exists());
    assert!(!reg.recover().unwrap().committed);
    for expected in old {
        assert_eq!(live(&expected.path), expected);
    }
}
