//! Native `codex-harness improve` planning/implementation workflow checks
//! over synthetic owned inputs: a real `bd` board, a real OpenSpec workspace,
//! real Git worktrees and private run state. The model conversations are
//! simulated through the same durable seam the controller recovery uses: a
//! dispatcher receipt with a terminal state is seeded and the controller
//! settles and consumes it on `resume`. No check here contacts a model, a
//! provider or a subscription.
#![cfg(windows)]

use harness_core::build_identity::hash_bytes;
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
        let mut document = json!({
            "schema": 1,
            "run": "workflow-fixture",
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
            "spec": "openspec/changes/add-synthetic",
            "basis": observation,
            "treatment": "addition",
            "evidence": [{"locator": observation, "kind": "observed"}],
            "next_check": null,
        }],
        "idle_reason": null,
    })
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
#[test]
fn an_ambiguous_terminal_message_is_refused_without_model_churn() {
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
    assert_eq!(report["phase"], "idle", "{report}");
    assert!(report["candidate"].is_null(), "{report}");
    assert!(
        report["condition"]
            .as_str()
            .unwrap()
            .contains("not a bounded schema-1 investigator report"),
        "{report}"
    );
    let cursor = fixture.cursor();
    assert!(cursor["intake"].is_null(), "{cursor}");
    assert_eq!(
        cursor["attempts"].as_array().unwrap().len(),
        baseline + 1,
        "ambiguity starts no model work: {cursor}"
    );

    // A repeated resume keeps refusing the same ambiguous message without
    // admitting anything or dispatching a fresh round.
    let again = fixture.resume();
    assert!(again.status.success(), "{}", text(&again));
    assert!(fixture.status_json()["candidate"].is_null());
    assert_eq!(
        fixture.cursor()["attempts"].as_array().unwrap().len(),
        baseline + 1
    );
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
    let report = json!({
        "schema": 1,
        "candidates": [{
            "mechanism": "bounded-output",
            "conditions": "local-tool-runs",
            "observation": locator,
            "predicted": "less repeated context loading",
            "counterexample": "diagnostics vanish on failure",
            "acceptance": "the independent oracle passes",
            "spec": format!("openspec/changes/{change}"),
            "basis": locator,
            "treatment": "addition",
            "evidence": [{"locator": locator, "kind": "observed"}],
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
            "spec": "add-incomplete",
            "basis": locator,
            "treatment": "addition",
            "evidence": [{"locator": locator, "kind": "observed"}],
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
    assert!(
        status["condition"].as_str().unwrap().contains("removal"),
        "{status}"
    );
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

    // Record the informed decision; the dependent implementation becomes
    // eligible and reaches the dispatch owner.
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
        "synthetic-loss",
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
        "--loss",
        "synthetic-loss",
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
            "spec": "add-narrow-context",
            "basis": locator,
            "treatment": "addition",
            "evidence": [{"locator": locator, "kind": "observed"}],
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
