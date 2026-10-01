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

// The native frontend the host attaches is `codex --remote ... resume THREAD`.
// A real TUI replaces its own console caption with `{title} | ` once the named
// thread is loaded; this double does exactly that and then stays alive until
// the owning host ends it.
unsafe extern "system" {
    fn GetConsoleTitleW(lp_console_title: *mut u16, n_size: u32) -> u32;
    fn SetConsoleTitleW(lp_console_title: *const u16) -> u32;
}

fn frontend(caption_source: &str) -> ! {
    let mut buffer = [0u16; 1024];
    let count = unsafe { GetConsoleTitleW(buffer.as_mut_ptr(), buffer.len() as u32) };
    let mut title = String::from_utf16_lossy(&buffer[..count as usize]);
    if title.is_empty() {
        title = caption_source.to_owned();
    }
    let caption = format!("{title} | ");
    let mut wide: Vec<u16> = caption.encode_utf16().collect();
    wide.push(0);
    unsafe { SetConsoleTitleW(wide.as_ptr()) };
    loop {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
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
    if args.iter().any(|arg| arg == "--remote") {
        frontend(env::args().next().as_deref().unwrap_or("codex"));
    }
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
    /// Workload B's own durable hypothesis card.
    workload_card: String,
    state: PathBuf,
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
        // Ownership-neutral line endings: a worktree of the same commit must
        // record the same source identity as the working tree it was built
        // from, whatever the ambient Git configuration does.
        git(&proj, &["config", "core.autocrlf", "false"]);
        fs::create_dir_all(proj.join("crates/one/src")).unwrap();
        fs::write(proj.join("crates/one/src/lib.rs"), "// synthetic\n").unwrap();
        fs::create_dir_all(proj.join("global")).unwrap();
        fs::write(
            proj.join("global/orchestration.toml"),
            "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\n",
        )
        .unwrap();
        fs::write(proj.join("README.md"), "synthetic\n").unwrap();
        // The candidate project is also the harness kit source: the prepared
        // builds record its compiled inputs, and the arm installation reads
        // its manifest, profile, instructions, hooks and skills.
        fs::write(
            proj.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        fs::write(proj.join("Cargo.lock"), "# fixture lock\n").unwrap();
        for directory in ["global/agents", ".agents/skills/arm-skill"] {
            fs::create_dir_all(proj.join(directory)).unwrap();
        }
        // Git does not track empty directories: keep the agents directory
        // present in every worktree of this commit.
        fs::write(proj.join("global/agents/.gitkeep"), "\n").unwrap();
        fs::write(
            proj.join("global/kit.json"),
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
            proj.join("global/harness.config.toml"),
            "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nweb_search = \"disabled\"\n\n[features]\ncode_mode = true\napps = false\n",
        )
        .unwrap();
        fs::write(
            proj.join("global/principles-of-work.md"),
            "# Principles\nfixture arm instructions\n",
        )
        .unwrap();
        fs::write(proj.join("global/hooks.json"), "{}\n").unwrap();
        fs::write(proj.join("global/rtk-hooks.json"), "{}\n").unwrap();
        fs::write(
            proj.join(".agents/skills/arm-skill/SKILL.md"),
            "---\nname: arm-skill\ndescription: Fixture skill.\n---\n\nfixture skill body\n",
        )
        .unwrap();
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
            "[profiles.ds]\nmodel = 'fixture-glyph-1'\nmodel_provider = 'local'\nmodel_reasoning_effort = 'low'\n",
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
        git(&wl, &["config", "core.autocrlf", "false"]);
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

        // The owned native state the two prepared builds are published into;
        // the builds themselves are created from the frozen baseline source
        // and the ready candidate checkout by `prepare_builds`.
        let state = root.join("state");
        fs::create_dir_all(state.join("builds")).unwrap();
        fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
        let programs = programs();

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
            workload_card: String::new(),
            state,
            upstream: programs.upstream.clone(),
            launcher: programs.launcher.clone(),
            request,
            request_sha256,
            policy,
            qualification,
            workload_revision,
        };
        fixture.card = fixture.admit();
        fixture.workload_card = fixture.admit_workload();
        fixture.spec = fixture.root.join(format!("run-spec-{name}.json"));
        fixture.write_spec(&[], None);
        fixture
    }

    /// Workload B's own admitted card on the same board, referencing B's own
    /// OpenSpec change in the frozen workload project.
    fn admit_workload(&self) -> String {
        let out = self.feedback(&[
            "hypothesis-admit",
            "--mechanism",
            "frozen-workload",
            "--conditions",
            "frozen-task-snapshot",
            "--observation",
            "workload:fixture",
            "--predicted",
            "the workload is solved within the declared conditions",
            "--counterexample",
            "the task snapshot changed between arms",
            "--acceptance",
            "the frozen checker program passes",
            "--spec",
            "openspec/changes/add-workload",
            "--basis",
            "workload-basis-1",
        ]);
        assert!(out.status.success(), "workload admit: {}", text(&out));
        let text = text(&out);
        let id = text
            .strip_prefix("hypothesis ")
            .and_then(|rest| rest.split_whitespace().next())
            .unwrap_or_default()
            .to_owned();
        assert!(!id.is_empty(), "workload admission named no card: {text}");
        id
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
        self.improve_with_env(args, &[])
    }

    /// The controller is run as an ordinary operator process: the agent
    /// session markers that would legitimately refuse nested executor
    /// dispatch are removed, and only the explicitly declared environment
    /// reaches the dispatched arm.
    fn improve_with_env(&self, args: &[&str], environment: &[(&str, &str)]) -> Output {
        let mut command = Command::new(manager());
        command.arg("improve").args(args);
        command.env_remove("HARNESS_EXECUTOR_SESSION");
        command.env_remove("HARNESS_EXECUTOR_FIXTURE_MODE");
        command.env_remove("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE");
        // An operator process is not itself a dispatched executor run, so the
        // session's own copied run marker and any lead id supplied by the
        // surrounding session are not authority and must not reach the
        // controller. CODEX_THREAD_ID stays: the operator thread id is the
        // legitimate origination identity.
        command.env_remove("HARNESS_EXECUTOR_RUN");
        command.env_remove("HARNESS_ORIGINATING_LEAD");
        command.env_remove("HARNESS_LEAD_THREAD");
        command.env_remove("HARNESS_LEAD_RECIPIENT");
        for (name, value) in environment {
            command.env(name, value);
        }
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

    /// One resume that may dispatch a measured arm: the child-only fixture
    /// mode reaches the installed launcher double through the host's explicit
    /// forward, exactly as the arm's own settings do.
    fn resume_dispatched(&self, extra: &[(&str, &str)]) -> Output {
        let mut environment: Vec<(&str, &str)> = vec![CONTROL_CHILD_MODE];
        environment.extend_from_slice(extra);
        self.improve_with_env(
            &["resume", "--run", self.run.to_str().unwrap()],
            &environment,
        )
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
            "publication_scope": ["experiment", "integration"],
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
                "workload_card": self.workload_card,
                "task": {
                    "source": self.wl,
                    "revision": self.workload_revision,
                    "name": "workload-b",
                    "writable_scope": ["solution.txt", "sleep_ms"],
                },
                "runtimes": {
                    "state": self.state,
                    "baseline_build": self.state.join("builds").join("h-build"),
                    "candidate_build": self.state.join("builds").join("ha-build"),
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

    /// Allocate the ready candidate's owned worktree before the run starts and
    /// optionally commit the candidate treatment on its branch, so the
    /// prepared builds can be created from an exact source identity.
    fn prepare_candidate(&self, change: Option<&str>) -> task_worktree::CandidateCheckout {
        let base = git_output(&self.proj, &["rev-parse", "HEAD"]);
        let worktree = self.root.join("candidate-worktree");
        let branch = format!("improve/comparison-fixture/{}", self.card);
        let mut checkout =
            task_worktree::allocate_candidate_checkout(&self.proj, &worktree, &branch, &base)
                .expect("the candidate allocation is created");
        if let Some(change) = change {
            fs::write(checkout.path.join("crates/one/src/lib.rs"), change).unwrap();
            // A combined-tree artifact the integration owner's declared check
            // reads, so a real fast-forward can be verified on the fixture.
            fs::write(checkout.path.join("answer.txt"), "integrated\n").unwrap();
            git(&checkout.path, &["add", "."]);
            git(
                &checkout.path,
                &[
                    "-c",
                    "user.email=fixture@example.test",
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-qm",
                    "candidate implementation",
                ],
            );
            checkout.revision = git_output(&checkout.path, &["rev-parse", "HEAD"]);
        }
        checkout
    }

    /// Publish the two explicit builds: the baseline runtime from the frozen
    /// baseline source (the project at its declared base) and the candidate
    /// runtime from the ready candidate checkout.
    fn prepare_builds(&self, checkout: &task_worktree::CandidateCheckout) {
        let launcher = &programs().launcher;
        fixture_build(&self.state, "h-build", &self.proj, launcher, "baseline");
        fixture_build(
            &self.state,
            "ha-build",
            &checkout.path,
            launcher,
            "candidate",
        );
    }

    /// The same two builds with the owned executor fixture as the installed
    /// launcher, so the arm can serve the real control-backed app-server
    /// contract instead of refusing before submission.
    fn prepare_real_builds(&self, checkout: &task_worktree::CandidateCheckout) {
        let launcher = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
        fixture_build(&self.state, "h-build", &self.proj, &launcher, "baseline");
        fixture_build(
            &self.state,
            "ha-build",
            &checkout.path,
            &launcher,
            "candidate",
        );
    }

    /// Start the run and replace the resulting idle cursor with the retained
    /// ready candidate the planning/implementation workflow reaches.
    fn start_with_ready_candidate(
        &self,
        checkout: &task_worktree::CandidateCheckout,
        removal_required: bool,
    ) {
        let start = self.start();
        assert!(start.status.success(), "{}", text(&start));
        let mut cursor = self.cursor();
        cursor["phase"] = json!("candidate-ready");
        cursor["condition"] = Value::Null;
        cursor["candidate"] = json!({
            "hypothesis": self.card,
            "change": "add-synthetic",
            "removal_required": removal_required,
            "removal_frozen": Value::Null,
            "worktree": serde_json::to_value(checkout).unwrap(),
            "planning_receipt": self.run.join("planning.json").display().to_string(),
            "planner_attempt": Value::Null,
            "implementer_attempt": Value::Null,
            "revision": checkout.revision,
            "result": Value::Null,
        });
        self.write_cursor(&cursor);
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
        // A real dispatch attempt in this run may already have registered the
        // pooled slot name; the simulated arm reuses the same pooled slot, so
        // clear the stale registration of the removed directory first.
        git(&checkout, &["worktree", "prune"]);
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
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
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
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
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
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("workload"), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["attempts"].as_array().unwrap().len(), 0, "{status}");
    assert!(!fixture.run.join("comparison").exists(), "{status}");

    // Changed frozen acceptance bytes are refused instead of being re-frozen.
    let fixture = Fixture::new("changed-request");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
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
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
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
            .contains("outcome=reject"),
        "{status}"
    );
    assert_eq!(status["phase"], "decision-recorded", "{status}");
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
    // The supported non-adoption is published on the real board, parses as a
    // complete non-adoption, and carries the same exact lineage the
    // integration owner re-derives: raw base and candidate revisions plus the
    // declared acceptance reference.
    let comments =
        harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, &fixture.card)
            .unwrap();
    let records = harness_core::benefit_gate::parse_gate_comments(&comments);
    let joined = comments.join("\n");
    assert!(joined.contains("benefit-gate v2"), "{joined}");
    assert!(joined.contains("outcome=reject"), "{joined}");
    let assessment =
        harness_core::benefit_gate::assess(&records, &fixture.card).expect("attributable");
    assert_eq!(
        assessment.verdict,
        harness_core::benefit_gate::Verdict::NonAdoption
    );
    assert!(!harness_core::benefit_gate::default_allowed(
        &records,
        &fixture.card
    ));
    let expected_revisions = format!("{}..{}", checkout.base, checkout.revision);
    assert!(
        assessment
            .latest
            .revisions
            .as_deref()
            .is_some_and(|revisions| revisions == expected_revisions),
        "{joined}"
    );
    assert!(
        !joined.contains("matched="),
        "the non-adoption never fabricates a matched count: {joined}"
    );
    // The newest non-adoption authorizes no integration.
    let bindings: harness_core::improvement_experiment::ExperimentBindings =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/bindings.json")).unwrap())
            .unwrap();
    let reject_evaluation: harness_core::improvement_policy::PolicyEvaluation =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/evaluation.json")).unwrap())
            .unwrap();
    let spec: harness_core::improvement_loop::RunSpec =
        serde_json::from_slice(&fs::read(fixture.run.join("spec.json")).unwrap()).unwrap();
    let experiment = fixture.cursor()["experiment"].as_str().unwrap().to_owned();
    fs::create_dir_all(fixture.root.join("integration-evidence")).unwrap();
    let outcome = harness_core::improvement_activation::integrate(
        &harness_core::improvement_activation::IntegrationRequest {
            spec: &spec,
            bindings: &bindings,
            evaluation: &reject_evaluation,
            experiment,
            frozen_removal: None,
            mainline: fixture.proj.clone(),
            check: harness_core::improvement_activation::CheckSpec {
                program: fixture.proj.join("Cargo.toml"),
                args: Vec::new(),
                timeout: std::time::Duration::from_secs(30),
            },
            evidence: fixture.root.join("integration-evidence"),
            prior: None,
        },
    )
    .unwrap();
    assert!(
        matches!(
            outcome,
            harness_core::improvement_activation::IntegrationOutcome::Blocked(_)
        ),
        "a reject decision must not authorize integration"
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
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, true);
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

    // The published adoption is consumable by the existing integration owner:
    // it re-derives the expected record from the exact raw revisions and the
    // binding acceptance, so the comparison-produced decision must match it
    // byte-for-field, and the declared combined-tree check then fast-forwards
    // the exact evaluated revision into the accepted mainline.
    let integration_approval = fixture.feedback(&[
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
        "experiment,integration",
        "--loss",
        "synthetic-loss",
    ]);
    assert!(
        integration_approval.status.success(),
        "{}",
        text(&integration_approval)
    );
    let expected_answer = fixture.root.join("answer-expected.txt");
    fs::write(&expected_answer, "integrated\n").unwrap();
    fs::create_dir_all(fixture.root.join("integration-evidence")).unwrap();
    let bindings: harness_core::improvement_experiment::ExperimentBindings =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/bindings.json")).unwrap())
            .unwrap();
    let evaluation: harness_core::improvement_policy::PolicyEvaluation =
        serde_json::from_slice(&fs::read(fixture.run.join("comparison/evaluation.json")).unwrap())
            .unwrap();
    let spec: harness_core::improvement_loop::RunSpec =
        serde_json::from_slice(&fs::read(fixture.run.join("spec.json")).unwrap()).unwrap();
    let experiment = fixture.cursor()["experiment"].as_str().unwrap().to_owned();
    let frozen_removal = fixture.cursor()["removal_frozen"]
        .as_str()
        .map(str::to_owned);
    let outcome = harness_core::improvement_activation::integrate(
        &harness_core::improvement_activation::IntegrationRequest {
            spec: &spec,
            bindings: &bindings,
            evaluation: &evaluation,
            experiment,
            frozen_removal,
            mainline: fixture.proj.clone(),
            check: harness_core::improvement_activation::CheckSpec {
                program: PathBuf::from(env!("CARGO_BIN_EXE_harness-improvement-fixture")),
                args: vec![
                    "check".into(),
                    checkout.path.as_os_str().to_owned(),
                    expected_answer.as_os_str().to_owned(),
                ],
                timeout: std::time::Duration::from_secs(120),
            },
            evidence: fixture.root.join("integration-evidence"),
            prior: None,
        },
    )
    .unwrap();
    match &outcome {
        harness_core::improvement_activation::IntegrationOutcome::Integrated(receipt) => {
            assert_eq!(receipt.candidate_revision, checkout.revision);
            assert_eq!(receipt.acceptance, bindings.acceptance);
        }
        harness_core::improvement_activation::IntegrationOutcome::Confirmed(receipt) => {
            assert_eq!(receipt.candidate_revision, checkout.revision);
        }
        harness_core::improvement_activation::IntegrationOutcome::Blocked(blocked) => {
            panic!(
                "the comparison-produced adoption must be consumable by the integration owner: {blocked:?}"
            );
        }
    }
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        checkout.revision,
        "the declared check and fast-forward integrated the exact evaluated revision"
    );

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
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
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

/// Write one API-observed qualification produced by the owner against the
/// local observation fixture, and declare the same explicit client inputs in
/// the run spec: the observed client file is the overlay the arms consume.
fn install_api_observed_qualification(fixture: &Fixture, server: &ObservationServer) -> PathBuf {
    use harness_core::outcome_qualification::{
        ApiObservationPlan, ApiObservedPolicy, ClientInput, DeclaredObservation, LocalRunner,
        MaterialIdentity, ObservationBinding, ObservationRequest, QualificationAttempt,
        RepeatabilityPolicy, RunnerRecord, collect_observations, qualify_api_observed,
    };
    let overlay = fixture.root.join("client-overlay.toml");
    fs::write(&overlay, "model_context_window = 262144\n").unwrap();
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
            required_client_inputs: vec!["overlay".to_owned()],
        },
    };
    let inputs = vec![ClientInput {
        name: "overlay".to_owned(),
        path: overlay.clone(),
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
    let mut spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    spec["comparison"]["runtimes"]["client"]["runner"] = serde_json::to_value(&runner).unwrap();
    spec["comparison"]["runtimes"]["client"]["overlay"] = json!(overlay);
    spec["comparison"]["observation_inputs"] = json!([
        {"name": "overlay", "path": overlay},
    ]);
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    overlay
}

/// A qualified API-observed record and an unconsumed observation template are
/// both handled through the real controller entry point: the record is
/// accepted, and an observation input that is not a consumed client file is
/// refused before any preparation.
#[test]
fn api_observed_qualification_drift_blocks_the_measured_arm() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("api-observed");
    let server = ObservationServer::start("{\"build\":\"b-1\"}\n");
    install_api_observed_qualification(&fixture, &server);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
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

    // An observation input that is not one of the consumed client files is
    // refused before any preparation: an unused qualified template cannot
    // stand in for the arm's actual configuration.
    let unconsumed = Fixture::new("api-unconsumed-input");
    let server = ObservationServer::start("{\"build\":\"b-1\"}\n");
    install_api_observed_qualification(&unconsumed, &server);
    let mut spec: Value = serde_json::from_slice(&fs::read(&unconsumed.spec).unwrap()).unwrap();
    spec["comparison"]["observation_inputs"] = json!([
        {"name": "profile", "path": unconsumed.home.join("config.toml")},
    ]);
    fs::write(&unconsumed.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    let checkout = unconsumed.prepare_candidate(Some("// candidate implementation\n"));
    unconsumed.prepare_builds(&checkout);
    unconsumed.start_with_ready_candidate(&checkout, false);
    let resume = unconsumed.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("not one of the client files"), "{output}");
    assert_eq!(
        unconsumed.cursor()["attempts"].as_array().unwrap().len(),
        0,
        "no measured attempt is even prepared from an unconsumed template"
    );
    assert!(!unconsumed.run.join("comparison").exists());
}

/// The selected API-observed policy reaches the planning/implementation
/// dispatch through the real controller: a qualified API record lets the
/// bounded investigator conversation start, while a blocked full-material
/// record keeps its refusal and starts nothing.
#[test]
fn api_observed_qualification_reaches_planning_and_implementation() {
    // Qualified API-observed identity: the investigator dispatch is attempted.
    let fixture = Fixture::new("api-planning");
    let server = ObservationServer::start("{\"build\":\"b-1\"}\n");
    let overlay = install_api_observed_qualification(&fixture, &server);
    let evidence = fixture.root.join("evidence");
    fs::create_dir_all(&evidence).unwrap();
    fs::write(evidence.join("observation.txt"), "retained observation\n").unwrap();
    let mut spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    spec["evidence_root"] = json!(evidence);
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    let launcher = fixture.home.join("harness/bin/codex.exe");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    fs::copy(programs().launcher.as_path(), &launcher).unwrap();
    let start = fixture.start();
    let output = text(&start);
    assert!(start.status.success(), "{output}");
    assert!(
        !output.contains("qualification"),
        "the qualified API record must not block planning work: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        1,
        "the bounded investigator conversation is dispatched: {output}"
    );
    assert_eq!(fixture.cursor()["attempts"][0]["role"], "investigator");
    assert!(
        overlay.is_file(),
        "the observed client overlay stays the explicit private input"
    );

    // A blocked full-material record keeps its refusal for the measured pair:
    // no comparison preparation, no dispatch, the owner's reasons retained.
    let fixture = Fixture::new("legacy-blocked");
    fs::write(
        &fixture.qualification,
        serde_json::to_vec_pretty(&json!({
            "status": "blocked",
            "policy": {
                "repeats": 2,
                "required_outputs": ["solution.txt"],
                "ignored_metadata": [],
            },
            "missing_identity": ["weights"],
            "unfinished_attempts": [],
            "unverified_attempts": [],
            "tool_exchange_missing": [],
            "runner_mismatch": [],
            "missing_outputs": ["solution.txt"],
            "divergent_outputs": [],
            "observed_repeats": 1,
            "required_repeats": 2,
        }))
        .unwrap(),
    )
    .unwrap();
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("qualification") && output.contains("missing required outputs"),
        "a blocked full-material record keeps its refusal: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        0,
        "an unqualified record starts no model work: {output}"
    );
    assert!(!fixture.run.join("comparison").exists());
}

/// Unrelated or swapped runtime builds cannot authorize a measured arm: the
/// explicit build inputs are refused before any installation or dispatch.
#[test]
fn unrelated_or_swapped_builds_cannot_authorize_a_measured_arm() {
    // Swapped sources: the baseline build records the candidate checkout and
    // the candidate build the project working tree.
    let fixture = Fixture::new("swapped-builds");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture_build(
        &fixture.state,
        "h-build",
        &checkout.path,
        &programs().launcher,
        "baseline",
    );
    fixture_build(
        &fixture.state,
        "ha-build",
        &fixture.proj,
        &programs().launcher,
        "candidate",
    );
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("does not match the frozen baseline source"),
        "a swapped baseline build is refused before any dispatch: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        0,
        "{output}"
    );
    assert!(
        !fixture
            .run
            .join("comparison/baseline/runtime.json")
            .exists(),
        "the refused build is never installed"
    );

    // An unrelated source: the candidate build is a valid build of a
    // different checkout and must not enter the comparison.
    let fixture = Fixture::new("unrelated-build");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    let unrelated = fixture.root.join("unrelated-kit");
    for directory in ["crates/one/src", "global/agents"] {
        fs::create_dir_all(unrelated.join(directory)).unwrap();
    }
    fs::write(
        unrelated.join("Cargo.toml"),
        "[package]\nname = \"unrelated\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    fs::write(unrelated.join("Cargo.lock"), "# unrelated lock\n").unwrap();
    fs::write(unrelated.join("crates/one/src/lib.rs"), "// unrelated\n").unwrap();
    fixture_build(
        &fixture.state,
        "h-build",
        &fixture.proj,
        &programs().launcher,
        "baseline",
    );
    fixture_build(
        &fixture.state,
        "ha-build",
        &unrelated,
        &programs().launcher,
        "candidate",
    );
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("does not match the frozen candidate source"),
        "an unrelated candidate build is refused before any dispatch: {output}"
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        0,
        "{output}"
    );
    assert!(
        !fixture
            .run
            .join("comparison/candidate/runtime.json")
            .exists(),
        "the refused build is never installed"
    );
}

/// Workload B has its own durable card and its own removal authority: a
/// refusal blocks both measured arms before either dispatch, and the informed
/// approval unblocks them.
#[test]
fn workload_removal_decision_gates_both_measured_arms() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("workload-removal");
    let mut spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    spec["comparison"]["workload_removal"] =
        json!({"proposal": "workload-alpha", "target": "workload-target"});
    let comparison = spec["comparison"].take();
    fixture.write_spec(&[("comparison", comparison)], None);
    let proposed = fixture.feedback(&[
        "removal-propose",
        "--item",
        &fixture.workload_card,
        "--proposal",
        "workload-alpha",
        "--target",
        "workload-target",
        "--evidence",
        "evidence-b",
        "--loss",
        "workload-loss",
    ]);
    assert!(proposed.status.success(), "{}", text(&proposed));
    let refused = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.workload_card,
        "--decision",
        "refuse",
        "--proposal",
        "workload-alpha",
        "--target",
        "workload-target",
        "--loss",
        "workload-loss",
    ]);
    assert!(refused.status.success(), "{}", text(&refused));
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("workload")
            && (output.contains("refused by the user") || output.contains("declined")),
        "the workload removal refusal gates both arms: {output}"
    );
    let attempts = fixture.cursor()["attempts"].as_array().unwrap().len();
    assert_eq!(attempts, 0, "neither measured arm starts: {output}");

    // The informed approval unblocks the baseline arm first.
    let approved = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.workload_card,
        "--decision",
        "approve",
        "--proposal",
        "workload-alpha",
        "--target",
        "workload-target",
        "--actions",
        "experiment",
        "--loss",
        "workload-loss",
    ]);
    assert!(approved.status.success(), "{}", text(&approved));
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 1, "{output}");
    assert_eq!(attempts[0]["role"], "baseline");
}

/// The observed conversation must carry the declared model and effort: a
/// rollout that records a different model or effort refuses that arm instead
/// of entering the comparison as unverified evidence.
#[test]
fn wrong_observed_model_or_effort_refuses_the_arm() {
    let _serial = INSTALL.lock().unwrap();
    // Wrong observed model.
    let fixture = Fixture::new("wrong-model");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("wrong-model-session");
    fixture.simulate_arm("baseline", "baseline", "solved", 50, 5.0, 2, 3, &session);
    rewrite_rollout(&fixture, "baseline", &session, "another-model", "low");
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("recorded model another-model instead of the declared fixture-glyph-1"),
        "a wrong observed model refuses the arm: {output}"
    );
    assert_eq!(
        fixture.cursor()["comparison"]["baseline"]["accepted"],
        Value::Null
    );
    assert!(
        fixture.cursor()["comparison"]["baseline"]["condition"]
            .as_str()
            .is_some_and(|condition| condition.contains("another-model")),
        "the refusal is retained on the arm"
    );

    // Wrong observed reasoning effort.
    let fixture = Fixture::new("wrong-effort");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let session = session_id("wrong-effort-session");
    fixture.simulate_arm("baseline", "baseline", "solved", 50, 5.0, 2, 3, &session);
    rewrite_rollout(&fixture, "baseline", &session, "fixture-glyph-1", "xhigh");
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("recorded reasoning effort xhigh instead of the declared low"),
        "a wrong observed effort refuses the arm: {output}"
    );
    assert_eq!(
        fixture.cursor()["comparison"]["baseline"]["accepted"],
        Value::Null
    );
}

/// Rewrite the model and effort facts of one seeded arm rollout, exactly as a
/// differently configured client would have recorded them.
fn rewrite_rollout(fixture: &Fixture, arm: &str, session: &str, model: &str, effort: &str) {
    let rollout = fixture
        .arm_dir(arm)
        .join("home/sessions/2026/10/01")
        .join(format!("rollout-{session}.jsonl"));
    let text = fs::read_to_string(&rollout).unwrap();
    let mut lines: Vec<Value> = text
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for line in &mut lines {
        if line["type"] == "turn_context" {
            line["payload"]["model"] = json!(model);
            line["payload"]["effort"] = json!(effort);
        }
    }
    let rewritten = lines
        .iter()
        .map(|line| serde_json::to_string(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&rollout, format!("{rewritten}\n")).unwrap();
}

// ---------------------------------------------------------------- native path

/// The explicit child-only fixture mode: the harness forwards
/// `HARNESS_EXECUTOR_CHILD_FIXTURE_MODE` to the app-server child (never to the
/// host or the native frontend), and the installed launcher double serves the
/// ordinary control contract under it.
const CONTROL_CHILD_MODE: (&str, &str) =
    ("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE", "control-app-server");

/// The exact dispatch receipt recorded for one attempt.
fn attempt_receipt(fixture: &Fixture, attempt_id: &str) -> PathBuf {
    let cursor = fixture.cursor();
    let attempt = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == attempt_id)
        .expect("the controller recorded the attempt");
    PathBuf::from(
        attempt["retained"]["receipt"]
            .as_str()
            .or_else(|| attempt["receipt"].as_str())
            .expect("the attempt records a receipt"),
    )
}

/// Bounded wait until the host's own observation record of one attempt reached
/// a terminal state. The host writes it while the visible console runs.
fn wait_for_terminal_receipt(receipt: &Path) -> Value {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        if let Ok(bytes) = fs::read(receipt)
            && let Ok(record) = serde_json::from_slice::<Value>(&bytes)
            && matches!(
                record["observation"]["state"].as_str(),
                Some("completed" | "failed" | "defect" | "interrupted" | "stopped")
            )
        {
            return record;
        }
        assert!(
            std::time::Instant::now() < until,
            "the control-backed run produced no terminal observation record at {}",
            receipt.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// One successful model-free measured pair through the ordinary control-backed
/// dispatch route: real visible dispatch, native frontend attachment, the
/// host's own lifecycle receipt, settlement from that receipt, independent
/// acceptance and a published Beads decision - no seeded success receipt.
#[test]
fn one_real_control_backed_pair_settles_through_native_observation() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("native-pair");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_real_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    // First resume: preparation plus the real baseline dispatch.
    let resume = fixture.resume_dispatched(&[]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let cursor = fixture.cursor();
    let baseline = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["role"] == "baseline")
        .cloned()
        .unwrap_or_else(|| {
            panic!("the visible owner accepted the baseline attempt: {cursor}\n{output}")
        });
    let generation = baseline["binding"]["generation"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("the accepted dispatch recorded its generation: {baseline}\n{output}")
        });
    assert_eq!(baseline["state"], "started", "{cursor}");
    let baseline_receipt = attempt_receipt(&fixture, "base-1");
    let record = wait_for_terminal_receipt(&baseline_receipt);
    assert_eq!(
        record["observation"]["state"], "completed",
        "the controlled conversation completed through the real host: {record}"
    );
    assert!(
        !record["observation"]["session"].is_null(),
        "the host recorded the native session: {record}"
    );

    // A stale generation is refused through the real receipt, then restored.
    let real_bytes = fs::read(&baseline_receipt).unwrap();
    let mut stale: Value = serde_json::from_slice(&real_bytes).unwrap();
    stale["originatingLead"]["runGeneration"] = json!("stale-generation");
    fs::write(
        &baseline_receipt,
        serde_json::to_vec_pretty(&stale).unwrap(),
    )
    .unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let cursor = fixture.cursor();
    let baseline_attempt = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == "base-1")
        .unwrap();
    assert!(
        baseline_attempt["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("generation")),
        "a stale generation is refused from the real receipt: {cursor}"
    );
    assert_eq!(
        cursor["comparison"]["baseline"]["accepted"],
        Value::Null,
        "no comparison result is derived from the stale generation"
    );
    fs::write(&baseline_receipt, &real_bytes).unwrap();

    // The harness's own trusted-project addition is accepted, but any further
    // change to the consumed arm configuration still blocks the post-attempt
    // consumption instead of entering the comparison.
    let arm_config = fixture.arm_dir("baseline").join("home").join("config.toml");
    let served = fs::read_to_string(&arm_config).unwrap();
    assert!(
        served.contains("trust_level = \"trusted\""),
        "the real dispatch trusted its bound workspace: {served}"
    );
    fs::write(
        &arm_config,
        format!("{served}\n[fixture-drift]\nvalue = 1\n"),
    )
    .unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("the arm configuration changed since preparation"),
        "an added arm configuration key still refuses consumption: {output}"
    );
    assert_eq!(
        fixture.cursor()["comparison"]["baseline"]["accepted"],
        Value::Null,
        "a drifting arm configuration cannot enter the comparison"
    );
    fs::write(&arm_config, &served).unwrap();

    // Restoring the accepted generation settles the baseline from its own
    // receipt, runs the independent oracle, and dispatches the candidate.
    let resume = fixture.resume_dispatched(&[]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "the real baseline solution passed the frozen oracle: {status}\n{output}"
    );
    let candidate_receipt = attempt_receipt(&fixture, "cand-1");
    let record = wait_for_terminal_receipt(&candidate_receipt);
    assert_eq!(record["observation"]["state"], "completed", "{record}");

    // The candidate settles, is independently checked and the frozen policy
    // publishes its decision.
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    let comments =
        harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, &fixture.card)
            .unwrap();
    let records = harness_core::benefit_gate::parse_gate_comments(&comments);
    let assessment = harness_core::benefit_gate::assess(&records, &fixture.card)
        .expect("a decision is published");
    assert!(
        assessment
            .latest
            .revisions
            .as_deref()
            .is_some_and(|revisions| revisions == format!("{base}..{}", checkout.revision)),
        "the published decision carries the exact evaluated revisions: {comments:?}"
    );
    // Both arms settled from real host receipts, and each arm's committed
    // solution was verified independently; nothing was seeded.
    let cursor = fixture.cursor();
    for (id, _role) in [("base-1", "baseline"), ("cand-1", "candidate")] {
        let attempt = cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|attempt| attempt["id"] == id)
            .unwrap();
        assert_eq!(attempt["state"], "completed", "{id}: {cursor}");
        assert!(
            attempt["retained"]["receipt_sha256"].is_string(),
            "the attempt settled from its own retained receipt: {cursor}"
        );
    }
    for arm in ["baseline", "candidate"] {
        let oracle: Value =
            serde_json::from_slice(&fs::read(fixture.arm_dir(arm).join("oracle.json")).unwrap())
                .unwrap();
        assert_eq!(oracle["executed"], true, "{arm}: {oracle}");
        assert_eq!(oracle["checker_executed"], true, "{arm}: {oracle}");
        assert_eq!(oracle["passed"], true, "{arm}: {oracle}");
        assert!(
            status["comparison"][arm]["revision"].is_string(),
            "{arm}: the verified revision is retained: {status}"
        );
    }
    assert!(!generation.is_empty());
}

/// Controlled protocol observations through the same real path: a rollout that
/// records another model or effort refuses that arm, and nothing is adopted.
#[test]
fn real_control_dispatches_refuse_wrong_observed_model_and_effort() {
    let _serial = INSTALL.lock().unwrap();
    for (name, variable, wrong, declared) in [
        (
            "real-wrong-model",
            "HARNESS_IMPROVEMENT_FIXTURE_MODEL",
            "another-model",
            "fixture-glyph-1",
        ),
        (
            "real-wrong-effort",
            "HARNESS_IMPROVEMENT_FIXTURE_EFFORT",
            "xhigh",
            "low",
        ),
    ] {
        let fixture = Fixture::new(name);
        let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
        fixture.prepare_real_builds(&checkout);
        fixture.start_with_ready_candidate(&checkout, false);
        let resume = fixture.resume_dispatched(&[]);
        assert!(resume.status.success(), "{}", text(&resume));
        let baseline_receipt = attempt_receipt(&fixture, "base-1");
        wait_for_terminal_receipt(&baseline_receipt);
        // Settle the baseline and dispatch the candidate under the wrong
        // controlled observation.
        let resume = fixture.resume_dispatched(&[(variable, wrong)]);
        assert!(resume.status.success(), "{}", text(&resume));
        let candidate_receipt = attempt_receipt(&fixture, "cand-1");
        wait_for_terminal_receipt(&candidate_receipt);
        let resume = fixture.resume();
        let output = text(&resume);
        assert!(resume.status.success(), "{output}");
        assert!(
            output.contains(&format!(
                "recorded {} {wrong}",
                if variable.ends_with("MODEL") {
                    "model"
                } else {
                    "reasoning effort"
                }
            )) || output.contains(wrong),
            "the wrong observed {declared} is refused: {output}"
        );
        assert_eq!(
            fixture.cursor()["comparison"]["candidate"]["accepted"],
            Value::Null
        );
        assert!(
            !fixture
                .bd_comments(&fixture.card)
                .contains("benefit-gate v2"),
            "no decision is published from a refused arm"
        );
    }
}
