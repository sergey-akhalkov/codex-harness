#![cfg(windows)]

use harness_core::installation_state::LegacyInstallation;
use serde_json::{Value, json};
use std::{
    fs,
    os::windows::fs::{symlink_dir, symlink_file},
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    user: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("harness-state-Юникод-")
            .tempdir()
            .unwrap();
        let home = root.path().join("codex home");
        let user = root.path().join("user home");
        let state = home.join("harness/installation.json");
        Self {
            root,
            home,
            user,
            state,
        }
    }

    fn metadata(&self) -> Value {
        let old = self.root.path().join("missing-old-checkout");
        json!({
            "schemaVersion":1, "sourceRoot":old, "codexHome":self.home, "userHome":self.user,
            "codexCommand": self.root.path().join("unavailable-upstream/codex.ps1"),
            "profileName":"harness", "pathScope":"Process", "pathAdded":false,
            "versions":{"codex":"private version sentinel", "future-version":"preserved"},
            "links":[
                {"kind":"instructions","name":"AGENTS","source":old.join("global/principles-of-work.md"),"destination":self.home.join("AGENTS.md"),"owned":false},
                {"kind":"skill","name":"example","source":old.join(".agents/skills/example"),"destination":self.user.join(".agents/skills/example"),"owned":true}
            ]
        })
    }

    fn write(&self, data: &Value) -> Vec<u8> {
        fs::create_dir_all(self.state.parent().unwrap()).unwrap();
        let bytes = serde_json::to_vec_pretty(data).unwrap();
        fs::write(&self.state, &bytes).unwrap();
        bytes
    }

    fn run(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .arg("inspect-installation")
            .arg("--codex-home")
            .arg(&self.home)
            .arg("--user-home")
            .arg(&self.user)
            .current_dir(self.root.path())
            .output()
            .unwrap()
    }

    fn refused(&self, expected: &[u8]) {
        let output = self.run();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private version sentinel"));
        assert_eq!(fs::read(&self.state).unwrap(), expected);
    }
}

#[test]
fn actual_command_preserves_absent_homes_and_legacy_adoption_after_relocation() {
    let f = Fixture::new();
    let absent = f.run();
    assert!(absent.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&absent.stdout).unwrap(),
        Value::Null
    );
    assert!(!f.home.exists());
    assert!(!f.user.exists());
    let mut aliased = f.metadata();
    aliased["links"][1]["name"] = "different-descriptor-name".into();
    aliased["pathScope"] = "pRoCeSs".into();
    let bytes = f.write(&aliased);
    let output = f.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["links"], 2);
    assert_eq!(summary["owned_links"], 1);
    assert_eq!(summary["adopted_links"], 1);
    assert_eq!(summary["path_scope"], "Process");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private version sentinel"));
    let state = LegacyInstallation::read(&f.home, &f.user, &f.user)
        .unwrap()
        .unwrap();
    assert!(!state.links()[0].owned);
    assert!(state.links()[1].owned);
    assert_eq!(state.links()[1].name, "different-descriptor-name");
    assert!(!format!("{state:?}").contains("private version sentinel"));
    state.verify_unchanged().unwrap();
    assert_eq!(fs::read(&f.state).unwrap(), bytes);
    assert!(!f.root.path().join("missing-old-checkout").exists());
}

#[test]
fn unsupported_foreign_duplicate_or_unbounded_metadata_never_becomes_fresh_state() {
    let f = Fixture::new();
    let original = f.metadata();
    for (pointer, value) in [
        ("/schemaVersion", json!(2)),
        ("/codexHome", json!(f.root.path().join("foreign"))),
        ("/userHome", json!(f.root.path().join("foreign"))),
        ("/pathScope", json!("Machine")),
        ("/profileName", json!("foreign")),
        ("/links/0/destination", json!(f.root.path().join("foreign"))),
        ("/links/0/source", json!(f.root.path().join("foreign"))),
        (
            "/links/0/source",
            json!(f.root.path().join("missing-old-checkout/../foreign")),
        ),
        ("/links/1/name", json!("../escape")),
        ("/links/0/kind", json!("unknown")),
        (
            "/versions/codex",
            json!("private version sentinel".repeat(60000)),
        ),
    ] {
        let mut candidate = original.clone();
        *candidate.pointer_mut(pointer).unwrap() = value;
        let bytes = f.write(&candidate);
        f.refused(&bytes);
    }
    let mut candidate = original.clone();
    candidate["links"]
        .as_array_mut()
        .unwrap()
        .push(original["links"][0].clone());
    let bytes = f.write(&candidate);
    f.refused(&bytes);
    candidate = original.clone();
    candidate["dependencyUserHome"] = json!(f.root.path().join("foreign-dependency-owner"));
    let bytes = f.write(&candidate);
    f.refused(&bytes);
    candidate = original;
    candidate["unrecognized"] = json!(true);
    let bytes = f.write(&candidate);
    f.refused(&bytes);
    fs::write(&f.state, b"{private version sentinel invalid").unwrap();
    f.refused(b"{private version sentinel invalid");
}

#[test]
fn pending_reparse_and_concurrent_edits_preserve_state_and_foreign_objects() {
    let f = Fixture::new();
    let bytes = f.write(&f.metadata());
    fs::write(
        f.home.join("harness/pending.json"),
        b"owned unresolved legacy transaction",
    )
    .unwrap();
    f.refused(&bytes);
    assert_eq!(
        fs::read(f.home.join("harness/pending.json")).unwrap(),
        b"owned unresolved legacy transaction"
    );
    fs::remove_file(f.home.join("harness/pending.json")).unwrap();
    let state = LegacyInstallation::read(&f.home, &f.user, &f.user)
        .unwrap()
        .unwrap();
    fs::write(&f.state, b"foreign replacement bytes").unwrap();
    assert!(state.verify_unchanged().is_err());
    assert_eq!(fs::read(&f.state).unwrap(), b"foreign replacement bytes");
    fs::write(&f.state, &bytes).unwrap();
    let state = LegacyInstallation::read(&f.home, &f.user, &f.user)
        .unwrap()
        .unwrap();
    fs::rename(&f.state, f.root.path().join("retained-state")).unwrap();
    fs::write(&f.state, &bytes).unwrap();
    assert!(state.verify_unchanged().is_err());
    assert_eq!(fs::read(&f.state).unwrap(), bytes);
    fs::remove_file(&f.state).unwrap();
    symlink_file(f.root.path().join("retained-state"), &f.state).unwrap();
    f.refused(&bytes);
    fs::remove_file(&f.state).unwrap();
    symlink_file(f.root.path().join("missing-file"), &f.state).unwrap();
    assert_eq!(f.run().status.code(), Some(2));
    assert!(
        fs::symlink_metadata(&f.state)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::remove_file(&f.state).unwrap();
    fs::write(&f.state, &bytes).unwrap();
    let foreign = f.root.path().join("foreign-user");
    fs::create_dir(&foreign).unwrap();
    symlink_dir(&foreign, &f.user).unwrap();
    f.refused(&bytes);
    assert_eq!(fs::read_dir(&foreign).unwrap().count(), 0);
}
