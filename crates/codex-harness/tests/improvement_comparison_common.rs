//! Entry-point checks for one declared sequential baseline/candidate
//! comparison. A real `bd` board, a real OpenSpec workspace, real Git
//! snapshots, the real runtime installation owner, a real local HTTP
//! observation fixture, a compiled checker program and the unchanged
//! `outcome-oracle` entry point are exercised; the two model conversations
//! are simulated through the same durable seam the controller recovery uses:
//! a dispatcher receipt with a terminal state is seeded and the controller
//! settles and consumes it on `resume`. No check here contacts a model or a
//! provider.
//!
//! Shared fixtures for `improvement_comparison_a` and `improvement_comparison_b`.
#![cfg(windows)]
// Cargo also discovers this file as an empty integration-test crate. The
// installed route excludes it. These allows cover that crate and helpers used
// by only one of the two including targets.
#![allow(dead_code, unused_imports)]

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

pub(super) fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

/// Serializes the arm-installing cases: the installation owner publishes
/// process-local PATH entries and holds shared installation locks.
pub(super) static INSTALL: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn bd_name() -> &'static str {
    "bd.exe"
}

pub(super) fn bd_executable() -> PathBuf {
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

pub(super) fn git(cwd: &Path, args: &[&str]) {
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

pub(super) fn git_output(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Runs one installed OpenSpec command exactly as the native adapter does.
pub(super) fn openspec(cwd: &Path, args: &[&str]) -> Output {
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

pub(super) fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

pub(super) fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

/// Model-free stand-in for the original Codex client, as the installation
/// owner's runtime check and the arm configuration exercise it.
pub(super) const UPSTREAM_SOURCE: &str = r##"
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

fn set_feature(text: &mut String, name: &str, enabled: bool) {
    let mut replaced = String::new();
    let mut in_features = false;
    let mut seen_features = false;
    let mut done = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            if in_features && !done {
                replaced.push_str(&format!("{name} = {enabled}\n"));
                done = true;
            }
            in_features = trimmed == "[features]";
            seen_features |= in_features;
        }
        if in_features && !done && trimmed.starts_with(name) && line.contains('=') {
            replaced.push_str(&format!("{name} = {enabled}\n"));
            done = true;
            continue;
        }
        replaced.push_str(line);
        replaced.push('\n');
    }
    if in_features && !done {
        replaced.push_str(&format!("{name} = {enabled}\n"));
        done = true;
    }
    *text = if done {
        replaced
    } else if seen_features {
        format!("{replaced}{name} = {enabled}\n")
    } else {
        format!("{replaced}\n[features]\n{name} = {enabled}\n")
    };
}

fn feature_state(text: &str, name: &str) -> bool {
    text.lines()
        .rev()
        .find(|line| line.trim_start().starts_with(name) && line.contains('='))
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
                (Some("disable"), Some(name @ ("hooks" | "code_mode"))) => {
                    set_feature(&mut text, name, false);
                    fs::write(&config, text).unwrap();
                }
                (Some("enable"), Some(name @ ("hooks" | "code_mode"))) => {
                    set_feature(&mut text, name, true);
                    fs::write(&config, text).unwrap();
                }
                (Some("list"), _) => {
                    println!("hooks stable {}", feature_state(&text, "hooks"));
                    println!("code_mode experimental {}", feature_state(&text, "code_mode"));
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
pub(super) const LAUNCHER_SOURCE: &str = r##"
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
pub(super) const CHECKER_SOURCE: &str = r##"
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

pub(super) struct FixturePrograms {
    pub(super) _root: tempfile::TempDir,
    pub(super) upstream: PathBuf,
    pub(super) launcher: PathBuf,
    pub(super) checker: PathBuf,
}

pub(super) fn programs() -> &'static FixturePrograms {
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

pub(super) fn compile_fixture(root: &Path, name: &str, source: &str) -> PathBuf {
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

pub(super) fn compile_identity(root: &Path, name: &str, id: &str) -> PathBuf {
    let source = format!("fn main() {{ println!(\"arm-tool-identity:{id}\"); }}");
    compile_fixture(root, name, &source)
}

pub(super) fn compile_relative(root: &Path, name: &str, id: &str, data_name: &str) -> PathBuf {
    let source = format!(
        "fn main() {{ let exe = std::env::current_exe().expect(\"exe\"); let data = exe.parent().expect(\"parent\").join(\"{data_name}\"); let body = std::fs::read_to_string(&data).unwrap_or_else(|error| format!(\"missing:{{error}}\")); println!(\"arm-tool-identity:{id}\"); println!(\"relative-data:{{body}}\"); println!(\"executed-from:{{}}\", exe.display()); }}"
    );
    compile_fixture(root, name, &source)
}

pub(super) fn prefixed_path(prefixes: &[&Path]) -> String {
    let mut entries: Vec<PathBuf> = prefixes.iter().map(|path| path.to_path_buf()).collect();
    if let Some(path) = std::env::var_os("PATH") {
        entries.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(entries)
        .unwrap()
        .to_str()
        .expect("PATH is Unicode")
        .to_owned()
}

pub(super) fn write_tool_probe(run: &Path, arm: &str, receipt: &Path, allow: &str) {
    let dir = run.join("tool-probes");
    fs::create_dir_all(&dir).unwrap();
    let probe = json!({
        "receipt": receipt,
        "allow": allow,
        "commands": [
            "codex-harness",
            "codex-harness.exe",
            "marker-tool",
        ],
    });
    fs::write(
        dir.join(format!("{arm}.json")),
        serde_json::to_vec_pretty(&probe).unwrap(),
    )
    .unwrap();
}

/// One published immutable build inside the owned state.
pub(super) fn fixture_build(
    state: &Path,
    name: &str,
    source: &Path,
    launcher: &Path,
    marker: &str,
) -> PathBuf {
    fixture_build_with(state, name, source, launcher, marker, &[])
}

pub(super) fn fixture_build_with(
    state: &Path,
    name: &str,
    source: &Path,
    launcher: &Path,
    marker: &str,
    replacements: &[(&str, &[u8])],
) -> PathBuf {
    let build = state.join("builds").join(name);
    fs::create_dir_all(&build).unwrap();
    let mut binaries = BTreeMap::new();
    for binary in build_identity::BINARIES {
        let bytes =
            if let Some((_, custom)) = replacements.iter().find(|(name, _)| *name == *binary) {
                custom.to_vec()
            } else if *binary == "codex.exe" {
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
pub(super) struct Fixture {
    pub(super) _root: tempfile::TempDir,
    pub(super) root: PathBuf,
    pub(super) proj: PathBuf,
    pub(super) wl: PathBuf,
    pub(super) home: PathBuf,
    pub(super) run: PathBuf,
    pub(super) spec: PathBuf,
    pub(super) bd: PathBuf,
    pub(super) card: String,
    /// Workload B's own durable hypothesis card.
    pub(super) workload_card: String,
    pub(super) state: PathBuf,
    pub(super) upstream: PathBuf,
    pub(super) launcher: PathBuf,
    pub(super) request: PathBuf,
    pub(super) request_sha256: String,
    pub(super) policy: PathBuf,
    pub(super) qualification: PathBuf,
    pub(super) workload_revision: String,
}

impl Fixture {
    pub(super) fn new(name: &str) -> Self {
        Self::with_workload_declaration(name, None)
    }

    /// The same owned synthetic run, with the frozen workload tree carrying
    /// its own `global/orchestration.toml` executor declaration exactly as an
    /// existing kit project does.
    pub(super) fn with_workload_declaration(name: &str, declaration: Option<&str>) -> Self {
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
        if let Some(declaration) = declaration {
            // Existing kit projects carry their own executor declaration; the
            // dispatch must use it instead of assuming a native default.
            fs::create_dir_all(wl.join("global")).unwrap();
            fs::write(wl.join("global/orchestration.toml"), declaration).unwrap();
        }
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
    pub(super) fn admit_workload(&self) -> String {
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

    pub(super) fn admit(&self) -> String {
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

    pub(super) fn feedback(&self, args: &[&str]) -> Output {
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

    pub(super) fn improve(&self, args: &[&str]) -> Output {
        self.improve_with_env(args, &[])
    }

    /// The controller is run as an ordinary operator process: the agent
    /// session markers that would legitimately refuse nested executor
    /// dispatch are removed, and only the explicitly declared environment
    /// reaches the dispatched arm.
    pub(super) fn improve_with_env(&self, args: &[&str], environment: &[(&str, &str)]) -> Output {
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

    pub(super) fn start(&self) -> Output {
        // The comparison cases advance one explicit boundary at a time; they
        // declare the single-step mode while the CLI default for new starts
        // stays continuous.
        self.improve(&[
            "start",
            "--run",
            self.run.to_str().unwrap(),
            "--spec",
            self.spec.to_str().unwrap(),
            "--supervision",
            "once",
        ])
    }

    pub(super) fn resume(&self) -> Output {
        self.improve(&["resume", "--run", self.run.to_str().unwrap()])
    }

    pub(super) fn stop(&self) -> Output {
        self.improve(&["stop", "--run", self.run.to_str().unwrap()])
    }

    /// One resume that may dispatch a measured arm: the child-only fixture
    /// mode reaches the installed launcher double through the host's explicit
    /// forward, exactly as the arm's own settings do.
    pub(super) fn resume_dispatched(&self, extra: &[(&str, &str)]) -> Output {
        let mut environment: Vec<(&str, &str)> = vec![CONTROL_CHILD_MODE];
        environment.extend_from_slice(extra);
        self.improve_with_env(
            &["resume", "--run", self.run.to_str().unwrap()],
            &environment,
        )
    }

    /// Host the measured arm in this process. A terminal tab would not carry
    /// the probe environment into the child, and would return before the child
    /// recorded which command it executed.
    pub(super) fn resume_in_process(&self, extra: &[(&str, &str)]) -> Output {
        let mut command = Command::new(manager());
        command
            .arg("improve")
            .args(["resume", "--run", self.run.to_str().unwrap()]);
        command.env_remove("HARNESS_EXECUTOR_SESSION");
        command.env_remove("HARNESS_EXECUTOR_FIXTURE_MODE");
        command.env_remove("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE");
        command.env_remove("HARNESS_EXECUTOR_RUN");
        command.env_remove("HARNESS_ORIGINATING_LEAD");
        command.env_remove("HARNESS_LEAD_THREAD");
        command.env_remove("HARNESS_LEAD_RECIPIENT");
        command.env_remove("WT_SESSION");
        command.env(CONTROL_CHILD_MODE.0, CONTROL_CHILD_MODE.1);
        for (name, value) in extra {
            command.env(name, value);
        }
        command.output().expect("improve runs")
    }

    pub(super) fn status_json(&self) -> Value {
        let status = self.improve(&["status", "--run", self.run.to_str().unwrap(), "--json"]);
        assert!(status.status.success(), "status: {}", text(&status));
        serde_json::from_str(&text(&status)).expect("status --json")
    }

    pub(super) fn cursor(&self) -> Value {
        serde_json::from_slice(&fs::read(self.run.join("cursor.json")).unwrap()).unwrap()
    }

    pub(super) fn write_cursor(&self, value: &Value) {
        fs::write(
            self.run.join("cursor.json"),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }

    pub(super) fn bd_comments(&self, item: &str) -> String {
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

pub(super) fn write_change(project: &Path, change: &str, group: &str, scenario: &str) {
    let dir = project.join("openspec/changes").join(change);
    fs::create_dir_all(dir.join("specs").join(group)).unwrap();
    fs::write(
        dir.join("proposal.md"),
        format!(
            "## Why\n\n{scenario}.\n\n## Measurement\n\nObserved problem: the {scenario} flow \
             repeats measurable work. Investigation scope: one frozen workload revision on this \
             fixture. Measurement question: how much accepted time does the repeated work cost? \
             Workload: the existing comparison operation linked from this change. Evidence: the \
             retained outcome record. Limits: one local fixture and one frozen revision.\n"
        ),
    )
    .unwrap();
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
    pub(super) fn write_spec(&self, replacements: &[(&str, Value)], remove: Option<&str>) {
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
        // The directed-measurement gate requires the declared scope beside the
        // frozen spec and the matching section in the hypothesis change.
        let scope = json!({
            "observed_problem": "the synthetic flow repeats measurable work",
            "investigation_scope": "one frozen workload revision on this fixture",
            "measurement_question": "how much accepted time does the repeated work cost?",
            "workload": {
                "operation": "comparison workload-b",
                "contract": "openspec/changes/add-synthetic/proposal.md#Measurement",
            },
            "evidence_references": ["retained outcome record: synthetic comparison"],
            "limits": "one local fixture and one frozen revision",
            "declaration_artifact": "proposal.md",
            "declaration_heading": "## Measurement",
        });
        fs::create_dir_all(&self.run).unwrap();
        fs::write(
            self.run.join("measurement-scope.json"),
            serde_json::to_vec_pretty(&scope).unwrap(),
        )
        .unwrap();
    }

    /// Allocate the ready candidate's owned worktree before the run starts and
    /// optionally commit the candidate treatment on its branch, so the
    /// prepared builds can be created from an exact source identity.
    pub(super) fn prepare_candidate(
        &self,
        change: Option<&str>,
    ) -> task_worktree::CandidateCheckout {
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
    pub(super) fn prepare_builds(&self, checkout: &task_worktree::CandidateCheckout) {
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
    pub(super) fn prepare_real_builds(&self, checkout: &task_worktree::CandidateCheckout) {
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

    /// The same two builds, with a distinct executable identity for the
    /// managed `codex-harness` command so a host can prove which arm ran.
    pub(super) fn prepare_identity_builds(
        &self,
        checkout: &task_worktree::CandidateCheckout,
        baseline_tool: &[u8],
        candidate_tool: &[u8],
    ) {
        let launcher = PathBuf::from(env!("CARGO_BIN_EXE_harness-executor-fixture"));
        fixture_build_with(
            &self.state,
            "h-build",
            &self.proj,
            &launcher,
            "baseline",
            &[("codex-harness.exe", baseline_tool)],
        );
        fixture_build_with(
            &self.state,
            "ha-build",
            &checkout.path,
            &launcher,
            "candidate",
            &[("codex-harness.exe", candidate_tool)],
        );
    }

    /// Start the run and replace the resulting idle cursor with the retained
    /// ready candidate the planning/implementation workflow reaches.
    pub(super) fn start_with_ready_candidate(
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

    pub(super) fn arm_dir(&self, arm: &str) -> PathBuf {
        self.run.join("comparison").join(arm)
    }

    /// Simulate one measured conversation: a pooled-style worktree of the
    /// arm's dispatch checkout with a committed solution, the retained
    /// terminal receipt of its visible dispatch and the rollout the native
    /// client would have written into the arm home.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn simulate_arm(
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
        let mut lines = vec![
            json!({
                "type": "session_meta",
                "payload": {"id": session, "base_instructions": "fixture"},
            }),
            json!({
                "type": "turn_context",
                "payload": {"model": "fixture-glyph-1", "effort": "low", "turn_id": "turn-1"},
            }),
        ];
        // One recorded outer function call per completed tool item: the
        // unbatched conversation these fixtures simulate.
        for index in 1..=tool_calls {
            lines.push(json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call",
                    "call_id": format!("call-{index}"),
                    "name": "exec_command",
                },
            }));
        }
        lines.push(json!({
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
        }));
        // The cumulative snapshot a recorded conversation carries beside its
        // per-response usage; the accounting reader requires it to measure a
        // complete total.
        lines.push(json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": 100,
                        "cached_input_tokens": 40,
                        "output_tokens": 20,
                        "reasoning_output_tokens": 5,
                        "total_tokens": 120,
                    },
                },
            },
        }));
        let lines = lines
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
                    "rounds": 1,
                    "toolOperationCounts": {"commandExecution": tool_calls},
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

pub(super) fn session_id(seed: &str) -> String {
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
pub(super) struct ObservationServer {
    pub(super) port: u16,
    pub(super) body: std::sync::Arc<std::sync::Mutex<String>>,
}

impl ObservationServer {
    pub(super) fn start(initial: &str) -> Self {
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

    pub(super) fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    pub(super) fn set(&self, body: &str) {
        *self.body.lock().unwrap() = body.to_owned();
    }
}

/// The predeclared policy that requires one additional independent unit beyond
/// the run's own declared plan unit.
pub(super) fn corroboration_scope_policy() -> Value {
    json!({
        "schema": 1,
        "objective": "time",
        "basis": "efficiency",
        "meaningfulEffectPercent": 10.0,
        "tolerancePercent": 5.0,
        "requireAcceptance": true,
        "taskMix": "one frozen workload case",
        "stopping": {"maxAttemptsPerArm": 1, "requiredUnits": 2},
        "repeatedSelection": "predeclared",
        "tradeOff": Value::Null,
        "uncertainty": "unknown evidence stays inconclusive",
        "horizonTasks": 1.0,
        "overhead": {
            "implementationSeconds": 0.0,
            "evaluationSeconds": 0.0,
            "maintenanceSecondsPerTask": 0.0,
        },
    })
}

/// One admitted hypothesis card on the fixture board; admission reuses one
/// card per mechanism/conditions identity, so this returns the existing card
/// rather than creating a duplicate.
pub(super) fn admit_prior_hypothesis_card(fixture: &Fixture, mechanism: &str) -> String {
    let out = fixture.feedback(&[
        "hypothesis-admit",
        "--mechanism",
        mechanism,
        "--conditions",
        "local-tool-runs",
        "--observation",
        "token-audit:prior#7",
        "--predicted",
        "the prior task completed under the same conditions",
        "--counterexample",
        "the prior solution is unavailable",
        "--acceptance",
        "the independent oracle passed for the prior task",
        "--spec",
        "openspec/changes/prior",
        "--basis",
        "token-audit:prior#7",
    ]);
    assert!(out.status.success(), "prior admit: {}", text(&out));
    let printed = text(&out);
    let id = printed
        .strip_prefix("hypothesis ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default()
        .to_owned();
    assert!(!id.is_empty(), "prior admission named no card: {printed}");
    id
}

/// A retained completed real task that a prior run already recorded: the
/// frozen pre-solution copy plus a committed answer the retention never keeps.
pub(super) fn prior_retained_task(
    fixture: &Fixture,
    revision: &str,
    owner: &str,
    case_id: &str,
    mechanism: &str,
) -> harness_core::improvement_experiment::RetainedTask {
    let completed = harness_core::task_worktree::frozen_copy(
        &fixture.proj,
        revision,
        &fixture.root.join(format!("{case_id}-completed")),
    )
    .unwrap();
    fs::write(
        completed.path.join("answer.txt"),
        "prior solution: earlier attempt answer\n",
    )
    .unwrap();
    git(&completed.path, &["add", "."]);
    git(
        &completed.path,
        &[
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "user.name=Fixture",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "prior answer",
        ],
    );
    harness_core::improvement_experiment::retain_completed_task(
        &completed,
        &fixture.root.join(format!("{case_id}-retained")),
        &harness_core::improvement_experiment::TaskRetention {
            owner: owner.to_owned(),
            case_id: case_id.to_owned(),
            experiment: "exp-prior".to_owned(),
            mechanism: mechanism.to_owned(),
            conditions: "local-tool-runs".to_owned(),
            oracle: "oracle-7".to_owned(),
            acceptance: "acceptance/run-9".to_owned(),
        },
    )
    .unwrap()
}

/// Record one retained task on its owner card through the durable board
/// writer, exactly as the controller does at the decision boundary, so a later
/// run discovers it from the board instead of a run-local index.
pub(super) fn record_prior_retention_on_board(
    fixture: &Fixture,
    owner: &str,
    retained: &harness_core::improvement_experiment::RetainedTask,
) {
    let draft = harness_core::board_hypothesis::RetentionDraft {
        case_id: retained.case_id.clone(),
        experiment: retained.experiment.clone(),
        mechanism: retained.mechanism.clone(),
        conditions: retained.conditions.clone(),
        revision: retained.replay.source_revision.clone(),
        frozen: retained.replay.revision.clone(),
        tree: retained.replay.tree_sha256.clone(),
        oracle: retained.oracle.clone(),
        acceptance: retained.acceptance.clone(),
        replay: retained.replay.path.display().to_string(),
        detail: Some("prior completed real task retained for corroboration".to_owned()),
    };
    let bounded = harness_core::board_hypothesis::BoundedRetention::try_from_draft(draft)
        .expect("the fixture retention draft is bounded");
    harness_core::board_hypothesis::record_retention(&fixture.bd, &fixture.proj, owner, &bounded)
        .expect("the owner card records the retention");
}

/// Write one API-observed qualification produced by the owner against the
/// local observation fixture, and declare the same explicit client inputs in
/// the run spec: the observed client file is the overlay the arms consume, and
/// an optional bearer transport input is declared through `observation_auth`.
pub(super) fn install_api_observed_qualification(
    fixture: &Fixture,
    server: &ObservationServer,
    bearer: Option<&str>,
) -> (PathBuf, Option<PathBuf>) {
    use harness_core::outcome_qualification::{
        ApiObservationPlan, ApiObservedPolicy, ClientInput, DeclaredObservation, LocalRunner,
        MaterialIdentity, ObservationBinding, ObservationRequest, QualificationAttempt,
        RepeatabilityPolicy, RunnerRecord, collect_observations, qualify_api_observed,
    };
    let overlay = fixture.root.join("client-overlay.toml");
    fs::write(&overlay, "model_context_window = 262144\n").unwrap();
    let bearer_path = bearer.map(|name| {
        let path = fixture.root.join("client-bearer.token");
        fs::write(&path, format!("synthetic-{name}\n")).unwrap();
        path
    });
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
            bearer_auth: bearer.map(str::to_owned),
        },
    };
    let mut inputs = vec![ClientInput {
        name: "overlay".to_owned(),
        path: overlay.clone(),
    }];
    if let (Some(name), Some(path)) = (bearer, &bearer_path) {
        inputs.push(ClientInput {
            name: name.to_owned(),
            path: path.clone(),
        });
    }
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
    if let (Some(name), Some(path)) = (bearer, &bearer_path) {
        spec["comparison"]["observation_auth"] = json!({"name": name, "path": path});
    }
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    (overlay, bearer_path)
}

/// Rewrite the model and effort facts of one seeded arm rollout, exactly as a
/// differently configured client would have recorded them.
pub(super) fn rewrite_rollout(
    fixture: &Fixture,
    arm: &str,
    session: &str,
    model: &str,
    effort: &str,
) {
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

/// Rewrites one arm's recorded rollout so only the first `keep` outer function
/// calls remain. The completed tool-item observation is unchanged, which is
/// exactly the batched-call shape the call and operation counters must keep
/// distinct.
pub(super) fn keep_recorded_calls(fixture: &Fixture, arm: &str, session: &str, keep: usize) {
    let rollout = fixture
        .arm_dir(arm)
        .join("home/sessions/2026/10/01")
        .join(format!("rollout-{session}.jsonl"));
    let text = fs::read_to_string(&rollout).unwrap();
    let mut kept = 0usize;
    let mut lines = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let value: Value = serde_json::from_str(line).unwrap();
        if value["type"] == "response_item" && value["payload"]["type"] == "function_call" {
            kept += 1;
            if kept > keep {
                continue;
            }
        }
        lines.push(line.to_owned());
    }
    fs::write(&rollout, format!("{}\n", lines.join("\n"))).unwrap();
}

// ---------------------------------------------------------------- native path

/// The explicit child-only fixture mode: the harness forwards
/// `HARNESS_EXECUTOR_CHILD_FIXTURE_MODE` to the app-server child (never to the
/// host or the native frontend), and the installed launcher double serves the
/// ordinary control contract under it.
pub(super) const CONTROL_CHILD_MODE: (&str, &str) =
    ("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE", "control-app-server");

/// The exact dispatch receipt recorded for one attempt.
pub(super) fn attempt_receipt(fixture: &Fixture, attempt_id: &str) -> PathBuf {
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
pub(super) fn wait_for_terminal_receipt(receipt: &Path) -> Value {
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

pub(super) fn load_json(path: &Path) -> Value {
    serde_json::from_slice(
        &fs::read(path).unwrap_or_else(|error| {
            panic!("missing comparison evidence {}: {error}", path.display())
        }),
    )
    .unwrap_or_else(|error| panic!("unreadable comparison evidence {}: {error}", path.display()))
}

pub(super) fn comparison_reasons(report: &Value) -> Vec<String> {
    report["comparisons"][0]["excluded_reasons"]
        .as_array()
        .unwrap_or_else(|| panic!("the controller published no comparison: {report}"))
        .iter()
        .filter_map(|reason| reason.as_str().map(str::to_owned))
        .collect()
}

pub(super) fn stage_token_workflow_package(
    home: &Path,
    identity: &str,
    vendor: &[u8],
    adapter: &[u8],
) {
    let vendor_path = home.join("harness/rtk/packages/0.48.0/rtk.exe");
    if !vendor_path.is_file() {
        fs::create_dir_all(vendor_path.parent().unwrap()).unwrap();
        fs::write(&vendor_path, vendor).unwrap();
    }
    let adapter_dir = home.join(format!("harness/rtk/build/{identity}"));
    let adapter_path = adapter_dir.join("harness-rtk.exe");
    if !adapter_path.is_file() {
        fs::create_dir_all(&adapter_dir).unwrap();
        fs::write(&adapter_path, adapter).unwrap();
    }
    let record = adapter_dir.join("build.json");
    if !record.is_file() {
        fs::write(
            &record,
            serde_json::to_vec(&json!({
                "sourceIdentity": identity,
                "binarySha256": build_identity::hash_bytes(adapter),
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

pub(super) fn settle_dispatched_pair(fixture: &Fixture) {
    let baseline = session_id(&format!("{}-baseline", fixture.card));
    fixture.simulate_arm("baseline", "baseline", "solved", 20, 5.0, 2, 3, &baseline);
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let candidate = session_id(&format!("{}-candidate", fixture.card));
    fixture.simulate_arm(
        "candidate",
        "candidate",
        "solved",
        20,
        1.0,
        1,
        1,
        &candidate,
    );
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
}

pub(super) fn tool_receipt(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap_or_else(|error| {
        panic!(
            "the hosted arm did not record command identity at {}: {error}",
            path.display()
        )
    });
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("tool receipt is not JSON: {error}"))
}

pub(super) fn assert_tool(receipt: &Value, command: &str, identity: &str, foreign: &str) {
    let probe = &receipt["commands"][command];
    let stdout = probe["stdout"].as_str().unwrap_or("");
    let resolved = probe["resolved"].as_str().unwrap_or("");
    assert!(
        probe["executed"] == true,
        "{command} was not executed from the selected installation: {probe}"
    );
    assert!(
        stdout.contains(identity),
        "{command} did not consume the selected binary: {probe}"
    );
    assert!(
        !stdout.contains(foreign) && !resolved.to_ascii_lowercase().contains(foreign),
        "{command} resolved a foreign copy: {probe}"
    );
}

pub(super) fn assert_shell(receipt: &Value) {
    let stdout = receipt["shell"]["stdout"].as_str().unwrap_or("");
    assert!(
        stdout.starts_with("7"),
        "the owner PowerShell 7 must remain usable: {}",
        receipt["shell"]
    );
    assert!(
        !stdout.to_ascii_lowercase().contains("path-view")
            && !stdout.to_ascii_lowercase().contains("harness\\bin"),
        "the owner PowerShell was relocated into the selected installation: {}",
        receipt["shell"]
    );
}

pub(super) fn assert_relative_tool(receipt: &Value, command: &str, sentinel: &str) {
    let probe = &receipt["commands"][command];
    let stdout = probe["stdout"].as_str().unwrap_or("");
    assert!(
        stdout.contains(&format!("relative-data:{sentinel}")),
        "{command} did not read data beside its original directory: {probe}"
    );
    assert!(
        !stdout.to_ascii_lowercase().contains("path-view"),
        "{command} was relocated: {probe}"
    );
}

// ---------------------------------------------------------------------------
// Workload B artifacts: an arm that passes the frozen independent acceptance
// leaves an exact, selectable solution under B's own card; a failed arm never
// does. The comparison still completes without inventing a B candidate.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// A hypothesis that targets controller or evaluation components still runs
// beneath the unchanged frozen supervisor: it cannot activate itself, rewrite
// its decision policy or check digests, or weaken the frozen acceptance.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// One short real-operation comparison through the controller.
//
// The predeclared selection is `method=real-operation` `claim=local-operation`:
// each measured arm executes the declared operation program through the real
// heavy-command route inside its own frozen checkout and with its own prepared
// runtime. The operation itself runs a real build/check cycle (Cargo over a
// frozen crate, unchanged-input reuse, changed-input invalidation, the built
// binary's output) and the arm's own runtime emits the workload result the
// frozen checker verifies independently. No model conversation is opened.
// ---------------------------------------------------------------------------

/// The declared real operation: one real build/check cycle over the frozen
/// crate, plus the arm runtime's own workload result. It exits nonzero on a
/// genuine failure (a build that does not compile, a missing reuse or a wrong
/// built output); no counter is simulated anywhere in the cycle.
pub(super) const SHORT_OPERATION_SOURCE: &str = r##"
use std::{env, fs, path::{Path, PathBuf}, process::{Command, exit}};

struct BuildReport {
    compiled: bool,
    fresh: usize,
    binaries: usize,
}

fn build(crate_dir: &Path, target: &Path) -> Option<BuildReport> {
    let out = Command::new("cargo")
        .args(["build", "--offline", "--message-format=json"])
        .current_dir(crate_dir)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!("cargo build failed: {}", String::from_utf8_lossy(&out.stderr));
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut compiled = false;
    let mut fresh = 0usize;
    let mut binaries = 0usize;
    for line in stdout.lines() {
        if !line.contains("compiler-artifact") {
            continue;
        }
        if line.contains("\"fresh\":true") {
            fresh += 1;
        } else if line.contains("\"fresh\":false") {
            compiled = true;
        }
        if line.contains("\"executable\":\"") {
            binaries += 1;
        }
    }
    Some(BuildReport { compiled, fresh, binaries })
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let fail_build = args.iter().any(|arg| arg == "--fail-build");
    let mut values = args.iter().filter(|arg| *arg != "--fail-build");
    let workspace = PathBuf::from(values.next().cloned().unwrap_or_default());
    let runtime = PathBuf::from(values.next().cloned().unwrap_or_default());
    let target = PathBuf::from(values.next().cloned().unwrap_or_default());
    let crate_dir = workspace.join("build");
    let cargo_target = target.join("cargo");
    if fail_build {
        fs::write(crate_dir.join("src/main.rs"), "fn main() { this is not rust }\n")
            .expect("the broken input is written");
    }
    let tool = runtime.join("codex-harness.exe");
    if !tool.is_file() {
        eprintln!("the arm runtime tool {} is missing", tool.display());
        exit(5);
    }
    let emitted = Command::new(&tool).arg("solution").output().expect("the runtime tool runs");
    if !emitted.status.success() {
        eprintln!("the arm runtime tool failed");
        exit(6);
    }
    let solution = String::from_utf8_lossy(&emitted.stdout).trim().to_owned();
    let Some(first) = build(&crate_dir, &cargo_target) else { exit(7) };
    if !first.compiled || first.binaries == 0 {
        eprintln!("the first build compiled nothing");
        exit(8);
    }
    let Some(second) = build(&crate_dir, &cargo_target) else { exit(9) };
    if second.compiled || second.fresh == 0 {
        eprintln!(
            "the unchanged-input build did not reuse the compilation (compiled={} fresh={})",
            second.compiled, second.fresh
        );
        exit(10);
    }
    let source = crate_dir.join("src/main.rs");
    let original = fs::read(&source).expect("the build source is readable");
    let mut changed = original.clone();
    changed.extend_from_slice(b"\n// invalidation probe\n");
    fs::write(&source, &changed).expect("the build source is writable");
    let Some(third) = build(&crate_dir, &cargo_target) else { exit(11) };
    let restored = fs::write(&source, &original).is_ok();
    if !third.compiled || !restored {
        eprintln!("the changed-input build did not recompile or the source was not restored");
        exit(12);
    }
    let binary = cargo_target.join("debug").join("short-operation-workload.exe");
    let run = Command::new(&binary).output().expect("the built binary runs");
    let printed = String::from_utf8_lossy(&run.stdout).trim().to_owned();
    if printed != "workload-ok" {
        eprintln!("the built binary printed {printed:?}");
        exit(13);
    }
    fs::write(workspace.join("solution.txt"), format!("{solution}\n"))
        .expect("the workload result is written");
    fs::write(
        target.join("report.json"),
        format!(
            "{{\"first_compiled\":{},\"second_compiled\":{},\"second_fresh\":{},\"third_compiled\":{}}}\n",
            first.compiled, second.compiled, second.fresh, third.compiled
        ),
    )
    .expect("the operation report is written");
    println!("operation-report written");
}
"##;

/// The predeclared comparison policy of the short real-operation fixture.
pub(super) fn short_operation_policy() -> Value {
    use harness_core::improvement_policy::{
        EffectPath, ExperimentMethod, ExperimentSelection, experiment_selection_clause,
    };
    let selection = ExperimentSelection {
        method: ExperimentMethod::RealOperation,
        claim: EffectPath::LocalOperation,
        outcome: "the declared build/check cycle is measured in both variants".to_owned(),
        rationale: "the chosen unit exercises the claimed local build mechanism".to_owned(),
        controls: "frozen inputs, one predeclared pair and the accepted baseline conditions"
            .to_owned(),
        projection: "one bounded experiment and bounded retention cost".to_owned(),
        baseline: "the accepted revision excluding the candidate edit".to_owned(),
        stopping:
            "stop after the declared attempts and escalate only for a named missing observation"
                .to_owned(),
    };
    json!({
        "schema": 1,
        "objective": "time",
        "basis": "efficiency",
        "meaningfulEffectPercent": 10.0,
        "tolerancePercent": 5.0,
        "requireAcceptance": true,
        "taskMix": "one frozen build/check workload",
        "stopping": {"maxAttemptsPerArm": 1, "requiredUnits": 1},
        "repeatedSelection": "predeclared",
        "tradeOff": null,
        "uncertainty": format!(
            "unknown evidence stays inconclusive; {}",
            experiment_selection_clause(&selection)
        ),
        "horizonTasks": 1.0,
        "overhead": {
            "implementationSeconds": 0.0,
            "evaluationSeconds": 0.0,
            "maintenanceSecondsPerTask": 0.0,
        },
    })
}

/// One resume that runs the direct operation under an owned isolated heavy
/// account, so the real admitted build is independent of shared contention.
pub(super) fn resume_short_operation(fixture: &Fixture) -> Output {
    let heavy = fixture
        .root
        .join("heavy-account")
        .to_string_lossy()
        .into_owned();
    let cpu = fixture
        .root
        .join("cpu-account")
        .to_string_lossy()
        .into_owned();
    fixture.improve_with_env(
        &["resume", "--run", fixture.run.to_str().unwrap()],
        &[
            ("CODEX_HARNESS_HEAVY_ACCOUNT", &heavy),
            ("CODEX_HARNESS_CPU_ACCOUNT", &cpu),
        ],
    )
}

/// The short real-operation fixture: a frozen build crate, two real arm
/// runtime tools whose workload results differ, the compiled declared
/// operation and the predeclared real-operation selection.
pub(super) fn short_operation_fixture(
    name: &str,
    baseline_solution: &str,
    candidate_solution: &str,
    operation_arguments: &[&str],
) -> (Fixture, task_worktree::CandidateCheckout) {
    let fixture = Fixture::new(name);
    // The frozen workload carries a real build/check crate the declared
    // operation compiles through the real Cargo owner.
    fs::create_dir_all(fixture.wl.join("build/src")).unwrap();
    fs::write(
        fixture.wl.join("build/Cargo.toml"),
        "[package]\nname = \"short-operation-workload\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        fixture.wl.join("build/src/main.rs"),
        "fn main() {\n    println!(\"workload-ok\");\n}\n",
    )
    .unwrap();
    fs::write(
        fixture.wl.join("build/Cargo.lock"),
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"short-operation-workload\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    git(&fixture.wl, &["add", "."]);
    git(
        &fixture.wl,
        &["commit", "-qm", "add the workload build crate"],
    );
    let revision = git_output(&fixture.wl, &["rev-parse", "HEAD"]);
    let mut spec: Value = load_json(&fixture.spec);
    // The run keeps its declared local runner (the conversation route's
    // route); this comparison's direct-operation selection opens no
    // conversation, so the route is declared but never executed, and the
    // absent qualification record proves the short path triggers no
    // unrelated model qualification.
    spec["comparison"]["task"]["revision"] = json!(revision);
    spec["qualification"] = json!(fixture.root.join("absent-qualification.json"));
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();

    // The declared measurement scope names the existing operation; the
    // executable declaration binds exactly that identity.
    let operation_identity = "cargo build --offline --manifest-path build/Cargo.toml";
    let scope = json!({
        "observed_problem": "the unchanged build repeats compilation",
        "investigation_scope": "one frozen build/check cycle at this revision",
        "measurement_question": "does the real cycle reuse unchanged input?",
        "workload": {
            "operation": operation_identity,
            "contract": "openspec/changes/add-workload/proposal.md#Measurement",
        },
        "evidence_references": ["retained operation report of the declared build/check cycle"],
        "limits": "one local fixture and one frozen revision",
        "declaration_artifact": "proposal.md",
        "declaration_heading": "## Measurement",
    });
    fs::write(
        fixture.run.join("measurement-scope.json"),
        serde_json::to_vec_pretty(&scope).unwrap(),
    )
    .unwrap();

    let programs_root = fixture.root.join("short-operation-programs");
    fs::create_dir_all(&programs_root).unwrap();
    let operation = compile_fixture(&programs_root, "short-operation", SHORT_OPERATION_SOURCE);
    let baseline_tool = compile_fixture(
        &programs_root,
        "baseline-tool",
        &format!("fn main() {{ println!(\"{baseline_solution}\"); }}"),
    );
    let candidate_tool = compile_fixture(
        &programs_root,
        "candidate-tool",
        &format!("fn main() {{ println!(\"{candidate_solution}\"); }}"),
    );
    fs::write(
        fixture.run.join("operation.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "identity": operation_identity,
            "program": operation,
            "arguments": operation_arguments,
            "timeout_seconds": 600,
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        &fixture.policy,
        serde_json::to_vec_pretty(&short_operation_policy()).unwrap(),
    )
    .unwrap();

    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    let baseline_bytes = fs::read(&baseline_tool).unwrap();
    let candidate_bytes = fs::read(&candidate_tool).unwrap();
    fixture.prepare_identity_builds(&checkout, &baseline_bytes, &candidate_bytes);
    fixture.start_with_ready_candidate(&checkout, false);
    (fixture, checkout)
}

pub(super) fn operation_arguments(extra: &[&str]) -> Vec<String> {
    ["{workspace}", "{runtime}", "{target}"]
        .iter()
        .map(|value| (*value).to_owned())
        .chain(extra.iter().map(|value| (*value).to_owned()))
        .collect()
}

pub(super) fn status_json_phase(fixture: &Fixture) -> String {
    fixture.status_json()["phase"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

// ------------------------------------------------- frozen nuisance-control plan

/// The frozen nuisance-control plan of the direct-operation checks: both arms
/// start from empty owned state, the controller's predetermined baseline-first
/// order is declared, shared caches are disclosed as unobserved, load is
/// recorded without any utilization correction, faults are classified by
/// observed effect and retries follow the policy's own stopping rule.
pub(super) fn fixture_nuisance_plan() -> harness_core::improvement_policy::NuisanceControlPlan {
    use harness_core::improvement_policy::{
        FaultRule, InitialState, LoadRule, NuisanceControlPlan, OrderRule, RetryRule, SharedState,
    };
    NuisanceControlPlan {
        initial: InitialState::OwnedCold,
        recipe: None,
        shared: SharedState::Unobserved,
        order: OrderRule::Fixed,
        seed: None,
        pairs: None,
        load: LoadRule::Recorded,
        faults: FaultRule::ObservedEffect,
        retries: RetryRule::PolicyStopping,
    }
}

/// Append the canonical nuisance-control clause to the fixture policy and
/// write it back before the comparison is first advanced.
pub(super) fn write_nuisance_policy(
    path: &Path,
    plan: &harness_core::improvement_policy::NuisanceControlPlan,
) {
    let mut policy: Value =
        serde_json::from_slice(&fs::read(path).expect("fixture policy exists")).unwrap();
    let uncertainty = policy["uncertainty"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    policy["uncertainty"] = json!(format!(
        "{uncertainty}; {}",
        harness_core::improvement_policy::nuisance_control_clause(plan)
    ));
    fs::write(path, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
}

// ---------------------------------------------------------------------------
// Independent owned calibration controls
//
// These controls exercise the real controller, report and decision path with
// owned inputs whose expected properties are fixed before the run: an arm
// blocked on an unrelated owned holder, an arm whose own admitted work is
// preserved, a real cache benefit, a quality failure and evidence the
// collector cannot use. Every document is written through the same public
// writers a real dispatch uses and lands in the same owned evidence
// directory, so the accounting, attribution and policy owners read it exactly
// as they read a produced trace. The controls prove the measurement path;
// they do not replace the real local-model and installed-loop acceptance.
// ---------------------------------------------------------------------------

/// One command item a controlled arm records, with producer milliseconds
/// measured backwards from the seeding instant.
pub(super) struct ControlledCommand {
    pub(super) id: String,
    pub(super) started_ms_ago: u64,
    pub(super) completed_ms_ago: u64,
    /// An unrelated owned holder blocked this admission; otherwise the
    /// resource was granted immediately and the interval is the arm's own
    /// admitted work.
    pub(super) blocked: bool,
    /// The opaque producer process id the selected route reports on the item;
    /// only a blocked command carries one.
    pub(super) process_id: Option<String>,
}

/// The evidence defect one control injects. Each defect is a boundary,
/// ownership, clock or retention fault the report/decision path must expose
/// instead of presenting a complete correction.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlledDefect {
    None,
    /// The blocked admission keeps no measured end clamp.
    BoundaryDropped,
    /// The blocked admission records an ancestry but no control-owner OS link.
    OwnershipUnverified,
    /// The blocked admission was recorded in another clock domain.
    ClockMisaligned,
}

/// One controlled measured arm: its own recorded work script, the attempt
/// clock window the accounting owner reads, and the injected evidence defect.
pub(super) struct ControlledArm {
    pub(super) arm: &'static str,
    pub(super) role: &'static str,
    pub(super) session: String,
    pub(super) solution: &'static str,
    pub(super) sleep_ms: u64,
    pub(super) started_offset_seconds: f64,
    pub(super) window_seconds: f64,
    pub(super) commands: Vec<ControlledCommand>,
    pub(super) defect: ControlledDefect,
    /// Mark the retained compact activity and raw detail as overflowed.
    pub(super) overflow: bool,
    /// Omit the attempt clock so no producer time can be mapped.
    pub(super) no_clock: bool,
}

impl ControlledArm {
    pub(super) fn new(
        arm: &'static str,
        role: &'static str,
        session: &str,
        solution: &'static str,
    ) -> Self {
        Self {
            arm,
            role,
            session: session.to_owned(),
            solution,
            sleep_ms: 0,
            started_offset_seconds: 0.0,
            window_seconds: 0.0,
            commands: Vec::new(),
            defect: ControlledDefect::None,
            overflow: false,
            no_clock: false,
        }
    }
}

pub(super) fn controlled_command(
    id: &str,
    started_ms_ago: u64,
    completed_ms_ago: u64,
    blocked: bool,
) -> ControlledCommand {
    ControlledCommand {
        id: id.to_owned(),
        started_ms_ago,
        completed_ms_ago,
        blocked,
        process_id: blocked.then(|| OPAQUE_PRODUCER.to_owned()),
    }
}

/// The opaque producer process id the selected app-server route reports on a
/// command item; it is not the spawned OS pid.
pub(super) const OPAQUE_PRODUCER: &str = "10307";

/// One pair of owned controlled arms through the real controller: prepare the
/// frozen runtimes, dispatch each arm, install its controlled evidence, settle
/// it through the ordinary consumption path and publish the frozen decision.
/// The policy binding is fixed before either arm starts.
pub(super) fn controlled_pair(
    name: &str,
    work_efficiency_binding: bool,
    baseline: ControlledArm,
    candidate: ControlledArm,
) -> Fixture {
    let fixture = Fixture::new(name);
    if work_efficiency_binding {
        let mut policy: Value = load_json(&fixture.policy);
        policy["uncertainty"] = json!(harness_core::infrastructure_accounting::binding_clause(
            harness_core::infrastructure_accounting::MetricView::WorkEfficiency,
            harness_core::infrastructure_accounting::Mechanism::None,
        ));
        fs::write(&fixture.policy, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
    }
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let dispatch = fixture.resume();
    assert!(dispatch.status.success(), "{}", text(&dispatch));
    seed_controlled_arm(&fixture, &baseline);
    let settle = fixture.resume();
    assert!(settle.status.success(), "{}", text(&settle));
    seed_controlled_arm(&fixture, &candidate);
    let decided = fixture.resume();
    assert!(decided.status.success(), "{}", text(&decided));
    fixture
}

pub(super) fn unix_ms_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub(super) fn filetime_of_ms(ms: i64) -> u64 {
    (ms.max(0) as u64)
        .saturating_mul(10_000)
        .saturating_add(116_444_736_000_000_000)
}

/// The recorded terminal receipt and rollout of one controlled arm, and the
/// admission documents the accounting owner reads for it.
pub(super) fn seed_controlled_arm(fixture: &Fixture, plan: &ControlledArm) {
    fixture.simulate_arm(
        plan.arm,
        plan.role,
        plan.solution,
        plan.sleep_ms,
        plan.started_offset_seconds,
        2,
        plan.commands.len() as u64,
        &plan.session,
    );
    let attempt_id = fixture.cursor()["attempts"]
        .as_array()
        .expect("the controller recorded the attempts")
        .iter()
        .rev()
        .find(|attempt| attempt["role"] == plan.role)
        .and_then(|attempt| attempt["id"].as_str())
        .expect("the controller recorded this arm's dispatch")
        .to_owned();
    let receipt = attempt_receipt(fixture, &attempt_id);
    let (frequency, sample) = harness_core::heavy_command_trace::sample_clock()
        .expect("the shared performance counter is readable");
    let boot =
        harness_core::heavy_command_trace::boot_filetime().expect("the boot identity is readable");
    let window_ns = (plan.window_seconds * 1_000_000_000.0) as u64;
    let started_qpc = sample
        .qpc
        .saturating_sub(window_ns.saturating_mul(frequency) / 1_000_000_000);
    let started_filetime = sample.filetime.saturating_sub(window_ns / 100);
    let span = sample.sample_span_ticks.unwrap_or(0);

    // The retained observation is patched before the arm settles: it keeps the
    // attempt window in the host clock domain and, for an overflow control,
    // records that the compact activity and raw detail were truncated.
    let mut record: Value = load_json(&receipt);
    {
        let observation = record["observation"]
            .as_object_mut()
            .expect("the seeded receipt carries an observation");
        if !plan.no_clock {
            observation.insert(
                "clock".to_owned(),
                json!({
                    "startedQpc": started_qpc,
                    "endedQpc": sample.qpc,
                    "startedFiletime": started_filetime,
                    "endedFiletime": sample.filetime,
                    "frequency": frequency,
                    "boot": boot,
                    "startedSpan": span,
                    "endedSpan": span,
                }),
            );
        }
        if plan.overflow {
            observation.insert("detailTruncated".to_owned(), json!(true));
            observation.insert("activityTruncated".to_owned(), json!(true));
        }
    }
    fs::write(&receipt, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    write_controlled_rollout(fixture, plan);

    let directory = fixture
        .run
        .join("comparison")
        .join("queue-evidence")
        .join(&attempt_id);
    fs::create_dir_all(&directory).unwrap();
    let now_ms = unix_ms_now();
    for command in &plan.commands {
        let started_ms = now_ms - i64::try_from(command.started_ms_ago).unwrap_or(0);
        let completed_ms = now_ms - i64::try_from(command.completed_ms_ago).unwrap_or(0);
        write_controlled_admission(
            &directory,
            &attempt_id,
            command,
            plan.defect,
            frequency,
            boot,
            started_qpc,
            started_filetime,
            span,
            started_ms,
            completed_ms,
        );
    }
}

/// The rollout the arm's client would have written: the session identity, the
/// declared model and effort, one command item per recorded operation and the
/// matching function call. No token usage is recorded, exactly as a local
/// route whose usage reader measured nothing would leave it.
pub(super) fn write_controlled_rollout(fixture: &Fixture, plan: &ControlledArm) {
    let sessions = fixture.arm_dir(plan.arm).join("home/sessions/2026/10/01");
    fs::create_dir_all(&sessions).unwrap();
    let now_ms = unix_ms_now();
    let mut lines = vec![
        json!({
            "type": "session_meta",
            "payload": {"id": plan.session, "base_instructions": "fixture"},
        }),
        json!({
            "type": "turn_context",
            "payload": {"model": "fixture-glyph-1", "effort": "low", "turn_id": "turn-1"},
        }),
    ];
    for (index, command) in plan.commands.iter().enumerate() {
        let mut item = json!({"type": "CommandExecution", "id": command.id.clone()});
        if let Some(process_id) = &command.process_id {
            item["process_id"] = json!(process_id);
            item["source"] = json!("unified_exec_startup");
        }
        lines.push(json!({
            "type": "event_msg",
            "payload": {
                "type": "item_completed",
                "turn_id": format!("turn-{}", index + 1),
                "started_at_ms": now_ms - i64::try_from(command.started_ms_ago).unwrap_or(0),
                "completed_at_ms": now_ms - i64::try_from(command.completed_ms_ago).unwrap_or(0),
                "item": item,
            },
        }));
        lines.push(json!({
            "type": "response_item",
            "payload": {
                "type": "function_call",
                "call_id": command.id.clone(),
                "name": "exec_command",
            },
        }));
    }
    let text = lines
        .iter()
        .map(|line| serde_json::to_string(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        sessions.join(format!("rollout-{}.jsonl", plan.session)),
        format!("{text}\n"),
    )
    .unwrap();
}

/// One owned admission document for a controlled command: a waited grant
/// blocked on an unrelated tagged holder, or an immediate grant of the arm's
/// own admitted work. The document is written through the public writer the
/// real heavy-command route uses, in the same clock domain the attempt
/// observation records.
#[allow(clippy::too_many_arguments)]
pub(super) fn write_controlled_admission(
    directory: &Path,
    attempt_id: &str,
    command: &ControlledCommand,
    defect: ControlledDefect,
    frequency: u64,
    boot: u64,
    started_qpc: u64,
    started_filetime: u64,
    span: u64,
    started_ms: i64,
    completed_ms: i64,
) {
    use harness_core::heavy_command_trace::{
        ClockSample, Correlation, EpisodeDraft, EpisodeKind, HolderClass, HolderFinding,
        HolderReason, TerminalKind,
    };
    let at = |ms: i64| -> ClockSample {
        let delta_ns = filetime_of_ms(ms)
            .saturating_sub(started_filetime)
            .saturating_mul(100);
        ClockSample {
            qpc: started_qpc.saturating_add(delta_ns.saturating_mul(frequency) / 1_000_000_000),
            filetime: filetime_of_ms(ms),
            sample_span_ticks: Some(span),
        }
    };
    let recorded_frequency = if defect == ControlledDefect::ClockMisaligned {
        frequency.saturating_add(1)
    } else {
        frequency
    };
    let mut draft = EpisodeDraft {
        admission_id: format!("admission-{}", command.id),
        parent_admission_id: None,
        correlation: Correlation {
            attempt_id: Some(attempt_id.to_owned()),
            tool_call_id: Some(command.id.clone()),
            command_id: Some(command.id.clone()),
        },
        episode: EpisodeKind::Queue,
        terminal: if command.blocked {
            TerminalKind::WaitedGrant
        } else {
            TerminalKind::ImmediateGrant
        },
        failure: None,
        frequency: recorded_frequency,
        admitted_at: None,
        queue_start: None,
        queue_end: None,
        waited: false,
        holders: Vec::new(),
        holder_samples: 1,
        holder_changed: false,
        poll_resolution_ns: 50_000_000,
        observed_poll_gap_ns: None,
        endpoint_start_ns: None,
        endpoint_end_ns: None,
        boot: Some(boot),
        payload_started: Some(true),
    };
    if command.blocked {
        draft.waited = true;
        draft.queue_start = Some(at(started_ms));
        draft.queue_end = Some(at(completed_ms));
        draft.admitted_at = draft.queue_end;
        draft.holders = vec![HolderFinding {
            classification: HolderClass::OtherAttempt,
            reason: HolderReason::Tagged,
            slot: Some(1),
        }];
        draft.holder_samples = 3;
        draft.observed_poll_gap_ns = Some(50_000_000);
        draft.endpoint_start_ns = Some(1_000_000);
        if defect != ControlledDefect::BoundaryDropped {
            draft.endpoint_end_ns = Some(1_000_000);
        }
    } else {
        draft.admitted_at = Some(at(completed_ms));
    }
    harness_core::heavy_command_trace::write_episode(directory, &draft)
        .expect("the controlled admission document is written");
    if command.blocked
        && defect != ControlledDefect::OwnershipUnverified
        && let Some(opaque) = &command.process_id
    {
        // The control owner's private OS link for the process it spawned, in
        // the same shape the real route records while the process is alive.
        harness_core::heavy_command_trace::write_process_ancestry(
            directory,
            &draft.admission_id,
            &[harness_core::heavy_command_trace::ProcessAncestor {
                pid: 4242,
                creation_time: 99,
            }],
        )
        .unwrap();
        harness_core::heavy_command_trace::write_command_process_link(
            directory,
            &harness_core::heavy_command_trace::CommandProcessLink {
                item_id: command.id.clone(),
                opaque_process_id: opaque.clone(),
                os_pid: 4242,
                creation_time: 99,
            },
        )
        .unwrap();
    }
}

pub(super) fn attempt_of_arm<'a>(report: &'a Value, arm: &str) -> &'a Value {
    report["attempts"]
        .as_array()
        .unwrap_or_else(|| panic!("the report carries attempts: {report}"))
        .iter()
        .find(|attempt| attempt["arm"] == arm)
        .unwrap_or_else(|| panic!("the report carries the {arm} attempt: {report}"))
}

pub(super) fn evaluation_reasons(evaluation: &Value) -> Vec<String> {
    evaluation["reasons"]
        .as_array()
        .unwrap_or_else(|| panic!("the evaluation carries reasons: {evaluation}"))
        .iter()
        .filter_map(|reason| reason.as_str().map(str::to_owned))
        .collect()
}

/// One controlled arm whose single command waited on an unrelated owned
/// holder for a known interval; the injected defect changes only the evidence
/// the accounting owner reads.
pub(super) fn delayed_arm(
    arm: &'static str,
    session_seed: &str,
    delay_ms: u64,
    defect: ControlledDefect,
) -> ControlledArm {
    let mut plan = ControlledArm::new(arm, arm, &session_id(session_seed), "solved");
    plan.started_offset_seconds = (delay_ms as f64) / 1000.0 + 3.0;
    plan.window_seconds = (delay_ms as f64) / 1000.0 + 14.0;
    plan.commands = vec![controlled_command(
        "call_heavy_blocked_1",
        delay_ms + 1_500,
        1_500,
        true,
    )];
    plan.defect = defect;
    plan
}

/// One controlled arm whose commands were all granted immediately: each
/// recorded interval is the arm's own admitted work.
pub(super) fn granted_arm(
    arm: &'static str,
    session_seed: &str,
    started_offset_seconds: f64,
    commands: &[(&str, u64, u64)],
) -> ControlledArm {
    let mut plan = ControlledArm::new(arm, arm, &session_id(session_seed), "solved");
    plan.started_offset_seconds = started_offset_seconds;
    let longest = commands
        .iter()
        .map(|(_, started, _)| *started)
        .max()
        .unwrap_or(0);
    plan.window_seconds = started_offset_seconds + (longest as f64) / 1000.0 + 4.0;
    plan.commands = commands
        .iter()
        .map(|(id, started, completed)| controlled_command(id, *started, *completed, false))
        .collect();
    plan
}
