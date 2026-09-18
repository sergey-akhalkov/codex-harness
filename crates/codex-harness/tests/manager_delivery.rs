//! Fresh-manager delivery: the stable link moves to a newer published build
//! while earlier processes keep the previous one, without any build file being
//! replaced, deleted or rewritten.
#![cfg(windows)]
use harness_core::{
    build_identity::{self, BINARIES, BuildRecord, INSPECTION_SCHEMA, SCHEMA},
    registration::{LinkChange, Registration},
};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    os::windows::fs::{OpenOptionsExt, symlink_file},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    source: PathBuf,
    builds: PathBuf,
    registration: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("manager-delivery-")
            .tempdir()
            .unwrap();
        let home = root.path().join("home");
        let source = root.path().join("source");
        for directory in [
            source.join("crates/one/src"),
            source.join("tools/rtk-adapter/src"),
            source.join(INSPECTION_SCHEMA).parent().unwrap().to_owned(),
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        for file in [
            "Cargo.toml",
            "Cargo.lock",
            "crates/one/src/lib.rs",
            INSPECTION_SCHEMA,
        ] {
            fs::write(source.join(file), "fixture").unwrap();
        }
        Self {
            builds: root.path().join("state/builds"),
            registration: home.join("harness/native-registration"),
            root,
            home,
            source,
        }
    }

    /// Publish one immutable build directory containing a runnable manager
    /// stand-in plus a verified record.
    fn publish(&self, name: &str) -> PathBuf {
        let build = self.builds.join(name);
        fs::create_dir_all(&build).unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_harness-launch-fixture"),
            build.join("codex-harness.exe"),
        )
        .unwrap();
        let mut binaries = BTreeMap::new();
        for binary in BINARIES {
            let path = build.join(binary);
            if *binary != "codex-harness.exe" {
                fs::write(&path, binary).unwrap();
            }
            binaries.insert(
                (*binary).to_owned(),
                build_identity::hash_file(&path).unwrap(),
            );
        }
        let record = BuildRecord {
            schema: SCHEMA,
            source_root: self.source.clone(),
            source: build_identity::source_identity(&self.source).unwrap(),
            rustc: "fixture".into(),
            cargo: "fixture".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
            binaries,
        };
        fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        build
    }

    fn manager_link(&self) -> PathBuf {
        self.home.join("harness/bin/codex-harness.exe")
    }

    /// The delivery step of an Install/Update for an already connected home:
    /// replace the owned command link with the newly delivered build, exactly
    /// as the core plan records the changed source.
    fn deliver(&self, previous: &Path, build: &Path) -> harness_core::registration::ApplyReport {
        let source = build.join("codex-harness.exe");
        let change = LinkChange::replace(
            &self.manager_link(),
            &previous.join("codex-harness.exe"),
            &source,
        )
        .unwrap();
        Registration::open(&self.registration)
            .unwrap()
            .apply_with_changes(&[], &[], &[], &[change])
            .unwrap()
    }
}

#[test]
fn delivery_moves_the_stable_link_while_the_previous_manager_keeps_running() {
    let fixture = Fixture::new();
    let older = fixture.publish("aaaa0000aaaa0000-1500-1");
    let newer = fixture.publish("bbbb0000bbbb0000-2500-2");
    let older_exe = older.join("codex-harness.exe");
    fs::create_dir_all(fixture.manager_link().parent().unwrap()).unwrap();
    symlink_file(&older_exe, fixture.manager_link()).unwrap();
    let before = fs::read(&older_exe).unwrap();

    // A live session from the previous build, plus an exclusive reader that
    // would reject any replacement of the file itself.
    let mut session = Command::new(&older_exe)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "interactive")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let held = OpenOptions::new()
        .read(true)
        .share_mode(0x0000_0001)
        .open(&older_exe)
        .unwrap();
    assert!(
        fs::remove_file(&older_exe).is_err(),
        "an exclusive reader must block replacing the previous build"
    );

    let report = fixture.deliver(&older, &newer);
    assert_eq!(report.changed_links.len(), 1);

    // Only the link moved: the previous build keeps its bytes, its reader and
    // its running process.
    assert_eq!(fs::read(&older_exe).unwrap(), before);
    assert!(
        session.try_wait().unwrap().is_none(),
        "the old session was interrupted"
    );
    assert_eq!(
        fixture.manager_link().canonicalize().unwrap(),
        newer.join("codex-harness.exe").canonicalize().unwrap()
    );
    // A new process resolves the delivered build through the same link.
    let launched = Command::new(fixture.manager_link())
        .arg("probe")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stderr)
    );
    assert!(
        String::from_utf8_lossy(&launched.stdout).contains("args"),
        "the delivered build did not run through the stable link"
    );

    session.stdin.take().unwrap().write_all(b"done\n").unwrap();
    drop(held);
    let finished = session.wait().unwrap();
    assert!(finished.success(), "the old session did not finish cleanly");
}

#[test]
fn core_install_outside_an_owned_state_still_requires_an_explicit_build() {
    let fixture = Fixture::new();
    let new_home = fixture.root.path().join("new-home");
    let new_user = fixture.root.path().join("new-user");
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args([
            "install",
            "--core-only",
            "--source",
            fixture.source.to_str().unwrap(),
            "--codex-home",
            new_home.to_str().unwrap(),
            "--user-home",
            new_user.to_str().unwrap(),
            "--preview",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--build DIRECTORY"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!new_home.exists());
    assert!(!new_user.exists());
}
