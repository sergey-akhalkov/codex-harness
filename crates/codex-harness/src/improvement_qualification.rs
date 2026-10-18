//! Ordinary control-workflow preparation and exact native lead framing.
//! Comparison policy and candidate selection are not prerequisites for observing
//! the control. All participant effects use the existing visible dispatch owner.

use super::*;
use harness_core::improvement_experiment::{ArmBinding, prepare_home, prepare_variant};
use harness_core::improvement_loop::{
    BaselineQualificationInputs, BaselineQualificationState, QualificationBindingIdentity,
    WorkflowParticipant,
};
use harness_core::improvement_runtime::{self, ArmRequest, ArmRuntime};
use harness_core::task_worktree::{self, FrozenCopy};
use serde_json::Value;
use std::fs;

pub(super) fn advance(run: &mut Run) -> io::Result<Vec<String>> {
    let inputs = run
        .spec
        .baseline_qualification
        .clone()
        .ok_or_else(|| invalid("missing qualification declaration"))?;
    inputs.validate()?;
    let digest = inputs.digest()?;
    let mut state = run
        .cursor
        .baseline_qualification
        .clone()
        .unwrap_or_else(|| {
            BaselineQualificationState::new(digest.clone(), inputs.workflow.effective_capacity())
        });
    if state.declaration_digest != digest || state.capacity != inputs.workflow.effective_capacity()
    {
        return Err(invalid(
            "the qualification declaration changed after preparation; preserve this run and use its frozen inputs",
        ));
    }
    let runtime = prepare(run, &inputs, &mut state)?;
    run.cursor.baseline_qualification = Some(state.clone());
    run.cursor.phase = Phase::BaselineQualification;
    run.store.save_cursor(&run.cursor)?;

    if let Some(id) = state.workflow.lead_frame.clone() {
        let attempt = run
            .cursor
            .attempt(&id)
            .cloned()
            .ok_or_else(|| invalid("the recorded lead-frame attempt is missing"))?;
        if attempt.state != AttemptState::Completed {
            let note = format!(
                "qualification lead frame {} is {:?}; its exact native outcome remains owned by the existing resume/wait path",
                id, attempt.state
            );
            return Ok(vec![note]);
        }
        if let Some(reason) = &attempt.reuse_refused {
            return Err(invalid(format!(
                "the lead frame cannot be consumed: {reason}"
            )));
        }
        let retained = attempt
            .retained
            .as_ref()
            .ok_or_else(|| invalid("the completed lead frame has no retained evidence"))?;
        let result = retained
            .result
            .as_ref()
            .ok_or_else(|| invalid("the completed lead frame has no retained result"))?;
        if retained.result_sha256.as_deref() != Some(build_identity::hash_file(result)?.as_str()) {
            return Err(invalid(
                "the retained lead frame changed; no executor assignment was dispatched",
            ));
        }
        crate::executor_assignment::Assignment::load(result)?;
        let binding = attempt
            .binding
            .as_ref()
            .ok_or_else(|| invalid("the lead frame has no exact native dispatch binding"))?;
        let session = binding
            .session
            .clone()
            .ok_or_else(|| invalid("the lead frame has no exact native session"))?;
        if state.workflow.lead.is_none() {
            state.workflow.lead = Some(WorkflowParticipant {
                slot: binding.slot,
                owner: binding.owner.clone(),
                session,
                profile: attempt.profile,
            });
            run.cursor.baseline_qualification = Some(state.clone());
            run.store.save_cursor(&run.cursor)?;
        }
        return advance_work(run, &inputs, &mut state, &runtime);
    }
    let home = runtime.home.clone();
    let facts = DispatchFacts {
        runner_declared: run.spec.runner.is_some(),
        launcher: Some(runtime.home.join("harness/bin/codex.exe")),
        ..DispatchFacts::default()
    };
    if let DispatchGate::Blocked { reason } =
        dispatch_gate(&run.cursor, AttemptRole::LeadFrame, &facts)
    {
        run.cursor.condition = Some(reason.clone());
        run.store.save_cursor(&run.cursor)?;
        return Ok(vec![reason]);
    }
    let source = state
        .checkout
        .clone()
        .ok_or_else(|| invalid("prepared qualification has no dispatch checkout"))?;
    let base = state
        .task_base
        .clone()
        .ok_or_else(|| invalid("prepared qualification has no frozen task base"))?;
    let id = "lead-frame-1".to_owned();
    let owner = dispatch_owner(&run.spec.run, AttemptRole::LeadFrame, 1);
    let prompt = format!(
        "Frame this ordinary task for an executor in the assigned frozen checkout. Read the relevant project guidance and actual inputs. Do not implement changes or dispatch anyone. Return one usable structured executor assignment as JSON with schema=1, objective, inputs, outputs, invariants, acceptance, consumer and escalate. Every file path must be relative to this checkout and verified here. Writable scope: {}. Preserve all task acceptance. Do not add experiment labels or comparison policy.\n\nTask instructions:\n{}",
        inputs.task.writable_scope.join(", "),
        fs::read_to_string(&inputs.task_prompt.path)?
    );
    let prompt_path = run.store.assignments_dir().join("lead-frame-1.txt");
    fs::create_dir_all(run.store.assignments_dir())?;
    fs::write(&prompt_path, &prompt)?;
    let selected = state
        .lead_binding
        .as_ref()
        .ok_or_else(|| invalid("prepared qualification has no lead binding"))?;
    run.cursor.push_attempt(Attempt {
        id: id.clone(),
        role: AttemptRole::LeadFrame,
        binding: None,
        retained: None,
        owner: owner.clone(),
        title: format!("CLead ({}) - {owner}", selected.profile),
        profile: selected.profile.clone(),
        model: selected.model.clone(),
        model_provider: selected.model_provider.clone(),
        reasoning_effort: selected.reasoning_effort.clone(),
        checkout: None,
        assignment: Some(prompt_path),
        receipt: None,
        result: None,
        detail: None,
        state: AttemptState::Requested,
        reason: None,
        reuse_refused: None,
        started_ms: now_ms(),
        updated_ms: now_ms(),
    })?;
    state.workflow.lead_frame = Some(id.clone());
    run.cursor.baseline_qualification = Some(state);
    run.cursor.effect(
        EffectKind::DispatchPrepared,
        format!("attempt={id} role=lead-frame owner={owner}"),
    );
    run.store.save_cursor(&run.cursor)?;
    match dispatch_visible_lead_conversation(&VisibleLeadConversation {
        codex_home: home,
        source,
        owner,
        base: Some(base),
        prompt: PromptSource::FreeText(prompt),
    }) {
        Ok(accepted) => {
            record_accepted(&mut run.cursor, &id, &accepted);
            run.store.save_cursor(&run.cursor)?;
            Ok(vec![format!(
                "qualification: visible lead framing accepted through {}",
                accepted.title
            )])
        }
        Err(error) => {
            if let Some(attempt) = run.cursor.attempts.iter_mut().find(|a| a.id == id) {
                attempt.state = AttemptState::Failed;
                attempt.reason = Some(error.to_string());
                attempt.updated_ms = now_ms();
            }
            run.cursor.condition = Some(format!(
                "lead framing dispatch failed: {error}; retained attempt is not replayed"
            ));
            run.store.save_cursor(&run.cursor)?;
            Err(error)
        }
    }
}

fn retained_result(attempt: &Attempt) -> io::Result<PathBuf> {
    if let Some(reason) = &attempt.reuse_refused {
        return Err(invalid(format!(
            "{} cannot be consumed: {reason}",
            attempt.id
        )));
    }
    let retained = attempt
        .retained
        .as_ref()
        .ok_or_else(|| invalid("completed turn has no retained evidence"))?;
    let path = retained
        .result
        .as_ref()
        .ok_or_else(|| invalid("completed turn has no retained result"))?;
    if retained.result_sha256.as_deref() != Some(build_identity::hash_file(path)?.as_str()) {
        return Err(invalid(format!(
            "the retained result of {} changed; no next turn was dispatched",
            attempt.id
        )));
    }
    Ok(path.clone())
}

fn participant(attempt: &Attempt) -> io::Result<WorkflowParticipant> {
    let binding = attempt
        .binding
        .as_ref()
        .ok_or_else(|| invalid("completed turn has no native binding"))?;
    Ok(WorkflowParticipant {
        slot: binding.slot,
        owner: binding.owner.clone(),
        session: binding
            .session
            .clone()
            .ok_or_else(|| invalid("completed turn has no exact native session"))?,
        profile: attempt.profile.clone(),
    })
}

fn persist(run: &mut Run, state: &BaselineQualificationState) -> io::Result<()> {
    run.cursor.baseline_qualification = Some(state.clone());
    run.store.save_cursor(&run.cursor)
}

fn pending(attempt: &Attempt) -> Vec<String> {
    vec![format!(
        "qualification {} is {:?}; resume observes its exact native outcome without replay",
        attempt.id, attempt.state
    )]
}

fn advance_work(
    run: &mut Run,
    inputs: &BaselineQualificationInputs,
    state: &mut BaselineQualificationState,
    runtime: &ArmRuntime,
) -> io::Result<Vec<String>> {
    if state.workflow.executor_attempt.is_none() {
        let frame = run
            .cursor
            .attempt(state.workflow.lead_frame.as_deref().unwrap())
            .ok_or_else(|| invalid("recorded frame is missing"))?;
        let assignment = retained_result(frame)?;
        return dispatch_turn(
            run,
            inputs,
            state,
            runtime,
            AttemptRole::Executor,
            "executor-1".to_owned(),
            assignment,
        );
    }
    let executor_id = state.workflow.executor_attempt.as_deref().unwrap();
    let executor = run
        .cursor
        .attempt(executor_id)
        .cloned()
        .ok_or_else(|| invalid("recorded executor attempt is missing"))?;
    if executor.state != AttemptState::Completed {
        return Ok(pending(&executor));
    }
    if state.workflow.executor.is_none() {
        retain_solution(run, inputs, state, &executor)?;
        state.workflow.executor = Some(participant(&executor)?);
        persist(run, state)?;
    }
    if state.workflow.completed {
        return finalize(run, inputs, state, runtime);
    }
    if let Some(review_id) = state.workflow.review.clone() {
        let review = run
            .cursor
            .attempt(&review_id)
            .cloned()
            .ok_or_else(|| invalid("recorded review is missing"))?;
        if review.state != AttemptState::Completed {
            return Ok(pending(&review));
        }
        if state.workflow.review_token.is_none() {
            let result = retained_result(&review)?;
            let decision = harness_core::improvement_loop::parse_review_decision(
                &fs::read_to_string(result)?,
            )?;
            state.workflow.record_review(&review_id, decision)?;
            persist(run, state)?;
        }
    }
    use harness_core::improvement_loop::QualificationStep;
    match state.workflow.next_step(&inputs.workflow)? {
        QualificationStep::Review => {
            let report = state
                .report
                .as_ref()
                .ok_or_else(|| invalid("executor report is not retained"))?;
            let checkout = latest_solution_attempt(run, state)?
                .checkout
                .ok_or_else(|| invalid("executor checkout is missing"))?;
            let prompt = format!(
                "Review the completed ordinary task from your original framing. The exact executor checkout is {}; its committed revision is {}. Read the actual changes, checks and the retained executor report at {}. Preserve the original task acceptance. Do not implement or delegate. Start your result with ACCEPT, CORRECT or REJECT. ACCEPT needs a supported acceptance decision, CORRECT needs concrete bounded corrections for this same executor, and REJECT explains the remaining blocker. Do not add experiment policy.\n\nOriginal task:\n{}",
                checkout.display(),
                state.source_revision.as_deref().unwrap_or("unrecorded"),
                report.display(),
                fs::read_to_string(&inputs.task_prompt.path)?
            );
            let id = format!("lead-review-{}", state.workflow.corrections.len() + 1);
            let path = run.store.assignments_dir().join(format!("{id}.txt"));
            fs::write(&path, prompt)?;
            dispatch_turn(
                run,
                inputs,
                state,
                runtime,
                AttemptRole::LeadReview,
                id,
                path,
            )
        }
        QualificationStep::Correct => {
            let id = format!("correction-{}", state.workflow.corrections.len() + 1);
            if let Some(correction) = run.cursor.attempt(&id).cloned() {
                if correction.state != AttemptState::Completed {
                    return Ok(pending(&correction));
                }
                retain_solution(run, inputs, state, &correction)?;
                state.workflow.record_correction(&id, &inputs.workflow)?;
                persist(run, state)?;
                return advance_work(run, inputs, state, runtime);
            }
            let review = run
                .cursor
                .attempt(state.workflow.review.as_deref().unwrap())
                .ok_or_else(|| invalid("correction has no retained lead review"))?;
            let result = retained_result(review)?;
            let prompt = format!(
                "Continue your ordinary task in the same checkout. Read the retained lead review at {} and address its concrete corrections; preserve all original acceptance. Run the applicable checks, commit the scoped product and report changes, actual checks and unresolved issues. Do not delegate.\n\nLead review:\n{}",
                result.display(),
                fs::read_to_string(&result)?
            );
            let path = run.store.assignments_dir().join(format!("{id}.txt"));
            fs::write(&path, prompt)?;
            dispatch_turn(
                run,
                inputs,
                state,
                runtime,
                AttemptRole::Correction,
                id,
                path,
            )
        }
        QualificationStep::Finalize
        | QualificationStep::Rejected
        | QualificationStep::Exhausted => {
            state
                .workflow
                .record_completion(state.workflow.lead_accepted == Some(true));
            if state.workflow.next_step(&inputs.workflow)? == QualificationStep::Rejected {
                state.condition = Some("the lead rejected the task or its declared correction allowance was exhausted; preserve all turns, no replay".to_owned());
            }
            persist(run, state)?;
            finalize(run, inputs, state, runtime)
        }
        other => Err(invalid(format!(
            "unexpected qualification step after framing/execution: {other:?}"
        ))),
    }
}

fn retain_solution(
    run: &mut Run,
    inputs: &BaselineQualificationInputs,
    state: &mut BaselineQualificationState,
    attempt: &Attempt,
) -> io::Result<()> {
    let result = retained_result(attempt)?;
    let solution = super::improvement_comparison::validate_committed_solution(
        attempt,
        state
            .checkout
            .as_deref()
            .ok_or_else(|| invalid("dispatch checkout is missing"))?,
        state
            .task_base
            .as_deref()
            .ok_or_else(|| invalid("frozen task base is missing"))?,
        &inputs.task.writable_scope,
        None,
    )
    .map_err(invalid)?;
    fs::copy(&result, &inputs.report)?;
    state.source_revision = Some(solution.revision);
    // The attempt owns the executor checkout; task_workspace remains the oracle's owned copy.
    state.report_sha256 = Some(build_identity::hash_file(&inputs.report)?);
    state.report_bytes = Some(fs::metadata(&inputs.report)?.len());
    state.report = Some(inputs.report.clone());
    state.report_source = Some(result);
    persist(run, state)
}

fn latest_solution_attempt(run: &Run, state: &BaselineQualificationState) -> io::Result<Attempt> {
    let id = state
        .workflow
        .corrections
        .last()
        .or(state.workflow.executor_attempt.as_ref())
        .ok_or_else(|| invalid("qualification has no executor result"))?;
    run.cursor
        .attempt(id)
        .cloned()
        .ok_or_else(|| invalid("the recorded solution attempt is missing"))
}

fn prepare_oracle_workspace(
    run: &mut Run,
    inputs: &BaselineQualificationInputs,
    state: &mut BaselineQualificationState,
    runtime: &ArmRuntime,
) -> io::Result<()> {
    use super::improvement_comparison as accounting;
    let request = accounting::read_acceptance_request(&inputs.acceptance).map_err(invalid)?;
    let workspace = PathBuf::from(
        request["case_root"]
            .as_str()
            .ok_or_else(|| invalid("independent acceptance names no case_root"))?,
    );
    if !workspace.is_absolute() {
        return Err(invalid(
            "the independent acceptance workspace must be absolute",
        ));
    }
    harness_core::inventory::ordinary_parents(&workspace)?;
    let key = |path: &Path| {
        accounting::overlap_path(path)
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_lowercase()
    };
    let target = key(&workspace);
    for protected in [
        &inputs.task.source,
        &run.spec.project,
        &runtime.home,
        state.checkout.as_ref().unwrap(),
        state.task_copy.as_ref().unwrap(),
    ] {
        let protected = key(protected);
        if target == protected
            || target.starts_with(&format!("{protected}/"))
            || protected.starts_with(&format!("{target}/"))
        {
            return Err(invalid(
                "the independent acceptance workspace overlaps protected task, runtime or dispatch state",
            ));
        }
    }
    if let Some(owned) = &state.task_workspace {
        if key(owned) != target || !workspace.is_dir() {
            return Err(invalid(
                "the retained acceptance workspace is missing or mismatched; preserve it instead of cleaning another checkout",
            ));
        }
        return Ok(());
    }
    if workspace.exists() {
        return Err(invalid(format!(
            "independent acceptance workspace {} already exists without this run's ownership; choose a fresh declared path",
            workspace.display()
        )));
    }
    if let Some(parent) = workspace.parent() {
        fs::create_dir_all(parent)?;
    }
    accounting::clone_workspace(state.task_copy.as_deref().unwrap(), &workspace)
        .map_err(invalid)?;
    state.task_workspace = Some(workspace);
    persist(run, state)
}

fn finalize(
    run: &mut Run,
    inputs: &BaselineQualificationInputs,
    state: &mut BaselineQualificationState,
    runtime: &ArmRuntime,
) -> io::Result<Vec<String>> {
    use super::improvement_comparison as accounting;
    use std::collections::BTreeMap;
    let attempt = latest_solution_attempt(run, state)?;
    let solution = accounting::validate_committed_solution(
        &attempt,
        state.checkout.as_deref().unwrap(),
        state.task_base.as_deref().unwrap(),
        &inputs.task.writable_scope,
        None,
    )
    .map_err(invalid)?;
    if state.source_revision.as_deref() != Some(solution.revision.as_str())
        || state.report_sha256.as_deref()
            != Some(build_identity::hash_file(&inputs.report)?.as_str())
    {
        return Err(invalid(
            "the reviewed revision or retained executor report changed; no independent acceptance was run",
        ));
    }
    if let Some(row_path) = &state.row {
        let row: Value = read_json(row_path, MAX_RUN_SPEC_BYTES)?;
        let oracle = state
            .oracle
            .as_deref()
            .ok_or_else(|| invalid("recorded accounting row has no independent check"))?;
        if row["checks"][0]["evidence_sha256"].as_str()
            != Some(build_identity::hash_file(oracle)?.as_str())
        {
            return Err(invalid(
                "the retained independent acceptance record changed; its result cannot be reused",
            ));
        }
        run.cursor.phase = Phase::BaselineQualificationRecorded;
        persist(run, state)?;
        return Ok(vec![format!(
            "ordinary baseline result retained at {}; no comparison policy or candidate has been qualified",
            row_path.display()
        )]);
    }

    // Each exact participant session contributes native usage once even when
    // it contains several resumed turns. Turn receipts remain separately named.
    let mut participants: BTreeMap<String, Vec<&Attempt>> = BTreeMap::new();
    for turn in &run.cursor.attempts {
        if !matches!(
            turn.role,
            AttemptRole::LeadFrame
                | AttemptRole::Executor
                | AttemptRole::LeadReview
                | AttemptRole::Correction
                | AttemptRole::LeadAcceptance
        ) {
            continue;
        }
        let identity = participant(turn)?;
        if turn.state != AttemptState::Completed {
            return Err(invalid(
                "a participant turn is unresolved; no complete workflow cost can be recorded",
            ));
        }
        retained_result(turn)?;
        participants.entry(identity.session).or_default().push(turn);
    }
    let mut native_runs = Vec::new();
    let mut all_paths = Vec::new();
    for (session, turns) in participants {
        let first = turns[0];
        if turns.iter().any(|turn| {
            turn.model != first.model
                || turn.model_provider != first.model_provider
                || turn.reasoning_effort != first.reasoning_effort
        }) {
            return Err(invalid(
                "one participant changed model/provider/effort between turns",
            ));
        }
        let model = first
            .model
            .as_deref()
            .ok_or_else(|| invalid("participant model is unknown"))?;
        let observed = accounting::discover_attempt_sessions(
            first,
            &runtime.home,
            model,
            first.reasoning_effort.as_deref(),
        )?;
        if !observed.verified {
            return Err(invalid(observed.reason.unwrap_or_else(|| {
                "participant observation is unverified".to_owned()
            })));
        }
        let usage = accounting::usage_totals(&observed.paths).ok_or_else(|| invalid(format!("native usage remains incomplete for {session}; preserve the completed work and reconcile its rollout, no model turn was repeated")))?;
        all_paths.extend(observed.paths.iter().cloned());
        let observations: Vec<Value> = turns
            .iter()
            .map(|turn| {
                accounting::retained_observation(turn)
                    .ok_or_else(|| invalid("a completed turn has no retained native observation"))
            })
            .collect::<io::Result<_>>()?;
        let sum_counter = |key: &str| -> Option<u64> {
            observations.iter().try_fold(0u64, |total, observation| {
                total.checked_add(observation[key].as_u64()?)
            })
        };
        let ended_ms = observations
            .iter()
            .filter_map(|o| o["updatedMs"].as_u64())
            .max()
            .ok_or_else(|| invalid("participant completion timing is missing"))?;
        let mut native = json!({
            "session": session, "profile": first.profile, "model": first.model,
            "model_provider": first.model_provider, "reasoning_effort": first.reasoning_effort,
            "started_at": first.started_ms as f64 / 1000.0,
            "ended_at": ended_ms as f64 / 1000.0, "status":"completed",
            "usage":usage, "requests":usage["response_count"],
            "attempts":turns.iter().map(|turn| turn.id.clone()).collect::<Vec<_>>(),
            "evidence":observed.paths,
        });
        if let Some(rounds) = sum_counter("rounds") {
            native["rounds"] = json!(rounds);
        }
        if let Some(operations) = sum_counter("toolCalls") {
            native["tool_operations"] = json!(operations);
        }
        if let Some(calls) = observed.tool_calls {
            native["tool_calls"] = json!(calls);
        }
        native_runs.push(native);
    }
    all_paths.sort();
    all_paths.dedup();
    let usage = accounting::usage_totals(&all_paths)
        .ok_or_else(|| invalid("aggregate native usage is incomplete or conflicting"))?;
    let oracle_path = run.store.root().join("baseline-qualification/oracle.json");
    let acceptance;
    if state.oracle.is_some() {
        if !oracle_path.is_file() {
            return Err(invalid(
                "the independent check was submitted but its result is unknown; preserve the owned oracle workspace and recover the original check evidence before continuing",
            ));
        }
        let record: Value = read_json(&oracle_path, MAX_RUN_SPEC_BYTES)?;
        let request = accounting::read_acceptance_request(&inputs.acceptance).map_err(invalid)?;
        if record["kind"] != "real-task"
            || record["executed"] != true
            || record["task_contract_sha256"] != request["task_contract_sha256"]
            || record["oracle_program_sha256"] != request["oracle"]["program_sha256"]
            || record["oracle_inputs"] != request["oracle"]["inputs"]
        {
            return Err(invalid(
                "retained independent acceptance does not bind the frozen checker and task contract",
            ));
        }
        acceptance = accounting::Acceptance {
            passed: record["passed"] == true && record["checker_executed"] == true,
            record_path: oracle_path.clone(),
            started_at: record["started_at"]
                .as_f64()
                .ok_or_else(|| invalid("oracle start time is missing"))?,
            ended_at: record["ended_at"]
                .as_f64()
                .ok_or_else(|| invalid("oracle end time is missing"))?,
            workspace_preparation: "source-only-fresh-workspace".to_owned(),
        };
    } else {
        let workspace = state.task_workspace.as_deref().unwrap();
        let head = accounting::git(workspace, &["rev-parse", "HEAD"]).map_err(invalid)?;
        let changes = accounting::git(
            workspace,
            &[
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--ignored=matching",
            ],
        )
        .map_err(invalid)?;
        if Some(head.trim()) != state.task_base.as_deref() || !changes.is_empty() {
            return Err(invalid(
                "the owned oracle workspace changed before acceptance; preserve its work before retrying, no check was submitted",
            ));
        }
        state.oracle = Some(oracle_path.clone());
        persist(run, state)?;
        acceptance = accounting::execute_oracle(
            &inputs.acceptance.request,
            &inputs.acceptance.request_sha256,
            state.task_workspace.as_deref().unwrap(),
            &solution.checkout,
            &solution.revision,
            &oracle_path,
        )
        .map_err(invalid)?;
    }
    let accepted = state.workflow.lead_accepted == Some(true) && acceptance.passed;
    let row_path = run.store.root().join("baseline-qualification/row.json");
    let row = json!({
        "attempt_id":format!("{}-ordinary-control", run.spec.run),
        "case_id":inputs.task.name, "unit":format!("{}/{}", run.spec.run, inputs.task.name),
        "experiment_id":run.cursor.experiment, "arm":"baseline",
        "started_at":state.prepared_ms.unwrap_or(run.cursor.attempts[0].started_ms) as f64 / 1000.0,
        "execution_started_at":run.cursor.attempts[0].started_ms as f64 / 1000.0,
        "ended_at":acceptance.ended_at,
        "discovery_verified":true, "observed_model_metadata_verified":true,
        "native_runs":native_runs, "usage":usage,
        "checks":[{
            "id":"independent-acceptance", "round":0, "required":true,
            "executed":true, "passed":acceptance.passed,
            "started_at":acceptance.started_at, "ended_at":acceptance.ended_at,
            "evidence":acceptance.record_path,
            "evidence_sha256":build_identity::hash_file(&oracle_path)?,
        }],
        "children":[], "interventions":[], "retry_of":null,
        "workflow":state.workflow,
        "matched":{
            "task_source_revision":inputs.task.revision, "task_base":state.task_base,
            "task_prompt_sha256":inputs.task_prompt.sha256,
            "acceptance_request_sha256":inputs.acceptance.request_sha256,
            "runtime_source_sha256":runtime.variant.source_sha256,
            "lead_binding":state.lead_binding, "executor_binding":state.executor_binding,
            "capacity":state.capacity, "workspace_preparation":acceptance.workspace_preparation,
        },
        "solution_revision":solution.revision, "solution_paths":solution.changed_paths,
        "report_sha256":state.report_sha256, "accepted":accepted,
        "qualification_scope":"ordinary control observation; no comparison margins, reflection qualification or benefit verdict",
    });
    write_json_atomic(&row_path, &row)?;
    state.accepted = Some(accepted);
    state.row = Some(row_path.clone());
    run.cursor.phase = Phase::BaselineQualificationRecorded;
    run.cursor.effect(
        EffectKind::BaselineQualificationRecorded,
        format!(
            "task={} accepted={accepted} row={}",
            inputs.task.name,
            row_path.display()
        ),
    );
    persist(run, state)?;
    Ok(vec![format!(
        "ordinary baseline independently checked (accepted={accepted}); native workflow usage retained at {}",
        row_path.display()
    )])
}

fn dispatch_turn(
    run: &mut Run,
    _inputs: &BaselineQualificationInputs,
    state: &mut BaselineQualificationState,
    runtime: &ArmRuntime,
    role: AttemptRole,
    id: String,
    input: PathBuf,
) -> io::Result<Vec<String>> {
    let facts = DispatchFacts {
        runner_declared: run.spec.runner.is_some(),
        launcher: Some(runtime.home.join("harness/bin/codex.exe")),
        ..DispatchFacts::default()
    };
    if let DispatchGate::Blocked { reason } = dispatch_gate(&run.cursor, role, &facts) {
        run.cursor.condition = Some(reason.clone());
        run.store.save_cursor(&run.cursor)?;
        return Ok(vec![reason]);
    }
    let selected = if role == AttemptRole::LeadReview {
        state.lead_binding.as_ref()
    } else {
        state.executor_binding.as_ref()
    }
    .ok_or_else(|| invalid("prepared participant binding is missing"))?
    .clone();
    let prior = match role {
        AttemptRole::LeadReview => state.workflow.lead.clone(),
        AttemptRole::Correction => state.workflow.executor.clone(),
        _ => None,
    };
    let owner = prior
        .as_ref()
        .map(|p| p.owner.clone())
        .unwrap_or_else(|| dispatch_owner(&run.spec.run, role, 1));
    run.cursor.push_attempt(Attempt {
        id: id.clone(),
        role,
        binding: None,
        retained: None,
        owner: owner.clone(),
        title: format!(
            "{} ({}) - {owner}",
            if role == AttemptRole::LeadReview {
                "CLead"
            } else {
                "CEx"
            },
            selected.profile
        ),
        profile: selected.profile.clone(),
        model: selected.model.clone(),
        model_provider: selected.model_provider.clone(),
        reasoning_effort: selected.reasoning_effort.clone(),
        checkout: None,
        assignment: Some(input.clone()),
        receipt: None,
        result: None,
        detail: None,
        state: AttemptState::Requested,
        reason: None,
        reuse_refused: None,
        started_ms: now_ms(),
        updated_ms: now_ms(),
    })?;
    match role {
        AttemptRole::Executor => state.workflow.executor_attempt = Some(id.clone()),
        AttemptRole::LeadReview => state.workflow.review = Some(id.clone()),
        _ => {}
    }
    run.cursor.effect(
        EffectKind::DispatchPrepared,
        format!("attempt={id} role={} owner={owner}", role.as_str()),
    );
    persist(run, state)?;
    let source = state
        .checkout
        .clone()
        .ok_or_else(|| invalid("dispatch checkout is missing"))?;
    let accepted: io::Result<VisibleAccepted> = match role {
        AttemptRole::Executor => dispatch_visible_conversation(&VisibleConversation {
            codex_home: runtime.home.clone(),
            source,
            owner,
            profile: selected.profile.clone(),
            base: state.task_base.clone(),
            assignment: input,
        }),
        AttemptRole::LeadReview => {
            let p = prior.ok_or_else(|| invalid("review has no exact lead participant"))?;
            continue_visible_lead_conversation(&VisibleLeadContinuation {
                codex_home: runtime.home.clone(),
                source,
                slot: p.slot,
                owner: p.owner,
                session: p.session,
                prompt: fs::read_to_string(input)?,
            })
            .map(|a| VisibleAccepted {
                owner: a.owner,
                title: format!("CLead ({}) - {}", selected.profile, owner),
                slot: a.slot,
                model: selected.model.clone(),
                model_provider: selected.model_provider.clone(),
                reasoning_effort: selected.reasoning_effort.clone(),
                checkout: a.checkout,
                receipt: a.receipt,
                result: a.result,
                detail: a.detail,
            })
        }
        AttemptRole::Correction => {
            let p = prior.ok_or_else(|| invalid("correction has no exact executor participant"))?;
            continue_visible_executor_conversation(&VisibleExecutorContinuation {
                codex_home: runtime.home.clone(),
                source,
                slot: p.slot,
                owner: p.owner,
                session: p.session,
                profile: p.profile,
                prompt: fs::read_to_string(input)?,
            })
            .map(|a| VisibleAccepted {
                owner: a.owner,
                title: format!("CEx ({}) - {}", selected.profile, owner),
                slot: a.slot,
                model: selected.model.clone(),
                model_provider: selected.model_provider.clone(),
                reasoning_effort: selected.reasoning_effort.clone(),
                checkout: a.checkout,
                receipt: a.receipt,
                result: a.result,
                detail: a.detail,
            })
        }
        _ => Err(invalid("unsupported qualification participant turn")),
    };
    match accepted {
        Ok(accepted) => {
            record_accepted(&mut run.cursor, &id, &accepted);
            run.cursor.condition = None;
            run.store.save_cursor(&run.cursor)?;
            Ok(vec![format!(
                "qualification {id} accepted through {}",
                accepted.title
            )])
        }
        Err(error) => {
            if let Some(attempt) = run.cursor.attempts.iter_mut().find(|a| a.id == id) {
                attempt.state = AttemptState::Failed;
                attempt.reason = Some(error.to_string());
                attempt.updated_ms = now_ms();
            }
            run.cursor.condition = Some(format!(
                "{id} dispatch failed: {error}; the retained attempt is not replayed"
            ));
            run.store.save_cursor(&run.cursor)?;
            Err(error)
        }
    }
}

fn prepare(
    run: &mut Run,
    inputs: &BaselineQualificationInputs,
    state: &mut BaselineQualificationState,
) -> io::Result<ArmRuntime> {
    harness_core::inventory::ordinary_parents(&inputs.report)?;
    if state.report.is_some() {
        if state.report.as_ref() != Some(&inputs.report)
            || state.report_sha256.as_deref()
                != Some(build_identity::hash_file(&inputs.report)?.as_str())
        {
            return Err(invalid(
                "the owned qualification report changed; preserve it before continuing",
            ));
        }
    } else if inputs.report.exists() {
        return Err(invalid(
            "qualification report already exists without this run's ownership; no participant was dispatched",
        ));
    }
    if let Some(parent) = inputs.report.parent() {
        fs::create_dir_all(parent)?;
    }
    let root = run.store.root().join("baseline-qualification");
    let runtime_path = root.join("runtime.json");
    let frozen_path = root.join("workload.json");
    let runtime: ArmRuntime;
    let workload: FrozenCopy;
    if runtime_path.is_file() {
        runtime = read_json(&runtime_path, MAX_RUN_SPEC_BYTES)?;
        workload = read_json(&frozen_path, MAX_RUN_SPEC_BYTES)?;
        if workload.source.canonicalize()? != inputs.task.source.canonicalize()?
            || workload.source_revision != inputs.task.revision
            || runtime.variant.build.canonicalize()? != inputs.runtime.build.canonicalize()?
        {
            return Err(invalid(
                "the retained qualification preparation does not bind the frozen task/runtime",
            ));
        }
    } else {
        if root.exists() {
            return Err(invalid(format!(
                "partial qualification preparation at {}; preserve it and recover the owned installation before preparing a fresh run",
                root.display()
            )));
        }
        let variant = prepare_variant(
            &inputs.runtime.state,
            Arm::Baseline,
            &inputs.runtime.label,
            &inputs.runtime.build,
        )?;
        fs::create_dir_all(&root)?;
        workload = task_worktree::frozen_copy(
            &inputs.task.source,
            &inputs.task.revision,
            &root.join("workload"),
        )?;
        task_worktree::verify_frozen_pristine(&workload)?;
        write_json_atomic(&frozen_path, &workload)?;
        let home = prepare_home(&root.join("home"))?;
        let user_home = prepare_home(&root.join("home-user"))?;
        let dependency_user_home = prepare_home(&root.join("home-dep"))?;
        if let Some(config) = &inputs.runtime.native_config {
            // This is explicit configuration of a fresh home before connection,
            // the same supported input as the isolated installer consumer uses.
            // The runtime owner retains its complete installed configuration hash.
            fs::copy(&config.path, home.join("config.toml"))?;
        }
        let mut client = inputs.runtime.client.clone();
        if let Some(client) = &mut client {
            client.executor_profile = Some(inputs.workflow.executor_profile.clone());
        }
        runtime = improvement_runtime::install_arm(&ArmRequest {
            variant,
            home,
            user_home,
            dependency_user_home,
            upstream: inputs.runtime.upstream.clone(),
            timeout: Duration::from_secs(300),
            client,
            private_inputs: inputs.runtime.private_inputs.clone(),
            protected: vec![
                inputs.task.source.clone(),
                workload.path.clone(),
                run.spec.project.clone(),
            ],
        })?;
        write_json_atomic(&runtime_path, &runtime)?;
    }
    task_worktree::verify_frozen_pristine(&workload)?;
    let checkout = root.join("checkout");
    let arm = ArmBinding {
        arm: Arm::Baseline,
        home: runtime.home.clone(),
        workload: workload.clone(),
        runtime: runtime.variant.clone(),
    };
    super::improvement_comparison::ensure_dispatch_checkout(&checkout, &arm).map_err(invalid)?;
    let declaration = format!(
        "schema = 1\nlead_profile = {}\nsuccessor_lead_profile = {}\nexecutor_profiles = [{}]\nmax_concurrent_executors = {}\nworktree_pool_size = {}\n",
        serde_json::to_string(&inputs.workflow.lead_profile)?,
        serde_json::to_string(&inputs.workflow.lead_profile)?,
        serde_json::to_string(&inputs.workflow.executor_profile)?,
        state.capacity,
        state.capacity,
    );
    let declaration_path = checkout.join("global/orchestration.toml");
    if state.prepared_ms.is_some() && fs::read_to_string(&declaration_path)? != declaration {
        return Err(invalid(
            "the qualification dispatch profile/capacity declaration changed; no launch is allowed",
        ));
    }
    if state.prepared_ms.is_none() {
        fs::write(&declaration_path, declaration)?;
    }
    let lead = QualificationBindingIdentity::of(&orchestration_config::binding(
        &runtime.home,
        &inputs.workflow.lead_profile,
    )?);
    let executor = QualificationBindingIdentity::of(&orchestration_config::binding(
        &runtime.home,
        &inputs.workflow.executor_profile,
    )?);
    for binding in [&lead, &executor] {
        if binding.model.as_deref().is_none_or(|v| v.trim().is_empty())
            || binding
                .reasoning_effort
                .as_deref()
                .is_none_or(|v| v.trim().is_empty())
        {
            return Err(invalid(
                "qualification requires explicit model and effort for both native participant profiles",
            ));
        }
    }
    if state.lead_binding.as_ref().is_some_and(|old| old != &lead)
        || state
            .executor_binding
            .as_ref()
            .is_some_and(|old| old != &executor)
    {
        return Err(invalid(
            "a prepared participant model/provider/effort changed; no fallback or fresh binding is allowed",
        ));
    }
    let mut trusted = vec![checkout.clone()];
    trusted.extend(
        run.cursor
            .attempts
            .iter()
            .filter_map(|a| a.checkout.clone()),
    );
    improvement_runtime::verify_consumption_with_trust(&runtime, &trusted)?;
    state.runtime = Some(runtime_path);
    state.label = Some(runtime.label.clone());
    state.build = Some(runtime.variant.build.clone());
    state.task_copy = Some(workload.path.clone());
    state.task_base = Some(workload.revision.clone());
    state.checkout = Some(checkout);
    state.home = Some(runtime.home.clone());
    state.lead_binding = Some(lead);
    state.executor_binding = Some(executor);
    if state.prepared_ms.is_none() {
        state.prepared_ms = Some(now_ms());
        run.cursor.effect(
            EffectKind::BaselineQualificationPrepared,
            format!("runtime={} workload={}", runtime.label, inputs.task.name),
        );
    }
    prepare_oracle_workspace(run, inputs, state, &runtime)?;
    Ok(runtime)
}
