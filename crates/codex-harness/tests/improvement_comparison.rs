//! Entry-point checks for one declared sequential baseline/candidate
//! comparison. A real `bd` board, a real OpenSpec workspace, real Git
//! snapshots, the real runtime installation owner, a real local HTTP
//! observation fixture, a compiled checker program and the unchanged
//! `outcome-oracle` entry point are exercised; the two model conversations
//! are simulated through the same durable seam the controller recovery uses:
//! a dispatcher receipt with a terminal state is seeded and the controller
//! settles and consumes it on `resume`. No check here contacts a model or a
//! provider.
#![cfg(windows)]

use harness_core::{build_identity, task_worktree};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

/// Serializes the arm-installing cases: the installation owner publishes
/// process-local PATH entries and holds shared installation locks.
static INSTALL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn bd_name() -> &'static str {
    "bd.exe"
}

fn bd_executable() -> PathBuf {
    if let Some(value) = std::env::var_os("HARNESS_BD_EXE") {
        return PathBuf::from(value);
    }
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        let candidate = PathBuf::from(home).join("harness/bin").join(bd_name());
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(bd_name());
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_output(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Runs one installed OpenSpec command exactly as the native adapter does.
fn openspec(cwd: &Path, args: &[&str]) -> Output {
    let mut command = Command::new("pwsh");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-CommandWithArgs",
        "& openspec @args; exit $LASTEXITCODE",
    ]);
    command.args(args).current_dir(cwd);
    command.env("OPENSPEC_TELEMETRY", "0");
    command.output().expect("pwsh runs the openspec CLI")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

/// Model-free stand-in for the original Codex client, as the installation
/// owner's runtime check and the arm configuration exercise it.
const UPSTREAM_SOURCE: &str = r##"
use std::{env, fs, path::PathBuf, process::exit};

fn home() -> PathBuf {
    PathBuf::from(env::var_os("CODEX_HOME").expect("CODEX_HOME"))
}

fn set_hooks(text: &mut String, enabled: bool) {
    let mut replaced = String::new();
    let mut in_features = false;
    let mut seen_features = false;
    let mut done = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            if in_features && !done {
                replaced.push_str(&format!("hooks = {enabled}\n"));
                done = true;
            }
            in_features = trimmed == "[features]";
            seen_features |= in_features;
        }
        if in_features && !done && trimmed.starts_with("hooks") && line.contains('=') {
            replaced.push_str(&format!("hooks = {enabled}\n"));
            done = true;
            continue;
        }
        replaced.push_str(line);
        replaced.push('\n');
    }
    if in_features && !done {
        replaced.push_str(&format!("hooks = {enabled}\n"));
        done = true;
    }
    *text = if done {
        replaced
    } else if seen_features {
        format!("{replaced}hooks = {enabled}\n")
    } else {
        format!("{replaced}\n[features]\nhooks = {enabled}\n")
    };
}

fn hooks_state(text: &str) -> bool {
    text.lines()
        .rev()
        .find(|line| line.trim_start().starts_with("hooks") && line.contains('='))
        .and_then(|line| line.split('=').nth(1))
        .map(|value| value.trim() == "true")
        .unwrap_or(false)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => println!("codex-cli 0.160.0"),
        Some("--help") => println!(
            "Codex CLI\n\nUsage: codex [OPTIONS]\n      --profile <PROFILE>  Configuration profile from config.toml\n      Select <name>.config.toml with --profile.\n"
        ),
        Some("features") => {
            let config = home().join("config.toml");
            let mut text = fs::read_to_string(&config).unwrap_or_default();
            match (args.get(1).map(String::as_str), args.get(2).map(String::as_str)) {
                (Some("disable"), Some("hooks")) => {
                    set_hooks(&mut text, false);
                    fs::write(&config, text).unwrap();
                }
                (Some("enable"), Some("hooks")) => {
                    set_hooks(&mut text, true);
                    fs::write(&config, text).unwrap();
                }
                (Some("list"), _) => {
                    println!("hooks stable {}", hooks_state(&text));
                    println!("code_mode experimental false");
                }
                _ => exit(2),
            }
        }
        _ => exit(2),
    }
}
"##;

/// Model-free stand-in for the installed launcher of an arm. It answers the
/// installation owner's prompt diagnostic (installed instructions plus the
/// shared permission defaults) but deliberately does not expose the
/// executor-shell prompt contract, so the visible dispatch owner refuses the
/// arm before submission and no console, tab or model request is created. The
/// checks seed the conversation receipts instead; real launcher execution is
/// parent acceptance.
const LAUNCHER_SOURCE: &str = r##"
use std::{env, fs, path::PathBuf, process::exit};

fn escape(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("debug")
        || args.get(1).map(String::as_str) != Some("prompt-input")
    {
        exit(2);
    }
    let home = PathBuf::from(env::var_os("CODEX_HOME").expect("CODEX_HOME"));
    let instructions = fs::read_to_string(home.join("AGENTS.md")).expect("installed instructions");
    let permissions = "Filesystem sandboxing defines which files can be read or written. `sandbox_mode` is `danger-full-access`: No filesystem sandboxing - all commands are permitted.\nApproval policy is currently never.";
    println!(
        "[{{\"role\":\"developer\",\"content\":[{{\"type\":\"input_text\",\"text\":{}}},{{\"type\":\"input_text\",\"text\":{}}}]}}]",
        escape(&instructions),
        escape(permissions)
    );
}
"##;

/// The frozen independent acceptance checker: it sleeps for the work item's
/// declared duration and accepts only a solved workspace.
const CHECKER_SOURCE: &str = r##"
use std::{env, fs, path::PathBuf, process::exit, thread, time::Duration};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let root = PathBuf::from(args.first().cloned().unwrap_or_default());
    let sleep: u64 = fs::read_to_string(root.join("sleep_ms"))
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);
    if sleep > 0 {
        thread::sleep(Duration::from_millis(sleep));
    }
    let text = fs::read_to_string(root.join("solution.txt")).unwrap_or_default();
    if text.trim() == "solved" {
        exit(0);
    }
    exit(1);
}
"##;

struct FixturePrograms {
    _root: tempfile::TempDir,
    upstream: PathBuf,
    launcher: PathBuf,
    checker: PathBuf,
}

fn programs() -> &'static FixturePrograms {
    static PROGRAMS: OnceLock<FixturePrograms> = OnceLock::new();
    PROGRAMS.get_or_init(|| {
        let root = tempfile::Builder::new()
            .prefix("improvement-comparison-fixtures-")
            .tempdir()
            .unwrap();
        let upstream = compile_fixture(root.path(), "upstream", UPSTREAM_SOURCE);
        let launcher = compile_fixture(root.path(), "launcher", LAUNCHER_SOURCE);
        let checker = compile_fixture(root.path(), "checker", CHECKER_SOURCE);
        FixturePrograms {
            _root: root,
            upstream,
            launcher,
            checker,
        }
    })
}

fn compile_fixture(root: &Path, name: &str, source: &str) -> PathBuf {
    let source_path = root.join(format!("{name}.rs"));
    fs::write(&source_path, source).unwrap();
    let located = Command::new("where.exe").arg("rustc.exe").output().unwrap();
    assert!(located.status.success(), "rustc is required for fixtures");
    let located = String::from_utf8(located.stdout).unwrap();
    let rustc = located.lines().next().unwrap();
    let executable = root.join(format!("{name}.exe"));
    let output = Command::new(rustc)
        .arg(&source_path)
        .arg("--edition=2024")
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture compile failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

/// Synthetic kit source: the layout the installation owner reads, plus the
/// minimal compiled-input tree the build identity records.
fn kit_source(root: &Path, name: &str, marker: &str) -> PathBuf {
    let source = root.join(name);
    for directory in [
        "crates/one/src",
        "global/agents",
        ".agents/skills/arm-skill",
    ] {
        fs::create_dir_all(source.join(directory)).unwrap();
    }
    fs::write(source.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
    fs::write(source.join("Cargo.lock"), "").unwrap();
    fs::write(source.join("crates/one/src/lib.rs"), "pub fn one() {}\n").unwrap();
    fs::write(
        source.join("global/kit.json"),
        serde_json::to_vec(&json!({
            "schema": 1,
            "profile_name": "harness",
            "profile": "global/harness.config.toml",
            "instructions": "global/principles-of-work.md",
            "skills": ".agents/skills",
            "agents": "global/agents",
            "hooks": "global/hooks.json",
            "token_hooks": "global/rtk-hooks.json",
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        source.join("global/harness.config.toml"),
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nweb_search = \"disabled\"\n\n[features]\ncode_mode = true\napps = false\n",
    )
    .unwrap();
    fs::write(
        source.join("global/principles-of-work.md"),
        format!("# Principles\n{marker} arm instructions\n"),
    )
    .unwrap();
    fs::write(source.join("global/hooks.json"), "{}\n").unwrap();
    fs::write(source.join("global/rtk-hooks.json"), "{}\n").unwrap();
    fs::write(
        source.join(".agents/skills/arm-skill/SKILL.md"),
        format!("---\nname: arm-skill\ndescription: Fixture skill.\n---\n\n{marker} skill body\n"),
    )
    .unwrap();
    source
}

/// One published immutable build inside the owned state.
fn fixture_build(
    state: &Path,
    name: &str,
    source: &Path,
    launcher: &Path,
    marker: &str,
) -> PathBuf {
    let build = state.join("builds").join(name);
    fs::create_dir_all(&build).unwrap();
    let mut binaries = BTreeMap::new();
    for binary in build_identity::BINARIES {
        let bytes = if *binary == "codex.exe" {
            fs::read(launcher).unwrap()
        } else {
            format!("{binary} fixture {marker}\n").into_bytes()
        };
        fs::write(build.join(binary), &bytes).unwrap();
        binaries.insert((*binary).to_string(), build_identity::hash_bytes(&bytes));
    }
    let record = build_identity::BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.to_path_buf(),
        source: build_identity::source_identity(source).unwrap(),
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

/// One owned synthetic run: the candidate project with a committed OpenSpec
/// change and an admitted hypothesis card, the frozen workload project, the
/// prepared native state with two builds, the two installation fixture
/// programs and the frozen acceptance request.
struct Fixture {
    _root: tempfile::TempDir,
    root: PathBuf,
    proj: PathBuf,
    wl: PathBuf,
    home: PathBuf,
    run: PathBuf,
    spec: PathBuf,
    bd: PathBuf,
    card: String,
    state: PathBuf,
    baseline_build: PathBuf,
    candidate_build: PathBuf,
    upstream: PathBuf,
    launcher: PathBuf,
    request: PathBuf,
    request_sha256: String,
    policy: PathBuf,
    qualification: PathBuf,
    workload_revision: String,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let cleanup = tempfile::tempdir().unwrap();
        let canonical = cleanup.path().canonicalize().unwrap();
        let root = canonical
            .to_string_lossy()
            .strip_prefix(r"\\?\")
            .map(PathBuf::from)
            .unwrap_or(canonical);
        let proj = root.join(format!("proj-{name}"));
        let wl = root.join(format!("workload-{name}"));
        let home = root.join("codex-home");
        let run = root.join("runs").join(name);
        for directory in [&proj, &wl, &home] {
            fs::create_dir_all(directory).unwrap();
        }
        fs::create_dir_all(run.parent().unwrap()).unwrap();

        // The candidate project: a committed synthetic OpenSpec change and an
        // admitted hypothesis card.
        git(
            &root,
            &[
                "init",
                "-q",
                "--initial-branch=main",
                proj.to_str().unwrap(),
            ],
        );
        git(&proj, &["config", "user.email", "fixture@example.test"]);
        git(&proj, &["config", "user.name", "Fixture"]);
        fs::create_dir_all(proj.join("crates/one/src")).unwrap();
        fs::write(proj.join("crates/one/src/lib.rs"), "// synthetic\n").unwrap();
        fs::create_dir_all(proj.join("global")).unwrap();
        fs::write(
            proj.join("global/orchestration.toml"),
            "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\n",
        )
        .unwrap();
        fs::write(proj.join("README.md"), "synthetic\n").unwrap();
        let init = openspec(
            &proj,
            &["init", "--tools", "none", "--no-animation", "--force"],
        );
        assert!(init.status.success(), "openspec init: {}", text(&init));
        let created = openspec(
            &proj,
            &[
                "new",
                "change",
                "add-synthetic",
                "--schema",
                "spec-driven",
                "--json",
            ],
        );
        assert!(created.status.success(), "openspec new: {}", text(&created));
        write_change(&proj, "add-synthetic", "synthetic", "Synthetic case");
        git(&proj, &["add", "."]);
        git(&proj, &["commit", "-qm", "seed with planning workspace"]);

        let bd = bd_executable();
        let init = Command::new(&bd)
            .args([
                "init",
                "--skip-agents",
                "--non-interactive",
                "--quiet",
                "--prefix",
                "bdcp",
            ])
            .current_dir(&proj)
            .output()
            .expect("bd init runs");
        assert!(
            init.status.success(),
            "bd init: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        fs::write(
            home.join("config.toml"),
            "[profiles.ds]\nmodel = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
        )
        .unwrap();

        // The frozen workload project: its own committed OpenSpec change plus
        // the work item the measured conversations transform.
        git(
            &root,
            &["init", "-q", "--initial-branch=main", wl.to_str().unwrap()],
        );
        git(&wl, &["config", "user.email", "fixture@example.test"]);
        git(&wl, &["config", "user.name", "Fixture"]);
        let init = openspec(
            &wl,
            &["init", "--tools", "none", "--no-animation", "--force"],
        );
        assert!(
            init.status.success(),
            "workload openspec init: {}",
            text(&init)
        );
        let created = openspec(
            &wl,
            &[
                "new",
                "change",
                "add-workload",
                "--schema",
                "spec-driven",
                "--json",
            ],
        );
        assert!(
            created.status.success(),
            "workload openspec new: {}",
            text(&created)
        );
        write_change(&wl, "add-workload", "workload", "Workload case");
        fs::write(wl.join("solution.txt"), "todo\n").unwrap();
        fs::write(wl.join("sleep_ms"), "0\n").unwrap();
        git(&wl, &["add", "."]);
        git(&wl, &["commit", "-qm", "frozen workload snapshot"]);

        // The two prepared builds over two distinct kit sources.
        let state = root.join("state");
        fs::create_dir_all(state.join("builds")).unwrap();
        fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
        let programs = programs();
        let baseline_source = kit_source(&root, "kit-h", "baseline");
        let candidate_source = kit_source(&root, "kit-ha", "candidate");
        let baseline_build = fixture_build(
            &state,
            "h-build",
            &baseline_source,
            &programs.launcher,
            "baseline",
        );
        let candidate_build = fixture_build(
            &state,
            "ha-build",
            &candidate_source,
            &programs.launcher,
            "candidate",
        );

        // The frozen acceptance request: one host-owned checker program, one
        // frozen contract input and the workspace the controller materializes
        // each arm's committed solution into.
        let contract = root.join("task-contract.txt");
        fs::write(&contract, "the workload acceptance contract\n").unwrap();
        let request = root.join("acceptance-request.json");
        let request_bytes = serde_json::to_vec(&json!({
            "schema": 1,
            "kind": "real-task",
            "case_root": root.join("task-workspace"),
            "task_contract_sha256": build_identity::hash_file(&contract).unwrap(),
            "oracle": {
                "program": programs.checker,
                "program_sha256": build_identity::hash_file(&programs.checker).unwrap(),
                "arguments": ["{workspace}"],
                "inputs": {
                    contract.to_string_lossy().into_owned():
                        build_identity::hash_file(&contract).unwrap(),
                },
            },
            "timeout_seconds": 120,
        }))
        .unwrap();
        fs::write(&request, &request_bytes).unwrap();
        let request_sha256 = build_identity::hash_bytes(&request_bytes);

        let policy = root.join("policy.json");
        fs::write(
            &policy,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "objective": "time",
                "basis": "efficiency",
                "meaningfulEffectPercent": 10.0,
                "tolerancePercent": 5.0,
                "requireAcceptance": true,
                "taskMix": "one frozen workload case",
                "stopping": {"maxAttemptsPerArm": 1, "requiredUnits": 1},
                "repeatedSelection": "predeclared",
                "tradeOff": null,
                "uncertainty": "unknown evidence stays inconclusive",
                "horizonTasks": 1.0,
                "overhead": {
                    "implementationSeconds": 0.0,
                    "evaluationSeconds": 0.0,
                    "maintenanceSecondsPerTask": 0.0,
                },
            }))
            .unwrap(),
        )
        .unwrap();
        let qualification = root.join("qualification.json");
        fs::write(
            &qualification,
            serde_json::to_vec_pretty(&json!({
                "status": "qualified",
                "policy": {
                    "repeats": 2,
                    "required_outputs": ["solution.txt"],
                    "ignored_metadata": [],
                },
                "missing_identity": [],
                "unfinished_attempts": [],
                "unverified_attempts": [],
                "tool_exchange_missing": [],
                "runner_mismatch": [],
                "missing_outputs": [],
                "divergent_outputs": [],
                "observed_repeats": 2,
                "required_repeats": 2,
            }))
            .unwrap(),
        )
        .unwrap();

        let workload_revision = git_output(&wl, &["rev-parse", "HEAD"]);
        let mut fixture = Self {
            _root: cleanup,
            root,
            proj,
            wl,
            home,
            run,
            spec: PathBuf::new(),
            bd,
            card: String::new(),
            state,
            baseline_build,
            candidate_build,
            upstream: programs.upstream.clone(),
            launcher: programs.launcher.clone(),
            request,
            request_sha256,
            policy,
            qualification,
            workload_revision,
        };
        fixture.card = fixture.admit();
        fixture.spec = fixture.root.join(format!("run-spec-{name}.json"));
        fixture.write_spec(&[], None);
        fixture
    }

    fn admit(&self) -> String {
        let out = self.feedback(&[
            "hypothesis-admit",
            "--mechanism",
            "bounded-output",
            "--conditions",
            "local-tool-runs",
            "--observation",
            "token-audit:findings#12",
            "--predicted",
            "less repeated context loading",
            "--counterexample",
            "diagnostics vanish on failure",
            "--acceptance",
            "the independent oracle passes",
            "--spec",
            "openspec/changes/add-synthetic",
            "--basis",
            "evidence-1",
        ]);
        assert!(out.status.success(), "admit: {}", text(&out));
        let text = text(&out);
        let id = text
            .strip_prefix("hypothesis ")
            .and_then(|rest| rest.split_whitespace().next())
            .unwrap_or_default()
            .to_owned();
        assert!(!id.is_empty(), "admission named no card: {text}");
        id
    }

    fn feedback(&self, args: &[&str]) -> Output {
        let mut command = Command::new(manager());
        command
            .arg("feedback")
            .args(args)
            .arg("--project")
            .arg(&self.proj)
            .arg("--bd")
            .arg(&self.bd)
            .arg("--source")
            .arg(&self.proj)
            .env("CODEX_HOME", &self.home);
        command.output().expect("feedback runs")
    }

    fn improve(&self, args: &[&str]) -> Output {
        let mut command = Command::new(manager());
        command.arg("improve").args(args);
        command.output().expect("improve runs")
    }

    fn start(&self) -> Output {
        self.improve(&[
            "start",
            "--run",
            self.run.to_str().unwrap(),
            "--spec",
            self.spec.to_str().unwrap(),
        ])
    }

    fn resume(&self) -> Output {
        self.improve(&["resume", "--run", self.run.to_str().unwrap()])
    }

    fn status_json(&self) -> Value {
        let status = self.improve(&["status", "--run", self.run.to_str().unwrap(), "--json"]);
        assert!(status.status.success(), "status: {}", text(&status));
        serde_json::from_str(&text(&status)).expect("status --json")
    }

    fn cursor(&self) -> Value {
        serde_json::from_slice(&fs::read(self.run.join("cursor.json")).unwrap()).unwrap()
    }

    fn write_cursor(&self, value: &Value) {
        fs::write(
            self.run.join("cursor.json"),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }

    fn bd_comments(&self, item: &str) -> String {
        let out = Command::new(&self.bd)
            .args(["comments", item, "--json"])
            .current_dir(&self.proj)
            .env("BD_NON_INTERACTIVE", "1")
            .output()
            .expect("bd comments runs");
        assert!(out.status.success(), "bd comments: {}", text(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

fn write_change(project: &Path, change: &str, group: &str, scenario: &str) {
    let dir = project.join("openspec/changes").join(change);
    fs::create_dir_all(dir.join("specs").join(group)).unwrap();
    fs::write(dir.join("proposal.md"), format!("## Why\n\n{scenario}.\n")).unwrap();
    fs::write(
        dir.join("design.md"),
        format!("## Context\n\n{scenario}.\n"),
    )
    .unwrap();
    fs::write(
        dir.join("tasks.md"),
        format!("## 1. Work\n\n- [ ] 1.1 Do the {scenario} thing.\n"),
    )
    .unwrap();
    fs::write(
        dir.join("specs").join(group).join("spec.md"),
        format!(
            "## ADDED Requirements\n\n### Requirement: {scenario} behavior\n\nThe system SHALL do the {scenario} thing.\n\n#### Scenario: {scenario}\n\n- **WHEN** the probe runs\n- **THEN** it reports success\n"
        ),
    )
    .unwrap();
}

impl Fixture {
    /// The run spec JSON with optional top-level replacements.
    fn write_spec(&self, replacements: &[(&str, Value)], remove: Option<&str>) {
        let client_runner = json!({
            "endpoint": "http://127.0.0.1:45999/v1",
            "model": "fixture-glyph-1",
            "identity": {},
        });
        let mut document = json!({
            "schema": 1,
            "run": "comparison-fixture",
            "project": self.proj,
            "codex_home": self.home,
            "board": {"bd": self.bd, "project": self.proj},
            "specification": {
                "project": self.proj,
                "change": "add-synthetic",
                "store": Value::Null,
                "planning_root": self.proj,
            },
            "hypothesis_item": self.card,
            "experiment": {
                "acceptance_artifact": "specs/synthetic/spec.md",
                "acceptance_heading": "#### Scenario: Synthetic case",
                "mechanism": "bounded-output",
                "counterexample": "diagnostics vanish",
                "applicability": "local tool runs",
                "independent_acceptance": "the frozen checker program executes",
                "meaningful_effect": "fewer repeated loads",
                "operating_conditions": "cold context",
                "comparison_policy": "matched pairs",
                "stopping_rule": "one predeclared pair",
            },
            "base_revision": git_output(&self.proj, &["rev-parse", "HEAD"]),
            "writable_scope": ["crates/one"],
            "runner": {
                "profile": "ds",
                "model": Value::Null,
                "model_provider": Value::Null,
                "reasoning_effort": Value::Null,
            },
            "local_runner": client_runner.clone(),
            "qualification": self.qualification,
            "evidence_root": Value::Null,
            "publication_scope": ["experiment"],
            "oracle": "outcome-oracle:fixture-request",
            "removal": Value::Null,
            "comparison": {
                "schema": 1,
                "specification": {
                    "project": self.wl,
                    "change": "add-workload",
                    "store": Value::Null,
                    "planning_root": self.wl,
                },
                "contract": {
                    "acceptance_artifact": "specs/workload/spec.md",
                    "acceptance_heading": "#### Scenario: Workload case",
                    "mechanism": "frozen-workload",
                    "counterexample": "the workload is changed between arms",
                    "applicability": "the frozen task snapshot",
                    "independent_acceptance": "the frozen checker program executes",
                    "meaningful_effect": "less time through acceptance",
                    "operating_conditions": "one predeclared pair",
                    "comparison_policy": "matched pairs",
                    "stopping_rule": "one predeclared pair",
                },
                "task": {
                    "source": self.wl,
                    "revision": self.workload_revision,
                    "name": "workload-b",
                    "writable_scope": ["solution.txt", "sleep_ms"],
                },
                "runtimes": {
                    "state": self.state,
                    "baseline_build": self.baseline_build,
                    "candidate_build": self.candidate_build,
                    "baseline_label": "H",
                    "candidate_label": "H+A",
                    "upstream": self.upstream,
                    "client": {
                        "runner": client_runner,
                        "reasoningEffort": "low",
                        "catalogue": Value::Null,
                        "overlay": Value::Null,
                    },
                },
                "policy": self.policy,
                "acceptance": {
                    "request": self.request,
                    "request_sha256": self.request_sha256,
                },
                "observation_inputs": [],
            },
        });
        let object = document.as_object_mut().unwrap();
        for (key, value) in replacements {
            object.insert((*key).to_owned(), value.clone());
        }
        if let Some(key) = remove {
            object.remove(key);
        }
        fs::write(&self.spec, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    }

    /// Start the run and replace the resulting idle cursor with the retained
    /// ready candidate the planning/implementation workflow reaches.
    fn start_with_ready_candidate(&self, removal_required: bool) -> PathBuf {
        let start = self.start();
        assert!(start.status.success(), "{}", text(&start));
        let base = git_output(&self.proj, &["rev-parse", "HEAD"]);
        let worktree = self.root.join("candidate-worktree");
        let branch = format!("improve/comparison-fixture/{}", self.card);
        let checkout =
            task_worktree::allocate_candidate_checkout(&self.proj, &worktree, &branch, &base)
                .expect("the candidate allocation is created");
        let mut cursor = self.cursor();
        cursor["phase"] = json!("candidate-ready");
        cursor["condition"] = Value::Null;
        cursor["candidate"] = json!({
            "hypothesis": self.card,
            "change": "add-synthetic",
            "removal_required": removal_required,
            "removal_frozen": Value::Null,
            "worktree": serde_json::to_value(&checkout).unwrap(),
            "planning_receipt": self.run.join("planning.json").display().to_string(),
            "planner_attempt": Value::Null,
            "implementer_attempt": Value::Null,
            "revision": base,
            "result": Value::Null,
        });
        self.write_cursor(&cursor);
        checkout.path
    }

    fn arm_dir(&self, arm: &str) -> PathBuf {
        self.run.join("comparison").join(arm)
    }

    /// Simulate one measured conversation: a pooled-style worktree of the
    /// arm's dispatch checkout with a committed solution, the retained
    /// terminal receipt of its visible dispatch and the rollout the native
    /// client would have written into the arm home.
    #[allow(clippy::too_many_arguments)]
    fn simulate_arm(
        &self,
        arm: &str,
        role: &str,
        solution: &str,
        sleep_ms: u64,
        started_offset_seconds: f64,
        messages: u64,
        tool_calls: u64,
        session: &str,
    ) -> PathBuf {
        let dir = self.arm_dir(arm);
        let checkout = dir.join("checkout");
        let slot = dir.join("checkout-wt1");
        if slot.exists() {
            fs::remove_dir_all(&slot).unwrap();
        }
        git(
            &checkout,
            &[
                "worktree",
                "add",
                "--detach",
                slot.to_str().unwrap(),
                "HEAD",
            ],
        );
        fs::write(slot.join("solution.txt"), format!("{solution}\n")).unwrap();
        fs::write(slot.join("sleep_ms"), format!("{sleep_ms}\n")).unwrap();
        git(&slot, &["add", "."]);
        git(
            &slot,
            &[
                "-c",
                "user.email=fixture@example.test",
                "-c",
                "user.name=Fixture",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "solve the workload",
            ],
        );
        let home = dir.join("home");
        let sessions = home.join("sessions/2026/10/01");
        fs::create_dir_all(&sessions).unwrap();
        let rollout = sessions.join(format!("rollout-{session}.jsonl"));
        let lines = [
            json!({
                "type": "session_meta",
                "payload": {"id": session, "base_instructions": "fixture"},
            }),
            json!({
                "type": "turn_context",
                "payload": {"model": "fixture-glyph-1", "effort": "low", "turn_id": "turn-1"},
            }),
            json!({
                "type": "token_usage_record",
                "payload": {
                    "response_id": "response-1",
                    "usage": {
                        "input_tokens": 100,
                        "cached_input_tokens": 40,
                        "output_tokens": 20,
                        "reasoning_output_tokens": 5,
                        "total_tokens": 120,
                    },
                },
            }),
        ]
        .iter()
        .map(|value| serde_json::to_string(value).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(&rollout, format!("{lines}\n")).unwrap();

        let receipt = self.run.join(format!("{arm}-receipt.json"));
        let owner = {
            let cursor = self.cursor();
            cursor["attempts"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|attempt| attempt["role"] == role)
                .and_then(|attempt| attempt["owner"].as_str())
                .expect("the controller recorded this arm's dispatch owner")
                .to_owned()
        };
        let slot_index = {
            let cursor = self.cursor();
            cursor["attempts"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|attempt| attempt["role"] == role)
                .and_then(|attempt| attempt["binding"]["slot"].as_u64())
                .unwrap_or(1)
        };
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "launcher": self.launcher.display().to_string(),
                "profile": "default",
                "mode": "tui",
                "visible": true,
                "host": "owned-console",
                "slot": {
                    "index": slot_index,
                    "path": slot.display().to_string(),
                    "source": checkout.display().to_string(),
                    "owner": owner,
                    "base": "frozen",
                    "remote": "origin",
                    "branch": Value::Null,
                },
                "originatingLead": {
                    "schema": 1,
                    "threadId": "fixture-thread",
                    "runGeneration": "gen-1",
                    "dispatcher": {"pid": 1, "creationTime": 1, "program": "C:\\fixture\\dispatcher.exe"},
                },
                "observation": {
                    "schema": 1,
                    "coverage": "native",
                    "reason": Value::Null,
                    "state": "completed",
                    "session": session,
                    "previousSession": Value::Null,
                    "exitCode": 0,
                    "events": 5,
                    "messages": messages,
                    "toolCalls": tool_calls,
                    "malformed": 0,
                    "cause": Value::Null,
                    "host": {"pid": 4294967294u32, "created": 1, "program": "C:\\missing\\host.exe"},
                    "result": Value::Null,
                    "detail": Value::Null,
                    "updatedMs": 1,
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let result = self.run.join(format!("{arm}-result.txt"));
        fs::write(
            &result,
            "the workload is committed; the controller verifies it\n",
        )
        .unwrap();
        let started_ms = ((now_seconds() - started_offset_seconds) * 1000.0) as u64;
        let mut cursor = self.cursor();
        let attempts = cursor["attempts"].as_array_mut().unwrap();
        let attempt = attempts
            .iter_mut()
            .rev()
            .find(|attempt| attempt["role"] == role)
            .expect("the arm dispatch was recorded");
        attempt["state"] = json!("started");
        attempt["binding"] = json!({
            "slot": slot_index,
            "owner": owner,
            "generation": "gen-1",
            "receipt": receipt.display().to_string(),
            "session": session,
            "host": Value::Null,
        });
        attempt["retained"] = Value::Null;
        attempt["checkout"] = json!(slot.display().to_string());
        attempt["receipt"] = json!(receipt.display().to_string());
        attempt["result"] = json!(result.display().to_string());
        attempt["started_ms"] = json!(started_ms);
        attempt["updated_ms"] = json!(started_ms);
        self.write_cursor(&cursor);
        slot
    }
}

fn session_id(seed: &str) -> String {
    let digest = build_identity::hash_bytes(seed.as_bytes());
    format!(
        "{}-{}-{}-{}-{}",
        &digest[..8],
        &digest[8..12],
        &digest[12..16],
        &digest[16..20],
        &digest[20..32]
    )
}

/// One bounded local HTTP observation fixture: the declared server facts the
/// API-observed qualification collects, with an operator-controlled body so a
/// drift is observable.
struct ObservationServer {
    port: u16,
    body: std::sync::Arc<std::sync::Mutex<String>>,
}

impl ObservationServer {
    fn start(initial: &str) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = std::sync::Arc::new(std::sync::Mutex::new(initial.to_owned()));
        let shared = body.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                // Read the request headers completely before answering so a
                // split request cannot race the response.
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                loop {
                    match std::io::Read::read(&mut stream, &mut buffer) {
                        Ok(0) => break,
                        Ok(count) => {
                            request.extend_from_slice(&buffer[..count]);
                            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let payload = shared.lock().unwrap().clone();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
                let _ = <std::net::TcpStream as std::io::Write>::flush(&mut stream);
            }
        });
        Self { port, body }
    }

    fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    fn set(&self, body: &str) {
        *self.body.lock().unwrap() = body.to_owned();
    }
}

/// A missing policy, qualification or workload planning receipt blocks
/// before any comparison workspace, installation or model dispatch exists.
#[test]
fn comparison_preparation_requires_policy_qualification_and_workload_planning() {
    // No usable qualification record: the measured arms never begin.
    let fixture = Fixture::new("unqualified");
    fixture.write_spec(
        &[(
            "qualification",
            json!(fixture.root.join("missing-qualification.json")),
        )],
        None,
    );
    fixture.start_with_ready_candidate(false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("qualification"), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["attempts"].as_array().unwrap().len(), 0, "{status}");
    assert!(
        !fixture.run.join("comparison").exists(),
        "an unqualified run creates no comparison workspace"
    );

    // An unreadable declaration is refused instead of being reinterpreted.
    let fixture = Fixture::new("bad-policy");
    fs::write(&fixture.policy, "{\"schema\": 9}\n").unwrap();
    fixture.start_with_ready_candidate(false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("policy"), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["attempts"].as_array().unwrap().len(), 0, "{status}");
    assert!(!fixture.run.join("comparison").exists(), "{status}");

    // A workload whose own change no longer qualifies blocks the preparation
    // even when the run inputs are otherwise complete.
    let fixture = Fixture::new("unplanned-workload");
    fs::remove_dir_all(fixture.wl.join("openspec/changes/add-workload")).unwrap();
    fixture.start_with_ready_candidate(false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("workload"), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["attempts"].as_array().unwrap().len(), 0, "{status}");
    assert!(!fixture.run.join("comparison").exists(), "{status}");

    // Changed frozen acceptance bytes are refused instead of being re-frozen.
    let fixture = Fixture::new("changed-request");
    fixture.start_with_ready_candidate(false);
    let mut request: Value = serde_json::from_slice(&fs::read(&fixture.request).unwrap()).unwrap();
    request["timeout_seconds"] = json!(30);
    fs::write(
        &fixture.request,
        serde_json::to_vec_pretty(&request).unwrap(),
    )
    .unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("changed since the run declared it"),
        "{output}"
    );
    let status = fixture.status_json();
    assert_eq!(status["attempts"].as_array().unwrap().len(), 0, "{status}");
    assert!(!fixture.run.join("comparison").exists(), "{status}");
}

/// A forged candidate result is rejected by the frozen checker, the decision
/// is published, and nothing is integrated or activated.
#[test]
fn a_forged_candidate_result_is_rejected_and_never_integrated() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("forged");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fixture.start_with_ready_candidate(false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("before submission"),
        "the fixture launcher cannot execute a conversation: {output}"
    );
    let status = fixture.status_json();
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 1, "{cursor}");
    assert_eq!(attempts[0]["role"], "baseline");
    assert_eq!(attempts[0]["state"], "failed");
    assert!(
        status["comparison"]["baseline"]["runtime"].is_string(),
        "the baseline installation receipt is retained: {status}"
    );
    assert!(
        fixture.run.join("comparison/bindings.json").is_file(),
        "the prepared bindings are retained"
    );
    assert!(
        fixture.run.join("comparison/planning.json").is_file(),
        "the workload's own planning receipt is retained"
    );

    // A receipt that no longer matches the accepted dispatch generation is
    // never settled into the comparison.
    let session = session_id("baseline-session");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let receipt_path = fixture.run.join("baseline-receipt.json");
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt["originatingLead"]["runGeneration"] = json!("gen-2");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        fixture.cursor()["attempts"][0]["reason"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("generation"),
        "{status}"
    );
    assert_eq!(
        status["comparison"]["baseline"]["accepted"],
        Value::Null,
        "an unverified generation never enters the comparison: {status}"
    );

    // Restoring the accepted generation lets the retained evidence settle.
    receipt["originatingLead"]["runGeneration"] = json!("gen-1");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "the verified baseline solution is accepted: {status}"
    );
    assert!(
        status["comparison"]["baseline"]["revision"].is_string(),
        "{status}"
    );
    assert!(
        status["comparison"]["baseline"]["oracle"].is_string(),
        "{status}"
    );

    // The candidate conversation returns a forged solution: the frozen
    // checker rejects it and the frozen policy records a reject that cannot
    // become a board decision record because the matched evidence is absent.
    let session = session_id("candidate-session");
    let slot = fixture.simulate_arm("candidate", "candidate", "wrong", 60, 2.0, 1, 1, &session);
    // Uncommitted or untracked output never enters acceptance.
    fs::write(slot.join("scratch-output.txt"), "forged scratch\n").unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("uncommitted or untracked"),
        "uncommitted output is refused instead of entering acceptance: {output}"
    );
    assert_eq!(
        fixture.cursor()["comparison"]["candidate"]["accepted"],
        Value::Null
    );
    fs::remove_file(slot.join("scratch-output.txt")).unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], false,
        "{status}"
    );
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap()
            .contains("unrecorded:reject"),
        "{status}"
    );
    let evaluation: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/evaluation.json")).unwrap())
            .unwrap();
    assert_eq!(evaluation["decision"], "reject", "{evaluation}");
    assert!(
        evaluation["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .contains("independent acceptance failed")),
        "{evaluation}"
    );
    assert!(
        !fixture
            .bd_comments(&fixture.card)
            .contains("benefit-gate v2"),
        "a forged result never produces an adoptable board decision"
    );
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        base,
        "a rejected experiment never changes the accepted mainline"
    );

    // A repeated resume reuses the retained verdict: no replay, no dispatch,
    // no second verdict record.
    let before = fixture.cursor();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert_eq!(fixture.cursor()["attempts"], before["attempts"]);
    assert_eq!(
        fixture.cursor()["comparison"]["decision"],
        before["comparison"]["decision"]
    );
}

/// An accepted, measurably faster candidate yields the evidence-bound adopt
/// decision through the frozen policy; the removal authority is re-checked
/// before the candidate's own measured dispatch and nothing is integrated or
/// activated by the verdict.
#[test]
fn an_accepted_faster_candidate_is_adopted_without_activation_or_integration() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("adopt");
    fixture.write_spec(
        &[(
            "removal",
            json!({"proposal": "proposal-alpha", "target": "target-beta"}),
        )],
        None,
    );
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fixture.start_with_ready_candidate(true);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("adopt-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 1200, 60.0, 2, 3, &session);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("removal"),
        "the candidate treatment waits for the informed removal decision: {output}"
    );
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}"
    );
    let candidate_attempts = fixture.cursor()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|attempt| attempt["role"] == "candidate")
        .count();
    assert_eq!(
        candidate_attempts, 0,
        "no candidate dispatch before the decision"
    );

    // The recorded refusal blocks the dependent measured dispatch; the
    // controller does not repeat the request for the same basis.
    let proposed = fixture.feedback(&[
        "removal-propose",
        "--item",
        &fixture.card,
        "--proposal",
        "proposal-alpha",
        "--target",
        "target-beta",
        "--evidence",
        "evidence-1",
        "--loss",
        "synthetic-loss",
        "--preview",
        "preview-1",
    ]);
    assert!(proposed.status.success(), "{}", text(&proposed));
    let refused = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "refuse",
        "--proposal",
        "proposal-alpha",
        "--target",
        "target-beta",
        "--loss",
        "synthetic-loss",
    ]);
    assert!(refused.status.success(), "{}", text(&refused));
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("declined"),
        "the refusal is visible and unbypassable: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|attempt| attempt["role"] == "candidate")
            .count(),
        0,
        "a refusal cannot be bypassed through workload dispatch: {status}"
    );

    // The informed approval unblocks the candidate's measured conversation.
    let approved = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "approve",
        "--proposal",
        "proposal-alpha",
        "--target",
        "target-beta",
        "--actions",
        "experiment",
        "--loss",
        "synthetic-loss",
    ]);
    assert!(approved.status.success(), "{}", text(&approved));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let candidate_attempts = fixture.cursor()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|attempt| attempt["role"] == "candidate")
        .count();
    assert_eq!(candidate_attempts, 1, "the approved treatment dispatches");

    // The candidate solves the workload and is measurably faster.
    let session = session_id("adopt-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.5, 1, 1, &session);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap()
            .contains("outcome=adopt"),
        "{status}"
    );
    let comments = fixture.bd_comments(&fixture.card);
    assert!(comments.contains("outcome=adopt"), "{comments}");
    assert!(comments.contains("matched=1"), "{comments}");
    assert!(
        fixture.run.join("comparison/report.json").is_file()
            && fixture.run.join("comparison/evaluation.json").is_file(),
        "the authoritative accounting and evaluation are retained"
    );
    // The verdict is not an integration or an activation.
    assert_eq!(git_output(&fixture.proj, &["rev-parse", "HEAD"]), base);
    assert_eq!(status["selected_variant"], Value::Null, "{status}");
    let before = fixture.cursor();
    let comments_before = fixture.bd_comments(&fixture.card);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert_eq!(fixture.cursor()["attempts"], before["attempts"]);
    assert_eq!(fixture.bd_comments(&fixture.card), comments_before);
}

/// Missing measured counters stay visible: the declared rounds/tool evidence
/// is incomplete, so the frozen policy cannot adopt and the decision records
/// the missing coverage instead of assuming zero.
#[test]
fn missing_counter_evidence_stays_inconclusive() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("missing-counters");
    fixture.start_with_ready_candidate(false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("counter-baseline");
    fixture.simulate_arm("baseline", "baseline", "solved", 50, 30.0, 2, 3, &session);
    let receipt_path = fixture.run.join("baseline-receipt.json");
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt["observation"]
        .as_object_mut()
        .unwrap()
        .remove("toolCalls");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("counter-candidate");
    fixture.simulate_arm("candidate", "candidate", "solved", 50, 1.0, 1, 1, &session);
    let receipt_path = fixture.run.join("candidate-receipt.json");
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt["observation"]
        .as_object_mut()
        .unwrap()
        .remove("messages");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert!(
        status["comparison"]["decision"]
            .as_str()
            .unwrap()
            .contains("outcome=inconclusive"),
        "{status}"
    );
    let comments = fixture.bd_comments(&fixture.card);
    assert!(comments.contains("outcome=inconclusive"), "{comments}");
}

/// The selected API-observed qualification is validated through its own owner
/// and its declared facts are re-collected before the measured arm: an
/// inconsistent record or an observed drift blocks the arm without a model
/// call.
#[test]
fn api_observed_qualification_drift_blocks_the_measured_arm() {
    let _serial = INSTALL.lock().unwrap();
    use harness_core::outcome_qualification::{
        ApiObservationPlan, ApiObservedPolicy, ClientInput, DeclaredObservation, LocalRunner,
        MaterialIdentity, ObservationBinding, ObservationRequest, QualificationAttempt,
        RepeatabilityPolicy, RunnerRecord, collect_observations, qualify_api_observed,
    };

    let fixture = Fixture::new("api-observed");
    let server = ObservationServer::start("{\"build\":\"b-1\"}\n");
    let runner = LocalRunner {
        endpoint: server.endpoint(),
        model: "fixture-glyph-1".to_owned(),
        identity: MaterialIdentity {
            reasoning: Some("low".to_owned()),
            ..MaterialIdentity::default()
        },
    };
    let policy = ApiObservedPolicy {
        output: RepeatabilityPolicy {
            repeats: 2,
            required_outputs: vec!["solution.txt".to_owned()],
            ignored_metadata: Vec::new(),
        },
        plan: ApiObservationPlan {
            requests: vec![ObservationRequest {
                path: "/props".to_owned(),
                fields: vec![DeclaredObservation {
                    name: "server.build".to_owned(),
                    pointer: "/build".to_owned(),
                    required: true,
                    binding: ObservationBinding::Value,
                }],
            }],
            required_client_inputs: vec!["profile".to_owned()],
        },
    };
    let inputs = vec![ClientInput {
        name: "profile".to_owned(),
        path: fixture.home.join("config.toml"),
    }];
    let observations = collect_observations(&runner, &policy.plan, &inputs)
        .expect("the local observation fixture answers the declared facts");
    let attempt = |id: &str| QualificationAttempt {
        attempt_id: id.to_owned(),
        completed: true,
        model_metadata_verified: true,
        tool_operations: 1,
        runner: Some(RunnerRecord::new(&runner)),
        outputs: BTreeMap::from([("solution.txt".to_owned(), "a".repeat(64))]),
    };
    let qualification = qualify_api_observed(
        &runner,
        &policy,
        &observations,
        &[attempt("api-1"), attempt("api-2")],
    )
    .expect("the declared policy is evaluated");
    assert!(
        qualification.qualified(),
        "the fixture qualification is complete: {qualification:?}"
    );
    fs::write(
        &fixture.qualification,
        serde_json::to_vec_pretty(&qualification).unwrap(),
    )
    .unwrap();
    fixture.write_spec(
        &[("local_runner", serde_json::to_value(&runner).unwrap())],
        None,
    );
    // The dynamic endpoint and the explicit observation inputs are patched in
    // after the shared fixture spec is written.
    let mut spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    spec["comparison"]["runtimes"]["client"]["runner"] = serde_json::to_value(&runner).unwrap();
    spec["comparison"]["observation_inputs"] = json!([
        {"name": "profile", "path": fixture.home.join("config.toml")},
    ]);
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();

    fixture.start_with_ready_candidate(false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("before submission"),
        "the API-observed arm reaches the visible dispatch owner: {output}"
    );
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 1);

    // A retained record whose bound digests no longer agree is refused by the
    // qualification owner before dependent work.
    let mut record: Value =
        serde_json::from_slice(&fs::read(&fixture.qualification).unwrap()).unwrap();
    let digest = record["observation_digest"].as_str().unwrap().to_owned();
    record["observation_digest"] = json!(format!("{digest}0"));
    fs::write(
        &fixture.qualification,
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("internally inconsistent"), "{output}");
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        1,
        "an inconsistent qualification dispatches nothing"
    );
    record["observation_digest"] = json!(digest);
    fs::write(
        &fixture.qualification,
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();

    // A changed observed server fact refuses the arm before its conversation:
    // the retained qualification no longer matches what the endpoint serves.
    server.set("{\"build\":\"b-2\"}\n");
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("differs from the qualified identity"),
        "observed drift blocks the measured arm: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        1,
        "a drifted observation dispatches nothing"
    );
    assert_eq!(
        fixture.cursor()["comparison"]["baseline"]["accepted"],
        Value::Null
    );
}
