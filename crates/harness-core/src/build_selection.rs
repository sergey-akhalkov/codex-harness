//! Journaled selection of immutable builds in already owned native state.
//! These operations do not change global command links or manage services.
//! Repair started from damaged artifacts recovers forward to a verified build.
use crate::{
    build_identity::{self, Health},
    native_build,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

const ACTIVE: &str = "active-build.json";
const JOURNAL: &str = "build-selection-journal.json";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    schema: u32,
    build: String,
    record_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
    rollback_usable: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub build: Option<PathBuf>,
    pub changed: bool,
}

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    native_build::ordinary_ancestors(path)?;
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= 64 * 1024 => fs::read(path).map(Some),
        Ok(_) => Err(io::Error::other(
            "Invalid or oversized native selection metadata; preserving it.",
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn pointer_reference(state: &Path, bytes: &[u8]) -> io::Result<(PathBuf, Pointer)> {
    if bytes.len() > 4096 {
        return Err(io::Error::other(
            "Oversized native build pointer; preserving it.",
        ));
    }
    let pointer: Pointer = serde_json::from_slice(bytes)?;
    let name = Path::new(&pointer.build);
    if pointer.schema != 1
        || name.components().count() != 1
        || !matches!(name.components().next(), Some(Component::Normal(_)))
        || pointer.build.contains(['/', '\\', ':'])
        || pointer.record_sha256.len() != 64
        || !pointer.record_sha256.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(io::Error::other(
            "Incompatible native build selection; preserving it.",
        ));
    }
    let build = state.join("builds").join(name);
    native_build::ordinary_ancestors(&build.join("build.json"))?;
    Ok((build, pointer))
}

fn pointer_path(state: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let (build, pointer) = pointer_reference(state, bytes)?;
    if build_identity::hash_file(&build.join("build.json"))? != pointer.record_sha256 {
        return Err(io::Error::other(
            "Selected build metadata changed; explicit bootstrap/repair is required.",
        ));
    }
    Ok(build)
}

fn finish_journal(state: &Path, receipt: &[u8]) -> io::Result<()> {
    let history = state.join("build-selection-history");
    native_build::ordinary_ancestors(&history)?;
    fs::create_dir_all(&history)?;
    let record = history.join(format!("{}.json", build_identity::hash_bytes(receipt)));
    match read_optional(&record)? {
        None => replace_expected(&record, None, Some(receipt))?,
        Some(existing) if existing == receipt => (),
        Some(_) => {
            return Err(io::Error::other(
                "Native selection history conflict; preserving the recovery journal.",
            ));
        }
    }
    replace_expected(&state.join(JOURNAL), Some(receipt), None)
}

fn verified_previous(state: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let build = pointer_path(state, bytes)?;
    let mut check = build_identity::check(&build, None);
    if check.status == Health::Incompatible {
        // A v1 predecessor may have a different compiled binary/input set.
        // Verify every recorded artifact before asking that exact manager.
        // Runtime validation stays with its owning consumer.
        check = native_build::consumer_check(&build, None)?;
    }
    if !matches!(
        check.status,
        Health::Healthy | Health::SourceStale | Health::SourceUnavailable
    ) {
        return Err(io::Error::other(check.action));
    }
    Ok(build)
}

fn replace_expected(path: &Path, before: Option<&[u8]>, after: Option<&[u8]>) -> io::Result<()> {
    if read_optional(path)?.as_deref() != before {
        return Err(io::Error::other(
            "Native selection ownership conflict; target and recovery journal preserved.",
        ));
    }
    if let Some(bytes) = after {
        let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
        temp.write_all(bytes)?;
        temp.as_file().sync_all()?;
        // Recheck after preparing the file. The state lock serializes kit writers.
        if read_optional(path)?.as_deref() != before {
            return Err(io::Error::other(
                "Native selection changed while preparing the write; preserving it.",
            ));
        }
        let file = if before.is_some() {
            temp.persist(path)
        } else {
            temp.persist_noclobber(path)
        }
        .map_err(|e| e.error)?;
        file.sync_all()?;
    } else if before.is_some() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Resolve a healthy selection without Cargo, hooks, model calls or mutation.
pub fn selected(state: &Path) -> io::Result<PathBuf> {
    native_build::verify_owned_state(state)?;
    let state = state.canonicalize()?;
    if read_optional(&state.join(JOURNAL))?.is_some() {
        return Err(io::Error::other(
            "Interrupted build selection; run recover-build --state DIRECTORY.",
        ));
    }
    let bytes = read_optional(&state.join(ACTIVE))?.ok_or_else(|| {
        io::Error::other("No active native build; run activate-build explicitly.")
    })?;
    let build = pointer_path(&state, &bytes)?;
    let check = build_identity::check(&build, None);
    if !check.runtime_allowed {
        return Err(io::Error::other(check.action));
    }
    Ok(build)
}

/// Only observed missing/changed owned bytes establish a damaged predecessor.
/// Unsupported metadata, unreadable files, and failed checks are indeterminate
/// and must stop selection before its journal or pointer is written.
fn predecessor_rollback_usable(state: &Path, bytes: &[u8]) -> io::Result<bool> {
    let (build, pointer) = pointer_reference(state, bytes)?;
    let record = match build_identity::read_record(&build) {
        Ok(record) => record,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(io::Error::other(format!(
                "Unknown predecessor metadata or unreadable record; preserving selection: {error}"
            )));
        }
    };
    build_identity::verify_record_metadata(&record).map_err(|error| {
        io::Error::other(format!(
            "Unknown predecessor metadata; preserving selection: {error}"
        ))
    })?;
    if build_identity::hash_file(&build.join("build.json"))? != pointer.record_sha256 {
        return Ok(false);
    }
    for (name, expected) in &record.binaries {
        let path = build.join(name);
        native_build::ordinary_ancestors(&path)?;
        match build_identity::hash_file(&path) {
            Ok(actual) if actual == *expected => (),
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    verified_previous(state, bytes)?;
    Ok(true)
}

fn stage(state: &Path, build: &Path) -> io::Result<Journal> {
    if read_optional(&state.join(JOURNAL))?.is_some() {
        return Err(io::Error::other(
            "Interrupted build selection; recover it before selecting another build.",
        ));
    }
    native_build::ordinary_ancestors(build)?;
    let build = build.canonicalize()?;
    if build.parent() != Some(state.join("builds").canonicalize()?.as_path()) {
        return Err(io::Error::other(
            "Only an immutable build published in this owned state can be selected.",
        ));
    }
    let check = build_identity::check(&build, None);
    if !check.runtime_allowed {
        return Err(io::Error::other(check.action));
    }
    let before = read_optional(&state.join(ACTIVE))?;
    let rollback_usable = before
        .as_ref()
        .map(|bytes| predecessor_rollback_usable(state, bytes))
        .transpose()?
        .unwrap_or(true);
    let after = serde_json::to_vec_pretty(&Pointer {
        schema: 1,
        build: build
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| io::Error::other("Non-Unicode build directory"))?
            .into(),
        record_sha256: build_identity::hash_file(&build.join("build.json"))?,
    })?;
    Ok(Journal {
        schema: 2,
        before,
        after,
        rollback_usable,
    })
}

pub fn activate(state: &Path, build: &Path) -> io::Result<Selection> {
    let _lock = native_build::lock_owned_state(state)?;
    let state = state.canonicalize()?;
    let journal = stage(&state, build)?;
    let build = pointer_path(&state, &journal.after)?;
    if journal.before.as_deref() == Some(journal.after.as_slice()) {
        return Ok(Selection {
            build: Some(build),
            changed: false,
        });
    }
    let receipt = serde_json::to_vec_pretty(&journal)?;
    replace_expected(&state.join(JOURNAL), None, Some(&receipt))?;
    replace_expected(
        &state.join(ACTIVE),
        journal.before.as_deref(),
        Some(&journal.after),
    )?;
    // Keep damaged starting-state evidence even after successful explicit repair.
    finish_journal(&state, &receipt)?;
    Ok(Selection {
        build: Some(build),
        changed: true,
    })
}

pub fn recover(state: &Path) -> io::Result<Selection> {
    let _lock = native_build::lock_owned_state(state)?;
    let state = state.canonicalize()?;
    let Some(receipt) = read_optional(&state.join(JOURNAL))? else {
        let build = read_optional(&state.join(ACTIVE))?
            .map(|bytes| verified_previous(&state, &bytes))
            .transpose()?;
        return Ok(Selection {
            build,
            changed: false,
        });
    };
    let journal: Journal = serde_json::from_slice(&receipt)?;
    if journal.schema != 2 {
        return Err(io::Error::other(
            "Incompatible recovery journal; preserving it.",
        ));
    }
    // Validate both references before permitting any mutation, including initial install.
    pointer_path(&state, &journal.after)?;
    if let Some(bytes) = &journal.before {
        pointer_reference(&state, bytes)?;
    }
    let active = read_optional(&state.join(ACTIVE))?;
    if active != journal.before && active.as_deref() != Some(journal.after.as_slice()) {
        return Err(io::Error::other(
            "Native selection ownership conflict; target and recovery journal preserved.",
        ));
    }
    let (desired, previous) = if journal.rollback_usable {
        (
            journal.before.as_deref(),
            journal
                .before
                .as_ref()
                .map(|bytes| verified_previous(&state, bytes))
                .transpose()?,
        )
    } else {
        // The old build was already broken before explicit repair began. Finish
        // the verified replacement; never restore it or strand a retry forever.
        (
            Some(journal.after.as_slice()),
            Some(verified_previous(&state, &journal.after)?),
        )
    };
    if active.as_deref() != desired {
        replace_expected(&state.join(ACTIVE), active.as_deref(), desired)?;
    }
    finish_journal(&state, &receipt)?;
    Ok(Selection {
        build: previous,
        changed: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_identity::{BINARIES, BuildRecord, SCHEMA};
    use std::collections::BTreeMap;

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        let source = temp.path().join("source");
        let schema = source.join(build_identity::INSPECTION_SCHEMA);
        fs::create_dir_all(schema.parent().unwrap()).unwrap();
        fs::write(schema, "{}").unwrap();
        fs::create_dir_all(source.join("crates/one/src")).unwrap();
        fs::create_dir_all(source.join("tools/rtk-adapter/src")).unwrap();
        for name in ["Cargo.toml", "Cargo.lock", "crates/one/src/lib.rs"] {
            fs::write(source.join(name), "fixture").unwrap();
        }
        fs::create_dir_all(state.join("builds")).unwrap();
        fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
        let a = state.join("builds/a");
        let b = state.join("builds/b");
        for build in [&a, &b] {
            fs::create_dir(build).unwrap();
            let mut binaries = BTreeMap::new();
            for name in BINARIES {
                fs::write(build.join(name), name).unwrap();
                binaries.insert(
                    name.to_string(),
                    build_identity::hash_bytes(name.as_bytes()),
                );
            }
            let record = BuildRecord {
                schema: SCHEMA,
                source_root: source.clone(),
                source: build_identity::source_identity(&source).unwrap(),
                rustc: "test".into(),
                cargo: "test".into(),
                target: "x86_64-pc-windows-msvc".into(),
                profile: "release".into(),
                binaries,
            };
            fs::write(
                build.join("build.json"),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        }
        (temp, state, a, b)
    }

    fn interrupt(state: &Path, candidate: &Path, replaced: bool) {
        let _lock = native_build::lock_owned_state(state).unwrap();
        let journal = stage(state, candidate).unwrap();
        replace_expected(
            &state.join(JOURNAL),
            None,
            Some(&serde_json::to_vec(&journal).unwrap()),
        )
        .unwrap();
        if replaced {
            replace_expected(
                &state.join(ACTIVE),
                journal.before.as_deref(),
                Some(&journal.after),
            )
            .unwrap();
        }
    }

    #[test]
    fn activation_reuse_stale_repair_and_immutable_artifacts() {
        let (_temp, state, a, b) = fixture();
        let held = fs::File::open(a.join("codex-harness.exe")).unwrap();
        assert!(activate(&state, &a).unwrap().changed);
        assert!(!activate(&state, &a).unwrap().changed);
        let record = build_identity::read_record(&a).unwrap();
        fs::write(record.source_root.join("crates/one/src/lib.rs"), "changed").unwrap();
        assert!(selected(&state).is_err());
        assert!(activate(&state, &b).is_err());
        assert!(recover(&state).unwrap().build.is_some());
        fs::write(record.source_root.join("crates/one/src/lib.rs"), "fixture").unwrap();
        assert!(activate(&state, &b).unwrap().changed);
        assert_eq!(selected(&state).unwrap(), b.canonicalize().unwrap());
        assert_eq!(
            held.metadata().unwrap().len(),
            "codex-harness.exe".len() as u64
        );
        // Explicit bootstrap can replace an altered manager; nothing overwrites it.
        fs::write(b.join("codex-harness.exe"), "altered").unwrap();
        assert!(activate(&state, &a).unwrap().changed);
        assert_eq!(
            fs::read_to_string(b.join("codex-harness.exe")).unwrap(),
            "altered"
        );
    }

    #[test]
    fn interruption_before_and_after_pointer_swap_recovers_idempotently() {
        for initial in [true, false] {
            for replaced in [false, true] {
                let (_temp, state, a, b) = fixture();
                if !initial {
                    activate(&state, &a).unwrap();
                }
                let original = read_optional(&state.join(ACTIVE)).unwrap();
                interrupt(&state, &b, replaced);
                assert!(selected(&state).is_err());
                assert!(activate(&state, &a).is_err());
                assert!(recover(&state).unwrap().changed);
                assert_eq!(read_optional(&state.join(ACTIVE)).unwrap(), original);
                assert!(!recover(&state).unwrap().changed);
                assert!(a.join("codex-harness.exe").exists());
                assert!(b.join("codex-harness.exe").exists());
            }
        }
    }

    #[test]
    fn conflicts_tampering_and_lock_contention_preserve_state() {
        let (_temp, state, a, b) = fixture();
        activate(&state, &a).unwrap();
        let lock = native_build::lock_owned_state(&state).unwrap();
        assert!(activate(&state, &b).is_err());
        drop(lock);
        interrupt(&state, &b, true);
        fs::write(state.join(ACTIVE), "foreign edit").unwrap();
        let journal = fs::read(state.join(JOURNAL)).unwrap();
        assert!(recover(&state).is_err());
        assert_eq!(fs::read(state.join(ACTIVE)).unwrap(), b"foreign edit");
        assert_eq!(fs::read(state.join(JOURNAL)).unwrap(), journal);
        assert!(
            pointer_path(
                &state,
                br#"{"schema":1,"build":"../foreign","record_sha256":""}"#
            )
            .is_err()
        );
        fs::write(state.join("owner"), "foreign owner").unwrap();
        assert!(recover(&state).is_err());
        assert!(activate(&state, &a).is_err());
    }

    #[test]
    fn failed_recovery_keeps_journal_when_previous_build_was_altered() {
        let (_temp, state, a, b) = fixture();
        activate(&state, &a).unwrap();
        interrupt(&state, &b, true);
        fs::write(a.join("codex-harness.exe"), "corrupt").unwrap();
        let active = fs::read(state.join(ACTIVE)).unwrap();
        assert!(recover(&state).is_err());
        assert_eq!(fs::read(state.join(ACTIVE)).unwrap(), active);
        assert!(state.join(JOURNAL).exists());
    }

    #[test]
    fn unknown_predecessor_metadata_preserves_selection_and_both_builds() {
        for unknown_field in ["schema", "top-level", "nested-source"] {
            let (_temp, state, a, b) = fixture();
            activate(&state, &a).unwrap();
            let mut pointer: Pointer =
                serde_json::from_slice(&fs::read(state.join(ACTIVE)).unwrap()).unwrap();
            let mut record: serde_json::Value =
                serde_json::from_slice(&fs::read(a.join("build.json")).unwrap()).unwrap();
            match unknown_field {
                "top-level" => record["future_contract"] = true.into(),
                "nested-source" => record["source"]["future_contract"] = true.into(),
                _ => record["schema"] = 2.into(),
            }
            let bytes = serde_json::to_vec(&record).unwrap();
            fs::write(a.join("build.json"), &bytes).unwrap();
            pointer.record_sha256 = build_identity::hash_bytes(&bytes);
            let pointer = serde_json::to_vec_pretty(&pointer).unwrap();
            fs::write(state.join(ACTIVE), &pointer).unwrap();
            let error = activate(&state, &b).unwrap_err();
            assert!(error.to_string().contains("Unknown predecessor"));
            assert_eq!(fs::read(state.join(ACTIVE)).unwrap(), pointer);
            assert_eq!(fs::read(a.join("build.json")).unwrap(), bytes);
            assert!(b.join("codex-harness.exe").is_file());
            assert!(!state.join(JOURNAL).exists());
        }
    }

    #[cfg(windows)]
    #[test]
    fn indeterminate_legacy_check_never_becomes_confirmed_damage() {
        use crate::process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason};
        use std::time::Duration;
        for mode in ["malformed", "timeout"] {
            let (temp, state, a, b) = fixture();
            let root = temp.keep();
            let source = root.join("predecessor.rs");
            fs::write(&source, include_str!("../tests/fixtures/predecessor.rs")).unwrap();
            let rustc = std::process::Command::new("where.exe")
                .arg("rustc.exe")
                .output()
                .unwrap();
            let rustc = String::from_utf8(rustc.stdout).unwrap();
            let mut command = CommandSpec::new(PathBuf::from(rustc.lines().next().unwrap()));
            command.args = vec![
                source.as_os_str().to_owned(),
                "--edition=2024".into(),
                "-o".into(),
                a.join("codex-harness.exe").into_os_string(),
            ];
            let log = fs::File::create(root.join("compile.log")).unwrap();
            command.stdout = Some(log.try_clone().unwrap());
            command.stderr = Some(log);
            let job = Job::new(Limits {
                memory_bytes: Some(512 * 1024 * 1024),
                cpu_percent: Some(50.0),
            })
            .unwrap();
            let child = job.spawn(&command).unwrap();
            let outcome = job
                .wait(
                    &child,
                    Deadline::after(Duration::from_secs(30)).unwrap(),
                    &Cancellation::default(),
                    Duration::from_secs(1),
                )
                .unwrap();
            assert_eq!((outcome.reason, outcome.exit_code), (StopReason::Exited, 0));
            fs::write(a.join("failure-mode.txt"), mode).unwrap();
            let mut record = build_identity::read_record(&a).unwrap();
            record.binaries.remove("harness-observe.exe");
            record.binaries.insert(
                "codex-harness.exe".into(),
                build_identity::hash_file(&a.join("codex-harness.exe")).unwrap(),
            );
            fs::write(
                a.join("build.json"),
                serde_json::to_vec_pretty(&record).unwrap(),
            )
            .unwrap();
            let pointer = serde_json::to_vec_pretty(&Pointer {
                schema: 1,
                build: "a".into(),
                record_sha256: build_identity::hash_file(&a.join("build.json")).unwrap(),
            })
            .unwrap();
            fs::write(state.join(ACTIVE), &pointer).unwrap();
            assert!(build_identity::verify_record_integrity(&a).is_ok());
            let result = activate(&state, &b);
            let observed = serde_json::json!({"mode":mode, "activation_accepted":result.is_ok(), "error":result.as_ref().err().map(ToString::to_string), "pointer_preserved":fs::read(state.join(ACTIVE)).unwrap()==pointer, "journal_present":state.join(JOURNAL).exists()});
            fs::write(
                root.join("predecessor-result.json"),
                serde_json::to_vec_pretty(&observed).unwrap(),
            )
            .unwrap();
            assert!(
                result.is_err(),
                "indeterminate predecessor accepted; evidence {}",
                root.display()
            );
            assert_eq!(fs::read(state.join(ACTIVE)).unwrap(), pointer);
            assert!(!state.join(JOURNAL).exists());
            assert!(a.join("codex-harness.exe").is_file());
            assert!(b.join("codex-harness.exe").is_file());
            println!("predecessor refusal evidence {}", root.display());
        }
    }

    #[test]
    fn interrupted_repair_of_already_damaged_build_finishes_verified_replacement() {
        for damage_metadata in [false, true] {
            for replaced in [false, true] {
                let (_temp, state, a, b) = fixture();
                activate(&state, &a).unwrap();
                let original = fs::read(state.join(ACTIVE)).unwrap();
                if damage_metadata {
                    fs::remove_file(a.join("build.json")).unwrap();
                } else {
                    fs::write(a.join("codex-harness.exe"), "corrupt before repair").unwrap();
                }
                interrupt(&state, &b, replaced);
                assert!(recover(&state).unwrap().changed);
                assert_eq!(selected(&state).unwrap(), b.canonicalize().unwrap());
                assert!(!recover(&state).unwrap().changed);
                let history: Vec<Journal> = fs::read_dir(state.join("build-selection-history"))
                    .unwrap()
                    .map(|file| {
                        serde_json::from_slice(&fs::read(file.unwrap().path()).unwrap()).unwrap()
                    })
                    .collect();
                assert!(
                    history
                        .iter()
                        .any(|entry| entry.before.as_deref() == Some(original.as_slice())
                            && !entry.rollback_usable)
                );
            }
        }
    }
}
