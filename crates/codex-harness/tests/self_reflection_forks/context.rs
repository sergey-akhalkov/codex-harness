//! Accepted-source attribution across a reused pool checkout.

use super::*;
use harness_core::{
    board_cli,
    build_identity::hash_bytes,
    delivery_reflection::{
        AcceptanceBinding, ActivationRequest, JournalPolicy, Purpose, ReflectionController,
        acceptance_metadata, board_source_identity,
    },
    task_worktree::{
        self, AssignmentSubmission, CompletionFacts, RetainedCompletion, SlotAssignmentBinding,
    },
};

const ACTOR: &str = "reflection-fixture-lead";
const RUN: &str = "accepted-context-run";

struct AcceptedContextFixture {
    workspace: Workspace,
    controller: ReflectionController,
    request: Value,
    task: String,
    acceptance: String,
    retained: RetainedCompletion,
    accepted_revision: String,
    accepted_result_path: PathBuf,
}

fn accepted_context_fixture(name: &str) -> AcceptedContextFixture {
    let workspace = Workspace::new(name, LeadMode::Delivered);
    let (controller, task) = accepted::prepare(&workspace);
    controller
        .activate(&ActivationRequest {
            lead_session: LEAD_THREAD.to_owned(),
            run: None,
            boundary: None,
            journal: JournalPolicy::RequireExisting,
            limits: None,
        })
        .unwrap();
    board_cli::set_purpose_metadata(
        Path::new("bd.exe"),
        &workspace.checkout,
        &task,
        Purpose::Ordinary,
    )
    .unwrap();
    accepted::native(&workspace, "git", &["add", "."]);
    accepted::native(
        &workspace,
        "git",
        &["commit", "--allow-empty", "-qm", "accepted source"],
    );
    let accepted_revision = head(&workspace);
    let accepted_result_path = workspace.root().join(format!("{name}-accepted-result.txt"));
    fs::write(
        &accepted_result_path,
        "Accepted source result: the original checks passed.\n",
    )
    .unwrap();
    let assignment = hash_bytes(format!("synthetic assignment for {name}").as_bytes());
    let pool = task_worktree::pool_state_dir(&workspace.home, &workspace.checkout).unwrap();
    fs::create_dir_all(&pool).unwrap();
    let retained = task_worktree::retain_completion(
        &pool,
        &CompletionFacts {
            run_generation: RUN.to_owned(),
            slot: 1,
            owner: "fixture-ordinary-executor".to_owned(),
            checkout: workspace.checkout.clone(),
            task: Some("fixture-durable-task".to_owned()),
            handle: Some("fixture-task-handle".to_owned()),
            assignment: Some(SlotAssignmentBinding {
                source: board_source_identity(Path::new("bd.exe"), &workspace.checkout).unwrap(),
                issue: task.clone(),
                assignment_sha256: assignment.clone(),
                purpose: Some("ordinary".to_owned()),
                purpose_source: "board".to_owned(),
            }),
            submitted_assignment: Some(AssignmentSubmission::structured(assignment.clone())),
            role: Some("executor".to_owned()),
            state: "completed".to_owned(),
            exit_code: Some(0),
            session: Some(EXECUTOR_SOURCE.to_owned()),
            turn: Some(EXECUTOR_SOURCE_TURN.to_owned()),
            result: Some(accepted_result_path.clone()),
        },
    )
    .unwrap();
    assert!(retained.eligibility.eligible, "{retained:?}");
    let result = retained.result.as_ref().unwrap();
    let binding = AcceptanceBinding::new(
        task.clone(),
        RUN,
        assignment,
        result.sha256.clone(),
        retained.source.revision.clone().unwrap(),
        vec![EXECUTOR_SOURCE.to_owned()],
        LEAD_THREAD,
        ACTOR,
        Purpose::Ordinary,
    )
    .with_turns(
        Some(EXECUTOR_SOURCE.to_owned()),
        Some(EXECUTOR_SOURCE_TURN.to_owned()),
        Some(LEAD_SOURCE_TURN.to_owned()),
    );
    board_cli::close_accepted(
        Path::new("bd.exe"),
        &workspace.checkout,
        ACTOR,
        &task,
        &acceptance_metadata(&binding).unwrap(),
    )
    .unwrap();
    let polled = controller.poll().unwrap();
    assert_eq!(polled.accepted, 1, "{polled:?}");
    let acceptance = polled.jobs[0].acceptance_id.clone();
    let mut request = workspace.request(RUN);
    request["task"] = json!(task);
    request["acceptance"] = json!(acceptance);
    request["leadSource"]["thread"] = json!(LEAD_THREAD);
    workspace.lead_state.lock().unwrap().source_turn = Some(LEAD_SOURCE_TURN.to_owned());
    AcceptedContextFixture {
        workspace,
        controller,
        request,
        task,
        acceptance,
        retained,
        accepted_revision,
        accepted_result_path,
    }
}

fn head(workspace: &Workspace) -> String {
    String::from_utf8(accepted::native(workspace, "git", &["rev-parse", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_owned()
}

fn reuse_checkout(fixture: &AcceptedContextFixture) -> String {
    fs::write(
        fixture.workspace.checkout.join("README.md"),
        "Later reused checkout contents; not accepted evidence.\n",
    )
    .unwrap();
    fs::write(
        fixture.workspace.checkout.join("later-work.txt"),
        "Different code from the accepted source revision.\n",
    )
    .unwrap();
    accepted::native(&fixture.workspace, "git", &["add", "."]);
    accepted::native(
        &fixture.workspace,
        "git",
        &["commit", "-qm", "reuse the pool checkout"],
    );
    let reused = head(&fixture.workspace);
    fs::write(
        &fixture.accepted_result_path,
        "Replacement live slot result; not the retained accepted result.\n",
    )
    .unwrap();
    reused
}

fn run_context_chain(fixture: &AcceptedContextFixture) -> ConsoleOutcome {
    wait_session(spawn_chain_in_board(
        &fixture.workspace,
        &fixture.request,
        "complete",
        "complete",
        "NO FINDING",
        LEAD_RESULT,
        Some(&fixture.workspace.checkout),
    ))
}

fn accepted_evidence(prompt: &str) -> Value {
    let references = prompt
        .split_once("<<<ACCEPTED EVIDENCE\n")
        .unwrap_or_else(|| panic!("accepted evidence block is missing: {prompt}"))
        .1
        .split_once("\nACCEPTED EVIDENCE>>>")
        .unwrap_or_else(|| panic!("accepted evidence block is incomplete: {prompt}"))
        .0;
    serde_json::from_str(references).unwrap()
}

fn both_stage_evidence(workspace: &Workspace) -> Vec<Value> {
    ["executor-self-reflection", "lead-self-reflection"]
        .into_iter()
        .map(|role| {
            let turns = fixture_requests(workspace, role, "turn/start");
            assert_eq!(turns.len(), 1, "{role}: {turns:?}");
            let prompt = turns[0]["params"]["input"][0]["text"].as_str().unwrap();
            accepted_evidence(prompt)
        })
        .collect()
}

#[test]
fn accepted_reused_checkout_keeps_exact_retained_evidence_in_both_stages() {
    let fixture = accepted_context_fixture("accepted-context-reused");
    let retained_result = fixture.retained.result.as_ref().unwrap().clone();
    let original_result = fs::read(&retained_result.locator).unwrap();
    let reused_revision = reuse_checkout(&fixture);
    assert_ne!(fixture.accepted_revision, reused_revision);

    let completed = run_context_chain(&fixture);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    for evidence in both_stage_evidence(&fixture.workspace) {
        assert_eq!(evidence["status"], "verified", "{evidence}");
        assert_eq!(evidence["task"], fixture.task, "{evidence}");
        assert_eq!(evidence["run"], RUN, "{evidence}");
        assert_eq!(
            evidence["acceptedRevision"], fixture.accepted_revision,
            "{evidence}"
        );
        assert_eq!(
            evidence["retainedSourceRevision"], fixture.accepted_revision,
            "{evidence}"
        );
        assert_eq!(
            evidence["acceptedResultSha256"], retained_result.sha256,
            "{evidence}"
        );
        assert_eq!(
            evidence["retainedResult"]["sha256"], retained_result.sha256,
            "{evidence}"
        );
        assert_eq!(
            evidence["retainedResult"]["locator"],
            json!(retained_result.locator.to_string_lossy()),
            "{evidence}"
        );
        assert_eq!(
            evidence["stageCheckoutIsAcceptedEvidence"], false,
            "{evidence}"
        );
    }
    assert_eq!(fs::read(&retained_result.locator).unwrap(), original_result);
    let reloaded = task_worktree::load_retained_completion(
        &fixture.workspace.home,
        &fixture.workspace.checkout,
        RUN,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        reloaded.source.revision.as_deref(),
        Some(fixture.accepted_revision.as_str())
    );
    assert_eq!(reloaded.result.unwrap().sha256, retained_result.sha256);
    let retained = fixture
        .controller
        .record(&fixture.acceptance)
        .unwrap()
        .unwrap();
    assert_eq!(retained.binding.run, RUN);

    let calls = request_counts(&fixture.workspace);
    let replay = run_context_chain(&fixture);
    assert_eq!(replay.outcome.exit_code, 0, "{}", replay.transcript);
    assert_eq!(request_counts(&fixture.workspace), calls);
}

#[test]
fn accepted_evidence_exposes_missing_retention_in_both_stages() {
    let fixture = accepted_context_fixture("accepted-context-missing");
    reuse_checkout(&fixture);
    let pool = task_worktree::pool_state_dir(&fixture.workspace.home, &fixture.workspace.checkout)
        .unwrap();
    fs::remove_dir_all(pool.join("completions").join(RUN)).unwrap();

    let completed = run_context_chain(&fixture);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    for evidence in both_stage_evidence(&fixture.workspace) {
        assert_eq!(evidence["status"], "missing", "{evidence}");
        assert_eq!(
            evidence["acceptedRevision"], fixture.accepted_revision,
            "{evidence}"
        );
        assert!(evidence["retainedResult"].is_null(), "{evidence}");
        assert!(
            evidence["gaps"].as_array().unwrap().iter().any(|gap| gap
                .as_str()
                .is_some_and(|gap| gap.contains("no retained completion"))),
            "{evidence}"
        );
        assert_eq!(
            evidence["stageCheckoutIsAcceptedEvidence"], false,
            "{evidence}"
        );
    }
}

#[test]
fn accepted_evidence_exposes_a_changed_result_in_both_stages() {
    let fixture = accepted_context_fixture("accepted-context-altered");
    reuse_checkout(&fixture);
    let locator = fixture.retained.result.as_ref().unwrap().locator.clone();
    fs::write(&locator, "Altered retained evidence.\n").unwrap();

    let completed = run_context_chain(&fixture);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    for evidence in both_stage_evidence(&fixture.workspace) {
        assert_eq!(evidence["status"], "unverified", "{evidence}");
        assert_eq!(
            evidence["acceptedRevision"], fixture.accepted_revision,
            "{evidence}"
        );
        assert!(evidence["retainedResult"].is_null(), "{evidence}");
        assert!(
            evidence["gaps"].as_array().unwrap().iter().any(|gap| {
                gap.as_str()
                    .is_some_and(|gap| gap.contains("missing or changed since retention"))
            }),
            "{evidence}"
        );
        assert_eq!(
            evidence["stageCheckoutIsAcceptedEvidence"], false,
            "{evidence}"
        );
    }
}
