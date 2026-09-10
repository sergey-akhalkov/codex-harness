use super::*;
use std::fs;

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("owned");
    native_build::owner_root(&state).unwrap();
    (temp, state)
}

fn pointer(stage: &str) -> Vec<u8> {
    serde_json::to_vec(&Pointer {
        schema: 1,
        slot: "nuphus".into(),
        stage: stage.into(),
        manifest_sha256: "a".repeat(64),
        node: None,
    })
    .unwrap()
}

fn receipt(state: &Path, hash: &str) -> Snapshot {
    observe(&history(state, "nuphus", hash)).unwrap().unwrap()
}

fn inspected_pointer(state: &Path) -> (PathBuf, String, Vec<u8>) {
    let manifest = br#"{"name":"@nuphus/nuphus-mcp-win32-x64","version":"0.2.2"}"#;
    // Inert bytes are used only by inspection/recovery, never runtime probes.
    let (stage, digest) = dependency_candidate::tests::write_stage(
        state,
        "@nuphus/nuphus-mcp-win32-x64",
        "0.2.2",
        &[
            ("package.json", manifest),
            ("bin/nuphus-mcp.exe", b"inert"),
            ("bin/onnxruntime.dll", b"inert"),
            ("bin/onnxruntime_providers_shared.dll", b"inert"),
        ],
        json!({}),
    );
    let pointer = Pointer {
        schema: 1,
        slot: "nuphus".into(),
        stage: stage.file_name().unwrap().to_str().unwrap().into(),
        manifest_sha256: digest.clone(),
        node: None,
    };
    (stage, digest, serde_json::to_vec(&pointer).unwrap())
}

#[test]
fn public_selection_refuses_a_foreign_identical_replacement_before_validation() {
    let (_temp, state) = fixture();
    let (stage, digest, pointer) = inspected_pointer(&state);
    transact(&state, "nuphus", None, Some(pointer)).unwrap();
    assert_eq!(selected(&state, "nuphus").unwrap()["status"], "selected");
    let original = observe(&active(&state, "nuphus")).unwrap().unwrap();
    fs::rename(active(&state, "nuphus"), state.join("original")).unwrap();
    fs::write(active(&state, "nuphus"), &original.bytes).unwrap();
    let foreign = observe(&active(&state, "nuphus")).unwrap().unwrap();
    assert_ne!(foreign.identity, original.identity);
    assert_eq!(
        selected(&state, "nuphus").unwrap_err().to_string(),
        invalid().to_string()
    );
    assert_eq!(
        activate(&state, "nuphus", &stage, &digest, None)
            .unwrap_err()
            .to_string(),
        invalid().to_string()
    );
    assert_eq!(observe(&active(&state, "nuphus")).unwrap(), Some(foreign));
    assert!(!pending(&state, "nuphus").exists());
}

#[test]
fn interrupted_update_and_deletion_recover_with_usable_rollback() {
    for deletion in [false, true] {
        for published in [false, true] {
            let (_temp, state) = fixture();
            let (_, _, first) = inspected_pointer(&state);
            let (_, _, second) = inspected_pointer(&state);
            let initial_receipt = transact(&state, "nuphus", None, Some(first)).unwrap();
            let original = observe(&active(&state, "nuphus")).unwrap().unwrap();
            let next = (!deletion).then(|| Snapshot {
                identity: original.identity.clone(),
                bytes: second,
            });
            let journal = Journal {
                schema: 1,
                slot: "nuphus".into(),
                before: Some(original.clone()),
                after: next.clone(),
            };
            StagedFile::create(
                &pending(&state, "nuphus"),
                &serde_json::to_vec(&journal).unwrap(),
            )
            .unwrap()
            .commit()
            .unwrap();
            if published {
                if let Some(next) = &next {
                    replace(&active(&state, "nuphus"), &original, next).unwrap();
                } else {
                    remove(&active(&state, "nuphus"), &original).unwrap();
                }
            }
            let recovered = recover(&state, "nuphus").unwrap();
            let current = observe(&active(&state, "nuphus")).unwrap().unwrap();
            assert_eq!(current.bytes, original.bytes);
            assert_eq!(selected(&state, "nuphus").unwrap()["status"], "selected");
            assert!(!pending(&state, "nuphus").exists());
            let usable = if deletion {
                // Simulate losing the successful response after journal removal.
                // A fresh recovery call must rediscover the durable authority.
                let retry = recover(&state, "nuphus").unwrap();
                assert_eq!(retry["changed"], false);
                assert_eq!(
                    retry["rollback_receipt_sha256"],
                    recovered["rollback_receipt_sha256"]
                );
                recovered["rollback_receipt_sha256"].as_str().unwrap()
            } else {
                &initial_receipt
            };
            if deletion && published {
                assert_ne!(current.identity, original.identity);
            }
            assert_eq!(rollback(&state, "nuphus", usable).unwrap()["changed"], true);
            assert!(!active(&state, "nuphus").exists());
        }
    }
}

#[test]
fn retained_metadata_transactions_create_replace_remove_and_preserve_package_trees() {
    let (_temp, state) = fixture();
    let adopted = state.join("unrelated-adopted-package");
    fs::create_dir(&adopted).unwrap();
    fs::write(adopted.join("payload"), b"foreign").unwrap();
    let _lock = native_build::lock_owned_state(&state).unwrap();
    let first = transact(&state, "nuphus", None, Some(pointer("candidate-first"))).unwrap();
    let initial = observe(&active(&state, "nuphus")).unwrap().unwrap();
    let second = transact(
        &state,
        "nuphus",
        Some(initial.clone()),
        Some(pointer("candidate-second")),
    )
    .unwrap();
    let updated = observe(&active(&state, "nuphus")).unwrap().unwrap();
    assert_eq!(initial.identity, updated.identity);
    let second = Journal::parse(&receipt(&state, &second).bytes, "nuphus").unwrap();
    assert_eq!(second.before, Some(initial));
    assert_eq!(second.after, Some(updated.clone()));
    let removed = transact(&state, "nuphus", Some(updated), None).unwrap();
    assert!(observe(&active(&state, "nuphus")).unwrap().is_none());
    assert!(
        Journal::parse(&receipt(&state, &removed).bytes, "nuphus")
            .unwrap()
            .after
            .is_none()
    );
    assert!(
        Journal::parse(&receipt(&state, &first).bytes, "nuphus")
            .unwrap()
            .before
            .is_none()
    );
    assert!(observe(&pending(&state, "nuphus")).unwrap().is_none());
    assert_eq!(fs::read(adopted.join("payload")).unwrap(), b"foreign");
}

#[test]
fn changed_bytes_and_same_byte_replacement_survive_stale_write_and_delete() {
    for same_bytes in [false, true] {
        for deletion in [false, true] {
            let (_temp, state) = fixture();
            let _lock = native_build::lock_owned_state(&state).unwrap();
            transact(&state, "nuphus", None, Some(pointer("candidate-first"))).unwrap();
            let original = observe(&active(&state, "nuphus")).unwrap().unwrap();
            fs::rename(active(&state, "nuphus"), state.join("retained-original")).unwrap();
            fs::write(
                active(&state, "nuphus"),
                if same_bytes {
                    original.bytes.clone()
                } else {
                    b"foreign".to_vec()
                },
            )
            .unwrap();
            let foreign = observe(&active(&state, "nuphus")).unwrap().unwrap();
            assert_ne!(original.identity, foreign.identity);
            assert!(
                transact(
                    &state,
                    "nuphus",
                    Some(original),
                    if deletion {
                        None
                    } else {
                        Some(pointer("candidate-second"))
                    }
                )
                .is_err()
            );
            assert_eq!(
                observe(&active(&state, "nuphus")).unwrap(),
                Some(foreign.clone())
            );
            let journal = observe(&pending(&state, "nuphus")).unwrap().unwrap();
            drop(_lock);
            assert!(recover(&state, "nuphus").is_err());
            assert_eq!(observe(&active(&state, "nuphus")).unwrap(), Some(foreign));
            assert_eq!(observe(&pending(&state, "nuphus")).unwrap(), Some(journal));
        }
    }
}

#[test]
fn interrupted_initial_creation_recovers_before_and_after_publication() {
    for published in [false, true] {
        let (_temp, state) = fixture();
        let staged =
            StagedFile::create(&active(&state, "nuphus"), &pointer("candidate-first")).unwrap();
        let journal = Journal {
            schema: 1,
            slot: "nuphus".into(),
            before: None,
            after: Some(Snapshot {
                identity: staged.identity(),
                bytes: pointer("candidate-first"),
            }),
        };
        StagedFile::create(
            &pending(&state, "nuphus"),
            &serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap()
        .commit()
        .unwrap();
        if published {
            staged.commit().unwrap();
        } else {
            drop(staged);
        }
        assert_eq!(recover(&state, "nuphus").unwrap()["changed"], true);
        assert!(observe(&active(&state, "nuphus")).unwrap().is_none());
        assert!(observe(&pending(&state, "nuphus")).unwrap().is_none());
        assert_eq!(recover(&state, "nuphus").unwrap()["changed"], false);
    }
}

#[test]
fn creation_and_pending_journal_conflicts_preserve_existing_objects() {
    let (_temp, state) = fixture();
    let _lock = native_build::lock_owned_state(&state).unwrap();
    fs::write(active(&state, "nuphus"), b"foreign").unwrap();
    assert!(transact(&state, "nuphus", None, Some(pointer("candidate-first"))).is_err());
    assert_eq!(fs::read(active(&state, "nuphus")).unwrap(), b"foreign");
    assert!(!pending(&state, "nuphus").exists());
    fs::remove_file(active(&state, "nuphus")).unwrap();
    fs::write(pending(&state, "nuphus"), b"foreign journal").unwrap();
    assert!(transact(&state, "nuphus", None, Some(pointer("candidate-first"))).is_err());
    assert!(!active(&state, "nuphus").exists());
    assert_eq!(
        fs::read(pending(&state, "nuphus")).unwrap(),
        b"foreign journal"
    );
}

#[test]
fn traversal_slot_schema_digest_and_runtime_mismatch_are_rejected() {
    for stage in [
        "candidate-../escape",
        "candidate-a/b",
        "candidate-a\\b",
        "candidate-a:",
        "candidate-a.",
        "candidate-a ",
        "candidate-",
        "other",
    ] {
        assert!(
            Pointer::parse(&pointer(stage), "nuphus").is_err(),
            "{stage}"
        );
    }
    assert!(Pointer::parse(&pointer("candidate-good"), "../nuphus").is_err());
    assert!(Pointer::parse(&pointer("candidate-good"), "basedpyright").is_err());
    let mut value: Value = serde_json::from_slice(&pointer("candidate-good")).unwrap();
    for hash in ["a".repeat(63), "A".repeat(64), "g".repeat(64)] {
        value["manifest_sha256"] = json!(hash);
        assert!(Pointer::parse(&serde_json::to_vec(&value).unwrap(), "nuphus").is_err());
    }
    value["manifest_sha256"] = json!("a".repeat(64));
    value["extra"] = json!(true);
    assert!(Pointer::parse(&serde_json::to_vec(&value).unwrap(), "nuphus").is_err());
}

#[test]
fn codegraph_slot_maps_to_the_published_package_without_a_node_companion() {
    assert_eq!(package("codegraph").unwrap(), "@colbymchenry/codegraph");
    let bytes = serde_json::to_vec(&Pointer {
        schema: 1,
        slot: "codegraph".into(),
        stage: "candidate-good".into(),
        manifest_sha256: "a".repeat(64),
        node: None,
    })
    .unwrap();
    let pointer = Pointer::parse(&bytes, "codegraph").unwrap();
    assert_eq!(pointer.slot, "codegraph");
    assert!(pointer.node().is_none());
    assert!(package("codegraph-win32-x64").is_err());
}

#[test]
fn absent_inspection_and_recovery_have_no_acquisition_or_selection_side_effects() {
    let (_temp, state) = fixture();
    assert_eq!(selected(&state, "nuphus").unwrap()["status"], "absent");
    assert_eq!(
        selected(&state, "nuphus").unwrap()["package_code_executed"],
        false
    );
    assert_eq!(recover(&state, "nuphus").unwrap()["changed"], false);
    assert!(!state.join("dependency-staging").exists());
    assert!(!active(&state, "nuphus").exists());
    assert!(!pending(&state, "nuphus").exists());
}
