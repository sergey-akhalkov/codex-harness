//! Native `codex-harness improve` controller checks over synthetic owned
//! inputs: a real `bd` board, a real OpenSpec workspace and private run state.
//! Every dispatch gate is exercised model-free; no check here contacts a
//! model, a provider or a subscription.
#![cfg(windows)]

use harness_core::build_identity::{BINARIES, hash_bytes};
use harness_core::{board_lifecycle, build_identity, core_install, installation_state::PathScope};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

fn powershell() -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH for the owner PowerShell 7");
    std::env::split_paths(&path)
        .map(|dir| dir.join("pwsh.exe"))
        .find(|candidate| candidate.is_file())
        .expect("owner PowerShell 7 (pwsh.exe) on PATH")
}

fn start_sleeper() -> Child {
    Command::new(powershell())
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 120",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("sleeper starts")
}

fn child_running(child: &mut Child) -> bool {
    child.try_wait().expect("child state is readable").is_none()
}

fn wait_gone(child: &mut Child) -> bool {
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        if !child_running(child) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

fn bd_name() -> &'static str {
    if cfg!(windows) { "bd.exe" } else { "bd" }
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

/// One owned synthetic project: git checkout, bd board, OpenSpec workspace
/// with a complete change, one admitted hypothesis card and a run spec.
struct Fixture {
    _root: tempfile::TempDir,
    root: PathBuf,
    proj: PathBuf,
    home: PathBuf,
    run: PathBuf,
    spec: PathBuf,
    bd: PathBuf,
    card: String,
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
        let home = root.join("codex-home");
        let run = root.join("runs").join(name);
        fs::create_dir_all(&proj).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(run.parent().unwrap()).unwrap();
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
        git(&proj, &["add", "."]);
        git(&proj, &["commit", "-qm", "seed"]);

        let bd = bd_executable();
        let init = Command::new(&bd)
            .args([
                "init",
                "--skip-agents",
                "--non-interactive",
                "--quiet",
                "--prefix",
                "bdct",
            ])
            .current_dir(&proj)
            .output()
            .expect("bd init runs");
        assert!(
            init.status.success(),
            "bd init: {}",
            String::from_utf8_lossy(&init.stderr)
        );

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
        write_change(
            &proj,
            "## Why\n\nSynthetic.\n",
            "## Context\n\nSynthetic.\n",
            "## 1. Work\n\n- [ ] 1.1 Do the synthetic thing.\n",
        );

        let mut fixture = Self {
            _root: cleanup,
            root,
            proj,
            home,
            run,
            spec: PathBuf::new(),
            bd,
            card: String::new(),
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

    /// The run spec JSON with optional top-level replacements.
    fn write_spec(&self, replacements: &[(&str, Value)], remove: Option<&str>) {
        let mut document = json!({
            "schema": 1,
            "run": "loop-fixture",
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
                "independent_acceptance": "the oracle checker executes",
                "meaningful_effect": "fewer repeated loads",
                "operating_conditions": "cold context",
                "comparison_policy": "matched pairs",
                "stopping_rule": "two repeats",
            },
            "base_revision": git_output(&self.proj, &["rev-parse", "HEAD"]),
            "writable_scope": ["crates/one"],
            "runner": Value::Null,
            "local_runner": Value::Null,
            "qualification": Value::Null,
            "publication_scope": ["experiment"],
            "oracle": "outcome-oracle:private-request",
            "removal": Value::Null,
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
}

fn write_change(proj: &Path, proposal: &str, design: &str, tasks: &str) {
    let change = proj.join("openspec/changes/add-synthetic");
    fs::create_dir_all(change.join("specs/synthetic")).unwrap();
    fs::write(change.join("proposal.md"), proposal).unwrap();
    fs::write(change.join("design.md"), design).unwrap();
    fs::write(change.join("tasks.md"), tasks).unwrap();
    fs::write(
        change.join("specs/synthetic/spec.md"),
        "## ADDED Requirements\n\n### Requirement: Synthetic behavior\n\nThe system SHALL do the synthetic thing.\n\n#### Scenario: Synthetic case\n\n- **WHEN** the probe runs\n- **THEN** it reports success\n",
    )
    .unwrap();
}

fn attempt_json(id: &str, role: &str, state: &str, receipt: Option<&Path>) -> Value {
    let owner = format!("loop-fixture-{id}");
    let binding = receipt.map(|path| {
        json!({
            "slot": 1,
            "owner": owner,
            "generation": "gen-1",
            "receipt": path.display().to_string(),
            "session": Value::Null,
            "host": Value::Null,
        })
    });
    json!({
        "id": id,
        "role": role,
        "owner": owner,
        "binding": binding,
        "retained": Value::Null,
        "title": format!("CEx (ds) - {id}"),
        "profile": "ds",
        "model": Value::Null,
        "model_provider": Value::Null,
        "reasoning_effort": Value::Null,
        "checkout": Value::Null,
        "assignment": Value::Null,
        "receipt": receipt.map(|path| path.display().to_string()),
        "result": Value::Null,
        "detail": Value::Null,
        "state": state,
        "reason": Value::Null,
        "reuse_refused": Value::Null,
        "started_ms": 1,
        "updated_ms": 1,
    })
}

fn seed_attempt(fixture: &Fixture, attempt: Value, phase: &str) {
    let mut cursor = fixture.cursor();
    cursor["attempts"].as_array_mut().unwrap().push(attempt);
    cursor["phase"] = json!(phase);
    fixture.write_cursor(&cursor);
}

fn seed_receipt(path: &Path, state: &str, exit_code: Option<i32>) {
    seed_bound_receipt(
        path,
        "loop-fixture-implementer-1",
        "gen-1",
        state,
        exit_code,
        None,
        None,
    );
}

/// One dispatch receipt shaped exactly as the native dispatcher writes it:
/// the pooled slot binding, the per-dispatch generation and the observed
/// lifecycle, with an optional recorded session/host.
#[allow(clippy::too_many_arguments)]
fn seed_bound_receipt(
    path: &Path,
    owner: &str,
    generation: &str,
    state: &str,
    exit_code: Option<i32>,
    session: Option<&str>,
    host: Option<(&Path, u32)>,
) {
    let host = host.map(|(program, pid)| {
        let user = harness_core::process_service::current_user().unwrap();
        let identity =
            harness_core::process_service::ServiceProcess::observe(pid, program, 0, &user)
                .expect("the recorded host is identifiable by its exact identity")
                .identity();
        json!({"pid": identity.pid, "created": identity.creation_time, "program": program})
    });
    fs::write(
        path,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "launcher": "C:\\fixture\\codex.exe",
            "profile": "ds",
            "mode": "tui",
            "visible": true,
            "host": "windows-terminal-tab",
            "slot": {
                "index": 1,
                "path": "C:\\fixture\\slot-1",
                "source": "C:\\fixture\\source",
                "owner": owner,
                "base": "base",
                "remote": "origin",
                "branch": Value::Null,
            },
            "originatingLead": {
                "schema": 1,
                "threadId": "fixture-thread",
                "runGeneration": generation,
                "dispatcher": {"pid": 1, "creationTime": 1, "program": "C:\\fixture\\dispatcher.exe"},
            },
            "observation": {
                "schema": 1,
                "coverage": "native",
                "reason": Value::Null,
                "state": state,
                "session": session,
                "previousSession": Value::Null,
                "exitCode": exit_code,
                "events": 0,
                "messages": 0,
                "toolCalls": 0,
                "malformed": 0,
                "cause": Value::Null,
                "host": host.unwrap_or_else(|| json!({"pid": 4294967294u32, "created": 1, "program": "C:\\missing\\host.exe"})),
                "result": Value::Null,
                "detail": Value::Null,
                "updatedMs": 1,
            }
        }))
        .unwrap(),
    )
    .unwrap();
}

/// One synthetic prepared runtime: an owned native build state with a verified
/// record and its recorded binaries, exactly the layout selection consumes.
fn prepare_runtime(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let state = root.join(format!("state-{name}"));
    let build = state.join("builds").join(format!("{name}-build"));
    fs::create_dir_all(&build).unwrap();
    fs::write(state.join("owner"), b"codex-harness-native-state-v1\n").unwrap();
    let mut binaries = BTreeMap::new();
    for name in BINARIES {
        let bytes = format!("fixture binary {name}\n");
        fs::write(build.join(name), &bytes).unwrap();
        binaries.insert((*name).to_owned(), hash_bytes(bytes.as_bytes()));
    }
    let mut files = BTreeMap::new();
    files.insert("src/lib.rs".to_owned(), hash_bytes(b"// synthetic\n"));
    let record = json!({
        "schema": 1,
        "source_root": root,
        "source": {
            "sha256": hash_bytes(&serde_json::to_vec(&files).unwrap()),
            "files": files,
        },
        "rustc": "rustc fixture",
        "cargo": "cargo fixture",
        "target": "x86_64-pc-windows-msvc",
        "profile": "release",
        "binaries": binaries,
    });
    fs::write(
        build.join("build.json"),
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();
    (state, build)
}

#[test]
fn help_status_and_incomplete_start_are_model_free() {
    let help = Command::new(manager())
        .args(["improve", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success(), "{}", text(&help));
    let help = text(&help);
    for word in ["start", "status", "select", "stop", "resume"] {
        assert!(help.contains(word), "{help}");
    }

    let fixture = Fixture::new("incomplete");
    // Missing required input: the schema refuses before anything else.
    fixture.write_spec(&[], Some("base_revision"));
    let out = fixture.start();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("base_revision") || text(&out).contains("missing field"),
        "{}",
        text(&out)
    );
    assert!(!fixture.run.join("spec.json").exists());

    // Inconsistent input: a declared model without its provider.
    fixture.write_spec(
        &[(
            "runner",
            json!({"profile": "ds", "model": "deepseek-flash", "model_provider": Value::Null, "reasoning_effort": Value::Null}),
        )],
        None,
    );
    let out = fixture.start();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("model and model_provider"),
        "{}",
        text(&out)
    );

    // A planning target the installed OpenSpec CLI cannot resolve refuses
    // start and creates no dependent state.
    fixture.write_spec(
        &[(
            "specification",
            json!({
                "project": fixture.proj,
                "change": "absent-change",
                "store": Value::Null,
                "planning_root": fixture.proj,
            }),
        )],
        None,
    );
    let out = fixture.start();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        !fixture.run.join("spec.json").exists(),
        "no run state is created"
    );

    // A readable status is available only for an existing run.
    let out = fixture.improve(&["status", "--run", fixture.run.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("no improvement run exists"),
        "{}",
        text(&out)
    );
}

#[test]
fn start_requires_the_board_card_and_a_complete_openspec_change() {
    let fixture = Fixture::new("prerequisites");
    fixture.write_spec(&[], None);

    // Missing card.
    fixture.write_spec(&[("hypothesis_item", json!("bdct-absent"))], None);
    let out = fixture.start();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("bdct-absent"), "{}", text(&out));

    // A card that references a different change cannot launch this run: every
    // implementation task needs its own complete linked change.
    let other = fixture.feedback(&[
        "hypothesis-admit",
        "--mechanism",
        "other-mechanism",
        "--conditions",
        "other-conditions",
        "--observation",
        "token-audit:findings#13",
        "--predicted",
        "other effect",
        "--counterexample",
        "other counterexample",
        "--acceptance",
        "other acceptance",
        "--spec",
        "openspec/changes/another-change",
        "--basis",
        "evidence-2",
    ]);
    assert!(other.status.success(), "{}", text(&other));
    let other_id = text(&other)
        .strip_prefix("hypothesis ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default()
        .to_owned();
    fixture.write_spec(&[], None);
    let mut document: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    document["hypothesis_item"] = json!(other_id);
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let out = fixture.start();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("instead of the run's change"),
        "{}",
        text(&out)
    );

    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert!(
        out.status.success(),
        "the valid fixture starts: {}",
        text(&out)
    );
    assert!(fixture.run.join("planning.json").is_file());
    let status = fixture.improve(&["status", "--run", fixture.run.to_str().unwrap(), "--json"]);
    assert!(status.status.success(), "{}", text(&status));
    let report: Value = serde_json::from_str(&text(&status)).expect("status --json");
    assert_eq!(report["phase"], "blocked");
    assert_eq!(report["runner"], Value::Null);
    let dispatch = report["dispatch"]["reason"].as_str().unwrap();
    assert!(dispatch.contains("model inputs are pending"), "{dispatch}");
    assert!(
        report["pending_phases"]
            .as_str()
            .unwrap()
            .contains("baseline-attempt"),
        "{report}"
    );

    // Duplicate run ownership.
    let out = fixture.start();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("duplicate run ownership"),
        "{}",
        text(&out)
    );
}

#[test]
fn incomplete_openspec_artifacts_prevent_start_and_dispatch() {
    let fixture = Fixture::new("planning");
    // Remove one mandatory artifact: the real OpenSpec validation must refuse
    // the run before any dependent dispatch and create no run state.
    fs::remove_file(fixture.proj.join("openspec/changes/add-synthetic/tasks.md")).unwrap();
    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("missing OpenSpec planning prerequisite")
            || text(&out).contains("tasks"),
        "{}",
        text(&out)
    );
    assert!(!fixture.run.join("spec.json").exists());

    // The acceptance section is required by the contract, not only the files.
    write_change(
        &fixture.proj,
        "## Why\n\nSynthetic.\n",
        "## Context\n\nSynthetic.\n",
        "## 1. Work\n\n- [ ] 1.1 Do the synthetic thing.\n",
    );
    let mut document: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    document["experiment"]["acceptance_heading"] = json!("#### Scenario: Absent");
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let out = fixture.start();
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("acceptance section"), "{}", text(&out));
}

#[test]
fn the_surface_gate_blocks_dispatch_without_visibility() {
    let fixture = Fixture::new("surface");
    fs::write(
        fixture.home.join("config.toml"),
        "[profiles.ds]\nmodel = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    fixture.write_spec(
        &[(
            "runner",
            json!({
                "profile": "ds",
                "model": "deepseek-flash",
                "model_provider": "deepseek",
                "reasoning_effort": "max",
            }),
        )],
        None,
    );
    // A declared runner must match the installed profile binding exactly.
    let mut document: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    document["runner"]["model"] = json!("another-model");
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let out_text = text(&out);
    assert!(
        out_text.contains("does not match the installed profile binding"),
        "{out_text}"
    );
    assert!(
        !fixture
            .run
            .join("assignments")
            .read_dir()
            .unwrap()
            .any(|entry| entry.is_ok())
    );

    // With the exact binding but no launcher the gate reports missing
    // visibility and still starts nothing.
    fs::remove_dir_all(&fixture.run).unwrap();
    fixture.write_spec(
        &[(
            "runner",
            json!({
                "profile": "ds",
                "model": "deepseek-flash",
                "model_provider": "deepseek",
                "reasoning_effort": "max",
            }),
        )],
        None,
    );
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let out_text = text(&out);
    assert!(out_text.contains("missing visibility"), "{out_text}");
    assert!(out_text.contains("launcher"), "{out_text}");
    assert!(
        !fixture
            .run
            .join("assignments")
            .read_dir()
            .unwrap()
            .any(|entry| entry.is_ok()),
        "no assignment is written when visibility is missing"
    );
    assert!(
        !fixture.home.join("harness/executor-pool").exists(),
        "no dispatch artifacts are created without a surface"
    );

    // Restoring the launcher makes the same gate ready (no hidden fallback was
    // needed and no model was called to get here).
    let launcher = fixture.home.join("harness/bin/codex.exe");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    fs::write(&launcher, "fixture launcher").unwrap();
    let status = fixture.improve(&["status", "--run", fixture.run.to_str().unwrap(), "--json"]);
    let report: Value = serde_json::from_str(&text(&status)).unwrap();
    assert_eq!(report["dispatch"]["state"], "ready", "{report}");
}

#[test]
fn removal_authority_blocks_candidate_selection_until_it_is_current() {
    let fixture = Fixture::new("removal");
    fixture.write_spec(
        &[(
            "removal",
            json!({"proposal": "remove-x", "target": "skill-x"}),
        )],
        None,
    );
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let run_arg = fixture.run.to_str().unwrap().to_owned();

    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(
        text(&select).contains("removal approval is pending"),
        "{}",
        text(&select)
    );

    // Interruption at the pending-approval boundary preserves the recorded
    // state: the resume resolves the current (still missing) decision instead
    // of inheriting anything from before the stop, and no decision is ever
    // recorded on the user's behalf.
    let stopped = fixture.improve(&["stop", "--run", &run_arg]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(
        text(&select).contains("no reviewed removal proposal"),
        "{}",
        text(&select)
    );
    assert!(
        removal_comments(&fixture)
            .iter()
            .all(|record| !record.contains("removal-decision v1")),
        "a resume never records a removal decision"
    );

    let propose = fixture.feedback(&[
        "removal-propose",
        "--item",
        &fixture.card,
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--evidence",
        "evidence-7",
        "--loss",
        "retired-skill",
        "--preview",
        "preview-1",
        "--detail",
        "consumer list: none known",
    ]);
    assert!(propose.status.success(), "{}", text(&propose));
    let decide = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "approve",
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--actions",
        "experiment",
        "--loss",
        "retired-skill",
        "--basis",
        "user-turn-7",
    ]);
    assert!(decide.status.success(), "{}", text(&decide));

    // The approval covers the experimental removal: selection now passes its
    // authority gate and fails only on the pending runtime preparation.
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(
        text(&select).contains("runtime preparation"),
        "{}",
        text(&select)
    );

    // A stop and resume around the unchanged approval reuses it: the proposal
    // and decision records survive interruption and no approval ritual is
    // repeated.
    let stopped = fixture.improve(&["stop", "--run", &run_arg]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(
        text(&select).contains("runtime preparation"),
        "the interrupted approved scope resumes without another question: {}",
        text(&select)
    );
    assert_eq!(
        removal_comments(&fixture)
            .iter()
            .filter(|record| record.contains("removal-decision v1"))
            .count(),
        1,
        "the unchanged approval is not asked for again"
    );

    // A withdrawal blocks the next effect; it too survives an interruption
    // and no resume silently restores the withdrawn consent.
    let withdraw = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "withdraw",
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--basis",
        "user-turn-8",
    ]);
    assert!(withdraw.status.success(), "{}", text(&withdraw));
    let stopped = fixture.improve(&["stop", "--run", &run_arg]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(text(&select).contains("withdrawn"), "{}", text(&select));
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(
        text(&select).contains("withdrawn"),
        "the withdrawal survives the interruption: {}",
        text(&select)
    );

    // A fresh informed decision is a new basis: the next effect follows the
    // latest decision instead of the withdrawn one.
    let reapprove = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "approve",
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--actions",
        "experiment",
        "--loss",
        "retired-skill",
        "--basis",
        "user-turn-9",
    ]);
    assert!(reapprove.status.success(), "{}", text(&reapprove));
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(
        text(&select).contains("runtime preparation"),
        "{}",
        text(&select)
    );

    // A changed reviewed detail is a changed proposal: the frozen consent no
    // longer covers it and dependent selection blocks again.
    let change = fixture.feedback(&[
        "removal-propose",
        "--item",
        &fixture.card,
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--evidence",
        "evidence-7",
        "--loss",
        "retired-skill",
        "--preview",
        "preview-1",
        "--detail",
        "consumer list: one indirect caller found",
    ]);
    assert!(change.status.success(), "{}", text(&change));
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(
        text(&select).contains("changed") && text(&select).contains("fresh decision"),
        "{}",
        text(&select)
    );
    // Interruption preserves every required input: both reviewed proposal
    // versions and the decided records stay readable for the updated
    // decision and for the recorded restoration route.
    let stopped = fixture.improve(&["stop", "--run", &run_arg]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let records = removal_comments(&fixture);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.contains("removal-proposal v1"))
            .count(),
        2,
        "both reviewed proposal versions survive the interruption: {records:?}"
    );
    assert!(
        records
            .iter()
            .any(|record| record.contains("preview=preview-1"))
            && records
                .iter()
                .any(|record| record.contains("loss=retired-skill")),
        "the evidence and restoration inputs survive the interruption: {records:?}"
    );
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(
        text(&select).contains("changed") && text(&select).contains("fresh decision"),
        "{}",
        text(&select)
    );

    // A refusal is not a measurement failure and the same request is not
    // repeated without a new basis; it also survives an interruption.
    let refuse = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "refuse",
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--basis",
        "user-turn-8",
    ]);
    assert!(refuse.status.success(), "{}", text(&refuse));
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(text(&select).contains("user declined"), "{}", text(&select));
    let stopped = fixture.improve(&["stop", "--run", &run_arg]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(
        text(&select).contains("user declined"),
        "the refusal survives the interruption: {}",
        text(&select)
    );
    assert_eq!(
        removal_comments(&fixture)
            .iter()
            .filter(|record| record.contains("removal-decision v1"))
            .count(),
        4,
        "no resume asks for the removal decision again"
    );
}

/// Every native removal record currently on the hypothesis card, as the
/// decision owner wrote it.
fn removal_comments(fixture: &Fixture) -> Vec<String> {
    harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, &fixture.card)
        .unwrap()
        .into_iter()
        .filter(|comment| {
            comment.contains("removal-proposal v1") || comment.contains("removal-decision v1")
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Removal authorization through interruption and resume (OpenSpec change task
// 5.7): the actual board decision owner and the real improve
// start/status/stop/resume verbs drive an adoption whose treatment is a
// removal. The latest decision and its actual scope control the next effect;
// a stale approval or retained receipt never authorizes integration or
// activation; refusals and withdrawals are not prompted again without a new
// basis; and interruption preserves the decision, proposal and evidence the
// later stages need.
// ---------------------------------------------------------------------------

/// One candidate checkout ahead of the frozen base: a real owned branch whose
/// committed revision is the exact evaluated candidate.
fn advance_candidate_checkout(fixture: &Fixture) -> harness_core::task_worktree::CandidateCheckout {
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let worktree = fixture.root.join("candidate-worktree");
    let branch = format!("improve/removal-fixture/{}", fixture.card);
    let mut checkout = harness_core::task_worktree::allocate_candidate_checkout(
        &fixture.proj,
        &worktree,
        &branch,
        &base,
    )
    .expect("the candidate allocation is created");
    fs::write(
        checkout.path.join("crates/one/src/lib.rs"),
        "// removal candidate\n",
    )
    .unwrap();
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
    checkout
}

/// The synthetic fixture's board and OpenSpec workspace enter the project
/// after its seed commit; record them once so the accepted mainline is clean
/// for the integration owner, exactly as the comparison fixtures commit their
/// planning workspace.
fn commit_planning_workspace(fixture: &Fixture) {
    git(&fixture.proj, &["add", "."]);
    git(
        &fixture.proj,
        &[
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "user.name=Fixture",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "planning workspace",
        ],
    );
}

/// The comparison run spec whose declared treatment is a removal and whose
/// authority covers integration as well as the isolated experiment.
fn seed_removal_comparison_spec(fixture: &Fixture, name: &str) -> String {
    let head = seed_comparison_spec(fixture, &json!({}), name);
    let mut spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    spec["publication_scope"] = json!(["experiment", "integration"]);
    spec["removal"] = json!({"proposal": "remove-x", "target": "skill-x"});
    fs::write(&fixture.spec, serde_json::to_vec_pretty(&spec).unwrap()).unwrap();
    head
}

/// The retained comparison bindings of a removal adoption: both arms share the
/// frozen pre-solution workload copy and the evaluated candidate is the real
/// branch ahead of the accepted base.
fn seed_removal_bindings(
    fixture: &Fixture,
    workload: &harness_core::task_worktree::FrozenCopy,
    checkout: &harness_core::task_worktree::CandidateCheckout,
    policy_digest: &str,
) -> PathBuf {
    let path = fixture.run.join("comparison/bindings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let arm = |name: &str, arm: &str| {
        json!({
            "arm": arm,
            "home": fixture.root.join(name).display().to_string(),
            "workload": workload,
            "runtime": {
                "arm": arm,
                "label": name,
                "build": fixture.root.join("builds").join(name).display().to_string(),
                "recordSha256": "a".repeat(64),
                "sourceSha256": "b".repeat(64),
            }
        })
    };
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "hypothesis": fixture.card,
            "caseId": "case-b",
            "baseRevision": checkout.base,
            "candidate": {
                "source": fixture.proj,
                "path": checkout.path,
                "branch": checkout.branch,
                "base": checkout.base,
                "revision": checkout.revision,
            },
            "oracle": "oracle-7",
            "acceptance": "acceptance/run-9",
            "policyDigest": policy_digest,
            "arms": [arm("baseline-home", "baseline"), arm("candidate-home", "candidate")],
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

/// The measured evaluation identity the removal adoption is bound to, exactly
/// as the comparison owner leaves it for the decision boundary.
fn adoption_evaluation(policy_digest: &str) -> Value {
    json!({
        "schema": 1,
        "policyDigest": policy_digest,
        "decision": "adopt",
        "basis": "efficiency",
        "quality": "improved",
        "matched": 1,
        "baselineSeconds": 2.0,
        "candidateSeconds": 1.0,
        "tolerancePercent": 5.0,
        "coverage": "fixture",
        "scope": "fixture",
        "reasons": ["the candidate reduced the declared metric"],
        "perSuccess": {
            "status": "complete",
            "seconds": 1.0,
            "acceptedTasks": 1,
            "tasks": 1,
            "reason": "measured",
        },
        "attempts": 2,
        "tasks": 1,
        "acceptedTasks": 1,
        "acceptanceRate": 1.0,
        "tradeOffUsed": false,
    })
}

/// Seed the removal adoption's decision boundary: the retained evaluation, the
/// cursor the decision boundary consumes, the published evidence-bound
/// adoption the activation owner re-derives from exactly those values, and a
/// readable candidate runtime receipt that does not belong to the bound
/// prepared variant (so an authorized activation attempt stops as a reportable
/// state condition instead of touching an installed runtime).
fn seed_removal_adoption_boundary(
    fixture: &Fixture,
    bindings: &Path,
    workload_card: &str,
    checkout: &harness_core::task_worktree::CandidateCheckout,
    policy_digest: &str,
) -> PathBuf {
    let evaluation_path = seed_decision_boundary(
        fixture,
        bindings,
        workload_card,
        &checkout.revision,
        "adopt",
    );
    let evaluation = adoption_evaluation(policy_digest);
    fs::write(
        &evaluation_path,
        serde_json::to_vec_pretty(&evaluation).unwrap(),
    )
    .unwrap();
    let parsed: harness_core::improvement_policy::PolicyEvaluation =
        serde_json::from_value(evaluation).unwrap();
    let draft = parsed
        .decision_draft(
            &fixture.card,
            "exp-fixture",
            &checkout.base,
            &checkout.revision,
            "acceptance/run-9",
        )
        .unwrap();
    harness_core::benefit_gate::publish_decision(&fixture.bd, &fixture.proj, &draft)
        .expect("the evidence-bound adoption is published");

    let runtime = fixture.run.join("candidate-runtime.json");
    let link = |name: &str| {
        json!({
            "name": name,
            "destination": fixture.home.join("harness").join(name).display().to_string(),
            "source": checkout.path.join(name).display().to_string(),
            "sha256": "e".repeat(64),
        })
    };
    fs::write(
        &runtime,
        serde_json::to_vec_pretty(&json!({
            "schema": 2,
            "arm": "candidate",
            "label": "candidate-home",
            "variant": {
                "arm": "candidate",
                "label": "candidate-home",
                "build": fixture.root.join("builds/candidate-home").display().to_string(),
                "recordSha256": "d".repeat(64),
                "sourceSha256": "b".repeat(64),
            },
            "source": checkout.path,
            "home": fixture.home,
            "userHome": fixture.home,
            "dependencyUserHome": fixture.home,
            "upstream": fixture.root.join("upstream.exe"),
            "upstreamSha256": "f".repeat(64),
            "launcher": link("bin/codex.exe"),
            "launchRegistration": fixture.home.join("harness/native-launch.json").display().to_string(),
            "launchSha256": "e".repeat(64),
            "instructions": link("AGENTS.md"),
            "agents": link("agents"),
            "skills": [],
            "commands": [],
            "private": [],
            "installation": {
                "status": "healthy",
                "links": 0,
                "changedLinks": 0,
                "pathChange": false,
                "runtimeExecutableSha256": "e".repeat(64),
                "runtimeEvidence": fixture.root.join("runtime-evidence").display().to_string(),
            },
            "modelCalls": 0,
        }))
        .unwrap(),
    )
    .unwrap();

    let mut cursor = fixture.cursor();
    cursor["candidate"]["removal_required"] = json!(true);
    cursor["comparison"]["policy_digest"] = json!(policy_digest);
    cursor["comparison"]["candidate"]["runtime"] = json!(runtime.display().to_string());
    fixture.write_cursor(&cursor);
    runtime
}

/// Spawn one continuous controller invocation in the background; the caller
/// observes status read-only and interrupts it through `improve stop`.
fn spawn_controller(fixture: &Fixture) -> Child {
    Command::new(manager())
        .arg("improve")
        .args(["resume", "--run", fixture.run.to_str().unwrap()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the controller starts")
}

/// Wait until the run's recorded condition names the expected state.
fn wait_for_condition(fixture: &Fixture, needle: &str, limit: Duration) -> Value {
    let deadline = Instant::now() + limit;
    loop {
        let report = status_value(fixture);
        if report["condition"]
            .as_str()
            .unwrap_or_default()
            .contains(needle)
        {
            return report;
        }
        if Instant::now() >= deadline {
            panic!("the run never reported {needle:?}: {report}");
        }
        thread::sleep(Duration::from_millis(150));
    }
}

/// Collect one background controller's output after it leaves.
fn controller_result(mut child: Child) -> String {
    let finished = wait_for(
        || matches!(child.try_wait(), Ok(Some(_))),
        Duration::from_secs(60),
    );
    if !finished {
        panic!("the controller did not leave: {}", controller_output(child));
    }
    let output = child.wait_with_output().unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_removal_adoption_takes_effect_only_under_the_current_scoped_decision() {
    let fixture = Fixture::new("removal-integration");
    commit_planning_workspace(&fixture);
    // The reviewed removal proposal is recorded before the run starts, so the
    // run freezes exactly the reviewed version the user decided on.
    let propose = fixture.feedback(&[
        "removal-propose",
        "--item",
        &fixture.card,
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--evidence",
        "evidence-7",
        "--loss",
        "retired-skill",
        "--preview",
        "preview-1",
        "--detail",
        "consumer list: none known",
    ]);
    assert!(propose.status.success(), "{}", text(&propose));

    let policy_digest = "c".repeat(64);
    let head = seed_removal_comparison_spec(&fixture, "removal-integration");
    let workload_card = admit_workload_card(&fixture);
    let checkout = advance_candidate_checkout(&fixture);
    assert_eq!(
        checkout.base, head,
        "the candidate branch is bound to the accepted base"
    );
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let workload = harness_core::task_worktree::frozen_copy(
        &fixture.proj,
        &head,
        &fixture.root.join("workload-pre"),
    )
    .unwrap();
    let bindings = seed_removal_bindings(&fixture, &workload, &checkout, &policy_digest);
    let runtime = seed_removal_adoption_boundary(
        &fixture,
        &bindings,
        &workload_card,
        &checkout,
        &policy_digest,
    );
    fs::write(
        fixture.run.join("integration-check.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "program": manager(),
            "args": ["--version"],
            "timeout_seconds": 120,
        }))
        .unwrap(),
    )
    .unwrap();

    // The evidence-bound adoption is complete and benefit-supported, but no
    // removal decision exists: the continuous controller waits for the user's
    // decision and nothing is integrated or activated.
    let controller = spawn_controller(&fixture);
    let report = wait_for_condition(
        &fixture,
        "waiting for removal authority",
        Duration::from_secs(60),
    );
    assert_eq!(report["phase"], "decision-recorded", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("no removal decision"),
        "{report}"
    );
    assert!(!fixture.run.join("integration.json").is_file());
    assert!(!fixture.run.join("activation.json").is_file());
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        head,
        "a pending removal decision leaves the mainline exactly as evaluated"
    );

    // Experiment-only consent covers the isolated experiment; the later
    // integration stage is not covered and the controller keeps waiting
    // instead of applying the treatment.
    let experiment_only = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "approve",
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--actions",
        "experiment",
        "--loss",
        "retired-skill",
        "--basis",
        "user-turn-7",
    ]);
    assert!(
        experiment_only.status.success(),
        "{}",
        text(&experiment_only)
    );
    let report = wait_for_condition(
        &fixture,
        "integration is not covered",
        Duration::from_secs(60),
    );
    assert!(
        report["removal"]["gate"]
            .as_str()
            .unwrap_or_default()
            .starts_with("authorized"),
        "the experiment stage is covered: {report}"
    );
    assert!(!fixture.run.join("integration.json").is_file());
    assert_eq!(git_output(&fixture.proj, &["rev-parse", "HEAD"]), head);

    // A fresh decision that expressly covers integration is the new basis:
    // the latest decision controls the next effect through the real
    // integration owner.
    let covered = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "approve",
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--actions",
        "experiment,integration",
        "--loss",
        "retired-skill",
        "--basis",
        "user-turn-8",
    ]);
    assert!(covered.status.success(), "{}", text(&covered));
    let output = controller_result(controller);
    assert!(
        output.contains("controller: integration owner applied"),
        "{output}"
    );
    assert!(
        output.contains("controller: activation blocked"),
        "{output}"
    );
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "decision-recorded", "{report}");
    let condition = report["condition"].as_str().unwrap_or_default();
    assert!(
        condition.contains("decision boundary blocked")
            && condition.contains("activation")
            && condition.contains("prepared candidate variant"),
        "the authorized activation attempt stops on its own state, not on removal authority: {report}"
    );
    assert!(fixture.run.join("integration.json").is_file());
    assert!(
        !fixture.run.join("activation.json").is_file(),
        "a runtime that is not the prepared variant is never activated"
    );
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        checkout.revision,
        "the checked integrated revision is the mainline"
    );

    // Interruption preserves every input the later stages need: the reviewed
    // proposal and decisions, the retained evaluation and bindings, the
    // combined-tree evidence and the prepared integration receipt.
    let evidence_files = [
        fixture.run.join("comparison/evaluation.json"),
        bindings.clone(),
        fixture.run.join("integration.json"),
        runtime.clone(),
    ];
    for path in &evidence_files {
        assert!(path.is_file(), "{} is retained", path.display());
    }
    let decisions_before = removal_comments(&fixture)
        .iter()
        .filter(|record| record.contains("removal-decision v1"))
        .count();

    // A withdrawal after the integration effect blocks the next activation
    // attempt: the retained integration receipt does not carry the withdrawn
    // consent, and a repeated resume neither re-prompts nor restores it.
    let withdraw = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "withdraw",
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--basis",
        "user-turn-9",
    ]);
    assert!(withdraw.status.success(), "{}", text(&withdraw));
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    assert!(
        report["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("withdrew approval for removal proposal"),
        "the withdrawal blocks the next effect: {report}"
    );
    assert!(!fixture.run.join("activation.json").is_file());
    let repeated = fixture.resume();
    assert!(repeated.status.success(), "{}", text(&repeated));
    let report = status_value(&fixture);
    assert!(
        report["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("withdrew approval"),
        "the withdrawn consent is re-resolved, never restored: {report}"
    );

    // A refusal is the user's current word too: activation stays blocked
    // without a new basis.
    let refuse = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "refuse",
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--basis",
        "user-turn-10",
    ]);
    assert!(refuse.status.success(), "{}", text(&refuse));
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    assert!(
        report["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("refused removal proposal"),
        "the refusal blocks the next effect: {report}"
    );
    assert!(!fixture.run.join("activation.json").is_file());

    // A fresh informed approval restores the covered scope and the next
    // attempt follows the latest decision.
    let reapprove = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "approve",
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--actions",
        "experiment,integration",
        "--loss",
        "retired-skill",
        "--basis",
        "user-turn-11",
    ]);
    assert!(reapprove.status.success(), "{}", text(&reapprove));
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    let condition = report["condition"].as_str().unwrap_or_default();
    assert!(
        condition.contains("prepared candidate variant")
            && !condition.contains("withdrew")
            && !condition.contains("refused"),
        "the latest decision controls the next effect: {report}"
    );
    let integration_bytes = fs::read(fixture.run.join("integration.json")).unwrap();

    // A newly discovered consumer loss is a changed reviewed proposal: the
    // stale approval and the retained integration receipt do not authorize
    // the next activation, and the controller waits for an updated informed
    // decision instead of applying anything.
    let changed = fixture.feedback(&[
        "removal-propose",
        "--item",
        &fixture.card,
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--evidence",
        "evidence-8",
        "--loss",
        "retired-skill+recovery",
        "--preview",
        "preview-1",
        "--detail",
        "consumer list: one indirect consumer recorded after the approval",
    ]);
    assert!(changed.status.success(), "{}", text(&changed));
    let controller = spawn_controller(&fixture);
    let report = wait_for_condition(
        &fixture,
        "changed after the frozen approval",
        Duration::from_secs(60),
    );
    assert!(
        report["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("fresh decision is required"),
        "{report}"
    );
    assert!(
        !fixture.run.join("activation.json").is_file(),
        "a retained receipt or stale approval never authorizes activation"
    );

    // Interrupt the waiting controller through the real stop verb; every
    // decision, proposal and evidence input survives.
    let stopped = fixture.improve(&["stop", "--run", fixture.run.to_str().unwrap()]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    let output = controller_result(controller);
    assert!(
        output.contains("integration is waiting for removal authority")
            || output.contains("consumed the exact stop request"),
        "{output}"
    );
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "stopped", "{report}");
    for path in &evidence_files {
        assert!(
            path.is_file(),
            "{} survives the interruption",
            path.display()
        );
    }
    assert_eq!(
        fs::read(fixture.run.join("integration.json")).unwrap(),
        integration_bytes,
        "the retained integration receipt is not rewritten while a fresh decision is required"
    );
    assert_eq!(
        removal_comments(&fixture)
            .iter()
            .filter(|record| record.contains("removal-proposal v1"))
            .count(),
        2,
        "both reviewed proposal versions are retained"
    );
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        checkout.revision
    );

    // Resuming while the changed proposal has no decision keeps waiting: the
    // interruption did not turn the stale approval into consent.
    let controller = spawn_controller(&fixture);
    let report = wait_for_condition(
        &fixture,
        "changed after the frozen approval",
        Duration::from_secs(60),
    );
    assert_eq!(report["phase"], "decision-recorded", "{report}");
    let stopped = fixture.improve(&["stop", "--run", fixture.run.to_str().unwrap()]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    let _ = controller_result(controller);
    assert!(!fixture.run.join("activation.json").is_file());

    // The updated informed decision on the changed content is the only thing
    // that lets the next activation attempt proceed.
    let covered_change = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "approve",
        "--proposal",
        "remove-x",
        "--target",
        "skill-x",
        "--actions",
        "experiment,integration",
        "--loss",
        "retired-skill+recovery",
        "--basis",
        "user-turn-12",
    ]);
    assert!(covered_change.status.success(), "{}", text(&covered_change));
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    let condition = report["condition"].as_str().unwrap_or_default();
    assert!(
        condition.contains("prepared candidate variant")
            && !condition.contains("fresh decision is required"),
        "the updated decision on exactly the changed content controls the next effect: {report}"
    );
    assert!(!fixture.run.join("activation.json").is_file());
    assert_eq!(
        removal_comments(&fixture)
            .iter()
            .filter(|record| record.contains("removal-decision v1"))
            .count(),
        decisions_before + 4,
        "no resume recorded a decision on the user's behalf"
    );
}

#[test]
fn stop_and_resume_recover_boundaries_and_never_replay_unknown_attempts() {
    let fixture = Fixture::new("recovery");
    fixture.write_spec(&[], None);
    // This case recovers explicit phase boundaries step by step: it is an
    // explicit single-step caller, so continuous supervision is not declared.
    let out = fixture.improve(&[
        "start",
        "--run",
        fixture.run.to_str().unwrap(),
        "--spec",
        fixture.spec.to_str().unwrap(),
        "--supervision",
        "once",
    ]);
    assert!(out.status.success(), "{}", text(&out));
    let run_arg = fixture.run.to_str().unwrap().to_owned();
    let attempts_before = fixture.cursor()["attempts"].as_array().unwrap().len();

    let stop = fixture.improve(&["stop", "--run", &run_arg, "--reason", "owner pause"]);
    assert!(stop.status.success(), "{}", text(&stop));
    let status = fixture.improve(&["status", "--run", &run_arg, "--json"]);
    let report: Value = serde_json::from_str(&text(&status)).unwrap();
    assert_eq!(report["phase"], "stopped");

    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(resume.status.success(), "{}", text(&resume));
    let report: Value = serde_json::from_str(&text(
        &fixture.improve(&["status", "--run", &run_arg, "--json"]),
    ))
    .unwrap();
    assert_eq!(report["phase"], "planning", "{report}");
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts_before,
        "resume never appends a replay"
    );

    // An in-flight attempt whose host is gone is unknown: it is preserved,
    // never resubmitted, and it suspends dependent dispatch.
    let receipt = fixture.run.join("unknown-receipt.json");
    seed_receipt(&receipt, "dispatch-accepted", None);
    seed_attempt(
        &fixture,
        attempt_json("implementer-1", "implementer", "started", Some(&receipt)),
        "candidate-attempt",
    );
    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(resume.status.success(), "{}", text(&resume));
    let resume_text = text(&resume);
    assert!(resume_text.contains("unknown outcomes"), "{resume_text}");
    assert!(resume_text.contains("never resubmit"), "{resume_text}");
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"][0]["state"], "unknown");
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 1);
    let report: Value = serde_json::from_str(&text(
        &fixture.improve(&["status", "--run", &run_arg, "--json"]),
    ))
    .unwrap();
    assert_eq!(report["dispatch"]["state"], "blocked", "{report}");
    assert!(
        report["dispatch"]["reason"]
            .as_str()
            .unwrap()
            .contains("unknown outcome"),
        "{report}"
    );
    let resume_again = fixture.improve(&["resume", "--run", &run_arg]);
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        1,
        "an unknown attempt is never duplicated or replayed"
    );
    assert!(
        text(&resume_again).contains("unknown outcomes"),
        "{}",
        text(&resume_again)
    );

    // A completed arm whose receipt is terminal is settled and reused while
    // the planning inputs still validate.
    let completed = fixture.run.join("completed-receipt.json");
    seed_bound_receipt(
        &completed,
        "loop-fixture-baseline-1",
        "gen-1",
        "completed",
        Some(0),
        None,
        None,
    );
    let mut cursor = fixture.cursor();
    cursor["attempts"]
        .as_array_mut()
        .unwrap()
        .push(attempt_json(
            "baseline-1",
            "baseline",
            "started",
            Some(&completed),
        ));
    fixture.write_cursor(&cursor);
    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(
        text(&resume).contains("settled attempt(s) baseline-1"),
        "{}",
        text(&resume)
    );
    let cursor = fixture.cursor();
    let baseline = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == "baseline-1")
        .unwrap();
    assert_eq!(baseline["state"], "completed");
    assert_eq!(baseline["reuse_refused"], Value::Null);

    // Changed planning inputs invalidate the completed arm: resume names the
    // remeasurement instead of reusing it.
    fs::write(
        fixture
            .proj
            .join("openspec/changes/add-synthetic/specs/synthetic/spec.md"),
        "## ADDED Requirements\n\n### Requirement: Synthetic behavior\n\nThe system SHALL do the synthetic thing differently.\n\n#### Scenario: Synthetic case\n\n- **WHEN** the probe runs\n- **THEN** it reports success\n",
    )
    .unwrap();
    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(text(&resume).contains("remeasurement"), "{}", text(&resume));
    let baseline = fixture.cursor()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == "baseline-1")
        .unwrap()
        .clone();
    assert!(
        baseline["reuse_refused"]
            .as_str()
            .unwrap()
            .contains("planning"),
        "{baseline}"
    );
}

#[test]
fn select_consumes_prepared_variants_and_refuses_an_active_attempt() {
    let fixture = Fixture::new("select");
    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let run_arg = fixture.run.to_str().unwrap().to_owned();

    let (baseline_state, baseline_build) = prepare_runtime(&fixture.root, "base");
    let (candidate_state, candidate_build) = prepare_runtime(&fixture.root, "cand");
    fs::write(
        fixture.run.join("variants.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "baseline": {"state": baseline_state, "build": baseline_build, "identity": Value::Null},
            "candidate": {"state": candidate_state, "build": candidate_build, "identity": Value::Null},
        }))
        .unwrap(),
    )
    .unwrap();

    // An active measured attempt keeps its frozen runtime.
    seed_attempt(
        &fixture,
        attempt_json("baseline-1", "baseline", "started", None),
        "baseline-attempt",
    );
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "baseline"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(text(&select).contains("active"), "{}", text(&select));

    // Once settled, selection consumes the prepared runtime and is idempotent:
    // a repeated selection changes nothing and performs no model call or build.
    let mut cursor = fixture.cursor();
    cursor["attempts"][0]["state"] = json!("completed");
    fixture.write_cursor(&cursor);
    let record_before = fs::read(baseline_build.join("build.json")).unwrap();
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "baseline"]);
    assert!(select.status.success(), "{}", text(&select));
    let select_text = text(&select);
    assert!(select_text.contains("identity=sha256:"), "{select_text}");
    assert!(select_text.contains("no model call"), "{select_text}");
    let cursor = fixture.cursor();
    assert_eq!(cursor["selected_variant"], "baseline");
    assert!(
        cursor["selected_runtime"]
            .as_str()
            .unwrap()
            .contains("base-build")
    );
    assert_eq!(
        fs::read(baseline_build.join("build.json")).unwrap(),
        record_before,
        "selection does not rebuild or rewrite the prepared runtime"
    );

    let again = fixture.improve(&["select", "--run", &run_arg, "--variant", "baseline"]);
    assert!(text(&again).contains("changed=false"), "{}", text(&again));

    // A stale prepared runtime is an explicit error, never fabricated.
    fs::remove_file(candidate_build.join("build.json")).unwrap();
    let select = fixture.improve(&["select", "--run", &run_arg, "--variant", "candidate"]);
    assert_eq!(select.status.code(), Some(2), "{}", text(&select));
    assert!(
        text(&select).contains("missing or stale"),
        "{}",
        text(&select)
    );
}

/// The declared source identity the real preparation owner records for one
/// prepared variant: `sha256:` plus the first 16 hex digits of the build
/// record's source digest.
fn declared_identity(build: &Path) -> String {
    let record: Value =
        serde_json::from_slice(&fs::read(build.join("build.json")).unwrap()).unwrap();
    let sha = record["source"]["sha256"].as_str().unwrap().to_owned();
    format!("sha256:{}", &sha[..16.min(sha.len())])
}

/// Ordinary (non-verbatim) canonical spelling for path comparison.
fn plain_path(path: &Path) -> PathBuf {
    let canonical = fs::canonicalize(path).unwrap();
    let text = canonical.to_string_lossy().into_owned();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned())
}

/// Every file under one prepared runtime build with its content digest.
fn build_files(dir: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, files);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                files.insert(relative, hash_bytes(&fs::read(&path).unwrap()));
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(dir, dir, &mut files);
    files
}

/// The full off/on cycle is driven through the CLI: candidate, baseline,
/// candidate and one repeated selection. Every step reports the identity it
/// actually consumed, flips the journaled selection to that exact prepared
/// build, and leaves both prepared runtimes and the accepted source byte-for-
/// byte unchanged with no rebuild and no model call. An active measured
/// attempt then refuses further selection through the same CLI.
#[test]
fn select_cycle_reports_consumed_identity_without_source_change_rebuild_or_model_call() {
    let fixture = Fixture::new("select-cycle");
    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let run_arg = fixture.run.to_str().unwrap().to_owned();

    // Both variants share the one owned state the real preparation owner
    // records, so the cycle flips the same journaled pointer between them.
    let (state, baseline_build) = prepare_runtime(&fixture.root, "base");
    let (candidate_state, staged_candidate) = prepare_runtime(&fixture.root, "cand");
    let candidate_build = state
        .join("builds")
        .join(staged_candidate.file_name().unwrap());
    fs::rename(&staged_candidate, &candidate_build).unwrap();
    fs::remove_dir_all(&candidate_state).unwrap();

    let baseline_identity = declared_identity(&baseline_build);
    let candidate_identity = declared_identity(&candidate_build);
    fs::write(
        fixture.run.join("variants.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "baseline": {"state": state, "build": baseline_build, "identity": baseline_identity},
            "candidate": {"state": state, "build": candidate_build, "identity": candidate_identity},
        }))
        .unwrap(),
    )
    .unwrap();

    let before_files = (build_files(&baseline_build), build_files(&candidate_build));
    let head_before = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let status_before = git_output(&fixture.proj, &["status", "--porcelain"]);
    let attempts_before = fixture.cursor()["attempts"].as_array().unwrap().len();
    let effects_before = fixture.cursor()["effects"].as_array().unwrap().len();

    let artifacts_before = [
        "build-job.json",
        "build-output.json",
        "prepared-builds.json",
    ]
    .map(|artifact| fixture.run.join(artifact).is_file());
    let select_variant = |variant: &str, expected: &str, changed: bool| {
        let selected = fixture.improve(&["select", "--run", &run_arg, "--variant", variant]);
        assert!(selected.status.success(), "{}", text(&selected));
        let report = text(&selected);
        assert!(report.contains(&format!("variant={variant}")), "{report}");
        assert!(
            report.contains(&format!("identity={expected}")),
            "the reported identity must be the prepared variant's actual identity: {report}"
        );
        assert!(report.contains(&format!("changed={changed}")), "{report}");
        assert!(report.contains("no model call"), "{report}");
        let runtime = report
            .split("runtime=")
            .nth(1)
            .and_then(|rest| rest.split(" identity=").next())
            .unwrap();
        assert_eq!(
            plain_path(Path::new(runtime)),
            plain_path(if variant == "baseline" {
                &baseline_build
            } else {
                &candidate_build
            }),
            "the reported runtime is the build actually selected"
        );
    };

    // candidate -> baseline -> candidate: the journaled selection follows the
    // requested variant and resolves to the exact prepared build every time.
    select_variant("candidate", &candidate_identity, true);
    let (active, _) = harness_core::build_selection::selected(&state).unwrap();
    assert_eq!(plain_path(&active), plain_path(&candidate_build));
    assert_eq!(fixture.cursor()["selected_identity"], candidate_identity);

    select_variant("baseline", &baseline_identity, true);
    let (active, _) = harness_core::build_selection::selected(&state).unwrap();
    assert_eq!(plain_path(&active), plain_path(&baseline_build));
    assert_eq!(fixture.cursor()["selected_variant"], "baseline");
    assert_eq!(fixture.cursor()["selected_identity"], baseline_identity);

    select_variant("candidate", &candidate_identity, true);
    let (active, _) = harness_core::build_selection::selected(&state).unwrap();
    assert_eq!(plain_path(&active), plain_path(&candidate_build));
    let cursor = fixture.cursor();
    assert_eq!(cursor["selected_variant"], "candidate");
    assert_eq!(cursor["selected_identity"], candidate_identity);
    assert_eq!(
        plain_path(Path::new(cursor["selected_runtime"].as_str().unwrap())),
        plain_path(&candidate_build)
    );

    // A repeated selection of the unchanged active variant reports the same
    // consumed identity without changing anything.
    select_variant("candidate", &candidate_identity, false);

    // No source change and no rebuild: both prepared runtimes and the accepted
    // source are exactly what preparation published, and no build or model
    // work was started by selection.
    assert_eq!(
        build_files(&baseline_build),
        before_files.0,
        "selection never rebuilds or rewrites the baseline runtime"
    );
    assert_eq!(
        build_files(&candidate_build),
        before_files.1,
        "selection never rebuilds or rewrites the candidate runtime"
    );
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        head_before
    );
    assert_eq!(
        git_output(&fixture.proj, &["status", "--porcelain"]),
        status_before,
        "selection does not change the accepted source tree"
    );
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["attempts"].as_array().unwrap().len(),
        attempts_before,
        "selection performs no model call or dispatch"
    );
    let selections = cursor["effects"]
        .as_array()
        .unwrap()
        .iter()
        .skip(effects_before)
        .filter(|effect| effect["kind"] == "variant-selected")
        .count();
    assert_eq!(
        selections, 3,
        "only the three real off/on changes are journaled: {cursor}"
    );
    for (artifact, existed) in [
        "build-job.json",
        "build-output.json",
        "prepared-builds.json",
    ]
    .into_iter()
    .zip(artifacts_before)
    {
        assert_eq!(
            fixture.run.join(artifact).is_file(),
            existed,
            "selection must not start a build: {artifact}"
        );
    }

    // An active measured attempt keeps its frozen runtime: the CLI refuses the
    // change and leaves the selected variant, runtimes and source untouched.
    seed_attempt(
        &fixture,
        attempt_json("baseline-1", "baseline", "started", None),
        "baseline-attempt",
    );
    let refused = fixture.improve(&["select", "--run", &run_arg, "--variant", "baseline"]);
    assert_eq!(refused.status.code(), Some(2), "{}", text(&refused));
    assert!(text(&refused).contains("active"), "{}", text(&refused));
    assert_eq!(fixture.cursor()["selected_variant"], "candidate");
    assert_eq!(fixture.cursor()["selected_identity"], candidate_identity);
    let (active, _) = harness_core::build_selection::selected(&state).unwrap();
    assert_eq!(plain_path(&active), plain_path(&candidate_build));
    assert_eq!(build_files(&baseline_build), before_files.0);
    assert_eq!(build_files(&candidate_build), before_files.1);
}

/// The declared directed-measurement scope: the hypothesis's own targeted
/// measurement, bound to a section of its own OpenSpec change.
fn measurement_scope_json() -> Value {
    json!({
        "observed_problem": "identical repeated reads waste accepted-task time",
        "investigation_scope": "the reader's repeated reads at one frozen source revision",
        "measurement_question": "how much accepted-task time do identical repeated reads cost?",
        "workload": {
            "operation": "cargo build -p example-reader",
            "contract": "openspec/changes/add-synthetic/proposal.md#Measurement",
        },
        "evidence_references": ["retained outcome record: repeated reads"],
        "limits": "one local machine and one frozen source revision",
        "declaration_artifact": "proposal.md",
        "declaration_heading": "## Measurement",
    })
}

/// A recorded block is cleared the way operator-driven recovery leaves it
/// before an explicit resume re-evaluates the gate.
fn clear_recorded_block(fixture: &Fixture) {
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("candidate-ready");
    cursor["condition"] = Value::Null;
    fixture.write_cursor(&cursor);
}

/// `improve status` (text and `--json`) surfaces the directed-measurement
/// gate state from recorded run state only: the declared measurement-scope
/// file (present/missing), the retained receipt path and the bounded blocking
/// reason while the gate holds the run.
#[test]
fn status_surfaces_the_directed_measurement_gate_state() {
    let fixture = Fixture::new("measurement-status");
    let state = fixture.root.join("state");
    fs::create_dir_all(state.join("builds/h-build")).unwrap();
    fs::create_dir_all(state.join("builds/ha-build")).unwrap();
    fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
    let upstream = fixture.root.join("upstream.exe");
    fs::write(&upstream, b"fixture-client").unwrap();
    let policy = fixture.root.join("policy.json");
    fs::write(&policy, b"{}\n").unwrap();
    let request = fixture.root.join("request.json");
    fs::write(&request, b"{\"schema\":1}\n").unwrap();
    let qualification = fixture.root.join("qualification.json");
    fs::write(&qualification, b"{}\n").unwrap();
    let request_sha = hash_bytes(&fs::read(&request).unwrap());
    let head = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fixture.write_spec(
        &[
            (
                "runner",
                json!({"profile":"ds","model":Value::Null,"model_provider":Value::Null,"reasoning_effort":Value::Null}),
            ),
            (
                "local_runner",
                json!({"endpoint":"http://127.0.0.1:9/v1","model":"fixture-glyph-1","identity":{}}),
            ),
            ("qualification", json!(qualification)),
            (
                "comparison",
                comparison_inputs(
                    &fixture,
                    &state,
                    &upstream,
                    &policy,
                    &request,
                    &request_sha,
                    &head,
                ),
            ),
        ],
        None,
    );
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("candidate-ready");
    cursor["condition"] = Value::Null;
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": "add-synthetic",
        "revision": head,
        "worktree": {
            "source": fixture.proj,
            "path": fixture.proj,
            "branch": "improve/fixture",
            "base": head,
            "revision": head,
        }
    });
    fixture.write_cursor(&cursor);

    // No declared scope: the gate holds the run, and status reports the exact
    // artifact, the absent receipt and the bounded reason.
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "blocked", "{report}");
    assert_eq!(
        report["measurement"]["scope"]["status"], "missing",
        "{report}"
    );
    assert!(
        report["measurement"]["scope"]["path"]
            .as_str()
            .unwrap()
            .ends_with("measurement-scope.json"),
        "{report}"
    );
    assert!(report["measurement"]["receipt"].is_null(), "{report}");
    assert_eq!(
        report["candidate"]["measurement_receipt"],
        Value::Null,
        "{report}"
    );
    let reason = report["measurement"]["blocked_reason"].as_str().unwrap();
    assert!(
        reason.contains("no hypothesis measurement scope is declared"),
        "{report}"
    );
    let printed = text(&fixture.improve(&["status", "--run", fixture.run.to_str().unwrap()]));
    assert!(printed.contains("measurement: declared scope"), "{printed}");
    assert!(printed.contains("missing"), "{printed}");
    assert!(printed.contains("measurement blocked:"), "{printed}");

    // A declared scope whose own change does not state the section keeps the
    // receipt unretained and reports the gate's exact reason.
    fs::write(
        fixture.run.join("measurement-scope.json"),
        serde_json::to_vec_pretty(&measurement_scope_json()).unwrap(),
    )
    .unwrap();
    clear_recorded_block(&fixture);
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    assert_eq!(
        report["measurement"]["scope"]["status"], "present",
        "{report}"
    );
    assert!(report["measurement"]["receipt"].is_null(), "{report}");
    let reason = report["measurement"]["blocked_reason"].as_str().unwrap();
    assert!(
        reason.contains("missing or empty measurement scope section"),
        "{report}"
    );

    // Once the hypothesis's own change states the section, the gate retains
    // the receipt before the measured-pair owner is engaged, and status
    // surfaces that exact retained path.
    let proposal = fixture
        .proj
        .join("openspec/changes/add-synthetic/proposal.md");
    let mut content = fs::read_to_string(&proposal).unwrap();
    content.push_str("\n## Measurement\n\nObserved problem: identical repeated reads waste accepted-task time. Investigation scope: reads at one frozen source revision. Measurement question: how much accepted-task time do they cost? Workload: the existing cargo build operation linked from this change. Evidence: the retained outcome record. Limits: one local machine and one frozen source revision.\n");
    fs::write(&proposal, content).unwrap();
    clear_recorded_block(&fixture);
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    assert_eq!(
        report["measurement"]["scope"]["status"], "present",
        "{report}"
    );
    let receipt = report["measurement"]["receipt"].as_str().unwrap();
    assert!(receipt.ends_with("measurement-receipt.json"), "{report}");
    assert!(Path::new(receipt).is_file(), "{report}");
    assert_eq!(
        report["candidate"]["measurement_receipt"], receipt,
        "{report}"
    );
}

#[test]
fn concurrent_starts_create_exactly_one_owner() {
    let fixture = Fixture::new("concurrent-start");
    fixture.write_spec(&[], None);
    let spawn = || {
        Command::new(manager())
            .args([
                "improve",
                "start",
                "--run",
                fixture.run.to_str().unwrap(),
                "--spec",
                fixture.spec.to_str().unwrap(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("improve start spawns")
    };
    let first = spawn();
    let second = spawn();
    let first_id = first.id() as u64;
    let second_id = second.id() as u64;
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    let first_ok = first.status.success();
    let second_ok = second.status.success();
    assert!(
        first_ok ^ second_ok,
        "exactly one concurrent start creates the run: {}\n{}",
        text(&first),
        text(&second)
    );
    let loser = if first_ok { &second } else { &first };
    assert_eq!(loser.status.code(), Some(2), "{}", text(loser));
    assert!(
        text(loser).contains("duplicate run ownership"),
        "{}",
        text(loser)
    );

    // One consistent run exists, owned by the winner, with a single cursor.
    assert_eq!(fixture.cursor()["phase"], "blocked");
    let owner: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("owner.json")).unwrap()).unwrap();
    let winner_pid = if first_ok { first_id } else { second_id };
    assert_eq!(owner["pid"].as_u64().unwrap(), winner_pid);
}

#[test]
fn concurrent_mutations_serialize_without_lost_updates() {
    let fixture = Fixture::new("concurrent-select");
    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let (baseline_state, baseline_build) = prepare_runtime(&fixture.root, "base");
    let (candidate_state, candidate_build) = prepare_runtime(&fixture.root, "cand");
    fs::write(
        fixture.run.join("variants.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "baseline": {"state": baseline_state, "build": baseline_build, "identity": Value::Null},
            "candidate": {"state": candidate_state, "build": candidate_build, "identity": Value::Null},
        }))
        .unwrap(),
    )
    .unwrap();

    let spawn = |variant: &'static str| {
        Command::new(manager())
            .args([
                "improve",
                "select",
                "--run",
                fixture.run.to_str().unwrap(),
                "--variant",
                variant,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("improve select spawns")
    };
    let first = spawn("baseline");
    let second = spawn("candidate");
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    assert!(first.status.success(), "{}", text(&first));
    assert!(second.status.success(), "{}", text(&second));

    // Serialized read-modify-write: both selections are journaled and the
    // persisted selection stays internally consistent with one variant.
    let cursor = fixture.cursor();
    let selected = cursor["selected_variant"].as_str().unwrap().to_owned();
    assert!(matches!(selected.as_str(), "baseline" | "candidate"));
    let runtime = cursor["selected_runtime"].as_str().unwrap();
    let expected = if selected == "baseline" {
        "base-build"
    } else {
        "cand-build"
    };
    assert!(runtime.contains(expected), "{cursor}");
    let selections = cursor["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|effect| effect["kind"] == "variant-selected")
        .count();
    assert_eq!(
        selections, 2,
        "no selection may be lost to a stale cursor overwrite: {cursor}"
    );
}

#[test]
fn resume_settles_retained_unknown_attempts_from_late_receipts_once() {
    let fixture = Fixture::new("late-completion");
    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let run_arg = fixture.run.to_str().unwrap().to_owned();

    // An attempt stops while its outcome is unknown; the owning dispatcher
    // later records an authoritative completion.
    let completed = fixture.run.join("late-completed.json");
    seed_attempt(
        &fixture,
        attempt_json("implementer-1", "implementer", "started", Some(&completed)),
        "candidate-attempt",
    );
    let stop = fixture.improve(&["stop", "--run", &run_arg, "--reason", "pause"]);
    assert!(stop.status.success(), "{}", text(&stop));
    assert!(
        text(&stop).contains("retained as unknown"),
        "an attempt without a locatable owned effect is retained explicitly: {}",
        text(&stop)
    );
    assert_eq!(fixture.cursor()["attempts"][0]["state"], "unknown");
    seed_receipt(&completed, "completed", Some(0));
    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        text(&resume).contains("settled attempt(s) implementer-1"),
        "{}",
        text(&resume)
    );
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"][0]["state"], "completed");
    assert_eq!(cursor["attempts"][0]["reuse_refused"], Value::Null);
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 1);

    // Settled once: another resume neither replays nor re-settles it.
    let again = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(
        !text(&again).contains("settled attempt(s)"),
        "{}",
        text(&again)
    );
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(fixture.cursor()["attempts"][0]["state"], "completed");

    // A failed outcome settles too, while a truly unobserved attempt stays
    // unknown and keeps dependent dispatch blocked.
    let failed = fixture.run.join("late-failed.json");
    seed_attempt(
        &fixture,
        attempt_json("implementer-2", "implementer", "started", Some(&failed)),
        "candidate-attempt",
    );
    let stop = fixture.improve(&["stop", "--run", &run_arg, "--reason", "pause"]);
    assert!(stop.status.success(), "{}", text(&stop));
    seed_bound_receipt(
        &failed,
        "loop-fixture-implementer-2",
        "gen-1",
        "failed",
        Some(1),
        None,
        None,
    );
    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(
        text(&resume).contains("settled attempt(s) implementer-2"),
        "{}",
        text(&resume)
    );
    let failed_attempt = fixture.cursor()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == "implementer-2")
        .unwrap()
        .clone();
    assert_eq!(failed_attempt["state"], "failed");

    seed_attempt(
        &fixture,
        attempt_json(
            "implementer-3",
            "implementer",
            "started",
            Some(&fixture.run.join("absent-receipt.json")),
        ),
        "candidate-attempt",
    );
    let stop = fixture.improve(&["stop", "--run", &run_arg, "--reason", "pause"]);
    assert!(stop.status.success(), "{}", text(&stop));
    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(text(&resume).contains("implementer-3"), "{}", text(&resume));
    let report: Value = serde_json::from_str(&text(
        &fixture.improve(&["status", "--run", &run_arg, "--json"]),
    ))
    .unwrap();
    let dispatch = report["dispatch"]["reason"].as_str().unwrap();
    assert!(dispatch.contains("unknown outcome"), "{report}");
}

#[test]
fn status_distinguishes_a_retained_live_attempt_from_unknown() {
    let fixture = Fixture::new("retained-live");
    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let run_arg = fixture.run.to_str().unwrap().to_owned();

    // The recorded host is this test process: still live and authoritative.
    let (pid, created, program) =
        harness_core::improvement_loop::current_process_identity().unwrap();
    let receipt = fixture.run.join("live-receipt.json");
    fs::write(
        &receipt,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "launcher": program,
            "profile": "ds",
            "mode": "tui",
            "visible": true,
            "host": "windows-terminal-tab",
            "slot": {
                "index": 1,
                "path": "C:\\fixture\\slot-1",
                "source": "C:\\fixture\\source",
                "owner": "loop-fixture-implementer-1",
                "base": "base",
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
                "state": "running",
                "session": Value::Null,
                "previousSession": Value::Null,
                "exitCode": Value::Null,
                "events": 0,
                "messages": 0,
                "toolCalls": 0,
                "malformed": 0,
                "cause": Value::Null,
                "host": {"pid": pid, "created": created, "program": program},
                "result": Value::Null,
                "detail": Value::Null,
                "updatedMs": 1,
            }
        }))
        .unwrap(),
    )
    .unwrap();
    seed_attempt(
        &fixture,
        attempt_json("implementer-1", "implementer", "unknown", Some(&receipt)),
        "candidate-attempt",
    );

    let report: Value = serde_json::from_str(&text(
        &fixture.improve(&["status", "--run", &run_arg, "--json"]),
    ))
    .unwrap();
    assert_eq!(report["attempts"][0]["state"], "unknown");
    assert_eq!(
        report["attempts"][0]["observed"], "active(host live)",
        "{report}"
    );

    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(text(&resume).contains("retained live"), "{}", text(&resume));
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"][0]["state"], "unknown");
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 1);
}

#[test]
fn stop_cleans_up_owned_attempts_and_preserves_foreign_processes() {
    let fixture = Fixture::new("cleanup");
    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let run_arg = fixture.run.to_str().unwrap().to_owned();

    // A real pooled binding for one owned attempt: slot worktree, slot record
    // and a dispatch receipt whose recorded host is a live owned process.
    let slot = fixture.root.join(format!(
        "{}-wt1",
        fixture.proj.file_name().unwrap().to_string_lossy()
    ));
    git(
        &fixture.proj,
        &[
            "worktree",
            "add",
            "--detach",
            slot.to_str().unwrap(),
            "HEAD",
        ],
    );
    let owner = "loop-fixture-investigator-1";
    let state_dir = harness_core::task_worktree::pool_state_dir(&fixture.home, &fixture.proj)
        .expect("pool state directory");
    fs::create_dir_all(&state_dir).unwrap();
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fs::write(
        state_dir.join("slot-1.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "source": fixture.proj,
            "index": 1,
            "path": slot,
            "state": "occupied",
            "owner": owner,
            "base": base,
            "disposition": Value::Null,
            "reason": Value::Null,
        }))
        .unwrap(),
    )
    .unwrap();

    let mut owned = start_sleeper();
    let user = harness_core::process_service::current_user().unwrap();
    let pwsh = powershell();
    let identity =
        harness_core::process_service::ServiceProcess::observe(owned.id(), &pwsh, 0, &user)
            .expect("the owned child is identifiable by its exact identity")
            .identity();
    let mut foreign = start_sleeper();
    let receipt = state_dir.join("spawn-1.json");
    fs::write(
        &receipt,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "launcher": pwsh,
            "profile": "ds",
            "mode": "tui",
            "args": [],
            "visible": true,
            "host": "windows-terminal-tab",
            "slot": {
                "index": 1,
                "path": slot,
                "source": fixture.proj,
                "owner": owner,
                "base": base,
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
                "state": "running",
                "session": "session-0001",
                "previousSession": Value::Null,
                "exitCode": Value::Null,
                "events": 0,
                "messages": 0,
                "toolCalls": 0,
                "malformed": 0,
                "cause": Value::Null,
                "host": {"pid": identity.pid, "created": identity.creation_time, "program": pwsh},
                "result": Value::Null,
                "detail": Value::Null,
                "updatedMs": 1,
            }
        }))
        .unwrap(),
    )
    .unwrap();
    seed_attempt(
        &fixture,
        attempt_json("investigator-1", "investigator", "started", Some(&receipt)),
        "planning",
    );

    let stop = fixture.improve(&[
        "stop",
        "--run",
        &run_arg,
        "--reason",
        "owner cleanup",
        "--timeout",
        "20",
    ]);
    assert!(stop.status.success(), "{}", text(&stop));
    assert!(
        wait_gone(&mut owned),
        "the owned recorded child is terminated: {}",
        text(&stop)
    );
    assert!(
        child_running(&mut foreign),
        "a foreign process the run never recorded stays untouched"
    );
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"][0]["state"], "stopped");
    let receipt_json: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    assert_eq!(receipt_json["stop"]["outcome"], "stopped", "{receipt_json}");
    let _ = foreign.kill();
    let _ = foreign.wait();
}

#[test]
fn resume_refuses_a_receipt_replaced_by_a_foreign_generation() {
    let fixture = Fixture::new("foreign-generation");
    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let run_arg = fixture.run.to_str().unwrap().to_owned();

    // The attempt accepted generation gen-1; the pool file now holds a
    // completed run of a later generation for the same slot and owner.
    let receipt = fixture.run.join("replaced-receipt.json");
    seed_bound_receipt(
        &receipt,
        "loop-fixture-implementer-1",
        "gen-newer",
        "completed",
        Some(0),
        None,
        None,
    );
    seed_attempt(
        &fixture,
        attempt_json("implementer-1", "implementer", "unknown", Some(&receipt)),
        "candidate-attempt",
    );

    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(resume.status.success(), "{}", text(&resume));
    let resume_text = text(&resume);
    assert!(resume_text.contains("generation"), "{resume_text}");
    assert!(
        resume_text.contains("never settled from unverifiable evidence"),
        "{resume_text}"
    );
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"][0]["state"], "unknown");
    assert_eq!(cursor["attempts"][0]["retained"], Value::Null);
    assert_eq!(cursor["attempts"][0]["reuse_refused"], Value::Null);
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 1);

    let report: Value = serde_json::from_str(&text(
        &fixture.improve(&["status", "--run", &run_arg, "--json"]),
    ))
    .unwrap();
    assert_eq!(report["attempts"][0]["state"], "unknown");
    let observed = report["attempts"][0]["observed"].as_str().unwrap();
    assert!(observed.contains("unverified"), "{report}");
    assert!(observed.contains("generation"), "{report}");
    let dispatch = report["dispatch"]["reason"].as_str().unwrap();
    assert!(dispatch.contains("unknown outcome"), "{report}");
}

#[test]
fn stop_refuses_a_replaced_generation_and_preserves_the_newer_child() {
    let fixture = Fixture::new("replaced-generation");
    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let run_arg = fixture.run.to_str().unwrap().to_owned();

    // A newer same-owner dispatch occupies the slot with a live child; the
    // retained attempt still froze the earlier generation.
    let mut newer = start_sleeper();
    let pwsh = powershell();
    let receipt = fixture.run.join("replaced-live.json");
    seed_bound_receipt(
        &receipt,
        "loop-fixture-implementer-1",
        "gen-newer",
        "running",
        None,
        Some("session-0002"),
        Some((&pwsh, newer.id())),
    );
    seed_attempt(
        &fixture,
        attempt_json("implementer-1", "implementer", "unknown", Some(&receipt)),
        "candidate-attempt",
    );

    let stop = fixture.improve(&[
        "stop",
        "--run",
        &run_arg,
        "--reason",
        "pause",
        "--timeout",
        "10",
    ]);
    assert!(stop.status.success(), "{}", text(&stop));
    let stop_text = text(&stop);
    assert!(stop_text.contains("cleanup refused"), "{stop_text}");
    assert!(stop_text.contains("generation"), "{stop_text}");
    assert!(
        child_running(&mut newer),
        "the newer generation's child survives a refused cleanup"
    );
    assert_eq!(fixture.cursor()["attempts"][0]["state"], "unknown");
    let _ = newer.kill();
    let _ = newer.wait();
}

#[test]
fn resume_settles_and_retains_evidence_for_the_unchanged_generation() {
    let fixture = Fixture::new("retained-evidence");
    fixture.write_spec(&[], None);
    let out = fixture.start();
    assert!(out.status.success(), "{}", text(&out));
    let run_arg = fixture.run.to_str().unwrap().to_owned();

    // The accepted generation is live but has no terminal record yet.
    let receipt = fixture.run.join("late-completed.json");
    seed_bound_receipt(
        &receipt,
        "loop-fixture-implementer-1",
        "gen-1",
        "running",
        None,
        None,
        None,
    );
    seed_attempt(
        &fixture,
        attempt_json("implementer-1", "implementer", "started", Some(&receipt)),
        "candidate-attempt",
    );
    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(resume.status.success(), "{}", text(&resume));
    assert_eq!(fixture.cursor()["attempts"][0]["state"], "unknown");
    let stop = fixture.improve(&["stop", "--run", &run_arg, "--reason", "pause"]);
    assert!(stop.status.success(), "{}", text(&stop));

    // The owning dispatcher later records the authoritative completion of the
    // same generation; resume settles it once and retains the evidence.
    seed_bound_receipt(
        &receipt,
        "loop-fixture-implementer-1",
        "gen-1",
        "completed",
        Some(0),
        Some("session-0001"),
        None,
    );
    let original = fs::read(&receipt).unwrap();
    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        text(&resume).contains("settled attempt(s) implementer-1"),
        "{}",
        text(&resume)
    );
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"][0]["state"], "completed");
    assert_eq!(cursor["attempts"][0]["reuse_refused"], Value::Null);
    let retained_path = PathBuf::from(
        cursor["attempts"][0]["retained"]["receipt"]
            .as_str()
            .unwrap(),
    );
    assert!(retained_path.is_file(), "{cursor}");
    assert_eq!(
        cursor["attempts"][0]["retained"]["receipt_sha256"],
        harness_core::build_identity::hash_bytes(&original)
    );

    // The pool file is later overwritten by a foreign run: the settled
    // attempt keeps its retained evidence, is not re-settled and is not
    // rebound to the new file's contents.
    seed_bound_receipt(
        &receipt,
        "loop-fixture-implementer-1",
        "gen-newer",
        "completed",
        Some(0),
        None,
        None,
    );
    let resume = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        !text(&resume).contains("settled attempt(s)"),
        "{}",
        text(&resume)
    );
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"][0]["state"], "completed");
    assert_eq!(
        cursor["attempts"][0]["retained"]["receipt_sha256"],
        harness_core::build_identity::hash_bytes(&original)
    );
    let report: Value = serde_json::from_str(&text(
        &fixture.improve(&["status", "--run", &run_arg, "--json"]),
    ))
    .unwrap();
    assert_eq!(report["attempts"][0]["state"], "completed");
    assert_eq!(
        report["attempts"][0]["observed"],
        "settled(retained evidence)"
    );
}

#[test]
fn continuous_resume_waits_without_a_second_controller_or_replay() {
    let fixture = Fixture::new("continuous-wait");
    fixture.write_spec(&[], None);
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let (pid, created, program) =
        harness_core::improvement_loop::current_process_identity().unwrap();
    let receipt = fixture.run.join("live-receipt.json");
    fs::write(
        &receipt,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "launcher": program,
            "profile": "ds",
            "mode": "tui",
            "visible": true,
            "host": "windows-terminal-tab",
            "slot": {
                "index": 1,
                "path": "C:\\fixture\\slot-1",
                "source": "C:\\fixture\\source",
                "owner": "loop-fixture-implementer-1",
                "base": "base",
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
                "state": "running",
                "session": Value::Null,
                "previousSession": Value::Null,
                "exitCode": Value::Null,
                "events": 0,
                "messages": 0,
                "toolCalls": 0,
                "malformed": 0,
                "cause": Value::Null,
                "host": {"pid": pid, "created": created, "program": program},
                "result": Value::Null,
                "detail": Value::Null,
                "updatedMs": 1,
            }
        }))
        .unwrap(),
    )
    .unwrap();
    seed_attempt(
        &fixture,
        attempt_json("implementer-1", "implementer", "started", Some(&receipt)),
        "candidate-attempt",
    );
    let run_arg = fixture.run.to_str().unwrap().to_owned();
    let mut controller = Command::new(manager())
        .arg("improve")
        .args(["resume", "--run", &run_arg])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("continuous resume starts");
    let waiting = wait_for(
        || {
            status_value(&fixture)
                .get("condition")
                .and_then(|value| value.as_str())
                .is_some_and(|condition| condition.contains("waiting for attempt"))
        },
        Duration::from_secs(15),
    );
    if !waiting {
        panic!(
            "controller did not stay waiting: {}",
            controller_output(controller)
        );
    }
    let during = status_value(&fixture);
    assert_eq!(during["attempts"].as_array().unwrap().len(), 1, "{during}");
    assert_eq!(during["supervision"], "continuous", "{during}");
    let duplicate = fixture.improve(&["resume", "--run", &run_arg]);
    assert_eq!(duplicate.status.code(), Some(2), "{}", text(&duplicate));
    assert!(
        text(&duplicate).contains("second controller is refused"),
        "{}",
        text(&duplicate)
    );
    let stopped = fixture.improve(&["stop", "--run", &run_arg, "--reason", "test stop"]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    let finished = wait_for(
        || matches!(controller.try_wait(), Ok(Some(_))),
        Duration::from_secs(15),
    );
    if !finished {
        panic!(
            "controller did not leave after stop: {}",
            controller_output(controller)
        );
    }
    let _ = controller.wait();
    let again = fixture.improve(&["resume", "--run", &run_arg]);
    assert!(again.status.success(), "{}", text(&again));
    let cursor = fixture.cursor();
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 1, "{cursor}");
    assert_ne!(cursor["phase"], "candidate-ready", "{cursor}");
}

fn wait_for(mut ready: impl FnMut() -> bool, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if ready() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    ready()
}

fn status_value(fixture: &Fixture) -> Value {
    let status = fixture.improve(&["status", "--run", fixture.run.to_str().unwrap(), "--json"]);
    assert!(status.status.success(), "status: {}", text(&status));
    serde_json::from_str(&text(&status)).expect("status --json")
}

fn controller_output(mut child: Child) -> String {
    let _ = child.kill();
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("controller output: {error}"));
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn continuous_rejection_keeps_the_baseline_and_records_lineage() {
    let fixture = Fixture::new("continuous-reject");
    let state = fixture.root.join("state");
    fs::create_dir_all(state.join("builds/h-build")).unwrap();
    fs::create_dir_all(state.join("builds/ha-build")).unwrap();
    let upstream = fixture.root.join("upstream.exe");
    fs::write(&upstream, b"fixture-client").unwrap();
    let policy = fixture.root.join("policy.json");
    fs::write(&policy, b"{}\n").unwrap();
    let request = fixture.root.join("request.json");
    fs::write(&request, b"{\"schema\":1}\n").unwrap();
    let qualification = fixture.root.join("qualification.json");
    fs::write(&qualification, b"{}\n").unwrap();
    let request_sha = hash_bytes(&fs::read(&request).unwrap());
    let head = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fixture.write_spec(
        &[
            (
                "runner",
                json!({"profile":"ds","model":Value::Null,"model_provider":Value::Null,"reasoning_effort":Value::Null}),
            ),
            (
                "local_runner",
                json!({"endpoint":"http://127.0.0.1:9/v1","model":"fixture-glyph-1","identity":{}}),
            ),
            ("qualification", json!(qualification)),
            (
                "comparison",
                comparison_inputs(
                    &fixture,
                    &state,
                    &upstream,
                    &policy,
                    &request,
                    &request_sha,
                    &head,
                ),
            ),
        ],
        None,
    );
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    fs::create_dir_all(fixture.run.join("comparison")).unwrap();
    fs::write(
        fixture.run.join("comparison/evaluation.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "policyDigest": "a".repeat(64),
            "decision": "reject",
            "basis": "efficiency",
            "quality": "regressed",
            "matched": 1,
            "baselineSeconds": 2.0,
            "candidateSeconds": 3.0,
            "tolerancePercent": 5.0,
            "coverage": "fixture",
            "scope": "fixture",
            "reasons": ["the candidate regressed the declared metric"],
            "perSuccess": {
                "status": "undefined",
                "seconds": Value::Null,
                "acceptedTasks": 0,
                "tasks": 1,
                "reason": "not used"
            },
            "attempts": 2,
            "tasks": 1,
            "acceptedTasks": 1,
            "acceptanceRate": 1.0,
            "tradeOffUsed": false
        }))
        .unwrap(),
    )
    .unwrap();
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("decision-recorded");
    cursor["condition"] = Value::Null;
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": "add-synthetic",
        "revision": head,
        "worktree": {
            "source": fixture.proj,
            "path": fixture.proj,
            "branch": "improve/fixture",
            "base": head,
            "revision": head,
        }
    });
    fixture.write_cursor(&cursor);
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "idle", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("leaves the baseline unchanged"),
        "{report}"
    );
    assert!(!fixture.run.join("integration.json").is_file());
    assert!(!fixture.run.join("activation.json").is_file());
    assert!(fixture.run.join("lineage.json").is_file());
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 0);
}

#[test]
fn a_missing_candidate_build_is_prepared_by_the_native_owner_and_keeps_its_error() {
    let fixture = Fixture::new("continuous-build");
    let state = fixture.root.join("state");
    fs::create_dir_all(state.join("builds/h-build")).unwrap();
    let upstream = fixture.root.join("upstream.exe");
    fs::write(&upstream, b"fixture-client").unwrap();
    let policy = fixture.root.join("policy.json");
    fs::write(&policy, b"{}\n").unwrap();
    let request = fixture.root.join("request.json");
    fs::write(&request, b"{\"schema\":1}\n").unwrap();
    let qualification = fixture.root.join("qualification.json");
    fs::write(&qualification, b"{}\n").unwrap();
    let request_sha = hash_bytes(&fs::read(&request).unwrap());
    let head = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fixture.write_spec(
        &[(
            "runner",
            json!({"profile":"ds","model":Value::Null,"model_provider":Value::Null,"reasoning_effort":Value::Null}),
        ), (
            "local_runner",
            json!({"endpoint":"http://127.0.0.1:9/v1","model":"fixture-glyph-1","identity":{}}),
        ), (
            "qualification",
            json!(qualification),
        ), (
            "comparison",
            comparison_inputs(&fixture, &state, &upstream, &policy, &request, &request_sha, &head),
        )],
        None,
    );
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("candidate-ready");
    cursor["condition"] = Value::Null;
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": "add-synthetic",
        "revision": head,
        "worktree": {
            "source": fixture.proj,
            "path": fixture.proj,
            "branch": "improve/fixture",
            "base": head,
            "revision": head,
        }
    });
    fixture.write_cursor(&cursor);
    let resumed = fixture.resume();
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "blocked", "{report}\n{output}");
    let condition = report["condition"].as_str().unwrap_or("");
    assert!(
        condition.contains("native") || condition.contains("build"),
        "{report}\n{output}"
    );
    assert!(
        fixture.run.join("build-child.log").is_file(),
        "the native build child was not started"
    );
    assert!(!fixture.run.join("integration.json").is_file());
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 0);
}

#[test]
fn continuous_activation_without_a_successor_idles_without_a_model_call() {
    let fixture = Fixture::new("continuous-next");
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let mut cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().unwrap().len();
    cursor["phase"] = json!("activation-confirmed");
    cursor["condition"] = Value::Null;
    fixture.write_cursor(&cursor);
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "activation-confirmed", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("no independently specified successor"),
        "{report}"
    );
    assert!(fixture.run.join("continuation.json").is_file());
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts
    );
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    assert!(
        text(&again).contains("already recorded"),
        "{}",
        text(&again)
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts
    );
}

/// The supported transition after adoption: A's experiment was decided and
/// the resulting baseline advanced; the independently specified successor
/// investigates this run's evaluated workload B on its own declared workload
/// C, and the controller starts it automatically with the retained lineage.
#[test]
fn an_adopted_experiment_continues_onto_the_independently_specified_successor() {
    let fixture = Fixture::new("adopt-successor");
    let workload = admit_distinct_hypothesis_card(&fixture, "bounded-input");
    seed_comparison_spec(&fixture, &json!({}), "adopt-successor");
    set_comparison_workload(&fixture.spec, &workload);
    // B's own run: the successor spec independently declares hypothesis B
    // evaluated on C - it is never inherited from this run's experiment.
    let spec_b = fixture.root.join("spec-adopt-successor.json");
    successor_run_spec(
        &fixture,
        &spec_b,
        "loop-fixture-successor",
        &workload,
        "workload-c",
    );
    let run_b = fixture.root.join("runs-successor");
    let started = fixture.improve(&[
        "start",
        "--run",
        fixture.run.to_str().unwrap(),
        "--spec",
        fixture.spec.to_str().unwrap(),
        "--successor-spec",
        spec_b.to_str().unwrap(),
        "--successor-run",
        run_b.to_str().unwrap(),
    ]);
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    // The settled adoption decision. This run's publication scope does not
    // permit integration, so the baseline stays unchanged, the decision
    // lineage is retained and the loop continues automatically.
    fs::create_dir_all(fixture.run.join("comparison")).unwrap();
    fs::write(
        fixture.run.join("comparison/evaluation.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "policyDigest": "a".repeat(64),
            "decision": "adopt",
            "basis": "efficiency",
            "quality": "improved",
            "matched": 1,
            "baselineSeconds": 2.0,
            "candidateSeconds": 1.0,
            "tolerancePercent": 5.0,
            "coverage": "fixture",
            "scope": "fixture",
            "reasons": ["the candidate improved the declared metric"],
            "perSuccess": {
                "status": "complete",
                "seconds": 1.0,
                "acceptedTasks": 1,
                "tasks": 1,
                "reason": "not used"
            },
            "attempts": 2,
            "tasks": 1,
            "acceptedTasks": 1,
            "acceptanceRate": 1.0,
            "tradeOffUsed": false
        }))
        .unwrap(),
    )
    .unwrap();
    let head = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("decision-recorded");
    cursor["condition"] = Value::Null;
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": "add-synthetic",
        "revision": head,
        "worktree": {
            "source": fixture.proj,
            "path": fixture.proj,
            "branch": "improve/fixture",
            "base": head,
            "revision": head,
        }
    });
    fixture.write_cursor(&cursor);

    let resumed = fixture.resume();
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    assert!(
        output.contains("independently specified successor"),
        "{output}"
    );
    let lineage: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("lineage.json")).unwrap()).unwrap();
    assert_eq!(lineage["decision"], "adopt", "{lineage}");
    assert_eq!(lineage["workload_lineage"], json!([]), "{lineage}");
    let marker: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("continuation.json")).unwrap()).unwrap();
    assert_eq!(marker["phase"], "completed", "{marker}");
    assert_eq!(marker["exit_code"], 0, "{marker}");
    assert_eq!(marker["decision"], "adopt", "{marker}");
    assert_eq!(marker["hypothesis"], workload, "{marker}");
    assert_eq!(marker["workload_card"], "workload-c", "{marker}");
    assert!(
        run_b.join("cursor.json").is_file(),
        "the independently specified successor run was started"
    );
    assert!(run_b.join("spec.json").is_file());
}

/// A workload without a retained candidate patch does not fabricate one: the
/// loop continues with another grounded hypothesis (the independently
/// specified successor), starts no model work here and invents no artifact.
#[test]
fn a_workload_without_a_retained_candidate_continues_with_another_grounded_hypothesis() {
    let fixture = Fixture::new("no-candidate-next");
    let workload = admit_distinct_hypothesis_card(&fixture, "bounded-input");
    let other = admit_distinct_hypothesis_card(&fixture, "bounded-quantity");
    assert_ne!(workload, other);
    seed_comparison_spec(&fixture, &json!({}), "no-candidate-next");
    set_comparison_workload(&fixture.spec, &workload);
    let workload_comments =
        harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, &workload).unwrap();
    assert!(
        !workload_comments
            .iter()
            .any(|comment| comment.contains("role=workload")),
        "the workload card retains no candidate solution: {workload_comments:?}"
    );
    let spec_b = fixture.root.join("spec-no-candidate.json");
    successor_run_spec(&fixture, &spec_b, "loop-fixture-next", &other, "workload-c");
    let run_b = fixture.root.join("runs-next");
    let started = fixture.improve(&[
        "start",
        "--run",
        fixture.run.to_str().unwrap(),
        "--spec",
        fixture.spec.to_str().unwrap(),
        "--successor-spec",
        spec_b.to_str().unwrap(),
        "--successor-run",
        run_b.to_str().unwrap(),
    ]);
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    fs::create_dir_all(fixture.run.join("comparison")).unwrap();
    fs::write(
        fixture.run.join("comparison/evaluation.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "policyDigest": "a".repeat(64),
            "decision": "reject",
            "basis": "efficiency",
            "quality": "regressed",
            "matched": 1,
            "baselineSeconds": 2.0,
            "candidateSeconds": 3.0,
            "tolerancePercent": 5.0,
            "coverage": "fixture",
            "scope": "fixture",
            "reasons": ["the candidate regressed the declared metric"],
            "perSuccess": {
                "status": "undefined",
                "seconds": Value::Null,
                "acceptedTasks": 0,
                "tasks": 1,
                "reason": "not used"
            },
            "attempts": 2,
            "tasks": 1,
            "acceptedTasks": 1,
            "acceptanceRate": 1.0,
            "tradeOffUsed": false
        }))
        .unwrap(),
    )
    .unwrap();
    let head = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("decision-recorded");
    cursor["condition"] = Value::Null;
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": "add-synthetic",
        "revision": head,
        "worktree": {
            "source": fixture.proj,
            "path": fixture.proj,
            "branch": "improve/fixture",
            "base": head,
            "revision": head,
        }
    });
    fixture.write_cursor(&cursor);

    let resumed = fixture.resume();
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    let marker: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("continuation.json")).unwrap()).unwrap();
    assert_eq!(marker["phase"], "completed", "{marker}");
    assert_eq!(marker["hypothesis"], other, "{marker}");
    assert_eq!(marker["workload_card"], "workload-c", "{marker}");
    assert!(
        run_b.join("cursor.json").is_file(),
        "the other grounded hypothesis started without a fabricated workload artifact"
    );
    let workload_comments_after =
        harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, &workload).unwrap();
    assert_eq!(
        workload_comments_after, workload_comments,
        "no workload candidate or record was fabricated"
    );
    assert!(fixture.cursor()["attempts"].as_array().unwrap().is_empty());
}

/// Grounded selection: a declared successor that is not a valid independent
/// run specification is refused with its exact reason; no successor process
/// is started and nothing is manufactured for it.
#[test]
fn a_successor_that_is_not_an_independent_specification_is_not_started() {
    let fixture = Fixture::new("invalid-successor");
    seed_comparison_spec(&fixture, &json!({}), "invalid-successor");
    let spec_b = fixture.root.join("spec-invalid-successor.json");
    fs::write(&spec_b, br#"{"schema":1}"#).unwrap();
    let run_b = fixture.root.join("runs-invalid");
    let started = fixture.improve(&[
        "start",
        "--run",
        fixture.run.to_str().unwrap(),
        "--spec",
        fixture.spec.to_str().unwrap(),
        "--successor-spec",
        spec_b.to_str().unwrap(),
        "--successor-run",
        run_b.to_str().unwrap(),
    ]);
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    fs::create_dir_all(fixture.run.join("comparison")).unwrap();
    fs::write(
        fixture.run.join("comparison/evaluation.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "policyDigest": "a".repeat(64),
            "decision": "reject",
            "basis": "efficiency",
            "quality": "regressed",
            "matched": 1,
            "baselineSeconds": 2.0,
            "candidateSeconds": 3.0,
            "tolerancePercent": 5.0,
            "coverage": "fixture",
            "scope": "fixture",
            "reasons": ["the candidate regressed the declared metric"],
            "perSuccess": {
                "status": "undefined",
                "seconds": Value::Null,
                "acceptedTasks": 0,
                "tasks": 1,
                "reason": "not used"
            },
            "attempts": 2,
            "tasks": 1,
            "acceptedTasks": 1,
            "acceptanceRate": 1.0,
            "tradeOffUsed": false
        }))
        .unwrap(),
    )
    .unwrap();
    let head = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("decision-recorded");
    cursor["condition"] = Value::Null;
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": "add-synthetic",
        "revision": head,
        "worktree": {
            "source": fixture.proj,
            "path": fixture.proj,
            "branch": "improve/fixture",
            "base": head,
            "revision": head,
        }
    });
    fixture.write_cursor(&cursor);

    let resumed = fixture.resume();
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "idle", "{report}\n{output}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap_or("")
            .contains("not a valid independent run specification"),
        "{report}\n{output}"
    );
    let marker: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("continuation.json")).unwrap()).unwrap();
    assert_eq!(marker["phase"], "intent", "{marker}");
    assert!(
        marker["note"]
            .as_str()
            .unwrap_or("")
            .contains("not a valid independent run specification"),
        "{marker}"
    );
    assert!(
        !run_b.exists(),
        "no successor process was started for an invalid specification"
    );
    assert!(fixture.cursor()["attempts"].as_array().unwrap().is_empty());
}

#[test]
fn a_new_start_defaults_to_continuous_and_consumes_one_stop_request_per_resume() {
    let fixture = Fixture::new("default-supervision");
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    let supervision: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("supervision.json")).unwrap()).unwrap();
    assert_eq!(supervision["mode"], "continuous", "{supervision}");
    let attempts = fixture.cursor()["attempts"].as_array().unwrap().len();

    // A stop request no live controller observed is consumed exactly once by
    // resume, and the run stays available afterwards instead of refusing.
    fs::write(
        fixture.run.join("stop-request.json"),
        br#"{"schema":1,"token":"resume-stop-1","reason":"owner pause"}"#,
    )
    .unwrap();
    let resumed = fixture.resume();
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    assert!(
        output.contains("consumed the exact stop request"),
        "{output}"
    );
    assert!(!fixture.run.join("stop-request.json").is_file());
    let ack: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("stop-acknowledgement.json")).unwrap())
            .unwrap();
    assert_eq!(ack["token"], "resume-stop-1", "{ack}");
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts
    );
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));

    // An explicit single-step start keeps `once` supervision.
    let once = Fixture::new("explicit-once");
    let started = once.improve(&[
        "start",
        "--run",
        once.run.to_str().unwrap(),
        "--spec",
        once.spec.to_str().unwrap(),
        "--supervision",
        "once",
    ]);
    assert!(started.status.success(), "{}", text(&started));
    let supervision: Value =
        serde_json::from_slice(&fs::read(once.run.join("supervision.json")).unwrap()).unwrap();
    assert_eq!(supervision["mode"], "once", "{supervision}");
}

#[test]
fn a_recorded_build_job_is_reconciled_before_another_build_is_started() {
    let fixture = Fixture::new("build-reconcile");
    let state = fixture.root.join("state");
    fs::create_dir_all(state.join("builds/h-build")).unwrap();
    let upstream = fixture.root.join("upstream.exe");
    fs::write(&upstream, b"fixture-client").unwrap();
    let policy = fixture.root.join("policy.json");
    fs::write(&policy, b"{}\n").unwrap();
    let request = fixture.root.join("request.json");
    fs::write(&request, b"{\"schema\":1}\n").unwrap();
    let qualification = fixture.root.join("qualification.json");
    fs::write(&qualification, b"{}\n").unwrap();
    let request_sha = hash_bytes(&fs::read(&request).unwrap());
    let head = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fixture.write_spec(
        &[(
            "runner",
            json!({"profile":"ds","model":Value::Null,"model_provider":Value::Null,"reasoning_effort":Value::Null}),
        ), (
            "local_runner",
            json!({"endpoint":"http://127.0.0.1:9/v1","model":"fixture-glyph-1","identity":{}}),
        ), (
            "qualification",
            json!(qualification),
        ), (
            "comparison",
            comparison_inputs(&fixture, &state, &upstream, &policy, &request, &request_sha, &head),
        )],
        None,
    );
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("candidate-ready");
    cursor["condition"] = Value::Null;
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": "add-synthetic",
        "revision": head,
        "worktree": {
            "source": fixture.proj,
            "path": fixture.proj,
            "branch": "improve/fixture",
            "base": head,
            "revision": head,
        }
    });
    fixture.write_cursor(&cursor);

    // A retained build job records a child that is not running and left no
    // receipt. The controller must not start a second build child.
    let mut dead = Command::new("cmd")
        .args(["/c", "exit", "0"])
        .spawn()
        .expect("a short-lived process starts");
    let pid = dead.id();
    dead.wait().unwrap();
    fs::write(
        fixture.run.join("build-job.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "arm": "candidate",
            "source": fixture.proj,
            "state": state,
            "output": fixture.run.join("build-output.json"),
            "phase": "observed",
            "token": "build-token-1",
            "pid": pid,
            "created": 1,
            "program": manager(),
        }))
        .unwrap(),
    )
    .unwrap();
    let resumed = fixture.resume();
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "blocked", "{report}\n{output}");
    let condition = report["condition"].as_str().unwrap_or("");
    assert!(
        condition.contains("recorded candidate build child") && condition.contains("unknown"),
        "{report}\n{output}"
    );
    assert!(
        !fixture.run.join("build-child.log").is_file(),
        "no second build child is started while a recorded one is unresolved"
    );
    assert!(
        fixture.run.join("build-job.json").is_file(),
        "the unresolved job stays retained"
    );
    assert!(!fixture.run.join("build-output.json").is_file());
}

#[test]
fn a_rejected_experiment_continues_into_the_declared_successor_with_lineage() {
    let fixture = Fixture::new("continue-reject");
    let state = fixture.root.join("state");
    fs::create_dir_all(state.join("builds/h-build")).unwrap();
    fs::create_dir_all(state.join("builds/ha-build")).unwrap();
    let upstream = fixture.root.join("upstream.exe");
    fs::write(&upstream, b"fixture-client").unwrap();
    let policy = fixture.root.join("policy.json");
    fs::write(&policy, b"{}\n").unwrap();
    let request = fixture.root.join("request.json");
    fs::write(&request, b"{\"schema\":1}\n").unwrap();
    let qualification = fixture.root.join("qualification.json");
    fs::write(&qualification, b"{}\n").unwrap();
    let request_sha = hash_bytes(&fs::read(&request).unwrap());
    let head = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fixture.write_spec(
        &[(
            "runner",
            json!({"profile":"ds","model":Value::Null,"model_provider":Value::Null,"reasoning_effort":Value::Null}),
        ), (
            "local_runner",
            json!({"endpoint":"http://127.0.0.1:9/v1","model":"fixture-glyph-1","identity":{}}),
        ), (
            "qualification",
            json!(qualification),
        ), (
            "comparison",
            comparison_inputs(&fixture, &state, &upstream, &policy, &request, &request_sha, &head),
        )],
        None,
    );
    // The successor's own spec: the same synthetic project and admitted
    // change, with its own run state directory.
    let spec_b = fixture.root.join("spec-successor.json");
    let mut document: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    document["run"] = json!("loop-fixture-successor");
    fs::write(&spec_b, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let run_b = fixture.root.join("runs-successor");
    let started = fixture.improve(&[
        "start",
        "--run",
        fixture.run.to_str().unwrap(),
        "--spec",
        fixture.spec.to_str().unwrap(),
        "--successor-spec",
        spec_b.to_str().unwrap(),
        "--successor-run",
        run_b.to_str().unwrap(),
    ]);
    assert!(started.status.success(), "{}", text(&started));
    fs::create_dir_all(fixture.run.join("comparison")).unwrap();
    fs::write(
        fixture.run.join("comparison/evaluation.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "policyDigest": "a".repeat(64),
            "decision": "reject",
            "basis": "efficiency",
            "quality": "regressed",
            "matched": 1,
            "baselineSeconds": 2.0,
            "candidateSeconds": 3.0,
            "tolerancePercent": 5.0,
            "coverage": "fixture",
            "scope": "fixture",
            "reasons": ["the candidate regressed the declared metric"],
            "perSuccess": {
                "status": "undefined",
                "seconds": Value::Null,
                "acceptedTasks": 0,
                "tasks": 1,
                "reason": "not used"
            },
            "attempts": 2,
            "tasks": 1,
            "acceptedTasks": 1,
            "acceptanceRate": 1.0,
            "tradeOffUsed": false
        }))
        .unwrap(),
    )
    .unwrap();
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("decision-recorded");
    cursor["condition"] = Value::Null;
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": "add-synthetic",
        "revision": head,
        "worktree": {
            "source": fixture.proj,
            "path": fixture.proj,
            "branch": "improve/fixture",
            "base": head,
            "revision": head,
        }
    });
    fixture.write_cursor(&cursor);

    // A rejected experiment still consumes the declared continuation, and the
    // successor is started only with retained lineage and its exact spec.
    let resumed = fixture.resume();
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    assert!(output.contains("successor"), "{output}");
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "idle", "{report}");
    let marker: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("continuation.json")).unwrap()).unwrap();
    assert_eq!(marker["phase"], "completed", "{marker}");
    assert_eq!(marker["exit_code"], 0, "{marker}");
    assert_eq!(
        marker["spec_sha256"],
        hash_bytes(&fs::read(&spec_b).unwrap()),
        "{marker}"
    );
    assert_eq!(
        marker["lineage_sha256"],
        hash_bytes(&fs::read(fixture.run.join("lineage.json")).unwrap()),
        "{marker}"
    );
    assert_eq!(marker["decision"], "reject", "{marker}");
    assert!(
        run_b.join("cursor.json").is_file(),
        "the declared successor run was started"
    );
    assert!(run_b.join("spec.json").is_file());

    // The completed continuation is not started again on a later resume.
    let again = fixture.resume();
    let output = text(&again);
    assert!(again.status.success(), "{output}");
    assert!(output.contains("already completed"), "{output}");
}

#[test]
fn an_idle_continuation_record_does_not_suppress_a_later_declared_successor() {
    let fixture = Fixture::new("continue-later");
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("activation-confirmed");
    cursor["condition"] = Value::Null;
    fixture.write_cursor(&cursor);
    fs::write(fixture.run.join("lineage.json"), b"{\"schema\":1}\n").unwrap();
    let first = fixture.resume();
    assert!(first.status.success(), "{}", text(&first));
    let report = status_value(&fixture);
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("no independently specified successor"),
        "{report}"
    );
    let marker: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("continuation.json")).unwrap()).unwrap();
    assert_eq!(marker["phase"], "idle", "{marker}");

    // A successor declared later is started instead of being suppressed by
    // the earlier no-successor record.
    let spec_b = fixture.root.join("spec-later.json");
    let mut document: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    document["run"] = json!("loop-fixture-later");
    fs::write(&spec_b, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let run_b = fixture.root.join("runs-later");
    fs::write(
        fixture.run.join("successor.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "spec": spec_b,
            "run": run_b,
        }))
        .unwrap(),
    )
    .unwrap();
    let resumed = fixture.resume();
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    let marker: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("continuation.json")).unwrap()).unwrap();
    assert_eq!(marker["phase"], "completed", "{marker}");
    assert_eq!(
        marker["run"].as_str().map(PathBuf::from),
        Some(run_b.clone()),
        "{marker}"
    );
    assert!(
        run_b.join("cursor.json").is_file(),
        "the later declared successor run was started"
    );
}

#[test]
fn stopping_the_controller_suspends_the_running_successor_through_its_own_owner() {
    // A real successor run state exists first, so the successor's own stop
    // owner can resolve it.
    let successor = Fixture::new("successor-stop-b");
    let started = successor.improve(&[
        "start",
        "--run",
        successor.run.to_str().unwrap(),
        "--spec",
        successor.spec.to_str().unwrap(),
        "--supervision",
        "once",
    ]);
    assert!(started.status.success(), "{}", text(&started));

    let fixture = Fixture::new("successor-stop-a");
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("idle");
    cursor["condition"] = Value::Null;
    fixture.write_cursor(&cursor);
    let lineage = fixture.run.join("lineage.json");
    fs::write(&lineage, b"{\"schema\":1,\"decision\":\"reject\"}\n").unwrap();
    fs::write(
        fixture.run.join("successor.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "spec": successor.spec,
            "run": successor.run,
        }))
        .unwrap(),
    )
    .unwrap();

    // A stand-in successor controller that this run did not spawn: the exact
    // recorded identity is what the stop path resolves.
    let program = powershell();
    let mut stand_in = start_sleeper();
    let user = harness_core::process_service::current_user().unwrap();
    let observed =
        harness_core::process_service::ServiceProcess::observe(stand_in.id(), &program, 0, &user)
            .expect("the stand-in identity is observed");
    fs::write(
        fixture.run.join("continuation.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "successor_started": true,
            "note": "stand-in observed successor",
            "phase": "observed",
            "token": "observed-1",
            "spec": successor.spec,
            "spec_sha256": hash_bytes(&fs::read(&successor.spec).unwrap()),
            "run": successor.run,
            "lineage": lineage,
            "lineage_sha256": hash_bytes(&fs::read(&lineage).unwrap()),
            "decision": "reject",
            "pid": observed.identity().pid,
            "created": observed.identity().creation_time,
            "program": program,
        }))
        .unwrap(),
    )
    .unwrap();

    let run_arg = fixture.run.to_str().unwrap().to_owned();
    let mut controller = Command::new(manager())
        .arg("improve")
        .args(["resume", "--run", &run_arg])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("continuous resume starts");
    let waiting = wait_for(
        || {
            status_value(&fixture)
                .get("condition")
                .and_then(|value| value.as_str())
                .is_some_and(|condition| condition.contains("recorded successor controller"))
        },
        Duration::from_secs(20),
    );
    if !waiting {
        let _ = stand_in.kill();
        panic!(
            "controller did not wait for the recorded successor: {}",
            controller_output(controller)
        );
    }
    let stopped = fixture.improve(&["stop", "--run", &run_arg, "--reason", "test stop"]);
    assert!(stopped.status.success(), "{}", text(&stopped));
    let successor_stopped = wait_for(
        || {
            successor
                .cursor()
                .get("phase")
                .and_then(|value| value.as_str())
                == Some("stopped")
        },
        Duration::from_secs(20),
    );
    let _ = stand_in.kill();
    let _ = stand_in.wait();
    assert!(
        successor_stopped,
        "the successor was not suspended through its own owner: {}",
        text(&stopped)
    );
    let finished = wait_for(
        || matches!(controller.try_wait(), Ok(Some(_))),
        Duration::from_secs(20),
    );
    if !finished {
        panic!(
            "controller did not leave after stop: {}",
            controller_output(controller)
        );
    }
    let output = controller.wait_with_output().unwrap();
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.contains("successor stop owner"),
        "the stop established cleanup through the successor's own owner: {output}"
    );
    let marker: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("continuation.json")).unwrap()).unwrap();
    assert_eq!(marker["phase"], "stopped", "{marker}");
    assert!(
        !fixture.run.join("stop-request.json").is_file(),
        "the consumed stop request does not linger"
    );
}

/// One comparison-ready run spec: owned state directories, the synthetic
/// client request, the declared qualification record and a predeclared policy
/// file, exactly as the existing continuous tests seed them.
fn seed_comparison_spec(fixture: &Fixture, policy_document: &Value, name: &str) -> String {
    let state = fixture.root.join(format!("state-{name}"));
    fs::create_dir_all(state.join("builds/h-build")).unwrap();
    fs::create_dir_all(state.join("builds/ha-build")).unwrap();
    let upstream = fixture.root.join("upstream.exe");
    fs::write(&upstream, b"fixture-client").unwrap();
    let policy = fixture.root.join(format!("policy-{name}.json"));
    fs::write(&policy, serde_json::to_vec_pretty(policy_document).unwrap()).unwrap();
    let request = fixture.root.join("request.json");
    fs::write(&request, b"{\"schema\":1}\n").unwrap();
    let qualification = fixture.root.join("qualification.json");
    fs::write(&qualification, b"{}\n").unwrap();
    let request_sha = hash_bytes(&fs::read(&request).unwrap());
    let head = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fixture.write_spec(
        &[
            (
                "runner",
                json!({"profile":"ds","model":Value::Null,"model_provider":Value::Null,"reasoning_effort":Value::Null}),
            ),
            (
                "local_runner",
                json!({"endpoint":"http://127.0.0.1:9/v1","model":"fixture-glyph-1","identity":{}}),
            ),
            ("qualification", json!(qualification)),
            (
                "comparison",
                comparison_inputs(
                    fixture,
                    &state,
                    &upstream,
                    &policy,
                    &request,
                    &request_sha,
                    &head,
                ),
            ),
        ],
        None,
    );
    head
}

/// A predeclared policy that requires one additional independent unit beyond
/// the run's own declared plan unit.
fn corroboration_policy() -> Value {
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

/// The workload's own hypothesis card. The merged admission owner reuses one
/// card per mechanism/conditions identity, so this returns the existing card
/// rather than creating a duplicate hypothesis card.
fn admit_workload_card(fixture: &Fixture) -> String {
    let out = fixture.feedback(&[
        "hypothesis-admit",
        "--mechanism",
        "bounded-output",
        "--conditions",
        "local-tool-runs",
        "--observation",
        "token-audit:findings#13",
        "--predicted",
        "the workload runs with bounded output",
        "--counterexample",
        "the frozen workload changes between arms",
        "--acceptance",
        "the independent oracle passes",
        "--spec",
        "openspec/changes/add-synthetic",
        "--basis",
        "evidence-2",
    ]);
    assert!(out.status.success(), "workload admit: {}", text(&out));
    let printed = text(&out);
    let id = printed
        .strip_prefix("hypothesis ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default()
        .to_owned();
    assert!(!id.is_empty(), "admission named no card: {printed}");
    id
}

/// The retained comparison bindings at a decision boundary: both arms bind
/// one frozen pre-solution workload copy, so the completed real task's inputs
/// are replayable without its solution.
fn seed_bindings(
    fixture: &Fixture,
    workload: &harness_core::task_worktree::FrozenCopy,
    revision: &str,
) -> PathBuf {
    let path = fixture.run.join("comparison/bindings.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let arm = |name: &str, arm: &str| {
        json!({
            "arm": arm,
            "home": fixture.root.join(name).display().to_string(),
            "workload": workload,
            "runtime": {
                "arm": arm,
                "label": name,
                "build": fixture.root.join("builds").join(name).display().to_string(),
                "recordSha256": "a".repeat(64),
                "sourceSha256": "b".repeat(64),
            },
        })
    };
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "hypothesis": fixture.card,
            "caseId": "case-b",
            "baseRevision": revision,
            "candidate": {
                "source": fixture.proj,
                "path": fixture.proj,
                "branch": "improve/fixture",
                "base": revision,
                "revision": revision,
            },
            "oracle": "oracle-7",
            "acceptance": "acceptance/run-9",
            "policyDigest": "c".repeat(64),
            "arms": [arm("baseline-home", "baseline"), arm("candidate-home", "candidate")],
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

/// Publish the exact `benefit-gate v2` non-adoption the comparison owner
/// publishes before the controller consumes the decision.
fn publish_rejection(fixture: &Fixture, baseline: &str, candidate: &str) {
    harness_core::benefit_gate::publish_non_adoption(
        &fixture.bd,
        &fixture.proj,
        &harness_core::benefit_gate::NonAdoptionDraft {
            item: fixture.card.clone(),
            experiment: "exp-fixture".to_owned(),
            outcome: harness_core::benefit_gate::DecisionOutcome::Reject,
            quality: harness_core::benefit_gate::QualityOutcome::Regressed,
            accounting: "attempts:2,tasks:1,accepted:1".to_owned(),
            baseline_revision: baseline.to_owned(),
            candidate_revision: candidate.to_owned(),
            acceptance: "acceptance/run-9".to_owned(),
            coverage: "fixture".to_owned(),
            scope: "fixture".to_owned(),
            reason: "regressed".to_owned(),
            detail: None,
        },
    )
    .unwrap();
}

/// Seed the retained evaluation receipt and the cursor the decision boundary
/// consumes, exactly as the comparison owner leaves them once a measured pair
/// produced its evidence-bound decision.
fn seed_decision_boundary(
    fixture: &Fixture,
    bindings: &Path,
    workload_card: &str,
    revision: &str,
    decision: &str,
) -> PathBuf {
    let comparison = fixture.run.join("comparison");
    fs::create_dir_all(&comparison).unwrap();
    let evaluation = comparison.join("evaluation.json");
    fs::write(
        &evaluation,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "policyDigest": "a".repeat(64),
            "decision": decision,
            "basis": "efficiency",
            "quality": "regressed",
            "matched": 1,
            "baselineSeconds": 2.0,
            "candidateSeconds": 3.0,
            "tolerancePercent": 5.0,
            "coverage": "fixture",
            "scope": "fixture",
            "reasons": ["the candidate regressed the declared metric"],
            "perSuccess": {
                "status": "undefined",
                "seconds": Value::Null,
                "acceptedTasks": 0,
                "tasks": 1,
                "reason": "not used"
            },
            "attempts": 2,
            "tasks": 1,
            "acceptedTasks": 1,
            "acceptanceRate": 1.0,
            "tradeOffUsed": false
        }))
        .unwrap(),
    )
    .unwrap();
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("decision-recorded");
    cursor["condition"] = Value::Null;
    cursor["experiment"] = json!("exp-fixture");
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": "add-synthetic",
        "revision": revision,
        "worktree": {
            "source": fixture.proj,
            "path": fixture.proj,
            "branch": "improve/fixture",
            "base": revision,
            "revision": revision,
        }
    });
    cursor["comparison"] = json!({
        "schema": 1,
        "policy_digest": "a".repeat(64),
        "bindings": bindings.display().to_string(),
        "workload_card": workload_card,
        "evaluation": evaluation.display().to_string(),
    });
    fixture.write_cursor(&cursor);
    evaluation
}

/// Every main-specification file under one planning root with its digest.
fn main_spec_files(root: &Path) -> BTreeMap<String, String> {
    let specs = root.join("openspec/specs");
    if specs.is_dir() {
        build_files(&specs)
    } else {
        BTreeMap::new()
    }
}

/// A retained completed real task that a prior run already recorded: the
/// frozen pre-solution copy plus a committed answer the retention never keeps.
fn prior_retained_task(
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
    git(&completed.path, &["commit", "-qm", "prior answer"]);
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

/// One admitted hypothesis card, as the durable owner a prior retained task is
/// recorded under. Retention records live on hypothesis cards, so a later run
/// discovers them through the board.
fn admit_prior_hypothesis_card(fixture: &Fixture, mechanism: &str) -> String {
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

/// One admitted hypothesis card with its own distinct mechanism identity and
/// the fixture's complete OpenSpec change. The merged admission owner reuses
/// one card per mechanism/conditions identity, so a distinct mechanism
/// yields a distinct durable owner - as an evaluated workload B or another
/// independently specified next hypothesis.
fn admit_distinct_hypothesis_card(fixture: &Fixture, mechanism: &str) -> String {
    let out = fixture.feedback(&[
        "hypothesis-admit",
        "--mechanism",
        mechanism,
        "--conditions",
        "local-tool-runs",
        "--observation",
        "token-audit:other#4",
        "--predicted",
        "another grounded hypothesis continues",
        "--counterexample",
        "the workload produced no candidate patch",
        "--acceptance",
        "the independent oracle passes",
        "--spec",
        "openspec/changes/add-synthetic",
        "--basis",
        "token-audit:other#4",
    ]);
    assert!(out.status.success(), "other admit: {}", text(&out));
    let printed = text(&out);
    let id = printed
        .strip_prefix("hypothesis ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default()
        .to_owned();
    assert!(!id.is_empty(), "other admission named no card: {printed}");
    id
}

/// Rewrite one run spec's declared comparison workload card in place.
fn set_comparison_workload(spec_path: &Path, workload: &str) {
    let mut document: Value = serde_json::from_slice(&fs::read(spec_path).unwrap()).unwrap();
    document["comparison"]["workload_card"] = json!(workload);
    fs::write(spec_path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
}

/// One independently specified successor run spec: this fixture's own run
/// inputs with their own run identity, hypothesis card and declared workload
/// C, exactly as an operator declares the next run.
fn successor_run_spec(fixture: &Fixture, path: &Path, run: &str, hypothesis: &str, workload: &str) {
    let mut document: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    document["run"] = json!(run);
    document["hypothesis_item"] = json!(hypothesis);
    document["comparison"]["workload_card"] = json!(workload);
    fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
}

/// Record one retained task on its owner card through the durable board
/// writer, exactly as the controller does at the decision boundary, so a later
/// run discovers it from the board instead of a run-local index.
fn record_prior_retention_on_board(
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

fn comparison_inputs(
    fixture: &Fixture,
    state: &Path,
    upstream: &Path,
    policy: &Path,
    request: &Path,
    request_sha: &str,
    revision: &str,
) -> Value {
    json!({
        "schema": 1,
        "specification": {
            "project": fixture.proj,
            "change": "add-synthetic",
            "store": Value::Null,
            "planning_root": fixture.proj,
        },
        "contract": {
            "acceptance_artifact": "specs/synthetic/spec.md",
            "acceptance_heading": "#### Scenario: Synthetic case",
            "mechanism": "frozen-workload",
            "counterexample": "the workload changes between arms",
            "applicability": "the frozen task snapshot",
            "independent_acceptance": "the oracle checker executes",
            "meaningful_effect": "less repeated work",
            "operating_conditions": "one pair",
            "comparison_policy": "matched pairs",
            "stopping_rule": "one pair",
        },
        "workload_card": "workload-b",
        "task": {
            "source": fixture.proj,
            "revision": revision,
            "name": "workload-b",
            "writable_scope": ["crates/one"],
        },
        "runtimes": {
            "state": state,
            "baseline_build": state.join("builds/h-build"),
            "candidate_build": state.join("builds/ha-build"),
            "baseline_label": "H",
            "candidate_label": "H+A",
            "upstream": upstream,
            "client": {
                "runner": {
                    "endpoint": "http://127.0.0.1:9/v1",
                    "model": "fixture-glyph-1",
                    "identity": {},
                },
                "reasoningEffort": "low",
            },
        },
        "policy": policy,
        "acceptance": {
            "request": request,
            "request_sha256": request_sha,
        },
        "observation_inputs": [],
    })
}

/// A synthetic harness source the native build owner can actually compile.
/// It mirrors the owned fixture in `tests/native_build.rs`: a stub manager
/// workspace whose `finalize-build-v1`/`check`/`activate-build` dispatch runs
/// over the real `harness-core` source.
fn scaffold_native_source(source: &Path) {
    let schema = source.join(harness_core::build_identity::INSPECTION_SCHEMA);
    fs::create_dir_all(schema.parent().unwrap()).unwrap();
    fs::write(schema, "{}").unwrap();
    fs::create_dir_all(source.join("crates/manager/src")).unwrap();
    fs::create_dir_all(source.join("crates/harness-rtk/src")).unwrap();
    fs::write(
        source.join("Cargo.toml"),
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
            .unwrap()
            .replace("crates/codex-harness", "crates/manager")
            .split("[profile.release]")
            .next()
            .unwrap()
            .to_owned()
            + "\n[profile.release]\nopt-level=0\n",
    )
    .unwrap();
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../harness-core");
    copy_source_tree(&core.join("src"), &source.join("crates/harness-core/src"));
    fs::copy(
        core.join("Cargo.toml"),
        source.join("crates/harness-core/Cargo.toml"),
    )
    .unwrap();
    let evolution = Path::new(env!("CARGO_MANIFEST_DIR")).join("../skill-evolution");
    copy_source_tree(
        &evolution.join("src"),
        &source.join("crates/skill-evolution/src"),
    );
    fs::copy(
        evolution.join("Cargo.toml"),
        source.join("crates/skill-evolution/Cargo.toml"),
    )
    .unwrap();
    for (directory, name) in [
        ("crates/manager", "codex-harness"),
        ("crates/harness-rtk", "harness-rtk"),
        ("crates/token-audit", "token-audit"),
    ] {
        fs::create_dir_all(source.join(directory).join("src")).unwrap();
        fs::write(
            source.join(directory).join("Cargo.toml"),
            format!(
                "[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n{}",
                if name == "codex-harness" {
                    "[dependencies]\nharness-core={path='../harness-core'}\nserde_json.workspace=true\n"
                } else {
                    ""
                }
            ),
        )
        .unwrap();
        fs::write(
            source.join(directory).join("src/main.rs"),
            if name == "codex-harness" {
                manager_source("fn main() { println!(\"owned native fixture\"); }\n")
            } else {
                "fn main() {}\n".into()
            },
        )
        .unwrap();
    }
    // The synthetic manifest keeps the real workspace member list so ancestor
    // Cargo configuration stays part of build identity; every listed member
    // must exist, even when the fixture replaces it with a stub.
    let manifest = fs::read_to_string(source.join("Cargo.toml")).unwrap();
    let members = manifest
        .split("members = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap();
    for member in members.split_whitespace() {
        let member = member.trim_matches(|character| character == '"' || character == ',');
        if member.is_empty() {
            continue;
        }
        let directory = source.join(member);
        if directory.join("Cargo.toml").exists() {
            continue;
        }
        fs::create_dir_all(directory.join("src")).unwrap();
        fs::write(
            directory.join("Cargo.toml"),
            format!(
                "[package]\nname='{}'\nversion='0.1.0'\nedition='2024'\n",
                member.rsplit('/').next().unwrap_or(member)
            ),
        )
        .unwrap();
        fs::write(directory.join("src/lib.rs"), "").unwrap();
    }
    fs::create_dir_all(source.join("crates/manager/src/bin")).unwrap();
    for name in harness_core::build_identity::BINARIES {
        if !["codex-harness.exe", "harness-rtk.exe"].contains(name) {
            fs::write(
                source
                    .join("crates/manager/src/bin")
                    .join(name.replace(".exe", ".rs")),
                "fn main() {}\n",
            )
            .unwrap();
        }
    }
    let lock = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(source)
        .output()
        .unwrap();
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
}

/// The same dispatch the native build tests use, so the compiled stub manager
/// can finish its own handoff without a real manager source.
fn manager_source(program: &str) -> String {
    let dispatch = r#"
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "finalize-build-v1") {
        if let Err(error) = harness_core::native_build::finalize(std::path::Path::new(&args[1])) {
            eprintln!("{error}"); std::process::exit(2);
        }
        return;
    }
    if args.first().is_some_and(|a| a == "check") {
        let source = args.windows(2).find(|w| w[0] == "--source").map(|w| std::path::Path::new(&w[1]));
        let build = args.windows(2).find(|w| w[0] == "--build").unwrap();
        let report = harness_core::build_identity::check(std::path::Path::new(&build[1]), source);
        println!("{}", serde_json::to_string(&report).unwrap());
        std::process::exit(i32::from(
            report.status != harness_core::build_identity::Health::Healthy,
        ));
    }
    if args.first().is_some_and(|a| a == "activate-build") {
        let state = args.windows(2).find(|w| w[0] == "--state").unwrap();
        let build = args.windows(2).find(|w| w[0] == "--build").unwrap();
        match harness_core::build_selection::activate(std::path::Path::new(&state[1]), std::path::Path::new(&build[1])) {
            Ok(result) => println!("{}", serde_json::to_string(&result).unwrap()),
            Err(error) => { eprintln!("{error}"); std::process::exit(2); }
        }
        return;
    }
"#;
    program.replacen("fn main() {", &format!("fn main() {{{dispatch}"), 1)
}

fn copy_source_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        assert!(!entry.file_type().unwrap().is_symlink());
        if path.is_dir() {
            copy_source_tree(&path, &destination.join(entry.file_name()));
        } else {
            fs::copy(&path, destination.join(entry.file_name())).unwrap();
        }
    }
}

/// The controller prepares the missing candidate runtime through the real
/// native build owner: the stub harness workspace is compiled and published
/// inside the owned state with its receipt, while the pre-published baseline
/// build is left alone.
#[test]
fn the_controller_prepares_a_missing_runtime_through_the_native_owner() {
    let fixture = Fixture::new("native-build-success");
    let state = fixture.root.join("state");
    fs::create_dir_all(state.join("builds/h-build")).unwrap();
    fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
    let source = fixture.root.join("native-source");
    scaffold_native_source(&source);
    let upstream = fixture.root.join("upstream.exe");
    fs::write(&upstream, b"fixture-client").unwrap();
    let policy = fixture.root.join("policy.json");
    fs::write(&policy, b"{}\n").unwrap();
    let request = fixture.root.join("request.json");
    fs::write(&request, b"{\"schema\":1}\n").unwrap();
    let qualification = fixture.root.join("qualification.json");
    fs::write(&qualification, b"{}\n").unwrap();
    let request_sha = hash_bytes(&fs::read(&request).unwrap());
    let head = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fixture.write_spec(
        &[(
            "runner",
            json!({"profile":"ds","model":Value::Null,"model_provider":Value::Null,"reasoning_effort":Value::Null}),
        ), (
            "local_runner",
            json!({"endpoint":"http://127.0.0.1:9/v1","model":"fixture-glyph-1","identity":{}}),
        ), (
            "qualification",
            json!(qualification),
        ), (
            "comparison",
            comparison_inputs(&fixture, &state, &upstream, &policy, &request, &request_sha, &head),
        )],
        None,
    );
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("candidate-ready");
    cursor["condition"] = Value::Null;
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": "add-synthetic",
        "revision": head,
        "worktree": {
            "source": fixture.proj,
            "path": source,
            "branch": "improve/fixture",
            "base": head,
            "revision": head,
        }
    });
    fixture.write_cursor(&cursor);

    // The heavy build runs in an owned isolated account so the check is
    // independent of any ambient shared heavy-command lease.
    let heavy_account = fixture.root.join("heavy-account");
    let cpu_account = fixture.root.join("cpu-account");
    let mut command = Command::new(manager());
    command
        .arg("improve")
        .args(["resume", "--run", fixture.run.to_str().unwrap()]);
    command.env("CODEX_HARNESS_HEAVY_ACCOUNT", &heavy_account);
    command.env("CODEX_HARNESS_CPU_ACCOUNT", &cpu_account);
    let resumed = command.output().expect("resume runs");
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    assert!(
        output.contains("prepared the missing candidate runtime"),
        "{output}"
    );

    let prepared: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("prepared-builds.json")).unwrap())
            .unwrap();
    let recorded = prepared["candidate"].as_str().expect("candidate build");
    let build = PathBuf::from(recorded.strip_prefix(r"\\?\").unwrap_or(recorded));
    assert!(build.starts_with(state.join("builds")), "{prepared}");
    assert!(build.join("build.json").is_file(), "{prepared}");
    assert!(build.join("codex-harness.exe").is_file(), "{prepared}");
    let identity = prepared["candidate_identity"].as_str().unwrap();
    assert_eq!(identity.len(), 64, "{prepared}");
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("build-output.json")).unwrap()).unwrap();
    assert_eq!(receipt["reused"], false, "{receipt}");
    assert_eq!(receipt["build"], prepared["candidate"], "{receipt}");
    assert!(
        !fixture.run.join("build-job.json").is_file(),
        "the reconciled build job is removed once its child was reaped"
    );
    assert!(fixture.run.join("build-child.log").is_file());
}

/// A settled unadopted decision consumes the merged owners at the decision
/// boundary with no user confirmation: the completed real workload task is
/// retained under its existing card as identity-only replayable pre-solution
/// inputs, and the hypothesis' own change is reconciled without closing its
/// unfinished required task or synchronizing its delta into the main specs.
#[test]
fn a_settled_unadopted_decision_retains_the_task_and_reconciles_its_change() {
    let fixture = Fixture::new("decision-consumption");
    let head = seed_comparison_spec(&fixture, &json!({}), "consume");
    let workload = harness_core::task_worktree::frozen_copy(
        &fixture.proj,
        &head,
        &fixture.root.join("workload-pre"),
    )
    .unwrap();
    let workload_card = admit_workload_card(&fixture);
    let bindings = seed_bindings(&fixture, &workload, &head);
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    publish_rejection(&fixture, &head, &workload.revision);
    seed_decision_boundary(&fixture, &bindings, &workload_card, &head, "reject");

    // The routine decision-boundary transition needs no per-hypothesis
    // confirmation: one resume consumes the merged owners.
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "idle", "{report}");

    // Retention: identity-only replayable pre-solution inputs under the card
    // that already owns the completed real task.
    let retention = report["retention"].clone();
    assert_eq!(retention["status"], "retained", "{report}");
    assert_eq!(retention["owner"], workload_card, "{report}");
    assert_eq!(retention["case_id"], "case-b", "{report}");
    let replay = PathBuf::from(retention["replay"].as_str().expect("replay locator"));
    assert!(replay.is_dir(), "{report}");
    let index: Vec<harness_core::improvement_experiment::RetainedTask> =
        serde_json::from_slice(&fs::read(fixture.run.join("retained-tasks.json")).unwrap())
            .unwrap();
    assert_eq!(index.len(), 1);
    assert_eq!(index[0].case_id, "case-b");
    harness_core::task_worktree::verify_frozen_pristine(&index[0].replay).unwrap();
    assert!(
        !index[0].replay.path.join("answer.txt").exists(),
        "the retained copy is pre-solution"
    );
    let comments =
        harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, &workload_card)
            .unwrap();
    assert!(
        comments
            .iter()
            .any(|comment| comment.starts_with("hypothesis-retention v1")),
        "{comments:?}"
    );
    assert!(
        comments
            .iter()
            .all(|comment| !comment.contains("prior solution") && !comment.contains("answer")),
        "{comments:?}"
    );

    // Reconcile: the finished experiment outcome never closes an unfinished
    // required task and never pushes the unadopted delta into the specs.
    let reconcile = report["reconcile"].clone();
    assert_eq!(reconcile["item"], fixture.card, "{report}");
    assert_eq!(reconcile["outcome"], "reject", "{report}");
    assert_eq!(reconcile["action"], "archive", "{report}");
    assert_eq!(reconcile["change"], "add-synthetic", "{report}");
    assert_eq!(reconcile["status"], "retained", "{report}");
    assert!(
        reconcile["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("archive=unresolved"),
        "{report}"
    );
    let tasks =
        fs::read_to_string(fixture.proj.join("openspec/changes/add-synthetic/tasks.md")).unwrap();
    assert!(tasks.contains("- [ ] 1.1"), "{tasks}");
    assert!(
        fixture.proj.join("openspec/changes/add-synthetic").is_dir(),
        "the unfinished change stays active"
    );
    let archive = fixture.proj.join("openspec/changes/archive");
    assert!(
        !archive.is_dir()
            || fs::read_dir(&archive)
                .unwrap()
                .all(|entry| !entry.unwrap().path().is_dir()),
        "an unfinished change is not archived"
    );
    assert!(!fixture.run.join("integration.json").is_file());
    assert!(!fixture.run.join("activation.json").is_file());

    // Status surfaces the consumption; the decision itself is unchanged.
    let printed = text(&fixture.improve(&["status", "--run", fixture.run.to_str().unwrap()]));
    assert!(printed.contains("retention: status=retained"), "{printed}");
    assert!(printed.contains("reconcile:"), "{printed}");

    // Idle/deferred truthfulness: a repeated resume repeats no identical
    // retention, reconcile or model call, and starts no duplicate card.
    let retained_receipt = fs::read(fixture.run.join("retention.json")).unwrap();
    let reconcile_receipt = fs::read(fixture.run.join("reconcile.json")).unwrap();
    let cards = harness_core::board_hypothesis::list_hypothesis_cards(&fixture.bd, &fixture.proj)
        .unwrap()
        .len();
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(
        fs::read(fixture.run.join("retention.json")).unwrap(),
        retained_receipt,
        "the identical retention is not repeated"
    );
    assert_eq!(
        fs::read(fixture.run.join("reconcile.json")).unwrap(),
        reconcile_receipt,
        "the identical reconcile is not repeated"
    );
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 0);
    assert_eq!(
        harness_core::board_hypothesis::list_hypothesis_cards(&fixture.bd, &fixture.proj)
            .unwrap()
            .len(),
        cards,
        "no duplicate hypothesis card is created"
    );
    let report = status_value(&fixture);
    assert_eq!(report["phase"], "idle", "{report}");
    assert!(
        report["next"]
            .as_str()
            .unwrap_or_default()
            .contains("no model work is started while idle"),
        "{report}"
    );
}

/// A completed but unadopted change is archived through the merged owner's
/// supported non-synchronizing path: its artifacts stay referencable in the
/// change archive while the main specifications keep their exact content.
#[test]
fn a_completed_unadopted_change_is_archived_without_syncing_its_delta() {
    let fixture = Fixture::new("decision-archive");
    fs::write(
        fixture.proj.join("openspec/changes/add-synthetic/tasks.md"),
        "## 1. Work\n\n- [x] 1.1 Do the synthetic thing.\n",
    )
    .unwrap();
    let head = seed_comparison_spec(&fixture, &json!({}), "archive");
    let workload = harness_core::task_worktree::frozen_copy(
        &fixture.proj,
        &head,
        &fixture.root.join("workload-pre"),
    )
    .unwrap();
    let workload_card = admit_workload_card(&fixture);
    let bindings = seed_bindings(&fixture, &workload, &head);
    let specs_before = main_spec_files(&fixture.proj);
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    publish_rejection(&fixture, &head, &workload.revision);
    seed_decision_boundary(&fixture, &bindings, &workload_card, &head, "reject");

    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let report = status_value(&fixture);
    let reconcile = report["reconcile"].clone();
    assert_eq!(reconcile["status"], "archived", "{report}");
    assert!(
        reconcile["detail"]
            .as_str()
            .unwrap_or_default()
            .contains("archived-as="),
        "{report}"
    );
    assert!(
        !fixture.proj.join("openspec/changes/add-synthetic").exists(),
        "the archived change left the active changes"
    );
    let archive = fixture.proj.join("openspec/changes/archive");
    assert!(
        archive.is_dir(),
        "the change is referencable in the archive"
    );
    let archived = fs::read_dir(&archive)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_dir())
        .expect("one archived change directory");
    assert!(
        archived.join("tasks.md").is_file(),
        "the archived artifacts stay readable"
    );
    assert_eq!(
        main_spec_files(&fixture.proj),
        specs_before,
        "the unadopted delta is not synchronized into the main specifications"
    );
}

/// Corroboration selection consumes the merged selector by identity only: a
/// retained task that does not exercise the declared mechanism is excluded as
/// non-evidence, and too few applicable units leave the broader claim
/// explicitly inconclusive instead of fabricating a summary.
#[test]
fn declared_corroboration_excludes_inapplicable_workloads_and_stays_inconclusive() {
    let fixture = Fixture::new("corroboration-short");
    let head = seed_comparison_spec(&fixture, &corroboration_policy(), "short");
    let workload = harness_core::task_worktree::frozen_copy(
        &fixture.proj,
        &head,
        &fixture.root.join("workload-pre"),
    )
    .unwrap();
    let workload_card = admit_workload_card(&fixture);
    let bindings = seed_bindings(&fixture, &workload, &head);
    let owner = admit_prior_hypothesis_card(&fixture, "other-mechanism");
    let prior = prior_retained_task(&fixture, &head, &owner, "case-x", "other-mechanism");
    record_prior_retention_on_board(&fixture, &owner, &prior);
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    publish_rejection(&fixture, &head, &workload.revision);
    seed_decision_boundary(&fixture, &bindings, &workload_card, &head, "reject");

    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("corroboration.json")).unwrap()).unwrap();
    assert_eq!(receipt["status"], "selected", "{receipt}");
    assert_eq!(receipt["required_units"], 1, "{receipt}");
    assert!(
        receipt["selection"]["status"]["inconclusive"].is_string(),
        "{receipt}"
    );
    assert!(
        receipt["selection"]["units"].as_array().unwrap().is_empty(),
        "{receipt}"
    );
    let excluded = receipt["selection"]["excluded"].as_array().unwrap();
    assert_eq!(excluded.len(), 2, "{receipt}");
    assert_eq!(excluded[0]["caseId"], "case-x", "{receipt}");
    assert_eq!(excluded[0]["reason"], "notApplicable", "{receipt}");
    assert_eq!(excluded[1]["caseId"], "case-b", "{receipt}");
    assert_eq!(excluded[1]["reason"], "alreadyUsed", "{receipt}");
    let reason = receipt["selection"]["status"]["inconclusive"]
        .as_str()
        .unwrap_or_default();
    assert!(
        reason.contains("fewer applicable independent replayable retained tasks"),
        "{receipt}"
    );
    assert!(
        !text(&resumed).contains("prior solution"),
        "{}",
        text(&resumed)
    );
    let report = status_value(&fixture);
    assert_eq!(report["corroboration"]["status"], "selected", "{report}");
    assert_eq!(report["corroboration"]["ready"], false, "{report}");
    assert_eq!(report["corroboration"]["required_units"], 1, "{report}");
    let printed = text(&fixture.improve(&["status", "--run", fixture.run.to_str().unwrap()]));
    assert!(
        printed.contains("corroboration: required=+1 status=inconclusive"),
        "{printed}"
    );
}

/// With one applicable independent retained unit available, the selection is
/// ready and returns that unit's identity and replay references only - never
/// the earlier solution - so a fresh executor reimplements the task.
#[test]
fn declared_corroboration_selects_an_independent_retained_unit_by_identity() {
    let fixture = Fixture::new("corroboration-ready");
    let head = seed_comparison_spec(&fixture, &corroboration_policy(), "ready");
    // A second committed revision so the independent unit is a distinct task
    // snapshot rather than a byte-identical replay of the run's own unit.
    fs::write(fixture.proj.join("prior-task.txt"), "prior real task\n").unwrap();
    git(&fixture.proj, &["add", "."]);
    git(&fixture.proj, &["commit", "-qm", "prior task identity"]);
    let prior_revision = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let workload = harness_core::task_worktree::frozen_copy(
        &fixture.proj,
        &head,
        &fixture.root.join("workload-pre"),
    )
    .unwrap();
    let workload_card = admit_workload_card(&fixture);
    let bindings = seed_bindings(&fixture, &workload, &head);
    // An applicable prior unit is admitted to the same hypothesis card the
    // run's own workload uses - admission reuses one card per
    // mechanism/conditions identity - so only its case id distinguishes the
    // prior unit from the run's own.
    let owner = admit_prior_hypothesis_card(&fixture, "bounded-output");
    let prior = prior_retained_task(
        &fixture,
        &prior_revision,
        &owner,
        "case-c",
        "bounded-output",
    );
    record_prior_retention_on_board(&fixture, &owner, &prior);
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    publish_rejection(&fixture, &head, &workload.revision);
    seed_decision_boundary(&fixture, &bindings, &workload_card, &head, "reject");

    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let receipt: Value =
        serde_json::from_slice(&fs::read(fixture.run.join("corroboration.json")).unwrap()).unwrap();
    assert_eq!(receipt["selection"]["status"], "ready", "{receipt}");
    let units = receipt["selection"]["units"].as_array().unwrap();
    assert_eq!(units.len(), 1, "{receipt}");
    assert_eq!(units[0]["caseId"], "case-c", "{receipt}");
    assert_eq!(units[0]["owner"], owner, "{receipt}");
    assert_eq!(units[0]["mechanism"], "bounded-output", "{receipt}");
    assert_eq!(units[0]["conditions"], "local-tool-runs", "{receipt}");
    assert!(units[0].get("answer").is_none(), "{receipt}");
    assert!(!receipt.to_string().contains("prior solution"), "{receipt}");
    let report = status_value(&fixture);
    assert_eq!(report["corroboration"]["ready"], true, "{report}");
    assert_eq!(report["corroboration"]["units"][0]["case_id"], "case-c");
}

/// One ready corroboration receipt whose single unit carries identity and
/// replay references only.
fn ready_corroboration_receipt(case_id: &str, tree_sha256: &str) -> Value {
    json!({
        "schema": 1,
        "status": "selected",
        "required_units": 1,
        "selection": {
            "schema": 1,
            "requiredUnits": 1,
            "status": "ready",
            "units": [{
                "owner": "card-prior",
                "caseId": case_id,
                "experiment": "exp-prior",
                "mechanism": "bounded-output",
                "conditions": "local-tool-runs",
                "revision": "rev-prior",
                "treeSha256": tree_sha256,
            }],
            "excluded": [],
        },
        "reason": Value::Null,
    })
}

/// A predeclared policy whose declared scope needs no additional unit.
fn single_unit_policy() -> Value {
    let mut policy = corroboration_policy();
    policy["stopping"]["requiredUnits"] = json!(1);
    policy
}

/// Seed the decision boundary of an adoption whose recorded evaluation
/// consumed one ready corroboration section, and write the exact receipt the
/// decision is bound to. Returns the evaluation path and the receipt.
fn seed_corroborated_adoption_boundary(
    fixture: &Fixture,
    bindings: &Path,
    workload_card: &str,
    revision: &str,
) -> (PathBuf, Value) {
    let evaluation_path =
        seed_decision_boundary(fixture, bindings, workload_card, revision, "adopt");
    let receipt = ready_corroboration_receipt("case-c", &"c".repeat(64));
    let section = harness_core::outcome_report::corroboration_section(&receipt)
        .expect("the fixture receipt is a valid corroboration receipt");
    let mut evaluation: Value =
        serde_json::from_slice(&fs::read(&evaluation_path).unwrap()).unwrap();
    evaluation["corroboration"] = serde_json::to_value(&section).unwrap();
    fs::write(
        &evaluation_path,
        serde_json::to_vec_pretty(&evaluation).unwrap(),
    )
    .unwrap();
    fs::write(
        fixture.run.join("corroboration.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    (evaluation_path, receipt)
}

/// A recorded adoption is bound to the corroboration section its decision
/// consumed: a matching receipt inherits it, while a changed, invalid or
/// missing receipt blocks the inheritance with the exact identity instead of
/// consuming the adoption from a state the decision never saw.
#[test]
fn a_changed_or_missing_corroboration_receipt_cannot_inherit_an_adoption() {
    let fixture = Fixture::new("corroboration-inheritance");
    let head = seed_comparison_spec(&fixture, &corroboration_policy(), "inheritance");
    let workload = harness_core::task_worktree::frozen_copy(
        &fixture.proj,
        &head,
        &fixture.root.join("workload-pre"),
    )
    .unwrap();
    let workload_card = admit_workload_card(&fixture);
    let bindings = seed_bindings(&fixture, &workload, &head);
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let (_, receipt) =
        seed_corroborated_adoption_boundary(&fixture, &bindings, &workload_card, &head);

    // The matching receipt inherits the adoption: the run consumes the
    // recorded decision to this run's own publication scope instead of
    // blocking on it.
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let status = status_value(&fixture);
    assert_eq!(status["phase"], "idle", "{status}");
    assert!(
        !status["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("corroboration"),
        "{status}"
    );

    // A changed selection state cannot inherit the recorded adoption.
    seed_corroborated_adoption_boundary(&fixture, &bindings, &workload_card, &head);
    let mut changed = receipt.clone();
    changed["selection"]["units"][0]["caseId"] = json!("case-substituted");
    fs::write(
        fixture.run.join("corroboration.json"),
        serde_json::to_vec_pretty(&changed).unwrap(),
    )
    .unwrap();
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let status = status_value(&fixture);
    assert_eq!(status["phase"], "blocked", "{status}");
    let condition = status["condition"].as_str().unwrap_or_default();
    assert!(
        condition.contains("does not match the digest")
            && condition.contains("cannot inherit the adoption"),
        "{status}"
    );

    // A missing receipt cannot inherit it either.
    seed_corroborated_adoption_boundary(&fixture, &bindings, &workload_card, &head);
    fs::remove_file(fixture.run.join("corroboration.json")).unwrap();
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let status = status_value(&fixture);
    assert_eq!(status["phase"], "blocked", "{status}");
    assert!(
        status["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("missing corroboration state"),
        "{status}"
    );

    // A receipt that contradicts its own declared contract cannot inherit it
    // either, and restoring the exact recorded receipt lets the same adoption
    // be consumed recoverably.
    seed_corroborated_adoption_boundary(&fixture, &bindings, &workload_card, &head);
    fs::write(
        fixture.run.join("corroboration.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "status": "selected",
            "required_units": 1,
            "selection": Value::Null,
            "reason": Value::Null,
        }))
        .unwrap(),
    )
    .unwrap();
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let status = status_value(&fixture);
    assert_eq!(status["phase"], "blocked", "{status}");
    let condition = status["condition"].as_str().unwrap_or_default();
    assert!(
        condition.contains("not a valid receipt")
            && condition.contains("cannot inherit the adoption"),
        "{status}"
    );

    // The block is a state condition, not a verdict: with the boundary's
    // recorded decision re-established and the exact receipt restored to the
    // file, the same adoption is consumed instead of blocked.
    seed_corroborated_adoption_boundary(&fixture, &bindings, &workload_card, &head);
    fs::write(
        fixture.run.join("corroboration.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let status = status_value(&fixture);
    assert_eq!(status["phase"], "idle", "{status}");
}

/// A run whose declared policy needs no additional unit is unaffected: it
/// requires and writes no corroboration receipt, and a stray receipt beside
/// the run cannot change the consumption of a decision that recorded no
/// corroboration section.
#[test]
fn a_run_without_a_declared_corroboration_requirement_consumes_its_decision_unchanged() {
    let fixture = Fixture::new("corroboration-absent");
    let head = seed_comparison_spec(&fixture, &single_unit_policy(), "absent");
    let workload = harness_core::task_worktree::frozen_copy(
        &fixture.proj,
        &head,
        &fixture.root.join("workload-pre"),
    )
    .unwrap();
    let workload_card = admit_workload_card(&fixture);
    let bindings = seed_bindings(&fixture, &workload, &head);
    let started = fixture.start();
    assert!(started.status.success(), "{}", text(&started));
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    seed_decision_boundary(&fixture, &bindings, &workload_card, &head, "adopt");

    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let status = status_value(&fixture);
    assert_eq!(status["phase"], "idle", "{status}");
    assert!(
        !fixture.run.join("corroboration.json").is_file(),
        "a declared scope without additional units writes no corroboration receipt"
    );

    seed_decision_boundary(&fixture, &bindings, &workload_card, &head, "adopt");
    fs::write(
        fixture.run.join("corroboration.json"),
        serde_json::to_vec_pretty(&ready_corroboration_receipt("case-stray", &"e".repeat(64)))
            .unwrap(),
    )
    .unwrap();
    let resumed = fixture.resume();
    assert!(resumed.status.success(), "{}", text(&resumed));
    let status = status_value(&fixture);
    assert_eq!(status["phase"], "idle", "{status}");
}

// ---------------------------------------------------------------------------
// Installed removal gates and restoration (OpenSpec change task 6.5)
// ---------------------------------------------------------------------------

/// The fixture-owned disposable capability: a synthetic skill package the
/// exercise publishes into an isolated installed home and may remove.
const DISPOSABLE_SKILL: &str = "owned-disposable";
const DISPOSABLE_PROPOSAL: &str = "remove-owned-capability";
const DISPOSABLE_TARGET: &str = "skill-owned-disposable";

/// One isolated installed kit outside this checkout: the installed manager and
/// the homes every child command is confined to. No verb runs the test build.
struct InstalledKit {
    manager: PathBuf,
    codex_home: PathBuf,
    user_home: PathBuf,
}

impl InstalledKit {
    fn command(&self, program: &Path, cwd: &Path) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(cwd)
            .env("CODEX_HOME", &self.codex_home)
            .env("USERPROFILE", &self.user_home)
            .env("HOME", &self.user_home);
        command
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> Output {
        self.command(&self.manager, cwd)
            .args(args)
            .output()
            .unwrap()
    }

    fn feedback(&self, project: &Path, bd: &Path, args: &[&str]) -> Output {
        let mut command = self.command(&self.manager, project);
        command
            .arg("feedback")
            .args(args)
            .arg("--project")
            .arg(project)
            .arg("--bd")
            .arg(bd);
        command.output().unwrap()
    }
}

fn assert_succeeded(label: &str, out: &Output) {
    assert!(out.status.success(), "{label}: {}", text(out));
}

fn assert_exit(label: &str, out: &Output, code: i32) {
    assert_eq!(out.status.code(), Some(code), "{label}: {}", text(out));
}

fn bd_run(bd: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(bd)
        .args(args)
        .current_dir(cwd)
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", "removal-gate-exercise")
        .output()
        .unwrap()
}

/// Writes the staged disposable capability: a minimal valid skill package the
/// installed skill lifecycle can publish and re-publish unchanged.
fn write_disposable_package(root: &Path) {
    for (relative, content) in [
        (
            "SKILL.md",
            "---\nname: owned-disposable\ndescription: Fixture-owned disposable capability for the removal-gate exercise.\n---\n\nSynthetic fixture capability; never a delivered skill.\n",
        ),
        ("payload.txt", "synthetic removal-gate payload\n"),
    ] {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
    }
}

/// Publishes one staged package through the installed skill lifecycle.
fn publish_package(
    installed: &InstalledKit,
    requests: &Path,
    staged: &Path,
    dest: &Path,
    expected_parent: &str,
) -> Output {
    let request = requests.join(format!(
        "publish-{}.json",
        dest.file_name().unwrap().to_string_lossy()
    ));
    fs::write(
        &request,
        serde_json::to_vec_pretty(&json!({
            "staged": staged,
            "dest": dest,
            "expected_parent": expected_parent,
        }))
        .unwrap(),
    )
    .unwrap();
    installed.run(
        requests,
        &["skills", "publish", "--request", request.to_str().unwrap()],
    )
}

/// Every entry under `root` as `relative -> file digest | link target | dir`.
/// Link and junction entries are recorded, never followed, so the delivered
/// skill links into this checkout stay single entries.
fn tree_snapshot(root: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if let Ok(target) = fs::read_link(&path) {
                out.insert(relative, format!("link:{}", target.display()));
            } else if path.is_dir() {
                out.insert(relative, "dir".to_owned());
                walk(root, &path, out);
            } else {
                out.insert(
                    relative,
                    format!("file:{}", hash_bytes(&fs::read(&path).unwrap())),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// The run spec for the isolated controller, declaring the removal treatment.
fn write_installed_run_spec(path: &Path, project: &Path, codex_home: &Path, bd: &Path, card: &str) {
    let document = json!({
        "schema": 1,
        "run": "installed-removal-gate",
        "project": project,
        "codex_home": codex_home,
        "board": {"bd": bd, "project": project},
        "specification": {
            "project": project,
            "change": "add-synthetic",
            "store": Value::Null,
            "planning_root": project,
        },
        "hypothesis_item": card,
        "experiment": {
            "acceptance_artifact": "specs/synthetic/spec.md",
            "acceptance_heading": "#### Scenario: Synthetic case",
            "mechanism": "bounded-output",
            "counterexample": "diagnostics vanish",
            "applicability": "local tool runs",
            "independent_acceptance": "the oracle checker executes",
            "meaningful_effect": "fewer repeated loads",
            "operating_conditions": "cold context",
            "comparison_policy": "matched pairs",
            "stopping_rule": "two repeats",
        },
        "base_revision": git_output(project, &["rev-parse", "HEAD"]),
        "writable_scope": ["crates/one"],
        "runner": Value::Null,
        "local_runner": Value::Null,
        "qualification": Value::Null,
        "publication_scope": ["experiment"],
        "oracle": "outcome-oracle:private-request",
        "removal": {"proposal": DISPOSABLE_PROPOSAL, "target": DISPOSABLE_TARGET},
    });
    fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
}

/// Resolves the current removal authority for the fixture scope through the
/// installed decision owner.
fn removal_check(
    installed: &InstalledKit,
    project: &Path,
    bd: &Path,
    item: &str,
    action: &str,
) -> Output {
    installed.feedback(
        project,
        bd,
        &[
            "removal-check",
            "--item",
            item,
            "--proposal",
            DISPOSABLE_PROPOSAL,
            "--target",
            DISPOSABLE_TARGET,
            "--action",
            action,
        ],
    )
}

/// Applies the publication effect exactly when the installed gate authorizes
/// the exact reviewed proposal: the check is the real installed verb, and the
/// effect is the fixture-owned removal of the capability directory.
fn publish_removal_if_authorized(
    installed: &InstalledKit,
    project: &Path,
    bd: &Path,
    item: &str,
    capability: &Path,
) -> bool {
    let check = removal_check(installed, project, bd, item, "publication");
    if !check.status.success() {
        eprintln!("publication withheld: {}", text(&check).trim());
        return false;
    }
    assert!(
        text(&check).contains("result=authorized"),
        "{}",
        text(&check)
    );
    fs::remove_dir_all(capability).unwrap();
    true
}

struct PathRestore(Option<std::ffi::OsString>);
impl PathRestore {
    fn capture() -> Self {
        Self(std::env::var_os("PATH"))
    }
}
impl Drop for PathRestore {
    fn drop(&mut self) {
        unsafe {
            match &self.0 {
                Some(path) => std::env::set_var("PATH", path),
                None => std::env::remove_var("PATH"),
            }
        }
    }
}

/// OpenSpec change task 6.5: an isolated kit is installed outside this
/// checkout from a genuine build; a fixture-owned disposable capability is
/// published into that installed home; real board, controller and decision
/// owner verbs gate its removal; and restoration returns it. Proves wiring and
/// authority boundaries only: it cannot replace task 6.3 or justify retiring
/// an actual capability. No model call and no mutation of the live user home
/// or this checkout's run state.
#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE, installed OpenSpec and the owner PowerShell; isolated homes only"]
fn installed_removal_gates_and_restoration_outside_checkout() {
    let source = plain_path(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let upstream = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    assert!(upstream.is_file(), "{}", upstream.display());

    let root = tempfile::Builder::new()
        .prefix("removal-gate-")
        .tempdir()
        .unwrap();
    let root_path = plain_path(root.path());
    eprintln!("isolated removal-gate evidence: {}", root_path.display());
    let build = root_path.join("build");
    let codex_home = root_path.join("codex");
    let user_home = root_path.join("user");
    let workspace = root_path.join("outside");
    let project = root_path.join("project");
    let run = root_path.join("runs/removal-gate");
    let requests = root_path.join("requests");
    let staged = root_path.join("staged").join(DISPOSABLE_SKILL);
    for dir in [
        &build,
        &codex_home,
        &user_home,
        &workspace,
        &project,
        &requests,
    ] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::create_dir_all(run.parent().unwrap()).unwrap();

    // The genuine build of this checkout: this test's own build directory
    // already holds every workspace binary the isolated install records.
    let compiled = Path::new(env!("CARGO_BIN_EXE_codex-harness"))
        .parent()
        .unwrap()
        .to_path_buf();
    for name in BINARIES {
        let from = compiled.join(name);
        assert!(
            from.is_file(),
            "workspace binary {name} is missing at {}; build the workspace before this exercise",
            from.display()
        );
        fs::copy(&from, build.join(name)).unwrap();
    }
    let record = build_identity::BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.clone(),
        source: build_identity::source_identity(&source).unwrap(),
        rustc: "isolated removal-gate install".into(),
        cargo: "isolated removal-gate install".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries: BINARIES
            .iter()
            .map(|name| {
                (
                    name.to_string(),
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

    // Unrelated state a real home carries, plus a foreign skill the kit must
    // preserve. Seeded before the install so the lifecycle preservation is
    // part of the exercise too.
    fs::write(
        codex_home.join("config.toml"),
        b"model = 'unrelated-kept'\n",
    )
    .unwrap();
    fs::write(codex_home.join("auth.json"), b"unrelated-auth").unwrap();
    let foreign = user_home.join(".agents/skills/foreign/SKILL.md");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    fs::write(&foreign, b"---\nname: foreign\ndescription: Kept.\n---\n").unwrap();

    let _path = PathRestore::capture();
    let request = core_install::Request {
        source: source.clone(),
        build: build.clone(),
        codex_home: codex_home.clone(),
        user_home: user_home.clone(),
        dependency_user_home: user_home.clone(),
        upstream: Some(upstream.clone()),
        timeout: Duration::from_secs(45),
        path_scope: Some(PathScope::Process),
    };
    let preview = core_install::connect(&request, true).unwrap();
    assert_eq!(preview.status, "preview");
    let connected = core_install::connect(&request, false).unwrap();
    assert_eq!(connected.status, "connected");
    assert!(connected.runtime.unwrap().passed);
    let installed = InstalledKit {
        manager: codex_home.join("harness/bin/codex-harness.exe"),
        codex_home: codex_home.clone(),
        user_home: user_home.clone(),
    };
    assert!(
        installed.manager.is_file(),
        "the installed manager is present"
    );
    let version = installed.run(&workspace, &["--version"]);
    assert_succeeded("installed manager version", &version);
    assert!(
        text(&version).contains("codex-harness"),
        "{}",
        text(&version)
    );
    let config_text = fs::read_to_string(codex_home.join("config.toml")).unwrap();
    assert!(
        config_text.contains("model = 'unrelated-kept'"),
        "unrelated configuration survives the install: {config_text}"
    );
    assert_eq!(
        fs::read(codex_home.join("auth.json")).unwrap(),
        b"unrelated-auth",
        "credentials are unrelated configuration"
    );
    assert_eq!(
        fs::read_to_string(&foreign).unwrap(),
        "---\nname: foreign\ndescription: Kept.\n---\n",
        "foreign skills are preserved"
    );

    // The real board component: the pinned bd is acquired through the board
    // lifecycle into the isolated home. A warm package from the machine's own
    // installation is copied in first so the pinned acquisition does not
    // re-download what is already verified locally.
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        let packages = PathBuf::from(home).join("harness/board/packages");
        if let Ok(entries) = fs::read_dir(&packages) {
            for entry in entries.flatten() {
                let cached = entry.path().join("bd.exe");
                if cached.is_file() {
                    let package = codex_home
                        .join("harness/board/packages")
                        .join(entry.file_name());
                    fs::create_dir_all(&package).unwrap();
                    fs::copy(&cached, package.join("bd.exe")).unwrap();
                }
            }
        }
    }
    let board = board_lifecycle::Request {
        source: source.clone(),
        codex_home: codex_home.clone(),
        user_home: user_home.clone(),
        preview: true,
    };
    let board_preview = board_lifecycle::install(&board).unwrap();
    assert_eq!(board_preview.status, "Preview board Install");
    let board_report = board_lifecycle::install(&board_lifecycle::Request {
        preview: false,
        ..board
    })
    .unwrap();
    assert_eq!(board_report.status, "Board connected");
    let bd = codex_home.join("harness/bin/bd.exe");
    assert!(bd.is_file(), "the installed board is present");
    let bd_version =
        String::from_utf8_lossy(&bd_run(&bd, &workspace, &["--version"]).stdout).into_owned();
    assert!(bd_version.contains("1.3.0"), "{bd_version}");

    // The isolated project outside this checkout: a real git repository with
    // its own board and OpenSpec workspace.
    git(
        &root_path,
        &[
            "init",
            "-q",
            "--initial-branch=main",
            project.to_str().unwrap(),
        ],
    );
    git(&project, &["config", "user.email", "fixture@example.test"]);
    git(&project, &["config", "user.name", "Fixture"]);
    fs::create_dir_all(project.join("crates/one/src")).unwrap();
    fs::write(project.join("crates/one/src/lib.rs"), "// synthetic\n").unwrap();
    fs::create_dir_all(project.join("global")).unwrap();
    fs::write(
        project.join("global/orchestration.toml"),
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\n",
    )
    .unwrap();
    fs::write(project.join("README.md"), "synthetic\n").unwrap();
    git(&project, &["add", "."]);
    git(&project, &["commit", "-qm", "seed"]);
    let init = bd_run(
        &bd,
        &project,
        &[
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "rgx",
        ],
    );
    assert!(init.status.success(), "bd init: {}", text(&init));
    let openspec_init = openspec(
        &project,
        &["init", "--tools", "none", "--no-animation", "--force"],
    );
    assert!(
        openspec_init.status.success(),
        "openspec init: {}",
        text(&openspec_init)
    );
    let created = openspec(
        &project,
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
    write_change(
        &project,
        "## Why\n\nSynthetic.\n",
        "## Context\n\nSynthetic.\n",
        "## 1. Work\n\n- [ ] 1.1 Do the synthetic thing.\n",
    );

    // The hypothesis card lives on the real board; its admission consumes the
    // limits from the installed kit record (no --source was passed).
    let admit = installed.feedback(
        &project,
        &bd,
        &[
            "hypothesis-admit",
            "--mechanism",
            "bounded-output",
            "--conditions",
            "installed-removal-gate",
            "--observation",
            "observation:removal-gate-exercise",
            "--predicted",
            "the removal gate holds without implicit consent",
            "--counterexample",
            "consent is inferred from a favorable benefit verdict",
            "--acceptance",
            "the independent oracle passes",
            "--spec",
            "openspec/changes/add-synthetic",
            "--basis",
            "basis:removal-gate-exercise",
        ],
    );
    assert_succeeded("hypothesis-admit", &admit);
    let admit_text = text(&admit);
    assert!(
        admit_text.contains("configured(installed kit"),
        "the installed kit record supplies the limits: {admit_text}"
    );
    let card = admit_text
        .strip_prefix("hypothesis ")
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default()
        .to_owned();
    assert!(!card.is_empty(), "admission named no card: {admit_text}");

    // The fixture-owned disposable capability enters the installed home
    // through the real skill lifecycle; the staged package is retained
    // outside the installation as the recorded restoration route.
    write_disposable_package(&staged);
    let skills_root = user_home.join(".agents/skills");
    let capability = skills_root.join(DISPOSABLE_SKILL);
    let published = publish_package(&installed, &requests, &staged, &capability, "");
    assert_succeeded("publish disposable capability", &published);
    assert!(
        capability.join("SKILL.md").is_file() && capability.join("payload.txt").is_file(),
        "the capability is installed"
    );
    let baseline = tree_snapshot(&skills_root);
    let disposable_keys: Vec<String> = baseline
        .keys()
        .filter(|key| {
            key.as_str() == DISPOSABLE_SKILL || key.starts_with(&format!("{DISPOSABLE_SKILL}/"))
        })
        .cloned()
        .collect();
    assert_eq!(disposable_keys.len(), 3, "{disposable_keys:?}");
    let foreign_bytes = fs::read(&foreign).unwrap();
    let config_bytes = fs::read(codex_home.join("config.toml")).unwrap();
    let auth_bytes = fs::read(codex_home.join("auth.json")).unwrap();

    // The reviewable proposal is recorded by the real decision owner before
    // the run starts, so the controller freezes its reviewed digest.
    let propose = installed.feedback(
        &project,
        &bd,
        &[
            "removal-propose",
            "--item",
            &card,
            "--proposal",
            DISPOSABLE_PROPOSAL,
            "--target",
            DISPOSABLE_TARGET,
            "--evidence",
            "evidence:removal-gate-exercise",
            "--loss",
            DISPOSABLE_TARGET,
            "--preview",
            "preview:isolated-disposable",
            "--detail",
            "consumer list: none known beyond this fixture; restoration route: the retained staged package is re-published through the installed skill lifecycle",
        ],
    );
    assert_succeeded("removal-propose", &propose);
    assert!(
        text(&propose).contains("record=written"),
        "{}",
        text(&propose)
    );

    let spec = root_path.join("run-spec.json");
    write_installed_run_spec(&spec, &project, &codex_home, &bd, &card);
    let run_arg = run.to_str().unwrap().to_owned();
    let spec_arg = spec.to_str().unwrap().to_owned();
    let started = installed.run(
        &workspace,
        &["improve", "start", "--run", &run_arg, "--spec", &spec_arg],
    );
    assert_succeeded("improve start", &started);
    let status = installed.run(
        &workspace,
        &["improve", "status", "--run", &run_arg, "--json"],
    );
    assert_succeeded("improve status", &status);
    let report: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(report["removal"]["declared"], true, "{report}");
    assert!(
        report["removal"]["gate"]
            .as_str()
            .is_some_and(|gate| gate.contains("pending")),
        "{report}"
    );
    let select = installed.run(
        &workspace,
        &[
            "improve",
            "select",
            "--run",
            &run_arg,
            "--variant",
            "candidate",
        ],
    );
    assert_exit("select without a decision", &select, 2);
    assert!(
        text(&select).contains("removal approval is pending"),
        "{}",
        text(&select)
    );
    assert!(
        text(&select).contains("no removal decision"),
        "{}",
        text(&select)
    );
    assert!(
        capability.join("SKILL.md").is_file(),
        "an undecided proposal never removes the capability"
    );

    // The test user refuses: the refusal preserves the capability, blocks the
    // controller and authorizes nothing.
    let refuse = installed.feedback(
        &project,
        &bd,
        &[
            "removal-decide",
            "--item",
            &card,
            "--decision",
            "refuse",
            "--proposal",
            DISPOSABLE_PROPOSAL,
            "--target",
            DISPOSABLE_TARGET,
            "--basis",
            "test-user:refusal-1",
        ],
    );
    assert_succeeded("removal-decide refuse", &refuse);
    assert!(
        text(&refuse).contains("decision=refuse"),
        "{}",
        text(&refuse)
    );
    let select = installed.run(
        &workspace,
        &[
            "improve",
            "select",
            "--run",
            &run_arg,
            "--variant",
            "candidate",
        ],
    );
    assert_exit("select after refusal", &select, 2);
    assert!(
        text(&select).contains("the user declined"),
        "{}",
        text(&select)
    );
    let refused = removal_check(&installed, &project, &bd, &card, "experiment");
    assert_exit("removal-check after refusal", &refused, 1);
    assert!(
        text(&refused).contains("result=refused"),
        "{}",
        text(&refused)
    );
    assert_eq!(
        tree_snapshot(&skills_root),
        baseline,
        "a refusal preserves the capability and every unrelated entry"
    );

    // A fresh informed approval covering only the isolated experiment clears
    // the controller's removal gate; the run then stops at the pending
    // runtime preparation instead of starting any model work.
    let approve_experiment = installed.feedback(
        &project,
        &bd,
        &[
            "removal-decide",
            "--item",
            &card,
            "--decision",
            "approve",
            "--proposal",
            DISPOSABLE_PROPOSAL,
            "--target",
            DISPOSABLE_TARGET,
            "--actions",
            "experiment",
            "--loss",
            DISPOSABLE_TARGET,
            "--basis",
            "test-user:approval-experiment-1",
        ],
    );
    assert_succeeded("removal-decide approve experiment", &approve_experiment);
    assert!(
        text(&approve_experiment).contains("actions=experiment"),
        "{}",
        text(&approve_experiment)
    );
    let select = installed.run(
        &workspace,
        &[
            "improve",
            "select",
            "--run",
            &run_arg,
            "--variant",
            "candidate",
        ],
    );
    assert_exit("select with the experiment approval", &select, 2);
    assert!(
        text(&select).contains("runtime preparation"),
        "{}",
        text(&select)
    );
    let experiment = removal_check(&installed, &project, &bd, &card, "experiment");
    assert_succeeded("removal-check experiment", &experiment);
    assert!(
        text(&experiment).contains("result=authorized"),
        "{}",
        text(&experiment)
    );
    let integration = removal_check(&installed, &project, &bd, &card, "integration");
    assert_exit("integration is not covered", &integration, 1);
    assert!(
        text(&integration).contains("integration is not covered"),
        "{}",
        text(&integration)
    );
    let publication = removal_check(&installed, &project, &bd, &card, "publication");
    assert_exit("publication is not covered", &publication, 1);
    assert!(
        text(&publication).contains("publication is not covered"),
        "{}",
        text(&publication)
    );
    let applied = publish_removal_if_authorized(&installed, &project, &bd, &card, &capability);
    assert!(
        !applied,
        "publication without its own coverage must not be applied"
    );
    assert_eq!(
        tree_snapshot(&skills_root),
        baseline,
        "experiment-only consent leaves the live installation intact"
    );

    // A fresh approval that expressly covers publication authorizes the live
    // removal, but not the controller's experiment stage.
    let approve_publication = installed.feedback(
        &project,
        &bd,
        &[
            "removal-decide",
            "--item",
            &card,
            "--decision",
            "approve",
            "--proposal",
            DISPOSABLE_PROPOSAL,
            "--target",
            DISPOSABLE_TARGET,
            "--actions",
            "publication",
            "--loss",
            DISPOSABLE_TARGET,
            "--basis",
            "test-user:approval-publication-1",
        ],
    );
    assert_succeeded("removal-decide approve publication", &approve_publication);
    assert!(
        text(&approve_publication).contains("actions=publication"),
        "{}",
        text(&approve_publication)
    );
    let select = installed.run(
        &workspace,
        &[
            "improve",
            "select",
            "--run",
            &run_arg,
            "--variant",
            "candidate",
        ],
    );
    assert_exit("select under the publication approval", &select, 2);
    assert!(
        text(&select).contains("experiment is not covered"),
        "{}",
        text(&select)
    );
    let applied = publish_removal_if_authorized(&installed, &project, &bd, &card, &capability);
    assert!(applied, "the covered publication authorization is consumed");
    assert!(
        !capability.exists(),
        "the authorized removal removed the capability"
    );

    // Only the intended capability is gone: every remaining entry is
    // byte-identical to the baseline, and the installation still runs.
    let after_removal = tree_snapshot(&skills_root);
    let missing: Vec<String> = baseline
        .keys()
        .filter(|key| !after_removal.contains_key(*key))
        .cloned()
        .collect();
    assert_eq!(
        missing, disposable_keys,
        "covered publication removes only the intended capability"
    );
    for (key, value) in &after_removal {
        assert_eq!(
            baseline.get(key),
            Some(value),
            "unrelated entry {key} changed"
        );
    }
    assert_eq!(fs::read(&foreign).unwrap(), foreign_bytes);
    assert_eq!(
        fs::read(codex_home.join("config.toml")).unwrap(),
        config_bytes
    );
    assert_eq!(fs::read(codex_home.join("auth.json")).unwrap(), auth_bytes);
    let version = installed.run(&workspace, &["--version"]);
    assert_succeeded("the installation still runs after the removal", &version);

    // Restoration follows the recorded route: the retained staged package is
    // re-published through the installed skill lifecycle. Recovering the
    // owned experimental capability is not a removal effect.
    let restored = publish_package(&installed, &requests, &staged, &capability, "");
    assert_succeeded("restore the capability", &restored);
    assert_eq!(
        tree_snapshot(&skills_root),
        baseline,
        "restoration returns the exact capability and touches nothing else"
    );
    assert_eq!(fs::read(&foreign).unwrap(), foreign_bytes);
    assert_eq!(
        fs::read(codex_home.join("config.toml")).unwrap(),
        config_bytes
    );
    assert_eq!(fs::read(codex_home.join("auth.json")).unwrap(), auth_bytes);

    // A changed reviewed proposal is a new version: the recorded approval
    // receipt is bound to the older content, so neither the decision owner
    // nor the run's frozen digest authorizes any further effect.
    let changed = installed.feedback(
        &project,
        &bd,
        &[
            "removal-propose",
            "--item",
            &card,
            "--proposal",
            DISPOSABLE_PROPOSAL,
            "--target",
            DISPOSABLE_TARGET,
            "--evidence",
            "evidence:removal-gate-exercise",
            "--loss",
            DISPOSABLE_TARGET,
            "--preview",
            "preview:isolated-disposable",
            "--detail",
            "consumer list: one indirect consumer recorded after the approval; restoration route: the retained staged package is re-published through the installed skill lifecycle",
        ],
    );
    assert_succeeded("removal-propose changed version", &changed);
    assert!(
        text(&changed).contains("record=written"),
        "{}",
        text(&changed)
    );
    let stale = removal_check(&installed, &project, &bd, &card, "publication");
    assert_exit("stale approval receipt", &stale, 1);
    assert!(
        text(&stale).contains("changed after the latest decision"),
        "{}",
        text(&stale)
    );
    assert!(
        text(&stale).contains("fresh decision is required"),
        "{}",
        text(&stale)
    );
    let select = installed.run(
        &workspace,
        &[
            "improve",
            "select",
            "--run",
            &run_arg,
            "--variant",
            "candidate",
        ],
    );
    assert_exit("select with a stale frozen digest", &select, 2);
    assert!(
        text(&select).contains("changed after the frozen approval"),
        "{}",
        text(&select)
    );
    let applied = publish_removal_if_authorized(&installed, &project, &bd, &card, &capability);
    assert!(
        !applied,
        "a stale approval receipt authorizes no further effect"
    );
    assert_eq!(tree_snapshot(&skills_root), baseline);

    // An explicit withdrawal of the current proposal version authorizes
    // nothing either, and the capability stays exactly as restored.
    let withdraw = installed.feedback(
        &project,
        &bd,
        &[
            "removal-decide",
            "--item",
            &card,
            "--decision",
            "withdraw",
            "--proposal",
            DISPOSABLE_PROPOSAL,
            "--target",
            DISPOSABLE_TARGET,
            "--basis",
            "test-user:withdrawal-1",
        ],
    );
    assert_succeeded("removal-decide withdraw", &withdraw);
    assert!(
        text(&withdraw).contains("decision=withdraw"),
        "{}",
        text(&withdraw)
    );
    let withdrawn = removal_check(&installed, &project, &bd, &card, "publication");
    assert_exit("withdrawn approval", &withdrawn, 1);
    assert!(
        text(&withdrawn).contains("result=withdrawn"),
        "{}",
        text(&withdrawn)
    );
    let applied = publish_removal_if_authorized(&installed, &project, &bd, &card, &capability);
    assert!(!applied, "a withdrawal authorizes no removal effect");
    assert_eq!(tree_snapshot(&skills_root), baseline);
    eprintln!(
        "installed removal-gate exercise complete: refusal preserved, experiment-only consent left the live installation intact, covered publication removed only {DISPOSABLE_SKILL}, restoration returned it, and stale receipts authorized nothing"
    );
}
