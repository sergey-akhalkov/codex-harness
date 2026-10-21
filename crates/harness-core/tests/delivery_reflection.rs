//! Native accepted-task reflection boundary. Every board below is an owned
//! temporary `bd` project; the controller is exercised through its public API
//! and makes no model calls.

use harness_core::delivery_reflection::{
    ACCEPTANCE_METADATA, AcceptanceBinding, ActivationRequest, BindingStatus, DeliveryState,
    EventRefusal, Finding, JournalOwnership, JournalPolicy, LifecycleStatus, PURPOSE_METADATA,
    PollReceipt, Purpose, RecoveryKind, ReflectionController, ReflectionLimits, RefusalCode,
    RetainedOwnerProcess, Stage, StageOutcome, StageState, StageWork, SubmittedAccounting,
    SuccessionRequest, TaskBindingStatus, UsageDelta, acceptance_metadata,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
};

#[path = "delivery_reflection/native_branch.rs"]
mod native_branch;
#[path = "delivery_reflection/native_import.rs"]
mod native_import;
const LEAD: &str = "lead-fixture";
const SUCCESSOR: &str = "successor-fixture";
const APPROVER: &str = "board-contract";

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

struct Board {
    project: PathBuf,
    bd: PathBuf,
    codex_home: PathBuf,
}

impl Board {
    fn new(root: &Path) -> Self {
        Self::new_with_init(root, &[])
    }

    fn new_with_init(root: &Path, init_options: &[&str]) -> Self {
        let project = root.join("project");
        let codex_home = root.join("codex-home");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&codex_home).unwrap();
        git(&project, &["init", "-q", "--initial-branch=main"]);
        git(&project, &["config", "user.email", "fixture@example.test"]);
        git(&project, &["config", "user.name", "Fixture"]);
        fs::write(project.join("README.md"), "synthetic reflection board\n").unwrap();
        git(&project, &["add", "."]);
        git(&project, &["commit", "-qm", "seed"]);
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
            .args(init_options)
            .current_dir(&project)
            .env("BD_NON_INTERACTIVE", "1")
            .env("BEADS_ACTOR", APPROVER)
            .output()
            .expect("bd init runs");
        assert!(
            init.status.success(),
            "bd init: {} {}",
            String::from_utf8_lossy(&init.stdout),
            String::from_utf8_lossy(&init.stderr)
        );
        let board = Self {
            project,
            bd,
            codex_home,
        };
        board.unset_journal();
        board
    }

    fn controller(&self) -> ReflectionController {
        ReflectionController::new(&self.bd, &self.project, &self.codex_home)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(&self.bd)
            .args(args)
            .current_dir(&self.project)
            .env("BD_NON_INTERACTIVE", "1")
            .env("BEADS_ACTOR", APPROVER)
            .output()
            .expect("bd runs")
    }

    fn json(&self, args: &[&str]) -> Value {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "bd {args:?}: {} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap_or_else(|error| {
            panic!(
                "bd {args:?} returned non-JSON: {error}: {}",
                String::from_utf8_lossy(&out.stdout)
            )
        })
    }

    fn set_metadata(&self, id: &str, key: &str, value: &str) {
        let pair = format!("{key}={value}");
        self.json(&["update", id, "--set-metadata", &pair, "--json"]);
    }

    fn set_binding(&self, id: &str, binding: &AcceptanceBinding) {
        for (key, value) in acceptance_metadata(binding).unwrap() {
            self.set_metadata(id, &key, &value);
        }
    }

    fn set_acceptance_only(&self, id: &str, binding: &AcceptanceBinding) {
        self.set_metadata(
            id,
            ACCEPTANCE_METADATA,
            &serde_json::to_string(binding).unwrap(),
        );
    }

    fn create_task(&self, title: &str, purpose: Option<Purpose>, parent: Option<&str>) -> String {
        let mut args = vec!["create", title, "--type", "task"];
        if let Some(parent) = parent {
            args.extend(["--parent", parent]);
        }
        args.push("--json");
        let task = self.json(&args);
        let id = task["id"].as_str().unwrap().to_owned();
        if let Some(purpose) = purpose {
            self.set_metadata(&id, "harness.purpose", purpose.as_str());
        }
        id
    }

    fn close(&self, id: &str) {
        self.close_with_reason(id, "accepted");
    }

    fn close_with_reason(&self, id: &str, reason: &str) {
        self.json(&["close", id, "--reason", reason, "--json"]);
    }

    fn reopen(&self, id: &str) {
        self.json(&["reopen", id, "--reason", "fixture reclose", "--json"]);
    }

    fn journal(&self) -> Option<bool> {
        let values = self.json(&["config", "show", "--json"]);
        let entry = values
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["key"] == "events-journal");
        let entry = entry?;
        match (
            entry["value"].as_str().unwrap_or(""),
            entry["source"].as_str().unwrap_or(""),
        ) {
            ("true", _) => Some(true),
            ("false", "default") => None,
            ("false", _) => Some(false),
            other => panic!("unexpected journal value {other:?}"),
        }
    }

    fn unset_journal(&self) {
        let out = self.run(&["config", "unset", "events-journal"]);
        assert!(
            out.status.success(),
            "config unset events-journal: {} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(self.journal(), None);
    }

    fn event_head(&self, since: u64) -> u64 {
        let since = since.to_string();
        let out = self.run(&[
            "events", "tail", "--since", &since, "--limit", "500", "--json",
        ]);
        assert!(
            out.status.success(),
            "events tail: {} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
            .map(|line| {
                serde_json::from_slice::<Value>(line).unwrap()["seq"]
                    .as_u64()
                    .unwrap()
            })
            .max()
            .unwrap_or(since.parse().unwrap())
    }
}

fn binding(task: &str, purpose: Purpose, suffix: &str) -> AcceptanceBinding {
    AcceptanceBinding::new(
        task,
        format!("run-{suffix}"),
        format!("assignment-{suffix}"),
        format!("result-{suffix}"),
        format!("revision-{suffix}"),
        vec!["executor-fixture".to_owned()],
        LEAD,
        APPROVER,
        purpose,
    )
}

fn outcome(finding: Finding, summary: &str) -> StageOutcome {
    StageOutcome {
        finding,
        summary: summary.to_owned(),
        coverage: vec!["assignment".to_owned(), "checks".to_owned()],
        retained_at: String::new(),
        session: Some("reflection-session".to_owned()),
    }
}

fn activate(
    controller: &ReflectionController,
    journal: JournalPolicy,
) -> harness_core::delivery_reflection::ActivationReceipt {
    controller
        .activate(&ActivationRequest {
            lead_session: LEAD.to_owned(),
            run: Some("run-main".to_owned()),
            boundary: None,
            journal,
            limits: None,
        })
        .unwrap()
}

fn activate_with_process(
    controller: &ReflectionController,
    journal: JournalPolicy,
    owner_process: &RetainedOwnerProcess,
) -> harness_core::delivery_reflection::ActivationReceipt {
    controller
        .activate_with_owner_process(
            &ActivationRequest {
                lead_session: LEAD.to_owned(),
                run: Some("run-main".to_owned()),
                boundary: None,
                journal,
                limits: None,
            },
            owner_process,
        )
        .unwrap()
}

/// Starts the exact process identity a retained lifecycle can prove exited.
/// The project PowerShell 7 host is already a required native dependency.
fn spawn_owner() -> (Child, RetainedOwnerProcess) {
    let program = PathBuf::from("C:/Program Files/PowerShell/7/pwsh.exe");
    let child = Command::new(&program)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 900",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the project PowerShell 7 host starts");
    let user = harness_core::process_service::current_user().unwrap();
    let identity =
        harness_core::process_service::ServiceProcess::observe(child.id(), &program, 0, &user)
            .unwrap()
            .identity();
    (child, RetainedOwnerProcess { identity, program })
}

fn stop_owner(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// A live successor proof for the process running these checks.
fn live_self() -> RetainedOwnerProcess {
    let program = std::env::current_exe().unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    let identity = harness_core::process_service::ServiceProcess::observe(
        std::process::id(),
        &program,
        0,
        &user,
    )
    .unwrap()
    .identity();
    RetainedOwnerProcess { identity, program }
}

fn acceptance_for_task(poll: &PollReceipt, task: &str) -> String {
    poll.jobs
        .iter()
        .find(|job| job.task_id == task)
        .unwrap_or_else(|| panic!("no reflection job for task {task}: {poll:?}"))
        .acceptance_id
        .clone()
}

fn unknown_executor_with_partial_accounting(
    board: &Board,
    title: &str,
) -> (ReflectionController, String) {
    let controller = board.controller();
    activate(&controller, JournalPolicy::EnableIfMissing);
    let task = board.create_task(title, Some(Purpose::Ordinary), None);
    board.set_binding(
        &task,
        &binding(&task, Purpose::Ordinary, "partial-accounting"),
    );
    board.close(&task);
    let poll = controller.poll().unwrap();
    let acceptance_id = acceptance_for_task(&poll, &task);
    controller
        .begin_stage(&acceptance_id, Stage::Executor)
        .unwrap();
    controller
        .mark_stage_unknown_with_work_and_usage(
            &acceptance_id,
            Stage::Executor,
            "fixture submitted turn is unknown",
            UsageDelta {
                turns: 1,
                tokens: 100,
            },
            StageWork {
                investigations: 3,
                context_bytes: 500,
            },
            false,
        )
        .unwrap();
    (controller, acceptance_id)
}

fn refusal_codes(refusals: &[EventRefusal]) -> Vec<RefusalCode> {
    refusals.iter().map(|refusal| refusal.code).collect()
}

fn rewrite_state(controller: &ReflectionController, change: impl FnOnce(&mut Value)) {
    let path = controller.state_file().unwrap();
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut value);
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

#[test]
fn accepted_close_is_eligible_and_binding_check_is_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    let controller = board.controller();
    let activation = activate(&controller, JournalPolicy::EnableIfMissing);
    assert_eq!(activation.journal, JournalOwnership::EnabledByController);
    assert_eq!(board.journal(), Some(true));

    let task = board.create_task("Eligible ordinary work", Some(Purpose::Ordinary), None);
    let accepted = binding(&task, Purpose::Ordinary, "one");
    board.set_binding(&task, &accepted);
    board.close(&task);

    let poll = controller.poll().unwrap();
    assert_eq!(poll.model_calls, 0);
    assert_eq!(poll.accepted, 1, "{poll:?}");
    assert_eq!(poll.jobs.len(), 1);
    let acceptance_id = acceptance_for_task(&poll, &task);
    let job = poll.jobs.first().unwrap();
    assert_eq!(job.stage, Stage::Executor);
    assert_eq!(job.source_sessions, vec!["executor-fixture"]);

    let state_file = controller.state_file().unwrap();
    let before = fs::read(&state_file).unwrap();
    let check = controller.check_active_binding(LEAD, Some(&task)).unwrap();
    assert_eq!(check.status, BindingStatus::Active, "{check:?}");
    assert_eq!(
        check.binding_id.as_deref(),
        Some(activation.binding_id.as_str())
    );
    assert_eq!(check.revision, activation.revision);
    assert_eq!(
        check.matched_acceptance_id.as_deref(),
        Some(acceptance_id.as_str())
    );
    assert_eq!(before, fs::read(&state_file).unwrap());
    board.create_task("Unrelated journaled board write", None, None);
    let after_write = controller.check_active_binding(LEAD, Some(&task)).unwrap();
    assert_eq!(after_write.status, BindingStatus::Active, "{after_write:?}");
    assert_eq!(
        controller
            .check_active_binding("other-lead", Some(&task))
            .unwrap()
            .status,
        BindingStatus::WrongLead
    );
    assert_eq!(
        controller
            .check_active_binding(LEAD, Some("unrelated-work"))
            .unwrap()
            .status,
        BindingStatus::UnrelatedWork
    );

    controller.stop().unwrap();
    assert_eq!(
        controller
            .check_active_binding(LEAD, Some(&task))
            .unwrap()
            .status,
        BindingStatus::Stopped
    );
    assert!(controller.poll().is_err());
    let restored = controller.restore_owned_journal().unwrap();
    assert!(restored.journal_restored);
    assert_eq!(board.journal(), None);
}

#[test]
fn activation_refuses_missing_or_explicitly_disabled_journal_and_preserves_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    let controller = board.controller();

    let request = || ActivationRequest {
        lead_session: LEAD.to_owned(),
        run: Some("run-main".to_owned()),
        boundary: None,
        journal: JournalPolicy::RequireExisting,
        limits: None,
    };
    let error = controller.activate(&request()).unwrap_err().to_string();
    assert!(error.contains("events-journal is not enabled"), "{error}");
    assert!(controller.stored_state().unwrap().is_none());
    let inactive = controller.check_active_binding(LEAD, None).unwrap();
    assert_eq!(inactive.status, BindingStatus::Inactive, "{inactive:?}");

    board.json(&["config", "set", "events-journal", "false", "--json"]);
    let mut enable = request();
    enable.journal = JournalPolicy::EnableIfMissing;
    let error = controller.activate(&enable).unwrap_err().to_string();
    assert!(error.contains("explicitly false"), "{error}");
    assert_eq!(board.journal(), Some(false));
    assert!(controller.stored_state().unwrap().is_none());

    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let existing = controller.activate(&request()).unwrap();
    assert_eq!(existing.journal, JournalOwnership::PreexistingEnabled);
    controller.stop().unwrap();
    let restored = controller.restore_owned_journal().unwrap();
    assert!(!restored.journal_restored);
    assert_eq!(board.journal(), Some(true));

    board.unset_journal();
    let owned = controller.activate(&enable).unwrap();
    assert_eq!(owned.journal, JournalOwnership::EnabledByController);
    controller.stop().unwrap();
    let restored = controller.restore_owned_journal().unwrap();
    assert!(restored.journal_restored);
    assert_eq!(board.journal(), None);
}

#[test]
fn initial_reports_rejections_and_nonordinary_lineage_never_trigger() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    activate(&controller, JournalPolicy::RequireExisting);

    let arbitrary = board.create_task("Arbitrary closure", None, None);
    board.close(&arbitrary);

    let awaiting_review = board.create_task("Initial report", Some(Purpose::Ordinary), None);
    board.set_binding(
        &awaiting_review,
        &binding(&awaiting_review, Purpose::Ordinary, "lead-review"),
    );
    board.json(&[
        "comment",
        &awaiting_review,
        "executor initial report, awaiting lead review",
        "--json",
    ]);
    board.close_with_reason(&awaiting_review, "lead_review");

    let cancelled = board.create_task("Cancelled work", Some(Purpose::Ordinary), None);
    board.set_binding(
        &cancelled,
        &binding(&cancelled, Purpose::Ordinary, "cancelled-close"),
    );
    board.close_with_reason(&cancelled, "cancelled");

    let missing_purpose = board.create_task("Missing purpose", None, None);
    board.set_acceptance_only(
        &missing_purpose,
        &binding(&missing_purpose, Purpose::Ordinary, "missing-purpose"),
    );
    board.close(&missing_purpose);

    let rejected = board.create_task("Rejected outcome", Some(Purpose::Ordinary), None);
    let mut rejected_binding = binding(&rejected, Purpose::Ordinary, "rejected");
    let mut rejected_json = serde_json::to_value(rejected_binding.clone()).unwrap();
    rejected_json["outcome"] = json!("rejected");
    board.set_metadata(&rejected, "harness.purpose", Purpose::Ordinary.as_str());
    board.set_metadata(
        &rejected,
        ACCEPTANCE_METADATA,
        &serde_json::to_string(&rejected_json).unwrap(),
    );
    board.close(&rejected);

    let unknown_purpose = board.create_task("Unknown purpose", None, None);
    board.set_binding(
        &unknown_purpose,
        &binding(&unknown_purpose, Purpose::Ordinary, "unknown-purpose"),
    );
    board.set_metadata(&unknown_purpose, PURPOSE_METADATA, "auto");
    board.close(&unknown_purpose);

    let contradictory = board.create_task("Contradictory purpose", Some(Purpose::Ordinary), None);
    board.set_acceptance_only(
        &contradictory,
        &binding(
            &contradictory,
            Purpose::Improvement,
            "contradictory-purpose",
        ),
    );
    board.close(&contradictory);

    let improvement = board.create_task("Improvement closure", Some(Purpose::Improvement), None);
    board.set_binding(
        &improvement,
        &binding(&improvement, Purpose::Improvement, "improvement"),
    );
    board.close(&improvement);

    let reflection = board.create_task("Reflection closure", Some(Purpose::Reflection), None);
    board.set_binding(
        &reflection,
        &binding(&reflection, Purpose::Reflection, "reflection"),
    );
    board.close(&reflection);

    let ancestor = board.create_task("Improvement parent", Some(Purpose::Improvement), None);
    let child = board.create_task(
        "Ordinary descendant",
        Some(Purpose::Ordinary),
        Some(&ancestor),
    );
    board.set_binding(&child, &binding(&child, Purpose::Ordinary, "descendant"));
    board.close(&child);

    let wrong_actor = board.create_task("Wrong close actor", Some(Purpose::Ordinary), None);
    rejected_binding = binding(&wrong_actor, Purpose::Ordinary, "wrong-actor");
    rejected_binding.approved_by = "different-approver".to_owned();
    board.set_binding(&wrong_actor, &rejected_binding);
    board.close(&wrong_actor);

    let wrong_lead = board.create_task("Wrong lead", Some(Purpose::Ordinary), None);
    rejected_binding = binding(&wrong_lead, Purpose::Ordinary, "wrong-lead");
    rejected_binding.lead_session = "different-lead".to_owned();
    board.set_binding(&wrong_lead, &rejected_binding);
    board.close(&wrong_lead);

    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 0, "{poll:?}");
    assert!(poll.jobs.is_empty(), "{poll:?}");
    let codes = refusal_codes(&poll.refusals);
    for expected in [
        RefusalCode::MissingPurpose,
        RefusalCode::OutcomeNotAccepted,
        RefusalCode::CloseReasonNotAccepted,
        RefusalCode::UnknownPurpose,
        RefusalCode::NonTriggerPurpose,
        RefusalCode::ActorMismatch,
        RefusalCode::WrongLead,
    ] {
        assert!(codes.contains(&expected), "missing {expected:?}: {codes:?}");
    }
    assert!(
        controller
            .stored_state()
            .unwrap()
            .unwrap()
            .records
            .is_empty()
    );
}

#[test]
fn queued_stages_revalidate_live_acceptance_and_ancestor_purpose_before_start() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    let controller = board.controller();
    activate(&controller, JournalPolicy::EnableIfMissing);
    let parent = board.create_task("Ordinary parent", Some(Purpose::Ordinary), None);
    let task = board.create_task(
        "Queued accepted work",
        Some(Purpose::Ordinary),
        Some(&parent),
    );
    let accepted = binding(&task, Purpose::Ordinary, "queued");
    board.set_binding(&task, &accepted);
    board.close(&task);
    let poll = controller.poll().unwrap();
    let acceptance_id = acceptance_for_task(&poll, &task);
    let state_file = controller.state_file().unwrap();

    board.reopen(&task);
    let before = fs::read(&state_file).unwrap();
    let error = controller
        .begin_stage(&acceptance_id, Stage::Executor)
        .unwrap_err();
    assert!(error.to_string().contains("closed"), "{error}");
    assert_eq!(before, fs::read(&state_file).unwrap());

    board.close(&task);
    let start = controller
        .begin_stage(&acceptance_id, Stage::Executor)
        .unwrap();
    assert_eq!(start.attempt, 1);
    controller
        .retain_stage(
            &acceptance_id,
            Stage::Executor,
            outcome(Finding::NoFinding, "No useful executor finding"),
            UsageDelta {
                turns: 1,
                tokens: 100,
            },
        )
        .unwrap();

    board.set_metadata(&parent, PURPOSE_METADATA, Purpose::Improvement.as_str());
    let before = fs::read(&state_file).unwrap();
    let check = controller
        .check_active_binding(LEAD, Some(&acceptance_id))
        .unwrap();
    assert_eq!(check.status, BindingStatus::StaleSource, "{check:?}");
    let error = controller
        .begin_stage(&acceptance_id, Stage::Lead)
        .unwrap_err();
    assert!(error.to_string().contains("ancestor"), "{error}");
    assert_eq!(before, fs::read(&state_file).unwrap());

    board.set_metadata(&parent, PURPOSE_METADATA, Purpose::Ordinary.as_str());
    board.set_binding(&task, &binding(&task, Purpose::Ordinary, "superseded"));
    let before = fs::read(&state_file).unwrap();
    assert!(controller.begin_stage(&acceptance_id, Stage::Lead).is_err());
    assert_eq!(before, fs::read(&state_file).unwrap());

    board.set_binding(&task, &accepted);
    let start = controller.begin_stage(&acceptance_id, Stage::Lead).unwrap();
    assert_eq!(start.attempt, 1);
}

#[test]
fn duplicate_reclose_and_new_identity_are_distinct_then_drift_is_exposed() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    activate(&controller, JournalPolicy::RequireExisting);

    let task = board.create_task("Replay ordinary work", Some(Purpose::Ordinary), None);
    let first_binding = binding(&task, Purpose::Ordinary, "first");
    board.set_binding(&task, &first_binding);
    board.close(&task);
    let first = controller.poll().unwrap();
    assert_eq!(first.accepted, 1, "{first:?}");
    let first_id = acceptance_for_task(&first, &task);

    let replay = controller.poll().unwrap();
    assert_eq!(replay.accepted, 0);
    assert_eq!(replay.duplicates, 0);

    board.reopen(&task);
    board.close(&task);
    let reclose = controller.poll().unwrap();
    assert_eq!(reclose.accepted, 0, "{reclose:?}");
    assert_eq!(reclose.duplicates, 1, "{reclose:?}");
    assert_eq!(controller.stored_state().unwrap().unwrap().records.len(), 1);

    board.reopen(&task);
    let second_binding = binding(&task, Purpose::Ordinary, "second");
    board.set_binding(&task, &second_binding);
    board.close(&task);
    let second = controller.poll().unwrap();
    assert_eq!(second.accepted, 1, "{second:?}");
    assert_eq!(controller.stored_state().unwrap().unwrap().records.len(), 2);
    let second_id = second
        .jobs
        .iter()
        .find(|job| job.task_id == task && job.acceptance_id != first_id)
        .unwrap_or_else(|| panic!("no new acceptance job for {task}: {second:?}"))
        .acceptance_id
        .clone();
    assert_ne!(first_id, second_id);
    let matched = controller
        .check_active_binding(LEAD, Some("result-second"))
        .unwrap();
    assert_eq!(matched.status, BindingStatus::Active, "{matched:?}");
    assert_eq!(
        matched.matched_acceptance_id.as_deref(),
        Some(second_id.as_str())
    );

    board.reopen(&task);
    board.set_binding(&task, &binding(&task, Purpose::Ordinary, "drifted"));
    let drifted = controller.check_active_binding(LEAD, Some(&task)).unwrap();
    assert_eq!(drifted.status, BindingStatus::StaleSource, "{drifted:?}");
    assert!(drifted.detail.contains(&task), "{drifted:?}");
    rewrite_state(&controller, |state| {
        state["source"]["branch"] = json!("other-branch");
    });
    assert_eq!(
        controller
            .check_active_binding(LEAD, Some(&task))
            .unwrap()
            .status,
        BindingStatus::StaleSource
    );
    let stale = controller.poll().unwrap();
    assert_eq!(
        stale.recovery.as_ref().unwrap().kind,
        RecoveryKind::BranchChanged
    );
    assert_eq!(
        controller
            .check_active_binding(LEAD, Some(&task))
            .unwrap()
            .status,
        BindingStatus::RecoveryPending
    );
    let error = controller
        .rebaseline_current("fixture branch reconciliation")
        .unwrap_err()
        .to_string();
    assert!(error.contains("drifted"), "{error}");
    let state = controller.stored_state().unwrap().unwrap();
    assert!(
        state
            .recovery
            .unwrap()
            .records
            .iter()
            .any(|record| { record.status == TaskBindingStatus::Drifted })
    );
}

#[test]
fn journal_disable_and_pruned_gap_recover_without_a_silent_cursor_jump() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    activate(&controller, JournalPolicy::RequireExisting);

    let task = board.create_task("Journal baseline", Some(Purpose::Ordinary), None);
    board.set_binding(&task, &binding(&task, Purpose::Ordinary, "baseline"));
    board.close(&task);
    let first = controller.poll().unwrap();
    assert_eq!(first.accepted, 1, "{first:?}");

    board.json(&["config", "set", "events-journal", "false", "--json"]);
    let hidden = board.create_task("Unjournaled closure", Some(Purpose::Ordinary), None);
    board.set_binding(&hidden, &binding(&hidden, Purpose::Ordinary, "hidden"));
    board.close(&hidden);
    let unavailable = controller.poll().unwrap();
    let recovery = unavailable.recovery.as_ref().unwrap();
    assert_eq!(recovery.kind, RecoveryKind::ReplicaChanged, "{recovery:?}");
    assert!(
        recovery.detail.contains("without a journal record"),
        "{recovery:?}"
    );
    assert!(
        recovery
            .records
            .iter()
            .all(|record| record.status == TaskBindingStatus::Bound)
    );
    assert_eq!(
        controller
            .check_active_binding(LEAD, Some(&task))
            .unwrap()
            .status,
        BindingStatus::RecoveryPending
    );

    board.json(&["config", "set", "events-journal", "true", "--json"]);
    controller
        .rebaseline_current("fixture journal restored")
        .unwrap();
    assert!(
        controller
            .stored_state()
            .unwrap()
            .unwrap()
            .recovery
            .is_none()
    );

    board.json(&["config", "set", "events-journal-retain-days", "0", "--json"]);
    board.json(&["config", "set", "events-journal-retain-rows", "0", "--json"]);
    board.json(&[
        "config",
        "set",
        "events-journal-auto-prune",
        "false",
        "--json",
    ]);
    let checkpoint = controller.stored_state().unwrap().unwrap().checkpoint;
    for index in 0..6 {
        board.create_task(&format!("Gap task {index}"), None, None);
    }
    let head = board.event_head(checkpoint);
    assert!(
        head > checkpoint + 2,
        "checkpoint {checkpoint}, head {head}"
    );
    board.json(&[
        "events",
        "prune",
        "--before",
        &(checkpoint + 2).to_string(),
        "--json",
    ]);

    let gap = controller.poll().unwrap();
    let recovery = gap.recovery.as_ref().unwrap();
    assert_eq!(recovery.kind, RecoveryKind::EventGap, "{recovery:?}");
    assert_eq!(recovery.floor, Some(checkpoint + 2));
    assert_eq!(recovery.head, Some(head));
    assert_eq!(gap.checkpoint, checkpoint);
    let resumed = controller.resume_from_floor().unwrap();
    assert_eq!(resumed.checkpoint, checkpoint + 1);
    let after = controller.poll().unwrap();
    assert!(after.recovery.is_none(), "{after:?}");
    assert_eq!(after.checkpoint, head);

    rewrite_state(&controller, |state| {
        state["source"]["project_id"] = json!("different-project");
    });
    let replica = controller.poll().unwrap();
    assert_eq!(
        replica.recovery.as_ref().unwrap().kind,
        RecoveryKind::ReplicaChanged
    );
    assert_eq!(
        controller
            .check_active_binding(LEAD, Some(&task))
            .unwrap()
            .status,
        BindingStatus::WrongProject
    );
    controller
        .rebaseline_current("fixture replica reconciled")
        .unwrap();
}

#[test]
fn durable_chain_unknown_capacity_and_cumulative_limits_survive_restart() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    activate(&controller, JournalPolicy::RequireExisting);

    let task_a = board.create_task("Chain A", Some(Purpose::Ordinary), None);
    let task_b = board.create_task("Chain B", Some(Purpose::Ordinary), None);
    let task_c = board.create_task("Chain C", Some(Purpose::Ordinary), None);
    let task_d = board.create_task("Chain D", Some(Purpose::Ordinary), None);
    for (task, suffix) in [
        (&task_a, "a"),
        (&task_b, "b"),
        (&task_c, "c"),
        (&task_d, "d"),
    ] {
        board.set_binding(task, &binding(task, Purpose::Ordinary, suffix));
        board.close(task);
    }
    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 4, "{poll:?}");
    let id_a = acceptance_for_task(&poll, &task_a);
    let id_b = acceptance_for_task(&poll, &task_b);
    let id_c = acceptance_for_task(&poll, &task_c);
    let id_d = acceptance_for_task(&poll, &task_d);

    let executor = controller.begin_stage(&id_a, Stage::Executor).unwrap();
    assert_eq!(executor.attempt, 1);
    assert_eq!(executor.source_sessions, vec!["executor-fixture"]);
    let retained = controller
        .retain_stage(
            &id_a,
            Stage::Executor,
            outcome(Finding::NoFinding, "no finding is a valid retained outcome"),
            UsageDelta {
                turns: 1,
                tokens: 10,
            },
        )
        .unwrap();
    assert!(retained.next_stage_eligible);
    let lead = controller.begin_stage(&id_a, Stage::Lead).unwrap();
    assert_eq!(lead.source_sessions, vec![LEAD]);
    let delivered = controller
        .retain_stage(
            &id_a,
            Stage::Lead,
            outcome(Finding::Findings, "one bounded recommendation"),
            UsageDelta {
                turns: 1,
                tokens: 5,
            },
        )
        .unwrap();
    assert_eq!(delivered.delivery, DeliveryState::Pending);
    assert!(
        controller
            .pending_jobs()
            .unwrap()
            .iter()
            .all(|job| job.acceptance_id != id_a)
    );
    let first_delivery = controller.observe_delivery(&id_a, "delivery-one").unwrap();
    assert!(!first_delivery.duplicate);
    let repeated = controller.observe_delivery(&id_a, "delivery-one").unwrap();
    assert!(repeated.duplicate);
    assert!(
        controller
            .observe_delivery(&id_a, "different-receipt")
            .is_err()
    );

    let restarted = board.controller();
    let record = restarted.record(&id_a).unwrap().unwrap();
    assert_eq!(
        record.lead.state,
        harness_core::delivery_reflection::StageState::Retained
    );
    assert_eq!(record.delivery, DeliveryState::Consumed);
    assert_eq!(record.usage_totals().tokens, 15);

    let capacity = ReflectionLimits {
        max_active_stages: 1,
        ..ReflectionLimits::default()
    };
    restarted.configure_limits(&capacity).unwrap();
    restarted.begin_stage(&id_b, Stage::Executor).unwrap();
    let error = restarted
        .begin_stage(&id_c, Stage::Executor)
        .unwrap_err()
        .to_string();
    assert!(error.contains("configured capacity is 1"), "{error}");

    restarted
        .mark_stage_unknown(&id_b, Stage::Executor, "fixture interruption")
        .unwrap();
    let error = restarted
        .retry_stage(&id_b, Stage::Executor)
        .unwrap_err()
        .to_string();
    assert!(error.contains("usage reconciled"), "{error}");
    restarted
        .record_unknown_usage(
            &id_b,
            Stage::Executor,
            UsageDelta {
                turns: 1,
                tokens: 5,
            },
        )
        .unwrap();
    restarted.retry_stage(&id_b, Stage::Executor).unwrap();
    let retried = restarted.begin_stage(&id_b, Stage::Executor).unwrap();
    assert_eq!(retried.attempt, 2);
    assert_eq!(retried.usage.tokens, 5);
    restarted
        .fail_stage(
            &id_b,
            Stage::Executor,
            "fixture retry failed",
            UsageDelta {
                turns: 1,
                tokens: 2,
            },
        )
        .unwrap();
    restarted.begin_stage(&id_c, Stage::Executor).unwrap();
    restarted
        .mark_stage_unknown(&id_c, Stage::Executor, "fixture capacity recovery")
        .unwrap();
    restarted
        .record_unknown_usage(
            &id_c,
            Stage::Executor,
            UsageDelta {
                turns: 1,
                tokens: 1,
            },
        )
        .unwrap();
    restarted.retry_stage(&id_c, Stage::Executor).unwrap();
    restarted.begin_stage(&id_c, Stage::Executor).unwrap();
    restarted
        .fail_stage(
            &id_c,
            Stage::Executor,
            "fixture released capacity",
            UsageDelta::default(),
        )
        .unwrap();

    let finite = ReflectionLimits {
        max_tokens_per_stage: 20,
        max_tokens_per_record: 20,
        ..ReflectionLimits::default()
    };
    restarted.configure_limits(&finite).unwrap();
    restarted.begin_stage(&id_d, Stage::Executor).unwrap();
    restarted
        .retain_stage(
            &id_d,
            Stage::Executor,
            outcome(Finding::NoFinding, "bounded no-finding result"),
            UsageDelta {
                turns: 1,
                tokens: 20,
            },
        )
        .unwrap();
    let error = restarted
        .begin_stage(&id_d, Stage::Lead)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("token allowance across both stages"),
        "{error}"
    );
    let error = board
        .controller()
        .begin_stage(&id_d, Stage::Lead)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("token allowance across both stages"),
        "{error}"
    );
    let record = restarted.record(&id_d).unwrap().unwrap();
    assert_eq!(record.usage_totals().tokens, 20);
    assert_eq!(record.executor.usage.attempts, 1);
}

#[test]
fn explicit_missing_accounting_adds_to_the_submitted_cost() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    let (controller, acceptance_id) =
        unknown_executor_with_partial_accounting(&board, "Explicit missing reflection accounting");

    controller
        .record_unknown_usage(
            &acceptance_id,
            Stage::Executor,
            UsageDelta {
                turns: 0,
                tokens: 20,
            },
        )
        .unwrap();
    controller
        .record_observed_work(
            &acceptance_id,
            Stage::Executor,
            StageWork {
                investigations: 1,
                context_bytes: 30,
            },
        )
        .unwrap();

    let record = controller.record(&acceptance_id).unwrap().unwrap();
    assert_eq!(record.executor.usage.turns, 1);
    assert_eq!(record.executor.usage.tokens, 120);
    assert_eq!(record.executor.usage.investigations, 4);
    assert_eq!(record.executor.usage.context_bytes, 530);
    assert!(record.executor.usage_reconciled);
    assert!(record.executor.work_reconciled);
    let submitted = SubmittedAccounting {
        usage: UsageDelta {
            turns: 1,
            tokens: 120,
        },
        work: StageWork {
            investigations: 4,
            context_bytes: 530,
        },
    };
    assert_eq!(record.executor.submitted_accounting, Some(submitted));

    // A later complete cumulative observation covers the explicit additions,
    // so replacing the submitted contribution must not charge them again.
    controller
        .reopen_submitted_stage(&acceptance_id, Stage::Executor, submitted)
        .unwrap();
    controller
        .retain_stage_with_work(
            &acceptance_id,
            Stage::Executor,
            outcome(Finding::NoFinding, "completed after explicit additions"),
            submitted.usage,
            submitted.work,
        )
        .unwrap();
    let completed = controller.record(&acceptance_id).unwrap().unwrap();
    assert_eq!(completed.executor.usage.turns, 1);
    assert_eq!(completed.executor.usage.tokens, 120);
    assert_eq!(completed.executor.usage.investigations, 4);
    assert_eq!(completed.executor.usage.context_bytes, 530);
    assert!(completed.executor.usage_reconciled);
    assert!(completed.executor.work_reconciled);
    assert_eq!(completed.executor.submitted_accounting, None);
}

#[test]
fn submitted_replacement_never_erases_known_cost_or_double_charges_completion() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    let (controller, acceptance_id) =
        unknown_executor_with_partial_accounting(&board, "Submitted replacement lower bound");
    let submitted = SubmittedAccounting {
        usage: UsageDelta {
            turns: 1,
            tokens: 100,
        },
        work: StageWork {
            investigations: 3,
            context_bytes: 500,
        },
    };

    controller
        .reopen_submitted_stage(&acceptance_id, Stage::Executor, submitted)
        .unwrap();
    controller
        .mark_stage_unknown_with_work_and_usage(
            &acceptance_id,
            Stage::Executor,
            "native retrieval became unavailable",
            UsageDelta {
                turns: 1,
                tokens: 0,
            },
            StageWork::default(),
            false,
        )
        .unwrap();
    let unavailable = controller.record(&acceptance_id).unwrap().unwrap();
    assert_eq!(
        unavailable.executor.state,
        harness_core::delivery_reflection::StageState::Unknown
    );
    assert_eq!(unavailable.executor.usage.turns, 1);
    assert_eq!(unavailable.executor.usage.tokens, 100);
    assert_eq!(unavailable.executor.usage.investigations, 3);
    assert_eq!(unavailable.executor.usage.context_bytes, 500);
    assert!(!unavailable.executor.usage_reconciled);
    assert!(!unavailable.executor.work_reconciled);
    assert_eq!(unavailable.executor.submitted_accounting, Some(submitted));

    let decreased_accounting = SubmittedAccounting {
        usage: UsageDelta {
            turns: 1,
            tokens: 80,
        },
        work: StageWork {
            investigations: 2,
            context_bytes: 400,
        },
    };
    controller
        .reopen_submitted_stage(&acceptance_id, Stage::Executor, decreased_accounting)
        .unwrap();
    let reopened_lower = controller.record(&acceptance_id).unwrap().unwrap();
    assert_eq!(reopened_lower.executor.usage.tokens, 100);
    assert_eq!(reopened_lower.executor.usage.investigations, 3);
    assert_eq!(reopened_lower.executor.usage.context_bytes, 500);
    assert_eq!(
        reopened_lower.executor.submitted_accounting,
        Some(submitted)
    );
    controller
        .mark_stage_unknown_with_work_and_usage(
            &acceptance_id,
            Stage::Executor,
            "native retrieval decreased",
            UsageDelta {
                turns: 1,
                tokens: 80,
            },
            StageWork {
                investigations: 2,
                context_bytes: 400,
            },
            false,
        )
        .unwrap();
    let decreasing = controller.record(&acceptance_id).unwrap().unwrap();
    assert_eq!(decreasing.executor.usage.tokens, 100);
    assert_eq!(decreasing.executor.usage.investigations, 3);
    assert_eq!(decreasing.executor.usage.context_bytes, 500);
    assert!(!decreasing.executor.usage_reconciled);
    assert!(!decreasing.executor.work_reconciled);
    assert_eq!(decreasing.executor.submitted_accounting, Some(submitted));
    assert!(
        decreasing
            .executor
            .usage
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("already observed lower bound"))
    );

    controller
        .reopen_submitted_stage(&acceptance_id, Stage::Executor, submitted)
        .unwrap();
    controller
        .retain_stage_with_work(
            &acceptance_id,
            Stage::Executor,
            outcome(
                Finding::NoFinding,
                "completed after repeated reconciliation",
            ),
            UsageDelta {
                turns: 1,
                tokens: 120,
            },
            StageWork {
                investigations: 4,
                context_bytes: 600,
            },
        )
        .unwrap();
    let completed = controller.record(&acceptance_id).unwrap().unwrap();
    assert_eq!(completed.executor.usage.turns, 1);
    assert_eq!(completed.executor.usage.tokens, 120);
    assert_eq!(completed.executor.usage.investigations, 4);
    assert_eq!(completed.executor.usage.context_bytes, 600);
    assert!(completed.executor.usage_reconciled);
    assert!(completed.executor.work_reconciled);
    assert_eq!(completed.executor.submitted_accounting, None);
}

#[test]
fn acceptance_binds_exact_native_turns_and_identity_ignores_a_later_lead_turn() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    let controller = board.controller();
    activate(&controller, JournalPolicy::EnableIfMissing);
    let task = board.create_task("Exact turn binding", Some(Purpose::Ordinary), None);
    let accepted = binding(&task, Purpose::Ordinary, "turn").with_turns(
        Some("executor-fixture".to_owned()),
        Some("executor-turn-1".to_owned()),
        Some("lead-turn-1".to_owned()),
    );
    let acceptance_id = accepted.acceptance_id().unwrap();
    board.set_binding(&task, &accepted);
    board.close(&task);

    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 1, "{poll:?}");
    let job = poll
        .jobs
        .iter()
        .find(|job| job.task_id == task)
        .expect("accepted ordinary closure yields one job");
    assert_eq!(job.executor_session.as_deref(), Some("executor-fixture"));
    assert_eq!(job.executor_turn.as_deref(), Some("executor-turn-1"));
    assert_eq!(job.lead_turn.as_deref(), Some("lead-turn-1"));

    let start = controller
        .begin_stage(&acceptance_id, Stage::Executor)
        .unwrap();
    assert_eq!(start.executor_session.as_deref(), Some("executor-fixture"));
    assert_eq!(start.executor_turn.as_deref(), Some("executor-turn-1"));
    assert_eq!(start.lead_turn.as_deref(), Some("lead-turn-1"));

    // The accepting lead turn may still be running when the closure lands.
    // Recording a different lead turn for the same accepted executor work must
    // not fork the one closure into two records.
    let later = accepted.clone().with_turns(
        Some("executor-fixture".to_owned()),
        Some("executor-turn-1".to_owned()),
        Some("lead-turn-2".to_owned()),
    );
    assert_eq!(later.acceptance_id().unwrap(), acceptance_id);
}

#[test]
fn acceptance_turn_identities_are_trimmed_and_must_name_a_recorded_session() {
    let normalized = binding("bdct-task", Purpose::Ordinary, "trimmed")
        .with_turns(
            Some(" executor-fixture ".to_owned()),
            Some("  ".to_owned()),
            None,
        )
        .normalized()
        .unwrap();
    assert_eq!(
        normalized.executor_session.as_deref(),
        Some("executor-fixture")
    );
    assert_eq!(normalized.executor_turn, None);
    assert_eq!(normalized.lead_turn, None);

    let error = binding("bdct-task", Purpose::Ordinary, "mismatch")
        .with_turns(Some("another-session".to_owned()), None, None)
        .normalized()
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("is not one of executor_sessions"),
        "{error}"
    );
}

#[test]
fn succession_preserves_retained_work_and_refuses_retired_owner_reclaim() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    let (mut owner, owner_process) = spawn_owner();
    activate_with_process(&controller, JournalPolicy::RequireExisting, &owner_process);
    let task = board.create_task("Succession work", Some(Purpose::Ordinary), None);
    board.set_binding(&task, &binding(&task, Purpose::Ordinary, "succession"));
    board.close(&task);
    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 1, "{poll:?}");
    let acceptance = acceptance_for_task(&poll, &task);
    let before = controller.stored_state().unwrap().unwrap();

    let state_file = controller.state_file().unwrap();
    let settled_before_succession = fs::read(&state_file).unwrap();
    let live = controller
        .succeed(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(live.contains("still live"), "{live}");
    assert_eq!(fs::read(&state_file).unwrap(), settled_before_succession);

    stop_owner(&mut owner);
    let receipt = controller
        .succeed(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap();
    assert!(receipt.changed);
    assert_eq!(receipt.previous_lead_session, LEAD);
    assert_eq!(receipt.lead_session, SUCCESSOR);
    assert_eq!(receipt.binding_id, before.binding.id);
    assert_eq!(receipt.revision, before.binding.revision + 1);

    let after = controller.stored_state().unwrap().unwrap();
    assert_eq!(after.binding.lead_session, SUCCESSOR);
    assert_eq!(after.binding.run, before.binding.run);
    assert_eq!(after.source, before.source);
    assert_eq!(after.journal, before.journal);
    assert_eq!(after.checkpoint, before.checkpoint);
    assert_eq!(after.current_session_floor, before.current_session_floor);
    assert_eq!(after.records, before.records);
    assert_eq!(
        after.records.get(&acceptance).unwrap().binding.lead_session,
        LEAD,
        "accepted provenance keeps the original lead"
    );
    assert_eq!(after.limits, before.limits);
    assert_eq!(after.recovery, before.recovery);
    assert_eq!(after.last_poll, before.last_poll);
    assert_eq!(after.refusals, before.refusals);
    assert_eq!(after.stopped_at, before.stopped_at);
    assert_eq!(after.last_recovery_evidence, before.last_recovery_evidence);
    assert_eq!(after.retired_leads, vec![LEAD.to_owned()]);
    assert_eq!(
        controller
            .check_active_binding(SUCCESSOR, Some(&acceptance))
            .unwrap()
            .status,
        BindingStatus::Active
    );
    assert_eq!(
        controller
            .check_active_binding(LEAD, Some(&acceptance))
            .unwrap()
            .status,
        BindingStatus::WrongLead
    );

    let settled = fs::read(&state_file).unwrap();
    let repeat = controller
        .succeed(&SuccessionRequest {
            expected_lead_session: SUCCESSOR.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap();
    assert!(!repeat.changed);
    assert_eq!(repeat.revision, receipt.revision);
    assert_eq!(fs::read(&state_file).unwrap(), settled);

    let stale = controller
        .succeed(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: "another-successor".to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(stale.contains("ownership changed"), "{stale}");
    assert_eq!(fs::read(&state_file).unwrap(), settled);

    let reclaim = controller
        .succeed(&SuccessionRequest {
            expected_lead_session: SUCCESSOR.to_owned(),
            successor_lead: LEAD.to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(reclaim.contains("already superseded"), "{reclaim}");
    assert_eq!(fs::read(&state_file).unwrap(), settled);

    let activate_old = controller
        .activate(&ActivationRequest {
            lead_session: LEAD.to_owned(),
            run: Some("run-main".to_owned()),
            boundary: None,
            journal: JournalPolicy::RequireExisting,
            limits: None,
        })
        .unwrap_err()
        .to_string();
    assert!(activate_old.contains("cannot reclaim"), "{activate_old}");
    assert_eq!(fs::read(&state_file).unwrap(), settled);
}

#[test]
fn succession_refuses_stopped_and_recovery_pending_lifecycles() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    activate(&controller, JournalPolicy::RequireExisting);
    controller.stop().unwrap();
    let state_file = controller.state_file().unwrap();
    let stopped = fs::read(&state_file).unwrap();
    let error = controller
        .succeed(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("stopped"), "{error}");
    assert_eq!(fs::read(&state_file).unwrap(), stopped);

    activate(&controller, JournalPolicy::RequireExisting);
    board.json(&["config", "set", "events-journal", "false", "--json"]);
    let hidden = board.create_task("Unjournaled succession work", Some(Purpose::Ordinary), None);
    board.set_binding(&hidden, &binding(&hidden, Purpose::Ordinary, "unjournaled"));
    board.close(&hidden);
    let polled = controller.poll().unwrap();
    assert!(polled.recovery.is_some(), "{polled:?}");
    let pending = fs::read(&state_file).unwrap();
    let error = controller
        .succeed(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("reflection recovery is pending"), "{error}");
    assert_eq!(fs::read(&state_file).unwrap(), pending);
}

#[test]
fn stopped_current_session_succession_retains_predecessor_work_without_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    let (mut owner, owner_process) = spawn_owner();
    activate_with_process(&controller, JournalPolicy::RequireExisting, &owner_process);
    let task = board.create_task("Stopped succession work", Some(Purpose::Ordinary), None);
    board.set_binding(
        &task,
        &binding(&task, Purpose::Ordinary, "stopped-succession"),
    );
    board.close(&task);
    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 1, "{poll:?}");
    let acceptance = acceptance_for_task(&poll, &task);

    // This predecessor closure is committed but deliberately never polled.
    // Automatic succession must consume it without creating historical work.
    let historical = board.create_task("Unpolled predecessor work", Some(Purpose::Ordinary), None);
    let historical_binding = binding(&historical, Purpose::Ordinary, "unpolled-predecessor");
    board.set_binding(&historical, &historical_binding);
    board.close(&historical);
    let historical_acceptance = historical_binding.acceptance_id().unwrap();

    controller.stop().unwrap();
    let before = controller.stored_state().unwrap().unwrap();
    assert_eq!(before.lifecycle, LifecycleStatus::Stopped);
    assert!(before.stopped_at.is_some());
    stop_owner(&mut owner);

    let state_file = controller.state_file().unwrap();
    let receipt = controller
        .succeed_current_session(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap();
    assert!(receipt.changed);
    assert_eq!(receipt.previous_lead_session, LEAD);
    assert_eq!(receipt.lead_session, SUCCESSOR);
    assert_eq!(receipt.binding_id, before.binding.id);
    assert_eq!(receipt.revision, before.binding.revision + 1);

    let after = controller.stored_state().unwrap().unwrap();
    assert_eq!(after.lifecycle, LifecycleStatus::Active);
    assert_eq!(after.stopped_at, None);
    assert_eq!(after.binding.lead_session, SUCCESSOR);
    assert_eq!(after.binding.run, before.binding.run);
    assert_eq!(after.source, before.source);
    assert_eq!(after.journal, before.journal);
    assert_eq!(after.checkpoint, before.checkpoint);
    let floor = after.current_session_floor.unwrap();
    assert!(
        floor > before.checkpoint,
        "the boundary must include the unpolled predecessor closure: floor {floor}, checkpoint {}",
        before.checkpoint
    );
    assert_eq!(after.records, before.records);
    assert_eq!(
        after.records.get(&acceptance).unwrap().binding.lead_session,
        LEAD,
        "accepted provenance keeps the original lead"
    );
    assert_eq!(after.limits, before.limits);
    assert_eq!(after.recovery, before.recovery);
    assert_eq!(after.last_poll, before.last_poll);
    assert_eq!(after.refusals, before.refusals);
    assert_eq!(after.last_recovery_evidence, before.last_recovery_evidence);
    assert_eq!(after.retired_leads, vec![LEAD.to_owned()]);
    assert_eq!(
        controller
            .check_active_binding(SUCCESSOR, Some(&acceptance))
            .unwrap()
            .status,
        BindingStatus::Active
    );
    assert!(
        controller.pending_jobs().unwrap().is_empty(),
        "predecessor work must remain retained but not dispatchable"
    );
    assert_eq!(
        controller
            .record(&acceptance)
            .unwrap()
            .unwrap()
            .executor
            .state,
        StageState::Pending
    );

    let settled = fs::read(&state_file).unwrap();
    let repeat = controller
        .succeed_current_session(&SuccessionRequest {
            expected_lead_session: SUCCESSOR.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap();
    assert!(!repeat.changed);
    assert_eq!(repeat.revision, receipt.revision);
    assert_eq!(fs::read(&state_file).unwrap(), settled);

    let skipped = controller.poll().unwrap();
    assert_eq!(skipped.accepted, 0, "{skipped:?}");
    assert!(skipped.ignored >= 1, "{skipped:?}");
    assert!(
        controller.record(&historical_acceptance).unwrap().is_none(),
        "an unpolled predecessor closure was backfilled"
    );
    assert!(controller.stored_state().unwrap().unwrap().checkpoint >= floor);

    let current = board.create_task("Current-session work", Some(Purpose::Ordinary), None);
    let mut current_binding = binding(&current, Purpose::Ordinary, "current-session");
    current_binding.lead_session = SUCCESSOR.to_owned();
    board.set_binding(&current, &current_binding);
    board.close(&current);
    let polled = controller.poll().unwrap();
    assert_eq!(polled.accepted, 1, "{polled:?}");
    assert_eq!(polled.jobs.len(), 1, "{polled:?}");
    let current_acceptance = acceptance_for_task(&polled, &current);
    assert_eq!(polled.jobs[0].acceptance_id, current_acceptance);
    assert_ne!(current_acceptance, acceptance);
    let after_current = controller.stored_state().unwrap().unwrap();
    assert_eq!(after_current.current_session_floor, Some(floor));
    assert!(after_current.records.contains_key(&acceptance));
    assert_eq!(
        after_current.records[&acceptance].executor.state,
        StageState::Pending,
        "retained predecessor history is unchanged"
    );
    assert_eq!(
        controller.pending_jobs().unwrap()[0].acceptance_id,
        current_acceptance
    );
}

#[test]
fn active_current_session_succession_sets_the_no_backfill_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    let (mut owner, owner_process) = spawn_owner();
    activate_with_process(&controller, JournalPolicy::RequireExisting, &owner_process);
    let task = board.create_task("Active succession work", Some(Purpose::Ordinary), None);
    board.set_binding(
        &task,
        &binding(&task, Purpose::Ordinary, "active-current-session"),
    );
    board.close(&task);
    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 1, "{poll:?}");
    let acceptance = acceptance_for_task(&poll, &task);
    let before = controller.stored_state().unwrap().unwrap();
    assert_eq!(before.lifecycle, LifecycleStatus::Active);
    stop_owner(&mut owner);

    let receipt = controller
        .succeed_current_session(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap();
    assert!(receipt.changed);
    let after = controller.stored_state().unwrap().unwrap();
    assert_eq!(after.current_session_floor, Some(before.checkpoint));
    assert_eq!(after.records, before.records);
    assert!(controller.pending_jobs().unwrap().is_empty());
    assert_eq!(
        controller
            .record(&acceptance)
            .unwrap()
            .unwrap()
            .executor
            .state,
        StageState::Pending
    );
}

#[test]
fn same_owner_resume_consumes_the_stopped_interval_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    let (mut owner, owner_process) = spawn_owner();
    activate_with_process(&controller, JournalPolicy::RequireExisting, &owner_process);
    let task = board.create_task("Current session work", Some(Purpose::Ordinary), None);
    board.set_binding(
        &task,
        &binding(&task, Purpose::Ordinary, "same-owner-resume"),
    );
    board.close(&task);
    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 1, "{poll:?}");
    let acceptance = acceptance_for_task(&poll, &task);

    controller.stop().unwrap();
    let stopped = board.create_task("Stopped interval work", Some(Purpose::Ordinary), None);
    let stopped_binding = binding(&stopped, Purpose::Ordinary, "stopped-interval");
    board.set_binding(&stopped, &stopped_binding);
    board.close(&stopped);
    let stopped_acceptance = stopped_binding.acceptance_id().unwrap();
    let before = controller.stored_state().unwrap().unwrap();

    // The retained app-server process stays the same, so false-to-true is a
    // resume of the current session rather than a new lifetime. The stopped
    // interval is consumed, but the pending acceptance remains dispatchable.
    let receipt = controller
        .activate_after_stop_with_owner_process(
            &ActivationRequest {
                lead_session: LEAD.to_owned(),
                run: Some("run-main".to_owned()),
                boundary: None,
                journal: JournalPolicy::EnableIfMissing,
                limits: None,
            },
            &owner_process,
        )
        .unwrap();
    let after = controller.stored_state().unwrap().unwrap();
    assert_eq!(after.lifecycle, LifecycleStatus::Active);
    assert_eq!(after.stopped_at, None);
    assert!(after.checkpoint > before.checkpoint);
    assert_eq!(receipt.checkpoint, after.checkpoint);
    assert_eq!(after.records, before.records);
    assert_eq!(after.current_session_floor, before.current_session_floor);
    assert!(controller.record(&stopped_acceptance).unwrap().is_none());
    assert_eq!(
        controller.pending_jobs().unwrap()[0].acceptance_id,
        acceptance
    );
    let polled = controller.poll().unwrap();
    assert_eq!(polled.accepted, 0, "{polled:?}");
    assert!(controller.record(&stopped_acceptance).unwrap().is_none());
    stop_owner(&mut owner);
}

#[test]
fn new_process_lifetime_activation_keeps_predecessor_work_dormant() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    let (mut owner, owner_process) = spawn_owner();
    activate_with_process(&controller, JournalPolicy::RequireExisting, &owner_process);
    let task = board.create_task("Predecessor active work", Some(Purpose::Ordinary), None);
    board.set_binding(
        &task,
        &binding(&task, Purpose::Ordinary, "predecessor-active"),
    );
    board.close(&task);
    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 1, "{poll:?}");
    let acceptance = acceptance_for_task(&poll, &task);

    // The predecessor leaves a committed but never-polled closure behind.
    let historical = board.create_task("Unpolled predecessor work", Some(Purpose::Ordinary), None);
    let historical_binding = binding(&historical, Purpose::Ordinary, "unpolled-active");
    board.set_binding(&historical, &historical_binding);
    board.close(&historical);
    let historical_acceptance = historical_binding.acceptance_id().unwrap();

    let before = controller.stored_state().unwrap().unwrap();
    assert_eq!(before.lifecycle, LifecycleStatus::Active);
    let head = board.event_head(before.checkpoint);
    assert!(
        head > before.checkpoint,
        "head {head}, checkpoint {}",
        before.checkpoint
    );
    stop_owner(&mut owner);

    // Native resume keeps the same lead thread but creates a new app-server
    // process lifetime. The same thread id alone must not revive predecessor
    // records or unpolled predecessor events.
    let receipt = controller
        .activate_with_owner_process(
            &ActivationRequest {
                lead_session: LEAD.to_owned(),
                run: Some("run-main".to_owned()),
                boundary: None,
                journal: JournalPolicy::RequireExisting,
                limits: None,
            },
            &live_self(),
        )
        .unwrap();
    assert!(receipt.reused);
    let after = controller.stored_state().unwrap().unwrap();
    assert_eq!(after.lifecycle, LifecycleStatus::Active);
    assert_eq!(after.binding.lead_session, LEAD);
    assert_eq!(after.owner_process, Some(live_self()));
    assert_eq!(after.checkpoint, before.checkpoint);
    assert_eq!(after.records, before.records);
    let floor = after
        .current_session_floor
        .expect("a new process lifetime establishes the session boundary");
    assert!(floor >= head, "floor {floor}, head {head}");
    assert!(floor > before.checkpoint);
    assert!(
        controller.pending_jobs().unwrap().is_empty(),
        "predecessor work must remain retained but not dispatchable"
    );
    assert_eq!(
        controller
            .record(&acceptance)
            .unwrap()
            .unwrap()
            .executor
            .state,
        StageState::Pending
    );

    let skipped = controller.poll().unwrap();
    assert_eq!(skipped.accepted, 0, "{skipped:?}");
    assert!(skipped.ignored >= 1, "{skipped:?}");
    assert!(
        controller.record(&historical_acceptance).unwrap().is_none(),
        "an unpolled predecessor closure was backfilled"
    );
    assert!(controller.stored_state().unwrap().unwrap().checkpoint >= floor);

    // Work accepted during the new lifetime remains eligible exactly once.
    let current = board.create_task("New-lifetime work", Some(Purpose::Ordinary), None);
    board.set_binding(
        &current,
        &binding(&current, Purpose::Ordinary, "new-lifetime"),
    );
    board.close(&current);
    let polled = controller.poll().unwrap();
    assert_eq!(polled.accepted, 1, "{polled:?}");
    assert_eq!(polled.jobs.len(), 1, "{polled:?}");
    let current_acceptance = acceptance_for_task(&polled, &current);
    assert_eq!(polled.jobs[0].acceptance_id, current_acceptance);
    assert_ne!(current_acceptance, acceptance);
    let after_current = controller.stored_state().unwrap().unwrap();
    assert_eq!(after_current.current_session_floor, Some(floor));
    assert!(after_current.records.contains_key(&acceptance));
    assert_eq!(
        after_current.records[&acceptance].executor.state,
        StageState::Pending,
        "retained predecessor history is unchanged"
    );
    assert_eq!(
        controller.pending_jobs().unwrap()[0].acceptance_id,
        current_acceptance
    );
}

#[test]
fn retain_owner_process_establishes_a_new_lifetime_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    let (mut owner, owner_process) = spawn_owner();
    activate_with_process(&controller, JournalPolicy::RequireExisting, &owner_process);
    let task = board.create_task("Retained predecessor work", Some(Purpose::Ordinary), None);
    board.set_binding(
        &task,
        &binding(&task, Purpose::Ordinary, "retained-predecessor"),
    );
    board.close(&task);
    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 1, "{poll:?}");
    let acceptance = acceptance_for_task(&poll, &task);

    let historical = board.create_task("Unpolled retained work", Some(Purpose::Ordinary), None);
    let historical_binding = binding(&historical, Purpose::Ordinary, "unpolled-retained");
    board.set_binding(&historical, &historical_binding);
    board.close(&historical);
    let historical_acceptance = historical_binding.acceptance_id().unwrap();
    let before = controller.stored_state().unwrap().unwrap();
    let head = board.event_head(before.checkpoint);
    assert!(
        head > before.checkpoint,
        "head {head}, checkpoint {}",
        before.checkpoint
    );
    stop_owner(&mut owner);

    // This is the manual continuation/first-process route: the same lead thread
    // is observed under a new app-server lifetime without going through
    // activation. Refreshing its proof must establish the same session boundary.
    controller.retain_owner_process(LEAD, &live_self()).unwrap();
    let after = controller.stored_state().unwrap().unwrap();
    assert_eq!(after.lifecycle, LifecycleStatus::Active);
    assert_eq!(after.binding.lead_session, LEAD);
    assert_eq!(after.owner_process, Some(live_self()));
    assert_eq!(after.checkpoint, before.checkpoint);
    assert_eq!(after.records, before.records);
    let floor = after
        .current_session_floor
        .expect("a new process lifetime establishes the session boundary");
    assert!(floor >= head, "floor {floor}, head {head}");
    assert!(controller.pending_jobs().unwrap().is_empty());
    assert_eq!(
        controller
            .record(&acceptance)
            .unwrap()
            .unwrap()
            .executor
            .state,
        StageState::Pending,
        "retained predecessor history is unchanged"
    );

    let state_file = controller.state_file().unwrap();
    let settled = fs::read(&state_file).unwrap();
    controller.retain_owner_process(LEAD, &live_self()).unwrap();
    assert_eq!(
        fs::read(&state_file).unwrap(),
        settled,
        "an unchanged process identity is a no-op"
    );

    let skipped = controller.poll().unwrap();
    assert_eq!(skipped.accepted, 0, "{skipped:?}");
    assert!(skipped.ignored >= 1, "{skipped:?}");
    assert!(controller.record(&historical_acceptance).unwrap().is_none());

    let current = board.create_task("New retained-lifetime work", Some(Purpose::Ordinary), None);
    board.set_binding(
        &current,
        &binding(&current, Purpose::Ordinary, "new-retained-lifetime"),
    );
    board.close(&current);
    let polled = controller.poll().unwrap();
    assert_eq!(polled.accepted, 1, "{polled:?}");
    let current_acceptance = acceptance_for_task(&polled, &current);
    assert_eq!(polled.jobs[0].acceptance_id, current_acceptance);
    assert_eq!(
        controller.pending_jobs().unwrap()[0].acceptance_id,
        current_acceptance
    );
}

#[test]
fn new_process_lifetime_stopped_resume_keeps_predecessor_work_dormant() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    let (mut owner, owner_process) = spawn_owner();
    activate_with_process(&controller, JournalPolicy::RequireExisting, &owner_process);
    let task = board.create_task("Predecessor stopped work", Some(Purpose::Ordinary), None);
    board.set_binding(
        &task,
        &binding(&task, Purpose::Ordinary, "predecessor-stopped"),
    );
    board.close(&task);
    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 1, "{poll:?}");
    let acceptance = acceptance_for_task(&poll, &task);

    controller.stop().unwrap();
    let stopped = board.create_task("Stopped interval work", Some(Purpose::Ordinary), None);
    let stopped_binding = binding(&stopped, Purpose::Ordinary, "stopped-new-lifetime");
    board.set_binding(&stopped, &stopped_binding);
    board.close(&stopped);
    let stopped_acceptance = stopped_binding.acceptance_id().unwrap();
    let before = controller.stored_state().unwrap().unwrap();
    assert_eq!(before.lifecycle, LifecycleStatus::Stopped);
    let head = board.event_head(before.checkpoint);
    assert!(
        head > before.checkpoint,
        "head {head}, checkpoint {}",
        before.checkpoint
    );
    stop_owner(&mut owner);

    // The same lead thread resumes under a new app-server lifetime while the
    // lifecycle is stopped. Host activation may resume the lifecycle, but it
    // consumes both the stopped interval and the predecessor session as
    // retained history instead of dispatching either one.
    let receipt = controller
        .activate_after_stop_with_owner_process(
            &ActivationRequest {
                lead_session: LEAD.to_owned(),
                run: Some("run-main".to_owned()),
                boundary: None,
                journal: JournalPolicy::RequireExisting,
                limits: None,
            },
            &live_self(),
        )
        .unwrap();
    let after = controller.stored_state().unwrap().unwrap();
    assert_eq!(after.lifecycle, LifecycleStatus::Active);
    assert_eq!(after.stopped_at, None);
    assert_eq!(after.binding.lead_session, LEAD);
    assert_eq!(after.binding.revision, before.binding.revision + 1);
    assert_eq!(after.checkpoint, head);
    assert_eq!(receipt.checkpoint, after.checkpoint);
    assert_eq!(after.records, before.records);
    let floor = after
        .current_session_floor
        .expect("a new process lifetime establishes the session boundary");
    assert!(floor >= head, "floor {floor}, head {head}");
    assert!(
        controller.pending_jobs().unwrap().is_empty(),
        "predecessor work must remain retained but not dispatchable"
    );
    assert!(controller.record(&stopped_acceptance).unwrap().is_none());

    let skipped = controller.poll().unwrap();
    assert_eq!(skipped.accepted, 0, "{skipped:?}");
    assert_eq!(
        skipped.events_seen, 0,
        "the activation checkpoint atomically consumed the stopped interval: {skipped:?}"
    );
    assert!(controller.record(&stopped_acceptance).unwrap().is_none());
    assert_eq!(
        controller
            .record(&acceptance)
            .unwrap()
            .unwrap()
            .executor
            .state,
        StageState::Pending
    );
    assert!(controller.stored_state().unwrap().unwrap().checkpoint >= floor);

    let current = board.create_task("New-lifetime stopped work", Some(Purpose::Ordinary), None);
    board.set_binding(
        &current,
        &binding(&current, Purpose::Ordinary, "new-lifetime-stopped"),
    );
    board.close(&current);
    let polled = controller.poll().unwrap();
    assert_eq!(polled.accepted, 1, "{polled:?}");
    assert_eq!(polled.jobs.len(), 1, "{polled:?}");
    let current_acceptance = acceptance_for_task(&polled, &current);
    assert_eq!(polled.jobs[0].acceptance_id, current_acceptance);
    assert_eq!(
        controller.pending_jobs().unwrap()[0].acceptance_id,
        current_acceptance
    );
}

#[test]
fn stopped_succession_refuses_live_or_unverifiable_owner_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    let (mut owner, owner_process) = spawn_owner();
    activate_with_process(&controller, JournalPolicy::RequireExisting, &owner_process);
    controller.stop().unwrap();
    let state_file = controller.state_file().unwrap();
    let stopped = fs::read(&state_file).unwrap();

    let live = controller
        .succeed_current_session(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(live.contains("still live"), "{live}");
    assert_eq!(fs::read(&state_file).unwrap(), stopped);

    let mut mismatched = owner_process.clone();
    mismatched.program = std::env::current_exe().unwrap();
    rewrite_state(&controller, |value| {
        value["owner_process"] = serde_json::to_value(&mismatched).unwrap();
    });
    let unverifiable = fs::read(&state_file).unwrap();
    let error = controller
        .succeed_current_session(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("cannot be verified"), "{error}");
    assert_eq!(fs::read(&state_file).unwrap(), unverifiable);

    rewrite_state(&controller, |value| {
        value.as_object_mut().unwrap().remove("owner_process");
    });
    let legacy = fs::read(&state_file).unwrap();
    let error = controller
        .succeed_current_session(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("no exact managed-lead process identity"),
        "{error}"
    );
    assert_eq!(fs::read(&state_file).unwrap(), legacy);
    stop_owner(&mut owner);
}

#[test]
fn succession_refuses_legacy_owner_without_process_proof() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    activate(&controller, JournalPolicy::RequireExisting);
    // Compatibility activation predates retained owner proof. Removing the
    // field leaves the truthful legacy shape rather than inventing an exit.
    rewrite_state(&controller, |value| {
        value.as_object_mut().unwrap().remove("owner_process");
    });
    let state_file = controller.state_file().unwrap();
    let before = fs::read(&state_file).unwrap();
    let error = controller
        .succeed(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("no exact managed-lead process identity"),
        "{error}"
    );
    assert!(error.contains("enable"), "{error}");
    assert_eq!(fs::read(&state_file).unwrap(), before);
}

#[test]
fn succession_refuses_incomplete_or_unverifiable_retained_process_proof() {
    let temp = tempfile::tempdir().unwrap();
    let board = Board::new(temp.path());
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    activate(&controller, JournalPolicy::RequireExisting);
    let state_file = controller.state_file().unwrap();

    rewrite_state(&controller, |value| {
        value["owner_process"] = json!({
            "identity": {"pid": 0, "creation_time": 0},
            "program": ""
        });
    });
    let incomplete = fs::read(&state_file).unwrap();
    let error = controller
        .succeed(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("identity is incomplete"), "{error}");
    assert!(error.contains("enable"), "{error}");
    assert_eq!(fs::read(&state_file).unwrap(), incomplete);

    // The documented recovery works: the current managed lead refreshes the
    // proof through the ordinary activation route instead of editing state.
    activate_with_process(&controller, JournalPolicy::RequireExisting, &live_self());
    assert_eq!(
        controller.stored_state().unwrap().unwrap().owner_process,
        Some(live_self())
    );

    let mut mismatched = live_self();
    mismatched.program = PathBuf::from("C:/Program Files/PowerShell/7/pwsh.exe");
    rewrite_state(&controller, |value| {
        value["owner_process"] = serde_json::to_value(&mismatched).unwrap();
    });
    let unverifiable = fs::read(&state_file).unwrap();
    let error = controller
        .succeed(&SuccessionRequest {
            expected_lead_session: LEAD.to_owned(),
            successor_lead: SUCCESSOR.to_owned(),
            successor_process: live_self(),
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("cannot be verified"), "{error}");
    assert!(error.contains("enable"), "{error}");
    assert_eq!(fs::read(&state_file).unwrap(), unverifiable);
    activate_with_process(&controller, JournalPolicy::RequireExisting, &live_self());
}
