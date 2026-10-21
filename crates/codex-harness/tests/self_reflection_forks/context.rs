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
const PREDECESSOR_SOURCE: &str = "fixture-predecessor-executor-source";
const MISMATCHED_SOURCE: &str = "fixture-other-recorded-source";

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
    accepted_context_fixture_with_sessions(
        name,
        &[EXECUTOR_SOURCE],
        EXECUTOR_SOURCE,
        EXECUTOR_SOURCE_TURN,
    )
}

fn accepted_context_fixture_with_sessions(
    name: &str,
    executor_sessions: &[&str],
    accepted_session: &str,
    accepted_turn: &str,
) -> AcceptedContextFixture {
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
            session: Some(accepted_session.to_owned()),
            turn: Some(accepted_turn.to_owned()),
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
        executor_sessions
            .iter()
            .map(|session| (*session).to_owned())
            .collect(),
        LEAD_THREAD,
        ACTOR,
        Purpose::Ordinary,
    )
    .with_turns(
        Some(accepted_session.to_owned()),
        Some(accepted_turn.to_owned()),
        Some(LEAD_SOURCE_TURN.to_owned()),
    );
    for comment in [
        "Original assignment: inspect the accepted source and preserve its exact revision.",
        "Lead correction: do not treat the reused checkout as reviewed work; report missing source coverage explicitly.",
    ] {
        accepted::native(
            &workspace,
            "bd.exe",
            &["comment", task.as_str(), "--json", comment],
        );
    }
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
    request["executorSource"]["thread"] = json!(accepted_session);
    request["executorSource"]["lastTurn"] = json!(accepted_turn);
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

fn write_source_rollout(workspace: &Workspace, thread: &str, compacted: bool) -> PathBuf {
    let mut lines = vec![source_session_meta(thread), visible_source_message()];
    if compacted {
        lines.push(json!({
            "type": "compacted",
            "payload": {"summary": "synthetic opaque replacement"}
        }));
    }
    write_source_rollout_lines(workspace, thread, &lines)
}

fn write_source_rollout_lines(
    workspace: &Workspace,
    file_thread: &str,
    lines: &[Value],
) -> PathBuf {
    let directory = workspace
        .home
        .join("sessions")
        .join("2026")
        .join("10")
        .join("18");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("rollout-2026-10-18T00-00-00-{file_thread}.jsonl"));
    let mut contents = String::new();
    for line in lines {
        contents.push_str(&line.to_string());
        contents.push('\n');
    }
    fs::write(&path, contents).unwrap();
    path
}

fn source_session_meta(thread: &str) -> Value {
    json!({
        "type": "session_meta",
        "payload": {"id": thread, "session_id": thread}
    })
}

fn visible_source_message() -> Value {
    json!({
        "type": "response_item",
        "payload": {
            "type": "message",
            "role": "assistant",
            "response_id": "fixture-visible-source-response",
            "content": [{"type": "output_text", "text": "synthetic visible source input"}]
        }
    })
}

fn write_forked_source_rollout(workspace: &Workspace, thread: &str) -> PathBuf {
    let lines = [
        json!({
            "type": "session_meta",
            "payload": {
                "id": thread,
                "session_id": thread,
                "forked_from_id": "fixture-parent-source"
            }
        }),
        visible_source_message(),
    ];
    write_source_rollout_lines(workspace, thread, &lines)
}

fn write_mismatched_source_rollout(workspace: &Workspace, requested: &str) -> PathBuf {
    let lines = [
        json!({
            "type": "session_meta",
            "payload": {
                "id": MISMATCHED_SOURCE,
                "session_id": MISMATCHED_SOURCE,
                "forked_from_id": "fixture-parent-source"
            }
        }),
        source_session_meta(requested),
        visible_source_message(),
    ];
    write_source_rollout_lines(workspace, requested, &lines)
}

fn source_gaps(evidence: &Value) -> Vec<String> {
    evidence["sourceContext"]["gaps"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|gap| gap.as_str().map(str::to_owned))
        .collect()
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

fn both_stage_prompts(workspace: &Workspace) -> Vec<String> {
    ["executor-self-reflection", "lead-self-reflection"]
        .into_iter()
        .map(|role| {
            let turns = fixture_requests(workspace, role, "turn/start");
            assert_eq!(turns.len(), 1, "{role}: {turns:?}");
            turns[0]["params"]["input"][0]["text"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

fn both_stage_prompt_evidence(workspace: &Workspace) -> Vec<(String, Value)> {
    both_stage_prompts(workspace)
        .into_iter()
        .map(|prompt| {
            let evidence = accepted_evidence(&prompt);
            (prompt, evidence)
        })
        .collect()
}

fn both_stage_evidence(workspace: &Workspace) -> Vec<Value> {
    both_stage_prompt_evidence(workspace)
        .into_iter()
        .map(|(_, evidence)| evidence)
        .collect()
}

#[test]
fn accepted_reused_checkout_keeps_exact_retained_evidence_in_both_stages() {
    let fixture = accepted_context_fixture("accepted-context-reused");
    let retained_result = fixture.retained.result.as_ref().unwrap().clone();
    let assignment = fixture.retained.assignment.as_ref().unwrap().clone();
    let original_result = fs::read(&retained_result.locator).unwrap();
    let reused_revision = reuse_checkout(&fixture);
    assert_ne!(fixture.accepted_revision, reused_revision);
    write_source_rollout(&fixture.workspace, EXECUTOR_SOURCE, false);
    write_source_rollout(&fixture.workspace, LEAD_THREAD, false);

    let completed = run_context_chain(&fixture);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    for (prompt, evidence) in both_stage_prompt_evidence(&fixture.workspace) {
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
            evidence["acceptedAssignmentSha256"], assignment.assignment_sha256,
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
        assert_eq!(
            evidence["assignmentContext"]["retained"]["assignment"]["issue"], fixture.task,
            "{evidence}"
        );
        assert_eq!(
            evidence["assignmentContext"]["retained"]["assignment"]["assignmentSha256"],
            assignment.assignment_sha256,
            "{evidence}"
        );
        assert_eq!(
            evidence["assignmentContext"]["retained"]["submitted"]["kind"], "structured",
            "{evidence}"
        );
        assert_eq!(
            evidence["assignmentContext"]["retained"]["submitted"]["assignmentSha256"],
            assignment.assignment_sha256,
            "{evidence}"
        );
        assert!(
            evidence["assignmentContext"]["boardTask"]["comments"]
                .as_array()
                .unwrap()
                .iter()
                .any(|comment| comment["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("Lead correction"))),
            "{evidence}"
        );
        assert_eq!(
            evidence["assignmentContext"]["boardTask"]["statusIsAcceptanceEvidence"], false,
            "{evidence}"
        );
        assert_eq!(
            evidence["guidance"]["executor"]["role"], "executor-self-reflection",
            "{evidence}"
        );
        assert_eq!(
            evidence["guidance"]["lead"]["role"], "lead-self-reflection",
            "{evidence}"
        );
        assert_eq!(
            evidence["guidance"]["executor"]["revision"],
            role_facts().executor_revision,
            "{evidence}"
        );
        assert_eq!(
            evidence["guidance"]["lead"]["revision"],
            role_facts().lead_revision,
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["status"], "available",
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["executor"]["thread"], EXECUTOR_SOURCE,
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["lead"]["thread"], LEAD_THREAD,
            "{evidence}"
        );
        assert!(
            prompt.contains(
                "sourceContext.gaps identify missing, mismatched, ambiguous or compacted coverage"
            ),
            "{prompt}"
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
fn accepted_source_missing_stays_explicit_in_both_stages() {
    let fixture = accepted_context_fixture("accepted-context-source-missing");
    reuse_checkout(&fixture);

    let completed = run_context_chain(&fixture);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    for (prompt, evidence) in both_stage_prompt_evidence(&fixture.workspace) {
        assert_eq!(evidence["status"], "verified", "{evidence}");
        assert_eq!(
            evidence["acceptedRevision"], fixture.accepted_revision,
            "{evidence}"
        );
        assert_eq!(evidence["sourceContext"]["status"], "missing", "{evidence}");
        assert_eq!(
            evidence["sourceContext"]["executor"]["status"], "missing",
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["lead"]["status"], "missing",
            "{evidence}"
        );
        let gaps = source_gaps(&evidence);
        assert!(
            gaps.iter().any(|gap| gap.contains(&format!(
                "no source rollout for executor session '{EXECUTOR_SOURCE}'"
            ))),
            "{evidence}"
        );
        assert!(
            gaps.iter().any(|gap| gap.contains(&format!(
                "no source rollout for lead session '{LEAD_THREAD}'"
            ))),
            "{evidence}"
        );
        assert!(
            prompt.contains(
                "sourceContext.gaps identify missing, mismatched, ambiguous or compacted coverage"
            ),
            "{prompt}"
        );
        assert!(
            prompt.contains("no source rollout for executor session"),
            "{prompt}"
        );
        assert_eq!(
            evidence["stageCheckoutIsAcceptedEvidence"], false,
            "{evidence}"
        );
    }
}

#[test]
fn accepted_source_compaction_stays_explicit_in_both_stages() {
    let fixture = accepted_context_fixture("accepted-context-source-compacted");
    write_source_rollout(&fixture.workspace, EXECUTOR_SOURCE, true);
    write_source_rollout(&fixture.workspace, LEAD_THREAD, false);

    let completed = run_context_chain(&fixture);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    for (prompt, evidence) in both_stage_prompt_evidence(&fixture.workspace) {
        assert_eq!(evidence["status"], "verified", "{evidence}");
        assert_eq!(
            evidence["sourceContext"]["status"], "compacted",
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["executor"]["status"], "compacted",
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["executor"]["compactionObserved"], true,
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["lead"]["status"], "available",
            "{evidence}"
        );
        assert!(
            source_gaps(&evidence)
                .iter()
                .any(|gap| gap.contains("compacted window(s)")),
            "{evidence}"
        );
        assert!(prompt.contains("compacted window(s)"), "{prompt}");
        assert!(
            prompt.contains("Opaque reasoning, pre-compaction content"),
            "{prompt}"
        );
        assert!(
            !prompt.contains("synthetic opaque replacement"),
            "opaque source content must not be decoded: {prompt}"
        );
    }
}

#[test]
fn accepted_source_fork_marker_is_ambiguous_not_compaction_in_both_stages() {
    let fixture = accepted_context_fixture("accepted-context-fork-marker");
    write_forked_source_rollout(&fixture.workspace, EXECUTOR_SOURCE);
    write_source_rollout(&fixture.workspace, LEAD_THREAD, false);

    let completed = run_context_chain(&fixture);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    for (prompt, evidence) in both_stage_prompt_evidence(&fixture.workspace) {
        assert_eq!(evidence["status"], "verified", "{evidence}");
        assert_eq!(
            evidence["sourceContext"]["status"], "available",
            "{evidence}"
        );
        let executor = &evidence["sourceContext"]["executor"];
        assert_eq!(executor["thread"], EXECUTOR_SOURCE, "{evidence}");
        assert_eq!(executor["recordedThread"], EXECUTOR_SOURCE, "{evidence}");
        assert_eq!(executor["identityMatches"], true, "{evidence}");
        assert_eq!(executor["status"], "available", "{evidence}");
        assert_eq!(executor["historyMarkerObserved"], true, "{evidence}");
        assert_eq!(executor["compactionObserved"], false, "{evidence}");
        assert_eq!(executor["compactedWindows"], 0, "{evidence}");
        assert!(
            executor["coverage"]["recognizedEvents"].as_u64().unwrap() > 0,
            "visible source content must be recognized: {evidence}"
        );
        let gaps = source_gaps(&evidence);
        assert!(
            gaps.iter().any(|gap| {
                gap.contains("fork marker is ambiguous")
                    && gap.contains("does not establish compaction")
                    && gap.contains("unavailable visible content")
            }),
            "{evidence}"
        );
        assert!(
            !gaps.iter().any(|gap| gap.contains("compacted window(s)")),
            "{evidence}"
        );
        assert!(prompt.contains("fork marker is ambiguous"), "{prompt}");
        assert!(
            !prompt.contains("records 1 compacted window(s)"),
            "{prompt}"
        );
        assert!(
            !prompt.contains("pre-compaction content is not decoded"),
            "{prompt}"
        );
    }
}

#[test]
fn accepted_source_recorded_identity_mismatch_stays_explicit_in_both_stages() {
    let fixture = accepted_context_fixture("accepted-context-source-mismatched");
    write_mismatched_source_rollout(&fixture.workspace, EXECUTOR_SOURCE);
    write_source_rollout(&fixture.workspace, LEAD_THREAD, false);

    let completed = run_context_chain(&fixture);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    for (prompt, evidence) in both_stage_prompt_evidence(&fixture.workspace) {
        assert_eq!(evidence["status"], "verified", "{evidence}");
        assert_eq!(
            evidence["sourceContext"]["status"], "mismatched",
            "{evidence}"
        );
        let executor = &evidence["sourceContext"]["executor"];
        assert_eq!(executor["thread"], EXECUTOR_SOURCE, "{evidence}");
        assert_eq!(executor["recordedThread"], MISMATCHED_SOURCE, "{evidence}");
        assert_eq!(executor["identityMatches"], false, "{evidence}");
        assert_eq!(executor["status"], "mismatched", "{evidence}");
        assert_eq!(executor["compactionObserved"], false, "{evidence}");
        let gaps = source_gaps(&evidence);
        assert!(
            gaps.iter().any(|gap| {
                gap.contains(&format!(
                    "records thread identity {MISMATCHED_SOURCE} instead of requested executor source '{EXECUTOR_SOURCE}'"
                )) && gap.contains("not attributed")
            }),
            "{evidence}"
        );
        assert!(prompt.contains("records thread identity"), "{prompt}");
        assert!(prompt.contains(MISMATCHED_SOURCE), "{prompt}");
        assert!(
            prompt.contains("its coverage is not attributed"),
            "{prompt}"
        );
    }
}

#[test]
fn accepted_source_successor_boundary_is_exact_in_both_stages() {
    let fixture = accepted_context_fixture_with_sessions(
        "accepted-context-successor",
        &[PREDECESSOR_SOURCE, EXECUTOR_SOURCE],
        EXECUTOR_SOURCE,
        EXECUTOR_SOURCE_TURN,
    );
    let retained_result = fixture.retained.result.as_ref().unwrap().clone();
    let assignment = fixture.retained.assignment.as_ref().unwrap().clone();
    write_source_rollout(&fixture.workspace, PREDECESSOR_SOURCE, false);
    write_source_rollout(&fixture.workspace, EXECUTOR_SOURCE, false);
    write_source_rollout(&fixture.workspace, LEAD_THREAD, false);
    let reused_revision = reuse_checkout(&fixture);
    assert_ne!(fixture.accepted_revision, reused_revision);

    let completed = run_context_chain(&fixture);
    assert_eq!(completed.outcome.exit_code, 0, "{}", completed.transcript);
    for (prompt, evidence) in both_stage_prompt_evidence(&fixture.workspace) {
        assert_eq!(evidence["status"], "verified", "{evidence}");
        assert_eq!(evidence["task"], fixture.task, "{evidence}");
        assert_eq!(evidence["run"], RUN, "{evidence}");
        assert_eq!(
            evidence["acceptedRevision"], fixture.accepted_revision,
            "{evidence}"
        );
        assert_eq!(
            evidence["acceptedAssignmentSha256"], assignment.assignment_sha256,
            "{evidence}"
        );
        assert_eq!(
            evidence["acceptedResultSha256"], retained_result.sha256,
            "{evidence}"
        );
        assert_eq!(
            evidence["acceptedSourceBoundary"]["acceptedExecutorSession"], EXECUTOR_SOURCE,
            "{evidence}"
        );
        assert_eq!(
            evidence["acceptedSourceBoundary"]["acceptedExecutorTurn"], EXECUTOR_SOURCE_TURN,
            "{evidence}"
        );
        assert_eq!(
            evidence["acceptedSourceBoundary"]["otherExecutorSessions"],
            json!([PREDECESSOR_SOURCE]),
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["status"], "available",
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["executor"]["thread"], EXECUTOR_SOURCE,
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["executor"]["turn"], EXECUTOR_SOURCE_TURN,
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["executor"]["otherSessions"],
            json!([PREDECESSOR_SOURCE]),
            "{evidence}"
        );
        assert_eq!(
            evidence["sourceContext"]["lead"]["thread"], LEAD_THREAD,
            "{evidence}"
        );
        assert!(
            prompt.contains(&format!(
                "only executor session '{EXECUTOR_SOURCE}' at completed turn '{EXECUTOR_SOURCE_TURN}'"
            )),
            "{prompt}"
        );
        assert!(prompt.contains(PREDECESSOR_SOURCE), "{prompt}");
        assert!(
            !prompt.contains("Different code from the accepted source revision."),
            "the reused checkout must not enter accepted source evidence: {prompt}"
        );
        assert_eq!(
            evidence["stageCheckoutIsAcceptedEvidence"], false,
            "{evidence}"
        );
    }
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
    let retained_result = fixture.retained.result.as_ref().unwrap().clone();
    let assignment = fixture.retained.assignment.as_ref().unwrap().clone();
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
        assert_eq!(evidence["task"], fixture.task, "{evidence}");
        assert_eq!(evidence["run"], RUN, "{evidence}");
        assert_eq!(
            evidence["acceptedAssignmentSha256"], assignment.assignment_sha256,
            "{evidence}"
        );
        assert_eq!(
            evidence["acceptedResultSha256"], retained_result.sha256,
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
        assert_eq!(
            evidence["assignmentContext"]["boardTask"]["status"], "closed",
            "{evidence}"
        );
        assert_eq!(
            evidence["assignmentContext"]["boardTask"]["statusIsAcceptanceEvidence"], false,
            "{evidence}"
        );
    }
}
