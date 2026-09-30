//! Consumed-package identity for skill comparison arms (model-free).
//!
//! The outcome oracle answers a name-level question: was a path matching
//! `<name>/SKILL.md` used by a successful completed tool item? That signal
//! cannot distinguish the frozen compared revision from a live or drifted
//! library copy, and it does not attribute references or executable helpers.
//! This module classifies the same retained native tool evidence against the
//! frozen package: the catalogue identity and every frozen replica root, the
//! selected body, consumed references and helpers, and any live-library
//! fallback. A treatment arm without attributable frozen consumption is
//! missing treatment; a contaminated or drifted arm is retained with explicit
//! evidence and excluded from a causal claim. All of this is deterministic and
//! makes no model call.
//!
//! Comparison flows include this module through `#[path]`; not every target
//! consumes every record below, so the shared contract surface allows dead
//! code per target.
#![allow(dead_code)]

use serde::Serialize;
use serde_json::Value;
use skill_evolution::{comparison, decision, package, plan};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

const EVIDENCE_FILE_LIMIT: u64 = 64 * 1024 * 1024;
const EVIDENCE_RECORD_LIMIT: usize = 8 * 1024 * 1024;
const SNIPPET_LEAD: usize = 60;
const SNIPPET_TAIL: usize = 40;

/// Successful completed tool items from one retained native evidence root,
/// reduced to their tool-input text (`command` plus `arguments`). Success
/// semantics mirror the outcome oracle: a command item must be `completed`
/// with an integer zero exit code; an MCP item must be `completed` without an
/// `error` field or a true `result.isError`. A missing log, non-UTF-8 bytes,
/// an oversized record or a malformed JSON row is an error: unknown evidence
/// is never treated as a clean absence.
pub fn evidence_inputs(evidence_root: &Path) -> io::Result<Vec<String>> {
    let path = evidence_root.join("events.jsonl");
    let bytes = match fs::read(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(io::Error::other("native event log is missing"));
        }
        Err(error) => return Err(error),
        Ok(bytes) => bytes,
    };
    if bytes.len() as u64 > EVIDENCE_FILE_LIMIT {
        return Err(io::Error::other("native event log exceeds its bound"));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| io::Error::other("native event log is not UTF-8"))?;
    let mut inputs = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > EVIDENCE_RECORD_LIMIT {
            return Err(io::Error::other("native event record exceeds its bound"));
        }
        let row: Value = serde_json::from_str(line)
            .map_err(|_| io::Error::other("native event record is malformed"))?;
        if row.get("type").and_then(Value::as_str) != Some("item.completed") {
            continue;
        }
        if let Some(input) = row.get("item").and_then(successful_input) {
            inputs.push(input);
        }
    }
    Ok(inputs)
}

fn successful_input(item: &Value) -> Option<String> {
    let completed = item.get("status").and_then(Value::as_str) == Some("completed");
    match item.get("type").and_then(Value::as_str) {
        Some("command_execution")
            if completed
                && matches!(
                    item.get("exit_code").and_then(|value| value.as_i64()),
                    Some(0)
                ) =>
        {
            Some(tool_input(item))
        }
        Some("mcp_tool_call")
            if completed
                && item.get("error").is_none_or(Value::is_null)
                && matches!(
                    item.pointer("/result/isError"),
                    None | Some(Value::Bool(false)) | Some(Value::Null)
                ) =>
        {
            Some(tool_input(item))
        }
        _ => None,
    }
}

fn tool_input(item: &Value) -> String {
    let command = item.get("command").and_then(Value::as_str).unwrap_or("");
    let arguments = match item.get("arguments") {
        None | Some(Value::Null) => "{}".to_owned(),
        Some(value) => value.to_string(),
    };
    command.to_owned() + &arguments
}

/// Case role in the declared comparison workflows. Intended, boundary and
/// held-out arms exercise the treatment; similar-unsuitable and protected
/// overlapping workflows exercise the expected absence of activation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Intended,
    SimilarUnsuitable,
    BoundaryFailure,
    IndependentHeldOut,
    ProtectedOverlapping,
}

/// The declared workflows of the two local comparison batches plus the
/// protected process workflow. Held-out cases are never refinement cases.
pub fn declared_workflows() -> Vec<(&'static str, &'static str, Role)> {
    vec![
        ("shortening", plan::INTENDED_CASE, Role::Intended),
        ("shortening", plan::NEGATIVE_CASE, Role::SimilarUnsuitable),
        ("shortening", plan::BOUNDARY_CASE, Role::BoundaryFailure),
        ("shortening", plan::HELD_OUT_CASE, Role::IndependentHeldOut),
        ("add-absence", plan::ACCEPT_INTENDED, Role::Intended),
        (
            "add-absence",
            plan::ACCEPT_NEGATIVE,
            Role::SimilarUnsuitable,
        ),
        ("add-absence", plan::ACCEPT_NEGATIVE, Role::BoundaryFailure),
        (
            "add-absence",
            plan::ACCEPT_HELD_OUT,
            Role::IndependentHeldOut,
        ),
        ("protected", "process", Role::ProtectedOverlapping),
    ]
}

/// What one arm of a workflow must demonstrate. Absence arms (the skill is
/// disabled) and similar-unsuitable candidates must show no activation of the
/// compared skill; other candidate arms must consume the frozen revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expectation {
    TreatmentExpected,
    ActivationForbidden,
}

pub fn expectation(kind: comparison::Kind, role: Role, candidate: bool) -> Expectation {
    match kind {
        comparison::Kind::AddAbsence if !candidate => Expectation::ActivationForbidden,
        _ if role == Role::SimilarUnsuitable => Expectation::ActivationForbidden,
        _ => Expectation::TreatmentExpected,
    }
}

/// One frozen compared package: the catalogue identity, every replica root
/// that must hold it, and the declared live-library roots whose use is
/// fallback contamination.
pub struct Frozen {
    pub identity: package::Identity,
    pub roots: Vec<PathBuf>,
    pub live_roots: Vec<PathBuf>,
}

impl Frozen {
    pub fn new(identity: package::Identity, roots: Vec<PathBuf>, live_roots: Vec<PathBuf>) -> Self {
        Self {
            identity,
            roots,
            live_roots,
        }
    }

    /// Post-run proof that every declared replica root still holds the frozen
    /// revision. A changed, missing or unreadable replica is reference drift:
    /// the attempt is not a comparable execution of the declared candidate.
    pub fn replica_drift(&self) -> Vec<String> {
        let mut drift = Vec::new();
        for root in &self.roots {
            match package::load(root) {
                Ok(identity) if identity.revision == self.identity.revision => {}
                Ok(_) => drift.push(format!("{} changed revision", root.display())),
                Err(error) => drift.push(format!("{} unreadable ({error})", root.display())),
            }
        }
        drift
    }

    /// Attributed and unattributed consumption in the retained tool inputs.
    pub fn classify(&self, inputs: &[String]) -> Consumption {
        let mut consumption = Consumption::default();
        for input in inputs {
            let text = normalize(input);
            for root in &self.roots {
                let base = normalize(&root.to_string_lossy());
                for relative in self.identity.files.keys() {
                    let needle = format!("{base}/{}", normalize(relative));
                    if text.contains(&needle) {
                        let absolute = root.join(relative).to_string_lossy().into_owned();
                        match relative.as_str() {
                            "SKILL.md" => consumption.body.push(absolute),
                            _ if relative.starts_with("references/") => {
                                consumption.references.push(absolute);
                            }
                            _ => consumption.helpers.push(absolute),
                        }
                    }
                }
            }
            for root in &self.live_roots {
                let needle = format!("{}/", normalize(&root.to_string_lossy()));
                if let Some(start) = text.find(&needle) {
                    consumption.live.push(snippet(&text, start, &needle));
                }
            }
            let marker = format!("{}/skill.md", normalize(&self.identity.name));
            for start in name_positions(&text, &marker) {
                let prefix = &text[..start];
                let attributed = self.roots.iter().any(|root| {
                    prefix.ends_with(&format!("{}/", normalize(&root.to_string_lossy())))
                }) || self.live_roots.iter().any(|root| {
                    prefix.ends_with(&format!("{}/", normalize(&root.to_string_lossy())))
                });
                if !attributed {
                    consumption.foreign.push(snippet(&text, start, &marker));
                }
            }
        }
        consumption.body.sort();
        consumption.body.dedup();
        consumption.references.sort();
        consumption.references.dedup();
        consumption.helpers.sort();
        consumption.helpers.dedup();
        consumption.live.sort();
        consumption.live.dedup();
        consumption.foreign.sort();
        consumption.foreign.dedup();
        consumption
    }

    /// The oracle's name-level signal for the same inputs: any successful
    /// `<name>/SKILL.md` reference regardless of which library it resolves to.
    pub fn name_referenced(&self, inputs: &[String]) -> bool {
        let marker = format!("{}/skill.md", normalize(&self.identity.name));
        inputs
            .iter()
            .any(|input| !name_positions(&normalize(input), &marker).is_empty())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Consumption {
    /// Consumed frozen `SKILL.md` paths (the selected body).
    pub body: Vec<String>,
    /// Consumed frozen `references/` files.
    pub references: Vec<String>,
    /// Consumed other frozen package files (executable resources/helpers).
    pub helpers: Vec<String>,
    /// Consumed paths under a declared live-library root (fallback).
    pub live: Vec<String>,
    /// `<name>/SKILL.md` references that resolve to no declared root
    /// (an unattributable revision).
    pub foreign: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArmVerdict {
    /// Treatment arm with attributable frozen body consumption.
    Attributed,
    /// Treatment arm with no consumed frozen body (retained, not comparable).
    MissingTreatment,
    /// Consumed a live or unattributable library (retained, excluded).
    Contaminated,
    /// Replica revision differs from the frozen revision (retained, excluded).
    Drifted,
    /// Evidence unreadable or incomplete (retained, not comparable).
    Incomplete,
    /// Absence expectation demonstrated: no activation of the compared skill.
    Absent,
    /// Absence expectation violated: the compared skill activated.
    Activated,
}

/// Frozen arm rule. Contamination and drift are decided before treatment, so a
/// live-library read can never be counted as a comparable treatment execution.
pub fn arm_verdict(
    expectation: Expectation,
    consumption: &Consumption,
    replica_drift: &[String],
    errors: &[String],
) -> ArmVerdict {
    if !errors.is_empty() {
        return ArmVerdict::Incomplete;
    }
    if !replica_drift.is_empty() {
        return ArmVerdict::Drifted;
    }
    if !consumption.live.is_empty() || !consumption.foreign.is_empty() {
        return ArmVerdict::Contaminated;
    }
    match expectation {
        Expectation::TreatmentExpected if consumption.body.is_empty() => {
            ArmVerdict::MissingTreatment
        }
        Expectation::TreatmentExpected => ArmVerdict::Attributed,
        Expectation::ActivationForbidden if consumption.body.is_empty() => ArmVerdict::Absent,
        Expectation::ActivationForbidden => ArmVerdict::Activated,
    }
}

/// Consumed-package evidence for one executed arm: classification, verdict,
/// the oracle cross-check and any replica drift. Model-free and serializable
/// so an arm record always carries its consumption identity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ArmConsumption {
    pub consumed: Option<Consumption>,
    pub verdict: Option<ArmVerdict>,
    pub name_referenced: Option<bool>,
    pub oracle_cross_check: Option<bool>,
    pub replica_drift: Vec<String>,
    pub error: Option<String>,
}

/// Collect consumption evidence from one retained arm. The oracle
/// cross-check compares the oracle's activation check with this module's
/// independent name-level reference classification; a mismatch is retained as
/// disagreement, never as agreement.
pub fn collect_arm(
    kind: comparison::Kind,
    role: Role,
    candidate: bool,
    frozen: &Frozen,
    evidence_root: &Path,
    oracle: &Value,
) -> ArmConsumption {
    let mut evidence = ArmConsumption {
        replica_drift: frozen.replica_drift(),
        ..ArmConsumption::default()
    };
    match evidence_inputs(evidence_root) {
        Ok(inputs) => {
            let consumption = frozen.classify(&inputs);
            let expectation = expectation(kind, role, candidate);
            evidence.verdict = Some(arm_verdict(
                expectation,
                &consumption,
                &evidence.replica_drift,
                &[],
            ));
            let referenced = frozen.name_referenced(&inputs);
            evidence.name_referenced = Some(referenced);
            let checks = &oracle["details"]["checks"];
            evidence.oracle_cross_check = match (
                checks.get("positive_activation").and_then(Value::as_bool),
                checks.get("negative_activation").and_then(Value::as_bool),
            ) {
                (Some(positive), _) => Some(positive == referenced),
                (_, Some(negative)) => Some(!referenced || !negative),
                _ => Some(false),
            };
            evidence.consumed = Some(consumption);
        }
        Err(error) => evidence.error = Some(error.to_string()),
    }
    evidence
}

/// The comparison dimensions a completed arm must account for before it can
/// support a decision: complete accepted-task cost, required capability/tool
/// discovery, visible errors with detail recovery, cache basis, and stated
/// uncertainty. Missing accounting is incomplete evidence, never a pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EvidenceVector {
    pub accepted_task_cost: Option<String>,
    pub required_discovery: bool,
    pub errors_detail_recovery: bool,
    pub cache_basis: Option<String>,
    pub uncertainty: Option<String>,
}

impl EvidenceVector {
    pub fn complete(&self) -> bool {
        self.accepted_task_cost
            .as_deref()
            .is_some_and(|cost| !cost.is_empty())
            && self.required_discovery
            && self.errors_detail_recovery
            && self
                .cache_basis
                .as_deref()
                .is_some_and(|basis| !basis.is_empty())
            && self
                .uncertainty
                .as_deref()
                .is_some_and(|uncertainty| !uncertainty.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmSummary {
    pub run_status: String,
    pub oracle_passed: Option<bool>,
    pub oracle_readable: bool,
    pub oracle_agreement: bool,
    pub verdict: Option<ArmVerdict>,
    pub live_unchanged: bool,
    pub model: String,
    pub effort: String,
    pub elapsed_seconds: Option<u64>,
    pub evidence: EvidenceVector,
}

impl Default for ArmSummary {
    fn default() -> Self {
        Self {
            run_status: String::new(),
            oracle_passed: None,
            oracle_readable: false,
            oracle_agreement: true,
            verdict: None,
            live_unchanged: true,
            model: String::new(),
            effort: String::new(),
            elapsed_seconds: None,
            evidence: EvidenceVector::default(),
        }
    }
}

pub struct CasePair {
    pub case_id: String,
    pub role: Role,
    pub baseline: ArmSummary,
    pub candidate: ArmSummary,
}

/// One executed batch: its declared workflows, frozen acceptance conditions
/// and the recorded arms. Only executed batches are ever passed here.
pub struct BatchFacts {
    pub kind: comparison::Kind,
    pub model: String,
    pub effort: String,
    pub timeout_seconds: u64,
    pub declared: Vec<(String, Role)>,
    pub pairs: Vec<CasePair>,
}

/// Frozen acceptance rule for one executed batch. Contamination, reference
/// drift, missing treatment, incomplete accounting, provider mismatch, an
/// over-budget arm or a missing declared workflow cannot authorize a
/// candidate; a harmed similar-unsuitable workflow is a protected regression.
pub fn frozen_batch_evidence(facts: &BatchFacts) -> decision::ComparisonEvidence {
    let skipped = facts.declared.is_empty()
        || facts.pairs.is_empty()
        || facts
            .declared
            .iter()
            .any(|(case_id, _)| !facts.pairs.iter().any(|pair| &pair.case_id == case_id))
        || facts.pairs.iter().any(|pair| {
            !facts
                .declared
                .iter()
                .any(|(case_id, _)| case_id == &pair.case_id)
        });
    let mut integrity_ok = !skipped;
    let mut complete = !skipped;
    let mut must_pass = !facts.pairs.is_empty();
    let mut protected_regression = false;
    let (mut saw_intended, mut saw_negative, mut saw_held_out) = (false, false, false);
    let (mut intended_gain, mut held_out_gain, mut negative_ok) = (false, false, true);
    for pair in &facts.pairs {
        for arm in [&pair.baseline, &pair.candidate] {
            integrity_ok &= arm.run_status == "completed"
                && arm.oracle_readable
                && arm.oracle_agreement
                && arm.live_unchanged;
            complete &= arm.verdict.is_some() && arm.evidence.complete();
        }
        let candidate_expectation = expectation(facts.kind, pair.role, true);
        let candidate_required = match candidate_expectation {
            Expectation::TreatmentExpected => ArmVerdict::Attributed,
            Expectation::ActivationForbidden => ArmVerdict::Absent,
        };
        let baseline_required = match expectation(facts.kind, pair.role, false) {
            Expectation::TreatmentExpected => ArmVerdict::Attributed,
            Expectation::ActivationForbidden => ArmVerdict::Absent,
        };
        complete &= pair.candidate.verdict.as_ref() == Some(&candidate_required);
        complete &= pair.baseline.verdict.as_ref() == Some(&baseline_required);
        must_pass &= pair.candidate.oracle_passed == Some(true);
        let candidate_passed = pair.candidate.oracle_passed == Some(true);
        let baseline_passed = pair.baseline.oracle_passed == Some(true);
        match pair.role {
            Role::Intended => {
                saw_intended = true;
                intended_gain = candidate_passed && !baseline_passed;
            }
            Role::IndependentHeldOut => {
                saw_held_out = true;
                held_out_gain = candidate_passed && !baseline_passed;
            }
            Role::SimilarUnsuitable => {
                saw_negative = true;
                negative_ok = candidate_passed && baseline_passed;
                protected_regression |= !candidate_passed;
            }
            Role::BoundaryFailure | Role::ProtectedOverlapping => {}
        }
    }
    let provider_matched = !facts.pairs.is_empty()
        && facts.pairs.iter().all(|pair| {
            [&pair.baseline, &pair.candidate].iter().all(|arm| {
                !arm.model.is_empty()
                    && arm.model == facts.model
                    && !arm.effort.is_empty()
                    && arm.effort == facts.effort
            })
        });
    let within_budgets = !facts.pairs.is_empty()
        && facts.pairs.iter().all(|pair| {
            [&pair.baseline, &pair.candidate].iter().all(|arm| {
                arm.elapsed_seconds
                    .is_some_and(|elapsed| elapsed <= facts.timeout_seconds)
            })
        });
    let evidence_complete =
        complete && provider_matched && saw_intended && saw_negative && saw_held_out;
    let selection_demonstrated = complete && saw_intended && saw_negative;
    let benefit_established =
        evidence_complete && intended_gain && held_out_gain && negative_ok && must_pass;
    decision::ComparisonEvidence {
        integrity_ok,
        evidence_complete,
        provider_matched,
        must_pass,
        selection_demonstrated,
        protected_regression,
        benefit_established,
        within_budgets,
        claim: decision::Claim::Capability,
        skipped_required_check: skipped,
        single_lucky_run: !(intended_gain && held_out_gain),
        meaningful_difference: intended_gain || held_out_gain,
    }
}

/// One explicitly selected comparison. Execution stays behind its gate and a
/// model-backed comparison that has not been authorized or run is pending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComparisonSelection {
    pub id: &'static str,
    pub kind: comparison::Kind,
    pub gate: &'static str,
    pub batch: &'static str,
}

pub fn declared_comparisons() -> [ComparisonSelection; 4] {
    [
        ComparisonSelection {
            id: "shortening-first-pair",
            kind: comparison::Kind::UpdateOld,
            gate: "HARNESS_NATIVE_CODEX",
            batch: "shortening first pair (entrypoint, --run-model-probes)",
        },
        ComparisonSelection {
            id: "shortening-update-old",
            kind: comparison::Kind::UpdateOld,
            gate: "HARNESS_SKILL_PILOT_NEXT",
            batch: "shortening batch (intended/negative/boundary/held-out)",
        },
        ComparisonSelection {
            id: "independent-add-absence",
            kind: comparison::Kind::AddAbsence,
            gate: "HARNESS_SKILL_LEARNING_CYCLE",
            batch: "independent add-absence batch",
        },
        ComparisonSelection {
            id: "process-absence-probes",
            kind: comparison::Kind::AddAbsence,
            gate: "HARNESS_SKILL_LEARNING_CYCLE",
            batch: "protected process workflow probes",
        },
    ]
}

pub fn selection_state(gate_value: Option<&str>) -> &'static str {
    if gate_value == Some("1") {
        "selected"
    } else {
        "pending"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Adopt,
    Reject,
    Inconclusive,
}

/// Frozen integration rule: nothing is adopted without an executed comparison
/// that the frozen acceptance accepted. Unproven candidates keep their prior
/// defaults and remain pending.
pub fn integration_decision(executed: Option<decision::Verdict>) -> (Decision, &'static str) {
    match executed {
        None => (
            Decision::Inconclusive,
            "no selected comparison has executed; a model-backed comparison stays pending, not passed",
        ),
        Some(decision::Verdict::Accept) => (
            Decision::Adopt,
            "the frozen acceptance accepted the executed comparison",
        ),
        Some(decision::Verdict::Reject) => (
            Decision::Reject,
            "the frozen acceptance rejected the executed comparison",
        ),
        Some(decision::Verdict::Inconclusive) => (
            Decision::Inconclusive,
            "the executed comparison stayed inconclusive under the frozen acceptance",
        ),
    }
}

/// The comparison dimensions every frozen acceptance plan must name.
pub const ACCEPTANCE_METRICS: [&str; 6] = [
    "complete_accepted_task_cost",
    "required_discovery",
    "errors_and_detail_recovery",
    "cache_basis",
    "intended_negative_held_out_behavior",
    "uncertainty_and_stopping",
];

pub struct AcceptancePlan {
    pub roles: &'static [Role],
    pub metrics: &'static [&'static str],
    pub uncertainty: &'static str,
}

pub enum Support {
    /// Installed support and a presentation surface are qualified.
    Qualified { evidence: &'static str },
    /// The installed surface provides no supported mechanism. No replacement
    /// runtime is introduced; the candidate stays recorded and pending.
    Unsupported { reason: &'static str },
}

pub struct Candidate {
    pub id: &'static str,
    pub area: &'static str,
    pub finding: &'static str,
    pub recurring_measurement: &'static str,
    pub treatment: &'static str,
    pub acceptance: AcceptancePlan,
    pub support: Support,
    pub observability: &'static [&'static str],
    pub owner_boundary: &'static str,
    pub prior_default: &'static str,
    pub reconsideration: &'static str,
}

const ALL_ROLES: [Role; 5] = [
    Role::Intended,
    Role::SimilarUnsuitable,
    Role::BoundaryFailure,
    Role::IndependentHeldOut,
    Role::ProtectedOverlapping,
];

/// The four context candidates selected from the audit and design decision 12.
/// Each freezes its treatment and acceptance plan; the deferred-discovery
/// mechanism is unsupported on the installed consumer and no replacement
/// runtime is introduced for it.
pub fn context_candidates() -> [Candidate; 4] {
    [
        Candidate {
            id: "instruction-mechanics-layout",
            area: "instruction mechanics/layout",
            finding: "A29",
            recurring_measurement: "Skill bodies restate mechanical procedures that already have deterministic owners; catalogue metadata cost is distinct from body/reference cost after selection.",
            treatment: "Restructure an owned skill body so authority, publication, recovery and completion rules stay intact while mechanical procedures move to their deterministic owner; the compared package revision is frozen as a fixture before any arm runs.",
            acceptance: AcceptancePlan {
                roles: &ALL_ROLES,
                metrics: &ACCEPTANCE_METRICS,
                uncertainty: "Predeclared cases only; a provider-unmatched, unreadable or unattributed arm stays inconclusive and reduced instruction bytes never override a failed outcome.",
            },
            support: Support::Qualified {
                evidence: "Installed Codex 0.157.1 registers [[skills.config]] path/enabled entries and the existing outcome-prepare/outcome-run/outcome-oracle flow executes compared package revisions (model-gated).",
            },
            observability: &[
                "per-arm consumed-package classification (body, references, helpers, fallback)",
                "host-frozen oracle checks over retained native tool evidence",
            ],
            owner_boundary: "owned skill/instruction revision outside this assignment's outputs; selection and acceptance are frozen here, application stays with its owner",
            prior_default: "current instruction layout and owned skill bodies",
            reconsideration: "an explicitly authorized model-backed batch on the declared workflows with attributable frozen consumption and complete accounting",
        },
        Candidate {
            id: "stable-prefix-rendering",
            area: "stable-prefix rendering",
            finding: "A32",
            recurring_measurement: "Every request re-renders instruction sections; prefix reuse is observable only as per-attempt cached input tokens, so reordering is a candidate, not an assumed saving.",
            treatment: "Keep stable authoritative sections ahead of changing sections in the rendered request without changing their authority or relative priority; stable instructions retain their meaning.",
            acceptance: AcceptancePlan {
                roles: &ALL_ROLES,
                metrics: &ACCEPTANCE_METRICS,
                uncertainty: "Cache basis is within-arm only; a change without comparable cache or task evidence stays inconclusive and no subscription saving is claimed from bytes.",
            },
            support: Support::Qualified {
                evidence: "Native per-attempt usage carries cached_input_tokens (harness-core rollout_reader.rs reads token_usage_record; codex-harness outcome_run.rs reports per-arm usage).",
            },
            observability: &[
                "per-attempt native usage including cached_input_tokens and turn/thread usage",
                "retained request/context evidence for the delivered section order",
            ],
            owner_boundary: "instruction text and any configuration ordering owner outside this assignment's outputs",
            prior_default: "current rendered section order",
            reconsideration: "a selected comparison on comparable cases whose recorded cache basis and complete-task acceptance are both available",
        },
        Candidate {
            id: "native-deferred-discovery",
            area: "native deferred discovery",
            finding: "A33",
            recurring_measurement: "Tool and MCP definitions are delivered each request; deferring discovery is only available if the installed consumer supports it.",
            treatment: "Defer tool definitions and discover them on demand through the native consumer when that mechanism is supported and observable.",
            acceptance: AcceptancePlan {
                roles: &ALL_ROLES,
                metrics: &ACCEPTANCE_METRICS,
                uncertainty: "A failed required discovery is an acceptance failure, not a saving; unsupported installed mechanisms are recorded, not replaced.",
            },
            support: Support::Unsupported {
                reason: "Model-free probe of installed Codex 0.157.1 (registered upstream sha256 8cb0e69e99ff2a158c54815db82d0f2e524d8f301bc30184722cfd1ae5973574): `codex features list` reports tool_search=removed, tool_search_always_defer_mcp_tools=removed, deferred_tool_world_state=under development/false, deferred_executor=under development/false, executor_capability_discovery=under development/false; skill_search and tool_suggest cover skills and suggestions only. No supported deferred tool-definition route exists on the installed surface.",
            },
            observability: &[
                "required native deferred-definition surface (absent on installed 0.157.1)",
            ],
            owner_boundary: "native installed feature surface (external); the kit introduces no replacement discovery runtime",
            prior_default: "current non-deferred tool definition delivery",
            reconsideration: "an installed consumer exposing a supported deferred-definition feature, qualified by the same model-free feature probe before any model run",
        },
        Candidate {
            id: "deterministic-aggregation",
            area: "deterministic Code Mode/native aggregation",
            finding: "A34",
            recurring_measurement: "Recurring deterministic chains (sorting, joining known identifiers, aggregating structured results) can move intermediate payloads out of model context when an existing owner can do the work.",
            treatment: "Route a measured deterministic chain through the existing Code Mode/native aggregation owner and compare the complete task rather than only the intermediate payload.",
            acceptance: AcceptancePlan {
                roles: &ALL_ROLES,
                metrics: &ACCEPTANCE_METRICS,
                uncertainty: "Foreign benchmark percentages are not local evidence; a chain without a measured recurring consumer stays unselected.",
            },
            support: Support::Qualified {
                evidence: "Existing owners: `codex features list` reports code_mode_host=stable/true and code_mode=under development/true on installed 0.157.1; harness-rtk diagnostics report measured raw and delivered bytes.",
            },
            observability: &[
                "compact delivered-presentation accounting (applied vs raw bytes)",
                "complete-task oracle acceptance over retained native evidence",
            ],
            owner_boundary: "existing Code Mode/RTK owners; measured chain selection is pending",
            prior_default: "current model-visible intermediate payloads",
            reconsideration: "a measured recurring deterministic chain with an existing owner, compared on the complete task",
        },
    ]
}

/// Live user and checkout skill roots. References under these roots are
/// live-library fallback, not comparable treatment consumption.
pub fn live_skill_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let profile = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME"));
    if let Some(profile) = profile {
        roots.push(PathBuf::from(profile).join(".agents").join("skills"));
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    roots.push(workspace.join(".agents").join("skills"));
    roots
        .into_iter()
        .filter_map(|root| root.canonicalize().ok())
        .collect()
}

fn normalize(text: &str) -> String {
    text.replace('\\', "/").to_lowercase()
}

/// Positions of `<name>/SKILL.md` occurrences that are preceded by a path
/// separator, mirroring the outcome oracle's `[/\\]+<name>[/\\]+SKILL\.md`.
fn name_positions(text: &str, marker: &str) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut from = 0;
    while let Some(offset) = text[from..].find(marker) {
        let start = from + offset;
        if text[..start].ends_with('/') {
            positions.push(start);
        }
        from = start + marker.len();
    }
    positions
}

fn snippet(text: &str, start: usize, needle: &str) -> String {
    let lead: Vec<char> = text[..start].chars().rev().take(SNIPPET_LEAD).collect();
    let lead: String = lead.into_iter().rev().collect();
    let tail: String = text[start + needle.len()..]
        .chars()
        .take(SNIPPET_TAIL)
        .collect();
    format!("{lead}{needle}{tail}")
}
