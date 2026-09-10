//! Native Diagnose entrypoint; deterministic protocol doubles and explicit real CLI.
#![cfg(windows)]
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    user: PathBuf,
    project: PathBuf,
    source: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("source-observation проверка-")
            .tempdir()
            .unwrap();
        let home = root.path().join("home");
        let user = root.path().join("user");
        let project = root.path().join("project");
        let source = root.path().join("source");
        for dir in [&home, &user, &project] {
            fs::create_dir(dir).unwrap();
        }
        fs::write(
            home.join("config.toml"),
            "model = 'gpt-6-astra'\ncheck_for_update_on_startup = false\n",
        )
        .unwrap();
        fs::write(
            home.join("harness.config.toml"),
            "model = 'gpt-6-astra'\nmodel_reasoning_effort = 'xhigh'\n",
        )
        .unwrap();
        fs::write(home.join("auth-sentinel"), "unrelated auth sentinel").unwrap();
        fs::write(home.join("history.jsonl"), "unrelated history sentinel").unwrap();
        for relative in ["global/agents", "skills/one"] {
            fs::create_dir_all(source.join(relative)).unwrap();
        }
        fs::write(
            source.join("global/profile.toml"),
            "model = 'gpt-6-astra'\nmodel_reasoning_effort = 'xhigh'\n",
        )
        .unwrap();
        fs::write(
            source.join("global/instructions.md"),
            "Owned source observation acceptance.\n",
        )
        .unwrap();
        for name in ["hooks.json", "token-hooks.json"] {
            fs::write(source.join("global").join(name), "{}").unwrap();
        }
        fs::write(
            source.join("skills/one/SKILL.md"),
            "---\nname: one\ndescription: Owned inert skill data.\n---\nPreserve source data.\n",
        )
        .unwrap();
        fs::write(source.join("global/kit.json"), serde_json::to_vec(&json!({"schema":1,"profile_name":"harness",
            "profile":"global/profile.toml","instructions":"global/instructions.md","skills":"skills",
            "agents":"global/agents","hooks":"global/hooks.json","token_hooks":"global/token-hooks.json"})).unwrap()).unwrap();
        Self {
            root,
            home,
            user,
            project,
            source,
        }
    }
    fn command(&self, upstream: &Path) -> Command {
        self.entry_command(upstream, &["diagnose"])
    }
    fn entry_command(&self, upstream: &Path, entry: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        command
            .args(entry)
            .arg("--source")
            .arg(&self.source)
            .arg("--codex-home")
            .arg(&self.home)
            .arg("--user-home")
            .arg(&self.user)
            .arg("--project")
            .arg(&self.project)
            .arg("--upstream")
            .arg(upstream)
            .current_dir(self.root.path());
        command
    }

    fn legacy_connect(&self, upstream: &Path) {
        fs::remove_file(self.home.join("harness.config.toml")).unwrap();
        let inventory =
            harness_core::inventory::read(&self.source, &self.home, &self.user).unwrap();
        let mut links = Vec::new();
        for link in inventory.links {
            let text = link.source.to_str().unwrap();
            let source = PathBuf::from(text.strip_prefix("\\\\?\\").unwrap_or(text));
            fs::create_dir_all(link.destination.parent().unwrap()).unwrap();
            if source.is_dir() {
                std::os::windows::fs::symlink_dir(&source, &link.destination).unwrap();
            } else {
                std::os::windows::fs::symlink_file(&source, &link.destination).unwrap();
            }
            links.push(json!({"kind":link.kind,"name":link.name,"source":source,"destination":link.destination,"owned":true}));
        }
        fs::create_dir_all(self.home.join("harness")).unwrap();
        fs::write(self.home.join("harness/installation.json"), serde_json::to_vec(&json!({"schemaVersion":1,
            "sourceRoot":self.source,"codexHome":self.home,"userHome":self.user,"dependencyUserHome":self.user,
            "codexCommand":upstream,"profileName":"harness","links":links,"pathScope":"Process","pathAdded":false,"versions":{}})).unwrap()).unwrap();
    }
}

#[test]
fn native_failure_keeps_independent_links_withholds_raw_errors_and_bounds_shutdown() {
    for mode in [
        "no-read",
        "init-error",
        "malformed",
        "unknown-id",
        "truncated",
    ] {
        let fixture = Fixture::new();
        let paths = [
            "config.toml",
            "harness.config.toml",
            "auth-sentinel",
            "history.jsonl",
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|p| fs::read(fixture.home.join(p)).unwrap())
            .collect();
        let started = Instant::now();
        let pid_file = fixture.root.path().join("native-started.pid");
        let output = fixture
            .command(Path::new(env!("CARGO_BIN_EXE_harness-launch-fixture")))
            .args(["--timeout-seconds", "1"])
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "discovery")
            .env("HARNESS_DISCOVERY_FIXTURE", mode)
            .env("HARNESS_LAUNCH_FIXTURE_STARTED", &pid_file)
            .output()
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "diagnostic shutdown: {mode}"
        );
        assert!(
            output.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["status"], "incomplete");
        assert_eq!(report["model_calls"], 0);
        assert!(!report["links"].as_array().unwrap().is_empty());
        assert_eq!(report["freshness"]["existingSessions"], "unknown");
        let expected = if mode == "no-read" {
            "native-timeout"
        } else {
            "native-unavailable-or-incompatible"
        };
        assert!(
            report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["code"] == expected),
            "{mode}: {report}"
        );
        for private in [
            "private discovery fixture sentinel",
            "invalid JSON private content",
            "unrelated auth sentinel",
        ] {
            assert!(!String::from_utf8_lossy(&output.stdout).contains(private));
            assert!(!String::from_utf8_lossy(&output.stderr).contains(private));
        }
        for (index, path) in paths.iter().enumerate() {
            assert_eq!(fs::read(fixture.home.join(path)).unwrap(), before[index]);
        }
        assert!(!fixture.home.join("harness").exists());
        assert!(!fixture.project.join("fixture-requests.jsonl").exists());
        let pid: u32 = fs::read_to_string(pid_file).unwrap().parse().unwrap();
        assert_exited(pid);
    }
}

#[test]
fn invalid_options_and_profile_paths_fail_without_starting_an_upstream() {
    let fixture = Fixture::new();
    let pid_file = fixture.root.path().join("unexpected-start.pid");
    for extra in [
        vec!["--profile", "../outside"],
        vec!["--timeout-seconds", "0"],
        vec!["--timeout-seconds", "61"],
        vec!["--unknown"],
    ] {
        let output = fixture
            .command(Path::new(env!("CARGO_BIN_EXE_harness-launch-fixture")))
            .args(extra)
            .env("HARNESS_LAUNCH_FIXTURE_STARTED", &pid_file)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!pid_file.exists());
        assert!(!fixture.home.join("harness").exists());
    }
}

#[test]
fn check_diagnose_keeps_its_report_and_rejects_mixed_selectors_before_execution() {
    let fixture = Fixture::new();
    let upstream = Path::new(env!("CARGO_BIN_EXE_harness-launch-fixture"));
    let output = fixture
        .entry_command(upstream, &["check", "--diagnose"])
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "discovery")
        .env("HARNESS_DISCOVERY_FIXTURE", "init-error")
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["model_calls"], 0);
    assert_eq!(report["status"], "incomplete");
    assert!(!report["links"].as_array().unwrap().is_empty());
    let pid_file = fixture.root.path().join("unexpected-check-start.pid");
    for flag in ["--core-only", "--diagnose"] {
        let output = fixture
            .entry_command(upstream, &["check", "--diagnose"])
            .arg(flag)
            .env("HARNESS_LAUNCH_FIXTURE_STARTED", &pid_file)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!pid_file.exists());
    }
    assert!(!fixture.home.join("harness").exists());
}

fn assert_exited(pid: u32) {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError, WAIT_OBJECT_0},
        System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    };
    let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if process.is_null() {
        assert_eq!(unsafe { GetLastError() }, ERROR_INVALID_PARAMETER);
    } else {
        let status = unsafe { WaitForSingleObject(process, 0) };
        unsafe { CloseHandle(process) };
        assert_eq!(status, WAIT_OBJECT_0, "owned diagnostic process survived");
    }
}

#[test]
fn relative_project_resolves_before_starting_the_isolated_consumer() {
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args([
            "diagnose",
            "--project",
            ".",
            "--timeout-seconds",
            "1",
            "--codex-home",
        ])
        .arg(&fixture.home)
        .arg("--user-home")
        .arg(&fixture.user)
        .arg("--source")
        .arg(&fixture.source)
        .arg("--upstream")
        .arg(env!("CARGO_BIN_EXE_harness-launch-fixture"))
        .current_dir(&fixture.project)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "discovery")
        .env("HARNESS_DISCOVERY_FIXTURE", "init-error")
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["project"], json!(fixture.project));
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["code"] == "native-unavailable-or-incompatible")
    );
}

#[test]
#[ignore = "explicit installed original CLI; owned legacy diagnostic source/home/project, no models"]
fn actual_native_layers_conflicts_privacy_and_restoration() {
    let upstream =
        PathBuf::from(std::env::var_os("HARNESS_SOURCE_REAL_CLI").expect("explicit original CLI"));
    let fixture = Fixture::new();
    fixture.legacy_connect(&upstream);
    let git =
        PathBuf::from(std::env::var_os("HARNESS_SOURCE_REAL_GIT").expect("explicit installed Git"));
    assert!(
        Command::new(git)
            .args(["init", "--quiet"])
            .arg(&fixture.project)
            .status()
            .unwrap()
            .success()
    );
    let config = format!(
        "[projects.'{}']\ntrust_level = 'trusted'\n[features]\nhooks = true\n",
        fixture.project.display()
    );
    fs::write(fixture.home.join("config.toml"), &config).unwrap();
    let hook_marker = fixture.root.path().join("hook-ran");
    fs::write(fixture.home.join("hooks.json"), serde_json::to_vec(&json!({"hooks":{"SessionStart":[{"hooks":[{
        "type":"command","command":format!("\"{}\"",env!("CARGO_BIN_EXE_harness-launch-fixture")),"timeout":5}]}]}})).unwrap()).unwrap();
    let paths = [
        "config.toml",
        "harness.config.toml",
        "auth-sentinel",
        "history.jsonl",
        "harness/installation.json",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|p| fs::read(fixture.home.join(p)).unwrap())
        .collect();
    let observe = |label: &str| {
        let output = fixture
            .command(&upstream)
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "delayed")
            .env("HARNESS_LAUNCH_FIXTURE_MARKER", &hook_marker)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_SOURCE_SENTINEL"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_SOURCE_SENTINEL"));
        assert!(
            !hook_marker.exists(),
            "source reads invoked a SessionStart hook"
        );
        assert_eq!(report["model_calls"], 0);
        fs::write(
            fixture.root.path().join(format!("{label}.json")),
            &output.stdout,
        )
        .unwrap();
        report
    };
    let clean = observe("clean");
    assert_eq!(clean["native"]["status"], "observed", "{clean}");
    assert_eq!(clean["status"], "healthy", "{clean}");
    let setting = |report: &Value, key: &str| {
        report["settings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|setting| setting["key"] == key)
            .unwrap()
            .clone()
    };
    assert_eq!(
        setting(&clean, "model_reasoning_effort")["origin"]["profile"],
        "harness"
    );
    assert_eq!(setting(&clean, "model_reasoning_effort")["value"], "xhigh");
    for (index, path) in paths.iter().enumerate() {
        assert_eq!(fs::read(fixture.home.join(path)).unwrap(), before[index]);
    }
    fs::create_dir_all(fixture.project.join(".codex")).unwrap();
    let project_config = fixture.project.join(".codex/config.toml");
    fs::write(&project_config, "model_reasoning_effort = 'low'\ndeveloper_instructions = 'PRIVATE_SOURCE_SENTINEL'\n[features]\nhooks = false\n").unwrap();
    let original = clean["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|skill| skill["enabled"] == true)
        .expect("native default skill catalogue");
    let duplicate = fixture.project.join(".agents/skills/duplicate");
    fs::create_dir_all(&duplicate).unwrap();
    fs::write(
        duplicate.join("SKILL.md"),
        format!(
            "---\nname: {}\ndescription: PRIVATE_SOURCE_SENTINEL\n---\nPRIVATE_SOURCE_SENTINEL\n",
            original["name"].as_str().unwrap()
        ),
    )
    .unwrap();
    let instructions = fixture.home.join("AGENTS.md");
    let alternate = fixture.root.path().join("alternate.md");
    fs::write(&alternate, "Other source.\n").unwrap();
    fs::remove_file(&instructions).unwrap();
    std::os::windows::fs::symlink_file(&alternate, &instructions).unwrap();
    let conflicted = observe("conflicts");
    assert_eq!(conflicted["status"], "attention", "{conflicted}");
    for code in [
        "setting-overridden",
        "skill-name-collision",
        "link-retargeted",
    ] {
        assert!(
            conflicted["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["code"] == code),
            "missing {code}"
        );
    }
    assert_eq!(
        setting(&conflicted, "model_reasoning_effort")["value"],
        "low"
    );
    assert_eq!(setting(&conflicted, "features.hooks")["value"], false);
    fs::remove_file(duplicate.join("SKILL.md")).unwrap();
    fs::remove_file(&project_config).unwrap();
    fs::remove_file(&instructions).unwrap();
    std::os::windows::fs::symlink_file(
        fixture.source.join("global/instructions.md"),
        &instructions,
    )
    .unwrap();
    assert_eq!(observe("restored")["status"], "healthy");
    fs::write(&project_config, "model_reasoning_effort = 'low'\n").unwrap();
    fs::write(
        fixture.home.join("config.toml"),
        format!(
            "[projects.'{}']\ntrust_level = 'untrusted'\n",
            fixture.project.display()
        ),
    )
    .unwrap();
    let untrusted = observe("untrusted");
    assert_eq!(
        setting(&untrusted, "model_reasoning_effort")["value"],
        "xhigh"
    );
    assert!(
        untrusted["layers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|layer| layer["status"] == "disabled")
    );
    let profile = fixture.source.join("global/profile.toml");
    let profile_before = fs::read(&profile).unwrap();
    fs::write(
        &profile,
        format!(
            "model='gpt-6-astra'\n[projects.'{}']\ntrust_level='trusted'\n",
            fixture.project.display()
        ),
    )
    .unwrap();
    let context = observe("profile-context");
    assert_eq!(context["status"], "incomplete");
    assert!(
        context["settings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["origin"].is_null())
    );
    fs::write(&profile, profile_before).unwrap();
    fs::write(
        fixture.home.join("config.toml"),
        "invalid PRIVATE_SOURCE_SENTINEL",
    )
    .unwrap();
    let invalid = observe("invalid-config");
    assert_eq!(invalid["status"], "incomplete");
    assert!(!invalid["links"].as_array().unwrap().is_empty());
    fs::write(fixture.home.join("config.toml"), config).unwrap();
    for (index, path) in paths.iter().enumerate() {
        assert_eq!(fs::read(fixture.home.join(path)).unwrap(), before[index]);
    }
    println!(
        "actual source diagnostics evidence: {}",
        fixture.root.keep().display()
    );
}
