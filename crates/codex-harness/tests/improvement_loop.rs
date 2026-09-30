//! Native `codex-harness improve` controller checks over synthetic owned
//! inputs: a real `bd` board, a real OpenSpec workspace and private run state.
//! Every dispatch gate is exercised model-free; no check here contacts a
//! model, a provider or a subscription.
#![cfg(windows)]

use harness_core::build_identity::{BINARIES, hash_bytes};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

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
    json!({
        "id": id,
        "role": role,
        "owner": format!("loop-fixture-{role}-1"),
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
    fs::write(
        path,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "launcher": "C:\\fixture\\codex.exe",
            "profile": "ds",
            "mode": "tui",
            "visible": true,
            "host": "windows-terminal-tab",
            "observation": {
                "schema": 1,
                "coverage": "native",
                "reason": Value::Null,
                "state": state,
                "session": Value::Null,
                "previousSession": Value::Null,
                "exitCode": exit_code,
                "events": 0,
                "messages": 0,
                "toolCalls": 0,
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

    // A refusal is not a measurement failure and the same request is not
    // repeated without a new basis.
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
}

#[test]
fn stop_and_resume_recover_boundaries_and_never_replay_unknown_attempts() {
    let fixture = Fixture::new("recovery");
    fixture.write_spec(&[], None);
    let out = fixture.start();
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
    seed_receipt(&completed, "completed", Some(0));
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
