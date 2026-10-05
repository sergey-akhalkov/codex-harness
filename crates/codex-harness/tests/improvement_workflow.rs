//! Native `codex-harness improve` planning/implementation workflow checks
//! over synthetic owned inputs: a real `bd` board, a real OpenSpec workspace,
//! real Git worktrees and private run state. The model conversations are
//! simulated through the same durable seam the controller recovery uses: a
//! dispatcher receipt with a terminal state is seeded and the controller
//! settles and consumes it on `resume`. No check here contacts a model, a
//! provider or a subscription.
#![cfg(windows)]

use harness_core::board_hypothesis::{
    self, BoundedImplementation, HypothesisRole, ImplementationDraft,
};
use harness_core::build_identity::hash_bytes;
use harness_core::improvement_loop::{AttemptRole, dispatch_owner};
use harness_core::improvement_policy::{
    EffectPath, ExperimentMethod, ExperimentSelection, experiment_selection_clause,
};
use serde_json::{Value, json};
use std::{
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

/// One owned synthetic project: git checkout with a committed OpenSpec change,
/// a bd board with one admitted hypothesis card, a declared runner profile
/// without a launcher (so model dispatch is visibly blocked) and a run spec.
struct Fixture {
    _root: tempfile::TempDir,
    root: PathBuf,
    proj: PathBuf,
    home: PathBuf,
    run: PathBuf,
    spec: PathBuf,
    bd: PathBuf,
    card: String,
    change: String,
}

impl Fixture {
    fn new(name: &str) -> Self {
        Self::new_with_change(name, "add-synthetic")
    }

    /// One owned synthetic project whose committed OpenSpec change is named
    /// `change`; the admitted card references that same change.
    fn new_with_change(name: &str, change: &str) -> Self {
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
        let init = openspec(
            &proj,
            &["init", "--tools", "none", "--no-animation", "--force"],
        );
        assert!(init.status.success(), "openspec init: {}", text(&init));
        let created = openspec(
            &proj,
            &["new", "change", change, "--schema", "spec-driven", "--json"],
        );
        assert!(created.status.success(), "openspec new: {}", text(&created));
        write_change(
            &proj,
            change,
            "## Why\n\nSynthetic.\n",
            "## Context\n\nSynthetic.\n",
            "## 1. Work\n\n- [ ] 1.1 Do the synthetic thing.\n",
        );
        // The frozen base must contain the candidate's OpenSpec workspace:
        // the candidate worktree is allocated from this exact commit.
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
                "bdcw",
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

        let mut fixture = Self {
            _root: cleanup,
            root,
            proj,
            home,
            run,
            spec: PathBuf::new(),
            bd,
            card: String::new(),
            change: change.to_owned(),
        };
        fixture.card = fixture.admit();
        fixture.spec = fixture.root.join(format!("run-spec-{name}.json"));
        fixture.write_spec(&[]);
        fixture
    }

    fn admit(&self) -> String {
        self.admit_change(&self.change)
    }

    /// One admitted hypothesis card naming a specific OpenSpec change, as
    /// grounded intake admits a new card for a proposed change.
    fn admit_change(&self, change: &str) -> String {
        let spec = format!("openspec/changes/{change}");
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
            &spec,
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

    fn write_spec(&self, replacements: &[(&str, Value)]) {
        self.write_run_spec("workflow-fixture", &self.spec, replacements);
    }

    /// Writes one run spec at an explicit path and run identity, so a single
    /// fixture project and board can back several declared configurations.
    fn write_run_spec(&self, run: &str, path: &Path, replacements: &[(&str, Value)]) {
        let mut document = json!({
            "schema": 1,
            "run": run,
            "project": self.proj,
            "codex_home": self.home,
            "board": {"bd": self.bd, "project": self.proj},
            "specification": {
                "project": self.proj,
                "change": self.change,
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
            "runner": {
                "profile": "ds",
                "model": "deepseek-flash",
                "model_provider": "deepseek",
                "reasoning_effort": "max",
            },
            "local_runner": Value::Null,
            "qualification": Value::Null,
            "evidence_root": Value::Null,
            "publication_scope": ["experiment"],
            "oracle": "outcome-oracle:private-request",
            "removal": Value::Null,
        });
        let object = document.as_object_mut().unwrap();
        for (key, value) in replacements {
            object.insert((*key).to_owned(), value.clone());
        }
        fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    }

    fn improve(&self, args: &[&str]) -> Output {
        let mut command = Command::new(manager());
        command.arg("improve").args(args);
        command.output().expect("improve runs")
    }

    fn start(&self) -> Output {
        // These cases recover explicit workflow boundaries step by step: they
        // are explicit single-step callers, while `start_continuous` declares
        // the CLI default for a supervised loop.
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

    fn start_continuous(&self) -> Output {
        self.improve(&[
            "start",
            "--run",
            self.run.to_str().unwrap(),
            "--spec",
            self.spec.to_str().unwrap(),
            "--supervision",
            "continuous",
        ])
    }

    fn resume(&self) -> Output {
        self.improve(&["resume", "--run", self.run.to_str().unwrap()])
    }

    /// Runs the native model-free structured-assignment check: it loads one
    /// generated assignment through the same validator a dispatch uses and
    /// renders the exact brief without claiming, writing or launching.
    fn assignment_check(&self, slot: u32, assignment: &Path) -> Output {
        let mut command = Command::new(manager());
        command
            .arg("executor")
            .arg("assignment")
            .arg("--source")
            .arg(&self.proj)
            .args(["--slot", &slot.to_string(), "--assignment"])
            .arg(assignment);
        command.output().expect("executor assignment check runs")
    }

    fn status_json(&self) -> Value {
        self.run_status(&self.run)
    }

    /// The `status --json` report of one explicitly named run directory.
    fn run_status(&self, run: &Path) -> Value {
        let status = self.improve(&["status", "--run", run.to_str().unwrap(), "--json"]);
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

    /// The candidate allocation the controller recorded for one hypothesis.
    fn candidate_worktree(&self, hypothesis: &str) -> PathBuf {
        candidate_area(&self.run).join(hypothesis)
    }

    /// The pre-fix allocation geometry: a candidate worktree nested inside the
    /// protected run state, as an older controller recorded it.
    fn legacy_candidate_worktree(&self, hypothesis: &str) -> PathBuf {
        self.run.join("candidates").join(hypothesis)
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

    /// The run's exact frozen base revision, as the declared spec records it.
    fn base_revision(&self) -> String {
        let spec: Value = serde_json::from_slice(&fs::read(&self.spec).unwrap()).unwrap();
        spec["base_revision"]
            .as_str()
            .expect("the run spec records its frozen base")
            .to_owned()
    }

    /// The next owner-assigned candidate location: the controller uses it when
    /// the natural location already holds preserved work.
    fn replacement_candidate_worktree(&self, hypothesis: &str) -> PathBuf {
        candidate_area(&self.run).join(format!("{hypothesis}-2"))
    }
}

/// The run's own candidate area: a sibling of the run root. The run root holds
/// only protected run state, so no candidate worktree nests inside it.
fn candidate_area(run: &Path) -> PathBuf {
    let name = run
        .file_name()
        .expect("the fixture run directory is named")
        .to_string_lossy()
        .into_owned();
    run.parent()
        .expect("the fixture run directory has a parent")
        .join(format!("{name}-candidates"))
}

fn write_change(proj: &Path, name: &str, proposal: &str, design: &str, tasks: &str) {
    let change = proj.join("openspec/changes").join(name);
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

/// One dispatch receipt shaped exactly as the native dispatcher writes it.
fn seed_bound_receipt(
    path: &Path,
    owner: &str,
    generation: &str,
    state: &str,
    exit_code: Option<i32>,
) {
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

/// One attempt record with the frozen accepted dispatch generation.
#[allow(clippy::too_many_arguments)]
fn attempt_json(
    id: &str,
    role: &str,
    owner: &str,
    generation: &str,
    receipt: &Path,
    result: Option<&Path>,
    checkout: Option<&Path>,
    state: &str,
) -> Value {
    json!({
        "id": id,
        "role": role,
        "owner": owner,
        "binding": {
            "slot": 1,
            "owner": owner,
            "generation": generation,
            "receipt": receipt.display().to_string(),
            "session": Value::Null,
            "host": Value::Null,
        },
        "retained": Value::Null,
        "title": format!("CEx (ds) - {id}"),
        "profile": "ds",
        "model": Value::Null,
        "model_provider": Value::Null,
        "reasoning_effort": Value::Null,
        "checkout": checkout.map(|path| path.display().to_string()),
        "assignment": Value::Null,
        "receipt": receipt.display().to_string(),
        "result": result.map(|path| path.display().to_string()),
        "detail": Value::Null,
        "state": state,
        "reason": Value::Null,
        "reuse_refused": Value::Null,
        "started_ms": 1,
        "updated_ms": 1,
    })
}

fn push_attempt(fixture: &Fixture, attempt: Value) {
    let id = attempt["id"].as_str().unwrap().to_owned();
    let role = attempt["role"].as_str().unwrap().to_owned();
    let mut cursor = fixture.cursor();
    link_stage_attempt(&mut cursor, &role, &id);
    cursor["attempts"].as_array_mut().unwrap().push(attempt);
    fixture.write_cursor(&cursor);
}

/// Replaces the attempt with the given id, as the dispatcher's own record
/// would look after a real conversation completed in its pooled slot.
fn replace_attempt(fixture: &Fixture, attempt: Value) {
    let mut cursor = fixture.cursor();
    let id = attempt["id"].as_str().unwrap().to_owned();
    let role = attempt["role"].as_str().unwrap().to_owned();
    link_stage_attempt(&mut cursor, &role, &id);
    let attempts = cursor["attempts"].as_array_mut().unwrap();
    let existing = attempts
        .iter_mut()
        .find(|existing| existing["id"] == json!(id));
    match existing {
        Some(existing) => *existing = attempt,
        None => attempts.push(attempt),
    }
    fixture.write_cursor(&cursor);
}

fn link_stage_attempt(cursor: &mut Value, role: &str, id: &str) {
    let linked = match role {
        "planner" => Some("planner_attempt"),
        "implementer" => Some("implementer_attempt"),
        _ => None,
    };
    if let Some(linked) = linked
        && !cursor["candidate"].is_null()
    {
        cursor["candidate"][linked] = json!(id);
    }
}

/// A launcher-shaped file: the visibility gate requires an installed launcher
/// to exist; no check here ever executes it.
fn fake_launcher(fixture: &Fixture) {
    let launcher = fixture.home.join("harness/bin/codex.exe");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    fs::write(&launcher, "fixture launcher").unwrap();
}

/// Writes one retained investigator terminal message beside the run and seeds
/// the completed investigator attempt that produced it.
fn seed_investigator_message(fixture: &Fixture, message: &[u8]) -> PathBuf {
    let result = fixture.run.join("investigator-result.json");
    fs::write(&result, message).unwrap();
    let receipt = fixture.run.join("investigator-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-investigator-1",
        "gen-1",
        "completed",
        Some(0),
    );
    push_attempt(
        fixture,
        attempt_json(
            "investigator-1",
            "investigator",
            "workflow-fixture-investigator-1",
            "gen-1",
            &receipt,
            Some(&result),
            None,
            "started",
        ),
    );
    result
}

/// Writes one retained investigator report beside the run and seeds the
/// completed investigator attempt that produced it.
fn seed_investigator_report(fixture: &Fixture, report: &Value) -> PathBuf {
    seed_investigator_message(fixture, &serde_json::to_vec_pretty(report).unwrap())
}

/// Writes one retained investigator terminal message for a specific round
/// beside the run and seeds the started attempt that produced it; the next
/// resume settles the seeded receipt, retains the message and consumes it
/// exactly like a real completed round. Unlike `seed_investigator_message`
/// this replaces an existing attempt for the same round, so a controller
/// re-dispatch and its later completion can be simulated in sequence.
fn seed_investigator_round(fixture: &Fixture, ordinal: u32, message: &[u8]) -> PathBuf {
    let result = fixture
        .run
        .join(format!("investigator-result-{ordinal}.json"));
    fs::write(&result, message).unwrap();
    let receipt = fixture
        .run
        .join(format!("investigator-receipt-{ordinal}.json"));
    seed_bound_receipt(
        &receipt,
        &format!("workflow-fixture-investigator-{ordinal}"),
        "gen-1",
        "completed",
        Some(0),
    );
    replace_attempt(
        fixture,
        attempt_json(
            &format!("investigator-{ordinal}"),
            "investigator",
            &format!("workflow-fixture-investigator-{ordinal}"),
            "gen-1",
            &receipt,
            Some(&result),
            None,
            "started",
        ),
    );
    result
}

/// One valid experiment-selection declaration for the report fixtures: a
/// local build/output treatment measured through a short real operation. The
/// report contract requires it for every treatment that selects an
/// experiment.
fn selection_json() -> Value {
    json!({
        "method": "real-operation",
        "claim": "local-operation",
        "outcome": "the declared outcome measured through the real operation",
        "rationale": "the chosen unit exercises the claimed mechanism",
        "controls": "frozen inputs and the accepted baseline conditions",
        "projection": "one bounded local cycle with the retention cost staying bounded",
        "baseline": "the accepted revision, excluding the candidate edit",
        "stopping": "stop after the declared attempts and escalate only for a named missing observation",
    })
}

/// One anchored report: the proposal matches the fixture's admitted card, so
/// grounded intake reuses that card as this run's candidate.
fn anchored_report(observation: &str) -> Value {
    json!({
        "schema": 1,
        "candidates": [{
            "mechanism": "bounded-output",
            "conditions": "local-tool-runs",
            "observation": observation,
            "predicted": "less repeated context loading",
            "counterexample": "diagnostics vanish on failure",
            "acceptance": "the independent oracle passes",
            "alternatives": "no change, reuse of the existing reader, simplification and subtraction leave the measured burden in place",
            "spec": "openspec/changes/add-synthetic",
            "basis": observation,
            "treatment": "addition",
            "evidence": [{"locator": observation, "kind": "observed"}],
            "next_check": null,
            "selection": selection_json(),
        }],
        "idle_reason": null,
    })
}

/// One report-shaped payload whose evidence entries are strings instead of
/// the schema's locator/kind objects: real model format variance that the
/// strict schema-1 contract refuses as unreadable output.
fn malformed_report_payload(observation: &str) -> Vec<u8> {
    let mut report = anchored_report(observation);
    report["candidates"][0]["evidence"] = json!([observation]);
    serde_json::to_vec_pretty(&report).unwrap()
}

fn write_evidence_root(fixture: &Fixture) -> (PathBuf, String) {
    let root = fixture.root.join("evidence");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("observation.txt"), "retained observation\n").unwrap();
    (root, "file:observation.txt".to_owned())
}

/// A detached slot worktree of the candidate's repository, exactly like the
/// dispatcher's pooled checkout: the conversation works there and the
/// controller validates and advances the owned candidate branch from it.
fn slot_worktree(source: &Path, path: &Path, base: &str) {
    git(
        source,
        &["worktree", "add", "--detach", path.to_str().unwrap(), base],
    );
}

fn commit_all(cwd: &Path, message: &str) {
    git(cwd, &["add", "."]);
    git(
        cwd,
        &["-c", "commit.gpgsign=false", "commit", "-qm", message],
    );
}

fn head(cwd: &Path) -> String {
    git_output(cwd, &["rev-parse", "HEAD"])
}

/// One retained workload solution committed in the fixture project's own
/// repository, exactly as a preserved independently accepted arm solution
/// leaves it: the scratch worktree is removed and the revision stays an
/// object of the repository.
fn retained_solution(
    proj: &Path,
    scratch: &Path,
    base: &str,
    relative: &str,
    content: &str,
    message: &str,
) -> String {
    git(
        proj,
        &[
            "worktree",
            "add",
            "--detach",
            scratch.to_str().unwrap(),
            base,
        ],
    );
    fs::write(scratch.join(relative), content).unwrap();
    commit_all(scratch, message);
    let revision = head(scratch);
    git(
        proj,
        &["worktree", "remove", "--force", scratch.to_str().unwrap()],
    );
    revision
}

/// Record one retained implementation on the run's hypothesis card, exactly
/// as the comparison owner records an independently accepted arm solution.
fn record_retained_solution(
    fixture: &Fixture,
    role: HypothesisRole,
    branch: &str,
    base: &str,
    revision: &str,
) {
    let implementation = BoundedImplementation::try_from_draft(ImplementationDraft {
        role,
        branch: branch.to_owned(),
        base: base.to_owned(),
        revision: revision.to_owned(),
        worktree: format!("retained/{branch}"),
        runtime: None,
        baseline_runtime: None,
    })
    .expect("the retained implementation record is bounded");
    board_hypothesis::record_implementation(
        &fixture.bd,
        &fixture.proj,
        &fixture.card,
        &implementation,
    )
    .expect("the hypothesis card records the retained implementation");
}

/// The activation owner's content identity rule over one committed range:
/// status and resulting blob identity per changed path, sorted.
fn change_signature(repo: &Path, base: &str, revision: &str) -> Vec<String> {
    let raw = git_output(
        repo,
        &[
            "diff",
            "--raw",
            "--no-abbrev",
            "--no-renames",
            base,
            revision,
        ],
    );
    let mut signature: Vec<String> = raw
        .lines()
        .filter_map(|line| {
            let (meta, path) = line.strip_prefix(':')?.split_once('\t')?;
            let fields: Vec<&str> = meta.split_whitespace().collect();
            (fields.len() >= 5).then(|| format!("{} {} {}", fields[4], fields[3], path))
        })
        .collect();
    signature.sort();
    signature
}

/// One source file's content with checkout line endings normalized, so a
/// preparation check compares actual content instead of Git's CRLF checkout.
fn source_content(path: &Path) -> String {
    fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

/// One dispatch-recorded attempt must carry the effective identity the
/// installed profile binding resolves for its conversation and the
/// deterministic titled surface derived from its own role and ordinal.
fn assert_dispatched_conversation(fixture: &Fixture, id: &str, role: AttemptRole) {
    let ordinal: u32 = id
        .rsplit('-')
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("the attempt id {id} carries no ordinal"));
    let cursor = fixture.cursor();
    let attempt = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == json!(id))
        .cloned()
        .unwrap_or_else(|| panic!("the {id} dispatch is retained"));
    assert_eq!(attempt["role"], role.as_str(), "{attempt}");
    assert_eq!(attempt["profile"], "ds", "{attempt}");
    assert_eq!(attempt["model"], "deepseek-flash", "{attempt}");
    assert_eq!(attempt["model_provider"], "deepseek", "{attempt}");
    assert_eq!(attempt["reasoning_effort"], "max", "{attempt}");
    let owner = dispatch_owner("workflow-fixture", role, ordinal);
    assert_eq!(attempt["owner"], owner, "{attempt}");
    assert_eq!(attempt["title"], format!("CEx (ds) - {owner}"), "{attempt}");
}

#[test]
fn no_evidence_idles_without_model_work() {
    let fixture = Fixture::new("no-evidence");
    fake_launcher(&fixture);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    let report = fixture.status_json();
    assert_eq!(report["phase"], "idle", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("no retained evidence"),
        "{report}"
    );
    assert_eq!(report["attempts"].as_array().unwrap().len(), 0);
    assert!(
        !fixture
            .run
            .join("assignments")
            .read_dir()
            .unwrap()
            .any(|entry| entry.is_ok()),
        "an idle loop writes no assignment"
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert_eq!(fixture.status_json()["phase"], "idle");
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 0);
}

#[test]
fn an_unreachable_route_records_a_failed_attempt_and_no_model_request() {
    let fixture = Fixture::new("unreachable");
    let (root, _) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fake_launcher(&fixture);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    let report = fixture.status_json();
    assert_eq!(report["phase"], "blocked", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("before submission"),
        "{report}"
    );
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 1, "{cursor}");
    assert_eq!(attempts[0]["role"], "investigator");
    assert_eq!(attempts[0]["state"], "failed");
    assert!(
        attempts[0]["reason"]
            .as_str()
            .unwrap()
            .contains("no model request was made"),
        "{cursor}"
    );
    // The refused conversation is still one explicit, attributable attempt:
    // the effective binding identity and its own titled surface are recorded
    // before any route is contacted.
    assert_dispatched_conversation(&fixture, "investigator-1", AttemptRole::Investigator);
}

#[test]
fn unsupported_citations_are_refused_without_model_churn() {
    let fixture = Fixture::new("unsupported");
    let (root, _) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    let baseline_attempts = fixture.cursor()["attempts"].as_array().unwrap().len();

    // The report cites a locator that is not retained in the index: intake
    // refuses the candidate instead of admitting it from investigator prose.
    let report = anchored_report("file:not-retained.txt");
    seed_investigator_report(&fixture, &report);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_eq!(report["phase"], "idle", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("not retained in the supplied evidence index"),
        "{report}"
    );
    assert!(report["candidate"].is_null(), "{report}");
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["attempts"].as_array().unwrap().len(),
        baseline_attempts + 1,
        "only the settled investigator attempt is recorded; no filler dispatch happens: {cursor}"
    );
    assert!(
        cursor["intake"]["outcomes"][0]["outcome"] == "refused",
        "{cursor}"
    );
    // Repeating the resume performs no new model work and keeps the refusal.
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        baseline_attempts + 1
    );
}

/// An addition that never states why the smaller routes cannot satisfy the
/// evidenced need is refused before admission: additional machinery is only
/// proposed after no change, reuse, simplification and subtraction were
/// considered.
#[test]
fn an_addition_without_the_smaller_route_consideration_is_refused() {
    let fixture = Fixture::new("missing-alternatives");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    let baseline_attempts = fixture.cursor()["attempts"].as_array().unwrap().len();

    let report = json!({
        "schema": 1,
        "candidates": [{
            "mechanism": "bounded-output",
            "conditions": "local-tool-runs",
            "observation": locator,
            "predicted": "less repeated context loading",
            "counterexample": "diagnostics vanish on failure",
            "acceptance": "the independent oracle passes",
            "spec": "openspec/changes/add-synthetic",
            "basis": locator,
            "treatment": "addition",
            "evidence": [{"locator": locator, "kind": "observed"}],
            "selection": selection_json(),
            "next_check": null,
        }],
        "idle_reason": null,
    });
    seed_investigator_report(&fixture, &report);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "idle", "{status}");
    assert_eq!(
        status["intake"]["outcomes"][0]["outcome"], "refused",
        "{status}"
    );
    assert!(
        status["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("alternatives consideration"),
        "{status}"
    );
    assert!(status["candidate"].is_null(), "{status}");
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["attempts"].as_array().unwrap().len(),
        baseline_attempts + 1,
        "only the settled investigator attempt is recorded: {cursor}"
    );
    assert!(
        cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|attempt| !matches!(
                attempt["role"].as_str(),
                Some("planner") | Some("implementer")
            )),
        "{cursor}"
    );
}

/// An evidenced need already satisfied by an existing attributable route is
/// concluded as reuse: the loop stays idle, creates no card and dispatches no
/// implementation conversation, so an overlapping capability is not
/// duplicated as additional machinery.
#[test]
fn an_overlapping_existing_route_concludes_reuse_without_dispatch() {
    let fixture = Fixture::new("overlapping-route");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    let baseline_attempts = fixture.cursor()["attempts"].as_array().unwrap().len();

    let report = json!({
        "schema": 1,
        "candidates": [{
            "mechanism": "reuse-existing-reader",
            "conditions": "local-tool-runs",
            "observation": locator,
            "predicted": "no additional machinery is needed",
            "counterexample": "the existing route loses the required isolation",
            "acceptance": "the independent oracle passes",
            "spec": "openspec/changes/add-synthetic",
            "basis": locator,
            "treatment": {"reuse": {"existing": locator}},
            "evidence": [{"locator": locator, "kind": "observed"}],
            "next_check": null,
        }],
        "idle_reason": null,
    });
    seed_investigator_report(&fixture, &report);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "idle", "{status}");
    assert_eq!(
        status["intake"]["outcomes"][0]["outcome"], "reuse-suffices",
        "{status}"
    );
    assert!(status["candidate"].is_null(), "{status}");
    assert!(
        status["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("reuse"),
        "{status}"
    );
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["attempts"].as_array().unwrap().len(),
        baseline_attempts + 1,
        "only the settled investigator attempt is recorded: {cursor}"
    );
    assert!(
        cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|attempt| !matches!(
                attempt["role"].as_str(),
                Some("planner") | Some("implementer")
            )),
        "{cursor}"
    );
}

/// An actual investigator conversation returns prose paragraphs plus its
/// schema-1 report as the final payload; the controller consumes that
/// terminal framing without replaying the model, and the digest of the raw
/// message (prose included) stays the recorded identity.
#[test]
fn prose_framed_terminal_result_is_consumed_and_never_replayed() {
    let fixture = Fixture::new("prose-framed");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    let baseline = fixture.cursor()["attempts"].as_array().unwrap().len();

    let payload = serde_json::to_string(&anchored_report(&locator)).unwrap();
    let message = format!(
        "The investigation inspected the retained evidence and found no source change to make.\nThe bounded report follows as the final line.\n\n{payload}\n"
    );
    let result = seed_investigator_message(&fixture, message.as_bytes());

    // Resume consumes the framed report through grounded intake with no new
    // investigator dispatch: the anchor card is reused as the candidate and
    // the raw result digest is the recorded intake identity.
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_eq!(report["candidate"]["hypothesis"], fixture.card, "{report}");
    assert_eq!(report["candidate"]["change"], "add-synthetic", "{report}");
    assert_eq!(
        report["intake"]["outcomes"][0]["outcome"], "existing",
        "{report}"
    );
    assert_eq!(
        report["intake"]["result_sha256"],
        json!(hash_bytes(message.as_bytes())),
        "{report}"
    );
    assert_eq!(
        fs::read(&result).unwrap().as_slice(),
        message.as_bytes(),
        "the raw terminal result is never edited"
    );
    let attempts = fixture.cursor()["attempts"].as_array().unwrap().len();
    assert_eq!(
        attempts,
        baseline + 1,
        "only the settled investigator attempt exists; no replay was dispatched: {}",
        fixture.cursor()
    );

    // A repeated resume waits on the same retained decision: no second
    // intake, no duplicate outcome and no new model attempt.
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["intake"]["result_sha256"],
        json!(hash_bytes(message.as_bytes())),
        "{cursor}"
    );
    assert_eq!(
        cursor["intake"]["outcomes"].as_array().unwrap().len(),
        1,
        "{cursor}"
    );
    assert_eq!(
        cursor["attempts"].as_array().unwrap().len(),
        attempts,
        "{cursor}"
    );
}

/// Two complete payloads in one terminal message are ambiguous: the intake
/// refuses the whole message and admits no candidate from either payload.
/// The unreadable bytes are still consumed once under their raw digest, so a
/// resume never re-reads them; without an installed launcher the fresh bounded
/// round stays gated instead of re-refusing the same message.
#[test]
fn an_ambiguous_terminal_message_is_consumed_once_without_admitting_a_candidate() {
    let fixture = Fixture::new("ambiguous-report");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    let baseline = fixture.cursor()["attempts"].as_array().unwrap().len();

    let payload = serde_json::to_string(&anchored_report(&locator)).unwrap();
    let message = format!("Investigator prose.\n\n{payload}\n{payload}\n");
    seed_investigator_message(&fixture, message.as_bytes());

    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert!(report["candidate"].is_null(), "{report}");
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["intake"]["result_sha256"],
        json!(hash_bytes(message.as_bytes())),
        "{cursor}"
    );
    assert_eq!(
        cursor["intake"]["outcomes"].as_array().unwrap().len(),
        0,
        "ambiguity admits no candidate outcome: {cursor}"
    );
    assert_eq!(
        cursor["attempts"].as_array().unwrap().len(),
        baseline + 1,
        "ambiguity starts no model work: {cursor}"
    );

    // A repeated resume never re-reads the consumed bytes: the recorded raw
    // digest stays untouched and, without an installed launcher, the fresh
    // bounded round waits on the missing visibility instead of re-refusing
    // the same message.
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    let report = fixture.status_json();
    assert!(report["candidate"].is_null(), "{report}");
    assert_eq!(report["dispatch"]["state"], "blocked", "{report}");
    assert!(
        report["dispatch"]["reason"]
            .as_str()
            .unwrap_or_default()
            .contains("missing visibility"),
        "{report}"
    );
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["intake"]["result_sha256"],
        json!(hash_bytes(message.as_bytes())),
        "{cursor}"
    );
    assert_eq!(
        cursor["attempts"].as_array().unwrap().len(),
        baseline + 1,
        "{cursor}"
    );
}

/// The real 2.8 wedge: a schema-1-unparseable investigator result (model
/// format variance - here string evidence entries) used to idle without
/// recording the intake digest, so every resume re-consumed the same
/// unreadable bytes and the run never progressed. The bytes are consumed once
/// with no outcomes; the next resume dispatches exactly one fresh bounded
/// round; and a valid second report proceeds through grounded intake
/// normally.
#[test]
fn an_unreadable_investigator_result_is_consumed_once_then_redispatched() {
    let fixture = Fixture::new("unreadable-redispatch");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fake_launcher(&fixture);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));

    let malformed = malformed_report_payload(&locator);
    seed_investigator_round(&fixture, 1, &malformed);

    // The resume settles and consumes the unreadable bytes exactly once: the
    // raw digest is recorded with no outcomes and no candidate is admitted.
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert!(report["candidate"].is_null(), "{report}");
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["intake"]["result_sha256"],
        json!(hash_bytes(&malformed)),
        "{cursor}"
    );
    assert_eq!(
        cursor["intake"]["outcomes"].as_array().unwrap().len(),
        0,
        "{cursor}"
    );
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 1, "{cursor}");

    // The next resume dispatches exactly one fresh bounded round instead of
    // re-consuming the same bytes. The fixture launcher refuses the
    // conversation before submission, so the re-dispatch is proven by the
    // attempt record it wrote through the real dispatch path.
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["intake"]["result_sha256"],
        json!(hash_bytes(&malformed)),
        "the consumed bytes are never re-read: {cursor}"
    );
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2, "{cursor}");
    assert_eq!(attempts[1]["id"], "investigator-2", "{cursor}");
    assert_dispatched_conversation(&fixture, "investigator-2", AttemptRole::Investigator);

    // A valid second report proceeds through grounded intake normally.
    let valid = serde_json::to_vec_pretty(&anchored_report(&locator)).unwrap();
    seed_investigator_round(&fixture, 2, &valid);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_eq!(report["candidate"]["hypothesis"], fixture.card, "{report}");
    assert_eq!(
        report["intake"]["outcomes"][0]["outcome"], "existing",
        "{report}"
    );
    assert_eq!(
        report["intake"]["result_sha256"],
        json!(hash_bytes(&valid)),
        "{report}"
    );
}

/// Three consecutive unreadable investigator rounds consume the bounded retry
/// budget: each round's bytes are consumed exactly once, below the bound the
/// next resume re-dispatches one fresh round, and the third unreadable round
/// records idle with the exact reason at the bound of three rounds.
#[test]
fn three_unreadable_investigator_rounds_end_idle_at_the_bound() {
    let fixture = Fixture::new("unreadable-bound");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fake_launcher(&fixture);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));

    let mut digests = Vec::new();
    for ordinal in 1..=3_u32 {
        // Each round's terminal message differs, exactly as separate model
        // rounds would: prose framing followed by the unreadable payload.
        let malformed = [
            format!("Investigator round {ordinal}.\n\n").into_bytes(),
            malformed_report_payload(&locator),
        ]
        .concat();
        let digest = hash_bytes(&malformed);
        digests.push(digest.clone());
        seed_investigator_round(&fixture, ordinal, &malformed);

        // This resume settles and consumes the unreadable round exactly once.
        let resume = fixture.resume();
        assert!(resume.status.success(), "{}", text(&resume));
        let cursor = fixture.cursor();
        assert_eq!(cursor["intake"]["result_sha256"], json!(digest), "{cursor}");
        assert_eq!(
            cursor["intake"]["outcomes"].as_array().unwrap().len(),
            0,
            "{cursor}"
        );
        assert!(cursor["candidate"].is_null(), "{cursor}");

        if ordinal < 3 {
            // Below the bound the next resume re-dispatches exactly one fresh
            // bounded round.
            let resume = fixture.resume();
            assert!(resume.status.success(), "{}", text(&resume));
            let cursor = fixture.cursor();
            let attempts = cursor["attempts"].as_array().unwrap();
            assert_eq!(attempts.len(), ordinal as usize + 1, "{cursor}");
            assert_eq!(
                attempts[ordinal as usize]["id"],
                format!("investigator-{}", ordinal + 1),
                "{cursor}"
            );
        }
    }

    // The third unreadable round reached the bound: the run idles with the
    // exact unreadable reason and dispatches no further investigator round.
    let report = fixture.status_json();
    assert_eq!(report["phase"], "idle", "{report}");
    let condition = report["condition"].as_str().unwrap_or_default().to_owned();
    assert!(
        condition.contains("not a bounded schema-1 investigator report"),
        "{report}"
    );
    assert!(
        condition.contains("bound of 3 investigator rounds"),
        "{report}"
    );
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    let report = fixture.status_json();
    assert_eq!(report["phase"], "idle", "{report}");
    assert_eq!(report["candidate"], Value::Null, "{report}");
    assert_eq!(
        report["condition"].as_str().unwrap_or_default(),
        condition,
        "the bounded idle reason stays recorded: {report}"
    );
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["intake"]["result_sha256"],
        json!(digests[2]),
        "{cursor}"
    );
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 3, "{cursor}");
    assert!(
        attempts
            .iter()
            .all(|attempt| attempt["id"] != "investigator-4"),
        "the bound starts no fourth round: {cursor}"
    );
}

/// A completed investigator round that retained no terminal result at all was
/// refused on every resume without recording any identity, so the run could
/// never move past it. The missing result is consumed once under the
/// attempt's own recorded identity, and the next resume dispatches one fresh
/// round.
#[test]
fn a_completed_investigator_without_a_retained_result_is_consumed_once() {
    let fixture = Fixture::new("unretained-result");
    let (root, _locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fake_launcher(&fixture);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));

    let receipt = fixture.run.join("investigator-receipt-1.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-investigator-1",
        "gen-1",
        "completed",
        Some(0),
    );
    replace_attempt(
        &fixture,
        attempt_json(
            "investigator-1",
            "investigator",
            "workflow-fixture-investigator-1",
            "gen-1",
            &receipt,
            None,
            None,
            "started",
        ),
    );

    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let cursor = fixture.cursor();
    let identity = cursor["intake"]["result_sha256"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert_eq!(identity.len(), 64, "{cursor}");
    assert_eq!(
        cursor["intake"]["outcomes"].as_array().unwrap().len(),
        0,
        "{cursor}"
    );
    assert_eq!(cursor["attempts"].as_array().unwrap().len(), 1, "{cursor}");

    // The recorded identity settles the attempt, so the next resume
    // re-dispatches one fresh round instead of re-refusing it forever.
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["intake"]["result_sha256"],
        json!(identity),
        "{cursor}"
    );
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2, "{cursor}");
    assert_eq!(attempts[1]["id"], "investigator-2", "{cursor}");
}

/// The reproducing ordinary case: one long change name, seven explicit
/// writable file paths and the fixture's absolute board and project roots.
/// Every generated workflow assignment must satisfy the native structured
/// contract through the native model-free path, keep the full declared scope
/// visible, and keep a pre-submission refusal a recorded failed attempt while
/// the settled investigator result is consumed exactly once.
#[test]
fn ordinary_multi_file_briefs_pass_the_native_assignment_contract() {
    let change = "add-evidence-grounded-terminal-report-intake";
    let fixture = Fixture::new_with_change("brief-budget", change);
    let (root, locator) = write_evidence_root(&fixture);
    let scope = [
        "crates/one/src/improvement_intake_adapter.rs",
        "crates/one/src/improvement_workflow_brief.rs",
        "crates/one/src/executor_assignment_contract.rs",
        "crates/one/src/terminal_framing_recovery.rs",
        "crates/one/src/retained_evidence_index.rs",
        "crates/one/src/dispatch_scope_validation.rs",
        "crates/one/src/no_replay_recovery.rs",
    ];
    fixture.write_spec(&[
        ("evidence_root", json!(root)),
        ("writable_scope", json!(scope)),
    ]);
    fake_launcher(&fixture);
    // The native `executor assignment` check names this pool slot; it is a
    // registered worktree of the fixture project, created model-free.
    slot_worktree(
        &fixture.proj,
        &fixture.root.join("proj-brief-budget-wt1"),
        &head(&fixture.proj),
    );

    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    // The start dispatch wrote its investigator assignment before the fake
    // surface failed; validate that builder through the native model-free path.
    let investigator_assignment = fixture.run.join("assignments/investigator-1.json");
    assert!(
        investigator_assignment.is_file(),
        "{investigator_assignment:?}"
    );
    let check = fixture.assignment_check(1, &investigator_assignment);
    assert!(check.status.success(), "{}", text(&check));
    let investigator_brief = text(&check);
    assert!(
        investigator_brief.contains("executor assignment valid"),
        "{investigator_brief}"
    );
    assert!(
        investigator_brief.contains(&locator),
        "the retained evidence locator stays visible: {investigator_brief}"
    );
    assert!(
        investigator_brief.contains(&format!("source: {}", fixture.proj.display())),
        "the exact source checkout stays visible: {investigator_brief}"
    );
    assert!(
        investigator_brief.contains(&format!("planning root {}", fixture.proj.display())),
        "the exact specification root stays visible: {investigator_brief}"
    );
    // The brief offers the smaller treatments before additional machinery and
    // states the reuse and review rules the intake enforces.
    for needle in [
        "\"no-change\"",
        "\"reuse\"",
        "\"simplification\"",
        "\"subtraction\"",
        "\"alternatives\"",
        "before additional machinery",
        "at most 2048 bytes",
        "at most 512 bytes",
        "counterexample",
        "actual consumption",
        "skills usage",
        "skill-evolution",
        "auto-delete",
        "fewer lines",
    ] {
        assert!(
            investigator_brief.contains(needle),
            "{needle}: {investigator_brief}"
        );
    }

    // The refused pre-submission dispatch is a recorded attempt with a known
    // outcome, never an unknown or billed one.
    let cursor = fixture.cursor();
    assert!(
        cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|attempt| attempt["state"] != "unknown"),
        "{cursor}"
    );

    // Replace the refused investigator conversation with the completed one the
    // real dispatcher would record; the controller must consume it exactly once.
    let result = fixture.run.join("investigator-result.json");
    // The consumed report carries the honest analysis of a real investigator
    // round: the five-route alternatives comparison and the selection clauses
    // exceed the former 256/192-byte bounds and stay single-line.
    let rich_alternatives = format!(
        "all five routes were compared: no change leaves the recorded burden repeating, reuse of the existing reader re-enters the same load path, simplification removes no measured step, and subtraction would drop a still-consumed capability; {}",
        "each smaller route fails the evidenced need under the frozen conditions ".repeat(3)
    );
    let mut rich_selection = selection_json();
    rich_selection["rationale"] = json!(format!(
        "the chosen unit exercises the claimed mechanism because {}",
        "the recorded repeated reads happen inside the same command the candidate changes "
            .repeat(3)
    ));
    rich_selection["controls"] = json!(format!(
        "frozen inputs and the accepted baseline conditions are retained for both arms, and {}",
        "the declared operating mode stays frozen while no shared state is written ".repeat(3)
    ));
    assert!(
        (257..=2048).contains(&rich_alternatives.len()),
        "the fixture must exceed the former statement bound: {}",
        rich_alternatives.len()
    );
    for field in ["rationale", "controls"] {
        let length = rich_selection[field].as_str().unwrap().len();
        assert!(
            (193..=512).contains(&length),
            "the fixture must exceed the former field bound within the raised one: {length}"
        );
    }
    let report = json!({
        "schema": 1,
        "candidates": [{
            "mechanism": "bounded-output",
            "conditions": "local-tool-runs",
            "observation": locator,
            "predicted": "less repeated context loading",
            "counterexample": "diagnostics vanish on failure",
            "acceptance": "the independent oracle passes",
            "alternatives": rich_alternatives,
            "spec": format!("openspec/changes/{change}"),
            "basis": locator,
            "treatment": "addition",
            "evidence": [{"locator": locator, "kind": "observed"}],
            "selection": rich_selection,
            "next_check": null,
        }],
        "idle_reason": null,
    });
    fs::write(&result, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    let receipt = fixture.run.join("investigator-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-investigator-1",
        "gen-1",
        "completed",
        Some(0),
    );
    replace_attempt(
        &fixture,
        attempt_json(
            "investigator-1",
            "investigator",
            "workflow-fixture-investigator-1",
            "gen-1",
            &receipt,
            Some(&result),
            None,
            "started",
        ),
    );
    let result_sha = hash_bytes(&fs::read(&result).unwrap());

    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["candidate"]["hypothesis"], fixture.card, "{status}");
    assert_eq!(status["candidate"]["change"], change, "{status}");
    assert_eq!(
        status["intake"]["outcomes"][0]["outcome"], "existing",
        "{status}"
    );
    assert_eq!(
        status["intake"]["result_sha256"],
        json!(result_sha),
        "{status}"
    );

    // The implementation dispatch wrote its brief before the fake surface
    // failed; the refusal is pre-submission and never an unknown billed attempt.
    let implementer = fixture.cursor()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == "implementer-1")
        .cloned()
        .expect("the implementation dispatch recorded its attempt");
    assert_eq!(implementer["state"], "failed", "{implementer}");
    let reason = implementer["reason"].as_str().unwrap().to_owned();
    assert!(reason.contains("before submission"), "{implementer}");
    assert!(
        !reason.contains("objective") && !reason.contains("1024"),
        "the generated assignment passes the native limits: {implementer}"
    );
    assert_dispatched_conversation(&fixture, "implementer-1", AttemptRole::Implementer);

    // Native model-free validation: the reproducing multi-file brief passes
    // the same validator dispatch uses and renders the full brief.
    let implementer_assignment = fixture.run.join("assignments/implementer-1.json");
    assert!(
        implementer_assignment.is_file(),
        "{implementer_assignment:?}"
    );
    let check = fixture.assignment_check(1, &implementer_assignment);
    assert!(check.status.success(), "{}", text(&check));
    let brief = text(&check);
    assert!(brief.contains("executor assignment valid"), "{brief}");
    assert!(brief.contains(change), "{brief}");
    assert!(
        brief.contains(&format!("source: {}", fixture.proj.display())),
        "the exact source checkout stays visible: {brief}"
    );
    assert!(
        brief.contains(&format!(
            "openspec/changes/{change}/specs/synthetic/spec.md"
        )),
        "the exact specification artifact stays visible: {brief}"
    );
    for path in &scope {
        assert!(
            brief.contains(*path),
            "the declared writable path {path} stays visible: {brief}"
        );
    }
    assert!(
        brief.contains(&fixture.card) && brief.contains(" show "),
        "the card read stays visible: {brief}"
    );
    assert!(
        brief.contains("the oracle checker executes"),
        "the predeclared acceptance stays visible: {brief}"
    );

    // A repeated resume dispatches no new investigator round and never
    // re-consumes the settled result.
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    let cursor = fixture.cursor();
    assert_eq!(
        cursor["intake"]["result_sha256"],
        json!(result_sha),
        "{cursor}"
    );
    assert_eq!(
        cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|attempt| attempt["role"] == "investigator")
            .count(),
        1,
        "{cursor}"
    );
    assert!(
        cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|attempt| attempt["state"] != "unknown"),
        "{cursor}"
    );
}

#[test]
fn a_lost_conversation_surface_suspends_new_dispatch_without_a_fallback() {
    let fixture = Fixture::new("surface-loss");
    let (root, _) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fake_launcher(&fixture);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    // Replace the pre-submission refusal with the interrupted surface loss the
    // owning dispatcher records when an accepted conversation's surface dies.
    let receipt = fixture.run.join("investigator-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-inv-1",
        "gen-1",
        "interrupted",
        None,
    );
    let mut interrupted = attempt_json(
        "investigator-1",
        "investigator",
        "workflow-fixture-inv-1",
        "gen-1",
        &receipt,
        None,
        None,
        "interrupted",
    );
    interrupted["reason"] = json!("the owned native frontend exited");
    replace_attempt(&fixture, interrupted);

    // While the surface loss stands, resume refuses to start new model work:
    // the missing visibility is the recorded blocker, not a hidden retry.
    for _ in 0..2 {
        let resume = fixture.resume();
        assert!(resume.status.success(), "{}", text(&resume));
        let status = fixture.status_json();
        assert_eq!(status["dispatch"]["state"], "blocked", "{status}");
        let reason = status["dispatch"]["reason"].as_str().unwrap_or_default();
        assert!(reason.contains("missing visibility"), "{status}");
        assert!(reason.contains("no hidden fallback"), "{status}");
        assert_eq!(
            fixture.cursor()["attempts"].as_array().unwrap().len(),
            1,
            "no conversation is dispatched while the surface is lost"
        );
    }

    // An explicit stop is the documented release: the next resume may start a
    // fresh conversation, and that conversation gets its own titled surface.
    let stop = fixture.improve(&["stop", "--run", fixture.run.to_str().unwrap()]);
    assert!(stop.status.success(), "{}", text(&stop));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2, "{cursor}");
    assert_eq!(attempts[1]["id"], "investigator-2", "{cursor}");
    assert_ne!(attempts[0]["owner"], attempts[1]["owner"], "{cursor}");
    assert_ne!(attempts[0]["title"], attempts[1]["title"], "{cursor}");
    assert_dispatched_conversation(&fixture, "investigator-2", AttemptRole::Investigator);
}

#[test]
fn a_declared_runner_that_cannot_be_honored_never_dispatches_a_model_request() {
    let fixture = Fixture::new("runner-refusal");
    let (root, _) = write_evidence_root(&fixture);
    fake_launcher(&fixture);
    type Case<'a> = (&'a str, Vec<(&'a str, Value)>, &'a str);
    let cases: Vec<Case> = vec![
        (
            "model-mismatch",
            vec![(
                "runner",
                json!({
                    "profile": "ds",
                    "model": "other-model",
                    "model_provider": "deepseek",
                    "reasoning_effort": "max",
                }),
            )],
            "does not match the installed profile binding",
        ),
        (
            "profile-absent",
            vec![(
                "runner",
                json!({
                    "profile": "ghost",
                    "model": "deepseek-flash",
                    "model_provider": "deepseek",
                    "reasoning_effort": "max",
                }),
            )],
            "is not installed in",
        ),
        (
            "local-runner-mismatch",
            vec![
                (
                    "local_runner",
                    json!({
                        "endpoint": "http://127.0.0.1:9/v1",
                        "model": "other-local",
                    }),
                ),
                (
                    "qualification",
                    json!(fixture.root.join("qualification.json")),
                ),
            ],
            "but the declared local runner serves",
        ),
    ];
    for (name, extra, expected) in cases {
        let run = fixture.root.join("runs").join(name);
        fs::create_dir_all(&run).unwrap();
        let spec = fixture.root.join(format!("run-spec-{name}.json"));
        let mut replacements: Vec<(&str, Value)> = vec![("evidence_root", json!(root.clone()))];
        replacements.extend(extra);
        fixture.write_run_spec(&format!("refuse-{name}"), &spec, &replacements);
        let start = fixture.improve(&[
            "start",
            "--run",
            run.to_str().unwrap(),
            "--spec",
            spec.to_str().unwrap(),
        ]);
        assert!(start.status.success(), "{name}: {}", text(&start));
        let status = fixture.run_status(&run);
        assert_eq!(status["dispatch"]["state"], "blocked", "{name}: {status}");
        let reason = status["dispatch"]["reason"].as_str().unwrap_or_default();
        assert!(reason.contains(expected), "{name}: {status}");
        assert_eq!(
            status["attempts"].as_array().unwrap().len(),
            0,
            "{name}: no conversation may be attempted: {status}"
        );

        // A resume re-evaluates the same declaration and still records the
        // refusal instead of dispatching under a different model or route.
        let resume = fixture.improve(&["resume", "--run", run.to_str().unwrap()]);
        assert!(resume.status.success(), "{name}: {}", text(&resume));
        let cursor: Value =
            serde_json::from_slice(&fs::read(run.join("cursor.json")).unwrap()).unwrap();
        assert_eq!(
            cursor["attempts"].as_array().unwrap().len(),
            0,
            "{name}: {cursor}"
        );
        assert!(
            cursor["effects"].as_array().unwrap().iter().any(|effect| {
                effect["kind"] == "dispatch-refused"
                    && effect["detail"]
                        .as_str()
                        .unwrap_or_default()
                        .contains(expected)
            }),
            "{name}: the refusal stays recorded: {cursor}"
        );
    }
}

#[test]
fn retained_anchor_result_reaches_candidate_ready_through_planning_and_implementation() {
    let fixture = Fixture::new("anchor-ready");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));

    // Resume one: the settled investigator report is consumed through
    // grounded intake, the anchor card becomes the candidate, its own change
    // is qualified in the candidate worktree and the implementation
    // conversation is blocked only by the missing launcher.
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_eq!(report["candidate"]["hypothesis"], fixture.card, "{report}");
    assert_eq!(report["candidate"]["change"], "add-synthetic", "{report}");
    assert_eq!(
        report["intake"]["outcomes"][0]["outcome"], "existing",
        "{report}"
    );
    let worktree = fixture.candidate_worktree(&fixture.card);
    assert!(worktree.is_dir(), "candidate worktree exists");
    assert!(
        worktree
            .join("openspec/changes/add-synthetic/proposal.md")
            .is_file(),
        "the change is available in the candidate worktree"
    );
    assert!(
        report["candidate"]["planning_receipt"].is_string(),
        "{report}"
    );
    let planner_attempts = fixture.cursor()["attempts"].as_array().unwrap().len();

    // Simulate the implementation conversation: a detached slot worktree of
    // the candidate branch with a committed, in-scope change.
    let candidate_worktree = fixture.candidate_worktree(&fixture.card);
    let slot = fixture.root.join("implementer-slot");
    slot_worktree(&candidate_worktree, &slot, &head(&candidate_worktree));
    fs::write(
        slot.join("crates/one/src/lib.rs"),
        "// implemented by the bounded conversation\n",
    )
    .unwrap();
    commit_all(&slot, "implement bounded output");
    let revision = head(&slot);
    let receipt = fixture.run.join("implementer-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-implementer-1",
        "gen-1",
        "completed",
        Some(0),
    );
    let result = fixture.run.join("implementer-result.txt");
    fs::write(&result, "checks: synthetic ok\n").unwrap();
    push_attempt(
        &fixture,
        attempt_json(
            "implementer-1",
            "implementer",
            "workflow-fixture-implementer-1",
            "gen-1",
            &receipt,
            Some(&result),
            Some(&slot),
            "started",
        ),
    );

    // Resume two: the completed implementation is settled, validated against
    // the exact base and scope, advanced onto the candidate branch and
    // retained as candidate-ready.
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_eq!(report["phase"], "candidate-ready", "{report}");
    assert_eq!(report["candidate"]["revision"], revision, "{report}");
    assert_eq!(
        report["candidate"]["branch"],
        format!("improve/{}/{}", "workflow-fixture", fixture.card),
        "{report}"
    );
    assert_eq!(head(&candidate_worktree), revision);
    // The unproven candidate revision exists only on its own candidate branch:
    // the accepted mainline still points at the frozen base and no mainline
    // ref contains the candidate commit.
    let base = fixture.base_revision();
    assert_eq!(
        head(&fixture.proj),
        base,
        "candidate work never advances the accepted mainline"
    );
    let candidate_ref = format!("improve/{}/{}", "workflow-fixture", fixture.card);
    assert_eq!(
        git_output(
            &fixture.proj,
            &[
                "for-each-ref",
                "--contains",
                &revision,
                "--format=%(refname:short)",
                "refs/heads"
            ]
        ),
        candidate_ref,
        "the candidate revision is contained only by its own candidate branch"
    );
    assert!(
        fixture.bd_comments(&fixture.card).contains(&format!(
            "hypothesis-implementation v1 item={}",
            fixture.card
        )),
        "the card records the implementation reference"
    );
    assert!(
        fixture.bd_comments(&fixture.card).contains(&revision),
        "the card records the validated revision"
    );
    assert!(
        fixture.cursor()["attempts"].as_array().unwrap().len() > planner_attempts,
        "the implementation attempt is retained"
    );

    // A repeated resume reuses the exact retained evidence: no replay, no
    // duplicate attempt, the phase stays candidate-ready.
    let attempts = fixture.cursor()["attempts"].as_array().unwrap().len();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert_eq!(fixture.status_json()["phase"], "candidate-ready");
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts
    );
}

/// B's run on the resulting baseline: the independently accepted `role=workload`
/// solution retained on B's own card is materialized onto the freshly
/// allocated candidate branch without opening any implementer conversation,
/// and a repeated resume reuses the exact same revision.
#[test]
fn a_retained_workload_revision_is_carried_onto_the_candidate_branch() {
    let fixture = Fixture::new("carry-exact");
    let base = fixture.base_revision();
    let retained = retained_solution(
        &fixture.proj,
        &fixture.root.join("scratch-retained"),
        &base,
        "crates/one/src/lib.rs",
        "// retained workload solution\n",
        "the accepted arm solves the workload",
    );
    record_retained_solution(
        &fixture,
        HypothesisRole::Workload,
        "workload-candidate",
        &base,
        &retained,
    );
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));

    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let report = fixture.status_json();
    assert_eq!(report["phase"], "candidate-ready", "{report}\n{output}");
    let carried = report["candidate"]["revision"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(!carried.is_empty(), "{report}");
    let worktree = fixture.candidate_worktree(&fixture.card);
    assert_eq!(head(&worktree), carried, "{report}");
    assert_eq!(
        change_signature(&worktree, &base, &carried),
        change_signature(&worktree, &base, &retained),
        "the carried revision reproduces the retained change identity exactly"
    );
    assert_eq!(
        source_content(&worktree.join("crates/one/src/lib.rs")),
        "// retained workload solution\n"
    );
    // No implementer conversation is opened for the model-free carry, and the
    // candidate-role implementation reference is recorded on the card.
    let attempts = fixture.cursor()["attempts"].as_array().unwrap().clone();
    assert!(
        attempts
            .iter()
            .all(|attempt| attempt["role"] != "implementer"),
        "{attempts:?}"
    );
    assert!(
        fixture.bd_comments(&fixture.card).contains(&carried),
        "the card records the carried candidate revision"
    );

    // A repeated resume reuses the exact retained revision without replay.
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(
        fixture.status_json()["candidate"]["revision"],
        json!(carried)
    );
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts.len()
    );
}

/// The supported A-on-B to B-on-C transition: A's adoption advanced the
/// baseline; B's retained solution is rebased onto the resulting baseline as
/// B's next candidate. The rebase changes the commit identity but not the
/// exact retained change identity the activation lineage owner verifies.
#[test]
fn a_retained_workload_revision_is_rebased_onto_the_resulting_baseline() {
    let fixture = Fixture::new("carry-rebase");
    let frozen = fixture.base_revision();
    let retained = retained_solution(
        &fixture.proj,
        &fixture.root.join("scratch-retained"),
        &frozen,
        "crates/one/src/lib.rs",
        "// retained workload solution\n",
        "the accepted arm solves the workload",
    );
    // A was adopted: the resulting baseline carries an unrelated source file
    // that neither retained solution touches.
    fs::write(
        fixture.proj.join("crates/one/src/from-a.rs"),
        "// adopted predecessor A\n",
    )
    .unwrap();
    commit_all(&fixture.proj, "adopted predecessor A");
    let resulting = head(&fixture.proj);
    assert_ne!(resulting, frozen);
    record_retained_solution(
        &fixture,
        HypothesisRole::Workload,
        "workload-candidate",
        &frozen,
        &retained,
    );
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    assert_eq!(fixture.base_revision(), resulting);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));

    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let report = fixture.status_json();
    assert_eq!(report["phase"], "candidate-ready", "{report}\n{output}");
    let carried = report["candidate"]["revision"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let worktree = fixture.candidate_worktree(&fixture.card);
    assert_ne!(
        carried, retained,
        "the rebased candidate has a changed commit identity"
    );
    assert_eq!(
        change_signature(&worktree, &resulting, &carried),
        change_signature(&worktree, &frozen, &retained),
        "the rebased candidate carries the same exact retained change"
    );
    assert_eq!(
        source_content(&worktree.join("crates/one/src/lib.rs")),
        "// retained workload solution\n"
    );
    assert_eq!(head(&worktree), carried, "{report}");
    assert!(
        fixture.bd_comments(&fixture.card).contains(&carried),
        "the card records the rebased candidate revision"
    );
}

/// The independent-justification gate and the no-invented-artifact rule: a
/// candidate-role record or an unattributable solution yields the ordinary
/// grounded implementation path, and a retained change that cannot be
/// materialized exactly (here: it escapes the writable scope) is refused with
/// its exact reason instead of a fabricated candidate.
#[test]
fn only_an_independently_justified_retained_solution_is_carried() {
    // A candidate-role record - any run's own allocation or implementation -
    // is not independently justified and is never carried.
    let fixture = Fixture::new("carry-role-gate");
    let base = fixture.base_revision();
    let revision = retained_solution(
        &fixture.proj,
        &fixture.root.join("scratch-candidate-role"),
        &base,
        "crates/one/src/lib.rs",
        "// candidate-role change\n",
        "a candidate-role record",
    );
    record_retained_solution(
        &fixture,
        HypothesisRole::Candidate,
        "workload-candidate",
        &base,
        &revision,
    );
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    // The ordinary implementation path is what remains pending: it needs the
    // installed model launcher this fixture does not provide.
    assert_eq!(report["phase"], "planning", "{report}");
    assert_eq!(report["dispatch"]["state"], "blocked", "{report}");
    assert!(
        report["dispatch"]["reason"]
            .as_str()
            .unwrap_or("")
            .contains("launcher"),
        "{report}"
    );
    assert!(report["candidate"]["revision"].is_null(), "{report}");

    // A workload record whose solution is not an object of this repository is
    // attributed to nothing; it never fabricates a candidate either.
    let fixture = Fixture::new("carry-unattributable");
    let base = fixture.base_revision();
    record_retained_solution(
        &fixture,
        HypothesisRole::Workload,
        "workload-candidate",
        &base,
        &"0".repeat(40),
    );
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_eq!(report["phase"], "planning", "{report}");
    assert_eq!(report["dispatch"]["state"], "blocked", "{report}");
    assert!(report["candidate"]["revision"].is_null(), "{report}");

    // A retained change that escapes the declared writable scope is refused
    // with its exact reason: no candidate is fabricated and no substitute
    // implementation is dispatched.
    let fixture = Fixture::new("carry-scope");
    let base = fixture.base_revision();
    let escaped = retained_solution(
        &fixture.proj,
        &fixture.root.join("scratch-escaped"),
        &base,
        "global/orchestration.toml",
        "schema = 2\n",
        "a change outside the declared writable scope",
    );
    record_retained_solution(
        &fixture,
        HypothesisRole::Workload,
        "workload-candidate",
        &base,
        &escaped,
    );
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let report = fixture.status_json();
    assert_eq!(report["phase"], "idle", "{report}\n{output}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap_or("")
            .contains("outside the declared writable scope"),
        "{report}\n{output}"
    );
    assert!(report["candidate"]["revision"].is_null(), "{report}");
    let worktree = fixture.candidate_worktree(&fixture.card);
    assert_eq!(
        head(&worktree),
        base,
        "the refused carry leaves the candidate branch at its allocation base"
    );
}

#[test]
fn escaped_or_uncommitted_output_never_reaches_candidate_ready() {
    let fixture = Fixture::new("escaped");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fixture.start();
    seed_investigator_report(&fixture, &anchored_report(&locator));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let candidate_worktree = fixture.candidate_worktree(&fixture.card);
    assert!(candidate_worktree.is_dir());
    let before = head(&candidate_worktree);

    // An implementation that changes a path outside the declared writable
    // scope is refused.
    let slot = fixture.root.join("escaped-slot");
    slot_worktree(&candidate_worktree, &slot, &before);
    fs::write(slot.join("global/orchestration.toml"), "schema = 2\n").unwrap();
    commit_all(&slot, "out of scope");
    let receipt = fixture.run.join("escaped-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-implementer-1",
        "gen-1",
        "completed",
        Some(0),
    );
    push_attempt(
        &fixture,
        attempt_json(
            "implementer-1",
            "implementer",
            "workflow-fixture-implementer-1",
            "gen-1",
            &receipt,
            None,
            Some(&slot),
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_ne!(report["phase"], "candidate-ready", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("outside the declared writable scope"),
        "{report}"
    );
    assert_eq!(report["candidate"]["revision"], Value::Null, "{report}");
    assert_eq!(
        head(&candidate_worktree),
        before,
        "the candidate branch stays at its pre-attempt revision"
    );

    // A dirty returned checkout (uncommitted work) is refused as well.
    let slot = fixture.root.join("dirty-slot");
    slot_worktree(&candidate_worktree, &slot, &before);
    fs::write(slot.join("crates/one/src/lib.rs"), "// committed\n").unwrap();
    commit_all(&slot, "committed work");
    fs::write(slot.join("crates/one/src/lib.rs"), "// dirty\n").unwrap();
    let receipt = fixture.run.join("dirty-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-implementer-2",
        "gen-2",
        "completed",
        Some(0),
    );
    push_attempt(
        &fixture,
        attempt_json(
            "implementer-2",
            "implementer",
            "workflow-fixture-implementer-2",
            "gen-2",
            &receipt,
            None,
            Some(&slot),
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_ne!(report["phase"], "candidate-ready", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("local or untracked"),
        "{report}"
    );
}

#[test]
fn changed_planning_artifacts_are_refused_before_the_branch_advances() {
    let fixture = Fixture::new("artifact-change");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fixture.start();
    seed_investigator_report(&fixture, &anchored_report(&locator));
    fixture.resume();
    let candidate_worktree = fixture.candidate_worktree(&fixture.card);
    let before = head(&candidate_worktree);

    let slot = fixture.root.join("artifact-slot");
    slot_worktree(&candidate_worktree, &slot, &before);
    fs::write(
        slot.join("openspec/changes/add-synthetic/specs/synthetic/spec.md"),
        "## ADDED Requirements\n\n### Requirement: Weakened behavior\n\nThe system SHALL do something narrower.\n\n#### Scenario: Synthetic case\n\n- **WHEN** the probe runs\n- **THEN** it reports success\n",
    )
    .unwrap();
    fs::write(slot.join("crates/one/src/lib.rs"), "// implemented\n").unwrap();
    commit_all(&slot, "weaken the acceptance artifact");
    let receipt = fixture.run.join("artifact-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-implementer-1",
        "gen-1",
        "completed",
        Some(0),
    );
    push_attempt(
        &fixture,
        attempt_json(
            "implementer-1",
            "implementer",
            "workflow-fixture-implementer-1",
            "gen-1",
            &receipt,
            None,
            Some(&slot),
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_ne!(report["phase"], "candidate-ready", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("planning artifacts"),
        "{report}"
    );
    assert_eq!(
        head(&candidate_worktree),
        before,
        "the candidate branch never advanced past a changed acceptance input"
    );
}

#[test]
fn new_candidate_scaffolds_plans_and_implements_its_own_change() {
    let fixture = Fixture::new("new-candidate");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fixture.start();
    seed_investigator_report(&fixture, &missing_change_report(&locator));
    // Readiness is per command: the launcher appears before the resume that
    // reaches planning, so the planning conversation is actually dispatched
    // and its attempt is recorded before its route fails.
    fake_launcher(&fixture);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    let candidate = status["candidate"]["hypothesis"]
        .as_str()
        .expect("a new candidate card was admitted")
        .to_owned();
    assert_ne!(candidate, fixture.card, "{status}");
    assert_eq!(
        status["intake"]["outcomes"][0]["outcome"], "admitted",
        "{status}"
    );
    assert_eq!(
        status["candidate"]["change"], "add-narrow-context",
        "{status}"
    );

    // The controller scaffolded the new change and committed it on the
    // candidate branch before dispatching the planning conversation.
    let candidate_worktree = fixture.candidate_worktree(&candidate);
    assert!(
        candidate_worktree
            .join("openspec/changes/add-narrow-context/.openspec.yaml")
            .is_file(),
        "the scaffolded change exists in the candidate worktree"
    );
    assert_ne!(
        head(&candidate_worktree),
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        "the scaffold is its own commit on the candidate branch"
    );
    let scaffold_revision = head(&candidate_worktree);
    assert_eq!(
        git_output(&candidate_worktree, &["status", "--porcelain"]),
        "",
        "the model-free scaffold leaves the candidate worktree clean"
    );

    // The ordinary missing-change path reaches its own planner: the scaffold
    // is committed and the bounded planning conversation is dispatched
    // through the controller before any implementation. Its generated
    // assignment passes the same native structured contract dispatch uses.
    assert_eq!(
        status["candidate"]["planner_attempt"], "planner-1",
        "the standard missing-change scenario dispatches its planner: {status}"
    );
    assert!(
        status["candidate"]["planning_receipt"].is_null(),
        "no planning receipt exists before the planner settles: {status}"
    );
    let cursor = fixture.cursor();
    let attempts = cursor["attempts"].as_array().unwrap();
    let planner = attempts
        .iter()
        .find(|attempt| attempt["id"] == "planner-1")
        .cloned()
        .expect("the planning dispatch recorded its attempt");
    assert_eq!(planner["role"], "planner", "{planner}");
    assert_eq!(
        planner["state"], "failed",
        "the fixture launcher refuses only before submission: {planner}"
    );
    assert!(
        attempts
            .iter()
            .all(|attempt| attempt["role"] != "implementer"),
        "no implementation conversation starts before the change qualifies: {cursor}"
    );
    assert_dispatched_conversation(&fixture, "planner-1", AttemptRole::Planner);
    let planner_assignment = fixture.run.join("assignments/planner-1.json");
    assert!(
        planner_assignment.is_file(),
        "the planning dispatch wrote its bounded assignment: {planner_assignment:?}"
    );
    // The native model-free check loads that exact assignment through the
    // same validator a dispatch uses; the slot is a registered worktree of
    // the fixture project holding the scaffold revision the planner sees.
    slot_worktree(
        &candidate_worktree,
        &fixture.root.join("proj-new-candidate-wt1"),
        &scaffold_revision,
    );
    let check = fixture.assignment_check(1, &planner_assignment);
    assert!(check.status.success(), "{}", text(&check));
    assert!(
        text(&check).contains("executor assignment valid"),
        "{}",
        text(&check)
    );

    // Simulate the planning conversation: author the artifacts in a detached
    // slot worktree and commit them.
    let planner_slot = fixture.root.join("planner-slot");
    slot_worktree(&candidate_worktree, &planner_slot, &scaffold_revision);
    let change = planner_slot.join("openspec/changes/add-narrow-context");
    fs::create_dir_all(change.join("specs/synthetic")).unwrap();
    fs::write(
        change.join("proposal.md"),
        "## Why\n\nThe cold start loads more context than the task needs.\n",
    )
    .unwrap();
    fs::write(
        change.join("design.md"),
        "## Context\n\nNarrow the default context exposure.\n",
    )
    .unwrap();
    fs::write(
        change.join("tasks.md"),
        "## 1. Work\n\n- [ ] 1.1 Narrow the default exposure.\n",
    )
    .unwrap();
    fs::write(
        change.join("specs/synthetic/spec.md"),
        "## ADDED Requirements\n\n### Requirement: Narrow exposure\n\nThe system SHALL expose a narrower default context.\n\n#### Scenario: Synthetic case\n\n- **WHEN** the probe runs\n- **THEN** it reports success\n",
    )
    .unwrap();
    commit_all(&planner_slot, "author the change");
    let planner_revision = head(&planner_slot);
    let receipt = fixture.run.join("planner-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-planner-1",
        "gen-1",
        "completed",
        Some(0),
    );
    let result = fixture.run.join("planner-result.txt");
    fs::write(&result, "change authored\n").unwrap();
    // The dispatch recorded planner-1 before its surface failed; overwrite it
    // with the accepted, completed conversation the dispatcher would record.
    replace_attempt(
        &fixture,
        attempt_json(
            "planner-1",
            "planner",
            "workflow-fixture-planner-1",
            "gen-1",
            &receipt,
            Some(&result),
            Some(&planner_slot),
            "started",
        ),
    );

    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        status["candidate"]["planning_receipt"].is_string(),
        "{status}"
    );
    assert_eq!(
        head(&candidate_worktree),
        planner_revision,
        "the planning commit advanced the candidate branch"
    );
    let artifact = fs::read(
        candidate_worktree.join("openspec/changes/add-narrow-context/specs/synthetic/spec.md"),
    )
    .unwrap();
    assert!(!artifact.is_empty());

    // Simulate the implementation conversation and confirm candidate-ready.
    let implementer_slot = fixture.root.join("new-candidate-slot");
    slot_worktree(&candidate_worktree, &implementer_slot, &planner_revision);
    fs::write(
        implementer_slot.join("crates/one/src/lib.rs"),
        "// narrowed exposure\n",
    )
    .unwrap();
    commit_all(&implementer_slot, "implement narrow context");
    let implementer_revision = head(&implementer_slot);
    let receipt = fixture.run.join("new-implementer-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-implementer-1",
        "gen-1",
        "completed",
        Some(0),
    );
    let result = fixture.run.join("new-implementer-result.txt");
    fs::write(&result, "checks: synthetic ok\n").unwrap();
    replace_attempt(
        &fixture,
        attempt_json(
            "implementer-1",
            "implementer",
            "workflow-fixture-implementer-1",
            "gen-1",
            &receipt,
            Some(&result),
            Some(&implementer_slot),
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "candidate-ready", "{status}");
    assert_eq!(
        status["candidate"]["revision"], implementer_revision,
        "{status}"
    );
    assert_eq!(head(&candidate_worktree), implementer_revision);
    assert!(
        fixture
            .bd_comments(&candidate)
            .contains(&implementer_revision),
        "the admitted card records the validated implementation"
    );
}

#[test]
fn incomplete_planner_artifacts_block_implementation() {
    let fixture = Fixture::new("incomplete-plan");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fixture.start();
    let report = json!({
        "schema": 1,
        "candidates": [{
            "mechanism": "narrow-context",
            "conditions": "cold-start",
            "observation": locator,
            "predicted": "smaller resident context",
            "counterexample": "rare fallback needs the full context",
            "acceptance": "the independent oracle passes",
            "alternatives": "no change, reuse of the existing route, simplification and subtraction leave the resident context unchanged",
            "spec": "add-incomplete",
            "basis": locator,
            "treatment": "addition",
            "evidence": [{"locator": locator, "kind": "observed"}],
            "selection": selection_json(),
            "next_check": null,
        }],
        "idle_reason": null,
    });
    seed_investigator_report(&fixture, &report);
    fake_launcher(&fixture);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let candidate = fixture.status_json()["candidate"]["hypothesis"]
        .as_str()
        .expect("a new candidate card was admitted")
        .to_owned();
    let candidate_worktree = fixture.candidate_worktree(&candidate);
    let scaffold_revision = head(&candidate_worktree);

    // The planning conversation returns a change whose requirements exist but
    // whose predeclared acceptance section is missing: implementation must
    // stay undispatched.
    let planner_slot = fixture.root.join("incomplete-planner-slot");
    slot_worktree(&candidate_worktree, &planner_slot, &scaffold_revision);
    let change = planner_slot.join("openspec/changes/add-incomplete");
    fs::create_dir_all(change.join("specs/synthetic")).unwrap();
    fs::write(change.join("proposal.md"), "## Why\n\nSomething.\n").unwrap();
    fs::write(change.join("design.md"), "## Context\n\nSomething.\n").unwrap();
    fs::write(
        change.join("tasks.md"),
        "## 1. Work\n\n- [ ] 1.1 Something.\n",
    )
    .unwrap();
    fs::write(
        change.join("specs/synthetic/spec.md"),
        "## ADDED Requirements\n\n### Requirement: Something\n\nThe system SHALL do something.\n\n#### Scenario: Other case\n\n- **WHEN** the probe runs\n- **THEN** it reports success\n",
    )
    .unwrap();
    commit_all(&planner_slot, "author an incomplete change");
    let receipt = fixture.run.join("incomplete-planner-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-planner-1",
        "gen-1",
        "completed",
        Some(0),
    );
    let result = fixture.run.join("incomplete-planner-result.txt");
    fs::write(&result, "authored\n").unwrap();
    replace_attempt(
        &fixture,
        attempt_json(
            "planner-1",
            "planner",
            "workflow-fixture-planner-1",
            "gen-1",
            &receipt,
            Some(&result),
            Some(&planner_slot),
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        status["condition"]
            .as_str()
            .unwrap()
            .contains("does not qualify"),
        "{status}"
    );
    assert_eq!(
        status["candidate"]["planning_receipt"],
        Value::Null,
        "{status}"
    );
    assert!(
        status["candidate"]["implementer_attempt"].is_null(),
        "no implementation conversation is dispatched for an unqualified change: {status}"
    );
    // The planner's authored work is retained on the candidate branch; only
    // the dependent implementation stays undispatched.
    let planner_revision = head(&planner_slot);
    assert_eq!(
        head(&candidate_worktree),
        planner_revision,
        "the authored planning work is preserved on the candidate branch"
    );
    let attempts = fixture.cursor()["attempts"].as_array().unwrap().len();
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts,
        "a repeated resume performs no new model work"
    );
}

#[test]
fn removal_candidate_waits_for_the_decision_and_blocks_on_withdrawal() {
    let fixture = Fixture::new("removal");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[
        ("evidence_root", json!(root)),
        (
            "removal",
            json!({"proposal": "proposal-alpha", "target": "target-beta"}),
        ),
    ]);
    // A ready surface isolates the removal gate: the visibility gate no
    // longer masks it.
    fake_launcher(&fixture);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    // The start attempt reached the pool and failed there; replace it with the
    // completed investigator conversation the dispatcher would have recorded.
    let result = fixture.run.join("investigator-result.json");
    fs::write(
        &result,
        serde_json::to_vec_pretty(&anchored_report(&locator)).unwrap(),
    )
    .unwrap();
    let receipt = fixture.run.join("investigator-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-investigator-1",
        "gen-1",
        "completed",
        Some(0),
    );
    replace_attempt(
        &fixture,
        attempt_json(
            "investigator-1",
            "investigator",
            "workflow-fixture-investigator-1",
            "gen-1",
            &receipt,
            Some(&result),
            None,
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["candidate"]["removal_required"], true, "{status}");
    assert_eq!(status["stage"], "planner", "{status}");
    assert_eq!(
        status["candidate"]["implementer_attempt"],
        Value::Null,
        "no implementation conversation is dispatched before the decision: {status}"
    );
    assert!(
        status["removal"]["gate"]
            .as_str()
            .unwrap()
            .contains("pending"),
        "{status}"
    );

    // The complete change states no reviewable proposal yet: the bounded
    // planning conversation authors it, and no decision is requested and no
    // removal is applied while it is missing.
    assert_eq!(
        status["candidate"]["planning_receipt"],
        Value::Null,
        "{status}"
    );
    assert!(
        !fixture
            .bd_comments(&fixture.card)
            .contains("removal-proposal v1"),
        "an unstated proposal is never recorded for a decision"
    );
    let candidate_worktree = fixture.candidate_worktree(&fixture.card);
    let base_source = source_content(&fixture.proj.join("crates/one/src/lib.rs"));
    assert_eq!(
        source_content(&candidate_worktree.join("crates/one/src/lib.rs")),
        base_source,
        "preparing the proposal applies no removal"
    );

    // Simulate the planning conversation stating the reviewable proposal; the
    // controller records it on the card before the decision is requested.
    author_removal_proposal(
        &fixture,
        &fixture.card,
        &removal_proposal_section("target-beta"),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        status["candidate"]["planning_receipt"].is_string(),
        "{status}"
    );
    assert_eq!(
        status["candidate"]["implementer_attempt"],
        Value::Null,
        "{status}"
    );
    let comments = fixture.bd_comments(&fixture.card);
    for needle in [
        "removal-proposal v1",
        "proposal=proposal-alpha",
        "target=target-beta",
        "evidence=outcome:cycle-1#task",
        "loss=rare-manual-recovery",
        "preview=preview:retained/unapplied-capability.diff",
    ] {
        assert!(comments.contains(needle), "{needle}: {comments}");
    }
    let request = status["dispatch"]["reason"].as_str().unwrap_or_default();
    assert!(
        request.contains("no removal decision")
            && request.contains("proposal=proposal-alpha")
            && request.contains("target=target-beta"),
        "the recorded proposal is available before the decision request: {status}"
    );
    assert_eq!(
        source_content(&candidate_worktree.join("crates/one/src/lib.rs")),
        base_source,
        "no removal is applied before the decision"
    );

    // Record the informed decision; the dependent implementation becomes
    // eligible and reaches the dispatch owner.
    let decided = fixture.feedback(&[
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
        "rare-manual-recovery",
    ]);
    assert!(decided.status.success(), "{}", text(&decided));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        status["removal"]["gate"]
            .as_str()
            .unwrap()
            .contains("authorized"),
        "{status}"
    );
    assert!(
        status["candidate"]["implementer_attempt"].is_string(),
        "the approved removal lets the implementation dispatch proceed: {status}"
    );
    // The unchanged approval covers the later resumes without another
    // proposal or decision ritual.
    assert_eq!(
        fixture
            .bd_comments(&fixture.card)
            .matches("removal-proposal v1")
            .count(),
        1,
        "the reviewed proposal is not rewritten on resume"
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        status["removal"]["gate"]
            .as_str()
            .unwrap()
            .contains("authorized"),
        "{status}"
    );
    assert_eq!(
        fixture
            .bd_comments(&fixture.card)
            .matches("removal-decision v1")
            .count(),
        1,
        "no repeated approval request for the unchanged scope"
    );

    // Withdrawing the approval blocks the dependent removal effect again.
    let withdrawn = fixture.feedback(&[
        "removal-decide",
        "--item",
        &fixture.card,
        "--decision",
        "withdraw",
        "--proposal",
        "proposal-alpha",
        "--target",
        "target-beta",
    ]);
    assert!(withdrawn.status.success(), "{}", text(&withdrawn));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        status["condition"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase()
            .contains("withdrawn"),
        "{status}"
    );
    assert_eq!(
        status["removal"]["gate"].as_str().unwrap(),
        "approval withdrawn",
        "{status}"
    );
}

#[test]
fn a_foreign_returned_revision_is_refused() {
    let fixture = Fixture::new("foreign-return");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fixture.start();
    seed_investigator_report(&fixture, &anchored_report(&locator));
    fixture.resume();
    let candidate_worktree = fixture.candidate_worktree(&fixture.card);

    // A checkout that is not a registered worktree of this repository is
    // refused before any branch movement.
    let before = head(&candidate_worktree);
    let foreign = fixture.root.join("foreign-repo");
    fs::create_dir_all(&foreign).unwrap();
    git(&foreign, &["init", "-q", "--initial-branch=main"]);
    git(&foreign, &["config", "user.email", "fixture@example.test"]);
    git(&foreign, &["config", "user.name", "Fixture"]);
    fs::write(foreign.join("lib.rs"), "// foreign\n").unwrap();
    commit_all(&foreign, "foreign work");
    let foreign_head = head(&foreign);
    let receipt = fixture.run.join("foreign-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-implementer-1",
        "gen-1",
        "completed",
        Some(0),
    );
    push_attempt(
        &fixture,
        attempt_json(
            "implementer-1",
            "implementer",
            "workflow-fixture-implementer-1",
            "gen-1",
            &receipt,
            None,
            Some(&foreign),
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_ne!(status["phase"], "candidate-ready", "{status}");
    assert!(
        status["condition"]
            .as_str()
            .unwrap()
            .contains("not a registered worktree"),
        "{status}"
    );
    assert_eq!(head(&candidate_worktree), before);
    assert_ne!(head(&candidate_worktree), foreign_head);
}

#[test]
fn the_candidate_ready_receipt_hash_is_retained() {
    // The retained result digest is part of the durable evidence the parent
    // measured-pair owner consumes; confirm it is recorded beside the
    // candidate.
    let fixture = Fixture::new("retained");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fixture.start();
    seed_investigator_report(&fixture, &anchored_report(&locator));
    fixture.resume();
    let candidate_worktree = fixture.candidate_worktree(&fixture.card);
    let slot = fixture.root.join("retained-slot");
    slot_worktree(&candidate_worktree, &slot, &head(&candidate_worktree));
    fs::write(slot.join("crates/one/src/lib.rs"), "// retained\n").unwrap();
    commit_all(&slot, "retained implementation");
    let receipt = fixture.run.join("retained-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-implementer-1",
        "gen-1",
        "completed",
        Some(0),
    );
    let result = fixture.run.join("retained-result.txt");
    let report = "checks: cargo test --locked passed\n";
    fs::write(&result, report).unwrap();
    push_attempt(
        &fixture,
        attempt_json(
            "implementer-1",
            "implementer",
            "workflow-fixture-implementer-1",
            "gen-1",
            &receipt,
            Some(&result),
            Some(&slot),
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "candidate-ready", "{status}");
    let retained = status["candidate"]["result"].as_str().unwrap();
    let retained_path = PathBuf::from(retained);
    assert!(retained_path.is_file(), "{status}");
    let bytes = fs::read(&retained_path).unwrap();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        report,
        "the retained execution evidence is the conversation's terminal result"
    );
    assert_eq!(hash_bytes(&bytes), hash_bytes(report.as_bytes()));
    let receipt_sha = status["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == "implementer-1")
        .and_then(|attempt| attempt["retained_receipt_sha256"].as_str())
        .expect("the terminal receipt is retained with its digest");
    assert_eq!(receipt_sha.len(), 64, "{status}");
}

/// The standard scenario's missing change, as the investigator report
/// proposes it: a change that does not exist under the run's planning root.
fn missing_change_report(locator: &str) -> Value {
    json!({
        "schema": 1,
        "candidates": [{
            "mechanism": "narrow-context",
            "conditions": "cold-start",
            "observation": locator,
            "predicted": "smaller resident context",
            "counterexample": "rare fallback needs the full context",
            "acceptance": "the independent oracle passes",
            "alternatives": "no change, reuse of the existing route, simplification and subtraction leave the resident context unchanged",
            "spec": "add-narrow-context",
            "basis": locator,
            "treatment": "addition",
            "evidence": [{"locator": locator, "kind": "observed"}],
            "selection": selection_json(),
            "next_check": null,
        }],
        "idle_reason": null,
    })
}

#[test]
fn a_scope_covering_its_own_planning_artifacts_still_refuses_and_keeps_the_scaffold() {
    let fixture = Fixture::new("planning-overlap");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[
        ("evidence_root", json!(root)),
        ("writable_scope", json!(["openspec"])),
    ]);
    fixture.start();
    seed_investigator_report(&fixture, &missing_change_report(&locator));
    fake_launcher(&fixture);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    let candidate = status["candidate"]["hypothesis"]
        .as_str()
        .expect("the missing change's card was admitted")
        .to_owned();
    assert_eq!(
        status["candidate"]["change"], "add-narrow-context",
        "{status}"
    );
    // A declared scope that covers the candidate's own planning change is
    // still real control-state overlap: the planner is never dispatched.
    assert_eq!(status["phase"], "idle", "{status}");
    assert!(
        status["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("planning artifacts"),
        "the controller still refuses a scope over its own planning change: {status}"
    );
    assert!(status["candidate"]["planner_attempt"].is_null(), "{status}");
    let cursor = fixture.cursor();
    assert!(
        cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|attempt| attempt["role"] != "planner"),
        "no planner is dispatched while the overlap stands: {cursor}"
    );
    // The model-free scaffold is the controller's own committed work: the
    // refusal never loses it.
    let candidate_worktree = fixture.candidate_worktree(&candidate);
    assert!(
        candidate_worktree
            .join("openspec/changes/add-narrow-context/.openspec.yaml")
            .is_file(),
        "the scaffolded change survives the refusal"
    );
    assert_ne!(
        head(&candidate_worktree),
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        "the scaffold commit stays on the candidate branch"
    );
}

#[test]
fn an_oracle_root_covering_the_candidate_allocation_still_refuses() {
    let fixture = Fixture::new("oracle-overlap");
    let (root, locator) = write_evidence_root(&fixture);
    // The declared independent acceptance input is an absolute directory that
    // contains every candidate allocation of this run: candidate writes could
    // reach it, so the gate must refuse instead of dispatching the planner.
    fixture.write_spec(&[
        ("evidence_root", json!(root)),
        ("oracle", json!(candidate_area(&fixture.run))),
    ]);
    fixture.start();
    seed_investigator_report(&fixture, &missing_change_report(&locator));
    fake_launcher(&fixture);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    let candidate = status["candidate"]["hypothesis"]
        .as_str()
        .expect("the missing change's card was admitted")
        .to_owned();
    assert_eq!(status["phase"], "idle", "{status}");
    assert!(
        status["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("independent oracle"),
        "acceptance inputs stay unreachable from candidate scopes: {status}"
    );
    assert!(status["candidate"]["planner_attempt"].is_null(), "{status}");
    let cursor = fixture.cursor();
    assert!(
        cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|attempt| attempt["role"] != "planner"),
        "no planner is dispatched while the oracle overlap stands: {cursor}"
    );
    assert!(
        fixture
            .candidate_worktree(&candidate)
            .join("openspec/changes/add-narrow-context/.openspec.yaml")
            .is_file(),
        "the scaffolded change survives the refusal"
    );
}

#[test]
fn a_legacy_allocation_inside_the_run_state_is_relocated_for_its_planner() {
    let fixture = Fixture::new("legacy-allocation");
    fixture.start();
    fake_launcher(&fixture);
    let card = fixture.admit_change("add-narrow-context");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let branch = format!("improve/workflow-fixture/{card}");
    // The pre-fix controller allocated the candidate inside the protected run
    // state and committed the model-free scaffold there before its supervisor
    // gate refused the planner.
    let legacy = fixture.legacy_candidate_worktree(&card);
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    git(
        &fixture.proj,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            legacy.to_str().unwrap(),
            &base,
        ],
    );
    let scaffold = openspec(
        &legacy,
        &[
            "new",
            "change",
            "add-narrow-context",
            "--schema",
            "spec-driven",
            "--json",
        ],
    );
    assert!(
        scaffold.status.success(),
        "openspec new: {}",
        text(&scaffold)
    );
    commit_all(&legacy, "scaffold OpenSpec change add-narrow-context");
    let scaffold_revision = head(&legacy);
    assert_ne!(scaffold_revision, base);

    let mut cursor = fixture.cursor();
    cursor["candidate"] = json!({
        "hypothesis": card,
        "change": "add-narrow-context",
        "removal_required": false,
        "removal_frozen": Value::Null,
        "worktree": {
            "source": fixture.proj,
            "path": legacy,
            "branch": branch,
            "base": base,
            "revision": scaffold_revision,
        },
        "planning_receipt": Value::Null,
        "planner_attempt": Value::Null,
        "implementer_attempt": Value::Null,
        "revision": Value::Null,
        "result": Value::Null,
    });
    // An unresolved planning attempt keeps its allocation: the legacy
    // worktree and its scaffold commit are left exactly where they are.
    cursor["attempts"]
        .as_array_mut()
        .unwrap()
        .push(attempt_json(
            "planner-1",
            "planner",
            "workflow-fixture-planner-1",
            "gen-1",
            &fixture.run.join("missing-planner-receipt.json"),
            None,
            Some(&legacy),
            "started",
        ));
    cursor["candidate"]["planner_attempt"] = json!("planner-1");
    fixture.write_cursor(&cursor);

    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        legacy.is_dir(),
        "the unresolved allocation is left untouched: {status}"
    );
    assert!(
        !fixture.candidate_worktree(&card).exists(),
        "an unresolved attempt relocates nothing: {status}"
    );
    assert_eq!(head(&legacy), scaffold_revision, "{status}");

    // With the attempt reconciled away, the inactive, clean allocation at its
    // recorded revision is relocated through Git's own worktree move: the
    // branch, revision and scaffold commit survive and the ordinary planner
    // path proceeds from the owner-assigned location.
    let mut cursor = fixture.cursor();
    cursor["attempts"] = json!([]);
    cursor["candidate"]["planner_attempt"] = Value::Null;
    cursor["condition"] = Value::Null;
    fixture.write_cursor(&cursor);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    let relocated = fixture.candidate_worktree(&card);
    assert!(
        !legacy.exists(),
        "the legacy allocation was relocated out of the run state: {status}"
    );
    assert!(relocated.is_dir(), "{status}");
    assert_eq!(
        head(&relocated),
        scaffold_revision,
        "the scaffold commit survives the relocation: {status}"
    );
    assert!(
        relocated
            .join("openspec/changes/add-narrow-context/.openspec.yaml")
            .is_file(),
        "the scaffolded change moved with the allocation"
    );
    assert_eq!(
        status["candidate"]["planner_attempt"], "planner-1",
        "the relocated allocation reaches its own planner: {status}"
    );
    assert!(
        status["candidate"]["planning_receipt"].is_null(),
        "{status}"
    );
    let planner = fixture.cursor()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == "planner-1")
        .cloned()
        .expect("the planning dispatch recorded its attempt");
    assert_eq!(planner["role"], "planner", "{planner}");
    assert_eq!(planner["state"], "failed", "{planner}");
    assert!(
        fixture.run.join("assignments/planner-1.json").is_file(),
        "the planning dispatch wrote its assignment"
    );
    let relocated_text = relocated.to_string_lossy().into_owned();
    let recorded = fixture.bd_comments(&card);
    assert!(
        recorded.contains(&relocated_text)
            || recorded.contains(&relocated_text.replace('\\', "\\\\")),
        "the admitted card records the relocated allocation: {recorded}"
    );
}

fn move_registered_worktree(project: &Path, from: &Path, to: &Path) {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    git(
        project,
        &[
            "worktree",
            "move",
            from.to_str().unwrap(),
            to.to_str().unwrap(),
        ],
    );
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn cursor_worktree_key(cursor: &Value) -> String {
    path_key(Path::new(
        cursor["candidate"]["worktree"]["path"]
            .as_str()
            .unwrap_or_default(),
    ))
}

fn assignment_files(run: &Path) -> Vec<String> {
    let dir = run.join("assignments");
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut names = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn comments_record(comments: &str, path: &Path) -> bool {
    let text = path.to_string_lossy();
    comments.contains(text.as_ref()) || comments.contains(&text.replace('\\', "\\\\"))
}

/// One preserved worktree at the natural owner-assigned candidate location,
/// exactly as an interrupted earlier allocation of this hypothesis leaves it.
fn preserved_candidate_worktree(fixture: &Fixture, branch: &str) -> PathBuf {
    let path = fixture.candidate_worktree(&fixture.card);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    git(
        &fixture.proj,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            path.to_str().unwrap(),
            &fixture.base_revision(),
        ],
    );
    path
}

/// One replacement worktree at the next owner-assigned candidate location,
/// exactly as a crash between allocation and the cursor save leaves it: a
/// clean owned checkout of the frozen base on its own dedicated branch.
fn replacement_candidate_worktree(fixture: &Fixture, branch: &str) -> PathBuf {
    let path = fixture.replacement_candidate_worktree(&fixture.card);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    git(
        &fixture.proj,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            path.to_str().unwrap(),
            &fixture.base_revision(),
        ],
    );
    path
}

/// The Git move can finish before the board publication and cursor save.
/// Allocation has already published the pre-scaffold base revision; planning
/// advanced the cursor to the scaffold commit without republishing. Resume
/// must adopt that registered scaffold identity, and must leave every
/// ambiguous state unmodified without replaying a model attempt.
#[test]
fn an_interrupted_relocation_reconciles_only_the_exact_registered_identity() {
    let fixture = Fixture::new("reloc-gap");
    fixture.start();
    fake_launcher(&fixture);
    let card = fixture.admit_change("add-narrow-context");
    let spec: Value = serde_json::from_slice(&fs::read(&fixture.spec).unwrap()).unwrap();
    let base = spec["base_revision"]
        .as_str()
        .expect("the run spec records its frozen base")
        .to_owned();
    let branch = format!("improve/workflow-fixture/{card}");
    let legacy = fixture.legacy_candidate_worktree(&card);
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    git(
        &fixture.proj,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            legacy.to_str().unwrap(),
            &base,
        ],
    );
    let scaffold = openspec(
        &legacy,
        &[
            "new",
            "change",
            "add-narrow-context",
            "--schema",
            "spec-driven",
            "--json",
        ],
    );
    assert!(
        scaffold.status.success(),
        "openspec new: {}",
        text(&scaffold)
    );
    commit_all(&legacy, "scaffold OpenSpec change add-narrow-context");
    let scaffold_revision = head(&legacy);
    assert_ne!(scaffold_revision, base);
    let destination = fixture.candidate_worktree(&card);
    let held = fixture.root.join("held-worktree");
    let original_error = "publication-stopped-before-cursor-save";

    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("planning");
    cursor["condition"] = json!(original_error);
    cursor["candidate"] = json!({
        "hypothesis": card,
        "change": "add-narrow-context",
        "removal_required": false,
        "removal_frozen": Value::Null,
        "worktree": {
            "source": fixture.proj,
            "path": legacy,
            "branch": branch,
            "base": base,
            "revision": scaffold_revision,
        },
        "planning_receipt": Value::Null,
        "planner_attempt": Value::Null,
        "implementer_attempt": Value::Null,
        "revision": Value::Null,
        "result": Value::Null,
    });
    cursor["attempts"]
        .as_array_mut()
        .unwrap()
        .push(attempt_json(
            "planner-1",
            "planner",
            "workflow-fixture-planner-1",
            "gen-1",
            &fixture.run.join("missing-planner-receipt.json"),
            None,
            Some(&legacy),
            "started",
        ));
    cursor["candidate"]["planner_attempt"] = json!("planner-1");
    fixture.write_cursor(&cursor);
    let recorded = fixture.feedback(&[
        "hypothesis-implement",
        "--item",
        &card,
        "--role",
        "candidate",
        "--branch",
        &branch,
        "--base",
        &base,
        "--revision",
        &base,
        "--worktree",
        legacy.to_str().unwrap(),
    ]);
    assert!(
        recorded.status.success(),
        "pre-move board record: {}",
        text(&recorded)
    );
    let allocation_record = fixture.bd_comments(&card);
    assert!(
        allocation_record.contains(&format!("revision={base} ")),
        "allocation publishes the pre-scaffold base revision: {allocation_record}"
    );
    assert!(
        !allocation_record.contains(&format!("revision={scaffold_revision}")),
        "the board must still be the allocation publication, not the scaffold revision: {allocation_record}"
    );
    move_registered_worktree(&fixture.proj, &legacy, &destination);
    assert!(
        !legacy.exists(),
        "the fault state has already moved the worktree"
    );
    assert_eq!(head(&destination), scaffold_revision);

    // An in-flight attempt keeps the stale cursor and does not adopt the move
    // or open another model attempt.
    let before_active = git_output(&fixture.proj, &["worktree", "list", "--porcelain"]);
    let before_assignments = assignment_files(&fixture.run);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let cursor = fixture.cursor();
    assert_eq!(cursor_worktree_key(&cursor), path_key(&legacy), "{cursor}");
    assert_eq!(
        cursor["attempts"].as_array().unwrap().len(),
        1,
        "the unresolved attempt was replayed: {cursor}"
    );
    assert_eq!(cursor["attempts"][0]["id"], "planner-1", "{cursor}");
    assert_eq!(
        assignment_files(&fixture.run),
        before_assignments,
        "an unresolved attempt must not dispatch"
    );
    assert_eq!(head(&destination), scaffold_revision);
    assert_eq!(
        git_output(&fixture.proj, &["worktree", "list", "--porcelain"]),
        before_active
    );
    assert!(
        !comments_record(&fixture.bd_comments(&card), &destination),
        "an unresolved attempt must not publish the destination"
    );

    let mut cursor = fixture.cursor();
    cursor["attempts"] = json!([]);
    cursor["candidate"]["planner_attempt"] = Value::Null;
    cursor["condition"] = Value::Null;
    cursor["phase"] = json!("planning");
    fixture.write_cursor(&cursor);

    // A dirty destination is preserved and not adopted.
    fs::write(destination.join("dirty-preserved.txt"), "keep\n").unwrap();
    let before_dirty = git_output(&fixture.proj, &["worktree", "list", "--porcelain"]);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        destination.join("dirty-preserved.txt").is_file(),
        "dirty work was reset or deleted: {}",
        text(&resume)
    );
    assert_eq!(head(&destination), scaffold_revision);
    assert_eq!(
        cursor_worktree_key(&fixture.cursor()),
        path_key(&legacy),
        "{}",
        text(&resume)
    );
    assert_eq!(
        git_output(&fixture.proj, &["worktree", "list", "--porcelain"]),
        before_dirty
    );
    assert!(text(&resume).contains("not adopted"), "{}", text(&resume));
    fs::remove_file(destination.join("dirty-preserved.txt")).unwrap();

    // Identity mismatches are refused without resetting the registered tree.
    for (field, wrong) in [
        ("revision", base.as_str()),
        ("branch", "improve/not-recorded"),
        ("base", "not-a-recorded-base"),
    ] {
        let mut cursor = fixture.cursor();
        cursor["candidate"]["worktree"]["path"] = json!(legacy);
        cursor["candidate"]["worktree"]["branch"] = json!(branch);
        cursor["candidate"]["worktree"]["base"] = json!(base);
        cursor["candidate"]["worktree"]["revision"] = json!(scaffold_revision);
        cursor["candidate"]["worktree"][field] = json!(wrong);
        cursor["candidate"]["planner_attempt"] = Value::Null;
        cursor["attempts"] = json!([]);
        cursor["phase"] = json!("planning");
        cursor["condition"] = Value::Null;
        fixture.write_cursor(&cursor);
        let before = git_output(&fixture.proj, &["worktree", "list", "--porcelain"]);
        let resume = fixture.resume();
        assert!(resume.status.success(), "{field}: {}", text(&resume));
        assert_eq!(head(&destination), scaffold_revision, "{field}");
        assert_eq!(
            cursor_worktree_key(&fixture.cursor()),
            path_key(&legacy),
            "{field}: {}",
            text(&resume)
        );
        assert_eq!(
            git_output(&fixture.proj, &["worktree", "list", "--porcelain"]),
            before,
            "{field}"
        );
        assert!(
            text(&resume).contains("not adopted") || text(&resume).contains("not the recorded"),
            "{field}: {}",
            text(&resume)
        );
    }

    // Both paths present stay untouched: the controller must not choose one.
    move_registered_worktree(&fixture.proj, &destination, &legacy);
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("both-present.txt"), "keep\n").unwrap();
    let mut cursor = fixture.cursor();
    cursor["candidate"]["worktree"]["path"] = json!(legacy);
    cursor["candidate"]["worktree"]["branch"] = json!(branch);
    cursor["candidate"]["worktree"]["base"] = json!(base);
    cursor["candidate"]["worktree"]["revision"] = json!(scaffold_revision);
    cursor["candidate"]["planner_attempt"] = Value::Null;
    cursor["attempts"] = json!([]);
    cursor["phase"] = json!("planning");
    cursor["condition"] = Value::Null;
    fixture.write_cursor(&cursor);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        destination.join("both-present.txt").is_file(),
        "{}",
        text(&resume)
    );
    assert!(legacy.join(".git").exists() || legacy.join(".git").is_file());
    assert_eq!(head(&legacy), scaffold_revision);
    assert_eq!(cursor_worktree_key(&fixture.cursor()), path_key(&legacy));
    fs::remove_file(destination.join("both-present.txt")).unwrap();
    fs::remove_dir(&destination).unwrap();

    // A foreign directory at the owned destination is not adopted, and the
    // real worktree held elsewhere is not moved or deleted.
    move_registered_worktree(&fixture.proj, &legacy, &held);
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("foreign-marker.txt"), "keep\n").unwrap();
    let before_foreign = git_output(&fixture.proj, &["worktree", "list", "--porcelain"]);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        destination.join("foreign-marker.txt").is_file(),
        "{}",
        text(&resume)
    );
    assert_eq!(head(&held), scaffold_revision);
    assert_eq!(cursor_worktree_key(&fixture.cursor()), path_key(&legacy));
    assert_eq!(
        git_output(&fixture.proj, &["worktree", "list", "--porcelain"]),
        before_foreign
    );
    assert!(text(&resume).contains("not adopted"), "{}", text(&resume));
    fs::remove_file(destination.join("foreign-marker.txt")).unwrap();
    fs::remove_dir(&destination).unwrap();
    move_registered_worktree(&fixture.proj, &held, &destination);

    // The verified move is reconciled without another move, a lost commit or
    // a replay of the already failed model attempt.
    let mut cursor = fixture.cursor();
    cursor["phase"] = json!("planning");
    cursor["condition"] = json!(original_error);
    cursor["attempts"] = json!([]);
    cursor["candidate"]["planner_attempt"] = Value::Null;
    cursor["candidate"]["worktree"]["path"] = json!(legacy);
    cursor["candidate"]["worktree"]["branch"] = json!(branch);
    cursor["candidate"]["worktree"]["base"] = json!(base);
    cursor["candidate"]["worktree"]["revision"] = json!(scaffold_revision);
    fixture.write_cursor(&cursor);
    let mut failed = attempt_json(
        "planner-failed",
        "planner",
        "workflow-fixture-planner-failed",
        "gen-1",
        &fixture.run.join("missing-planner-receipt.json"),
        None,
        Some(&destination),
        "failed",
    );
    failed["reason"] = json!("model attempt already failed; do not replay");
    push_attempt(&fixture, failed);
    let before_recovery = git_output(&fixture.proj, &["worktree", "list", "--porcelain"]);
    let before_assignments = assignment_files(&fixture.run);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert_eq!(
        git_output(&fixture.proj, &["worktree", "list", "--porcelain"]),
        before_recovery,
        "recovery moved or added a worktree: {}",
        text(&resume)
    );
    assert_eq!(head(&destination), scaffold_revision, "{}", text(&resume));
    assert!(
        destination
            .join("openspec/changes/add-narrow-context/.openspec.yaml")
            .is_file(),
        "the scaffold commit was lost"
    );
    let cursor = fixture.cursor();
    assert_eq!(
        cursor_worktree_key(&cursor),
        path_key(&destination),
        "{cursor}"
    );
    assert_eq!(
        cursor["candidate"]["worktree"]["branch"], branch,
        "{cursor}"
    );
    assert_eq!(cursor["candidate"]["worktree"]["base"], base, "{cursor}");
    assert_eq!(
        cursor["candidate"]["worktree"]["revision"], scaffold_revision,
        "{cursor}"
    );
    let attempts = cursor["attempts"].as_array().unwrap();
    assert_eq!(
        attempts.len(),
        1,
        "a model attempt was replayed: {attempts:?}"
    );
    assert_eq!(attempts[0]["id"], "planner-failed");
    assert_eq!(attempts[0]["state"], "failed");
    assert_eq!(
        attempts[0]["reason"],
        "model attempt already failed; do not replay"
    );
    assert_eq!(
        assignment_files(&fixture.run),
        before_assignments,
        "recovery dispatched a model assignment"
    );
    assert!(text(&resume).contains(original_error), "{}", text(&resume));
    assert!(
        text(&resume).contains("never resubmit"),
        "the failed model attempt was not retained as a blocked replay: {}",
        text(&resume)
    );
    let effects = cursor["effects"].as_array().unwrap();
    assert!(
        effects.iter().any(|effect| {
            effect["kind"] == "candidate-allocated"
                && effect["detail"]
                    .as_str()
                    .unwrap_or_default()
                    .contains(original_error)
                && effect["detail"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("reconciled interrupted relocation")
        }),
        "the original error is not inspectable in the cursor effects: {effects:?}"
    );
    let comments = fixture.bd_comments(&card);
    assert!(
        comments_record(&comments, &legacy),
        "the pre-move board identity disappeared: {comments}"
    );
    assert!(
        comments.contains(&format!("revision={base} ")),
        "recovery rewrote the allocation-time board record: {comments}"
    );
    assert!(
        comments_record(&comments, &destination),
        "the board was not repaired through its owner: {comments}"
    );
    assert!(
        comments.contains(&format!("revision={scaffold_revision} ")),
        "recovery did not publish the preserved scaffold revision: {comments}"
    );
}

/// The natural owner-assigned location can already hold preserved work from
/// an earlier interrupted allocation. The controller leaves it exactly as it
/// stands - files, branch and revision - and allocates the next owned worktree
/// from the run's exact committed base, binding that allocation to the Beads
/// card. The accepted mainline is untouched.
#[test]
fn a_preserved_dirty_allocation_is_left_intact_for_a_new_owned_allocation() {
    let fixture = Fixture::new("preserved-dirty");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));
    fake_launcher(&fixture);
    let base = fixture.base_revision();
    let branch = format!("improve/workflow-fixture/{}", fixture.card);
    let preserved = preserved_candidate_worktree(&fixture, &branch);
    fs::write(
        preserved.join("crates/one/src/lib.rs"),
        "// preserved work in progress\n",
    )
    .unwrap();

    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));

    // The ineligible location is left exactly as found: same branch, same
    // revision, and its uncommitted work is still present.
    assert!(preserved.is_dir());
    assert_eq!(
        head(&preserved),
        base,
        "the preserved revision is unchanged"
    );
    assert_eq!(
        git_output(&preserved, &["rev-parse", "--abbrev-ref", "HEAD"]),
        branch
    );
    assert_eq!(
        fs::read_to_string(preserved.join("crates/one/src/lib.rs")).unwrap(),
        "// preserved work in progress\n",
        "preserved local changes are not reset, cleaned or checked out"
    );

    // The next owned location was allocated instead, from the exact frozen
    // base and on its own dedicated branch.
    let replacement = fixture.replacement_candidate_worktree(&fixture.card);
    assert!(replacement.is_dir(), "the replacement allocation exists");
    assert_eq!(
        head(&replacement),
        base,
        "the replacement starts at the exact committed input"
    );
    assert_eq!(
        git_output(&replacement, &["rev-parse", "--abbrev-ref", "HEAD"]),
        format!("improve/workflow-fixture/{}-2", fixture.card)
    );
    let trees = git_output(&fixture.proj, &["worktree", "list", "--porcelain"])
        .replace('/', "\\")
        .to_ascii_lowercase();
    assert!(trees.contains(&path_key(&preserved)));
    assert!(trees.contains(&path_key(&replacement)));

    // The card and the run state bind the replacement branch/base/worktree.
    let comments = fixture.bd_comments(&fixture.card);
    assert!(
        comments_record(&comments, &replacement),
        "the card records the replacement worktree: {comments}"
    );
    assert!(
        comments.contains(&format!(
            "role=candidate branch=improve/workflow-fixture/{}-2",
            fixture.card
        )),
        "the card records the replacement branch: {comments}"
    );
    assert!(
        comments.contains(&format!("base={base} ")),
        "the card records the exact base: {comments}"
    );
    let report = fixture.status_json();
    assert_eq!(report["candidate"]["hypothesis"], fixture.card, "{report}");
    assert_eq!(
        report["candidate"]["branch"],
        format!("improve/workflow-fixture/{}-2", fixture.card),
        "{report}"
    );
    assert_eq!(
        path_key(Path::new(
            report["candidate"]["worktree"]
                .as_str()
                .expect("the replacement allocation is reported")
        )),
        path_key(&replacement),
        "{report}"
    );

    // The accepted mainline stays at the frozen base while candidate work
    // lives on its own branch.
    assert_eq!(head(&fixture.proj), base);
}

/// A preserved checkout can carry commits beyond the recorded revision. The
/// controller leaves that branch and its commits exactly where they are and
/// allocates the next owned worktree instead of adopting or resetting them.
#[test]
fn an_unpreserved_commit_beyond_the_base_is_left_intact_for_a_new_owned_allocation() {
    let fixture = Fixture::new("preserved-commit");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));
    fake_launcher(&fixture);
    let base = fixture.base_revision();
    let branch = format!("improve/workflow-fixture/{}", fixture.card);
    let preserved = preserved_candidate_worktree(&fixture, &branch);
    fs::write(
        preserved.join("crates/one/src/lib.rs"),
        "// unmerged previous work\n",
    )
    .unwrap();
    commit_all(&preserved, "preserve unmerged previous work");
    let preserved_revision = head(&preserved);
    assert_ne!(preserved_revision, base);

    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));

    // The unpreserved commits stay on their own branch; nothing is reset.
    assert_eq!(
        head(&preserved),
        preserved_revision,
        "the preserved commit is not reset"
    );
    assert_eq!(
        git_output(&preserved, &["rev-parse", "--abbrev-ref", "HEAD"]),
        branch
    );
    assert_eq!(
        git_output(
            &fixture.proj,
            &["rev-parse", &format!("refs/heads/{branch}")]
        ),
        preserved_revision
    );

    let replacement = fixture.replacement_candidate_worktree(&fixture.card);
    assert!(replacement.is_dir(), "a new owned allocation is used");
    assert_eq!(head(&replacement), base);
    assert_eq!(
        git_output(&replacement, &["rev-parse", "--abbrev-ref", "HEAD"]),
        format!("improve/workflow-fixture/{}-2", fixture.card)
    );
    let comments = fixture.bd_comments(&fixture.card);
    assert!(
        comments_record(&comments, &replacement),
        "the card records the replacement worktree: {comments}"
    );
    assert_eq!(head(&fixture.proj), base);
}

/// A Git operation in the preserved checkout (a live `index.lock`) makes it
/// busy. The controller does not wait on it, reset it or delete the lock; it
/// allocates the next owned worktree and leaves the busy one untouched.
#[test]
fn a_busy_allocation_is_left_intact_for_a_new_owned_allocation() {
    let fixture = Fixture::new("preserved-busy");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));
    fake_launcher(&fixture);
    let base = fixture.base_revision();
    let branch = format!("improve/workflow-fixture/{}", fixture.card);
    let preserved = preserved_candidate_worktree(&fixture, &branch);
    let lock = git_output(&preserved, &["rev-parse", "--git-path", "index.lock"]);
    let lock = PathBuf::from(lock);
    let lock = if lock.is_absolute() {
        lock
    } else {
        preserved.join(lock)
    };
    fs::write(&lock, "held by another Git operation\n").unwrap();

    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));

    // The busy checkout keeps its lock, revision and clean state.
    assert!(lock.is_file(), "the foreign Git lock is not removed");
    assert_eq!(head(&preserved), base);
    assert!(git_output(&preserved, &["status", "--porcelain"]).is_empty());

    let replacement = fixture.replacement_candidate_worktree(&fixture.card);
    assert!(replacement.is_dir(), "a new owned allocation is used");
    assert_eq!(head(&replacement), base);
    let comments = fixture.bd_comments(&fixture.card);
    assert!(
        comments_record(&comments, &replacement),
        "the card records the replacement worktree: {comments}"
    );
    assert_eq!(head(&fixture.proj), base);
}

/// An attempt whose receipt disappeared is reconciled to an explicit unknown
/// outcome. Its eligible preserved allocation at the owner-assigned path is
/// reused in place - never duplicated, reset or replayed - and dependent work
/// stays blocked until the unknown attempt is reconciled through its owner.
#[test]
fn an_unresolved_attempt_keeps_its_preserved_allocation_and_blocks_dependent_work() {
    let fixture = Fixture::new("unresolved-allocation");
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    fake_launcher(&fixture);
    let base = fixture.base_revision();
    let branch = format!("improve/workflow-fixture/{}", fixture.card);
    let preserved = preserved_candidate_worktree(&fixture, &branch);

    // The run still holds an unresolved planning attempt for this candidate
    // and no recorded allocation.
    let mut cursor = fixture.cursor();
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": fixture.change,
        "removal_required": false,
        "removal_frozen": Value::Null,
        "worktree": Value::Null,
        "planning_receipt": Value::Null,
        "planner_attempt": "planner-1",
        "implementer_attempt": Value::Null,
        "revision": Value::Null,
        "result": Value::Null,
    });
    assert!(cursor["attempts"].as_array().unwrap().is_empty());
    cursor["attempts"]
        .as_array_mut()
        .unwrap()
        .push(attempt_json(
            "planner-1",
            "planner",
            "workflow-fixture-planner-1",
            "gen-1",
            &fixture.run.join("missing-planner-receipt.json"),
            None,
            None,
            "started",
        ));
    fixture.write_cursor(&cursor);

    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_eq!(report["phase"], "blocked", "{report}");
    assert_eq!(report["dispatch"]["state"], "blocked", "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("unknown"),
        "the unknown attempt is reported with its reason: {report}"
    );
    assert_eq!(
        path_key(Path::new(
            report["candidate"]["worktree"]
                .as_str()
                .expect("the preserved allocation is reused")
        )),
        path_key(&preserved),
        "the eligible preserved allocation is reused in place: {report}"
    );
    assert_eq!(report["candidate"]["branch"], branch, "{report}");
    assert!(
        !fixture
            .replacement_candidate_worktree(&fixture.card)
            .exists(),
        "an unresolved attempt triggers no second allocation"
    );

    // The preserved tree is untouched and no conversation is dispatched while
    // the outcome stays unknown.
    assert_eq!(head(&preserved), base);
    assert!(git_output(&preserved, &["status", "--porcelain"]).is_empty());
    let after = fixture.cursor();
    assert_eq!(after["attempts"].as_array().unwrap().len(), 1, "{after}");
    assert_eq!(after["attempts"][0]["state"], "unknown", "{after}");
}

/// A crash between allocating a replacement worktree and saving its cursor
/// record leaves an eligible owned allocation on disk. The resume reuses that
/// exact allocation - branch, base revision and registration - without
/// allocating another location, while the preserved dirty neighbor at the
/// natural location stays untouched.
#[test]
fn an_eligible_preserved_allocation_is_reused_without_allocating_again() {
    let fixture = Fixture::new("preserved-eligible");
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    fake_launcher(&fixture);
    let base = fixture.base_revision();
    let natural_branch = format!("improve/workflow-fixture/{}", fixture.card);
    let natural = preserved_candidate_worktree(&fixture, &natural_branch);
    fs::write(
        natural.join("crates/one/src/lib.rs"),
        "// preserved dirty neighbor\n",
    )
    .unwrap();
    let replacement_branch = format!("improve/workflow-fixture/{}-2", fixture.card);
    let replacement = replacement_candidate_worktree(&fixture, &replacement_branch);

    let mut cursor = fixture.cursor();
    cursor["candidate"] = json!({
        "hypothesis": fixture.card,
        "change": fixture.change,
        "removal_required": false,
        "removal_frozen": Value::Null,
        "worktree": Value::Null,
        "planning_receipt": Value::Null,
        "planner_attempt": Value::Null,
        "implementer_attempt": Value::Null,
        "revision": Value::Null,
        "result": Value::Null,
    });
    fixture.write_cursor(&cursor);

    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_eq!(
        report["candidate"]["branch"], replacement_branch,
        "the eligible replacement branch is reused: {report}"
    );
    assert_eq!(
        path_key(Path::new(
            report["candidate"]["worktree"]
                .as_str()
                .expect("the reused allocation is reported")
        )),
        path_key(&replacement),
        "{report}"
    );
    assert!(
        !candidate_area(&fixture.run)
            .join(format!("{}-3", fixture.card))
            .exists(),
        "an eligible allocation is reused instead of allocating again"
    );
    assert_eq!(head(&replacement), base);
    assert_eq!(head(&natural), base);
    assert_eq!(
        fs::read_to_string(natural.join("crates/one/src/lib.rs")).unwrap(),
        "// preserved dirty neighbor\n",
        "the preserved neighbor is untouched by the reuse"
    );
}

#[test]
fn continuous_start_idles_without_a_model_call() {
    let fixture = Fixture::new("continuous-idle");
    fake_launcher(&fixture);
    let start = fixture.start_continuous();
    assert!(start.status.success(), "{}", text(&start));
    assert!(
        text(&start).contains("continuous supervision"),
        "{}",
        text(&start)
    );
    let report = fixture.status_json();
    assert_eq!(report["phase"], "idle", "{report}");
    assert_eq!(report["supervision"], "continuous", "{report}");
    assert_eq!(report["attempts"].as_array().unwrap().len(), 0, "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("no retained evidence"),
        "{report}"
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert_eq!(fixture.status_json()["phase"], "idle");
    assert_eq!(fixture.cursor()["attempts"].as_array().unwrap().len(), 0);
    assert!(
        !fixture
            .run
            .join("assignments")
            .read_dir()
            .unwrap()
            .any(|entry| entry.is_ok()),
        "an idle continuous loop writes no assignment"
    );
}

// ---------------------------------------------------------------------------
// Directed measurement: the declared scope gates the baseline direction.
// ---------------------------------------------------------------------------

/// One declared measurement scope: the hypothesis's existing targeted
/// measurement, stated in its own OpenSpec change under `## Measurement`. The
/// workload is an existing operation linked from that same change, never a
/// second hypothesis or change.
fn measurement_scope_value() -> Value {
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

/// The declared scope is explicit local run data beside the frozen spec.
fn write_measurement_scope(fixture: &Fixture, scope: &Value) {
    fs::create_dir_all(&fixture.run).unwrap();
    fs::write(
        fixture.run.join("measurement-scope.json"),
        serde_json::to_vec_pretty(scope).unwrap(),
    )
    .unwrap();
}

/// The minimal comparison declaration a measurement-gate fixture needs. It
/// passes the frozen run-input validation; real preparation then refuses the
/// missing policy before any measured dispatch, which keeps the gate isolated
/// from a full prepared pair.
fn comparison_declaration(fixture: &Fixture) -> Value {
    let upstream = fixture.root.join("upstream-client.exe");
    fs::write(&upstream, "synthetic client").unwrap();
    let state = fixture.root.join("runtime-state");
    fs::create_dir_all(&state).unwrap();
    let acceptance = fixture.root.join("acceptance-request.json");
    fs::write(&acceptance, "{}").unwrap();
    json!({
        "schema": 1,
        "specification": {
            "project": fixture.proj,
            "change": "add-workload",
            "store": Value::Null,
            "planning_root": fixture.proj,
        },
        "contract": {
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
        "workload_card": "bdcw-workload-card",
        "task": {
            "source": fixture.proj,
            "revision": git_output(&fixture.proj, &["rev-parse", "HEAD"]),
            "name": "workload-synthetic",
            "writable_scope": ["crates/one"],
        },
        "runtimes": {
            "state": state,
            "baseline_build": state.join("baseline-build"),
            "candidate_build": state.join("candidate-build"),
            "baseline_label": "H",
            "candidate_label": "H-A",
            "upstream": upstream,
            "client": {
                "runner": {
                    "endpoint": "http://127.0.0.1:45999/v1",
                    "model": "fixture-glyph-1",
                },
            },
        },
        "policy": fixture.run.join("comparison-policy.json"),
        "acceptance": {
            "request": acceptance,
            "request_sha256": "a".repeat(64),
        },
        "observation_inputs": [],
    })
}

/// One investigator report proposing a simplification (a removal treatment).
/// Grounded intake admits a new card for it and the dependent implementation
/// stays behind the user's informed removal decision.
fn simplification_report(locator: &str) -> Value {
    json!({
        "schema": 1,
        "candidates": [{
            "mechanism": "retire-unused-capability",
            "conditions": "owned-local-tool-runs",
            "observation": locator,
            "predicted": "less catalogue and instruction exposure",
            "counterexample": "a rare recovery use disappears",
            "acceptance": "the independent oracle passes",
            "spec": "openspec/changes/add-synthetic",
            "basis": locator,
            "treatment": {
                "simplification": {
                    "removal": {
                        "target": "capability-x",
                        "basis": {
                            "coverage": {
                                "interval": "90 days",
                                "tasks": "every owned local task",
                                "gaps": "none observed",
                                "lost_uses": "rare manual recovery",
                                "restoration": "restore from the pinned revision",
                                "consumption": "the retained observation records no consumption of capability-x in either arm",
                            },
                        },
                    },
                },
            },
            "evidence": [{"locator": locator, "kind": "observed"}],
            "selection": selection_json(),
            "next_check": null,
        }],
        "idle_reason": null,
    })
}

/// The reviewable removal proposal a hypothesis's own change states before the
/// user's decision: target and source references, the unapplied preview,
/// evidence and its gaps, measured versus predicted benefit, lost scenarios,
/// consumer/configuration/installation impact, alternatives, retained checks
/// and restoration.
fn removal_proposal_section(target: &str) -> String {
    [
        "## Removal proposal",
        "",
        &format!("- Target: {target}"),
        "- Source: openspec/changes/add-synthetic",
        "- Evidence: outcome:cycle-1#task",
        "- Gaps: no invocation telemetry covers the rare recovery path",
        "- Measured: not yet measured; the retained observation records no consumption in either arm",
        "- Predicted: less catalogue and instruction exposure on every accepted task",
        "- Loss: rare-manual-recovery",
        "- Lost scenarios: a manual recovery in a degraded environment loses its documented route",
        "- Impact: the installed catalogue, the owned configuration and the current installation",
        "- Alternatives: keep the capability or narrow its exposure instead of removing it",
        "- Retained checks: the independent oracle stays binding",
        "- Restoration: restore the capability from the pinned revision",
        "- Preview: preview:retained/unapplied-capability.diff",
        "",
    ]
    .join("\n")
}

/// Simulates the bounded planning conversation that states one removal
/// proposal section in the candidate's own change: a detached slot worktree
/// gains the section and commits it, and the recorded planner attempt is
/// replaced with the completed conversation the dispatcher would retain.
fn author_removal_proposal(fixture: &Fixture, candidate: &str, section: &str) -> String {
    let candidate_worktree = fixture.candidate_worktree(candidate);
    let revision = head(&candidate_worktree);
    let slot = fixture.root.join(format!("removal-planner-{candidate}"));
    slot_worktree(&candidate_worktree, &slot, &revision);
    let change = slot.join(format!("openspec/changes/{}", fixture.change));
    let design = fs::read_to_string(change.join("design.md")).unwrap();
    fs::write(change.join("design.md"), format!("{design}\n{section}")).unwrap();
    commit_all(&slot, "state the reviewable removal proposal");
    let returned = head(&slot);
    let receipt = fixture
        .run
        .join(format!("{candidate}-planner-receipt.json"));
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-planner-1",
        "gen-1",
        "completed",
        Some(0),
    );
    let result = fixture.run.join(format!("{candidate}-planner-result.txt"));
    fs::write(&result, "proposal authored\n").unwrap();
    replace_attempt(
        fixture,
        attempt_json(
            "planner-1",
            "planner",
            "workflow-fixture-planner-1",
            "gen-1",
            &receipt,
            Some(&result),
            Some(&slot),
            "started",
        ),
    );
    returned
}

/// The change directories of one project, excluding the archive.
fn change_dirs(project: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(project.join("openspec/changes"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir() && entry.file_name() != "archive")
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The latest recorded dispatch refusal of one cursor.
fn last_refusal(cursor: &Value) -> String {
    cursor["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|effect| effect["kind"] == "dispatch-refused")
        .filter_map(|effect| effect["detail"].as_str())
        .next_back()
        .unwrap_or_default()
        .to_owned()
}

/// Rewrites the candidate's own change so its proposal states (or no longer
/// states) the declared measurement scope section, committed on the candidate
/// branch.
fn commit_measurement_section(worktree: &Path, change: &str, section: &str) {
    fs::write(
        worktree.join(format!("openspec/changes/{change}/proposal.md")),
        format!("## Why\n\nSynthetic.\n\n{section}"),
    )
    .unwrap();
    commit_all(worktree, "state the declared measurement scope");
}

const MEASUREMENT_SECTION: &str = "## Measurement\n\nObserved problem: identical repeated reads waste accepted-task time. Investigation scope: reads at one frozen source revision. Measurement question: how much accepted-task time do they cost? Workload: the existing cargo build operation linked from this change. Evidence: the retained outcome record. Limits: one local machine and one frozen source revision.\n";

/// One predeclared experiment selection for the gate fixtures: a local
/// build/output treatment measured through the short real operation.
fn gate_selection(method: &str, claim: &str) -> ExperimentSelection {
    ExperimentSelection {
        method: ExperimentMethod::parse(method).expect("a known method"),
        claim: EffectPath::parse(claim).expect("a known claim path"),
        outcome: "the declared outcome measured through the real unit".to_owned(),
        rationale: "the chosen unit exercises the claimed mechanism".to_owned(),
        controls: "frozen inputs and the accepted baseline conditions".to_owned(),
        projection: "one bounded experiment with the retention cost staying bounded".to_owned(),
        baseline: "the accepted revision excluding the candidate edit".to_owned(),
        stopping:
            "stop after the declared attempts and escalate only for a named missing observation"
                .to_owned(),
    }
}

/// Writes the predeclared comparison policy beside the frozen run spec, with
/// the selection clause the controller resolves before dependent work.
fn write_selection_policy(fixture: &Fixture, method: &str, claim: &str) {
    fs::create_dir_all(&fixture.run).unwrap();
    fs::write(
        fixture.run.join("comparison-policy.json"),
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
            "uncertainty": format!(
                "unknown evidence stays inconclusive; {}",
                experiment_selection_clause(&gate_selection(method, claim))
            ),
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
}

/// Rewrites the candidate's own change so its proposal states the experiment
/// selection section, committed on the candidate branch.
fn commit_selection_section(worktree: &Path, change: &str, method: &str, claim: &str) {
    fs::write(
        worktree.join(format!("openspec/changes/{change}/proposal.md")),
        format!(
            "## Why\n\nSynthetic.\n\n## Experiment selection\n\n- Method: {method}\n- Claim: {claim}\n- Outcome: the declared outcome measured through the real unit\n- Rationale: the chosen unit exercises the claimed mechanism\n- Controls: frozen inputs and the accepted baseline conditions\n- Projection: one bounded experiment with the retention cost staying bounded\n- Baseline: the accepted revision excluding the candidate edit\n- Stopping: stop after the declared attempts and escalate only for a named missing observation\n"
        ),
    )
    .unwrap();
    commit_all(worktree, "state the predeclared experiment selection");
}

#[test]
fn a_predeclared_experiment_selection_is_frozen_before_implementation() {
    let fixture = Fixture::new("selection-gate");
    let (root, locator) = write_evidence_root(&fixture);
    write_selection_policy(&fixture, "real-operation", "local-operation");
    fixture.write_spec(&[
        ("evidence_root", json!(root)),
        (
            "local_runner",
            json!({
                "endpoint": "http://127.0.0.1:45999/v1",
                "model": "fixture-glyph-1",
            }),
        ),
        (
            "qualification",
            json!(fixture.root.join("qualification.json")),
        ),
        ("comparison", comparison_declaration(&fixture)),
    ]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let worktree = fixture.candidate_worktree(&fixture.card);
    assert!(worktree.is_dir(), "candidate worktree exists");

    // The change qualifies but does not state the predeclared selection: no
    // planning receipt is retained, no implementation is dispatched, and the
    // exact missing section is reported.
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let output = text(&resume);
    assert!(
        output.contains("does not yet state the predeclared experiment selection"),
        "{output}"
    );
    assert!(
        output.contains("## Experiment selection"),
        "the planning brief requires the exact heading: {output}"
    );
    assert!(
        !fixture.run.join("candidate-planning.json").exists(),
        "the planning receipt was retained without the selection"
    );
    let status = fixture.status_json();
    assert_ne!(status["phase"], "candidate-ready", "{status}");
    assert!(
        status["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|attempt| attempt["role"] != "implementer"),
        "no implementation is dispatched before the selection is frozen: {status}"
    );

    // The authored change states the predeclared selection: planning
    // completes with the frozen section identified.
    commit_selection_section(
        &worktree,
        &fixture.change,
        "real-operation",
        "local-operation",
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let output = text(&resume);
    assert!(
        output.contains(
            "states the predeclared selection method=real-operation claim=local-operation"
        ),
        "{output}"
    );
    assert!(
        fixture.run.join("candidate-planning.json").is_file(),
        "the planning receipt is retained once the selection is stated"
    );
}

#[test]
fn a_change_cannot_substitute_another_predeclared_selection() {
    let fixture = Fixture::new("selection-mismatch");
    let (root, locator) = write_evidence_root(&fixture);
    write_selection_policy(&fixture, "real-operation", "local-operation");
    fixture.write_spec(&[
        ("evidence_root", json!(root)),
        (
            "local_runner",
            json!({
                "endpoint": "http://127.0.0.1:45999/v1",
                "model": "fixture-glyph-1",
            }),
        ),
        (
            "qualification",
            json!(fixture.root.join("qualification.json")),
        ),
        ("comparison", comparison_declaration(&fixture)),
    ]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let worktree = fixture.candidate_worktree(&fixture.card);

    // A different but individually sufficient method does not substitute for
    // the predeclared binding: the plan was fixed before results.
    commit_selection_section(&worktree, &fixture.change, "agent-task", "local-operation");
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let output = text(&resume);
    assert!(
        output.contains("but the run predeclares method=real-operation claim=local-operation"),
        "{output}"
    );
    assert!(
        !fixture.run.join("candidate-planning.json").exists(),
        "a substituted selection never authorizes implementation"
    );
    let status = fixture.status_json();
    assert!(
        status["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|attempt| attempt["role"] != "implementer"),
        "{status}"
    );
}

#[test]
fn a_declared_measurement_scope_gates_the_baseline_direction_and_rebinds_on_resume() {
    let fixture = Fixture::new("measured-gate");
    let (root, locator) = write_evidence_root(&fixture);
    write_measurement_scope(&fixture, &measurement_scope_value());
    fixture.write_spec(&[
        ("evidence_root", json!(root)),
        (
            "local_runner",
            json!({
                "endpoint": "http://127.0.0.1:45999/v1",
                "model": "fixture-glyph-1",
            }),
        ),
        (
            "qualification",
            json!(fixture.root.join("qualification.json")),
        ),
        ("comparison", comparison_declaration(&fixture)),
    ]);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    seed_investigator_report(&fixture, &anchored_report(&locator));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let worktree = fixture.candidate_worktree(&fixture.card);
    assert!(worktree.is_dir(), "candidate worktree exists");

    // Simulate the implementation conversation: a detached slot worktree of
    // the candidate branch with a committed, in-scope implementation.
    let slot = fixture.root.join("gate-slot");
    slot_worktree(&worktree, &slot, &head(&worktree));
    fs::write(
        slot.join("crates/one/src/lib.rs"),
        "// implemented before the measured pair\n",
    )
    .unwrap();
    commit_all(&slot, "implement the bounded output");
    let revision = head(&slot);
    let receipt = fixture.run.join("gate-implementer-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-implementer-1",
        "gen-1",
        "completed",
        Some(0),
    );
    push_attempt(
        &fixture,
        attempt_json(
            "implementer-1",
            "implementer",
            "workflow-fixture-implementer-1",
            "gen-1",
            &receipt,
            None,
            Some(&slot),
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_eq!(report["phase"], "candidate-ready", "{report}");
    assert_eq!(report["candidate"]["revision"], revision, "{report}");

    // The declared scope is not yet stated in the hypothesis's own change:
    // directing the baseline measurement is refused with the exact artifact.
    let measurement_receipt = fixture.run.join("measurement-receipt.json");
    assert!(
        !measurement_receipt.exists(),
        "no receipt exists before the gate runs"
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let report = fixture.status_json();
    assert_ne!(report["phase"], "baseline-attempt", "{report}");
    assert!(
        report["comparison"].is_null(),
        "the comparison owner was engaged before the gate: {report}"
    );
    assert!(
        report["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|attempt| attempt["role"] != "baseline"),
        "no baseline attempt is dispatched from an unstated scope: {report}"
    );
    assert_eq!(
        report["candidate"]["measurement_receipt"],
        Value::Null,
        "{report}"
    );
    assert!(!measurement_receipt.exists());
    let refusal = last_refusal(&fixture.cursor());
    assert!(
        refusal
            .contains("missing or empty measurement scope section in the linked OpenSpec artifact"),
        "{refusal}"
    );
    // The workload stays linked from the hypothesis's own change: no second
    // hypothesis or change is created for it.
    assert_eq!(
        change_dirs(&fixture.proj),
        vec![fixture.change.clone()],
        "the measurement workload forced a second change"
    );

    // The change now states the declared scope. The gate retains the receipt
    // before the measured-pair owner is engaged at all.
    commit_measurement_section(&worktree, &fixture.change, MEASUREMENT_SECTION);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let output = text(&resume);
    assert!(
        output.contains("measurement: change add-synthetic states the declared measurement scope"),
        "{output}"
    );
    assert!(
        output.contains("the predeclared comparison policy is unusable"),
        "the comparison owner was never engaged after the gate: {output}"
    );
    assert!(measurement_receipt.is_file(), "the receipt is retained");
    let cursor = fixture.cursor();
    let retained_path = cursor["candidate"]["measurement_receipt"]
        .as_str()
        .unwrap_or_default();
    assert_eq!(
        PathBuf::from(retained_path),
        measurement_receipt,
        "{cursor}"
    );
    let retained: Value = serde_json::from_slice(&fs::read(&measurement_receipt).unwrap()).unwrap();
    assert_eq!(
        retained["scope"]["workload"]["operation"], "cargo build -p example-reader",
        "{retained}"
    );
    assert_eq!(
        retained["specification"]["change"], "add-synthetic",
        "{retained}"
    );
    assert!(
        retained["artifacts"].as_object().unwrap().len() >= 2,
        "{retained}"
    );

    // Resume revalidates the retained receipt against the unchanged change
    // and never rewrites it.
    let before = fs::read(&measurement_receipt).unwrap();
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        text(&resume).contains("measurement: the retained receipt rebinds to change add-synthetic"),
        "{}",
        text(&resume)
    );
    assert_eq!(
        fs::read(&measurement_receipt).unwrap(),
        before,
        "revalidation rewrote the retained receipt"
    );

    // A changed declared scope blocks reuse of the retained measurement.
    let mut changed = measurement_scope_value();
    changed["measurement_question"] = json!("a different declared question");
    write_measurement_scope(&fixture, &changed);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        text(&resume).contains(
            "the declared measurement scope changed after the directed-measurement receipt was retained"
        ),
        "{}",
        text(&resume)
    );
    assert_eq!(
        fs::read(&measurement_receipt).unwrap(),
        before,
        "a changed declaration rewrote the retained receipt"
    );
    write_measurement_scope(&fixture, &measurement_scope_value());
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        text(&resume).contains("the retained receipt rebinds"),
        "{}",
        text(&resume)
    );

    // A change that no longer states the declared scope blocks reuse as well,
    // and restoring it rebinds the same retained receipt.
    commit_measurement_section(&worktree, &fixture.change, "");
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let output = text(&resume);
    assert!(
        output.contains("no longer rebinds to the hypothesis's own change"),
        "{output}"
    );
    assert!(output.contains("measurement scope section"), "{output}");
    assert_eq!(
        fs::read(&measurement_receipt).unwrap(),
        before,
        "drift rewrote the retained receipt"
    );
    commit_measurement_section(&worktree, &fixture.change, MEASUREMENT_SECTION);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    assert!(
        text(&resume).contains("the retained receipt rebinds"),
        "{}",
        text(&resume)
    );
}

#[test]
fn a_scaffolded_candidate_receives_the_declared_measurement_scope_in_its_planner_brief() {
    let fixture = Fixture::new("planner-scope");
    let (root, locator) = write_evidence_root(&fixture);
    write_measurement_scope(&fixture, &measurement_scope_value());
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fixture.start();
    seed_investigator_report(&fixture, &missing_change_report(&locator));
    fake_launcher(&fixture);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    let candidate = status["candidate"]["hypothesis"]
        .as_str()
        .expect("a new candidate card was admitted")
        .to_owned();
    assert_ne!(candidate, fixture.card, "{status}");
    assert_eq!(
        status["candidate"]["change"], "add-narrow-context",
        "{status}"
    );

    // The bounded planning brief carries the declared scope: the exact
    // declaration locator, the observed problem, question, limits, evidence
    // and the workload link, without inventing a second hypothesis or change
    // for the measurement workload.
    let assignment = fixture.run.join("assignments/planner-1.json");
    assert!(
        assignment.is_file(),
        "the planning dispatch wrote its brief"
    );
    let document: Value = serde_json::from_slice(&fs::read(&assignment).unwrap()).unwrap();
    let invariants: Vec<&str> = document["invariants"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|value| value.as_str())
        .collect();
    for needle in [
        "the initial change states the declared measurement scope under the exact heading '## Measurement' in proposal.md",
        "declared observed problem: identical repeated reads waste accepted-task time",
        "declared measurement question: how much accepted-task time do identical repeated reads cost?",
        "declared limits: one local machine and one frozen source revision",
        "declared workload operation: cargo build -p example-reader",
        "declared workload contract link: openspec/changes/add-synthetic/proposal.md#Measurement",
        "do not create a second hypothesis, card or OpenSpec change for the workload",
        "declared evidence reference: retained outcome record: repeated reads",
    ] {
        assert!(
            invariants.iter().any(|item| item.contains(needle)),
            "{needle}: {document}"
        );
    }
    assert!(
        document["acceptance"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value
                .as_str()
                .unwrap()
                .contains("states the declared measurement scope section '## Measurement'")),
        "{document}"
    );

    // The generated brief passes the same native structured contract a
    // dispatch uses, with the scaffold revision as the registered slot base.
    let candidate_worktree = fixture.candidate_worktree(&candidate);
    let scaffold_revision = head(&candidate_worktree);
    slot_worktree(
        &candidate_worktree,
        &fixture.root.join("proj-planner-scope-wt1"),
        &scaffold_revision,
    );
    let check = fixture.assignment_check(1, &assignment);
    assert!(check.status.success(), "{}", text(&check));
    assert!(
        text(&check).contains("executor assignment valid"),
        "{}",
        text(&check)
    );
}

#[test]
fn an_admitted_simplification_waits_for_the_informed_decision_before_implementation() {
    let fixture = Fixture::new("admitted-simplification");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fake_launcher(&fixture);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    // Replace the failed start conversation with the completed investigator
    // report whose candidate is a simplification treatment.
    let result = fixture.run.join("investigator-result.json");
    fs::write(
        &result,
        serde_json::to_vec_pretty(&simplification_report(&locator)).unwrap(),
    )
    .unwrap();
    let receipt = fixture.run.join("investigator-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-investigator-1",
        "gen-1",
        "completed",
        Some(0),
    );
    replace_attempt(
        &fixture,
        attempt_json(
            "investigator-1",
            "investigator",
            "workflow-fixture-investigator-1",
            "gen-1",
            &receipt,
            Some(&result),
            None,
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert_eq!(
        status["intake"]["outcomes"][0]["outcome"], "admitted",
        "{status}"
    );
    let candidate = status["candidate"]["hypothesis"]
        .as_str()
        .expect("the admitted card is selected")
        .to_owned();
    assert_ne!(candidate, fixture.card, "{status}");
    assert_eq!(status["candidate"]["removal_required"], true, "{status}");
    assert_eq!(
        status["candidate"]["implementer_attempt"],
        Value::Null,
        "no implementation conversation precedes the informed decision: {status}"
    );
    // The admitted candidate's complete change states no reviewable proposal
    // yet: planning stays incomplete, the planning conversation authors it,
    // and no decision is requested while it is missing.
    assert_eq!(status["stage"], "planner", "{status}");
    assert_eq!(
        status["candidate"]["planning_receipt"],
        Value::Null,
        "{status}"
    );
    assert!(
        !fixture
            .bd_comments(&candidate)
            .contains("removal-proposal v1"),
        "an unstated proposal is never recorded for a decision"
    );

    // Simulate the planning conversation stating the reviewable proposal; the
    // controller records it on the card before the decision is requested.
    author_removal_proposal(
        &fixture,
        &candidate,
        &removal_proposal_section("capability-x"),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        status["candidate"]["planning_receipt"].is_string(),
        "{status}"
    );
    assert_eq!(
        status["candidate"]["implementer_attempt"],
        Value::Null,
        "{status}"
    );
    let comments = fixture.bd_comments(&candidate);
    for needle in [
        "removal-proposal v1",
        "proposal=openspec/changes/add-synthetic",
        "target=capability-x",
        "evidence=outcome:cycle-1#task",
        "loss=rare-manual-recovery",
        "preview=preview:retained/unapplied-capability.diff",
    ] {
        assert!(comments.contains(needle), "{needle}: {comments}");
    }
    // A reviewable proposal alone does not authorize the removal effect: the
    // decision request names the exact reviewed proposal.
    let request = status["dispatch"]["reason"].as_str().unwrap_or_default();
    assert!(
        request.contains("no removal decision")
            && request.contains("proposal=openspec/changes/add-synthetic")
            && request.contains("target=capability-x"),
        "the recorded proposal is available before the decision request: {status}"
    );
    assert_eq!(
        source_content(
            &fixture
                .candidate_worktree(&candidate)
                .join("crates/one/src/lib.rs")
        ),
        source_content(&fixture.proj.join("crates/one/src/lib.rs")),
        "preparing the proposal applies no removal"
    );

    // The informed decision covers the experiment treatment; the dependent
    // implementation then reaches the dispatch owner.
    let decided = fixture.feedback(&[
        "removal-decide",
        "--item",
        &candidate,
        "--decision",
        "approve",
        "--proposal",
        "openspec/changes/add-synthetic",
        "--target",
        "capability-x",
        "--actions",
        "experiment",
        "--loss",
        "rare-manual-recovery",
    ]);
    assert!(decided.status.success(), "{}", text(&decided));
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    assert!(
        status["candidate"]["implementer_attempt"].is_string(),
        "the approved simplification dispatches its implementation: {status}"
    );
    assert!(
        status["removal"]["gate"]
            .as_str()
            .unwrap()
            .contains("authorized"),
        "{status}"
    );
    // The unchanged approval covers a later resume without another proposal
    // or decision ritual.
    let resumes = fixture.resume();
    assert!(resumes.status.success(), "{}", text(&resumes));
    let comments = fixture.bd_comments(&candidate);
    assert_eq!(
        comments.matches("removal-proposal v1").count(),
        1,
        "the reviewed proposal is not rewritten on resume"
    );
    assert_eq!(
        comments.matches("removal-decision v1").count(),
        1,
        "no repeated approval request for the unchanged scope"
    );
}

#[test]
fn a_removal_candidate_without_a_complete_proposal_is_not_presented_for_decision() {
    let fixture = Fixture::new("incomplete-removal-proposal");
    let (root, locator) = write_evidence_root(&fixture);
    fixture.write_spec(&[("evidence_root", json!(root))]);
    fake_launcher(&fixture);
    let start = fixture.start();
    assert!(start.status.success(), "{}", text(&start));
    // Replace the failed start conversation with the completed investigator
    // report whose candidate is a simplification treatment.
    let result = fixture.run.join("investigator-result.json");
    fs::write(
        &result,
        serde_json::to_vec_pretty(&simplification_report(&locator)).unwrap(),
    )
    .unwrap();
    let receipt = fixture.run.join("investigator-receipt.json");
    seed_bound_receipt(
        &receipt,
        "workflow-fixture-investigator-1",
        "gen-1",
        "completed",
        Some(0),
    );
    replace_attempt(
        &fixture,
        attempt_json(
            "investigator-1",
            "investigator",
            "workflow-fixture-investigator-1",
            "gen-1",
            &receipt,
            Some(&result),
            None,
            "started",
        ),
    );
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    let candidate = status["candidate"]["hypothesis"]
        .as_str()
        .expect("the admitted card is selected")
        .to_owned();
    assert_eq!(status["candidate"]["removal_required"], true, "{status}");

    // The planning conversation returns a change whose removal proposal is
    // missing the restoration clause: the reviewable proposal is incomplete,
    // so it is never recorded and no decision is requested from it.
    let incomplete = removal_proposal_section("capability-x")
        .lines()
        .filter(|line| !line.starts_with("- Restoration:"))
        .collect::<Vec<_>>()
        .join("\n");
    author_removal_proposal(&fixture, &candidate, &incomplete);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    let status = fixture.status_json();
    let condition = status["condition"].as_str().unwrap_or_default();
    assert!(
        condition.contains("does not state a recordable reviewable removal proposal"),
        "{status}"
    );
    assert!(condition.contains("Restoration:"), "{status}");
    assert_eq!(
        status["candidate"]["planning_receipt"],
        Value::Null,
        "{status}"
    );
    assert_eq!(
        status["candidate"]["implementer_attempt"],
        Value::Null,
        "{status}"
    );
    assert!(
        !fixture
            .bd_comments(&candidate)
            .contains("removal-proposal v1"),
        "an incomplete proposal is never recorded for a decision"
    );
    assert!(
        !fixture
            .bd_comments(&candidate)
            .contains("removal-decision v1"),
        "no decision is requested before the proposal is complete"
    );
    // The authored work is preserved and a repeated resume performs no new
    // model work.
    let attempts = fixture.cursor()["attempts"].as_array().unwrap().len();
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        attempts,
        "a repeated resume performs no new model work"
    );
}
