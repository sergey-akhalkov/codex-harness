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
            run.cursor.baseline_qualification = Some(state);
            run.store.save_cursor(&run.cursor)?;
        }
        return Ok(vec![format!(
            "qualification lead frame retained at {}; executor, review and independent acceptance are still pending",
            result.display()
        )]);
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

fn prepare(
    run: &mut Run,
    inputs: &BaselineQualificationInputs,
    state: &mut BaselineQualificationState,
) -> io::Result<ArmRuntime> {
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
    Ok(runtime)
}
