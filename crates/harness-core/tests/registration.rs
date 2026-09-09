//! Direct source file/directory links with an owned exact-content journal.
#![cfg(windows)]

use harness_core::{
    config_create::ConfigCreation,
    config_file::ConfigSnapshot,
    inventory::{Connection, Link},
    registration::{COMPLETION, Journal, LinkChange, Outcome, Registration, SCHEMA},
};
use std::{
    fs, io,
    os::windows::fs::{symlink_dir, symlink_file},
    path::PathBuf,
};

struct Fixture {
    source: PathBuf,
    home: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = tempfile::Builder::new()
            .prefix(&format!("harness-rust-registration-{name}-"))
            .tempdir()
            .unwrap()
            .keep();
        println!("registration evidence: {}", root.display());
        let source = root.join("source");
        let home = root.join("home");
        let state = root.join("state");
        fs::create_dir_all(source.join("dir")).unwrap();
        fs::write(source.join("file.txt"), "source-file").unwrap();
        fs::write(source.join("dir/keep.txt"), "source-dir").unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("unrelated.txt"), "keep-me").unwrap();
        Self {
            source,
            home,
            state,
        }
    }

    fn file_dest(&self) -> PathBuf {
        self.home.join("AGENTS.md")
    }

    fn dir_dest(&self) -> PathBuf {
        self.home.join("skills/one")
    }

    fn file_link(&self) -> Link {
        item(
            "file",
            "instructions",
            self.source.join("file.txt"),
            self.file_dest(),
        )
    }

    fn dir_link(&self) -> Link {
        item("skill", "one", self.source.join("dir"), self.dir_dest())
    }

    fn open(&self) -> Registration {
        Registration::open(&self.state).unwrap()
    }
}

fn item(kind: &str, name: &str, source: PathBuf, destination: PathBuf) -> Link {
    Link {
        kind: kind.into(),
        name: name.into(),
        source,
        destination,
        connection: Connection::Missing,
    }
}

fn contains(err: io::Error, needle: &str) {
    let msg = err.to_string();
    assert!(msg.contains(needle), "{msg}");
}

fn journal(reg: &Registration) -> Journal {
    serde_json::from_slice(&fs::read(reg.journal_path()).unwrap()).unwrap()
}

#[test]
#[ignore = "requires an existing NTFS 8.3 path alias; explicit alias acceptance"]
fn aliased_configuration_paths_cannot_enter_a_journal() {
    let f = Fixture::new("config-8dot3");
    let config = f.home.join("configuration-original.toml");
    fs::write(&config, b"original").unwrap();
    let alias = short_alias(&config);
    let first = ConfigSnapshot::read(&config)
        .unwrap()
        .plan_replace(b"first candidate")
        .unwrap();
    let second = ConfigSnapshot::read(&alias)
        .unwrap()
        .plan_replace(b"second candidate")
        .unwrap();
    let reg = f.open();
    contains(
        reg.apply_with_configs(&[f.file_link()], &[first, second])
            .unwrap_err(),
        "duplicate configuration object identity",
    );
    assert_eq!(fs::read(&config).unwrap(), b"original");
    assert!(!f.file_dest().exists());
    assert!(!reg.journal_path().exists());
    assert!(reg.recover().unwrap().restored.is_empty());
}

fn short_alias(path: &std::path::Path) -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let name: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut short = vec![0u16; 32768];
    let size = unsafe { GetShortPathNameW(name.as_ptr(), short.as_mut_ptr(), short.len() as u32) };
    assert!(
        size > 0 && (size as usize) < short.len(),
        "8.3 oracle failed: {}",
        io::Error::last_os_error()
    );
    let alias = PathBuf::from(OsString::from_wide(&short[..size as usize]));
    assert_ne!(path, alias, "fixture requires an existing NTFS 8.3 alias");
    assert_eq!(
        fs::canonicalize(path).unwrap(),
        fs::canonicalize(&alias).unwrap()
    );
    alias
}

#[test]
#[ignore = "requires an existing NTFS 8.3 parent alias; explicit alias acceptance"]
fn aliased_link_changes_refuse_overlap_and_preserve_requested_paths() {
    let f = Fixture::new("change-8dot3");
    let old = f.source.join("file.txt");
    let next = f.source.join("next.txt");
    fs::write(&next, b"next").unwrap();
    symlink_file(&old, f.file_dest()).unwrap();
    let alias = short_alias(&f.home).join("AGENTS.md");
    let reg = f.open();
    assert!(
        reg.apply_with_changes(
            &[],
            &[],
            &[],
            &[
                LinkChange::replace(&f.file_dest(), &old, &next).unwrap(),
                LinkChange::remove(&alias, &old).unwrap(),
            ]
        )
        .is_err()
    );
    assert!(!reg.journal_path().exists());
    assert_eq!(fs::read_link(f.file_dest()).unwrap(), old);
    let state_link = f.state.join("owned-link");
    symlink_file(&old, &state_link).unwrap();
    let state_alias = short_alias(&f.state).join("owned-link");
    assert!(
        reg.apply_with_changes(
            &[],
            &[],
            &[],
            &[LinkChange::remove(&state_alias, &old).unwrap()]
        )
        .is_err()
    );
    assert!(!reg.journal_path().exists());
    assert_eq!(fs::read_link(&state_link).unwrap(), old);
    let result = reg
        .apply_with_changes(
            &[],
            &[],
            &[],
            &[LinkChange::replace(&alias, &old, &next).unwrap()],
        )
        .unwrap();
    assert_eq!(result.changed_links, vec![alias.clone()]);
    assert_eq!(fs::read_link(f.file_dest()).unwrap(), next);
    assert_eq!(reg.disconnect().unwrap().restored, vec![alias]);
    assert_eq!(fs::read_link(f.file_dest()).unwrap(), old);
    assert_eq!(fs::read(&old).unwrap(), b"source-file");
    assert_eq!(fs::read(&next).unwrap(), b"next");
}

#[test]
fn immediate_reconnection_preserves_file_and_directory_link_identity() {
    let f = Fixture::new("immediate-reconnect");
    let reg = f.open();
    for link in [f.file_link(), f.dir_link()] {
        for _ in 0..3 {
            reg.apply(std::slice::from_ref(&link)).unwrap();
            assert_eq!(fs::read_link(&link.destination).unwrap(), link.source);
            reg.disconnect().unwrap();
            assert!(fs::symlink_metadata(&link.destination).is_err());
            assert!(!reg.journal_path().exists());
            assert_eq!(fs::read(f.source.join("file.txt")).unwrap(), b"source-file");
            assert_eq!(
                fs::read(f.source.join("dir/keep.txt")).unwrap(),
                b"source-dir"
            );
        }
    }
}

#[test]
#[ignore = "requires an existing NTFS 8.3 parent alias; explicit alias acceptance"]
fn aliased_created_configs_conflicting_with_files_links_or_state_are_refused() {
    let f = Fixture::new("create-8dot3");
    let short = short_alias(&f.home);
    let reg = f.open();
    let state_alias = short_alias(&f.state);
    for other in [
        short.join("new.toml"),
        short.join("new.toml/child"),
        state_alias.join("journal.json"),
    ] {
        assert!(
            reg.apply_with_files(
                &[],
                &[],
                &[
                    ConfigCreation::new(&f.home.join("new.toml"), b"first").unwrap(),
                    ConfigCreation::new(&other, b"second").unwrap(),
                ]
            )
            .is_err()
        );
        assert!(!reg.journal_path().exists());
        assert!(!f.home.join("new.toml").exists());
    }
    assert!(
        reg.apply_with_files(
            &[f.file_link()],
            &[],
            &[ConfigCreation::new(&short.join("AGENTS.md"), b"conflict").unwrap()]
        )
        .is_err()
    );
    assert!(!reg.journal_path().exists());
    assert!(!f.file_dest().exists());
    reg.apply_with_files(
        &[f.file_link()],
        &[],
        &[ConfigCreation::new(&short.join("new.toml"), b"distinct").unwrap()],
    )
    .unwrap();
    assert_eq!(fs::read(f.home.join("new.toml")).unwrap(), b"distinct");
    reg.disconnect().unwrap();
    assert!(!f.home.join("new.toml").exists());
    assert!(f.source.join("file.txt").exists());
}

#[test]
#[ignore = "requires an existing NTFS 8.3 parent alias; explicit alias acceptance"]
fn aliased_new_link_destinations_cannot_enter_a_journal() {
    for (first, second, ancestor) in [
        ("missing", "missing", false),
        ("absent-parent/missing", "absent-parent/missing", false),
        ("missing", "missing/child", true),
        ("absent-parent/missing", "absent-parent/missing/child", true),
    ] {
        for reverse in [false, true] {
            let f = Fixture::new("link-8dot3-overlap");
            let alias = short_alias(&f.home);
            let mut links = vec![
                item(
                    "test",
                    "first",
                    f.source.join(if ancestor { "dir" } else { "file.txt" }),
                    f.home.join(first),
                ),
                item(
                    "test",
                    "second",
                    f.source.join("file.txt"),
                    alias.join(second),
                ),
            ];
            if reverse {
                links.reverse();
            }
            let reg = f.open();
            contains(
                reg.apply(&links).unwrap_err(),
                "overlapping registration destinations",
            );
            assert!(!reg.journal_path().exists());
            assert!(!reg.state().join(COMPLETION).exists());
            assert_eq!(fs::read_dir(&f.home).unwrap().count(), 1);
            assert_eq!(fs::read(f.home.join("unrelated.txt")).unwrap(), b"keep-me");
            assert_eq!(fs::read(f.source.join("file.txt")).unwrap(), b"source-file");
            assert_eq!(
                fs::read(f.source.join("dir/keep.txt")).unwrap(),
                b"source-dir"
            );
            let recovery = reg.recover().unwrap();
            assert!(recovery.removed.is_empty() && recovery.restored.is_empty());
        }
    }
}

#[test]
#[ignore = "requires an existing NTFS 8.3 state alias; explicit state-conflict acceptance"]
fn aliased_recovery_state_cannot_be_a_registration_destination() {
    let f = Fixture::new("state-8dot3-overlap");
    let reg = f.open();
    let state_alias = short_alias(&f.state);
    let link = item(
        "file",
        "conflicting-state",
        f.source.join("file.txt"),
        state_alias.join("journal.json"),
    );
    let error = reg.apply(&[link]).unwrap_err();
    println!("state alias rejection: {error}");
    assert!(
        !reg.journal_path().exists(),
        "state alias produced recovery intent"
    );
    assert_eq!(fs::read_dir(&f.state).unwrap().count(), 2);
    assert!(reg.recover().unwrap().removed.is_empty());
    assert_eq!(fs::read(f.source.join("file.txt")).unwrap(), b"source-file");
}

#[test]
#[ignore = "requires existing NTFS 8.3 aliases; explicit namespace acceptance"]
fn distinct_aliased_destinations_keep_requested_source_state_and_paths() {
    let f = Fixture::new("link-8dot3-distinct");
    fs::create_dir(&f.state).unwrap();
    let state = short_alias(&f.state);
    let home = short_alias(&f.home);
    let source = short_alias(&f.source).join("file.txt");
    let links = vec![
        item("file", "first", source.clone(), home.join("first")),
        item(
            "file",
            "second",
            f.source.join("file.txt"),
            f.home.join("absent/second"),
        ),
    ];
    let reg = Registration::open(&state).unwrap();
    assert_eq!(reg.state(), state);
    reg.apply(&links).unwrap();
    let recorded = journal(&reg);
    for (record, requested) in recorded.records.iter().zip(&links) {
        assert_eq!(record.path, requested.destination);
        assert_eq!(record.target, requested.source);
        assert_eq!(fs::read_link(&record.path).unwrap(), requested.source);
    }
    reg.disconnect().unwrap();
    assert_eq!(fs::read(f.home.join("unrelated.txt")).unwrap(), b"keep-me");
    assert_eq!(fs::read(source).unwrap(), b"source-file");
    assert!(!reg.journal_path().exists());
}

#[test]
fn links_and_configuration_share_completion_repeat_and_guarded_disconnect() {
    let f = Fixture::new("config-combined");
    let config = f.home.join("config.toml");
    fs::write(&config, b"original").unwrap();
    let baseline = ConfigSnapshot::read(&config).unwrap();
    let change = baseline.plan_replace(b"candidate").unwrap();
    let reg = f.open();
    let report = reg.apply_with_configs(&[f.file_link()], &[change]).unwrap();
    assert_eq!(report.configurations, vec![config.clone()]);
    assert_eq!(fs::read(&config).unwrap(), b"candidate");
    assert!(f.file_dest().is_symlink());
    let journal_before = fs::read(reg.journal_path()).unwrap();
    let repeated = ConfigSnapshot::read(&config)
        .unwrap()
        .plan_replace(b"candidate")
        .unwrap();
    reg.apply_with_configs(&[f.file_link()], &[repeated])
        .unwrap();
    assert_eq!(fs::read(reg.journal_path()).unwrap(), journal_before);
    assert!(reg.apply(&[f.file_link()]).is_err());
    let undo = reg.disconnect().unwrap();
    assert_eq!(undo.restored, vec![config.clone()]);
    assert_eq!(fs::read(&config).unwrap(), b"original");
    assert!(!f.file_dest().exists());
    assert!(reg.disconnect().unwrap().restored.is_empty());
}

#[test]
fn changed_configuration_preserves_all_links_and_the_bound_journal() {
    for replace_object in [false, true] {
        let f = Fixture::new("config-foreign");
        let config = f.home.join("config.toml");
        fs::write(&config, b"original").unwrap();
        let change = ConfigSnapshot::read(&config)
            .unwrap()
            .plan_replace(b"candidate")
            .unwrap();
        let reg = f.open();
        reg.apply_with_configs(&[f.file_link()], &[change]).unwrap();
        let journal_before = fs::read(reg.journal_path()).unwrap();
        if replace_object {
            fs::rename(&config, f.home.join("retained-original")).unwrap();
            fs::write(&config, b"candidate").unwrap();
        } else {
            fs::write(&config, b"foreign").unwrap();
        }
        let current = fs::read(&config).unwrap();
        assert!(reg.disconnect().is_err());
        assert_eq!(fs::read(&config).unwrap(), current);
        assert_eq!(fs::read(reg.journal_path()).unwrap(), journal_before);
        assert!(f.file_dest().is_symlink());
    }
}

#[test]
fn invalid_configuration_plans_cannot_create_links_or_intent() {
    let f = Fixture::new("config-preflight");
    let config = f.home.join("config.toml");
    fs::write(&config, b"original").unwrap();
    let baseline = ConfigSnapshot::read(&config).unwrap();
    let reg = f.open();
    for changes in [
        vec![
            baseline.plan_replace(b"one").unwrap(),
            baseline.plan_replace(b"two").unwrap(),
        ],
        vec![
            ConfigSnapshot::read(&f.state.join("owner"))
                .unwrap()
                .plan_replace(b"changed owner")
                .unwrap(),
        ],
    ] {
        assert!(reg.apply_with_configs(&[f.file_link()], &changes).is_err());
        assert!(!reg.journal_path().exists());
        assert!(!f.file_dest().exists());
        assert_eq!(fs::read(&config).unwrap(), b"original");
    }
    fs::write(&config, b"foreign").unwrap();
    assert!(
        reg.apply_with_configs(
            &[f.file_link()],
            &[baseline.plan_replace(b"candidate").unwrap()]
        )
        .is_err()
    );
    assert!(!reg.journal_path().exists());
    assert!(!f.file_dest().exists());
    assert_eq!(fs::read(&config).unwrap(), b"foreign");
}

#[test]
fn altered_configuration_intent_is_preserved_without_authorizing_rollback() {
    let f = Fixture::new("config-intent-tamper");
    let config = f.home.join("config.toml");
    fs::write(&config, b"original").unwrap();
    let change = ConfigSnapshot::read(&config)
        .unwrap()
        .plan_replace(b"candidate")
        .unwrap();
    let reg = f.open();
    reg.apply_with_configs(&[f.file_link()], &[change]).unwrap();
    let original_journal = fs::read(reg.journal_path()).unwrap();
    let mut altered: serde_json::Value = serde_json::from_slice(&original_journal).unwrap();
    altered["configurations"][0]["before"] = serde_json::json!([70, 79, 82, 69, 73, 71, 78]);
    fs::write(reg.journal_path(), serde_json::to_vec(&altered).unwrap()).unwrap();
    assert!(reg.disconnect().is_err());
    assert_eq!(fs::read(&config).unwrap(), b"candidate");
    assert!(f.file_dest().is_symlink());
    // A previous native schema is explicitly preserved, never interpreted as
    // a new config-aware intent or silently upgraded during destructive work.
    altered = serde_json::from_slice(&original_journal).unwrap();
    altered["schema"] = serde_json::json!(3);
    altered.as_object_mut().unwrap().remove("configurations");
    let legacy = serde_json::to_vec(&altered).unwrap();
    fs::write(reg.journal_path(), &legacy).unwrap();
    assert!(reg.disconnect().is_err());
    assert_eq!(fs::read(reg.journal_path()).unwrap(), legacy);
    assert_eq!(fs::read(&config).unwrap(), b"candidate");
}

#[test]
fn new_and_repeat_file_and_directory_links_disconnect_without_touching_source() {
    let f = Fixture::new("new-repeat");
    let reg = f.open();
    let first = reg.apply(&[f.file_link(), f.dir_link()]).unwrap();
    assert_eq!(first.links.len(), 2);
    assert!(first.links.iter().all(|l| l.outcome == Outcome::Created));
    assert_eq!(fs::read_to_string(f.file_dest()).unwrap(), "source-file");
    assert_eq!(
        fs::read_to_string(f.dir_dest().join("keep.txt")).unwrap(),
        "source-dir"
    );
    let recorded = journal(&reg);
    assert_eq!(recorded.schema, SCHEMA);
    assert!(reg.state().join(COMPLETION).is_file());
    assert!(recorded.records.iter().all(|r| r.created));
    assert!(
        recorded
            .records
            .iter()
            .all(|r| r.checksum.chars().all(|c| c.is_ascii_hexdigit()))
    );

    let second = reg.apply(&[f.file_link(), f.dir_link()]).unwrap();
    assert!(second.links.iter().all(|l| l.outcome == Outcome::Reused));
    drop(reg);

    let reg = f.open();
    let undone = reg.disconnect().unwrap();
    assert_eq!(undone.removed.len(), 2);
    assert!(!f.file_dest().exists());
    assert!(!f.dir_dest().exists());
    assert!(!reg.journal_path().exists());
    assert_eq!(
        fs::read_to_string(f.source.join("file.txt")).unwrap(),
        "source-file"
    );
    assert_eq!(
        fs::read_to_string(f.source.join("dir/keep.txt")).unwrap(),
        "source-dir"
    );
    assert_eq!(
        fs::read_to_string(f.home.join("unrelated.txt")).unwrap(),
        "keep-me"
    );
}

#[test]
fn matching_preexisting_symlink_is_reused_and_kept_on_disconnect() {
    let f = Fixture::new("preexisting");
    symlink_file(f.source.join("file.txt"), f.file_dest()).unwrap();
    let reg = f.open();
    let report = reg.apply(&[f.file_link()]).unwrap();
    assert_eq!(report.links[0].outcome, Outcome::Reused);
    assert!(!journal(&reg).records[0].created);
    reg.disconnect().unwrap();
    assert!(f.file_dest().exists());
    assert_eq!(
        fs::read_link(f.file_dest()).unwrap(),
        f.source.join("file.txt")
    );
}

#[test]
fn changed_creation_flag_cannot_claim_a_preexisting_link() {
    let fixture = Fixture::new("ownership-flag");
    symlink_file(fixture.source.join("file.txt"), fixture.file_dest()).unwrap();
    let reg = fixture.open();
    reg.apply(&[fixture.file_link()]).unwrap();
    let mut record: serde_json::Value =
        serde_json::from_slice(&fs::read(reg.journal_path()).unwrap()).unwrap();
    record["records"][0]["created"] = true.into();
    let changed = serde_json::to_vec_pretty(&record).unwrap();
    fs::write(reg.journal_path(), &changed).unwrap();
    let result = reg.disconnect();
    assert!(
        result.is_err(),
        "altered creation claim was accepted; evidence {}",
        fixture.state.display()
    );
    assert!(
        fs::symlink_metadata(fixture.file_dest())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(reg.journal_path()).unwrap(), changed);
}

#[test]
fn matching_foreign_link_created_after_intent_is_not_owned_by_recovery() {
    use std::time::{Duration, Instant};
    let fixture = Fixture::new("foreign-create-race");
    let reg = fixture.open();
    let mut links = Vec::new();
    for index in 0..256 {
        links.push(item(
            "file",
            "race",
            fixture.source.join("file.txt"),
            fixture.home.join(format!("link-{index:03}")),
        ));
    }
    let last = links.last().unwrap().destination.clone();
    let foreign = last.clone();
    let source = fixture.source.join("file.txt");
    let intent = reg.journal_path();
    let actor = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !intent.exists() {
            assert!(
                Instant::now() < deadline,
                "intent publication was not observed"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        symlink_file(source, foreign)
    });
    let applied = reg.apply(&links);
    actor
        .join()
        .unwrap()
        .expect("owned test must establish the foreign creation race");
    assert!(applied.is_err());
    assert!(
        fs::symlink_metadata(&last)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let recovered = reg.recover();
    assert!(
        recovered.is_err(),
        "recovery claimed a foreign link from intent alone; evidence {}",
        fixture.state.display()
    );
    assert!(
        fs::symlink_metadata(&last)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(reg.journal_path().is_file());
    // Removing only the owned test actor's conflicting link allows the exact
    // recorded staged/published objects to be recovered on a second attempt.
    fs::remove_file(&last).unwrap();
    let recovered = reg.recover().unwrap();
    assert!(recovered.removed.len() < links.len());
    assert!(!reg.journal_path().exists());
    for link in &links {
        assert!(fs::symlink_metadata(&link.destination).is_err());
    }
    assert!(fs::read_dir(&fixture.home).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".codex-harness-link-")
    }));
}

#[test]
fn foreign_regular_dangling_and_wrong_target_links_are_rejected() {
    let f = Fixture::new("foreign");
    fs::write(f.file_dest(), "foreign-bytes").unwrap();
    let reg = f.open();
    contains(
        reg.apply(&[f.file_link()]).unwrap_err(),
        "Foreign destination exists",
    );
    assert_eq!(fs::read_to_string(f.file_dest()).unwrap(), "foreign-bytes");
    assert!(!reg.journal_path().exists());
    drop(reg);
    fs::remove_file(f.file_dest()).unwrap();

    symlink_file(f.root_join_absent(), f.file_dest()).unwrap();
    let reg = f.open();
    contains(
        reg.apply(&[f.file_link()]).unwrap_err(),
        "Foreign destination exists",
    );
    assert!(
        fs::symlink_metadata(f.file_dest())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    drop(reg);
    fs::remove_file(f.file_dest()).unwrap();

    let other = f.source.join("other.txt");
    fs::write(&other, "other").unwrap();
    symlink_file(&other, f.file_dest()).unwrap();
    let reg = f.open();
    contains(
        reg.apply(&[f.file_link()]).unwrap_err(),
        "Foreign destination exists",
    );
    assert_eq!(fs::read_link(f.file_dest()).unwrap(), other);
}

impl Fixture {
    fn root_join_absent(&self) -> PathBuf {
        self.source.parent().unwrap().join("absent.txt")
    }
}

#[test]
fn preparation_failure_rolls_back_unpublished_links() {
    let f = Fixture::new("interrupt");
    fs::write(f.home.join("blocked"), "not-a-directory").unwrap();
    let blocked = item(
        "skill",
        "blocked",
        self_source_dir(&f),
        f.home.join("blocked/one"),
    );
    let reg = f.open();
    contains(
        reg.apply(&[f.file_link(), blocked]).unwrap_err(),
        "not an ordinary directory",
    );
    assert!(!f.file_dest().exists());
    assert!(!reg.state().join(COMPLETION).exists());
    let undone = reg.recover().unwrap();
    assert!(undone.removed.is_empty());
    assert!(!f.file_dest().exists());
    assert!(!reg.journal_path().exists());
    assert_eq!(
        fs::read_to_string(f.home.join("blocked")).unwrap(),
        "not-a-directory"
    );
    assert_eq!(
        fs::read_to_string(f.source.join("file.txt")).unwrap(),
        "source-file"
    );
    assert!(fs::read_dir(&f.home).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".codex-harness-link-")
    }));
}

fn self_source_dir(f: &Fixture) -> PathBuf {
    f.source.join("dir")
}

#[test]
fn external_alteration_preserves_destination_and_journal() {
    let f = Fixture::new("altered");
    let reg = f.open();
    reg.apply(&[f.file_link()]).unwrap();
    fs::remove_file(f.file_dest()).unwrap();
    fs::write(f.file_dest(), "external replacement").unwrap();
    contains(
        reg.disconnect().unwrap_err(),
        "destination ownership changed",
    );
    assert_eq!(
        fs::read_to_string(f.file_dest()).unwrap(),
        "external replacement"
    );
    assert!(reg.journal_path().exists());
    assert_eq!(
        fs::read_to_string(f.home.join("unrelated.txt")).unwrap(),
        "keep-me"
    );
}

#[test]
fn lock_contention_serializes_apply_and_recovery() {
    let f = Fixture::new("lock");
    let held = f.open();
    contains(
        Registration::open(&f.state).unwrap_err(),
        "Another native registration operation owns this state",
    );
    drop(held);
    let reg = f.open();
    reg.apply(&[f.file_link()]).unwrap();
}

#[test]
fn same_target_replacement_is_not_reused_or_deleted() {
    let f = Fixture::new("same-target-replacement");
    let reg = f.open();
    reg.apply(&[f.file_link()]).unwrap();
    let original = f.home.join("saved-owned-link");
    fs::rename(f.file_dest(), &original).unwrap();
    symlink_file(f.source.join("file.txt"), f.file_dest()).unwrap();
    let before = fs::read(reg.journal_path()).unwrap();
    assert!(reg.apply(&[f.file_link()]).is_err());
    assert!(reg.disconnect().is_err());
    assert_eq!(fs::read(reg.journal_path()).unwrap(), before);
    assert!(
        fs::symlink_metadata(f.file_dest())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::remove_file(f.file_dest()).unwrap();
    fs::rename(original, f.file_dest()).unwrap();
    reg.disconnect().unwrap();
    assert!(!f.file_dest().exists());
}

#[test]
fn disconnect_removes_owned_dangling_file_and_directory_links() {
    let f = Fixture::new("owned-dangling");
    let reg = f.open();
    reg.apply(&[f.file_link(), f.dir_link()]).unwrap();
    fs::remove_file(f.source.join("file.txt")).unwrap();
    fs::remove_file(f.source.join("dir/keep.txt")).unwrap();
    fs::remove_dir(f.source.join("dir")).unwrap();
    assert_eq!(reg.disconnect().unwrap().removed.len(), 2);
    assert!(fs::symlink_metadata(f.file_dest()).is_err());
    assert!(fs::symlink_metadata(f.dir_dest()).is_err());
    assert!(f.source.is_dir());
}

#[test]
fn malformed_oversized_or_shared_metadata_cannot_authorize_any_removal() {
    for case in [
        "unknown-journal",
        "changed-completion",
        "oversized",
        "shared-journal",
    ] {
        let f = Fixture::new(case);
        let reg = f.open();
        reg.apply(&[f.file_link(), f.dir_link()]).unwrap();
        let path = match case {
            "changed-completion" => reg.state().join(COMPLETION),
            _ => reg.journal_path(),
        };
        let original = fs::read(&path).unwrap();
        match case {
            "oversized" => fs::write(&path, vec![b' '; 4 * 1024 * 1024 + 1]).unwrap(),
            "shared-journal" => fs::hard_link(&path, f.home.join("foreign-hardlink")).unwrap(),
            _ => {
                let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
                value["unknown"] = true.into();
                fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            }
        }
        let changed = fs::read(&path).unwrap();
        assert!(reg.disconnect().is_err(), "accepted case {case}");
        assert_eq!(fs::read(&path).unwrap(), changed);
        assert!(f.file_dest().exists());
        assert!(f.dir_dest().join("keep.txt").exists());
        if case == "shared-journal" {
            assert_eq!(fs::read(f.home.join("foreign-hardlink")).unwrap(), original);
            fs::remove_file(f.home.join("foreign-hardlink")).unwrap();
        } else {
            fs::write(&path, original).unwrap();
        }
        reg.disconnect().unwrap();
    }
}

#[test]
fn orphan_completion_and_state_overlap_are_preserved() {
    let f = Fixture::new("state-conflict");
    let reg = f.open();
    let mut link = f.file_link();
    link.destination = reg.journal_path();
    assert!(reg.apply(&[link]).is_err());
    let marker = reg.state().join(COMPLETION);
    fs::write(&marker, "unknown completion").unwrap();
    assert!(reg.recover().is_err());
    assert!(reg.apply(&[f.file_link()]).is_err());
    assert_eq!(fs::read_to_string(marker).unwrap(), "unknown completion");
    assert!(!f.file_dest().exists());
}

#[test]
fn ancestor_destinations_are_rejected_before_any_staging() {
    for (ancestor, nested) in [
        ("foo", "foo/bar"),
        ("FOO", "foo/bar"),
        ("ДОМ", "дом/child"),
        ("foo", "./foo/bar"),
        ("foo", "foo//bar"),
    ] {
        for reverse in [false, true] {
            let f = Fixture::new("overlapping-paths");
            let reg = f.open();
            let mut links = vec![
                item(
                    "skill",
                    "ancestor",
                    f.source.join("dir"),
                    f.home.join(ancestor),
                ),
                item(
                    "file",
                    "nested",
                    f.source.join("file.txt"),
                    f.home.join(nested),
                ),
            ];
            if reverse {
                links.reverse();
            }
            contains(
                reg.apply(&links).unwrap_err(),
                "overlapping registration destinations",
            );
            assert!(!reg.journal_path().exists());
            assert_eq!(fs::read_dir(&f.home).unwrap().count(), 1);
        }
    }
}

#[test]
fn state_keeps_the_requested_namespace_and_refuses_redirect_to_another_installation() {
    let a = Fixture::new("state-namespace-a");
    let b = Fixture::new("state-namespace-b");
    let reg_a = a.open();
    let reg_b = b.open();
    assert_eq!(reg_a.state(), a.state);
    reg_a.apply(&[a.file_link()]).unwrap();
    reg_b.apply(&[b.file_link()]).unwrap();
    let b_journal = fs::read(reg_b.journal_path()).unwrap();
    let saved = a.state.with_file_name("saved-state");
    // The acquired lock already prevents a later state-directory move. The
    // resolution bug was before that lock; also exercise the next open against
    // a redirected requested namespace rather than a canonicalized alias.
    assert!(fs::rename(&a.state, &saved).is_err());
    drop(reg_a);
    fs::rename(&a.state, &saved).unwrap();
    symlink_dir(&b.state, &a.state).unwrap();
    assert!(Registration::open(&a.state).is_err());
    assert!(a.file_dest().exists());
    assert!(b.file_dest().exists());
    assert_eq!(fs::read(reg_b.journal_path()).unwrap(), b_journal);
    fs::remove_dir(&a.state).unwrap();
    fs::rename(&saved, &a.state).unwrap();
    a.open().disconnect().unwrap();
    reg_b.disconnect().unwrap();
}

#[test]
fn changed_state_owner_refuses_apply_and_removal() {
    let f = Fixture::new("changed-owner");
    let reg = f.open();
    let owner = reg.state().join("owner");
    let original = fs::read(&owner).unwrap();
    fs::write(&owner, "foreign owner").unwrap();
    assert!(reg.apply(&[f.file_link()]).is_err());
    assert!(!f.file_dest().exists());
    fs::write(&owner, &original).unwrap();
    reg.apply(&[f.file_link()]).unwrap();
    fs::write(&owner, "foreign owner").unwrap();
    assert!(reg.disconnect().is_err());
    assert!(reg.recover().is_err());
    assert!(f.file_dest().exists());
    assert_eq!(fs::read_to_string(&owner).unwrap(), "foreign owner");
    fs::write(&owner, original).unwrap();
    reg.disconnect().unwrap();
}

#[test]
#[ignore = "invoked only by interrupted_registration_process_is_recoverable"]
fn registration_process_fixture() {
    let root = PathBuf::from(std::env::var_os("HARNESS_REGISTRATION_PROCESS_ROOT").unwrap());
    let reg = Registration::open(&root.join("state")).unwrap();
    let links = (0..512)
        .map(|index| {
            item(
                "file",
                "process",
                root.join("source/file.txt"),
                root.join(format!("home/link-{index:03}")),
            )
        })
        .collect::<Vec<_>>();
    reg.apply(&links).unwrap();
}

#[test]
fn interrupted_registration_process_is_recoverable() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    for phase in ["journal", "first-published"] {
        let f = Fixture::new(&format!("process-interrupt-{phase}"));
        let root = f.source.parent().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "registration_process_fixture",
                "--nocapture",
            ])
            .env("HARNESS_REGISTRATION_PROCESS_ROOT", root)
            .stdout(fs::File::create(root.join("child.stdout.txt")).unwrap())
            .stderr(fs::File::create(root.join("child.stderr.txt")).unwrap())
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let intent = f.state.join("journal.json");
        let mut observed = false;
        while Instant::now() < deadline {
            if intent.exists() && (phase == "journal" || f.home.join("link-000").exists()) {
                observed = true;
                break;
            }
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // This Child handle identifies only this owned Rust fixture; it launches no
        // descendants. Always reap it, including a setup/deadline failure.
        let _ = child.kill();
        let exit = child.wait().unwrap();
        assert!(
            observed,
            "owned fixture never published intent; inspect {}",
            root.display()
        );
        assert!(
            !exit.success(),
            "fixture finished before the interruption was established"
        );
        assert!(!f.state.join(COMPLETION).exists());
        let published = fs::read_dir(&f.home)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("link-")
            })
            .count();
        assert!(published < 512);
        if phase == "first-published" {
            assert!(published > 0);
        }
        fs::write(root.join("interruption.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "phase": phase, "child_pid": child.id(), "exit": exit.code(), "published": published,
                "completion": false
    })).unwrap()).unwrap();
        let reg = f.open();
        reg.recover().unwrap();
        assert!(!intent.exists());
        assert_eq!(fs::read_dir(&f.home).unwrap().count(), 1);
        assert_eq!(
            fs::read_to_string(f.home.join("unrelated.txt")).unwrap(),
            "keep-me"
        );
        assert_eq!(
            fs::read_to_string(f.source.join("file.txt")).unwrap(),
            "source-file"
        );
    }
}

#[test]
fn nonempty_unowned_state_is_not_adopted() {
    let f = Fixture::new("adopt");
    fs::create_dir_all(&f.state).unwrap();
    fs::write(f.state.join("keep"), "foreign-state").unwrap();
    contains(
        Registration::open(&f.state).unwrap_err(),
        "no ownership record and is nonempty",
    );
    assert_eq!(
        fs::read_to_string(f.state.join("keep")).unwrap(),
        "foreign-state"
    );
}

#[test]
fn relative_escape_and_reparse_ancestors_are_refused() {
    let f = Fixture::new("paths");
    let reg = f.open();
    contains(
        reg.apply(&[item(
            "file",
            "rel",
            PathBuf::from("file.txt"),
            f.file_dest(),
        )])
        .unwrap_err(),
        "absolute without parent traversal",
    );
    contains(
        reg.apply(&[item(
            "file",
            "escape",
            f.source.join("file.txt"),
            f.home.join("nested").join("..").join("escape.md"),
        )])
        .unwrap_err(),
        "absolute without parent traversal",
    );

    let foreign = f.source.parent().unwrap().join("foreign-root");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("keep"), "foreign-root").unwrap();
    let agents = f.home.join(".agents");
    symlink_dir(&foreign, &agents).unwrap();
    contains(
        reg.apply(&[item(
            "skill",
            "reparse",
            f.source.join("dir"),
            agents.join("skills/one"),
        )])
        .unwrap_err(),
        "reparse point",
    );
    assert_eq!(
        fs::read_to_string(foreign.join("keep")).unwrap(),
        "foreign-root"
    );
    assert_eq!(fs::read_dir(&foreign).unwrap().count(), 1);
    assert!(!reg.journal_path().exists());
}
