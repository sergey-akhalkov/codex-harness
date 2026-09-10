use super::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    os::{
        raw::c_void,
        windows::{
            ffi::OsStrExt,
            fs::{symlink_dir, symlink_file},
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
    },
    path::PathBuf,
};

struct Fixture {
    root: PathBuf,
    codex: PathBuf,
    user: PathBuf,
    pending: Value,
    before: Vec<u8>,
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for shift in [18, 12, 6, 0] {
            let position = (18 - shift) / 6;
            output.push(if position > chunk.len() as u32 {
                '='
            } else {
                ALPHABET[((n >> shift) & 63) as usize] as char
            });
        }
    }
    output
}

impl Fixture {
    fn new(scope: PathScope) -> Self {
        let root = tempfile::Builder::new()
            .prefix("native-legacy-recover-")
            .tempdir()
            .unwrap()
            .keep();
        let codex = root.join("codex");
        let user = root.join("user");
        let source = root.join("source");
        fs::create_dir_all(codex.join("harness/bin")).unwrap();
        fs::create_dir_all(user.join(".agents/skills")).unwrap();
        fs::create_dir_all(source.join("old-skill")).unwrap();
        fs::write(source.join("old.md"), b"old source").unwrap();
        fs::write(source.join("new.md"), b"new source").unwrap();
        fs::write(source.join("old-skill/keep"), b"source stays").unwrap();
        let link = |kind: &str, name: &str, destination: PathBuf, source: PathBuf| json!({"kind":kind,"name":name,"destination":destination,"source":source,"owned":true});
        let old_links = vec![
            link(
                "instructions",
                "global-instructions",
                codex.join("AGENTS.md"),
                source.join("old.md"),
            ),
            link(
                "skill",
                "a-name-independent-of-folder",
                user.join(".agents/skills/old-skill"),
                source.join("old-skill"),
            ),
        ];
        let previous = json!({"schemaVersion":1,"sourceRoot":source,"codexHome":codex,"userHome":user,"dependencyUserHome":user,"codexCommand":root.join("upstream.exe"),"profileName":"harness","links":old_links,"pathScope":scope,"pathAdded":true,"versions":{}});
        let before = serde_json::to_vec_pretty(&previous).unwrap();
        let mut planned = previous.clone();
        planned["links"] = json!([
            link(
                "instructions",
                "global-instructions",
                codex.join("AGENTS.md"),
                source.join("new.md")
            ),
            link(
                "launcher",
                "codex",
                codex.join("harness/bin/codex.ps1"),
                source.join("new.md")
            ),
        ]);
        let after = serde_json::to_vec_pretty(&planned).unwrap();
        fs::write(codex.join("harness/installation.json"), &after).unwrap();
        symlink_file(source.join("new.md"), codex.join("AGENTS.md")).unwrap();
        symlink_file(source.join("new.md"), codex.join("harness/bin/codex.ps1")).unwrap();
        // An unrelated adopted destination is not in the operations list.
        fs::write(codex.join("harness.config.toml"), b"adopted user edit").unwrap();
        let path_after = Some("C:\\owned-after;C:\\общий".to_string());
        match scope {
            PathScope::Process => ProcessPathSnapshot::read()
                .unwrap()
                .legacy_restore(&path_after, &std::env::var("PATH").ok())
                .unwrap()
                .publish()
                .unwrap(),
            PathScope::User => {
                let snapshot = UserPathSnapshot::for_registration().unwrap();
                snapshot
                    .legacy_restore(&path_after, &snapshot.text().unwrap())
                    .unwrap()
                    .publish_legacy()
                    .unwrap();
            }
        }
        let pending = json!({
            "previousState":previous,"plannedState":planned,"pathScope":scope,
            "pathBefore":"C:\\owned-before;C:\\общий","pathAfter":path_after,
            "stateBeforeBytes":base64(&before),"stateAfterHash":crate::build_identity::hash_bytes(&after).to_ascii_uppercase(),
            "operations":[
                {"destination":codex.join("AGENTS.md"),"oldSource":source.join("old.md"),"newSource":source.join("new.md")},
                {"destination":user.join(".agents/skills/old-skill"),"oldSource":source.join("old-skill"),"newSource":null},
                {"destination":codex.join("harness/bin/codex.ps1"),"oldSource":null,"newSource":source.join("new.md")}
            ]
        });
        let fixture = Self {
            root,
            codex,
            user,
            pending,
            before,
        };
        fixture.save();
        fixture
    }
    fn save(&self) {
        fs::write(
            self.journal(),
            serde_json::to_vec_pretty(&self.pending).unwrap(),
        )
        .unwrap();
    }
    fn journal(&self) -> PathBuf {
        self.codex.join("harness/pending.json")
    }
    fn recover(&self) -> io::Result<crate::core_install::RecoveryReport> {
        crate::core_install::recover(&self.codex, &self.user, &self.user)
    }
    fn preview(&self) -> io::Result<crate::core_install::RecoveryPreview> {
        crate::core_install::preview_recovery(&self.codex, &self.user, &self.user)
    }
    fn snapshot_owned(&self) -> OwnedSnapshot {
        OwnedSnapshot {
            files: collect_owned(&self.root),
            registry: UserPathSnapshot::for_registration()
                .unwrap()
                .text()
                .unwrap(),
            process: std::env::var_os("PATH"),
        }
    }
    fn assert_restored(&self) {
        self.assert_restored_objects();
        let scope: PathScope = serde_json::from_value(self.pending["pathScope"].clone()).unwrap();
        let current = match scope {
            PathScope::User => UserPathSnapshot::for_registration()
                .unwrap()
                .text()
                .unwrap(),
            PathScope::Process => std::env::var("PATH").ok(),
        };
        assert!(current.as_deref() == self.pending["pathBefore"].as_str());
    }
    fn assert_restored_objects(&self) {
        assert_eq!(
            fs::read(self.codex.join("harness/installation.json")).unwrap(),
            self.before
        );
        assert_eq!(
            fs::read_link(self.codex.join("AGENTS.md")).unwrap(),
            self.root.join("source/old.md")
        );
        assert_eq!(
            fs::read_link(self.user.join(".agents/skills/old-skill")).unwrap(),
            self.root.join("source/old-skill")
        );
        assert!(fs::symlink_metadata(self.codex.join("harness/bin/codex.ps1")).is_err());
        assert!(!self.journal().exists());
        assert_eq!(
            fs::read(self.codex.join("harness.config.toml")).unwrap(),
            b"adopted user edit"
        );
        assert_eq!(
            fs::read(self.root.join("source/old-skill/keep")).unwrap(),
            b"source stays"
        );
    }
}

#[test]
#[ignore = "explicit current native manager; owned Process PATH fixture outside checkout"]
fn native_manager_recovers_legacy_pending_outside_checkout() {
    crate::process_path::with_test_environment(|| {
        let manager = PathBuf::from(
            std::env::var_os("HARNESS_CORE_REAL_MANAGER")
                .expect("explicit native manager required"),
        );
        let fixture = Fixture::new(PathScope::Process);
        let parent_path = std::env::var_os("PATH");
        let registry_path = UserPathSnapshot::read().unwrap().text().unwrap();
        for (index, expected) in ["recovered-legacy", "no-pending-operation"]
            .into_iter()
            .enumerate()
        {
            let mut command = crate::process::CommandSpec::new(&manager);
            command.args = vec![
                "recover".into(),
                "--core-only".into(),
                "--codex-home".into(),
                fixture.codex.as_os_str().into(),
                "--user-home".into(),
                fixture.user.as_os_str().into(),
            ];
            command.current_dir = Some(fixture.root.clone());
            let stdout = fixture.root.join(format!("cli-{index}.stdout"));
            let outcome = crate::native_build::invoke_management(
                command,
                &fixture.root.join(format!("cli-{index}.stderr")),
                Some(&stdout),
                std::time::Duration::from_secs(30),
            )
            .unwrap();
            assert_eq!(outcome.reason, crate::process::StopReason::Exited);
            assert_eq!(outcome.exit_code, 0, "evidence: {}", fixture.root.display());
            let report: Value = serde_json::from_slice(&fs::read(stdout).unwrap()).unwrap();
            assert_eq!(report["status"], expected);
            assert_eq!(report["committed"], false);
            assert_eq!(report["model_calls"], 0);
            fixture.assert_restored_objects();
            assert!(std::env::var_os("PATH") == parent_path);
            assert!(UserPathSnapshot::read().unwrap().text().unwrap() == registry_path);
        }
        println!(
            "legacy native CLI recovery evidence: {}",
            fixture.root.display()
        );
    });
}

#[test]
#[ignore = "owned child for killed_legacy_recovery_resumes_without_destructors"]
fn legacy_recovery_process_fixture() {
    let root = PathBuf::from(std::env::var_os("HARNESS_LEGACY_RECOVER_ROOT").unwrap());
    let phase = std::env::var("HARNESS_LEGACY_RECOVER_PHASE").unwrap();
    recover_inner(
        &root.join("codex"),
        &root.join("user"),
        &root.join("user"),
        |at| {
            if at == phase {
                fs::write(root.join("paused"), at).unwrap();
                std::thread::sleep(std::time::Duration::from_secs(20));
                std::process::exit(79);
            }
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn killed_legacy_recovery_resumes_without_destructors() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    crate::process_path::with_test_environment(|| {
        for phase in ["prepared", "removed", "operation", "path", "metadata"] {
            let fixture = Fixture::new(PathScope::Process);
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "legacy_pending::tests::legacy_recovery_process_fixture",
                    "--nocapture",
                ])
                .env("HARNESS_LEGACY_RECOVER_ROOT", &fixture.root)
                .env("HARNESS_LEGACY_RECOVER_PHASE", phase)
                .stdin(Stdio::null())
                .stdout(fs::File::create(fixture.root.join("child.stdout")).unwrap())
                .stderr(fs::File::create(fixture.root.join("child.stderr")).unwrap())
                .spawn()
                .unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while !fixture.root.join("paused").exists() && Instant::now() < until {
                if child.try_wait().unwrap().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let reached =
                fs::read(fixture.root.join("paused")).ok().as_deref() == Some(phase.as_bytes());
            let _ = child.kill();
            let exit = child.wait().unwrap();
            assert!(
                reached,
                "boundary {phase}, evidence: {}",
                fixture.root.display()
            );
            assert!(!exit.success());
            assert_ne!(exit.code(), Some(79));
            // Legacy intent has no PID claim. A matching explicit recovery may
            // restore this process even when the original recovery owner died.
            assert_eq!(fixture.recover().unwrap().status, "recovered-legacy");
            fixture.assert_restored();
        }
    });
}

#[test]
fn legacy_recovery_restores_exact_state_links_and_both_path_scopes_after_interruption() {
    crate::process_path::with_test_environment(|| {
        crate::environment_path::with_test_registry(|| {
            for scope in [PathScope::User, PathScope::Process] {
                for phase in [
                    "direct",
                    "prepared",
                    "removed",
                    "operation",
                    "path",
                    "metadata",
                    "journal",
                ] {
                    let fixture = Fixture::new(scope);
                    if phase != "direct" {
                        let result =
                            recover_inner(&fixture.codex, &fixture.user, &fixture.user, |at| {
                                if at == phase {
                                    Err(conflict("owned interruption"))
                                } else {
                                    Ok(())
                                }
                            });
                        assert!(result.is_err(), "boundary {phase}");
                        assert_eq!(fixture.journal().exists(), phase != "journal");
                    }
                    let report = fixture.recover().unwrap();
                    assert!(!report.committed);
                    fixture.assert_restored();
                    assert_eq!(fixture.recover().unwrap().status, "no-pending-operation");
                }
            }
        })
    });
}

#[test]
fn legacy_recovery_refuses_foreign_objects_hash_path_and_other_pending_before_mutation() {
    crate::process_path::with_test_environment(|| {
        for case in [
            "file",
            "symlink",
            "metadata",
            "metadata-link",
            "parent-link",
            "path",
            "native",
            "activation",
            "wrong-owner",
            "wrong-dependency",
        ] {
            let fixture = Fixture::new(PathScope::Process);
            let agent = fixture.codex.join("AGENTS.md");
            let metadata = fixture.codex.join("harness/installation.json");
            match case {
                "file" => {
                    fs::remove_file(&agent).unwrap();
                    fs::write(&agent, b"foreign object").unwrap();
                }
                "symlink" => {
                    fs::remove_file(&agent).unwrap();
                    symlink_file(fixture.root.join("foreign-missing"), &agent).unwrap();
                }
                "metadata" => fs::write(&metadata, b"foreign metadata").unwrap(),
                "metadata-link" => {
                    fs::remove_file(&metadata).unwrap();
                    symlink_file(fixture.root.join("source/old.md"), &metadata).unwrap();
                }
                "parent-link" => {
                    fs::remove_dir(fixture.user.join(".agents/skills")).unwrap();
                    let foreign = fixture.root.join("foreign-skills");
                    fs::create_dir(&foreign).unwrap();
                    fs::write(foreign.join("keep"), b"foreign directory stays").unwrap();
                    symlink_dir(foreign, fixture.user.join(".agents/skills")).unwrap();
                }
                "path" => ProcessPathSnapshot::read()
                    .unwrap()
                    .legacy_restore(
                        &Some("C:\\foreign-path".into()),
                        &std::env::var("PATH").ok(),
                    )
                    .unwrap()
                    .publish()
                    .unwrap(),
                "native" => {
                    fs::create_dir(fixture.codex.join("harness/native-registration")).unwrap();
                    fs::write(
                        fixture
                            .codex
                            .join("harness/native-registration/journal.json"),
                        b"other pending",
                    )
                    .unwrap();
                }
                "activation" => fs::write(
                    fixture.codex.join("harness/activation-pending.json"),
                    b"other pending",
                )
                .unwrap(),
                _ => {}
            }
            let before_journal = fs::read(fixture.journal()).unwrap();
            let before_meta = fs::read(&metadata).unwrap();
            let before_path = std::env::var_os("PATH");
            let result = if case == "wrong-owner" {
                crate::core_install::recover(
                    &fixture.codex,
                    &fixture.root.join("other-user"),
                    &fixture.user,
                )
            } else if case == "wrong-dependency" {
                crate::core_install::recover(
                    &fixture.codex,
                    &fixture.user,
                    &fixture.root.join("other-dependency"),
                )
            } else {
                fixture.recover()
            };
            assert!(result.is_err(), "case {case}");
            assert_eq!(fs::read(fixture.journal()).unwrap(), before_journal);
            assert_eq!(fs::read(&metadata).unwrap(), before_meta);
            assert!(std::env::var_os("PATH") == before_path);
            assert!(fixture.codex.join("harness/bin/codex.ps1").is_symlink());
            assert!(!fixture.user.join(".agents/skills/old-skill").exists());
        }
    });
}

#[test]
fn legacy_recovery_restores_dangling_targets_without_traversing_relocated_source() {
    crate::process_path::with_test_environment(|| {
        let fixture = Fixture::new(PathScope::Process);
        let moved = fixture.root.join("relocated-source");
        fs::rename(fixture.root.join("source"), &moved).unwrap();
        fixture.recover().unwrap();
        assert_eq!(
            fs::read(fixture.codex.join("harness/installation.json")).unwrap(),
            fixture.before
        );
        assert_eq!(
            fs::read_link(fixture.codex.join("AGENTS.md")).unwrap(),
            fixture.root.join("source/old.md")
        );
        assert_eq!(
            fs::read_link(fixture.user.join(".agents/skills/old-skill")).unwrap(),
            fixture.root.join("source/old-skill")
        );
        assert_eq!(
            fs::read(moved.join("old-skill/keep")).unwrap(),
            b"source stays"
        );
        assert!(!fixture.journal().exists());
    });
}

#[test]
fn legacy_recovery_pins_ordinary_parent_namespaces_through_prepare_and_replacement() {
    crate::process_path::with_test_environment(|| {
        for phase in ["prepared", "removed"] {
            let mut fixture = Fixture::new(PathScope::Process);
            let parent = fixture.user.join(".agents/skills");
            if phase == "removed" {
                let next = fixture.root.join("source/new-skill");
                fs::create_dir(&next).unwrap();
                symlink_dir(&next, parent.join("old-skill")).unwrap();
                fixture.pending["operations"][1]["newSource"] = json!(next);
                fixture.pending["plannedState"]["links"].as_array_mut().unwrap().push(json!({
                    "kind":"skill", "name":"replacement-skill", "destination":parent.join("old-skill"), "source":next,"owned":true
                }));
                let after = serde_json::to_vec_pretty(&fixture.pending["plannedState"]).unwrap();
                fs::write(fixture.codex.join("harness/installation.json"), &after).unwrap();
                fixture.pending["stateAfterHash"] = crate::build_identity::hash_bytes(&after)
                    .to_ascii_uppercase()
                    .into();
                fixture.save();
            }
            let replacement = fixture.root.join("foreign-parent");
            fs::create_dir(&replacement).unwrap();
            fs::write(replacement.join("keep"), b"foreign directory stays").unwrap();
            let mut attempted = false;
            recover_inner(&fixture.codex, &fixture.user, &fixture.user, |at| {
                if at == phase && !parent.join("old-skill").is_symlink() {
                    attempted = true;
                    let moved = fixture.root.join("moved-parent");
                    assert!(
                        fs::rename(&parent, &moved).is_err(),
                        "validated parent was replaceable at {phase}"
                    );
                }
                Ok(())
            })
            .unwrap();
            assert!(attempted);
            fixture.assert_restored();
            assert_eq!(
                fs::read(replacement.join("keep")).unwrap(),
                b"foreign directory stays"
            );
            assert!(!replacement.join("old-skill").exists());
        }
    });
}

#[test]
fn legacy_recovery_preserves_wrong_type_link_with_the_recorded_new_target() {
    crate::process_path::with_test_environment(|| {
        let fixture = Fixture::new(PathScope::Process);
        let path = fixture.codex.join("harness/bin/codex.ps1");
        let target = fs::read_link(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::remove_file(&target).unwrap();
        symlink_dir(&target, &path).unwrap();
        let journal = fs::read(fixture.journal()).unwrap();
        let before = fs::read(fixture.codex.join("harness/installation.json")).unwrap();
        assert!(fixture.recover().is_err());
        assert_eq!(fs::read_link(&path).unwrap(), target);
        assert!(std::os::windows::fs::FileTypeExt::is_symlink_dir(
            &fs::symlink_metadata(&path).unwrap().file_type()
        ));
        assert_eq!(fs::read(fixture.journal()).unwrap(), journal);
        assert_eq!(
            fs::read(fixture.codex.join("harness/installation.json")).unwrap(),
            before
        );
    });
}

#[test]
fn historical_hashless_journal_restores_recorded_state_and_preserves_unrelated_metadata() {
    crate::process_path::with_test_environment(|| {
        for foreign in [false, true] {
            let mut fixture = Fixture::new(PathScope::Process);
            fixture
                .pending
                .as_object_mut()
                .unwrap()
                .remove("stateAfterHash");
            fixture
                .pending
                .as_object_mut()
                .unwrap()
                .remove("stateBeforeBytes");
            fixture.save();
            let metadata = fixture.codex.join("harness/installation.json");
            if foreign {
                fs::write(&metadata, b"foreign unrelated metadata").unwrap();
                assert!(fixture.recover().is_err());
                assert_eq!(fs::read(&metadata).unwrap(), b"foreign unrelated metadata");
                assert!(fixture.journal().exists());
                assert!(fixture.codex.join("harness/bin/codex.ps1").is_symlink());
            } else {
                fixture.recover().unwrap();
                fixture.assert_restored();
            }
        }
    });
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OwnedSnapshot {
    files: BTreeMap<PathBuf, OwnedEntry>,
    registry: Option<String>,
    process: Option<std::ffi::OsString>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OwnedEntry {
    File {
        bytes: Vec<u8>,
        id: (u32, u64),
    },
    Directory {
        id: (u32, u64),
    },
    Link {
        target: PathBuf,
        directory: bool,
        id: (u32, u64),
    },
}

fn object_id(path: &std::path::Path) -> (u32, u64) {
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            GetFileInformationByHandle, OPEN_EXISTING,
        },
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut::<c_void>(),
        )
    };
    assert!(
        handle != INVALID_HANDLE_VALUE && !handle.is_null(),
        "{}",
        path.display()
    );
    let owned = unsafe { OwnedHandle::from_raw_handle(handle as _) };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    assert_ne!(
        unsafe { GetFileInformationByHandle(owned.as_raw_handle() as HANDLE, &mut info) },
        0
    );
    (
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    )
}

fn collect_owned(root: &std::path::Path) -> BTreeMap<PathBuf, OwnedEntry> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        let file_type = metadata.file_type();
        let id = object_id(&path);
        if file_type.is_symlink() {
            files.insert(
                path.clone(),
                OwnedEntry::Link {
                    target: fs::read_link(&path).unwrap(),
                    directory: std::os::windows::fs::FileTypeExt::is_symlink_dir(&file_type),
                    id,
                },
            );
            continue;
        }
        if file_type.is_dir() {
            files.insert(path.clone(), OwnedEntry::Directory { id });
            for child in fs::read_dir(&path).unwrap() {
                pending.push(child.unwrap().path());
            }
            continue;
        }
        files.insert(
            path.clone(),
            OwnedEntry::File {
                bytes: fs::read(&path).unwrap(),
                id,
            },
        );
    }
    files
}

fn assert_preview_rollback(preview: &crate::core_install::RecoveryPreview) {
    assert_eq!(preview.status, "preview");
    assert_eq!(preview.action, "rollback");
    assert_eq!(preview.journal, Some("legacy"));
    assert_eq!(preview.model_calls, 0);
}

fn assert_preview_none(preview: &crate::core_install::RecoveryPreview) {
    assert_eq!(preview.status, "preview");
    assert_eq!(preview.action, "none");
    assert_eq!(preview.journal, None);
    assert_eq!(preview.model_calls, 0);
}

fn assert_owned_unchanged(before: &OwnedSnapshot, after: &OwnedSnapshot) {
    assert_eq!(after.files, before.files);
    assert!(after.registry == before.registry);
    assert!(after.process == before.process);
}

fn apply_foreign_pending(fixture: &Fixture, case: &str) {
    let agent = fixture.codex.join("AGENTS.md");
    let metadata = fixture.codex.join("harness/installation.json");
    match case {
        "file" => {
            fs::remove_file(&agent).unwrap();
            fs::write(&agent, b"foreign object").unwrap();
        }
        "symlink" => {
            fs::remove_file(&agent).unwrap();
            symlink_file(fixture.root.join("foreign-missing"), &agent).unwrap();
        }
        "metadata" => fs::write(&metadata, b"foreign metadata").unwrap(),
        "metadata-link" => {
            fs::remove_file(&metadata).unwrap();
            symlink_file(fixture.root.join("source/old.md"), &metadata).unwrap();
        }
        "parent-link" => {
            fs::remove_dir(fixture.user.join(".agents/skills")).unwrap();
            let foreign = fixture.root.join("foreign-skills");
            fs::create_dir(&foreign).unwrap();
            fs::write(foreign.join("keep"), b"foreign directory stays").unwrap();
            symlink_dir(foreign, fixture.user.join(".agents/skills")).unwrap();
        }
        "path" => ProcessPathSnapshot::read()
            .unwrap()
            .legacy_restore(
                &Some("C:\\foreign-path".into()),
                &std::env::var("PATH").ok(),
            )
            .unwrap()
            .publish()
            .unwrap(),
        "native" => {
            fs::create_dir(fixture.codex.join("harness/native-registration")).unwrap();
            fs::write(
                fixture
                    .codex
                    .join("harness/native-registration/journal.json"),
                b"other pending",
            )
            .unwrap();
        }
        "activation" => fs::write(
            fixture.codex.join("harness/activation-pending.json"),
            b"other pending",
        )
        .unwrap(),
        _ => {}
    }
}

fn preview_result(
    fixture: &Fixture,
    case: &str,
) -> io::Result<crate::core_install::RecoveryPreview> {
    if case == "wrong-owner" {
        crate::core_install::preview_recovery(
            &fixture.codex,
            &fixture.root.join("other-user"),
            &fixture.user,
        )
    } else if case == "wrong-dependency" {
        crate::core_install::preview_recovery(
            &fixture.codex,
            &fixture.user,
            &fixture.root.join("other-dependency"),
        )
    } else {
        fixture.preview()
    }
}

fn invoke_recover_cli(
    fixture: &Fixture,
    manager: &std::path::Path,
    preview: bool,
    index: usize,
    evidence: &std::path::Path,
) -> Value {
    let mut command = crate::process::CommandSpec::new(manager);
    command.args = vec![
        "recover".into(),
        "--core-only".into(),
        "--codex-home".into(),
        fixture.codex.as_os_str().into(),
        "--user-home".into(),
        fixture.user.as_os_str().into(),
        "--dependency-user-home".into(),
        fixture.user.as_os_str().into(),
    ];
    if preview {
        command.args.push("--preview".into());
    }
    command.current_dir = Some(fixture.root.clone());
    let stdout = evidence.join(format!("cli-{index}.stdout"));
    let outcome = crate::native_build::invoke_management(
        command,
        &evidence.join(format!("cli-{index}.stderr")),
        Some(&stdout),
        std::time::Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(outcome.reason, crate::process::StopReason::Exited);
    assert_eq!(outcome.exit_code, 0, "evidence: {}", evidence.display());
    serde_json::from_slice(&fs::read(stdout).unwrap()).unwrap()
}

#[test]
fn legacy_recovery_preview_repeats_then_recovers_user_and_process_pending() {
    crate::process_path::with_test_environment(|| {
        crate::environment_path::with_test_registry(|| {
            for scope in [PathScope::User, PathScope::Process] {
                let fixture = Fixture::new(scope);
                let before = fixture.snapshot_owned();
                for _ in 0..2 {
                    let preview = fixture.preview().unwrap();
                    assert_preview_rollback(&preview);
                    assert_owned_unchanged(&before, &fixture.snapshot_owned());
                }
                let report = fixture.recover().unwrap();
                assert_eq!(report.status, "recovered-legacy");
                assert!(!report.committed);
                assert_eq!(report.model_calls, 0);
                fixture.assert_restored();
                let restored = fixture.snapshot_owned();
                let none = fixture.preview().unwrap();
                assert_preview_none(&none);
                assert_owned_unchanged(&restored, &fixture.snapshot_owned());
                assert_eq!(fixture.recover().unwrap().status, "no-pending-operation");
            }
        })
    });
}

#[test]
fn legacy_recovery_preview_refuses_foreign_objects_hash_path_and_other_pending_before_mutation() {
    crate::process_path::with_test_environment(|| {
        for case in [
            "file",
            "symlink",
            "metadata",
            "metadata-link",
            "parent-link",
            "path",
            "native",
            "activation",
            "wrong-owner",
            "wrong-dependency",
        ] {
            let fixture = Fixture::new(PathScope::Process);
            apply_foreign_pending(&fixture, case);
            let before = fixture.snapshot_owned();
            assert!(preview_result(&fixture, case).is_err(), "case {case}");
            assert_owned_unchanged(&before, &fixture.snapshot_owned());
            assert!(fixture.journal().exists());
            if case == "parent-link" {
                assert_eq!(
                    fs::read(fixture.root.join("foreign-skills/keep")).unwrap(),
                    b"foreign directory stays"
                );
            } else if case != "file" && case != "symlink" {
                assert!(fixture.codex.join("harness/bin/codex.ps1").is_symlink());
                assert!(!fixture.user.join(".agents/skills/old-skill").exists());
            }
        }
    });
}

#[test]
#[ignore = "explicit current native manager; owned Process PATH fixture outside checkout"]
fn native_manager_previews_legacy_pending_outside_checkout() {
    crate::process_path::with_test_environment(|| {
        let manager = PathBuf::from(
            std::env::var_os("HARNESS_CORE_REAL_MANAGER")
                .expect("explicit native manager required"),
        );
        let fixture = Fixture::new(PathScope::Process);
        let before = fixture.snapshot_owned();
        let real_user = UserPathSnapshot::read().unwrap().text().unwrap();
        let evidence = tempfile::Builder::new()
            .prefix("native-legacy-recover-cli-")
            .tempdir()
            .unwrap()
            .keep();
        for index in 0..2 {
            let report = invoke_recover_cli(&fixture, &manager, true, index, &evidence);
            assert_eq!(report["status"], "preview");
            assert_eq!(report["action"], "rollback");
            assert_eq!(report["journal"], "legacy");
            assert_eq!(report["model_calls"], 0);
            assert!(report.get("committed").is_none());
            assert_owned_unchanged(&before, &fixture.snapshot_owned());
            assert!(UserPathSnapshot::read().unwrap().text().unwrap() == real_user);
        }
        let recovered = invoke_recover_cli(&fixture, &manager, false, 2, &evidence);
        assert_eq!(recovered["status"], "recovered-legacy");
        assert_eq!(recovered["committed"], false);
        assert_eq!(recovered["model_calls"], 0);
        fixture.assert_restored_objects();
        let restored = fixture.snapshot_owned();
        let none = invoke_recover_cli(&fixture, &manager, true, 3, &evidence);
        assert_eq!(none["status"], "preview");
        assert_eq!(none["action"], "none");
        assert_eq!(none["journal"], Value::Null);
        assert_eq!(none["model_calls"], 0);
        assert!(none.get("committed").is_none());
        assert_owned_unchanged(&restored, &fixture.snapshot_owned());
        assert!(UserPathSnapshot::read().unwrap().text().unwrap() == real_user);
        println!(
            "legacy native CLI recovery preview evidence: {}",
            evidence.display()
        );
    });
}
