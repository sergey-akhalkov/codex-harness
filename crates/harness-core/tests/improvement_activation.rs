//! Native owned temporary Git and Beads cases for benefit-gated mainline
//! integration and experimental-baseline activation. Every case uses real
//! `git` repositories, a real owned `bd` board and real native checker
//! processes; the activation case additionally installs the real prepared arm
//! through the existing runtime owner with compiled fixture stand-ins for the
//! upstream client and the installed launcher. All projects, boards, builds
//! and homes are synthetic fixtures inside the test's temporary directory; no
//! ambient account state, model call or live installation is touched.

use harness_core::benefit_gate::{
    DecisionDraft, DecisionOutcome, Publication, QualityOutcome, publish_decision,
};
use harness_core::board_feedback;
use harness_core::board_hypothesis::{
    self, Admission, BoundedHypothesis, BoundedRemovalDecision, BoundedRemovalProposal,
    HypothesisDraft, RemovalAction, RemovalDecisionDraft, RemovalDecisionKind,
    RemovalProposalDraft,
};
use harness_core::build_identity;
use harness_core::improvement_activation::{
    ActivationOutcome,
    ActivationOutcome::{Activated, Confirmed},
    ActivationRequest, Blocked, CheckSpec, IntegrationOutcome, IntegrationReceipt,
    IntegrationRequest, activate, integrate,
};
use harness_core::improvement_experiment::{
    Arm, ArmBinding, ExperimentBindings, prepare_home, prepare_variant,
};
use harness_core::improvement_loop::{BoardInputs, PublicationStage, RemovalScope, RunSpec};
use harness_core::improvement_policy::{
    Basis, ComparisonPolicy, DeclaredComparison, Objective, Overhead, PolicyDecision,
    PolicyEvaluation, RepeatedSelection, StoppingRule, evaluate,
};
use harness_core::improvement_runtime::{ArmRequest, ArmRuntime, install_arm};
use harness_core::improvement_spec::{ExperimentContract, Specification};
use harness_core::outcome_report::{MATCH_FIELDS, summarize_attempts};
use harness_core::task_worktree::{CandidateCheckout, allocate_candidate_checkout, frozen_copy};
use harness_core::{build_selection, improvement_loop};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
    time::Duration,
};

// ---------------------------------------------------------------------------
// Real owned allocations: git repositories, a Beads board and native fixtures.
// ---------------------------------------------------------------------------

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

fn git(cwd: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn rev(cwd: &Path) -> String {
    git(cwd, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn configure(cwd: &Path) {
    git(cwd, &["config", "user.email", "fixture@example.test"]);
    git(cwd, &["config", "user.name", "Fixture"]);
}

/// One owned board project: an initialized `bd` board whose hypothesis cards
/// are the decision owner. The Git repositories under test stay separate so
/// board writes never dirty the mainline fixture.
fn board_project(root: &Path) -> PathBuf {
    let project = root.join("board");
    fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "--initial-branch=main"]);
    configure(&project);
    let init = Command::new(bd_executable())
        .args([
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "bdct",
        ])
        .current_dir(&project)
        .output()
        .expect("bd init runs");
    assert!(
        init.status.success(),
        "bd init: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    project
}

fn admit_card(bd: &Path, project: &Path, mechanism: &str, conditions: &str) -> String {
    let bounded = BoundedHypothesis::try_from_draft(HypothesisDraft {
        mechanism: mechanism.to_owned(),
        conditions: conditions.to_owned(),
        observation: "synthetic-observation".to_owned(),
        predicted: "Synthetic predicted effect".to_owned(),
        counterexample: "Synthetic counterexample".to_owned(),
        acceptance: "Synthetic independent acceptance".to_owned(),
        spec: "openspec/changes/add-synthetic".to_owned(),
        basis: "synthetic-basis".to_owned(),
    })
    .unwrap();
    match board_hypothesis::admit_hypothesis(bd, project, &bounded, None).unwrap() {
        Admission::Created { id } => id,
        other => panic!("expected a created hypothesis card, got {other:?}"),
    }
}

fn comment(bd: &Path, project: &Path, item: &str, text: &str) {
    let out = Command::new(bd)
        .args(["comment", item, "--json", text])
        .current_dir(project)
        .output()
        .expect("bd comment runs");
    assert!(
        out.status.success(),
        "bd comment: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// One synthetic kit repository: the layout the installation owner reads plus
/// the minimal compiled-input tree the build identity records. The candidate
/// branch changes a recorded source file, exactly like a real kit candidate.
fn kit_repo(root: &Path, name: &str) -> PathBuf {
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
        serde_json::to_vec(&serde_json::json!({
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
        "# Principles\nfixture arm instructions\n",
    )
    .unwrap();
    fs::write(source.join("global/hooks.json"), "{}\n").unwrap();
    fs::write(source.join("global/rtk-hooks.json"), "{}\n").unwrap();
    fs::write(
        source.join("global/agents/fixture-agent.toml"),
        "name = \"fixture-agent\"\ndescription = \"Fixture agent.\"\n",
    )
    .unwrap();
    fs::write(
        source.join(".agents/skills/arm-skill/SKILL.md"),
        "---\nname: arm-skill\ndescription: Fixture skill.\n---\n\nfixture skill body\n",
    )
    .unwrap();
    git(&source, &["init", "-q", "--initial-branch=main"]);
    configure(&source);
    git(&source, &["add", "."]);
    git(&source, &["commit", "-qm", "seed kit"]);
    source
}

// ---------------------------------------------------------------------------
// Compiled native fixtures: a declared checker plus the runtime stand-ins.
// ---------------------------------------------------------------------------

struct Fixtures {
    _root: tempfile::TempDir,
    checker: PathBuf,
    launcher: PathBuf,
    upstream: PathBuf,
}

static FIXTURES: OnceLock<Fixtures> = OnceLock::new();
/// Serializes the installation case: the runtime owner publishes process-local
/// PATH entries and holds shared installation locks.
static INSTALL: Mutex<()> = Mutex::new(());

fn fixtures() -> &'static Fixtures {
    FIXTURES.get_or_init(|| {
        let root = tempfile::Builder::new()
            .prefix("improvement-activation-fixtures-")
            .tempdir()
            .unwrap();
        let checker = compile_fixture(root.path(), "checker", CHECKER_SOURCE);
        let upstream = compile_fixture(root.path(), "upstream", UPSTREAM_SOURCE);
        let launcher = compile_fixture(root.path(), "launcher", LAUNCHER_SOURCE);
        Fixtures {
            _root: root,
            checker,
            launcher,
            upstream,
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

/// A real native checker: it writes declared output and exits with the
/// declared code, so a passing and a failing combined-tree check are actual
/// process outcomes, never asserted JSON. It can also write a file into the
/// checked tree or run one child program in a declared directory before
/// exiting, which is how the tests trigger board withdrawal or Git drift
/// *during* a check.
const CHECKER_SOURCE: &str = r##"
use std::{
    env, fs,
    process::{Command, exit},
};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut code = 0i32;
    let mut out = String::new();
    let mut err = String::new();
    let mut write: Option<String> = None;
    let mut spawn_cwd: Option<String> = None;
    let mut spawn: Option<(String, Vec<String>)> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--exit-code" if index + 1 < args.len() => {
                code = args[index + 1].parse().expect("exit code");
                index += 2;
            }
            "--stdout" if index + 1 < args.len() => {
                out = args[index + 1].clone();
                index += 2;
            }
            "--stderr" if index + 1 < args.len() => {
                err = args[index + 1].clone();
                index += 2;
            }
            "--write-file" if index + 1 < args.len() => {
                write = Some(args[index + 1].clone());
                index += 2;
            }
            "--spawn-cwd" if index + 1 < args.len() => {
                spawn_cwd = Some(args[index + 1].clone());
                index += 2;
            }
            "--spawn" if index + 1 < args.len() => {
                spawn = Some((args[index + 1].clone(), args[index + 2..].to_vec()));
                break;
            }
            _ => index += 1,
        }
    }
    print!("{out}");
    eprint!("{err}");
    if let Some(path) = write {
        fs::write(path, "drift\n").expect("write drift file");
    }
    if let Some((program, rest)) = spawn {
        let mut command = Command::new(&program);
        command.args(&rest);
        if let Some(cwd) = spawn_cwd {
            command.current_dir(cwd);
        }
        match command.status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("checker child exited with {:?}", status.code());
                exit(10);
            }
            Err(error) => {
                eprintln!("checker child failed to start: {error}");
                exit(9);
            }
        }
    }
    exit(code);
}
"##;

/// Model-free stand-in for the original Codex CLI: version/help contract and
/// the feature disable/list contract over `$CODEX_HOME/config.toml`.
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

/// Model-free stand-in for the installed launcher: answers the owner's
/// `debug prompt-input` probe from the arm home's own instruction link and the
/// shared permission contract.
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
    let permissions = "<permissions instructions>\nFilesystem sandboxing defines which files can be read or written. `sandbox_mode` is `danger-full-access`: No filesystem sandboxing - all commands are permitted.\nApproval policy is currently never.\n</permissions instructions>";
    println!(
        "[{{\"role\":\"developer\",\"content\":[{{\"type\":\"input_text\",\"text\":{}}},{{\"type\":\"input_text\",\"text\":{}}}]}}]",
        escape(&instructions),
        escape(permissions)
    );
}
"##;

struct EnvironmentGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvironmentGuard {
    fn capture(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        Self { name, previous }
    }

    fn set(&self, value: &Path) {
        unsafe { std::env::set_var(self.name, value) };
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The predeclared policy applied to the retained paired evidence.
// ---------------------------------------------------------------------------

fn policy() -> ComparisonPolicy {
    ComparisonPolicy {
        schema: 1,
        objective: Objective::Time,
        basis: Basis::Efficiency,
        meaningful_effect_percent: Some(10.0),
        tolerance_percent: 5.0,
        require_acceptance: true,
        task_mix: "one frozen task case".into(),
        stopping: StoppingRule {
            max_attempts_per_arm: 3,
            required_units: 1,
        },
        repeated_selection: RepeatedSelection::Predeclared,
        trade_off: None,
        uncertainty: "unknown evidence stays inconclusive".into(),
        horizon_tasks: 5.0,
        overhead: Overhead {
            implementation_seconds: 10.0,
            evaluation_seconds: 5.0,
            maintenance_seconds_per_task: 0.5,
        },
    }
}

fn declare(policy: &ComparisonPolicy) -> DeclaredComparison {
    policy.declare().expect("declared policy")
}

#[allow(clippy::too_many_arguments)]
fn attempt(
    id: &str,
    arm: &str,
    case: &str,
    start: f64,
    seconds: f64,
    accepted: bool,
    rounds: Option<u64>,
    tools: Option<u64>,
    usage: bool,
) -> Value {
    let matched: BTreeMap<&str, &str> = MATCH_FIELDS.iter().map(|key| (*key, "fixed")).collect();
    let mut native =
        serde_json::json!({"started_at": start, "ended_at": start + 2.0, "status": "completed"});
    if let Some(rounds) = rounds {
        native["rounds"] = serde_json::json!(rounds);
    }
    if let Some(tools) = tools {
        native["tool_operations"] = serde_json::json!(tools);
    }
    if usage {
        native["usage"] = serde_json::json!({
            "model": "fixed",
            "input_tokens": 100,
            "cached_input_tokens": 40,
            "output_tokens": 20,
            "reasoning_tokens": 5,
        });
    }
    serde_json::json!({
        "attempt_id": id,
        "case_id": case,
        "arm": arm,
        "experiment_id": "exp-1",
        "started_at": start,
        "ended_at": start + seconds,
        "discovery_verified": true,
        "observed_model_metadata_verified": true,
        "matched": matched,
        "native_runs": [native],
        "checks": [{
            "id": "acceptance",
            "started_at": start + 2.0,
            "ended_at": start + seconds,
            "required": true,
            "executed": true,
            "passed": accepted,
            "exit_code": if accepted { 0 } else { 1 },
            "evidence": "private/log",
        }],
        "children": [],
        "interventions": [],
        "retry_of": null,
    })
}

fn summarize(rows: &[Value], policy: &ComparisonPolicy) -> Value {
    let rows: Vec<Value> = rows
        .iter()
        .map(|row| {
            let mut row = row.clone();
            row["declaration"] = policy.declaration();
            row
        })
        .collect();
    summarize_attempts(&rows).expect("authoritative summary")
}

// ---------------------------------------------------------------------------
// Scenario: one hypothesis card, one kit repository, one candidate branch and
// one policy evaluation.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fixture {
    Adopt,
    Reject,
    Inconclusive,
}

struct Scenario {
    root: tempfile::TempDir,
    board: PathBuf,
    bd: PathBuf,
    source: PathBuf,
    checkout: CandidateCheckout,
    item: String,
    experiment: String,
    evaluation: PolicyEvaluation,
    bindings: ExperimentBindings,
    spec: RunSpec,
    frozen_removal: Option<String>,
}

fn fixture_scenario(prefix: &str, fixture: Fixture) -> Scenario {
    let root = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
    let board = board_project(root.path());
    let bd = bd_executable();
    let item = admit_card(&bd, &board, "fixture-mechanism", "fixture-conditions");
    let source = kit_repo(root.path(), "source");
    let base = rev(&source);
    let mut checkout = allocate_candidate_checkout(
        &source,
        &root.path().join("alloc/candidate"),
        "hypothesis-1",
        &base,
    )
    .unwrap();
    fs::write(
        checkout.path.join("crates/one/src/lib.rs"),
        "pub fn one() { /* candidate change */ }\n",
    )
    .unwrap();
    git(&checkout.path, &["add", "."]);
    git(&checkout.path, &["commit", "-qm", "candidate change"]);
    // The frozen candidate revision replaces the allocation-time base in the
    // binding, exactly as freezing a candidate before measurement does.
    checkout.revision = rev(&checkout.path);
    assert_ne!(checkout.revision, checkout.base);

    let policy = policy();
    let declared = declare(&policy);
    let report = match fixture {
        Fixture::Adopt => summarize(
            &[
                attempt(
                    "b1",
                    "baseline",
                    "case-b",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                attempt(
                    "c1",
                    "candidate",
                    "case-b",
                    200.0,
                    85.0,
                    true,
                    Some(3),
                    Some(6),
                    true,
                ),
            ],
            &policy,
        ),
        Fixture::Reject => summarize(
            &[
                attempt(
                    "b1",
                    "baseline",
                    "case-b",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                attempt(
                    "c1",
                    "candidate",
                    "case-b",
                    200.0,
                    120.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
            ],
            &policy,
        ),
        Fixture::Inconclusive => summarize(
            &[
                attempt(
                    "b1",
                    "baseline",
                    "case-b",
                    0.0,
                    100.0,
                    true,
                    Some(4),
                    Some(6),
                    true,
                ),
                attempt(
                    "c1",
                    "candidate",
                    "case-b",
                    200.0,
                    80.0,
                    true,
                    Some(3),
                    None,
                    true,
                ),
            ],
            &policy,
        ),
    };
    let evaluation = evaluate(&declared, &report).unwrap();
    let expected = match fixture {
        Fixture::Adopt => PolicyDecision::Adopt,
        Fixture::Reject => PolicyDecision::Reject,
        Fixture::Inconclusive => PolicyDecision::Inconclusive,
    };
    assert_eq!(
        evaluation.decision, expected,
        "fixture policy outcome: {:?}",
        evaluation.reasons
    );
    let bindings = ExperimentBindings {
        schema: harness_core::improvement_experiment::EXPERIMENT_SCHEMA,
        hypothesis: item.clone(),
        case_id: "case-b".into(),
        base_revision: base.clone(),
        candidate: checkout.clone(),
        oracle: "oracle:fixture".into(),
        acceptance: "acceptance:fixture".into(),
        policy_digest: evaluation.policy_digest.clone(),
        arms: Vec::new(),
    };
    let spec = run_spec(root.path(), &source, &board, &bd, &item, &base);
    spec.validate().expect("valid run inputs");
    Scenario {
        root,
        board,
        bd,
        source,
        checkout,
        item,
        experiment: "run-fixture-experiment-1".into(),
        evaluation,
        bindings,
        spec,
        frozen_removal: None,
    }
}

fn run_spec(
    root: &Path,
    source: &Path,
    board: &Path,
    bd: &Path,
    item: &str,
    base: &str,
) -> RunSpec {
    let codex_home = root.join("codex-home");
    let planning_root = root.join("openspec");
    fs::create_dir_all(&codex_home).unwrap();
    fs::create_dir_all(&planning_root).unwrap();
    RunSpec {
        schema: improvement_loop::RUN_SCHEMA,
        run: "run-fixture".into(),
        project: source.to_path_buf(),
        codex_home,
        board: BoardInputs {
            bd: bd.to_path_buf(),
            project: board.to_path_buf(),
        },
        specification: Specification {
            project: source.to_path_buf(),
            change: "add-synthetic".into(),
            store: None,
            planning_root,
        },
        hypothesis_item: item.to_owned(),
        experiment: ExperimentContract {
            acceptance_artifact: "specs/synthetic/spec.md".into(),
            acceptance_heading: "# Acceptance".into(),
            mechanism: "Synthetic mechanism".into(),
            counterexample: "Synthetic counterexample".into(),
            applicability: "Synthetic applicability".into(),
            independent_acceptance: "Synthetic independent acceptance".into(),
            meaningful_effect: "Synthetic meaningful effect".into(),
            operating_conditions: "Synthetic operating conditions".into(),
            comparison_policy: "Synthetic comparison policy".into(),
            stopping_rule: "Synthetic stopping rule".into(),
        },
        base_revision: base.to_owned(),
        writable_scope: vec!["crates".into()],
        runner: None,
        local_runner: None,
        qualification: None,
        publication_scope: vec![PublicationStage::Experiment, PublicationStage::Integration],
        oracle: "oracle:fixture".into(),
        removal: None,
        evidence_root: None,
        comparison: None,
    }
}

fn publish(scenario: &Scenario) -> String {
    let draft = scenario
        .evaluation
        .decision_draft(
            &scenario.item,
            &scenario.experiment,
            &scenario.bindings.base_revision,
            &scenario.bindings.candidate.revision,
            &scenario.bindings.acceptance,
        )
        .unwrap();
    match publish_decision(&scenario.bd, &scenario.board, &draft).unwrap() {
        Publication::Recorded { text } => text,
        Publication::Confirmed { .. } => panic!("the first publication must be recorded"),
    }
}

fn republish(scenario: &Scenario) -> Publication {
    let draft = scenario
        .evaluation
        .decision_draft(
            &scenario.item,
            &scenario.experiment,
            &scenario.bindings.base_revision,
            &scenario.bindings.candidate.revision,
            &scenario.bindings.acceptance,
        )
        .unwrap();
    publish_decision(&scenario.bd, &scenario.board, &draft).unwrap()
}

fn check(exit_code: i32, stdout: &str, stderr: &str) -> CheckSpec {
    CheckSpec {
        program: fixtures().checker.clone(),
        args: vec![
            "--exit-code".into(),
            exit_code.to_string().into(),
            "--stdout".into(),
            stdout.into(),
            "--stderr".into(),
            stderr.into(),
        ],
        timeout: Duration::from_secs(60),
    }
}

/// A passing check at an explicit checker path, used where the checker bytes
/// themselves are part of the case.
fn passing_check_at(program: &Path) -> CheckSpec {
    CheckSpec {
        program: program.to_path_buf(),
        args: vec![
            "--exit-code".into(),
            "0".into(),
            "--stdout".into(),
            "combined-tree check passed\n".into(),
        ],
        timeout: Duration::from_secs(60),
    }
}

/// A passing check whose real child process mutates state in `cwd` while the
/// combined-tree check is running: the declared board withdrawal or Git drift
/// happens between the check and the effect, not only before `integrate`.
fn check_spawning(program: &Path, cwd: &Path, child: &[&str]) -> CheckSpec {
    let mut args: Vec<std::ffi::OsString> = vec![
        "--exit-code".into(),
        "0".into(),
        "--stdout".into(),
        "combined-tree check passed\n".into(),
        "--spawn-cwd".into(),
        cwd.as_os_str().to_owned(),
        "--spawn".into(),
    ];
    args.extend(child.iter().map(|argument| argument.into()));
    CheckSpec {
        program: program.to_path_buf(),
        args,
        timeout: Duration::from_secs(60),
    }
}

/// A passing check that writes one file into the checked candidate tree while
/// it runs, so the tree no longer matches the committed checked revision.
fn check_writing_file(program: &Path, relative: &str) -> CheckSpec {
    CheckSpec {
        program: program.to_path_buf(),
        args: vec![
            "--write-file".into(),
            relative.into(),
            "--exit-code".into(),
            "0".into(),
            "--stdout".into(),
            "combined-tree check passed\n".into(),
        ],
        timeout: Duration::from_secs(60),
    }
}

fn passing_check() -> CheckSpec {
    check(0, "combined-tree check passed\n", "")
}

fn integration_request<'a>(
    scenario: &'a Scenario,
    check: CheckSpec,
    prior: Option<IntegrationReceipt>,
) -> IntegrationRequest<'a> {
    IntegrationRequest {
        spec: &scenario.spec,
        bindings: &scenario.bindings,
        evaluation: &scenario.evaluation,
        experiment: scenario.experiment.clone(),
        frozen_removal: scenario.frozen_removal.clone(),
        mainline: scenario.source.clone(),
        check,
        evidence: scenario.root.path().join("evidence"),
        prior,
    }
}

fn blocked_of(outcome: IntegrationOutcome) -> Blocked {
    match outcome {
        IntegrationOutcome::Blocked(blocked) => blocked,
        other => panic!("expected a blocked integration, got {other:?}"),
    }
}

fn activation_blocked(outcome: ActivationOutcome) -> Blocked {
    match outcome {
        ActivationOutcome::Blocked(blocked) => blocked,
        other => panic!("expected a blocked activation, got {other:?}"),
    }
}

fn check_logs(evidence: &Path) -> Vec<PathBuf> {
    let mut logs: Vec<PathBuf> = fs::read_dir(evidence)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("check-"))
                })
                .collect()
        })
        .unwrap_or_default();
    logs.sort();
    logs
}

// ---------------------------------------------------------------------------
// Integration: publication, evidence binding and exact-revision mainline.
// ---------------------------------------------------------------------------

#[test]
fn publication_is_idempotent_and_a_newer_conflicting_decision_blocks_integration() {
    let scenario = fixture_scenario("improvement-activation-publication-", Fixture::Adopt);
    publish(&scenario);
    let Publication::Confirmed { .. } = republish(&scenario) else {
        panic!("a retried identical decision must be confirmed, not recorded twice");
    };
    let comments = board_hypothesis::list_hypothesis_cards(&scenario.bd, &scenario.board).unwrap();
    assert_eq!(comments.len(), 1, "the card stays one durable hypothesis");

    let outcome = integrate(&integration_request(&scenario, passing_check(), None)).unwrap();
    let IntegrationOutcome::Integrated(receipt) = outcome else {
        panic!("the supported decision must integrate, got {outcome:?}");
    };
    assert!(receipt.applied);
    assert_eq!(rev(&scenario.source), scenario.checkout.revision);
    assert_eq!(
        fs::read_to_string(scenario.source.join("crates/one/src/lib.rs"))
            .unwrap()
            .replace("\r\n", "\n"),
        "pub fn one() { /* candidate change */ }\n"
    );

    // A newer adopt decision for different evaluated revisions is not the
    // decision this evaluation publishes: the newest record controls, and the
    // stale evidence must not be inherited.
    let contradictory = DecisionDraft {
        item: scenario.item.clone(),
        experiment: scenario.experiment.clone(),
        outcome: DecisionOutcome::Adopt,
        quality: QualityOutcome::Unchanged,
        matched: 1,
        tolerance_percent: 5.0,
        baseline_seconds: 100.0,
        candidate_seconds: 85.0,
        baseline_arm: "baseline".into(),
        candidate_arm: "candidate".into(),
        accounting: "attempts:2,tasks:1,accepted:2,per_success:complete".into(),
        baseline_revision: scenario.bindings.base_revision.clone(),
        candidate_revision: "0123456789abcdef0123456789abcdef01234567".into(),
        acceptance: scenario.bindings.acceptance.clone(),
        coverage: "time+rounds+tool_ops".into(),
        scope: "case-b".into(),
        reason: "contradictory-fixture".into(),
        detail: None,
    };
    let Publication::Recorded { .. } =
        publish_decision(&scenario.bd, &scenario.board, &contradictory).unwrap()
    else {
        panic!("a different decision is a new record");
    };
    let blocked =
        blocked_of(integrate(&integration_request(&scenario, passing_check(), None)).unwrap());
    assert!(
        blocked.reason.contains("newest recorded decision"),
        "{}",
        blocked.reason
    );
    assert_eq!(rev(&scenario.source), scenario.checkout.revision);

    // An incomplete newer v2 record supersedes the complete one and cannot
    // authorize adoption.
    comment(
        &scenario.bd,
        &scenario.board,
        &scenario.item,
        &format!(
            "benefit-gate v2 item={} experiment={} revisions={}..{} outcome=adopt",
            scenario.item,
            scenario.experiment,
            scenario.bindings.base_revision,
            scenario.checkout.revision
        ),
    );
    let blocked =
        blocked_of(integrate(&integration_request(&scenario, passing_check(), None)).unwrap());
    assert!(
        blocked.reason.contains("cannot authorize adoption"),
        "{}",
        blocked.reason
    );
    assert!(!blocked.pending);
    assert_eq!(rev(&scenario.source), scenario.checkout.revision);
}

#[test]
fn reject_and_inconclusive_decisions_leave_the_mainline_and_pointer_unchanged() {
    for fixture in [Fixture::Reject, Fixture::Inconclusive] {
        let scenario = fixture_scenario("improvement-activation-decision-", fixture);
        match publish_decision(
            &scenario.bd,
            &scenario.board,
            &scenario
                .evaluation
                .decision_draft(
                    &scenario.item,
                    &scenario.experiment,
                    &scenario.bindings.base_revision,
                    &scenario.checkout.revision,
                    &scenario.bindings.acceptance,
                )
                .unwrap(),
        )
        .unwrap()
        {
            Publication::Recorded { .. } | Publication::Confirmed { .. } => {}
        }
        let base = rev(&scenario.source);
        let blocked =
            blocked_of(integrate(&integration_request(&scenario, passing_check(), None)).unwrap());
        assert!(
            blocked.reason.contains(&format!(
                "decided {}",
                scenario.evaluation.decision.as_str()
            )),
            "{}",
            blocked.reason
        );
        assert!(!blocked.pending);
        assert_eq!(rev(&scenario.source), base, "the mainline stays unchanged");
        assert!(check_logs(&scenario.root.path().join("evidence")).is_empty());
    }
}

#[test]
fn changed_base_dirty_target_and_changed_candidate_block_integration() {
    // A mainline that moved after the decision cannot inherit the benefit
    // evidence.
    let scenario = fixture_scenario("improvement-activation-base-", Fixture::Adopt);
    publish(&scenario);
    fs::write(
        scenario.source.join("moved.txt"),
        "moved after the decision\n",
    )
    .unwrap();
    git(&scenario.source, &["add", "."]);
    git(&scenario.source, &["commit", "-qm", "moved base"]);
    let moved = rev(&scenario.source);
    let blocked =
        blocked_of(integrate(&integration_request(&scenario, passing_check(), None)).unwrap());
    assert!(
        blocked.reason.contains("changed base cannot inherit"),
        "{}",
        blocked.reason
    );
    assert_eq!(rev(&scenario.source), moved, "the moved base is preserved");

    // A dirty mainline is preserved and refused.
    let scenario = fixture_scenario("improvement-activation-dirty-", Fixture::Adopt);
    publish(&scenario);
    fs::write(scenario.source.join("uncommitted.txt"), "local work\n").unwrap();
    let blocked =
        blocked_of(integrate(&integration_request(&scenario, passing_check(), None)).unwrap());
    assert!(
        blocked.reason.contains("uncommitted or untracked work"),
        "{}",
        blocked.reason
    );
    assert!(scenario.source.join("uncommitted.txt").is_file());
    assert_eq!(rev(&scenario.source), scenario.bindings.base_revision);

    // A candidate checkout that changed after the decision is not the
    // evaluated revision.
    let scenario = fixture_scenario("improvement-activation-candidate-", Fixture::Adopt);
    publish(&scenario);
    fs::write(
        scenario.checkout.path.join("crates/one/src/lib.rs"),
        "pub fn one() { /* unrecorded work */ }\n",
    )
    .unwrap();
    git(&scenario.checkout.path, &["add", "."]);
    git(
        &scenario.checkout.path,
        &["commit", "-qm", "extra candidate work"],
    );
    let blocked =
        blocked_of(integrate(&integration_request(&scenario, passing_check(), None)).unwrap());
    assert!(
        blocked.reason.contains("bound revision"),
        "{}",
        blocked.reason
    );
    assert_eq!(rev(&scenario.source), scenario.bindings.base_revision);
}

#[test]
fn failed_combined_tree_check_blocks_integration_and_retains_its_output() {
    let scenario = fixture_scenario("improvement-activation-check-", Fixture::Adopt);
    publish(&scenario);
    let evidence = scenario.root.path().join("evidence");
    let blocked = blocked_of(
        integrate(&integration_request(
            &scenario,
            check(3, "checker stdout\n", "checker failed\n"),
            None,
        ))
        .unwrap(),
    );
    let retained = blocked.check.expect("the failed check receipt is retained");
    assert_eq!(retained.revision, scenario.checkout.revision);
    assert_eq!(retained.exit_code, 3);
    assert!(!retained.passed());
    assert!(retained.stdout.bytes > 0);
    assert_eq!(
        fs::read_to_string(&retained.stdout.path).unwrap(),
        "checker stdout\n"
    );
    assert_eq!(
        fs::read_to_string(&retained.stderr.path).unwrap(),
        "checker failed\n"
    );
    assert_eq!(
        retained.stdout.sha256,
        build_identity::hash_file(&retained.stdout.path).unwrap()
    );
    assert_eq!(
        rev(&scenario.source),
        scenario.bindings.base_revision,
        "a failed check never reaches the mainline"
    );
    assert!(
        !check_logs(&evidence).is_empty(),
        "actual output is retained"
    );
}

#[test]
fn removal_authority_gates_integration_with_its_latest_decision() {
    let mut scenario = fixture_scenario("improvement-activation-removal-", Fixture::Adopt);
    let proposal = "proposal:fixture".to_owned();
    let target = "skill:fixture".to_owned();
    scenario.spec.removal = Some(RemovalScope {
        proposal: proposal.clone(),
        target: target.clone(),
    });
    let recorded = BoundedRemovalProposal::try_from_draft(RemovalProposalDraft {
        proposal: proposal.clone(),
        target: target.clone(),
        evidence: "evidence:fixture".into(),
        loss: "loses-fixture".into(),
        preview: None,
        detail: None,
    })
    .unwrap();
    board_hypothesis::record_removal_proposal(
        &scenario.bd,
        &scenario.board,
        &scenario.item,
        &recorded,
    )
    .unwrap();
    publish(&scenario);
    let base = rev(&scenario.source);

    // No user decision yet: the removal gate pends and nothing moves.
    let blocked =
        blocked_of(integrate(&integration_request(&scenario, passing_check(), None)).unwrap());
    assert!(blocked.pending, "{}", blocked.reason);
    assert_eq!(rev(&scenario.source), base);

    let decide = |kind: RemovalDecisionKind, actions: Vec<RemovalAction>| {
        let bounded = BoundedRemovalDecision::try_from_draft(RemovalDecisionDraft {
            decision: kind,
            proposal: proposal.clone(),
            target: target.clone(),
            actions,
            loss: Some("loses-fixture".into()),
            basis: Some("basis:fixture".into()),
            detail: None,
        })
        .unwrap();
        board_hypothesis::record_removal_decision(
            &scenario.bd,
            &scenario.board,
            &scenario.item,
            &bounded,
        )
        .unwrap();
    };

    // Experiment-only consent cannot cover integration.
    decide(
        RemovalDecisionKind::Approve,
        vec![RemovalAction::Experiment],
    );
    let blocked =
        blocked_of(integrate(&integration_request(&scenario, passing_check(), None)).unwrap());
    assert!(blocked.pending, "{}", blocked.reason);
    assert_eq!(rev(&scenario.source), base);

    // A refusal is recorded separately from benefit and is not retried.
    decide(RemovalDecisionKind::Refuse, Vec::new());
    let blocked =
        blocked_of(integrate(&integration_request(&scenario, passing_check(), None)).unwrap());
    assert!(!blocked.pending);
    assert!(blocked.reason.contains("refused"), "{}", blocked.reason);
    assert_eq!(rev(&scenario.source), base);

    // The latest decision controls: a withdrawal supersedes the refusal.
    decide(RemovalDecisionKind::Withdraw, Vec::new());
    let blocked =
        blocked_of(integrate(&integration_request(&scenario, passing_check(), None)).unwrap());
    assert!(blocked.reason.contains("withdrew"), "{}", blocked.reason);

    // Approval covering integration authorizes exactly this effect.
    decide(
        RemovalDecisionKind::Approve,
        vec![RemovalAction::Integration],
    );
    let IntegrationOutcome::Integrated(receipt) =
        integrate(&integration_request(&scenario, passing_check(), None)).unwrap()
    else {
        panic!("covered removal approval must integrate");
    };
    assert_eq!(receipt.integrated_revision, scenario.checkout.revision);
    assert_eq!(rev(&scenario.source), scenario.checkout.revision);
}

#[test]
fn resume_confirms_observed_integration_without_replaying_effects() {
    let scenario = fixture_scenario("improvement-activation-resume-", Fixture::Adopt);
    publish(&scenario);
    let evidence = scenario.root.path().join("evidence");
    let IntegrationOutcome::Integrated(receipt) =
        integrate(&integration_request(&scenario, passing_check(), None)).unwrap()
    else {
        panic!("the supported candidate integrates");
    };
    assert!(receipt.applied);
    let logs_after_first = check_logs(&evidence);

    // A retained receipt makes the resume a verified no-op: no second check,
    // no replay and no second mainline effect.
    let IntegrationOutcome::Confirmed(confirmed) = integrate(&integration_request(
        &scenario,
        passing_check(),
        Some(receipt.clone()),
    ))
    .unwrap() else {
        panic!("the already integrated revision must be confirmed");
    };
    assert!(!confirmed.applied);
    assert_eq!(confirmed.checks, receipt.checks);
    assert_eq!(check_logs(&evidence), logs_after_first);
    assert_eq!(rev(&scenario.source), scenario.checkout.revision);

    // An interruption after the merge but before the receipt is reconciled
    // from the observed state: the check is re-derived, the merge is not
    // replayed, and the mainline keeps exactly one integrated revision.
    let scenario = fixture_scenario("improvement-activation-resume-unknown-", Fixture::Adopt);
    publish(&scenario);
    git(
        &scenario.source,
        &["merge", "--ff-only", &scenario.checkout.revision],
    );
    let before = git(&scenario.source, &["rev-list", "--count", "HEAD"])
        .trim()
        .to_owned();
    let IntegrationOutcome::Confirmed(confirmed) =
        integrate(&integration_request(&scenario, passing_check(), None)).unwrap()
    else {
        panic!("the observed integration must be confirmed, not replayed");
    };
    assert!(!confirmed.applied);
    assert_eq!(confirmed.integrated_revision, scenario.checkout.revision);
    assert!(confirmed.checks.passed());
    assert_eq!(rev(&scenario.source), scenario.checkout.revision);
    assert_eq!(
        git(&scenario.source, &["rev-list", "--count", "HEAD"]).trim(),
        before,
        "no second integration effect is counted"
    );
}

/// The exact `removal-decision v1` text the board owner writes for one
/// decision, captured from a real board round trip so a fixture never
/// re-implements the owner's record format. The recorded decision is the
/// caller's responsibility in the surrounding sequence.
fn removal_decision_text(
    bd: &Path,
    board: &Path,
    item: &str,
    proposal: &BoundedRemovalProposal,
    kind: RemovalDecisionKind,
    actions: Vec<RemovalAction>,
) -> String {
    let bounded = BoundedRemovalDecision::try_from_draft(RemovalDecisionDraft {
        decision: kind,
        proposal: proposal.proposal.clone(),
        target: proposal.target.clone(),
        actions,
        loss: Some("loses-fixture".into()),
        basis: Some("basis:fixture".into()),
        detail: None,
    })
    .unwrap();
    board_hypothesis::record_removal_decision(bd, board, item, &bounded).unwrap();
    let prefix = format!("removal-decision v1 item={item} decision={}", kind.as_str());
    board_feedback::list_comments(bd, board, item)
        .unwrap()
        .into_iter()
        .rev()
        .find(|text| text.starts_with(&prefix))
        .unwrap_or_else(|| panic!("the owner recorded no {} decision", kind.as_str()))
}

#[test]
fn withdrawal_during_the_check_blocks_without_mutation() {
    let mut scenario = fixture_scenario("improvement-activation-withdraw-", Fixture::Adopt);
    let proposal = BoundedRemovalProposal::try_from_draft(RemovalProposalDraft {
        proposal: "proposal:fixture".into(),
        target: "skill:fixture".into(),
        evidence: "evidence:fixture".into(),
        loss: "loses-fixture".into(),
        preview: None,
        detail: None,
    })
    .unwrap();
    scenario.spec.removal = Some(RemovalScope {
        proposal: proposal.proposal.clone(),
        target: proposal.target.clone(),
    });
    board_hypothesis::record_removal_proposal(
        &scenario.bd,
        &scenario.board,
        &scenario.item,
        &proposal,
    )
    .unwrap();
    publish(&scenario);
    let base = rev(&scenario.source);
    let checker = fixtures().checker.clone();

    let approve = || {
        let bounded = BoundedRemovalDecision::try_from_draft(RemovalDecisionDraft {
            decision: RemovalDecisionKind::Approve,
            proposal: proposal.proposal.clone(),
            target: proposal.target.clone(),
            actions: vec![RemovalAction::Integration],
            loss: Some("loses-fixture".into()),
            basis: Some("basis:fixture".into()),
            detail: None,
        })
        .unwrap();
        board_hypothesis::record_removal_decision(
            &scenario.bd,
            &scenario.board,
            &scenario.item,
            &bounded,
        )
        .unwrap();
    };
    approve();
    // The withdrawal text is captured through the owner, then the board is
    // returned to the authorized state the check must start from.
    let withdrawal = removal_decision_text(
        &scenario.bd,
        &scenario.board,
        &scenario.item,
        &proposal,
        RemovalDecisionKind::Withdraw,
        Vec::new(),
    );
    assert!(withdrawal.contains("decision=withdraw"));
    approve();

    let child: Vec<&str> = vec![
        scenario.bd.to_str().unwrap(),
        "comment",
        scenario.item.as_str(),
        "--json",
        withdrawal.as_str(),
    ];
    let check = check_spawning(&checker, &scenario.board, &child);
    let blocked = blocked_of(integrate(&integration_request(&scenario, check, None)).unwrap());
    assert!(blocked.reason.contains("withdrew"), "{}", blocked.reason);
    assert!(!blocked.pending);
    assert!(blocked.check.is_none(), "the declared check itself passed");
    assert_eq!(
        rev(&scenario.source),
        base,
        "a withdrawal during the check never integrates"
    );
    assert!(
        !check_logs(&scenario.root.path().join("evidence")).is_empty(),
        "the declared check actually ran before the effect was refused"
    );

    // The already-integrated path is reconciled just as conservatively: with
    // the mainline observed at the candidate revision, a check that observes
    // the withdrawal during its run blocks instead of confirming.
    approve();
    git(
        &scenario.source,
        &["merge", "--ff-only", &scenario.checkout.revision],
    );
    assert_eq!(rev(&scenario.source), scenario.checkout.revision);
    let check = check_spawning(&checker, &scenario.board, &child);
    let blocked = blocked_of(integrate(&integration_request(&scenario, check, None)).unwrap());
    assert!(blocked.reason.contains("withdrew"), "{}", blocked.reason);
    assert_eq!(
        rev(&scenario.source),
        scenario.checkout.revision,
        "the observed integration is preserved"
    );
}

#[test]
fn candidate_and_mainline_drift_during_the_check_blocks_the_effect() {
    let checker = fixtures().checker.clone();

    // The candidate revision moves while the combined-tree check runs.
    let scenario = fixture_scenario("improvement-activation-drift-candidate-", Fixture::Adopt);
    publish(&scenario);
    let base = rev(&scenario.source);
    let candidate = scenario.checkout.path.clone();
    let check = check_spawning(
        &checker,
        &candidate,
        &["git", "commit", "--allow-empty", "-qm", "candidate drift"],
    );
    let blocked = blocked_of(integrate(&integration_request(&scenario, check, None)).unwrap());
    assert!(
        blocked.reason.contains("candidate revision changed"),
        "{}",
        blocked.reason
    );
    assert_eq!(
        rev(&scenario.source),
        base,
        "the mainline stays at the base"
    );
    assert_ne!(
        rev(&candidate),
        scenario.checkout.revision,
        "the drifted candidate work is preserved"
    );

    // The mainline moves while the combined-tree check runs.
    let scenario = fixture_scenario("improvement-activation-drift-mainline-", Fixture::Adopt);
    publish(&scenario);
    let mainline = scenario.source.clone();
    let check = check_spawning(
        &checker,
        &mainline,
        &["git", "commit", "--allow-empty", "-qm", "mainline drift"],
    );
    let blocked = blocked_of(integrate(&integration_request(&scenario, check, None)).unwrap());
    assert!(
        blocked.reason.contains("mainline moved"),
        "{}",
        blocked.reason
    );
    assert_ne!(rev(&mainline), scenario.checkout.revision);
    assert_ne!(rev(&mainline), scenario.bindings.base_revision);
    assert_eq!(
        fs::read_to_string(mainline.join("crates/one/src/lib.rs"))
            .unwrap()
            .replace("\r\n", "\n"),
        "pub fn one() {}\n",
        "candidate content never reached the moved mainline"
    );

    // The candidate tree becomes dirty while the check runs.
    let scenario = fixture_scenario("improvement-activation-drift-dirty-", Fixture::Adopt);
    publish(&scenario);
    let check = check_writing_file(&checker, "drift.txt");
    let blocked = blocked_of(integrate(&integration_request(&scenario, check, None)).unwrap());
    assert!(
        blocked.reason.contains("candidate checkout"),
        "{}",
        blocked.reason
    );
    assert!(
        scenario.checkout.path.join("drift.txt").is_file(),
        "the drift is preserved"
    );
    assert_eq!(rev(&scenario.source), scenario.bindings.base_revision);
}

#[test]
fn changed_checker_bytes_and_missing_output_reject_receipt_reuse() {
    let scenario = fixture_scenario("improvement-activation-checkreuse-", Fixture::Adopt);
    publish(&scenario);
    let checker_path = scenario.root.path().join("checker-under-test.exe");
    fs::copy(&fixtures().checker, &checker_path).unwrap();
    let variant = compile_fixture(
        scenario.root.path(),
        "checker-variant",
        &format!("{CHECKER_SOURCE}\n// different checker bytes, same contract\n"),
    );
    let evidence = scenario.root.path().join("evidence");
    let IntegrationOutcome::Integrated(receipt) = integrate(&integration_request(
        &scenario,
        passing_check_at(&checker_path),
        None,
    ))
    .unwrap() else {
        panic!("the supported candidate integrates");
    };
    assert_eq!(
        receipt.checks.program_sha256,
        build_identity::hash_file(&checker_path).unwrap()
    );
    let logs = check_logs(&evidence);

    // A changed checker binary invalidates the retained receipt: the check is
    // re-derived with the current declared checker instead of being skipped.
    fs::copy(&variant, &checker_path).unwrap();
    let variant_digest = build_identity::hash_file(&checker_path).unwrap();
    assert_ne!(variant_digest, receipt.checks.program_sha256);
    let IntegrationOutcome::Confirmed(reused) = integrate(&integration_request(
        &scenario,
        passing_check_at(&checker_path),
        Some(receipt.clone()),
    ))
    .unwrap() else {
        panic!("the exact integration stays confirmed");
    };
    assert_eq!(reused.checks.program_sha256, variant_digest);
    assert!(
        check_logs(&evidence).len() > logs.len(),
        "the check was re-derived, not skipped"
    );

    // A missing retained output cannot skip the check either.
    fs::remove_file(&reused.checks.stdout.path).unwrap();
    let before = check_logs(&evidence);
    let IntegrationOutcome::Confirmed(rechecked) = integrate(&integration_request(
        &scenario,
        passing_check_at(&checker_path),
        Some(reused.clone()),
    ))
    .unwrap() else {
        panic!("the exact integration stays confirmed");
    };
    assert!(check_logs(&evidence).len() > before.len());
    assert!(rechecked.checks.stdout.path.is_file());

    // A modified retained output is likewise not reusable.
    fs::write(&rechecked.checks.stdout.path, "tampered\n").unwrap();
    let before = check_logs(&evidence);
    let IntegrationOutcome::Confirmed(_) = integrate(&integration_request(
        &scenario,
        passing_check_at(&checker_path),
        Some(rechecked.clone()),
    ))
    .unwrap() else {
        panic!("the exact integration stays confirmed");
    };
    assert!(check_logs(&evidence).len() > before.len());
}

// ---------------------------------------------------------------------------
// Activation: verified consumption and the checked integrated revision.
// ---------------------------------------------------------------------------

fn owned_state(root: &Path) -> PathBuf {
    let state = root.join("state");
    fs::create_dir_all(state.join("builds")).unwrap();
    fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
    state
}

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

fn activation_request<'a>(
    scenario: &'a Scenario,
    state: &Path,
    runtime: &'a ArmRuntime,
    integration: &IntegrationReceipt,
    attempt_active: bool,
) -> ActivationRequest<'a> {
    ActivationRequest {
        spec: &scenario.spec,
        bindings: &scenario.bindings,
        evaluation: &scenario.evaluation,
        experiment: scenario.experiment.clone(),
        frozen_removal: scenario.frozen_removal.clone(),
        mainline: scenario.source.clone(),
        integration: integration.clone(),
        state: state.to_path_buf(),
        runtime,
        attempt_active,
    }
}

#[test]
fn activation_requires_verified_consumption_and_binds_the_integrated_revision() {
    let _serial = INSTALL.lock().unwrap();
    let mut scenario = fixture_scenario("improvement-activation-runtime-", Fixture::Adopt);
    let fixtures = fixtures();
    let _cpu = EnvironmentGuard::capture("CODEX_HARNESS_CPU_ACCOUNT");
    fs::create_dir_all(scenario.root.path().join("cpu-account")).unwrap();
    _cpu.set(&scenario.root.path().join("cpu-account"));
    let _path = EnvironmentGuard::capture("PATH");

    // The real installation owner publishes the prepared candidate runtime
    // into an owned home and its consumption identity is re-verified.
    let state = owned_state(scenario.root.path());
    let build = fixture_build(
        &state,
        "candidate",
        &scenario.checkout.path,
        &fixtures.launcher,
        "candidate",
    );
    let variant = prepare_variant(&state, Arm::Candidate, "H+A", &build).unwrap();
    let home = prepare_home(&scenario.root.path().join("homes/candidate")).unwrap();
    let user_home = prepare_home(&scenario.root.path().join("homes/candidate-user")).unwrap();
    let dependency_user_home =
        prepare_home(&scenario.root.path().join("homes/candidate-dep")).unwrap();
    let request = ArmRequest::model_free(
        variant.clone(),
        home.clone(),
        user_home,
        dependency_user_home,
        fixtures.upstream.clone(),
    );
    let runtime = install_arm(&request).unwrap();
    assert_eq!(runtime.model_calls, 0);
    // The experiment binding carries the real candidate arm allocation.
    let workload = frozen_copy(
        &scenario.source,
        &scenario.bindings.base_revision,
        &scenario.root.path().join("workload/candidate"),
    )
    .unwrap();
    scenario.bindings.arms = vec![ArmBinding {
        arm: Arm::Candidate,
        home,
        workload,
        runtime: variant.clone(),
    }];

    // The heavy case uses its own checker copy so later cases can change the
    // checker bytes without touching the shared fixtures.
    let checker_path = scenario.root.path().join("checker-under-test.exe");
    fs::copy(&fixtures.checker, &checker_path).unwrap();
    publish(&scenario);
    let IntegrationOutcome::Integrated(integration) = integrate(&integration_request(
        &scenario,
        passing_check_at(&checker_path),
        None,
    ))
    .unwrap() else {
        panic!("the supported candidate integrates");
    };
    assert_eq!(rev(&scenario.source), scenario.checkout.revision);

    let Activated(activated) = activate(&activation_request(
        &scenario,
        &state,
        &runtime,
        &integration,
        false,
    ))
    .unwrap() else {
        panic!("the verified installed candidate must activate");
    };
    assert!(activated.applied);
    assert_eq!(
        activated.consumption.build.canonicalize().unwrap(),
        variant.build.canonicalize().unwrap()
    );
    assert_eq!(activated.consumption.record_sha256, variant.record_sha256);
    assert_eq!(
        activated.selected.build.canonicalize().unwrap(),
        variant.build.canonicalize().unwrap()
    );
    assert_eq!(activated.integrated_revision, scenario.checkout.revision);
    assert_eq!(activated.decision_sha256, integration.decision_sha256);
    assert_eq!(
        activated.runtime_source.canonicalize().unwrap(),
        scenario.checkout.path.canonicalize().unwrap()
    );
    let (selected, _) = build_selection::selected(&state).unwrap();
    assert_eq!(
        selected.canonicalize().unwrap(),
        variant.build.canonicalize().unwrap()
    );

    // Selecting the unchanged prepared variant again is a confirmed no-op.
    let Confirmed(confirmed) = activate(&activation_request(
        &scenario,
        &state,
        &runtime,
        &integration,
        false,
    ))
    .unwrap() else {
        panic!("a repeated activation of the same variant must be confirmed");
    };
    assert!(!confirmed.applied);
    assert_eq!(confirmed.selected.build, activated.selected.build);

    // A runtime whose retained identity no longer describes a model-free
    // installation is refused; the active pointer is unchanged.
    let mut tampered = runtime.clone();
    tampered.model_calls = 1;
    let blocked = activation_blocked(
        activate(&activation_request(
            &scenario,
            &state,
            &tampered,
            &integration,
            false,
        ))
        .unwrap(),
    );
    assert!(blocked.reason.contains("consumed"), "{}", blocked.reason);
    let (selected, _) = build_selection::selected(&state).unwrap();
    assert_eq!(
        selected.canonicalize().unwrap(),
        variant.build.canonicalize().unwrap()
    );

    // A measured attempt keeps its frozen runtime.
    let blocked = activation_blocked(
        activate(&activation_request(
            &scenario,
            &state,
            &runtime,
            &integration,
            true,
        ))
        .unwrap(),
    );
    assert!(
        blocked.reason.contains("attempt is active"),
        "{}",
        blocked.reason
    );

    // A receipt that does not belong to the current decision never authorizes
    // activation.
    let mut foreign = integration.clone();
    foreign.decision_sha256 = "0".repeat(64);
    let blocked = activation_blocked(
        activate(&activation_request(
            &scenario, &state, &runtime, &foreign, false,
        ))
        .unwrap(),
    );
    assert!(
        blocked.reason.contains("stale or foreign receipt"),
        "{}",
        blocked.reason
    );

    // A mainline that no longer records the checked integrated revision
    // cannot be activated against the retained receipt.
    fs::write(
        scenario.source.join("moved.txt"),
        "moved after integration\n",
    )
    .unwrap();
    git(&scenario.source, &["add", "."]);
    git(
        &scenario.source,
        &["commit", "-qm", "moved after integration"],
    );
    assert_ne!(rev(&scenario.source), scenario.checkout.revision);
    let blocked = activation_blocked(
        activate(&activation_request(
            &scenario,
            &state,
            &runtime,
            &integration,
            false,
        ))
        .unwrap(),
    );
    assert!(
        blocked.reason.contains("checked integrated revision"),
        "{}",
        blocked.reason
    );

    // A receipt whose check covered another revision cannot authorize
    // activation, even while the decision and everything else still match.
    let mut foreign_revision = integration.clone();
    foreign_revision.checks.revision = "0".repeat(40);
    let blocked = activation_blocked(
        activate(&activation_request(
            &scenario,
            &state,
            &runtime,
            &foreign_revision,
            false,
        ))
        .unwrap(),
    );
    assert!(
        blocked.reason.contains("covered another revision"),
        "{}",
        blocked.reason
    );

    // Missing retained check output cannot authorize activation.
    fs::remove_file(&integration.checks.stdout.path).unwrap();
    let blocked = activation_blocked(
        activate(&activation_request(
            &scenario,
            &state,
            &runtime,
            &integration,
            false,
        ))
        .unwrap(),
    );
    assert!(blocked.reason.contains("unavailable"), "{}", blocked.reason);

    // A changed checker binary cannot authorize activation either.
    let variant = compile_fixture(
        scenario.root.path(),
        "checker-variant",
        &format!("{CHECKER_SOURCE}\n// different checker bytes, same contract\n"),
    );
    fs::copy(&variant, &checker_path).unwrap();
    let blocked = activation_blocked(
        activate(&activation_request(
            &scenario,
            &state,
            &runtime,
            &integration,
            false,
        ))
        .unwrap(),
    );
    assert!(
        blocked.reason.contains("changed since the check"),
        "{}",
        blocked.reason
    );
    fs::copy(&fixtures.checker, &checker_path).unwrap();

    // A newer conflicting decision supersedes the decision the integration
    // was performed under.
    comment(
        &scenario.bd,
        &scenario.board,
        &scenario.item,
        &format!(
            "benefit-gate v2 item={} experiment={} revisions={}..{} acceptance={} coverage={} scope={} reason=superseded outcome=reject quality=unchanged matched=1 tolerance_percent=5.0 baseline_seconds=100.0 candidate_seconds=120.0 baseline=baseline candidate=candidate accounting=attempts:2",
            scenario.item,
            scenario.experiment,
            scenario.bindings.base_revision,
            scenario.checkout.revision,
            scenario.bindings.acceptance,
            "time+rounds",
            "case-b"
        ),
    );
    let blocked = activation_blocked(
        activate(&activation_request(
            &scenario,
            &state,
            &runtime,
            &integration,
            false,
        ))
        .unwrap(),
    );
    assert!(
        blocked.reason.contains("cannot authorize adoption"),
        "{}",
        blocked.reason
    );
}
