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
            "launcherSource": self.home.join("harness/launchers/fixture/codex.ps1"),
            // The script lifecycle records the configuration bridge as a
            // verbatim drive path.
            "configBridge": format!("\\\\?\\{}", self.home.join("harness/config-bridge/builds/fixture/codex-harness.exe").display()),
            "versions":{"codex":"private version sentinel", "future-version":"preserved"},
            "links":[
                {"kind":"instructions","name":"AGENTS","source":old.join("global/principles-of-work.md"),"destination":self.home.join("AGENTS.md"),"owned":false},
                // The published launcher copy lives inside CODEX_HOME.
                {"kind":"launcher","name":"codex","source":self.home.join("harness/launchers/fixture/codex.ps1"),"destination":self.home.join("harness/bin/codex.ps1"),"owned":true},
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
    aliased["links"][2]["name"] = "different-descriptor-name".into();
    aliased["pathScope"] = "pRoCeSs".into();
    let bytes = f.write(&aliased);
    let output = f.run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["links"], 3);
    assert_eq!(summary["owned_links"], 2);
    assert_eq!(summary["adopted_links"], 1);
    assert_eq!(summary["path_scope"], "Process");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private version sentinel"));
    let state = LegacyInstallation::read(&f.home, &f.user, &f.user)
        .unwrap()
        .unwrap();
    assert!(!state.links()[0].owned);
    assert!(state.links()[2].owned);
    assert_eq!(state.links()[2].name, "different-descriptor-name");
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
        (
            "/links/1/source",
            json!(f.root.path().join("foreign-launcher/codex.ps1")),
        ),
        ("/links/0/kind", json!("unknown")),
        ("/launcherSource", json!("relative/codex.ps1")),
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

mod shared_cpu_policy {
    use super::*;
    use harness_core::{build_identity, heavy_command};
    use std::{
        ffi::OsString,
        path::Path,
        process::{Child, Command, Stdio},
    };

    struct Sleeper(Child);
    impl Drop for Sleeper {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    struct Tree {
        root: PathBuf,
        source: PathBuf,
        build: PathBuf,
        cpu: PathBuf,
        heavy: PathBuf,
        upstream: PathBuf,
        replacement: PathBuf,
    }

    fn compile(dir: &Path, name: &str, source: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let input = dir.join(format!("{name}.rs"));
        let output = dir.join(format!("{name}.exe"));
        fs::write(&input, source).unwrap();
        let log = dir.join(format!("{name}.err"));
        let status = Command::new("rustc")
            .arg(&input)
            .args(["--edition=2024", "-o"])
            .arg(&output)
            .stderr(fs::File::create(&log).unwrap())
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(
            status.success(),
            "{name} compile failed: {}",
            fs::read_to_string(&log).unwrap_or_default()
        );
        output
    }

    fn tree() -> Tree {
        let root = tempfile::Builder::new()
            .prefix("cpu-policy-Юникод-")
            .tempdir()
            .unwrap()
            .keep();
        let source = root.join("source");
        let build = root.join("build");
        for dir in [
            source.join("global/agents"),
            source.join("skills/one"),
            source.join("crates/one/src"),
            source.join("tools/rtk-adapter/src"),
            build.clone(),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        for file in ["Cargo.toml", "Cargo.lock", "crates/one/src/lib.rs"] {
            fs::write(source.join(file), b"fixture\n").unwrap();
        }
        fs::write(source.join("tools/rtk-adapter/src/lib.rs"), b"fixture\n").unwrap();
        fs::write(
            source.join("global/profile.toml"),
            "approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\nmodel = 'gpt-6-astra'\n",
        )
        .unwrap();
        fs::write(
            source.join("global/instructions.md"),
            "Owned native core acceptance. Preserve foreign data.\n",
        )
        .unwrap();
        for file in ["global/hooks.json", "global/token-hooks.json"] {
            fs::write(source.join(file), b"{}\n").unwrap();
        }
        fs::write(
            source.join("skills/one/SKILL.md"),
            "---\nname: one\ndescription: Owned acceptance skill.\n---\nPreserve foreign data.\n",
        )
        .unwrap();
        fs::write(
            source.join("global/kit.json"),
            serde_json::to_vec(&json!({
                "schema": 1,
                "profile_name": "harness",
                "profile": "global/profile.toml",
                "instructions": "global/instructions.md",
                "skills": "skills",
                "agents": "global/agents",
                "hooks": "global/hooks.json",
                "token_hooks": "global/token-hooks.json"
            }))
            .unwrap(),
        )
        .unwrap();
        let compile_root = root.join("compile");
        let launcher = compile(&compile_root, "launcher", LAUNCHER);
        fs::copy(&launcher, build.join("codex.exe")).unwrap();
        for name in build_identity::BINARIES {
            let path = build.join(name);
            if !path.exists() {
                fs::write(&path, name.as_bytes()).unwrap();
            }
        }
        let record = build_identity::BuildRecord {
            schema: build_identity::SCHEMA,
            source_root: source.clone(),
            source: build_identity::source_identity(&source).unwrap(),
            rustc: "fixture".into(),
            cargo: "fixture".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
            binaries: build_identity::BINARIES
                .iter()
                .map(|name| {
                    (
                        (*name).to_owned(),
                        build_identity::hash_file(&build.join(name)).unwrap(),
                    )
                })
                .collect(),
        };
        fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        Tree {
            upstream: compile(&compile_root, "upstream-a", &upstream("0.153.4")),
            replacement: compile(&compile_root, "upstream-b", &upstream("0.154.0")),
            cpu: root.join("cpu-account"),
            heavy: root.join("heavy-account"),
            root,
            source,
            build,
        }
    }

    fn upstream(version: &str) -> String {
        format!(
            r#"fn main() {{
    let args: Vec<String> = std::env::args().skip(1).collect();
    let view: Vec<&str> = args.iter().map(String::as_str).collect();
    match view.as_slice() {{
        ["--version"] => println!("codex-cli {version}"),
        ["--help"] => println!("usage --profile <name>.config.toml"),
        ["features", "disable", name] => {{
            let home = std::path::PathBuf::from(std::env::var_os("CODEX_HOME").expect("home"));
            let path = home.join("config.toml");
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let mut lines: Vec<String> = text
                .lines()
                .filter(|line| !line.trim().starts_with(&format!("{{name}} ")))
                .map(str::to_owned)
                .collect();
            lines.push(format!("{{name}} = false"));
            std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        }}
        ["features", "list"] => {{
            println!("hooks stable false");
            println!("code_mode stable true");
        }}
        _ => std::process::exit(1),
    }}
}}
"#
        )
    }

    const LAUNCHER: &str = r#"fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let view: Vec<&str> = args.iter().map(String::as_str).collect();
    match view.as_slice() {
        ["--retained-session"] => std::thread::sleep(std::time::Duration::from_secs(180)),
        ["debug", "prompt-input"] => {
            let home = std::path::PathBuf::from(std::env::var_os("CODEX_HOME").expect("home"));
            let instructions = std::fs::read_to_string(home.join("AGENTS.md")).unwrap_or_default();
            let permissions = "Filesystem sandboxing defines which files can be read or written. sandbox_mode is danger-full-access. Approval policy is currently never.";
            println!(
                "[{{\"type\":\"message\",\"text\":\"{}\"}},{{\"type\":\"message\",\"text\":\"{}\"}}]",
                escape(&instructions),
                escape(permissions)
            );
        }
        _ => std::process::exit(1),
    }
}
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
"#;

    fn command(tree: &Tree, args: &[OsString]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(args)
            .env("CODEX_HARNESS_CPU_ACCOUNT", &tree.cpu)
            .env("CODEX_HARNESS_HEAVY_ACCOUNT", &tree.heavy)
            .env_remove("CODEX_HARNESS_CPU_PERCENT")
            .current_dir(&tree.root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}\n{}",
            args.first()
                .map(|arg| arg.to_string_lossy())
                .unwrap_or_default(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn lifecycle(
        tree: &Tree,
        verb: &str,
        preview: bool,
        home: &Path,
        user: &Path,
        upstream: &Path,
    ) -> Value {
        let mut args = vec![
            OsString::from(verb),
            OsString::from("--core-only"),
            OsString::from("--source"),
            tree.source.clone().into(),
            OsString::from("--build"),
            tree.build.clone().into(),
            OsString::from("--codex-home"),
            home.to_owned().into(),
            OsString::from("--user-home"),
            user.to_owned().into(),
            OsString::from("--dependency-user-home"),
            user.to_owned().into(),
            OsString::from("--upstream"),
            upstream.to_owned().into(),
            OsString::from("--path-scope"),
            OsString::from("process"),
            OsString::from("--timeout-seconds"),
            OsString::from("90"),
        ];
        if preview {
            args.push(OsString::from("--preview"));
        }
        let output = command(tree, &args);
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn policy_bytes(tree: &Tree) -> Vec<u8> {
        fs::read(tree.cpu.join("shared-cpu-policy.json")).unwrap()
    }

    fn assert_not_sampled(report: &Value) {
        let policy = &report["cpu_policy"];
        assert_eq!(policy["measured_consumption"], "not-sampled");
        assert_eq!(policy["model_calls"], 0);
        assert_eq!(report["model_calls"], 0);
        assert_eq!(policy["escape_hatch"], "CODEX_HARNESS_CPU_PERCENT");
        assert!(
            policy["kernel_configuration"]
                .as_str()
                .unwrap()
                .contains("not measured consumption")
                || policy["kernel_configuration"]
                    .as_str()
                    .unwrap()
                    .contains("did not create a job")
        );
    }

    #[test]
    fn missing_check_does_not_create_policy_homes_or_jobs() {
        let root = tempfile::Builder::new()
            .prefix("cpu-policy-absent-")
            .tempdir()
            .unwrap();
        let home = root.path().join("missing-codex");
        let user = root.path().join("missing-user");
        let cpu = root.path().join("cpu-account");
        let heavy = root.path().join("heavy-account");
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args([
                "check",
                "--core-only",
                "--codex-home",
                home.to_str().unwrap(),
                "--user-home",
                user.to_str().unwrap(),
                "--dependency-user-home",
                user.to_str().unwrap(),
                "--timeout-seconds",
                "5",
            ])
            .env("CODEX_HARNESS_CPU_ACCOUNT", &cpu)
            .env("CODEX_HARNESS_HEAVY_ACCOUNT", &heavy)
            .env_remove("CODEX_HARNESS_CPU_PERCENT")
            .current_dir(root.path())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(!cpu.exists());
        assert!(!heavy.exists());
        assert!(!home.exists());
        assert!(!user.exists());
        assert!(!cpu.join("cpu-budget.json").exists());
    }

    #[test]
    fn install_update_homes_sessions_and_rollback_preserve_policy_and_work() {
        let tree = tree();
        let legacy = heavy_command::Budget {
            cpu_percent: Some(50.0),
            ..heavy_command::Budget::default()
        };
        heavy_command::Budget::write(&tree.heavy, &legacy).unwrap();
        let legacy_bytes = fs::read(heavy_command::policy_path(&tree.heavy)).unwrap();
        let legacy_text = heavy_command::cpu_policy_summary(&legacy);
        let home = tree.root.join("codex-a");
        let user = tree.root.join("user-a");
        let other_home = tree.root.join("codex-b");
        let other_user = tree.root.join("user-b");

        let preview = lifecycle(&tree, "install", true, &home, &user, &tree.upstream);
        assert_eq!(preview["status"], "preview");
        assert_eq!(preview["cpu_policy"]["action"], "preview");
        assert_eq!(preview["cpu_policy"]["wrote_policy"], false);
        assert_eq!(preview["cpu_policy"]["activation"], "incomplete");
        assert_not_sampled(&preview);
        assert!(!tree.cpu.exists());
        assert!(!home.exists());
        assert!(!user.exists());
        assert_eq!(
            fs::read(heavy_command::policy_path(&tree.heavy)).unwrap(),
            legacy_bytes
        );

        let installed = lifecycle(&tree, "install", false, &home, &user, &tree.upstream);
        assert_eq!(installed["status"], "connected");
        assert_eq!(installed["cpu_policy"]["action"], "established");
        assert_eq!(installed["cpu_policy"]["wrote_policy"], true);
        assert_eq!(installed["cpu_policy"]["ceiling_percent"], 75.0);
        assert_eq!(installed["cpu_policy"]["activation"], "complete");
        assert_eq!(installed["cpu_policy"]["legacy_heavy_policy"], legacy_text);
        assert_not_sampled(&installed);
        let created: Value = serde_json::from_slice(&policy_bytes(&tree)).unwrap();
        assert_eq!(created["schema"], 1);
        assert_eq!(created["ceiling_percent"], 75.0);
        assert_eq!(created["escape_hatch"], "CODEX_HARNESS_CPU_PERCENT");
        assert!(created["note"].as_str().unwrap().contains("escape hatch"));
        assert!(!tree.cpu.join("cpu-budget.json").exists());
        assert!(
            !home.join("harness/installation.json").exists()
                || !fs::read_to_string(home.join("harness/installation.json"))
                    .unwrap()
                    .contains("ceiling_percent")
        );
        assert!(
            !fs::read_to_string(home.join("harness/installation.json"))
                .unwrap()
                .contains("CODEX_HARNESS_CPU_PERCENT")
        );
        assert_eq!(
            fs::read(heavy_command::policy_path(&tree.heavy)).unwrap(),
            legacy_bytes
        );

        let ambiguous = serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "ceiling_percent": 50.0,
            "edited": "ambiguous-legacy"
        }))
        .unwrap();
        fs::write(tree.cpu.join("shared-cpu-policy.json"), &ambiguous).unwrap();
        let repeated = lifecycle(&tree, "update", false, &home, &user, &tree.upstream);
        assert_eq!(repeated["cpu_policy"]["action"], "preserved");
        assert_eq!(repeated["cpu_policy"]["wrote_policy"], false);
        assert_eq!(repeated["cpu_policy"]["ambiguous_shared_ceiling"], true);
        assert_eq!(repeated["cpu_policy"]["legacy_heavy_policy"], legacy_text);
        assert_eq!(policy_bytes(&tree), ambiguous);
        assert_eq!(
            fs::read(heavy_command::policy_path(&tree.heavy)).unwrap(),
            legacy_bytes
        );

        let edited = serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "ceiling_percent": 40.0,
            "owner_note": "kept by the user"
        }))
        .unwrap();
        fs::write(tree.cpu.join("shared-cpu-policy.json"), &edited).unwrap();
        let preview_edit = lifecycle(&tree, "update", true, &home, &user, &tree.upstream);
        assert_eq!(preview_edit["status"], "preview");
        assert_eq!(preview_edit["cpu_policy"]["action"], "preview");
        assert_eq!(preview_edit["cpu_policy"]["wrote_policy"], false);
        assert_eq!(preview_edit["cpu_policy"]["ceiling_percent"], 40.0);
        assert_eq!(policy_bytes(&tree), edited);
        let preserved = lifecycle(&tree, "update", false, &home, &user, &tree.upstream);
        assert_eq!(preserved["cpu_policy"]["action"], "preserved");
        assert_eq!(preserved["cpu_policy"]["ceiling_percent"], 40.0);
        assert_eq!(preserved["cpu_policy"]["ambiguous_shared_ceiling"], false);
        assert_eq!(preserved["cpu_policy"]["wrote_policy"], false);
        assert_eq!(policy_bytes(&tree), edited);

        let first_metadata = fs::read(home.join("harness/installation.json")).unwrap();
        let other = lifecycle(
            &tree,
            "install",
            false,
            &other_home,
            &other_user,
            &tree.upstream,
        );
        assert_eq!(other["status"], "connected");
        assert_eq!(other["cpu_policy"]["action"], "preserved");
        assert_eq!(other["cpu_policy"]["ceiling_percent"], 40.0);
        assert_eq!(policy_bytes(&tree), edited);
        assert_eq!(
            fs::read(home.join("harness/installation.json")).unwrap(),
            first_metadata
        );
        assert!(
            !fs::read_to_string(other_home.join("harness/installation.json"))
                .unwrap()
                .contains("ceiling_percent")
        );

        let mut sleeper = Sleeper(
            Command::new(tree.build.join("codex.exe"))
                .arg("--retained-session")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let pid = sleeper.0.id();
        let active = lifecycle(&tree, "update", false, &home, &user, &tree.upstream);
        assert_eq!(active["cpu_policy"]["activation"], "incomplete");
        assert_eq!(active["cpu_policy"]["wrote_policy"], false);
        let boundary = active["cpu_policy"]["restart_boundary"].as_str().unwrap();
        assert!(boundary.contains(&pid.to_string()), "{boundary}");
        assert!(boundary.contains("does not terminate"), "{boundary}");
        assert!(boundary.contains("codex.exe"), "{boundary}");
        assert!(sleeper.0.try_wait().unwrap().is_none());
        assert_eq!(policy_bytes(&tree), edited);
        assert!(!tree.cpu.join("cpu-budget.json").exists());

        // Process-scope installation records the caller's PATH. Check is a new
        // process, so it must already contain harness/bin; the check itself
        // must not add it.
        let mut path = OsString::from(home.join("harness/bin"));
        if let Some(existing) = std::env::var_os("PATH") {
            path.push(";");
            path.push(existing);
        }
        let checked = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args([
                "check",
                "--core-only",
                "--codex-home",
                home.to_str().unwrap(),
                "--user-home",
                user.to_str().unwrap(),
                "--dependency-user-home",
                user.to_str().unwrap(),
                "--timeout-seconds",
                "90",
            ])
            .env("PATH", &path)
            .env("CODEX_HARNESS_CPU_ACCOUNT", &tree.cpu)
            .env("CODEX_HARNESS_HEAVY_ACCOUNT", &tree.heavy)
            .env_remove("CODEX_HARNESS_CPU_PERCENT")
            .current_dir(&tree.root)
            .output()
            .unwrap();
        assert!(
            checked.status.success(),
            "{}",
            String::from_utf8_lossy(&checked.stderr)
        );
        let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
        assert_eq!(checked["status"], "connected");
        assert_eq!(checked["cpu_policy"]["action"], "inspected");
        assert_eq!(checked["cpu_policy"]["wrote_policy"], false);
        assert_eq!(checked["cpu_policy"]["activation"], "incomplete");
        assert_eq!(checked["cpu_policy"]["ceiling_percent"], 40.0);
        assert!(
            checked["cpu_policy"]["kernel_configuration"]
                .as_str()
                .unwrap()
                .contains("did not create a job")
        );
        assert_not_sampled(&checked);
        assert_eq!(policy_bytes(&tree), edited);
        assert!(sleeper.0.try_wait().unwrap().is_none());

        let before_launch = fs::read(home.join("harness/native-launch.json")).unwrap();
        let updated = lifecycle(&tree, "update", false, &home, &user, &tree.replacement);
        assert_eq!(updated["cpu_policy"]["action"], "preserved");
        assert_eq!(policy_bytes(&tree), edited);
        assert_ne!(
            fs::read(home.join("harness/native-launch.json")).unwrap(),
            before_launch
        );
        assert!(sleeper.0.try_wait().unwrap().is_none());
        assert_eq!(
            fs::read(heavy_command::policy_path(&tree.heavy)).unwrap(),
            legacy_bytes
        );

        let preview_disconnect = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args([
                "disconnect",
                "--core-only",
                "--preview",
                "--codex-home",
                home.to_str().unwrap(),
                "--user-home",
                user.to_str().unwrap(),
                "--dependency-user-home",
                user.to_str().unwrap(),
            ])
            .env("CODEX_HARNESS_CPU_ACCOUNT", &tree.cpu)
            .env("CODEX_HARNESS_HEAVY_ACCOUNT", &tree.heavy)
            .current_dir(&tree.root)
            .output()
            .unwrap();
        assert!(
            preview_disconnect.status.success(),
            "{}",
            String::from_utf8_lossy(&preview_disconnect.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&preview_disconnect.stderr).contains("no longer provided")
        );
        assert_eq!(policy_bytes(&tree), edited);
        assert!(sleeper.0.try_wait().unwrap().is_none());

        let disconnected = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args([
                "disconnect",
                "--core-only",
                "--codex-home",
                home.to_str().unwrap(),
                "--user-home",
                user.to_str().unwrap(),
                "--dependency-user-home",
                user.to_str().unwrap(),
            ])
            .env("CODEX_HARNESS_CPU_ACCOUNT", &tree.cpu)
            .env("CODEX_HARNESS_HEAVY_ACCOUNT", &tree.heavy)
            .current_dir(&tree.root)
            .output()
            .unwrap();
        assert!(
            disconnected.status.success(),
            "{}",
            String::from_utf8_lossy(&disconnected.stderr)
        );
        let stderr = String::from_utf8_lossy(&disconnected.stderr);
        assert!(
            stderr.contains("default coverage is no longer provided"),
            "{stderr}"
        );
        assert!(stderr.contains("not-sampled"), "{stderr}");
        assert!(stderr.contains(&pid.to_string()), "{stderr}");
        assert_eq!(policy_bytes(&tree), edited);
        assert_eq!(
            fs::read(heavy_command::policy_path(&tree.heavy)).unwrap(),
            legacy_bytes
        );
        assert!(sleeper.0.try_wait().unwrap().is_none());
        assert!(!tree.cpu.join("cpu-budget.json").exists());
        assert!(!tree.source.join("target").exists());
    }
}
