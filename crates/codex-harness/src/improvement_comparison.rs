//! One sequential baseline/candidate comparison of a ready candidate.
//!
//! This module connects the controller's retained `candidate-ready` state to
//! the accepted owners instead of duplicating them: `improvement_experiment`
//! freezes the workload copies and binds the two prepared runtimes,
//! `improvement_runtime` installs each arm into a fresh owned home from the
//! explicit client inputs, `improvement_spec` qualifies the workload's own
//! OpenSpec change, the visible executor dispatch owner opens the two titled
//! conversations, the unchanged supervisor's `outcome-oracle` real-task entry
//! point checks the committed solutions, `outcome_report` +
//! `improvement_policy` account the matched attempts under the predeclared
//! policy and `benefit_gate` publishes the decision to Beads.
//!
//! Boundaries kept explicit here:
//!
//! - preparation is model-free; a missing policy, workload planning receipt,
//!   qualification, runtime consumption proof or acceptance request blocks
//!   before any dispatch;
//! - exactly one measured conversation is active at a time, the baseline
//!   before the candidate; an unknown outcome is never replayed;
//! - the oracle request bytes, the policy declaration and the frozen workload
//!   snapshots are never edited; a changed input refuses instead of being
//!   silently re-frozen;
//! - the published decision is an evidence-bound verdict only. Integration,
//!   baseline activation and live publication stay with their own owners and
//!   the parent run.

use super::*;
use harness_core::benefit_gate;
use harness_core::build_identity;
use harness_core::improvement_experiment::{
    Arm, ArmBinding, ExperimentBindings, prepare_home, prepare_variant,
};
use harness_core::improvement_loop::{
    COMPARISON_STATE_SCHEMA, ComparisonArm, ComparisonInputs, ComparisonState, EffectKind,
    VARIANTS_SCHEMA, Variant, VariantSet, candidate_change_dir, changed_paths_within_scope,
};
use harness_core::improvement_policy::DeclaredComparison;
use harness_core::improvement_runtime::{self, ArmRequest, ArmRuntime};
use harness_core::improvement_spec::{OpenSpec, PlanningReceipt};
use harness_core::outcome_qualification::{
    ApiObservedQualification, ClientInput, Qualification, QualificationMode, collect_observations,
    retained_qualification_reasons,
};
use harness_core::outcome_report::{MATCH_FIELDS, summarize_attempts};
use harness_core::rollout_reader;
use harness_core::task_worktree;
use serde_json::{Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Bound of one assignment document; mirrors the dispatcher's own limit.
const MAX_ASSIGNMENT_BYTES: usize = 256 * 1024;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_GIT_OUTPUT: usize = 1024 * 1024;
const MAX_ORACLE_OUTPUT: usize = 4 * 1024 * 1024;
const MAX_ROLLOUT_ENTRIES: usize = 100_000;

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

fn sha16(value: &str) -> String {
    value.chars().take(16).collect()
}

fn block(run: &mut Run, notes: &mut Vec<String>, reason: String) -> io::Result<()> {
    // The reason is retained durably: `resume` unwinds a stale block to the
    // suspended phase, so the bounded effect history is what keeps the refusal
    // visible in the run's own recovery record.
    run.cursor.effect(EffectKind::DispatchRefused, &reason);
    run.cursor.block(reason.clone());
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!("comparison: {reason}"));
    Ok(())
}

/// Run one bounded git command with captured output; the controller never
/// inherits a prompt or an interactive terminal.
fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map_err(|error| format!("git {} could not start: {error}", args.join(" ")))?;
    if output.stdout.len() > MAX_GIT_OUTPUT || output.stderr.len() > MAX_GIT_OUTPUT {
        return Err(format!("git {} produced unbounded output", args.join(" ")));
    }
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim().to_owned();
        let detail = &detail[..detail.len().min(512)];
        return Err(format!("git {} failed: {detail}", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_ok(cwd: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Entry point: drive one declared comparison as far as recorded state allows.
// ---------------------------------------------------------------------------

/// Advance the declared comparison. Returns after at most one dispatch, so a
/// stopped or unknown attempt is never worked around; every other step is
/// idempotent and re-runs from the retained state.
pub(super) fn advance(run: &mut Run, notes: &mut Vec<String>) -> io::Result<()> {
    if run.spec.comparison.is_none() || run.cursor.phase == Phase::Stopped {
        return Ok(());
    }
    if matches!(
        run.cursor.phase,
        Phase::DecisionRecorded | Phase::ActivationConfirmed | Phase::Idle
    ) {
        return Ok(());
    }
    if !matches!(
        run.cursor.phase,
        Phase::CandidateReady
            | Phase::BaselineAttempt
            | Phase::CandidateAttempt
            | Phase::Acceptance
            | Phase::Blocked
    ) {
        return Ok(());
    }
    if !ensure_prepared(run, notes)? {
        return Ok(());
    }
    consume_settled_arm(run, ComparisonArm::Baseline, notes)?;
    if run.cursor.phase != Phase::Blocked {
        consume_settled_arm(run, ComparisonArm::Candidate, notes)?;
    }
    if run.cursor.phase != Phase::Blocked {
        dispatch_next(run, notes)?;
    }
    if run.cursor.phase != Phase::Blocked {
        publish_decision(run, notes)?;
    }
    run.store.save_cursor(&run.cursor)
}

/// The model-free comparison preparation, idempotent across resumes. Returns
/// `true` when the two arms are prepared; a refusal is recorded as the run's
/// blocking condition and returns `false` without any dispatch.
fn ensure_prepared(run: &mut Run, notes: &mut Vec<String>) -> io::Result<bool> {
    let Some(comparison) = run.spec.comparison.clone() else {
        return Ok(false);
    };
    let Some(candidate) = run.cursor.candidate.clone() else {
        return Ok(false);
    };
    if !candidate.is_ready() {
        return Ok(false);
    }
    let declared = match comparison.declared_policy() {
        Ok(declared) => declared,
        Err(error) => {
            block(
                run,
                notes,
                format!("the predeclared comparison policy is unusable: {error}"),
            )?;
            return Ok(false);
        }
    };
    let state = match run.cursor.comparison.clone() {
        Some(state) => {
            if state.schema != COMPARISON_STATE_SCHEMA {
                block(
                    run,
                    notes,
                    "the retained comparison state declares an unsupported schema".to_owned(),
                )?;
                return Ok(false);
            }
            if state.policy_digest != declared.digest {
                block(
                    run,
                    notes,
                    "the comparison policy changed after its declaration; the retained evidence no longer binds it, so a fresh run is required".to_owned(),
                )?;
                return Ok(false);
            }
            state
        }
        None => ComparisonState::new(declared.digest.clone()),
    };
    run.cursor.comparison = Some(state.clone());

    // The measured arms must not even begin preparation without a usable
    // qualification: an unqualified or drifting record blocks here, before
    // any Git copy, installation or model-facing step.
    if let Some(reason) = qualification_block(&run.spec)? {
        block(
            run,
            notes,
            format!("local qualification blocks dependent model dispatch: {reason}"),
        )?;
        return Ok(false);
    }
    // The frozen acceptance request is validated before any copy, install or
    // dispatch: changed bytes are refused instead of being re-frozen.
    if let Err(reason) = validate_acceptance(&comparison) {
        block(run, notes, reason)?;
        return Ok(false);
    }
    // Every declared API observation input must be one of the declared client
    // files the arms actually consume; an unconsumed template is refused
    // before any preparation.
    if let Err(reason) = verify_observation_inputs(&comparison) {
        block(run, notes, reason)?;
        return Ok(false);
    }
    // The workload's own complete OpenSpec change must qualify separately
    // from the candidate's before the frozen copy is accepted.
    if let Err(reason) = ensure_workload_planning(run, &comparison) {
        block(run, notes, reason)?;
        return Ok(false);
    }
    // Workload B is its own durable hypothesis owner; its card, its spec
    // reference and its frozen removal proposal identity are validated here,
    // before any copy, installation or measured attempt.
    if let Err(reason) = ensure_workload_owner(run, &comparison) {
        block(run, notes, reason)?;
        return Ok(false);
    }
    // The prepared immutable runtimes and the two independent frozen copies
    // are bound together; a partial directory without its bindings refuses
    // instead of guessing.
    let bindings = match ensure_bindings(run, &comparison, &candidate, &declared) {
        Ok(bindings) => bindings,
        Err(reason) => {
            block(run, notes, reason)?;
            return Ok(false);
        }
    };
    // Exact build provenance through the existing build/source owners: the
    // baseline runtime must be the frozen base source and the candidate
    // runtime the ready candidate checkout. A swapped, unrelated or
    // changed-source build is refused before any installation or dispatch.
    if let Err(reason) = verify_build_provenance(run, &bindings) {
        block(run, notes, reason)?;
        return Ok(false);
    }
    if let Err(reason) = ensure_arm_installs(run, &comparison, &bindings) {
        block(run, notes, reason)?;
        return Ok(false);
    }
    // The declared client inputs must be the ones the arms actually consume;
    // an unused template cannot stand in for an installed configuration.
    for arm in [ComparisonArm::Baseline, ComparisonArm::Candidate] {
        if let Err(reason) = verify_consumed_arm(run, &comparison, arm, "preparation") {
            block(run, notes, reason)?;
            return Ok(false);
        }
    }
    if let Err(reason) = ensure_task_workspace(run, &comparison, &bindings) {
        block(run, notes, reason)?;
        return Ok(false);
    }
    let mut state = run
        .cursor
        .comparison
        .clone()
        .unwrap_or_else(|| ComparisonState::new(declared.digest.clone()));
    if state.prepared_ms.is_none() {
        state.prepared_ms = Some(now_ms());
        run.cursor.effect(
            EffectKind::ComparisonPrepared,
            format!(
                "baseline={} candidate={} policy={} workload={}",
                comparison.runtimes.baseline_label,
                comparison.runtimes.candidate_label,
                sha16(&declared.digest),
                comparison.task.name
            ),
        );
        notes.push(format!(
            "comparison prepared: {} and {} runtimes installed in fresh homes over the frozen workload {}",
            comparison.runtimes.baseline_label,
            comparison.runtimes.candidate_label,
            comparison.task.name
        ));
    }
    if run.cursor.phase == Phase::CandidateReady {
        run.cursor.phase = Phase::BaselineAttempt;
        run.cursor.condition = None;
    }
    state.bindings = Some(run.store.comparison_bindings_path());
    run.cursor.comparison = Some(state);
    run.store.save_cursor(&run.cursor)?;
    Ok(true)
}

/// Qualify or revalidate the workload's own OpenSpec change inside the
/// declared task project. A changed or missing receipt blocks dependent work.
fn ensure_workload_planning(run: &Run, comparison: &ComparisonInputs) -> Result<(), String> {
    let path = run.store.comparison_planning_path();
    let openspec = OpenSpec::default();
    let receipt: PlanningReceipt = if path.is_file() {
        read_json(&path, MAX_RUN_SPEC_BYTES).map_err(|error| {
            format!("the retained workload planning receipt is unreadable: {error}")
        })?
    } else {
        let receipt = openspec
            .qualify(&comparison.specification, &comparison.contract)
            .map_err(|error| {
                format!("the workload's own OpenSpec change does not qualify: {error}")
            })?;
        write_json_atomic(&path, &receipt).map_err(|error| {
            format!("the workload planning receipt could not be retained: {error}")
        })?;
        receipt
    };
    openspec
        .revalidate(&receipt)
        .map_err(|error| format!("the workload planning inputs changed: {error}"))
}

/// Workload B's own durable owner: the card must exist, be a hypothesis, not
/// closed or deferred, and its recorded spec reference must resolve to the
/// declared workload change. Its removal proposal digest, when the workload
/// declares a removal, is frozen here so a changed reviewed proposal needs a
/// fresh decision.
fn ensure_workload_owner(run: &mut Run, comparison: &ComparisonInputs) -> Result<(), String> {
    let card = board_hypothesis::load_card(
        &run.spec.board.bd,
        &run.spec.board.project,
        &comparison.workload_card,
    )
    .map_err(|error| {
        format!(
            "the workload's own hypothesis card {} is unavailable: {error}",
            comparison.workload_card
        )
    })?;
    if !card.labels.iter().any(|label| label == "hypothesis") {
        return Err(format!(
            "board item {} is not a hypothesis card: the hypothesis label is missing",
            comparison.workload_card
        ));
    }
    if matches!(card.status.as_str(), "closed" | "deferred") {
        return Err(format!(
            "workload card {} is {}; a closed or deferred workload needs a recorded reconsideration basis",
            comparison.workload_card, card.status
        ));
    }
    let Some(admission) = board_hypothesis::parse_admission(&card.description) else {
        return Err(format!(
            "workload card {} carries no recognized admission record; admit it before a run measures it",
            comparison.workload_card
        ));
    };
    let declared = admission.spec.unwrap_or_default().replace('\\', "/");
    if !declared.ends_with(&comparison.specification.change) {
        return Err(format!(
            "workload card {} references spec '{declared}' instead of the declared workload change '{}'",
            comparison.workload_card, comparison.specification.change
        ));
    }
    let comments = board_feedback::list_comments(
        &run.spec.board.bd,
        &run.spec.board.project,
        &comparison.workload_card,
    )
    .map_err(|error| error.to_string())?;
    let mut state = run
        .cursor
        .comparison
        .clone()
        .unwrap_or_else(|| ComparisonState::new("unset".to_owned()));
    state.workload_card = Some(comparison.workload_card.clone());
    if comparison.workload_removal.is_some() && state.workload_removal_frozen.is_none() {
        state.workload_removal_frozen =
            harness_core::improvement_loop::frozen_candidate_removal_digest(
                &comparison.workload_card,
                &comments,
            );
    }
    run.cursor.comparison = Some(state);
    Ok(())
}

/// The current experimental removal authority for workload B, resolved
/// against the live card comments and the digest frozen at preparation. It
/// applies to both measured arms: running B at all is the removal effect when
/// B retires a capability.
fn workload_removal_gate(
    run: &Run,
    comparison: &ComparisonInputs,
) -> io::Result<Option<RemovalGate>> {
    let Some(removal) = &comparison.workload_removal else {
        return Ok(None);
    };
    let comments = board_feedback::list_comments(
        &run.spec.board.bd,
        &run.spec.board.project,
        &comparison.workload_card,
    )?;
    let frozen = run
        .cursor
        .comparison
        .as_ref()
        .and_then(|state| state.workload_removal_frozen.clone());
    let request = board_hypothesis::AuthorityRequest {
        proposal: removal.proposal.clone(),
        target: removal.target.clone(),
        action: board_hypothesis::RemovalAction::Experiment,
    };
    Ok(Some(harness_core::improvement_loop::removal_gate_at(
        &comparison.workload_card,
        &request,
        &comments,
        frozen.as_deref(),
    )))
}

/// The frozen baseline source checkout: the run project at the declared base
/// revision, in a detached worktree owned by the run. It is the only source
/// identity the baseline runtime may have been built from.
fn ensure_baseline_source(run: &Run) -> Result<PathBuf, String> {
    let path = run.store.root().join("baseline-source");
    if !path.exists() {
        let plain = path.to_string_lossy().into_owned();
        git(
            &run.spec.project,
            &[
                "worktree",
                "add",
                "--detach",
                &plain,
                &run.spec.base_revision,
            ],
        )
        .map_err(|error| {
            format!(
                "the frozen baseline source checkout could not be materialized at {}: {error}",
                path.display()
            )
        })?;
    }
    if !git_ok(&path, &["rev-parse", "--is-inside-work-tree"]) {
        return Err(format!(
            "the frozen baseline source checkout {} is not a Git checkout",
            path.display()
        ));
    }
    let head = git(&path, &["rev-parse", "HEAD"]).map_err(|error| error.to_string())?;
    if head.trim() != run.spec.base_revision {
        return Err(format!(
            "the frozen baseline source checkout {} is at {} instead of the declared base {}",
            path.display(),
            head.trim(),
            run.spec.base_revision
        ));
    }
    Ok(path)
}

/// Exact provenance of both prepared runtimes through the existing build
/// identity owner: the baseline build must record the frozen base source and
/// the candidate build the ready candidate checkout. This is re-verified
/// before every measured dispatch and on reuse, so a swapped, unrelated or
/// changed-source build can never enter a comparison or adoption record.
fn verify_build_provenance(run: &Run, bindings: &ExperimentBindings) -> Result<(), String> {
    let baseline_source = ensure_baseline_source(run)?;
    let candidate_source = bindings.candidate.path.clone();
    for (arm, source) in [
        (Arm::Baseline, &baseline_source),
        (Arm::Candidate, &candidate_source),
    ] {
        let binding = bindings.arm(arm).map_err(|error| error.to_string())?;
        let check = build_identity::check(&binding.runtime.build, Some(source));
        if check.status != build_identity::Health::Healthy {
            return Err(format!(
                "the prepared {} runtime does not match the frozen {} source ({}); the explicit build inputs are refused before any measured dispatch",
                binding.runtime.label,
                match arm {
                    Arm::Baseline => "baseline",
                    Arm::Candidate => "candidate",
                },
                check.action
            ));
        }
    }
    Ok(())
}

/// Bind the declared client inputs to the client settings an installed arm
/// actually consumes. The retained consumption receipt names the exact
/// runner, effort, catalogue and overlay files, and every declared
/// API-observed input must be one of those consumed files: a qualified but
/// unconsumed template cannot mask a differently configured arm.
fn verify_client_binding(
    comparison: &ComparisonInputs,
    runtime: &ArmRuntime,
    phase: &str,
) -> Result<(), String> {
    let declared = &comparison.runtimes.client;
    let Some(configuration) = &runtime.configuration else {
        return Err(format!(
            "the {} arm retains no consumed configuration record for the declared client inputs",
            runtime.label
        ));
    };
    let Some(client) = &configuration.client else {
        return Err(format!(
            "the {} arm consumed no client configuration although the comparison declares an explicit local route",
            runtime.label
        ));
    };
    if client.runner.endpoint != declared.runner.endpoint
        || client.runner.model != declared.runner.model
    {
        return Err(format!(
            "the {phase} {} arm consumption names {}/{} instead of the declared {}/{}",
            runtime.label,
            client.runner.endpoint,
            client.runner.model,
            declared.runner.endpoint,
            declared.runner.model
        ));
    }
    for name in harness_core::outcome_qualification::MATERIAL_FIELDS {
        if let Some(declared_value) = declared.runner.identity.declared(name)
            && client.runner.identity.declared(name) != Some(declared_value)
        {
            return Err(format!(
                "the {phase} {} arm consumption does not confirm the declared material fact {name}",
                runtime.label
            ));
        }
    }
    if client.reasoning_effort != declared.reasoning_effort {
        return Err(format!(
            "the {phase} {} arm consumed reasoning effort {:?} instead of the declared {:?}",
            runtime.label, client.reasoning_effort, declared.reasoning_effort
        ));
    }
    verify_consumed_file(
        phase,
        &runtime.label,
        "catalogue",
        declared.catalogue.as_deref(),
        client.catalogue.as_ref(),
    )?;
    verify_consumed_file(
        phase,
        &runtime.label,
        "overlay",
        declared.overlay.as_deref(),
        configuration.overlay.as_ref(),
    )?;
    verify_observation_inputs(comparison)?;
    Ok(())
}

/// Every declared API observation input must be exactly one of the declared
/// client files the arms consume (overlay or catalogue). A qualified but
/// unconsumed template cannot stand in for the installed configuration.
fn verify_observation_inputs(comparison: &ComparisonInputs) -> Result<(), String> {
    let client = &comparison.runtimes.client;
    for input in &comparison.observation_inputs {
        let consumed = same_file_path(&input.path, client.overlay.as_deref())
            || same_file_path(&input.path, client.catalogue.as_deref());
        if !consumed {
            return Err(format!(
                "the declared API observation input {} is not one of the client files the arms actually consume (overlay or catalogue); an unconsumed template cannot authorize measured arms",
                input.name
            ));
        }
    }
    Ok(())
}

fn verify_consumed_file(
    phase: &str,
    label: &str,
    name: &str,
    declared: Option<&Path>,
    recorded: Option<&harness_core::improvement_runtime::FileIdentity>,
) -> Result<(), String> {
    match (declared, recorded) {
        (None, None) => Ok(()),
        (Some(path), Some(identity)) => {
            if !same_file_path(path, Some(identity.path.as_path())) {
                return Err(format!(
                    "the {phase} {label} arm consumes {name} {} instead of the declared {}",
                    identity.path.display(),
                    path.display()
                ));
            }
            let digest = build_identity::hash_file(path)
                .map_err(|error| format!("the declared {name} is unreadable: {error}"))?;
            if digest != identity.sha256 {
                return Err(format!(
                    "the {phase} {label} arm consumed a different {name} digest than the declared file"
                ));
            }
            Ok(())
        }
        (Some(path), None) => Err(format!(
            "the declared {name} {} was not consumed by the {phase} {label} arm",
            path.display()
        )),
        (None, Some(identity)) => Err(format!(
            "the {phase} {label} arm consumed an undeclared {name} at {}",
            identity.path.display()
        )),
    }
}

/// Path equality after canonicalization (case-insensitive on Windows), for
/// files that exist.
fn same_file_path(left: &Path, right: Option<&Path>) -> bool {
    let Some(right) = right else {
        return false;
    };
    let key = |path: &Path| -> String {
        fs::canonicalize(path)
            .unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    !left.as_os_str().is_empty() && key(left) == key(right)
}

/// Re-verify one installed arm's runtime consumption and client binding; the
/// same check runs before and after every measured attempt.
fn verify_consumed_arm(
    run: &Run,
    comparison: &ComparisonInputs,
    arm: ComparisonArm,
    phase: &str,
) -> Result<(), String> {
    let runtime_path = run
        .cursor
        .comparison
        .as_ref()
        .and_then(|state| state.arm(arm).runtime.clone())
        .ok_or_else(|| format!("the {} arm runtime receipt is missing", arm.as_str()))?;
    let runtime: ArmRuntime =
        read_json(&runtime_path, MAX_RUN_SPEC_BYTES).map_err(|error| error.to_string())?;
    let trusted = dispatch_workspaces(run, arm);
    improvement_runtime::verify_consumption_with_trust(&runtime, &trusted).map_err(|error| {
        format!(
            "the {phase} {} arm home no longer consumes its prepared runtime: {error}",
            arm.as_str()
        )
    })?;
    verify_client_binding(comparison, &runtime, phase)
}

/// The exact pooled workspaces this arm's own dispatcher can allocate: the
/// deterministic sibling slots of the arm's dispatch checkout (the owned
/// `task_worktree::slot_path` rule), sized by the frozen task tree's own
/// executor declaration. A dispatch trusts its allocated slot before the
/// model request even when it then fails before submission, so exactly those
/// workspaces may carry a trusted-project entry in the arm configuration -
/// and nothing else. A source without a declaration is dispatched through the
/// owned default-only declaration of one slot; an unusable declaration leaves
/// the single slot and the ordinary checks still refuse a foreign entry.
pub(super) fn dispatch_workspaces(run: &Run, arm: ComparisonArm) -> Vec<PathBuf> {
    let source = run.store.comparison_arm_dir(arm).join("checkout");
    let pool_size = dispatch_pool_size(run, arm);
    let mut workspaces: Vec<PathBuf> = Vec::new();
    for index in 1..=pool_size {
        let Ok(workspace) = task_worktree::slot_path(&source, index) else {
            continue;
        };
        if !workspaces.contains(&workspace) {
            workspaces.push(workspace);
        }
    }
    workspaces
}

/// The pool size of the arm's dispatch checkout, read from the frozen task
/// tree's own executor declaration. A source without one is dispatched
/// through the owned default-only declaration of one slot; an unreadable
/// declaration keeps that same single slot, and the dispatch itself refuses
/// the unusable source before any model request.
fn dispatch_pool_size(run: &Run, arm: ComparisonArm) -> u32 {
    let Some(bindings) = load_bindings(run).ok().flatten() else {
        return 1;
    };
    let Ok(binding) = bindings.arm(arm_to_experiment(arm)) else {
        return 1;
    };
    if !binding
        .workload
        .path
        .join("global/orchestration.toml")
        .is_file()
    {
        return 1;
    }
    orchestration_config::load(&binding.workload.path)
        .map(|declaration| declaration.max_concurrent_executors)
        .unwrap_or(1)
}

/// The executor profile the frozen task tree itself configures. The native
/// dispatch may name only a profile the task source declares as an executor,
/// and the prepared arm home mirrors the qualified route under that exact
/// name. A source without the declaration keeps the dispatch checkout's own
/// default-only declaration; a declaration that names no usable executor
/// profile refuses the arm instead of falling back to another route.
fn dispatch_profile(bindings: &ExperimentBindings, arm: Arm) -> Result<String, String> {
    let binding = bindings.arm(arm).map_err(|error| error.to_string())?;
    if !binding
        .workload
        .path
        .join("global/orchestration.toml")
        .is_file()
    {
        return Ok("default".to_owned());
    }
    let declaration = orchestration_config::load(&binding.workload.path).map_err(|error| {
        format!(
            "the {} frozen task tree declares an unusable orchestration: {error}",
            arm.as_str()
        )
    })?;
    orchestration_config::executor_profile(&declaration, None).map_err(|error| {
        format!(
            "the {} frozen task tree declares no executor profile: {error}",
            arm.as_str()
        )
    })
}

/// Build or reuse the two frozen workload copies, the fresh homes, the
/// prepared runtime identities and the experiment bindings receipt.
fn ensure_bindings(
    run: &Run,
    comparison: &ComparisonInputs,
    candidate: &harness_core::improvement_loop::CandidateState,
    declared: &DeclaredComparison,
) -> Result<ExperimentBindings, String> {
    let bindings_path = run.store.comparison_bindings_path();
    // The binding carries the full policy digest: the integration/activation
    // owner compares it against the retained evaluation's digest.
    let policy_digest = declared.digest.clone();
    if bindings_path.is_file() {
        let bindings: ExperimentBindings = read_json(&bindings_path, MAX_RUN_SPEC_BYTES)
            .map_err(|error| format!("the retained comparison bindings are unreadable: {error}"))?;
        bindings.validate().map_err(|error| {
            format!("the retained comparison bindings no longer validate: {error}")
        })?;
        if bindings.policy_digest != policy_digest {
            return Err(
                "the retained comparison bindings were created under another policy declaration"
                    .to_owned(),
            );
        }
        let checkout = candidate
            .worktree
            .clone()
            .ok_or_else(|| "the ready candidate records no allocation".to_owned())?;
        if bindings.candidate != checkout {
            return Err(
                "the comparison bindings name a different candidate allocation than the run retains"
                    .to_owned(),
            );
        }
        return Ok(bindings);
    }
    let root = run.store.comparison_dir();
    // Only the workload planning receipt may already exist: it is written
    // before the bindings so a refused planning qualification leaves no
    // half-prepared comparison behind. Anything else is a partial
    // preparation that is never adopted silently.
    let partial = root.exists()
        && fs::read_dir(&root)
            .map(|entries| {
                entries.filter_map(Result::ok).any(|entry| {
                    entry.file_name() != harness_core::improvement_loop::COMPARISON_PLANNING_FILE
                })
            })
            .unwrap_or(true);
    if partial {
        return Err(format!(
            "a partial comparison preparation exists at {} without its bindings; remove that private directory to prepare again",
            root.display()
        ));
    }
    fs::create_dir_all(&root)
        .map_err(|error| format!("the comparison workspace could not be created: {error}"))?;
    let checkout = candidate
        .worktree
        .clone()
        .ok_or_else(|| "the ready candidate records no allocation".to_owned())?;
    task_worktree::verify_candidate_checkout(&checkout)
        .map_err(|error| format!("the candidate allocation no longer verifies: {error}"))?;
    let mut arms = Vec::new();
    for (arm, build, label) in [
        (
            Arm::Baseline,
            &comparison.runtimes.baseline_build,
            &comparison.runtimes.baseline_label,
        ),
        (
            Arm::Candidate,
            &comparison.runtimes.candidate_build,
            &comparison.runtimes.candidate_label,
        ),
    ] {
        let comparison_arm = match arm {
            Arm::Baseline => ComparisonArm::Baseline,
            Arm::Candidate => ComparisonArm::Candidate,
        };
        let dir = run.store.comparison_arm_dir(comparison_arm);
        let workload_path = dir.join("workload");
        let workload = task_worktree::frozen_copy(
            &comparison.task.source,
            &comparison.task.revision,
            &workload_path,
        )
        .map_err(|error| format!("the {} workload copy is unavailable: {error}", arm.as_str()))?;
        task_worktree::verify_frozen_pristine(&workload).map_err(|error| {
            format!(
                "the freshly materialized {} workload copy is not an independent snapshot: {error}",
                arm.as_str()
            )
        })?;
        let home = prepare_home(&dir.join("home"))
            .map_err(|error| format!("the {} arm home is unavailable: {error}", arm.as_str()))?;
        prepare_home(&dir.join("home-user")).map_err(|error| {
            format!("the {} arm user home is unavailable: {error}", arm.as_str())
        })?;
        prepare_home(&dir.join("home-dep")).map_err(|error| {
            format!(
                "the {} arm dependency home is unavailable: {error}",
                arm.as_str()
            )
        })?;
        let variant = prepare_variant(&comparison.runtimes.state, arm, label, build)
            .map_err(|error| format!("the prepared {label} runtime is refused: {error}"))?;
        arms.push(ArmBinding {
            arm,
            home,
            workload,
            runtime: variant,
        });
    }
    let bindings = ExperimentBindings {
        schema: harness_core::improvement_experiment::EXPERIMENT_SCHEMA,
        hypothesis: candidate.hypothesis.clone(),
        case_id: comparison.task.name.clone(),
        base_revision: checkout.base.clone(),
        candidate: checkout,
        oracle: run.spec.oracle.clone(),
        // The decision's acceptance binding is the declared independent
        // acceptance reference (token-shaped), which is also the reference the
        // integration/activation owner re-derives its expected record from.
        acceptance: run.spec.oracle.clone(),
        policy_digest,
        arms,
    };
    bindings
        .validate()
        .map_err(|error| format!("the prepared comparison is not valid: {error}"))?;
    write_json_atomic(&bindings_path, &bindings)
        .map_err(|error| format!("the comparison bindings could not be retained: {error}"))?;
    let baseline = bindings.arm(Arm::Baseline).map_err(|e| e.to_string())?;
    let candidate_arm = bindings.arm(Arm::Candidate).map_err(|e| e.to_string())?;
    let variants = VariantSet {
        schema: VARIANTS_SCHEMA,
        baseline: Variant {
            state: comparison.runtimes.state.clone(),
            build: baseline.runtime.build.clone(),
            identity: Some(format!("sha256:{}", sha16(&baseline.runtime.source_sha256))),
        },
        candidate: Variant {
            state: comparison.runtimes.state.clone(),
            build: candidate_arm.runtime.build.clone(),
            identity: Some(format!(
                "sha256:{}",
                sha16(&candidate_arm.runtime.source_sha256)
            )),
        },
    };
    write_json_atomic(&run.store.variants_path(), &variants)
        .map_err(|error| format!("the prepared variants receipt could not be retained: {error}"))?;
    Ok(bindings)
}

/// Install both arms into their fresh homes through the runtime owner and
/// retain the exact consumed identity. An existing receipt is re-verified; a
/// missing one is installed model-free.
fn ensure_arm_installs(
    run: &mut Run,
    comparison: &ComparisonInputs,
    bindings: &ExperimentBindings,
) -> Result<(), String> {
    for (arm, comparison_arm) in [
        (Arm::Baseline, ComparisonArm::Baseline),
        (Arm::Candidate, ComparisonArm::Candidate),
    ] {
        let binding = bindings
            .arm(arm)
            .map_err(|error| format!("the {} arm is not bound: {error}", arm.as_str()))?;
        let dir = run.store.comparison_arm_dir(comparison_arm);
        let runtime_path = dir.join("runtime.json");
        let runtime: ArmRuntime = if runtime_path.is_file() {
            read_json(&runtime_path, MAX_RUN_SPEC_BYTES).map_err(|error| {
                format!(
                    "the retained {} arm runtime receipt is unreadable: {error}",
                    arm.as_str()
                )
            })?
        } else {
            let mut protected = vec![
                binding.workload.source.clone(),
                bindings.candidate.path.clone(),
            ];
            for other in &bindings.arms {
                if other.arm != arm {
                    protected.push(other.home.clone());
                }
            }
            let mut client = comparison.runtimes.client.clone();
            client.executor_profile = Some(dispatch_profile(bindings, arm)?);
            let request = ArmRequest {
                variant: binding.runtime.clone(),
                home: binding.home.clone(),
                user_home: dir.join("home-user"),
                dependency_user_home: dir.join("home-dep"),
                upstream: comparison.runtimes.upstream.clone(),
                timeout: INSTALL_TIMEOUT,
                client: Some(client),
                private_inputs: Vec::new(),
                protected,
            };
            let runtime = improvement_runtime::install_arm(&request).map_err(|error| {
                format!("the {} arm installation is refused: {error}", arm.as_str())
            })?;
            write_json_atomic(&runtime_path, &runtime).map_err(|error| {
                format!(
                    "the {} arm runtime receipt could not be retained: {error}",
                    arm.as_str()
                )
            })?;
            // The method is recorded with the install that applied it. A later
            // resume must not rewrite it to the current constant: an arm
            // prepared under another method stays incomparable.
            if let Err(error) = write_preparation_method(&runtime_path) {
                let _ = fs::remove_file(&runtime_path);
                return Err(format!(
                    "the {} arm preparation method could not be retained: {error}",
                    arm.as_str()
                ));
            }
            runtime
        };
        let trusted = dispatch_workspaces(run, comparison_arm);
        improvement_runtime::verify_consumption_with_trust(&runtime, &trusted).map_err(
            |error| {
                format!(
                    "the {} arm home no longer consumes its prepared runtime: {error}",
                    arm.as_str()
                )
            },
        )?;
        let mut state = run
            .cursor
            .comparison
            .clone()
            .unwrap_or_else(|| ComparisonState::new("unset".to_owned()));
        let arm_state = state.arm_mut(comparison_arm);
        arm_state.runtime = Some(runtime_path);
        arm_state.label = Some(runtime.label.clone());
        arm_state.build = Some(runtime.variant.build.clone());
        run.cursor.comparison = Some(state);
    }
    Ok(())
}

/// The frozen acceptance workspace the oracle checks. It is created from the
/// baseline frozen copy (both copies share one tree identity) and reused on
/// resume only while it still carries that frozen revision.
fn ensure_task_workspace(
    run: &mut Run,
    comparison: &ComparisonInputs,
    bindings: &ExperimentBindings,
) -> Result<PathBuf, String> {
    let request = read_acceptance_request(&comparison.acceptance)?;
    let workspace = PathBuf::from(
        request["case_root"]
            .as_str()
            .ok_or_else(|| "the frozen acceptance request names no case_root".to_owned())?,
    );
    if !workspace.is_absolute() {
        return Err("the frozen acceptance case_root must be an absolute path".to_owned());
    }
    let project = &run.spec.project;
    let candidate = bindings.candidate.path.clone();
    if workspace.starts_with(project) || workspace.starts_with(&candidate) {
        return Err(
            "the frozen acceptance workspace overlaps the run project or the candidate checkout; acceptance must stay outside candidate writes"
                .to_owned(),
        );
    }
    let baseline = bindings
        .arm(Arm::Baseline)
        .map_err(|error| error.to_string())?
        .workload
        .clone();
    if workspace.exists() {
        if !workspace.is_dir() {
            return Err(format!(
                "the frozen acceptance workspace {} is not a directory",
                workspace.display()
            ));
        }
        if git_ok(&workspace, &["rev-parse", "--is-inside-work-tree"]) {
            if !git_ok(
                &workspace,
                &[
                    "cat-file",
                    "-e",
                    &format!("{}^{{commit}}", baseline.revision),
                ],
            ) {
                return Err(format!(
                    "the frozen acceptance workspace {} does not carry the frozen task revision",
                    workspace.display()
                ));
            }
        } else if fs::read_dir(&workspace)
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        {
            return Err(format!(
                "the frozen acceptance workspace {} holds foreign state; preparation refuses to overwrite it",
                workspace.display()
            ));
        } else {
            clone_workspace(&baseline.path, &workspace)?;
        }
    } else {
        if let Some(parent) = workspace.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("the frozen acceptance workspace parent is unavailable: {error}")
            })?;
        }
        clone_workspace(&baseline.path, &workspace)?;
    }
    let mut state = run
        .cursor
        .comparison
        .clone()
        .unwrap_or_else(|| ComparisonState::new("unset".to_owned()));
    state.task_workspace = Some(workspace.clone());
    run.cursor.comparison = Some(state);
    Ok(workspace)
}

fn clone_workspace(source: &Path, target: &Path) -> Result<(), String> {
    git(
        source,
        &[
            "clone",
            "-q",
            "--no-hardlinks",
            &source.to_string_lossy(),
            &target.to_string_lossy(),
        ],
    )
    .map(|_| ())
    .map_err(|error| format!("the frozen acceptance workspace could not be created: {error}"))
}

/// Read and structurally validate the frozen acceptance request without
/// rewriting it. The supervisor's digest is checked at every use.
fn read_acceptance_request(
    acceptance: &harness_core::improvement_loop::AcceptanceInputs,
) -> Result<Value, String> {
    let bytes = crate::outcome_run::bounded_read(&acceptance.request, 2 * 1024 * 1024)
        .map_err(|error| format!("the frozen acceptance request is unreadable: {error}"))?;
    let digest = build_identity::hash_bytes(&bytes);
    if digest != acceptance.request_sha256.to_ascii_lowercase() {
        return Err(
            "the frozen acceptance request changed since the run declared it; a fresh declaration is required"
                .to_owned(),
        );
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("the frozen acceptance request is not JSON: {error}"))?;
    if value["schema"] != 1 || value["kind"] != "real-task" {
        return Err(
            "the frozen acceptance request is not a schema 1 real-task oracle request".to_owned(),
        );
    }
    let contract = value["task_contract_sha256"].as_str().unwrap_or("");
    if contract.len() != 64 || !contract.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("the frozen acceptance request carries no task contract digest".to_owned());
    }
    Ok(value)
}

fn validate_acceptance(comparison: &ComparisonInputs) -> Result<(), String> {
    let request = read_acceptance_request(&comparison.acceptance)?;
    let program = request["oracle"]["program"].as_str().unwrap_or("");
    if program.is_empty() {
        return Err("the frozen acceptance request names no oracle program".to_owned());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Qualification: the relevant owner validation per declared mode.
// ---------------------------------------------------------------------------

/// The qualification refusal for measured arms, when the retained record is
/// missing or unqualified. Full-material records are consumed unchanged;
/// API-observed records are validated through their own owner, including the
/// recomputed policy/observation digests and required declared facts.
pub(super) fn qualification_block(spec: &RunSpec) -> io::Result<Option<String>> {
    if spec.local_runner.is_none() {
        return Ok(None);
    }
    let Some(path) = &spec.qualification else {
        return Ok(Some(
            "no local qualification record path is declared".to_owned(),
        ));
    };
    if !path.is_file() {
        return Ok(Some(format!(
            "the local qualification record is not available at {}",
            path.display()
        )));
    }
    let value: Value = match read_json(path, 1024 * 1024) {
        Ok(value) => value,
        Err(error) => {
            return Ok(Some(format!(
                "the local qualification record is unreadable: {error}"
            )));
        }
    };
    if value.get("mode").is_some() {
        return Ok(api_observed_reasons(value, spec));
    }
    let qualification: Qualification = match serde_json::from_value(value) {
        Ok(qualification) => qualification,
        Err(error) => {
            return Ok(Some(format!(
                "the local qualification record is unreadable: {error}"
            )));
        }
    };
    if qualification.qualified() {
        return Ok(None);
    }
    let mut parts = Vec::new();
    if !qualification.missing_identity.is_empty() {
        parts.push(format!(
            "missing material identity: {}",
            qualification.missing_identity.join(", ")
        ));
    }
    if !qualification.unfinished_attempts.is_empty() {
        parts.push(format!(
            "unfinished attempts: {}",
            qualification.unfinished_attempts.join(", ")
        ));
    }
    if !qualification.unverified_attempts.is_empty() {
        parts.push(format!(
            "unverified model metadata: {}",
            qualification.unverified_attempts.join(", ")
        ));
    }
    if !qualification.tool_exchange_missing.is_empty() {
        parts.push(format!(
            "missing tool exchange: {}",
            qualification.tool_exchange_missing.join(", ")
        ));
    }
    if !qualification.runner_mismatch.is_empty() {
        parts.push("the recorded runner identity drifted across attempts".to_owned());
    }
    if !qualification.missing_outputs.is_empty() {
        parts.push(format!(
            "missing required outputs: {}",
            qualification.missing_outputs.join(", ")
        ));
    }
    if !qualification.divergent_outputs.is_empty() {
        parts.push(format!(
            "divergent outputs: {}",
            qualification.divergent_outputs.join(", ")
        ));
    }
    if parts.is_empty() {
        parts.push("the qualification is blocked".to_owned());
    }
    Ok(Some(format!(
        "{} ({} of {} repeats observed)",
        parts.join("; "),
        qualification.observed_repeats,
        qualification.required_repeats
    )))
}

fn api_observed_reasons(value: Value, spec: &RunSpec) -> Option<String> {
    let qualification: ApiObservedQualification = match serde_json::from_value(value) {
        Ok(qualification) => qualification,
        Err(error) => {
            return Some(format!(
                "the retained API-observed qualification is unreadable: {error}"
            ));
        }
    };
    if qualification.mode != QualificationMode::ApiObserved {
        return Some("the retained qualification declares another identity mode".to_owned());
    }
    let reasons = retained_qualification_reasons(&qualification);
    if !reasons.is_empty() {
        return Some(format!(
            "the retained API-observed qualification is internally inconsistent: {}",
            reasons.join(", ")
        ));
    }
    if !qualification.qualified() {
        let mut parts = Vec::new();
        if !qualification.missing_observations.is_empty() {
            parts.push(format!(
                "missing observations: {}",
                qualification.missing_observations.join(", ")
            ));
        }
        if !qualification.unfinished_attempts.is_empty() {
            parts.push(format!(
                "unfinished attempts: {}",
                qualification.unfinished_attempts.join(", ")
            ));
        }
        if !qualification.unverified_attempts.is_empty() {
            parts.push(format!(
                "unverified model metadata: {}",
                qualification.unverified_attempts.join(", ")
            ));
        }
        if !qualification.tool_exchange_missing.is_empty() {
            parts.push(format!(
                "missing tool exchange: {}",
                qualification.tool_exchange_missing.join(", ")
            ));
        }
        if !qualification.runner_mismatch.is_empty() {
            parts.push("the recorded runner identity drifted across attempts".to_owned());
        }
        if !qualification.missing_outputs.is_empty() {
            parts.push(format!(
                "missing required outputs: {}",
                qualification.missing_outputs.join(", ")
            ));
        }
        if !qualification.divergent_outputs.is_empty() {
            parts.push(format!(
                "divergent outputs: {}",
                qualification.divergent_outputs.join(", ")
            ));
        }
        if parts.is_empty() {
            parts.push("the qualification is blocked".to_owned());
        }
        return Some(format!(
            "{} ({} of {} repeats observed)",
            parts.join("; "),
            qualification.observed_repeats,
            qualification.required_repeats
        ));
    }
    if let Some(local) = &spec.local_runner
        && (qualification.runner.endpoint != local.endpoint
            || qualification.runner.model != local.model)
    {
        return Some(
            "the retained qualification was evaluated for a different endpoint or model than the run declares"
                .to_owned(),
        );
    }
    None
}

/// The API-observed recheck inputs, when the run selected that policy.
fn api_observed(run: &Run) -> io::Result<Option<ApiObservedQualification>> {
    let Some(path) = &run.spec.qualification else {
        return Ok(None);
    };
    let value: Value = match read_json(path, 1024 * 1024) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if value.get("mode").is_none() {
        return Ok(None);
    }
    let qualification: ApiObservedQualification =
        serde_json::from_value(value).map_err(|error| {
            invalid(format!(
                "the retained API-observed qualification is unreadable: {error}"
            ))
        })?;
    Ok(Some(qualification))
}

/// Re-collect the declared API observations and refuse drift before and after
/// each measured attempt. Model-free; a required observation that cannot be
/// fetched blocks instead of being dropped.
fn verify_observations(
    comparison: &ComparisonInputs,
    qualification: &ApiObservedQualification,
    phase: &str,
) -> Result<(), String> {
    let runner = qualification.runner.clone();
    let inputs: Vec<ClientInput> = comparison.observation_inputs.clone();
    let observed = collect_observations(&runner, &qualification.policy.plan, &inputs)
        .map_err(|failure| format!("the {phase} API observation set is unavailable: {failure}"))?;
    if observed.digest().map_err(|error| error.to_string())? != qualification.observation_digest {
        return Err(format!(
            "the {phase} API observation set differs from the qualified identity; the measured comparison is refused until requalified"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Dispatch of one measured arm.
// ---------------------------------------------------------------------------

fn facts_for_arm(
    run: &Run,
    runtime: &ArmRuntime,
    role: AttemptRole,
    profile: &str,
) -> io::Result<DispatchFacts> {
    let launcher = runtime.home.join("harness/bin/codex.exe");
    let binding_error = match orchestration_config::binding(&runtime.home, profile) {
        Ok(binding) => {
            let expected_model = run
                .spec
                .comparison
                .as_ref()
                .map(|comparison| comparison.runtimes.client.runner.model.clone());
            match (binding.model.clone(), expected_model) {
                (Some(installed), Some(expected)) if installed != expected => Some(format!(
                    "the installed arm binds model {installed} but the declared local runner serves {expected}"
                )),
                (None, Some(_)) => Some(
                    "the installed arm home binds no model for its executor profile".to_owned(),
                ),
                _ => match binding.model_provider.as_deref() {
                    Some("local") | None => None,
                    Some(other) => Some(format!(
                        "the installed arm binds provider {other} instead of the explicit local route"
                    )),
                },
            }
        }
        Err(error) => Some(format!(
            "the installed arm home has no usable executor binding: {error}"
        )),
    };
    let removal = super::dispatch_facts_for(run, role)?.removal;
    Ok(DispatchFacts {
        runner_declared: true,
        launcher: launcher.is_file().then_some(launcher),
        binding_error,
        qualification_block: qualification_block(&run.spec)?,
        removal,
        surface_loss: super::surface_loss(&run.cursor),
    })
}

fn arm_to_experiment(arm: ComparisonArm) -> Arm {
    match arm {
        ComparisonArm::Baseline => Arm::Baseline,
        ComparisonArm::Candidate => Arm::Candidate,
    }
}

fn load_bindings(run: &Run) -> io::Result<Option<ExperimentBindings>> {
    let path = run.store.comparison_bindings_path();
    if !path.is_file() {
        return Ok(None);
    }
    let bindings: ExperimentBindings = read_json(&path, MAX_RUN_SPEC_BYTES)?;
    Ok(Some(bindings))
}

fn latest_completed(run: &Run, role: AttemptRole) -> Option<&Attempt> {
    run.cursor
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.role == role && attempt.state == AttemptState::Completed)
}

fn latest_terminal(run: &Run, role: AttemptRole) -> Option<&Attempt> {
    run.cursor
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.role == role && attempt.state.is_terminal())
}

fn dispatch_next(run: &mut Run, notes: &mut Vec<String>) -> io::Result<()> {
    let Some(comparison) = run.spec.comparison.clone() else {
        return Ok(());
    };
    let Some(bindings) = load_bindings(run)? else {
        return Ok(());
    };
    for arm in [ComparisonArm::Baseline, ComparisonArm::Candidate] {
        let role = arm.role();
        let arm_state = run
            .cursor
            .comparison
            .as_ref()
            .map(|state| state.arm(arm).clone())
            .unwrap_or_default();
        if arm_state.condition.is_some() {
            continue;
        }
        if run
            .cursor
            .attempts
            .iter()
            .any(|attempt| attempt.role == role && attempt.state.is_in_flight())
        {
            return Ok(());
        }
        if !run.cursor.unresolved_attempts().is_empty() {
            return Ok(());
        }
        if let Some(completed) = latest_completed(run, role) {
            if arm_state.attempt.as_deref() == Some(completed.id.as_str()) {
                continue;
            }
            return Ok(());
        }
        // A refused dispatch (no model request) may be re-attempted on an
        // explicit resume; any other terminal attempt needs a fresh decision.
        if let Some(previous) = latest_terminal(run, role)
            && !(previous.state == AttemptState::Failed
                && previous
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("no model request was made")))
        {
            return block(
                run,
                notes,
                format!(
                    "the {} attempt {} is terminal as {} without a completed result; the comparison stops here and is never replayed automatically",
                    arm.as_str(),
                    previous.id,
                    previous.state.as_str()
                ),
            );
        }
        if arm == ComparisonArm::Candidate {
            let baseline = run
                .cursor
                .comparison
                .as_ref()
                .map(|state| state.baseline.clone())
                .unwrap_or_default();
            if baseline.accepted.is_none() {
                return Ok(());
            }
        }
        let runtime_path = arm_state
            .runtime
            .clone()
            .ok_or_else(|| invalid("the prepared arm runtime receipt is missing"))?;
        let runtime: ArmRuntime = read_json(&runtime_path, MAX_RUN_SPEC_BYTES)?;
        // Every dependent gate is re-evaluated before this arm's model work:
        // the experiment owner's pre-attempt check keeps the completed arm's
        // solution while requiring only the upcoming arm's workload to be the
        // pristine frozen snapshot.
        bindings
            .verify_pre_attempt(arm_to_experiment(arm))
            .map_err(|error| {
                invalid(format!(
                    "the {} arm is no longer a pristine pre-attempt snapshot: {error}",
                    arm.as_str()
                ))
            })?;
        // Exact build provenance and actual consumed client settings are
        // re-verified before the dispatch.
        if let Err(reason) = verify_build_provenance(run, &bindings) {
            return block(run, notes, reason);
        }
        if let Err(reason) = verify_consumed_arm(run, &comparison, arm, "pre-attempt") {
            return block(run, notes, reason);
        }
        // Workload B's own informed removal decision gates both measured arms.
        match workload_removal_gate(run, &comparison) {
            Ok(None) | Ok(Some(RemovalGate::Authorized { .. })) => {}
            Ok(Some(gate @ RemovalGate::Pending { .. })) => {
                return block(
                    run,
                    notes,
                    format!(
                        "workload {} removal approval is pending: {}",
                        comparison.workload_card,
                        super::removal_text(&Some(gate))
                    ),
                );
            }
            Ok(Some(gate @ (RemovalGate::Refused { .. } | RemovalGate::Withdrawn { .. }))) => {
                return block(
                    run,
                    notes,
                    format!(
                        "workload {} removal authority is {}; the dependent measured arms stay blocked and the request is not repeated without a new evidential basis",
                        comparison.workload_card,
                        super::removal_text(&Some(gate))
                    ),
                );
            }
            Err(error) => {
                return block(
                    run,
                    notes,
                    format!(
                        "workload {} removal authority could not be resolved: {error}",
                        comparison.workload_card
                    ),
                );
            }
        }
        if let Some(qualification) = api_observed(run)?
            && let Err(reason) = verify_observations(&comparison, &qualification, "pre-attempt")
        {
            return block(run, notes, reason);
        }
        let profile = match dispatch_profile(&bindings, arm_to_experiment(arm)) {
            Ok(profile) => profile,
            Err(reason) => return block(run, notes, reason),
        };
        let facts = facts_for_arm(run, &runtime, role, &profile)?;
        if let DispatchGate::Blocked { reason } = dispatch_gate(&run.cursor, role, &facts) {
            return block(
                run,
                notes,
                format!("the {} arm is not dispatchable: {reason}", arm.as_str()),
            );
        }
        return dispatch_arm(run, &comparison, &bindings, arm, &runtime, &profile, notes);
    }
    Ok(())
}

fn dispatch_arm(
    run: &mut Run,
    comparison: &ComparisonInputs,
    bindings: &ExperimentBindings,
    arm: ComparisonArm,
    runtime: &ArmRuntime,
    profile: &str,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let role = arm.role();
    let binding = bindings
        .arm(arm_to_experiment(arm))
        .map_err(|error| invalid(error.to_string()))?;
    let source = run.store.comparison_arm_dir(arm).join("checkout");
    if let Err(reason) = ensure_dispatch_checkout(&source, binding) {
        return block(run, notes, reason);
    }
    let base = match git(&source, &["rev-parse", "HEAD"]) {
        Ok(head) => head.trim().to_owned(),
        Err(error) => {
            return block(
                run,
                notes,
                format!(
                    "the {} dispatch checkout is unusable: {error}",
                    arm.as_str()
                ),
            );
        }
    };
    let attempt_id = next_arm_attempt_id(&run.cursor, role);
    let owner = dispatch_owner(&run.spec.run, role, attempt_ordinal(&attempt_id));
    let title = executor_title(profile, &owner);
    let assignment = match arm_assignment(comparison, arm, bindings) {
        Ok(assignment) => assignment,
        Err(error) => return block(run, notes, error.to_string()),
    };
    let assignment_path = run
        .store
        .assignments_dir()
        .join(format!("{attempt_id}.json"));
    let bytes = serde_json::to_vec_pretty(&assignment)?;
    if bytes.len() > MAX_ASSIGNMENT_BYTES {
        return block(
            run,
            notes,
            format!(
                "the {attempt_id} assignment exceeds the bounded assignment size; the dispatch is refused before any model request"
            ),
        );
    }
    let attempt = Attempt {
        id: attempt_id.clone(),
        role,
        binding: None,
        retained: None,
        owner: owner.clone(),
        title: title.clone(),
        profile: profile.to_owned(),
        model: Some(comparison.runtimes.client.runner.model.clone()),
        model_provider: Some("local".to_owned()),
        reasoning_effort: comparison.runtimes.client.reasoning_effort.clone(),
        checkout: None,
        assignment: Some(assignment_path.clone()),
        receipt: None,
        result: None,
        detail: None,
        state: AttemptState::Requested,
        reason: None,
        reuse_refused: None,
        started_ms: now_ms(),
        updated_ms: now_ms(),
    };
    run.cursor.push_attempt(attempt)?;
    run.cursor.effect(
        EffectKind::DispatchPrepared,
        format!(
            "attempt={attempt_id} role={} owner={owner} title=\"{title}\" assignment={}",
            role.as_str(),
            assignment_path.display()
        ),
    );
    write_json_atomic(&assignment_path, &assignment)?;
    let mut state = run
        .cursor
        .comparison
        .clone()
        .unwrap_or_else(|| ComparisonState::new("unset".to_owned()));
    state.arm_mut(arm).attempt = Some(attempt_id.clone());
    run.cursor.comparison = Some(state);
    run.store.save_cursor(&run.cursor)?;
    match dispatch_visible_conversation(&VisibleConversation {
        codex_home: runtime.home.clone(),
        source: source.clone(),
        owner,
        profile: profile.to_owned(),
        base: Some(base),
        assignment: assignment_path,
    }) {
        Ok(accepted) => {
            record_accepted(&mut run.cursor, &attempt_id, &accepted);
            run.store.save_cursor(&run.cursor)?;
            notes.push(format!(
                "comparison: the {} conversation was accepted through the visible owner with title \"{}\"",
                arm.as_str(),
                accepted.title
            ));
            Ok(())
        }
        Err(error) => {
            let reason = format!(
                "the {} dispatch was refused before submission: {error}; no fallback was attempted and no model request was made",
                arm.as_str()
            );
            if let Some(attempt) = run
                .cursor
                .attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
            {
                attempt.state = AttemptState::Failed;
                attempt.reason = Some(reason.clone());
                attempt.updated_ms = now_ms();
            }
            block(run, notes, reason)
        }
    }
}

/// Deterministic identity of the next measured conversation of one role.
fn next_arm_attempt_id(cursor: &Cursor, role: AttemptRole) -> String {
    let ordinal = cursor
        .attempts
        .iter()
        .filter(|attempt| attempt.role == role)
        .count()
        + 1;
    format!("{}-{ordinal}", role.short())
}

fn attempt_ordinal(attempt_id: &str) -> u32 {
    attempt_id
        .rsplit('-')
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

/// A dispatchable clone of the frozen workload: an independent minimal
/// repository (same tree identity) with a local origin and the orchestration
/// declaration the visible dispatch owner reads from its source checkout. The
/// declaration is host dispatch state, not task content, and is never
/// committed.
fn ensure_dispatch_checkout(source: &Path, binding: &ArmBinding) -> Result<(), String> {
    if !source.exists() {
        git(
            &binding.workload.source,
            &[
                "clone",
                "-q",
                "--no-hardlinks",
                &binding.workload.path.to_string_lossy(),
                &source.to_string_lossy(),
            ],
        )
        .map_err(|error| {
            format!(
                "the {} dispatch checkout could not be created: {error}",
                binding.arm.as_str()
            )
        })?;
    }
    if !git_ok(source, &["rev-parse", "--is-inside-work-tree"]) {
        return Err(format!(
            "the {} dispatch checkout {} is not a Git checkout",
            binding.arm.as_str(),
            source.display()
        ));
    }
    if !git_ok(
        source,
        &[
            "cat-file",
            "-e",
            &format!("{}^{{commit}}", binding.workload.revision),
        ],
    ) {
        return Err(format!(
            "the {} dispatch checkout lost the frozen task revision",
            binding.arm.as_str()
        ));
    }
    let orchestration = source.join("global/orchestration.toml");
    if !orchestration.is_file() {
        fs::create_dir_all(source.join("global")).map_err(|error| {
            format!("the dispatch declaration directory is unavailable: {error}")
        })?;
        fs::write(
            &orchestration,
            "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"default\"\nexecutor_profiles = [\"default\"]\nmax_concurrent_executors = 1\nvote_threshold = 2\nincubator_size_cap = 32\nfeedback_batch_limit = 8\n",
        )
        .map_err(|error| format!("the dispatch declaration could not be written: {error}"))?;
    }
    Ok(())
}

/// The bounded assignment one measured conversation receives: solve the
/// workload's own change in the bound checkout, commit everything, and expect
/// the frozen independent acceptance entry point afterwards.
fn arm_assignment(
    comparison: &ComparisonInputs,
    arm: ComparisonArm,
    bindings: &ExperimentBindings,
) -> io::Result<Value> {
    let binding = bindings
        .arm(arm_to_experiment(arm))
        .map_err(|error| invalid(error.to_string()))?;
    let change_dir = candidate_change_dir(&comparison.specification.change);
    let change_path = binding.workload.path.join(&change_dir);
    let mut inputs = Vec::new();
    if change_path.is_dir() {
        collect_relative_files(&change_path, &change_path, &change_dir, &mut inputs, 24)?;
    }
    let scope = comparison.task.writable_scope.join(", ");
    let label = match arm {
        ComparisonArm::Baseline => comparison.runtimes.baseline_label.clone(),
        ComparisonArm::Candidate => comparison.runtimes.candidate_label.clone(),
    };
    let objective = format!(
        "Complete the work items of the frozen OpenSpec change {} in this checkout (work items under {}). The frozen task snapshot and its independent acceptance are fixed; those change artifacts are read-only. Work only inside the declared writable scope: {scope}. Commit every change on this checkout's current HEAD and leave no uncommitted or untracked files. The controller independently re-verifies the committed revision, materializes it into the frozen acceptance workspace and runs the unchanged acceptance entry point; a success sentence alone is not evidence. Report the commit revision, the changed paths and the exact commands you ran with their observed results.",
        comparison.specification.change, change_dir,
    );
    Ok(json!({
        "schema": 1,
        "objective": format!("[{label}] {objective}"),
        "inputs": inputs,
        "outputs": [],
        "invariants": [
            format!("every edit stays inside the declared writable scope: {scope}"),
            format!("the frozen change artifacts under {change_dir} are read-only for this conversation"),
            "all work is committed on this checkout and the tree is left clean of uncommitted or untracked files; the controller materializes only the committed revision",
            "the independent acceptance entry point, its program and its inputs are outside this checkout and cannot be changed from here",
            "no new Git remote, no sibling solution and no reuse of another arm's result",
        ],
        "acceptance": [
            comparison.contract.independent_acceptance.clone(),
            "the controller's frozen real-task oracle entry point accepts the committed revision".to_owned(),
        ],
        "consumer": "the improvement controller (codex-harness improve)",
        "escalate": [],
    }))
}

fn collect_relative_files(
    root: &Path,
    directory: &Path,
    prefix: &str,
    out: &mut Vec<String>,
    limit: usize,
) -> io::Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(directory)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        if out.len() >= limit {
            return Ok(());
        }
        if path.is_dir() {
            collect_relative_files(root, &path, prefix, out, limit)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            let relative = relative.to_string_lossy().replace('\\', "/");
            if !relative.is_empty() && relative.len() <= 240 {
                out.push(format!("{prefix}/{relative}"));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Consuming one settled measured arm.
// ---------------------------------------------------------------------------

fn consume_settled_arm(
    run: &mut Run,
    arm: ComparisonArm,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let role = arm.role();
    let arm_state = run
        .cursor
        .comparison
        .as_ref()
        .map(|state| state.arm(arm).clone())
        .unwrap_or_default();
    if arm_state.condition.is_some() {
        return Ok(());
    }
    let Some(attempt) = latest_completed(run, role).cloned() else {
        return Ok(());
    };
    if arm_state.attempt.as_deref() == Some(attempt.id.as_str()) && arm_state.row.is_some() {
        return Ok(());
    }
    let Some(comparison) = run.spec.comparison.clone() else {
        return Ok(());
    };
    let Some(bindings) = load_bindings(run)? else {
        return Ok(());
    };
    let runtime_path = arm_state
        .runtime
        .clone()
        .ok_or_else(|| invalid("the prepared arm runtime receipt is missing"))?;
    let runtime: ArmRuntime = read_json(&runtime_path, MAX_RUN_SPEC_BYTES)?;

    let refusal = (|| -> Result<(), String> {
        if attempt.binding.is_none() {
            return Err(format!(
                "the completed {} attempt {} records no verifiable dispatch identity, so it cannot enter the comparison",
                arm.as_str(),
                attempt.id
            ));
        }
        verify_consumed_arm(run, &comparison, arm, "post-attempt")?;
        // The observed conversation must carry the declared model and effort
        // through the real rollout owner; a wrong or missing observation is
        // refused instead of entering the comparison unverified.
        let sessions = discovery_sessions(run, &attempt).map_err(|error| error.to_string())?;
        if !sessions.verified {
            return Err(sessions.reason.unwrap_or_else(|| {
                "the observed conversation did not verify the declared model and effort".to_owned()
            }));
        }
        if let Some(qualification) = api_observed(run).map_err(|error| error.to_string())? {
            verify_observations(&comparison, &qualification, "post-attempt")?;
        }
        Ok(())
    })();
    if let Err(reason) = refusal {
        return refuse_arm(run, arm, &attempt.id, reason, notes);
    }
    // A returned checkout that is contaminated or incomplete is preserved and
    // reported, not consumed: the operator can fix the working tree and an
    // explicit resume re-validates the same recorded attempt. The attempt is
    // never replayed and its identity is unchanged.
    let solution = match validate_solution(run, &attempt, &bindings, &comparison, arm) {
        Ok(solution) => solution,
        Err(reason) => {
            return block(
                run,
                notes,
                format!(
                    "the {} arm solution is not consumable yet: {reason}",
                    arm.as_str()
                ),
            );
        }
    };
    // The verified workload implementation is retained under its own durable
    // owner (B's card) with the exact arm and revision; B's later benefit
    // verdict is a separate decision and is not required for this reference.
    if let Err(reason) = record_workload_implementation(run, &comparison, arm, &solution) {
        return block(
            run,
            notes,
            format!(
                "the verified {} arm implementation could not be retained on workload card {}: {reason}",
                arm.as_str(),
                comparison.workload_card
            ),
        );
    }
    let acceptance = match run_oracle(run, &comparison, arm, &solution) {
        Ok(acceptance) => acceptance,
        Err(reason) => {
            return block(
                run,
                notes,
                format!(
                    "the {} arm acceptance did not complete: {reason}",
                    arm.as_str()
                ),
            );
        }
    };
    let row = build_row(
        run,
        &comparison,
        arm,
        &attempt,
        &runtime,
        &solution,
        &acceptance,
    )?;
    let row_path = run.store.comparison_arm_dir(arm).join("row.json");
    write_json_atomic(&row_path, &row)?;
    let mut state = run
        .cursor
        .comparison
        .clone()
        .unwrap_or_else(|| ComparisonState::new("unset".to_owned()));
    {
        let arm_state = state.arm_mut(arm);
        arm_state.attempt = Some(attempt.id.clone());
        arm_state.revision = Some(solution.revision.clone());
        arm_state.oracle = Some(acceptance.record_path.clone());
        arm_state.accepted = Some(acceptance.passed);
        arm_state.row = Some(row_path.clone());
    }
    run.cursor.comparison = Some(state);
    run.cursor.effect(
        EffectKind::ComparisonArmAccepted,
        format!(
            "arm={} attempt={} revision={} accepted={} oracle={}",
            arm.as_str(),
            attempt.id,
            solution.revision,
            acceptance.passed,
            acceptance.record_path.display()
        ),
    );
    run.cursor.phase = match arm {
        ComparisonArm::Baseline => Phase::CandidateAttempt,
        ComparisonArm::Candidate => Phase::Acceptance,
    };
    run.cursor.condition = None;
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "comparison: the {} arm completed and its committed solution was independently checked (accepted={})",
        arm.as_str(),
        acceptance.passed
    ));
    Ok(())
}

fn refuse_arm(
    run: &mut Run,
    arm: ComparisonArm,
    attempt_id: &str,
    reason: String,
    notes: &mut Vec<String>,
) -> io::Result<()> {
    let mut state = run
        .cursor
        .comparison
        .clone()
        .unwrap_or_else(|| ComparisonState::new("unset".to_owned()));
    {
        let arm_state = state.arm_mut(arm);
        arm_state.attempt = Some(attempt_id.to_owned());
        arm_state.condition = Some(reason.clone());
    }
    run.cursor.comparison = Some(state);
    run.cursor.effect(
        EffectKind::ComparisonArmRefused,
        format!("arm={} attempt={attempt_id}: {reason}", arm.as_str()),
    );
    block(
        run,
        notes,
        format!(
            "the {} arm result did not enter the comparison: {reason}; the attempt is never replayed automatically",
            arm.as_str()
        ),
    )
}

/// Retain one verified workload implementation under B's own card. The record
/// names the frozen base, the exact verified solution revision, the owned
/// worktree and the arm runtime actually consumed; it does not depend on B's
/// later benefit verdict.
fn record_workload_implementation(
    run: &Run,
    comparison: &ComparisonInputs,
    arm: ComparisonArm,
    solution: &Solution,
) -> Result<(), String> {
    let runtime_reference = run
        .cursor
        .comparison
        .as_ref()
        .and_then(|state| state.arm(arm).runtime.clone())
        .map(|path| path.to_string_lossy().into_owned());
    let implementation = board_hypothesis::BoundedImplementation {
        role: board_hypothesis::HypothesisRole::Workload,
        branch: format!("workload-{}", arm.as_str()),
        base: comparison.task.revision.clone(),
        revision: solution.revision.clone(),
        worktree: solution.checkout.to_string_lossy().into_owned(),
        runtime: runtime_reference,
        baseline_runtime: Some(comparison.runtimes.baseline_label.clone()),
    };
    board_hypothesis::record_implementation(
        &run.spec.board.bd,
        &run.spec.board.project,
        &comparison.workload_card,
        &implementation,
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

/// The verified committed solution of one measured arm.
struct Solution {
    revision: String,
    changed_paths: Vec<String>,
    checkout: PathBuf,
}

fn validate_solution(
    run: &Run,
    attempt: &Attempt,
    bindings: &ExperimentBindings,
    comparison: &ComparisonInputs,
    arm: ComparisonArm,
) -> Result<Solution, String> {
    let checkout = attempt
        .checkout
        .clone()
        .ok_or_else(|| "the attempt records no bound checkout".to_owned())?;
    if !checkout.is_dir() {
        return Err(format!(
            "the recorded checkout {} is missing",
            checkout.display()
        ));
    }
    let binding = bindings
        .arm(arm_to_experiment(arm))
        .map_err(|error| error.to_string())?;
    let revision = git(&checkout, &["rev-parse", "HEAD"])
        .map_err(|error| format!("the solution revision is unavailable: {error}"))?
        .trim()
        .to_owned();
    let arm_checkout = run.store.comparison_arm_dir(arm).join("checkout");
    if !git_ok(
        &arm_checkout,
        &["cat-file", "-e", &format!("{revision}^{{commit}}")],
    ) {
        return Err(format!(
            "the solution revision {revision} is not an object of the {} arm's own repository",
            arm.as_str()
        ));
    }
    let base = binding.workload.revision.clone();
    if !git_ok(
        &arm_checkout,
        &["merge-base", "--is-ancestor", &base, &revision],
    ) {
        return Err(format!(
            "the solution revision {revision} is not a descendant of the frozen task revision"
        ));
    }
    let status = git(
        &checkout,
        &["status", "--porcelain", "--untracked-files=normal"],
    )
    .map_err(|error| format!("the solution checkout status is unavailable: {error}"))?;
    let dirty: Vec<&str> = status
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if !dirty.is_empty() {
        return Err(format!(
            "the attempt left {} uncommitted or untracked path(s) ({}); only the committed revision enters acceptance",
            dirty.len(),
            dirty.iter().take(3).copied().collect::<Vec<_>>().join(", ")
        ));
    }
    let changed = git(
        &checkout,
        &["diff", "--name-only", &format!("{base}..{revision}")],
    )
    .map_err(|error| format!("the changed-path list is unavailable: {error}"))?;
    let changed: Vec<String> = changed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    changed_paths_within_scope(
        &changed,
        &comparison.task.writable_scope,
        &comparison.specification.change,
    )?;
    Ok(Solution {
        revision,
        changed_paths: changed,
        checkout,
    })
}

struct Acceptance {
    passed: bool,
    record_path: PathBuf,
    started_at: f64,
    ended_at: f64,
    /// Observed result of preparing the acceptance workspace: the committed
    /// solution was materialized and the workspace cleaned, so both arms are
    /// checked from the same fresh source-only state. Any build products an
    /// acceptance checker needs are prepared by the checker itself and their
    /// time is part of this check's accounted span.
    workspace_preparation: String,
}

/// Materialize the committed solution into the frozen acceptance workspace
/// and run the unchanged supervisor's real-task oracle entry point with the
/// frozen request bytes. The record is retained before its outcome is used.
fn run_oracle(
    run: &Run,
    comparison: &ComparisonInputs,
    arm: ComparisonArm,
    solution: &Solution,
) -> Result<Acceptance, String> {
    let workspace = run
        .cursor
        .comparison
        .as_ref()
        .and_then(|state| state.task_workspace.clone())
        .ok_or_else(|| "the frozen acceptance workspace is not prepared".to_owned())?;
    let frozen_root = read_acceptance_request(&comparison.acceptance).and_then(|request| {
        request["case_root"]
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| "the frozen acceptance request names no case_root".to_owned())
    })?;
    if workspace != frozen_root {
        return Err(
            "the prepared acceptance workspace is not the frozen request's case_root".to_owned(),
        );
    }
    let checkout = solution.checkout.to_string_lossy().into_owned();
    git(
        &workspace,
        &[
            "fetch",
            "-q",
            "--no-tags",
            "--force",
            &checkout,
            &solution.revision,
        ],
    )
    .map_err(|error| format!("the solution could not be materialized for acceptance: {error}"))?;
    git(
        &workspace,
        &["checkout", "-q", "--detach", "-f", &solution.revision],
    )
    .map_err(|error| format!("the acceptance workspace could not be checked out: {error}"))?;
    git(&workspace, &["clean", "-fdxq"])
        .map_err(|error| format!("the acceptance workspace could not be cleaned: {error}"))?;
    let workspace_preparation = "source-only-fresh-workspace".to_owned();

    let started_at = now_seconds();
    let supervisor = std::env::current_exe()
        .map_err(|error| format!("the supervisor executable is unavailable: {error}"))?;
    let output = Command::new(&supervisor)
        .args([
            "outcome-oracle",
            "--request",
            &comparison.acceptance.request.to_string_lossy(),
            "--request-sha256",
            &comparison.acceptance.request_sha256,
        ])
        .output()
        .map_err(|error| format!("the acceptance entry point could not start: {error}"))?;
    let ended_at = now_seconds();
    if output.stdout.len() > MAX_ORACLE_OUTPUT {
        return Err("the acceptance entry point returned unbounded output".to_owned());
    }
    let record: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        format!("the acceptance entry point returned no readable record: {error}")
    })?;
    if record["kind"] != "real-task" || record["executed"] != true {
        return Err(format!(
            "the acceptance entry point did not execute the frozen checker ({}); no result enters the comparison",
            record["failure"]
                .as_str()
                .unwrap_or("no failure cause recorded")
        ));
    }
    let record_path = run.store.comparison_arm_dir(arm).join("oracle.json");
    write_json_atomic(&record_path, &record)
        .map_err(|error| format!("the acceptance record could not be retained: {error}"))?;
    let passed = record["passed"] == true && record["checker_executed"] == true;
    Ok(Acceptance {
        passed,
        record_path,
        started_at,
        ended_at,
        workspace_preparation,
    })
}

/// One authoritative accounting row per arm. The identity fields are the
/// frozen comparison inputs; the measured fields come from the native
/// dispatcher receipt and the rollout owner, never from candidate prose.
fn build_row(
    run: &Run,
    comparison: &ComparisonInputs,
    arm: ComparisonArm,
    attempt: &Attempt,
    runtime: &ArmRuntime,
    solution: &Solution,
    acceptance: &Acceptance,
) -> io::Result<Value> {
    let started_at = attempt.started_ms as f64 / 1000.0;
    let observation = retained_observation(attempt);
    let sessions = discovery_sessions(run, attempt)?;
    let bindings =
        load_bindings(run)?.ok_or_else(|| invalid("the comparison bindings are missing"))?;
    let snapshot = snapshot_facts(
        &bindings
            .arm(Arm::Baseline)
            .map_err(|error| invalid(error.to_string()))?
            .workload,
    )?;
    let matched = matched_fields(
        run,
        comparison,
        runtime,
        &snapshot,
        &acceptance.workspace_preparation,
    )?;
    let mut native = json!({
        "started_at": started_at,
        "ended_at": acceptance.started_at,
        "status": if attempt.state == AttemptState::Completed { "completed" } else { "failed" },
    });
    if let Some(messages) = observation
        .as_ref()
        .and_then(|value| value["messages"].as_u64())
    {
        native["rounds"] = json!(messages);
    }
    if let Some(tools) = observation
        .as_ref()
        .and_then(|value| value["toolCalls"].as_u64())
    {
        native["tool_operations"] = json!(tools);
    }
    if let Some(usage) = usage_totals(&sessions.paths) {
        native["usage"] = usage;
    }
    let declaration = comparison.declared_policy()?.policy.declaration();
    Ok(json!({
        "attempt_id": format!("{}-{}", run.cursor.experiment, attempt.id),
        "case_id": comparison.task.name,
        "arm": arm.as_str(),
        "experiment_id": run.cursor.experiment,
        "unit": format!("{}/{}", run.cursor.experiment, comparison.task.name),
        "started_at": started_at,
        "ended_at": acceptance.ended_at,
        "execution_started_at": started_at,
        "discovery_verified": sessions.verified,
        "observed_model_metadata_verified": sessions.verified,
        "matched": matched,
        "declaration": declaration,
        "native_runs": [native],
        "checks": [{
            "id": "independent-acceptance",
            "round": 0,
            "started_at": acceptance.started_at,
            "ended_at": acceptance.ended_at,
            "required": true,
            "executed": true,
            "passed": acceptance.passed,
            "exit_code": if acceptance.passed { 0 } else { 1 },
            "evidence": acceptance.record_path.display().to_string(),
        }],
        "children": [],
        "interventions": [],
        "retry_of": null,
        "treatment": {
            "label": runtime.label,
            "build": runtime.variant.build.display().to_string(),
            "build_source_sha256": runtime.variant.source_sha256,
            "solution_revision": solution.revision,
            "solution_paths": solution.changed_paths,
            "components": component_inventory(runtime),
        },
    }))
}

fn retained_observation(attempt: &Attempt) -> Option<Value> {
    let retained = attempt.retained.as_ref()?;
    let bytes = crate::outcome_run::bounded_read(
        &retained.receipt,
        harness_core::improvement_loop::MAX_RETAINED_RECEIPT_BYTES,
    )
    .ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value.get("observation").cloned()
}

struct Sessions {
    paths: Vec<PathBuf>,
    verified: bool,
    /// Why the observation is unverified; named for the arm refusal.
    reason: Option<String>,
}

/// Discover this attempt's own rollout files under the arm home and verify
/// the observed model and reasoning effort through the rollout owner. A
/// missing or mismatched session stays unverified with a named reason instead
/// of being inferred.
fn discovery_sessions(run: &Run, attempt: &Attempt) -> io::Result<Sessions> {
    let unverified = |reason: String| Sessions {
        paths: Vec::new(),
        verified: false,
        reason: Some(reason),
    };
    let Some(binding) = &attempt.binding else {
        return Ok(unverified(
            "the attempt records no dispatch identity".to_owned(),
        ));
    };
    let Some(expected) = binding.session.clone() else {
        return Ok(unverified(
            "the accepted dispatch recorded no native session, so no rollout can be attributed to it"
                .to_owned(),
        ));
    };
    let runtime_path = run
        .cursor
        .comparison
        .as_ref()
        .and_then(|state| state.arm(arm_of_role(attempt.role)).runtime.clone());
    let Some(runtime_path) = runtime_path else {
        return Ok(unverified("the arm runtime receipt is missing".to_owned()));
    };
    let runtime: ArmRuntime = read_json(&runtime_path, MAX_RUN_SPEC_BYTES)?;
    let sessions = runtime.home.join("sessions");
    let mut paths = Vec::new();
    if sessions.is_dir() {
        let mut pending = vec![sessions];
        let mut entries = 0usize;
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(&directory)? {
                entries += 1;
                if entries > MAX_ROLLOUT_ENTRIES {
                    return Ok(unverified(
                        "the arm home holds more rollout files than the bounded scan allows"
                            .to_owned(),
                    ));
                }
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if let Some(name) = path.file_name().and_then(|name| name.to_str())
                    && let Some(stem) = name.strip_suffix(".jsonl")
                    && stem.ends_with(&expected)
                {
                    paths.push(path);
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    let (declared_model, declared_effort) = run
        .spec
        .comparison
        .as_ref()
        .map(|comparison| {
            (
                comparison.runtimes.client.runner.model.clone(),
                comparison.runtimes.client.reasoning_effort.clone(),
            )
        })
        .unwrap_or_default();
    if paths.is_empty() {
        return Ok(unverified(format!(
            "no rollout was observed for the attempt's recorded session {expected}; the measured model and effort cannot be verified"
        )));
    }
    let mut observed_model = None;
    for path in &paths {
        let summary = rollout_reader::read(path);
        let id = summary.row["id"].as_str().unwrap_or("");
        let model = summary.row["model"].as_str().unwrap_or("");
        let effort = summary.row["reasoning"].as_str().unwrap_or("");
        let model_matches = model == declared_model
            || model == format!("openai/{declared_model}")
            || model == format!("xai/{declared_model}");
        let effort_matches = declared_effort
            .as_deref()
            .is_none_or(|declared| effort == declared);
        // Identity conflicts disqualify the observation. Provider attribution
        // for a local alias stays a disclosed limit of the accounting, not a
        // reason to drop the measured rollout.
        let disqualified = summary.warnings.iter().any(|warning| {
            matches!(
                warning.as_str(),
                "conflicting_thread_ids"
                    | "missing_thread_id"
                    | "invalid_thread_id"
                    | "mixed_model_attribution"
            )
        });
        if id != expected {
            return Ok(unverified(format!(
                "the observed rollout {} names session {id} instead of the accepted session {expected}",
                path.display()
            )));
        }
        if disqualified {
            return Ok(unverified(format!(
                "the observed rollout {} has conflicting identity facts ({}); the measured model cannot be attributed",
                path.display(),
                summary
                    .warnings
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(",")
            )));
        }
        if !model_matches {
            return Ok(unverified(format!(
                "the observation recorded model {model} instead of the declared {declared_model}"
            )));
        }
        if !effort_matches {
            return Ok(unverified(format!(
                "the observation recorded reasoning effort {effort} instead of the declared {}",
                declared_effort.as_deref().unwrap_or("default")
            )));
        }
        observed_model = Some(model.to_owned());
    }
    Ok(Sessions {
        paths,
        verified: observed_model.is_some(),
        reason: None,
    })
}

fn arm_of_role(role: AttemptRole) -> ComparisonArm {
    match role {
        AttemptRole::Candidate => ComparisonArm::Candidate,
        _ => ComparisonArm::Baseline,
    }
}

/// Measured token totals through the existing delegation reader. A partial or
/// missing measurement is omitted, never reported as a fabricated zero.
fn usage_totals(paths: &[PathBuf]) -> Option<Value> {
    if paths.is_empty() {
        return None;
    }
    let (value, _sources) = crate::delegation_usage::summarize(paths);
    let totals = value["totals"].as_object()?;
    // A measured total requires every counted thread to carry usage and no
    // conflicting duplicate identities. Provider/price attribution for a
    // local alias stays a disclosed accounting limit instead of a fabricated
    // cost or a reason to discard measured tokens.
    if totals.get("missing_usage").and_then(Value::as_u64) != Some(0)
        || value["warnings"].as_array().is_some_and(|warnings| {
            warnings.iter().any(|warning| {
                matches!(
                    warning["code"].as_str(),
                    Some("counter_total_overflow" | "conflicting_duplicate_id")
                )
            })
        })
    {
        return None;
    }
    let mut result = serde_json::Map::new();
    for key in [
        "input_tokens",
        "cached_input_tokens",
        "output_tokens",
        "reasoning_output_tokens",
        "total_tokens",
    ] {
        let value = totals.get(key)?;
        value.as_u64()?;
        result.insert(key.to_owned(), value.clone());
    }
    Some(Value::Object(result))
}

/// The frozen workload snapshot's own instruction, skill and hook identities,
/// read from the frozen revision through the Git owner. An absent path is an
/// observed absence (`absent`), never an assumed constant.
struct SnapshotFacts {
    instructions: String,
    skills: String,
    hooks: String,
}

fn snapshot_facts(workload: &harness_core::task_worktree::FrozenCopy) -> io::Result<SnapshotFacts> {
    let listing = |pathspec: &[&str]| -> Option<String> {
        let mut args = vec!["ls-tree", "-r", workload.revision.as_str(), "--"];
        args.extend_from_slice(pathspec);
        let text = git(&workload.path, &args).ok()?;
        let text = text.trim();
        (!text.is_empty()).then(|| sha16(&build_identity::hash_bytes(text.as_bytes())))
    };
    let absent = || "absent".to_owned();
    let instructions = match git(
        &workload.path,
        &[
            "cat-file",
            "-e",
            &format!("{}:AGENTS.md", workload.revision),
        ],
    ) {
        Ok(_) => {
            let bytes = git(
                &workload.path,
                &["show", &format!("{}:AGENTS.md", workload.revision)],
            )
            .map_err(invalid)?
            .into_bytes();
            sha16(&build_identity::hash_bytes(&bytes))
        }
        Err(_) => absent(),
    };
    Ok(SnapshotFacts {
        instructions,
        skills: listing(&[".agents/skills"]).unwrap_or_else(absent),
        hooks: listing(&["hooks.json", ".codex/hooks.json"]).unwrap_or_else(absent),
    })
}

/// The invariant comparison identities the authoritative accounting requires,
/// derived from the frozen declaration and from verified per-arm consumption.
/// A fact that cannot be observed stays absent from the map so the accounting
/// reports it as unknown and refuses the pair instead of accepting a plausible
/// constant. The declared treatment (the harness build and its source) is
/// recorded outside `matched`.
fn matched_fields(
    run: &Run,
    comparison: &ComparisonInputs,
    runtime: &ArmRuntime,
    snapshot: &SnapshotFacts,
    workspace_preparation: &str,
) -> io::Result<Value> {
    let bindings =
        load_bindings(run)?.ok_or_else(|| invalid("the comparison bindings are missing"))?;
    let baseline = bindings
        .arm(Arm::Baseline)
        .map_err(|error| invalid(error.to_string()))?;
    let policy = comparison.declared_policy().map_err(|error| {
        invalid(format!(
            "the declared comparison policy is unusable: {error}"
        ))
    })?;
    let planning: PlanningReceipt =
        read_json(&run.store.comparison_planning_path(), MAX_RUN_SPEC_BYTES)?;
    let request = read_acceptance_request(&comparison.acceptance).map_err(invalid)?;
    let program_sha = request["oracle"]["program_sha256"]
        .as_str()
        .unwrap_or_default();
    let scope_digest = sha16(&build_identity::hash_bytes(
        comparison.task.writable_scope.join(",").as_bytes(),
    ));
    let frozen_task = sha16(&build_identity::hash_bytes(
        format!(
            "{}:{}",
            baseline.workload.revision, baseline.workload.tree_sha256
        )
        .as_bytes(),
    ));
    let client = runtime
        .configuration
        .as_ref()
        .and_then(|configuration| configuration.client.as_ref());
    let mut matched = serde_json::Map::new();
    for key in MATCH_FIELDS {
        let value: Option<String> = match *key {
            "case_revision" => Some(baseline.workload.revision.clone()),
            "source_state" => Some(baseline.workload.tree_sha256.clone()),
            "input_identity" => Some(format!("task:{frozen_task}")),
            "runtime" => client.map(|client| {
                format!(
                    "client:{}:{}",
                    sha16(&runtime.upstream_sha256),
                    client.runner.wire_api
                )
            }),
            "model" => client.map(|client| client.runner.model.clone()),
            "effort" => client.map(|client| {
                client
                    .reasoning_effort
                    .clone()
                    .unwrap_or_else(|| "default".to_owned())
            }),
            "provider" => client.map(|client| client.runner.kind.clone()),
            "config_identity" => runtime
                .configuration
                .as_ref()
                .map(|configuration| format!("sha256:{}", sha16(&configuration.sha256))),
            "tool_identity" => Some(format!(
                "accept:{program_sha}:frozen:{}",
                sha16(&baseline.workload.tree_sha256)
            )),
            "hook_revision" => Some(snapshot.hooks.clone()),
            "allowed_effects" => Some(format!("scope:{scope_digest}")),
            "cache_policy" => Some(workspace_preparation.to_owned()),
            "preparation_policy" => preparation_policy(run, runtime)?,
            "budget" => Some(format!(
                "attempts:{}",
                policy.policy.stopping.max_attempts_per_arm
            )),
            "oracle_identity" => Some(format!("sha256:{}", comparison.acceptance.request_sha256)),
            "instructions_identity" => Some(snapshot.instructions.clone()),
            "other_skills" => Some(snapshot.skills.clone()),
            "stop_conditions" => Some(policy.policy.stopping_text()),
            "criterion" => Some(format!("contract:{}", sha16(&planning.contract_digest))),
            _ => None,
        };
        if let Some(value) = value {
            matched.insert((*key).to_owned(), Value::String(value));
        }
    }
    Ok(Value::Object(matched))
}

/// Preparation rule both arms must share. Optional component names, statuses,
/// links and hashes are observed treatment identity on the arm runtime, not
/// part of this comparable method.
const PREPARATION_METHOD: &str = "owner-install/v1";

fn preparation_method_path(runtime_path: &Path) -> PathBuf {
    runtime_path.with_file_name("preparation-method.json")
}

fn write_preparation_method(runtime_path: &Path) -> io::Result<()> {
    write_json_atomic(
        &preparation_method_path(runtime_path),
        &json!({
            "schema": 1,
            "method": PREPARATION_METHOD,
        }),
    )
}

fn read_preparation_method(runtime_path: &Path) -> io::Result<Option<String>> {
    let path = preparation_method_path(runtime_path);
    if !path.is_file() {
        return Ok(None);
    }
    let value: Value = read_json(&path, 4 * 1024)?;
    let method = value.get("method").and_then(Value::as_str).unwrap_or("");
    if value.get("schema").and_then(Value::as_u64) == Some(1) && preparation_method_token(method) {
        Ok(Some(format!("method:{method}")))
    } else {
        Ok(None)
    }
}

fn preparation_method_token(method: &str) -> bool {
    !method.is_empty()
        && method.len() <= 64
        && method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.'))
}

/// Component inventory retained with the accounting row. The runtime receipt
/// remains the consumption record; this copy is the observed treatment identity
/// and is not a matched comparison field.
fn component_inventory(runtime: &ArmRuntime) -> Value {
    json!(
        runtime
            .components
            .iter()
            .map(|component| {
                json!({
                    "name": component.name,
                    "status": component.status,
                    "sourceIdentity": component.source_identity,
                    "vendorVersion": component.vendor_version,
                    "stateSha256": component.state_sha256,
                    "links": component.links.iter().map(|link| json!({
                        "name": link.name,
                        "sha256": link.sha256,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>()
    )
}

fn preparation_policy(run: &Run, runtime: &ArmRuntime) -> io::Result<Option<String>> {
    let arm = match runtime.arm {
        Arm::Baseline => ComparisonArm::Baseline,
        Arm::Candidate => ComparisonArm::Candidate,
    };
    let Some(runtime_path) = run
        .cursor
        .comparison
        .as_ref()
        .and_then(|state| state.arm(arm).runtime.clone())
    else {
        return Ok(None);
    };
    read_preparation_method(&runtime_path)
}

// ---------------------------------------------------------------------------
// Evaluation and decision publication.
// ---------------------------------------------------------------------------

fn publish_decision(run: &mut Run, notes: &mut Vec<String>) -> io::Result<()> {
    let Some(comparison) = run.spec.comparison.clone() else {
        return Ok(());
    };
    let Some(state) = run.cursor.comparison.clone() else {
        return Ok(());
    };
    if state.decision.is_some() {
        if run.cursor.phase != Phase::DecisionRecorded {
            run.cursor.phase = Phase::DecisionRecorded;
            run.store.save_cursor(&run.cursor)?;
        }
        return Ok(());
    }
    let (Some(baseline_row), Some(candidate_row)) =
        (state.baseline.row.clone(), state.candidate.row.clone())
    else {
        return Ok(());
    };
    let baseline_row: Value = read_json(&baseline_row, MAX_RUN_SPEC_BYTES)?;
    let candidate_row: Value = read_json(&candidate_row, MAX_RUN_SPEC_BYTES)?;
    let declared = comparison
        .declared_policy()
        .map_err(|error| invalid(format!("the predeclared policy is unusable: {error}")))?;
    let report = summarize_attempts(&[baseline_row, candidate_row])?;
    let report_path = run.store.comparison_dir().join("report.json");
    write_json_atomic(&report_path, &report)?;
    let evaluation = harness_core::improvement_policy::evaluate(&declared, &report)
        .map_err(|error| invalid(error.to_string()))?;
    let evaluation_path = run.store.comparison_dir().join("evaluation.json");
    write_json_atomic(&evaluation_path, &evaluation)?;
    let bindings =
        load_bindings(run)?.ok_or_else(|| invalid("the comparison bindings are missing"))?;
    // One lineage identity everywhere: the exact raw Git revisions of the
    // frozen experiment binding and its acceptance reference. The existing
    // integration/activation owner re-derives the expected decision record
    // from exactly these values, so a comparison-produced adoption is
    // consumable without a translation step.
    let baseline_revision = bindings.candidate.base.clone();
    let candidate_revision = bindings.candidate.revision.clone();
    let acceptance = bindings.acceptance.clone();
    let item = run
        .cursor
        .candidate
        .as_ref()
        .map(|candidate| candidate.hypothesis.clone())
        .unwrap_or_else(|| run.spec.hypothesis_item.clone());
    let experiment = run.cursor.experiment.clone();
    let publication = match evaluation.decision_draft(
        &item,
        &experiment,
        &baseline_revision,
        &candidate_revision,
        &acceptance,
    ) {
        Ok(draft) => {
            benefit_gate::publish_decision(&run.spec.board.bd, &run.spec.board.project, &draft)
                .map_err(|error| {
                    invalid(format!("the decision publication was refused: {error}"))
                })?
        }
        Err(_)
            if evaluation.decision != harness_core::improvement_policy::PolicyDecision::Adopt =>
        {
            // A comparison whose arms did not both produce a comparable,
            // independently accepted result has no matched arm seconds and no
            // defined per-success cost. The frozen policy's reject or
            // inconclusive verdict is published as a supported non-adoption
            // that binds the same experiment, revisions and acceptance; no
            // matched count, arm seconds or zero cost is fabricated, and the
            // newest non-adoption authorizes no integration or activation.
            let draft = benefit_gate::NonAdoptionDraft {
                item: item.clone(),
                experiment: experiment.clone(),
                outcome: match evaluation.decision {
                    harness_core::improvement_policy::PolicyDecision::Reject => {
                        benefit_gate::DecisionOutcome::Reject
                    }
                    _ => benefit_gate::DecisionOutcome::Inconclusive,
                },
                quality: match evaluation.quality {
                    harness_core::improvement_policy::Quality::Unchanged => {
                        benefit_gate::QualityOutcome::Unchanged
                    }
                    harness_core::improvement_policy::Quality::Improved => {
                        benefit_gate::QualityOutcome::Improved
                    }
                    harness_core::improvement_policy::Quality::Regressed => {
                        benefit_gate::QualityOutcome::Regressed
                    }
                    harness_core::improvement_policy::Quality::Unmeasurable => {
                        benefit_gate::QualityOutcome::Unmeasurable
                    }
                },
                accounting: harness_core::improvement_policy::board_token(
                    &format!(
                        "attempts:{} tasks:{} accepted:{} per_success:{}",
                        evaluation.attempts,
                        evaluation.tasks,
                        evaluation.accepted_tasks,
                        match evaluation.per_success.status {
                            harness_core::improvement_policy::PerSuccessStatus::Complete =>
                                "complete",
                            harness_core::improvement_policy::PerSuccessStatus::Incomplete =>
                                "incomplete",
                            harness_core::improvement_policy::PerSuccessStatus::Undefined =>
                                "undefined",
                        }
                    ),
                    160,
                ),
                baseline_revision: baseline_revision.clone(),
                candidate_revision: candidate_revision.clone(),
                acceptance: acceptance.clone(),
                coverage: harness_core::improvement_policy::board_token(
                    if evaluation.coverage.is_empty() {
                        "none"
                    } else {
                        &evaluation.coverage
                    },
                    160,
                ),
                scope: harness_core::improvement_policy::board_token(&evaluation.scope, 160),
                reason: harness_core::improvement_policy::board_token(
                    &evaluation.reasons.join(";"),
                    160,
                ),
                detail: (!evaluation.reasons.is_empty()
                    && evaluation.reasons.join("; ").len() <= 512
                    && !evaluation.reasons.join("; ").contains(['\n', '\r']))
                .then(|| evaluation.reasons.join("; ")),
            };
            benefit_gate::publish_non_adoption(&run.spec.board.bd, &run.spec.board.project, &draft)
                .map_err(|error| {
                    invalid(format!("the non-adoption publication was refused: {error}"))
                })?
        }
        Err(error) => {
            return block(
                run,
                notes,
                format!(
                    "the adopted comparison cannot produce a decision record ({error}); nothing was published"
                ),
            );
        }
    };
    let (status, text) = match publication {
        benefit_gate::Publication::Recorded { text } => ("recorded", text),
        benefit_gate::Publication::Confirmed { text } => ("already-recorded", text),
    };
    let head = text
        .split_once(" detail=")
        .map_or(text.as_str(), |(head, _)| head);
    let decision_path = run.store.comparison_dir().join("decision.json");
    write_json_atomic(
        &decision_path,
        &json!({
            "status": status,
            "head": head,
            "sha256": build_identity::hash_bytes(text.as_bytes()),
            "decision": evaluation.decision.as_str(),
            "experiment": experiment,
            "baseline_revision": baseline_revision,
            "candidate_revision": candidate_revision,
            "acceptance": acceptance,
        }),
    )?;
    let mut state = run.cursor.comparison.clone().unwrap_or(state);
    state.report = Some(report_path);
    state.evaluation = Some(evaluation_path);
    state.decision = Some(format!(
        "benefit-gate v2 item={item} experiment={experiment} revisions={baseline_revision}..{candidate_revision} outcome={} status={status}",
        evaluation.decision.as_str()
    ));
    run.cursor.comparison = Some(state);
    run.cursor.effect(
        EffectKind::ComparisonDecisionRecorded,
        format!(
            "decision={} status={status} (evidence-bound verdict only; integration and activation stay separate)",
            evaluation.decision.as_str()
        ),
    );
    run.cursor.phase = Phase::DecisionRecorded;
    run.cursor.condition = Some(format!(
        "the evidence-bound decision {} is published through the benefit-gate owner; integration, baseline activation and live publication remain separate owners",
        evaluation.decision.as_str()
    ));
    run.store.save_cursor(&run.cursor)?;
    notes.push(format!(
        "comparison: published decision {} ({status}) with matched={} coverage={}",
        evaluation.decision.as_str(),
        evaluation.matched,
        evaluation.coverage
    ));
    Ok(())
}
