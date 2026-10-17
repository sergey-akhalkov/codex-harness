//! Native accepted-task reflection boundary. Every board below is an owned
//! temporary `bd` project; the controller is exercised through its public API
//! and makes no model calls.

use harness_core::delivery_reflection::{
    ACCEPTANCE_METADATA, AcceptanceBinding, ActivationRequest, BindingStatus, DeliveryState,
    EventRefusal, Finding, JournalOwnership, JournalPolicy, PURPOSE_METADATA, PollReceipt, Purpose,
    RecoveryKind, ReflectionController, ReflectionLimits, RefusalCode, Stage, StageOutcome,
    TaskBindingStatus, UsageDelta, acceptance_metadata,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[path = "delivery_reflection/native_import.rs"]
mod native_import;
const LEAD: &str = "lead-fixture";
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

fn acceptance_for_task(poll: &PollReceipt, task: &str) -> String {
    poll.jobs
        .iter()
        .find(|job| job.task_id == task)
        .unwrap_or_else(|| panic!("no reflection job for task {task}: {poll:?}"))
        .acceptance_id
        .clone()
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
