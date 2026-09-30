//! Repeatability qualification and configuration drift for an explicit local
//! runner. Model-free: it consumes a declared serving identity and observed
//! attempt evidence; it never calls a model, endpoint or process.
//!
//! Qualification requires the controlled required solution output to repeat
//! under identical inputs through the actual agent and tool path; every repeat
//! must have exercised a tool round-trip. Sampling settings or a fixed seed are
//! recorded identity, not a qualification criterion. A finite qualification
//! states observed repeatability for one declared configuration, not
//! mathematical determinism of future executions. Divergence, unknown material
//! identity or configuration drift suspends dependent strict comparisons until
//! corrected or a different repeatability policy is explicitly agreed.
//!
//! Server-side cache and batching settings are material identity facts, not
//! noise: documented serving behavior shows that prompt-cache reuse under
//! differing batch state can change logits, so a fixed seed or temperature
//! setting alone cannot establish repeatability.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn invalid(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

/// Material identity field names in declaration order.
pub const MATERIAL_FIELDS: [&str; 11] = [
    "weights",
    "quantization",
    "tokenizer",
    "template",
    "backend",
    "sampling",
    "seed",
    "reasoning",
    "context",
    "cache",
    "environment",
];

/// Declared material identity facts of one local serving configuration.
///
/// Every fact is recorded as supplied by the operator. A missing fact is
/// unknown material identity and blocks strict dependent comparisons rather
/// than being filled with a guessed value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialIdentity {
    pub weights: Option<String>,
    pub quantization: Option<String>,
    pub tokenizer: Option<String>,
    pub template: Option<String>,
    pub backend: Option<String>,
    pub sampling: Option<String>,
    pub seed: Option<String>,
    pub reasoning: Option<String>,
    pub context: Option<String>,
    pub cache: Option<String>,
    pub environment: Option<String>,
}

impl MaterialIdentity {
    /// Names of the absent material facts, in declaration order.
    ///
    /// A blank or literal `unknown` placeholder counts as absent, matching the
    /// accounting rule for stale identity: it cannot authorize a comparison.
    pub fn missing(&self) -> Vec<String> {
        MATERIAL_FIELDS
            .iter()
            .filter(|name| {
                self.declared(name).is_none_or(|value| {
                    let value = value.trim();
                    value.is_empty() || value.eq_ignore_ascii_case("unknown")
                })
            })
            .map(|name| (*name).to_owned())
            .collect()
    }

    /// Raw declared value of a [`MATERIAL_FIELDS`] name; `None` when absent.
    pub fn declared(&self, name: &str) -> Option<&str> {
        let value = match name {
            "weights" => &self.weights,
            "quantization" => &self.quantization,
            "tokenizer" => &self.tokenizer,
            "template" => &self.template,
            "backend" => &self.backend,
            "sampling" => &self.sampling,
            "seed" => &self.seed,
            "reasoning" => &self.reasoning,
            "context" => &self.context,
            "cache" => &self.cache,
            "environment" => &self.environment,
            _ => &None,
        };
        value.as_deref()
    }
}

/// Explicit local runner configuration shared by both comparison arms.
///
/// The endpoint and model are route inputs; the declared identity is recorded
/// evidence for qualification and drift. A run never falls back to another
/// provider, endpoint or model when this configuration is selected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRunner {
    /// Local serving base URL (scheme, host and optional path; no credentials).
    pub endpoint: String,
    /// Model name the endpoint serves.
    pub model: String,
    #[serde(default)]
    pub identity: MaterialIdentity,
}

impl LocalRunner {
    /// Names of absent material identity facts; empty means fully declared.
    pub fn missing_identity(&self) -> Vec<String> {
        self.identity.missing()
    }
}

/// Wire protocol the installed native client accepts for custom providers.
/// `responses` is the only supported value for `model_providers.<id>.wire_api`.
pub const WIRE_API: &str = "responses";

/// One recorded local runner identity as it appears in attempt evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerRecord {
    pub kind: String,
    pub endpoint: String,
    pub model: String,
    pub wire_api: String,
    pub identity: MaterialIdentity,
    pub identity_missing: Vec<String>,
}

impl RunnerRecord {
    pub fn new(runner: &LocalRunner) -> Self {
        Self {
            kind: "local".into(),
            endpoint: runner.endpoint.clone(),
            model: runner.model.clone(),
            wire_api: WIRE_API.into(),
            identity: runner.identity.clone(),
            identity_missing: runner.missing_identity(),
        }
    }

    /// The configuration this record was produced from.
    pub fn runner(&self) -> LocalRunner {
        LocalRunner {
            endpoint: self.endpoint.clone(),
            model: self.model.clone(),
            identity: self.identity.clone(),
        }
    }
}

/// Rule fixed before repeated observations, echoed in every result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatabilityPolicy {
    /// Declared number of controlled repeats (at least two).
    pub repeats: usize,
    /// Required output paths compared by content digest across repeats.
    pub required_outputs: Vec<String>,
    /// Nonsemantic metadata explicitly excluded from output equality by this rule.
    pub ignored_metadata: Vec<String>,
}

impl RepeatabilityPolicy {
    /// Checks the declared rule; a contradiction or an undeclared comparison
    /// target is a caller error, not a blocked qualification.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.repeats < 2 {
            return Err("repeatability policy requires at least two repeats");
        }
        if self.required_outputs.is_empty() {
            return Err("repeatability policy requires at least one required output");
        }
        for name in &self.required_outputs {
            if !name_is_bounded(name) {
                return Err(
                    "required output name is empty, oversized or contains control characters",
                );
            }
        }
        if self.required_outputs.iter().collect::<BTreeSet<_>>().len()
            != self.required_outputs.len()
        {
            return Err("required output names must be unique");
        }
        for name in &self.ignored_metadata {
            if !name_is_bounded(name) {
                return Err(
                    "ignored metadata name is empty, oversized or contains control characters",
                );
            }
        }
        if self
            .ignored_metadata
            .iter()
            .any(|name| self.required_outputs.contains(name))
        {
            return Err("a required output cannot also be declared nonsemantic metadata");
        }
        Ok(())
    }
}

fn name_is_bounded(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty() && trimmed.len() <= 512 && !name.chars().any(char::is_control)
}

/// One completed controlled attempt observed through the actual agent/tool
/// path. `outputs` maps each required output path to its content digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationAttempt {
    /// Stable attempt locator (thread identity or local attempt identity).
    pub attempt_id: String,
    /// The attempt completed successfully through the real entry point. Failed,
    /// cancelled, timed-out or unknown outcomes stay recorded but block strict
    /// comparisons even when their required outputs match.
    pub completed: bool,
    /// The attempt's evidence verified the observed model/effort metadata.
    pub model_metadata_verified: bool,
    /// Completed tool round-trips observed in this attempt.
    pub tool_operations: u64,
    /// Runner identity recorded in this attempt's own evidence; `None` when the
    /// attempt recorded no runner identity.
    pub runner: Option<RunnerRecord>,
    /// Content digests of the compared outputs, keyed by required path.
    pub outputs: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QualificationStatus {
    Qualified,
    Blocked,
}

/// Result of one repeatability qualification over declared controlled repeats.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qualification {
    pub status: QualificationStatus,
    /// The declared rule this result was evaluated under.
    pub policy: RepeatabilityPolicy,
    /// Declared material identity facts that are absent.
    pub missing_identity: Vec<String>,
    /// Attempts that did not complete successfully through the real entry point.
    pub unfinished_attempts: Vec<String>,
    /// Attempts whose observed model metadata was not verified.
    pub unverified_attempts: Vec<String>,
    /// Attempts with no observed tool round-trip.
    pub tool_exchange_missing: Vec<String>,
    /// Attempts whose recorded runner identity was absent or drifted.
    pub runner_mismatch: Vec<AttemptDrift>,
    /// Required outputs absent from at least one repeat.
    pub missing_outputs: Vec<String>,
    /// Required outputs that differed across repeats.
    pub divergent_outputs: Vec<String>,
    pub observed_repeats: usize,
    pub required_repeats: usize,
}

impl Qualification {
    pub fn qualified(&self) -> bool {
        self.status == QualificationStatus::Qualified
    }

    /// Dependent strict comparisons must not run while a qualification blocks.
    pub fn blocks_comparisons(&self) -> bool {
        !self.qualified()
    }
}

/// One attempt whose recorded runner identity is absent or differs from the
/// expected configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptDrift {
    pub attempt_id: String,
    /// Changed configuration field names; empty when the attempt recorded no
    /// runner identity at all.
    pub changed: Vec<String>,
}

/// Basic execution evidence from controlled attempts through the real
/// agent/tool path.
///
/// This is a connectivity/repeated-tool smoke check, not strict repeatability
/// qualification: it declares no repeat count or equality rule, tolerates
/// unknown material identity and never authorizes dependent comparisons.
/// Weight hashes, startup flags and server shell access are not required to
/// establish that the configured route executes tool calls.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSmoke {
    pub observed_attempts: usize,
    /// Attempts whose observed model metadata was verified through the rollout.
    pub verified_attempts: usize,
    /// Attempts with at least one completed tool round-trip.
    pub tool_attempts: usize,
    /// Attempts that completed, verified their own observed model metadata and
    /// executed at least one tool round-trip in the same attempt.
    pub executing_verified_attempts: usize,
    /// Attempt ids of those attempts, in observation order.
    pub executing_attempt_ids: Vec<String>,
    pub tool_operations: u64,
    /// Material identity facts still unknown; strict comparisons stay blocked.
    pub missing_identity: Vec<String>,
}

impl ExecutionSmoke {
    /// The route executed through the agent and tools at least once: one
    /// attempt must, by itself, have completed, verified its observed model
    /// metadata and exercised a tool round-trip. Facts from different attempts
    /// are never combined.
    pub fn basic_execution_observed(&self) -> bool {
        self.executing_verified_attempts > 0
    }
}

/// Summarizes basic execution evidence. Strict comparisons remain gated by
/// [`qualify`]; this function reports only what the route observably executed.
pub fn smoke(runner: &LocalRunner, attempts: &[QualificationAttempt]) -> ExecutionSmoke {
    let executing: Vec<&QualificationAttempt> = attempts
        .iter()
        .filter(|attempt| {
            attempt.completed && attempt.model_metadata_verified && attempt.tool_operations > 0
        })
        .collect();
    ExecutionSmoke {
        observed_attempts: attempts.len(),
        verified_attempts: attempts
            .iter()
            .filter(|attempt| attempt.model_metadata_verified)
            .count(),
        tool_attempts: attempts
            .iter()
            .filter(|attempt| attempt.tool_operations > 0)
            .count(),
        executing_verified_attempts: executing.len(),
        executing_attempt_ids: executing
            .iter()
            .map(|attempt| attempt.attempt_id.clone())
            .collect(),
        tool_operations: attempts
            .iter()
            .map(|attempt| attempt.tool_operations)
            .fold(0_u64, u64::saturating_add),
        missing_identity: runner.missing_identity(),
    }
}

/// Evaluates the declared repeatability rule over controlled attempts.
///
/// Output equality uses only the declared required outputs; extra attempt
/// outputs and timing are not compared. Qualification requires every attempt
/// to have completed successfully, verified its own observed model metadata,
/// executed its own tool round-trip and recorded the exact expected runner
/// identity. Divergence, absent material identity, an unfinished attempt, an
/// absent or drifted attempt runner record, an absent tool round-trip, a
/// missing required output or a repeat-count mismatch all block qualification.
pub fn qualify(
    runner: &LocalRunner,
    policy: &RepeatabilityPolicy,
    attempts: &[QualificationAttempt],
) -> std::io::Result<Qualification> {
    policy.validate().map_err(invalid)?;
    let mut identities = BTreeSet::new();
    for attempt in attempts {
        if !name_is_bounded(&attempt.attempt_id)
            || !identities.insert(attempt.attempt_id.as_str())
            || attempt
                .outputs
                .iter()
                .any(|(path, digest)| !name_is_bounded(path) || !name_is_bounded(digest))
        {
            return Err(invalid(
                "qualification attempt identity or output digest is invalid or duplicated",
            ));
        }
    }
    let mut missing_outputs = BTreeSet::new();
    let mut divergent_outputs = BTreeSet::new();
    for required in &policy.required_outputs {
        let mut seen = Vec::new();
        let mut absent = false;
        for attempt in attempts {
            match attempt.outputs.get(required.as_str()) {
                Some(digest) => seen.push(digest.as_str()),
                None => absent = true,
            }
        }
        if absent {
            missing_outputs.insert(required.clone());
        } else if seen.windows(2).any(|pair| pair[0] != pair[1]) {
            divergent_outputs.insert(required.clone());
        }
    }
    let missing_identity = runner.missing_identity();
    let unfinished_attempts: Vec<String> = attempts
        .iter()
        .filter(|attempt| !attempt.completed)
        .map(|attempt| attempt.attempt_id.clone())
        .collect();
    let unverified_attempts: Vec<String> = attempts
        .iter()
        .filter(|attempt| !attempt.model_metadata_verified)
        .map(|attempt| attempt.attempt_id.clone())
        .collect();
    let tool_exchange_missing: Vec<String> = attempts
        .iter()
        .filter(|attempt| attempt.tool_operations == 0)
        .map(|attempt| attempt.attempt_id.clone())
        .collect();
    let expected = RunnerRecord::new(runner);
    let runner_mismatch: Vec<AttemptDrift> = attempts
        .iter()
        .filter_map(|attempt| match &attempt.runner {
            None => Some(AttemptDrift {
                attempt_id: attempt.attempt_id.clone(),
                changed: Vec::new(),
            }),
            Some(record) => {
                let observed = record_drift(&expected, record);
                observed.drifted.then_some(AttemptDrift {
                    attempt_id: attempt.attempt_id.clone(),
                    changed: observed.changed,
                })
            }
        })
        .collect();
    let blocked = !missing_identity.is_empty()
        || !unfinished_attempts.is_empty()
        || !unverified_attempts.is_empty()
        || !tool_exchange_missing.is_empty()
        || !runner_mismatch.is_empty()
        || !missing_outputs.is_empty()
        || !divergent_outputs.is_empty()
        || attempts.len() != policy.repeats;
    Ok(Qualification {
        status: if blocked {
            QualificationStatus::Blocked
        } else {
            QualificationStatus::Qualified
        },
        policy: policy.clone(),
        missing_identity,
        unfinished_attempts,
        unverified_attempts,
        tool_exchange_missing,
        runner_mismatch,
        missing_outputs: missing_outputs.into_iter().collect(),
        divergent_outputs: divergent_outputs.into_iter().collect(),
        observed_repeats: attempts.len(),
        required_repeats: policy.repeats,
    })
}

/// Names of configuration fields that differ between two recorded runners.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drift {
    pub drifted: bool,
    pub changed: Vec<String>,
}

/// Compares two declared runner configurations. Any changed or newly known or
/// newly unknown material fact is drift: the recorded identity no longer
/// describes the same serving configuration.
pub fn drift(before: &LocalRunner, after: &LocalRunner) -> Drift {
    record_drift(&RunnerRecord::new(before), &RunnerRecord::new(after))
}

/// Compares an expected configuration against a runner identity recorded in
/// one attempt's evidence. Kind, endpoint, model, wire protocol and every
/// material identity fact must match for the attempt to belong to the same
/// serving configuration.
pub fn record_drift(expected: &RunnerRecord, observed: &RunnerRecord) -> Drift {
    let mut changed = Vec::new();
    for (name, left, right) in [
        ("kind", &expected.kind, &observed.kind),
        ("endpoint", &expected.endpoint, &observed.endpoint),
        ("model", &expected.model, &observed.model),
        ("wire_api", &expected.wire_api, &observed.wire_api),
    ] {
        if left != right {
            changed.push(name.to_owned());
        }
    }
    for name in MATERIAL_FIELDS {
        if identity_field(&expected.identity, name) != identity_field(&observed.identity, name) {
            changed.push(format!("identity.{name}"));
        }
    }
    Drift {
        drifted: !changed.is_empty(),
        changed,
    }
}

fn identity_field<'a>(identity: &'a MaterialIdentity, name: &str) -> &'a Option<String> {
    match name {
        "weights" => &identity.weights,
        "quantization" => &identity.quantization,
        "tokenizer" => &identity.tokenizer,
        "template" => &identity.template,
        "backend" => &identity.backend,
        "sampling" => &identity.sampling,
        "seed" => &identity.seed,
        "reasoning" => &identity.reasoning,
        "context" => &identity.context,
        "cache" => &identity.cache,
        "environment" => &identity.environment,
        _ => &None,
    }
}

/// Builds one observed qualification attempt from a native attempt result and
/// the required output digests collected from its controlled case.
///
/// Missing observation evidence stays conservative: an absent model-metadata
/// verification is unverified, an absent tool-operation counter counts as no
/// observed tool round-trip, a status other than `completed` is unfinished and
/// an absent runner record cannot prove the expected configuration.
pub fn qualification_attempt(
    result: &Value,
    outputs: BTreeMap<String, String>,
) -> std::io::Result<QualificationAttempt> {
    let attempt_id = result["thread_id"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid("attempt result has no thread identity"))?;
    let runner = result
        .get("runner")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<RunnerRecord>(value.clone()))
        .transpose()
        .map_err(|_| invalid("attempt runner record is invalid"))?;
    Ok(QualificationAttempt {
        attempt_id: attempt_id.to_owned(),
        completed: result["status"] == "completed",
        model_metadata_verified: result["observed_model_metadata_verified"] == Value::Bool(true),
        tool_operations: result["tool_operations"].as_u64().unwrap_or(0),
        runner,
        outputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn identity() -> MaterialIdentity {
        MaterialIdentity {
            weights: Some("synth-gguf-sha256-0000".into()),
            quantization: Some("Q4_K_M".into()),
            tokenizer: Some("synth-tokenizer-v1".into()),
            template: Some("synth-template-v3".into()),
            backend: Some("synth-server-build-1".into()),
            sampling: Some("temperature=1.0;top_k=20;top_p=0.95".into()),
            seed: Some("server-default-random".into()),
            reasoning: Some("xhigh".into()),
            context: Some("262144".into()),
            cache: Some("single-slot".into()),
            environment: Some("owned fixture double; no inference".into()),
        }
    }

    fn runner() -> LocalRunner {
        LocalRunner {
            endpoint: "http://127.0.0.1:65500/v1".into(),
            model: "fixture-model-x".into(),
            identity: identity(),
        }
    }

    fn policy() -> RepeatabilityPolicy {
        RepeatabilityPolicy {
            repeats: 2,
            required_outputs: vec!["solution.txt".into()],
            ignored_metadata: vec!["timing".into(), "thread_id".into()],
        }
    }

    fn attempt(id: &str, tools: u64, digest: &str) -> QualificationAttempt {
        QualificationAttempt {
            attempt_id: id.into(),
            completed: true,
            model_metadata_verified: true,
            tool_operations: tools,
            runner: Some(RunnerRecord::new(&runner())),
            outputs: BTreeMap::from([("solution.txt".to_owned(), digest.to_owned())]),
        }
    }

    #[test]
    fn repeatable_required_output_through_tools_qualifies() {
        let result = qualify(
            &runner(),
            &policy(),
            &[attempt("a", 3, "d1"), attempt("b", 1, "d1")],
        )
        .unwrap();
        assert!(result.qualified(), "{result:?}");
        assert!(!result.blocks_comparisons());
        assert_eq!(result.status, QualificationStatus::Qualified);
        assert_eq!(result.policy, policy());
        assert!(result.missing_identity.is_empty());
        assert!(result.divergent_outputs.is_empty());
        assert_eq!(result.observed_repeats, 2);
        assert_eq!(result.required_repeats, 2);
    }

    #[test]
    fn divergent_required_output_blocks_and_names_the_output() {
        let result = qualify(
            &runner(),
            &policy(),
            &[attempt("a", 2, "d1"), attempt("b", 2, "d2")],
        )
        .unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(result.divergent_outputs, vec!["solution.txt"]);
        assert!(result.missing_outputs.is_empty());
    }

    #[test]
    fn missing_output_and_absent_tool_round_trip_block_separately() {
        let mut first = attempt("a", 2, "d1");
        let second = attempt("b", 0, "d1");
        first.outputs.clear();
        let result = qualify(&runner(), &policy(), &[first, second]).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(result.missing_outputs, vec!["solution.txt"]);
        assert_eq!(result.tool_exchange_missing, vec!["b"]);
        assert!(result.divergent_outputs.is_empty());
    }

    #[test]
    fn unknown_material_identity_blocks_with_declared_gaps() {
        let mut partial = runner();
        partial.identity.quantization = None;
        partial.identity.cache = None;
        let result = qualify(
            &partial,
            &policy(),
            &[attempt("a", 2, "d1"), attempt("b", 2, "d1")],
        )
        .unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(result.missing_identity, vec!["quantization", "cache"]);
        let empty = LocalRunner {
            identity: MaterialIdentity::default(),
            ..runner()
        };
        assert_eq!(empty.missing_identity(), MATERIAL_FIELDS.to_vec());
        // A literal `unknown` placeholder is not a declared material fact.
        let mut placeholder = runner();
        placeholder.identity.weights = Some(" unknown ".into());
        assert_eq!(placeholder.missing_identity(), vec!["weights"]);
    }

    #[test]
    fn unverified_model_metadata_blocks() {
        let mut second = attempt("b", 2, "d1");
        second.model_metadata_verified = false;
        let result = qualify(&runner(), &policy(), &[attempt("a", 2, "d1"), second]).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(result.unverified_attempts, vec!["b"]);
    }

    #[test]
    fn unfinished_attempts_cannot_qualify_even_with_matching_outputs() {
        // A failed, cancelled, timed-out or unknown outcome never qualifies,
        // even when the required output matches the completed repeat.
        let mut unfinished = attempt("b", 3, "d1");
        unfinished.completed = false;
        let result = qualify(&runner(), &policy(), &[attempt("a", 3, "d1"), unfinished]).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(result.unfinished_attempts, vec!["b"]);
        assert!(result.divergent_outputs.is_empty());
        assert!(result.runner_mismatch.is_empty());
    }

    #[test]
    fn attempt_runner_identity_mismatch_blocks_qualification() {
        let mut drifted = attempt("b", 3, "d1");
        drifted.runner.as_mut().unwrap().identity.quantization = Some("Q5_K_M".into());
        let result = qualify(&runner(), &policy(), &[attempt("a", 3, "d1"), drifted]).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(
            result.runner_mismatch,
            vec![AttemptDrift {
                attempt_id: "b".into(),
                changed: vec!["identity.quantization".into()],
            }]
        );
        assert!(result.divergent_outputs.is_empty());

        // An attempt that recorded no runner identity cannot prove the expected
        // configuration.
        let mut unrecorded = attempt("c", 3, "d1");
        unrecorded.runner = None;
        let result = qualify(&runner(), &policy(), &[attempt("a", 3, "d1"), unrecorded]).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(
            result.runner_mismatch,
            vec![AttemptDrift {
                attempt_id: "c".into(),
                changed: vec![],
            }]
        );
    }

    #[test]
    fn smoke_requires_one_attempt_with_its_own_verified_tool_execution() {
        // Verified metadata without tools plus an unverified attempt with tools
        // never proves basic execution: no single attempt did both.
        let mut text_only = attempt("a", 0, "d1");
        text_only.model_metadata_verified = true;
        let mut unverified_tools = attempt("b", 3, "d1");
        unverified_tools.model_metadata_verified = false;
        let result = smoke(&runner(), &[text_only.clone(), unverified_tools.clone()]);
        assert_eq!(result.verified_attempts, 1);
        assert_eq!(result.tool_attempts, 1);
        assert_eq!(result.executing_verified_attempts, 0);
        assert!(!result.basic_execution_observed());

        // An unfinished attempt does not prove basic execution either.
        let mut unfinished = attempt("c", 3, "d1");
        unfinished.completed = false;
        assert!(!smoke(&runner(), &[unfinished]).basic_execution_observed());

        // One attempt with its own completion, verified metadata and tool
        // round-trip does.
        let result = smoke(
            &runner(),
            &[text_only, unverified_tools, attempt("d", 2, "d1")],
        );
        assert_eq!(result.executing_verified_attempts, 1);
        assert_eq!(result.executing_attempt_ids, vec!["d"]);
        assert!(result.basic_execution_observed());
    }

    #[test]
    fn repeat_count_and_attempt_identity_are_enforced() {
        let result = qualify(&runner(), &policy(), &[attempt("a", 2, "d1")]).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!((result.observed_repeats, result.required_repeats), (1, 2));
        let result = qualify(
            &runner(),
            &policy(),
            &[
                attempt("a", 2, "d1"),
                attempt("b", 2, "d1"),
                attempt("c", 2, "d1"),
            ],
        )
        .unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!((result.observed_repeats, result.required_repeats), (3, 2));
        assert!(
            qualify(
                &runner(),
                &policy(),
                &[attempt("a", 2, "d1"), attempt("a", 2, "d1")]
            )
            .is_err()
        );
        assert!(
            qualify(
                &runner(),
                &policy(),
                &[attempt(" ", 2, "d1"), attempt("b", 2, "d1")]
            )
            .is_err()
        );
    }

    #[test]
    fn sampling_settings_alone_do_not_qualify() {
        let result = qualify(&runner(), &policy(), &[]).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(result.observed_repeats, 0);
        assert!(result.tool_exchange_missing.is_empty());
    }

    #[test]
    fn smoke_reports_basic_execution_without_authorizing_comparisons() {
        let mut partial = runner();
        partial.identity.weights = None;
        partial.identity.quantization = None;
        let attempts = [attempt("a", 3, "d1"), attempt("b", 2, "d1")];
        let result = smoke(&partial, &attempts);
        assert!(result.basic_execution_observed());
        assert_eq!(result.observed_attempts, 2);
        assert_eq!(result.verified_attempts, 2);
        assert_eq!(result.tool_attempts, 2);
        assert_eq!(result.tool_operations, 5);
        assert_eq!(result.missing_identity, vec!["weights", "quantization"]);
        // The unknown identity still blocks strict dependent comparisons.
        assert!(
            qualify(&partial, &policy(), &attempts)
                .unwrap()
                .blocks_comparisons()
        );
        assert!(!smoke(&partial, &[attempt("c", 0, "d1")]).basic_execution_observed());
    }

    #[test]
    fn policy_requires_a_declared_coherent_rule() {
        for policy in [
            RepeatabilityPolicy {
                repeats: 1,
                ..policy()
            },
            RepeatabilityPolicy {
                required_outputs: vec![],
                ..policy()
            },
            RepeatabilityPolicy {
                required_outputs: vec!["a".into(), "a".into()],
                ..policy()
            },
            RepeatabilityPolicy {
                required_outputs: vec!["timing".into()],
                ..policy()
            },
            RepeatabilityPolicy {
                required_outputs: vec![" ".into()],
                ..policy()
            },
            RepeatabilityPolicy {
                ignored_metadata: vec!["bad\u{7}name".into()],
                ..policy()
            },
        ] {
            assert!(policy.validate().is_err(), "{policy:?}");
            assert!(
                qualify(
                    &runner(),
                    &policy,
                    &[attempt("a", 1, "d"), attempt("b", 1, "d")]
                )
                .is_err()
            );
        }
        assert!(policy().validate().is_ok());
    }

    #[test]
    fn drift_names_changed_fields_and_ignores_equal_records() {
        let before = runner();
        assert_eq!(
            drift(&before, &before),
            Drift {
                drifted: false,
                changed: vec![]
            }
        );
        let mut after = before.clone();
        after.endpoint = "http://127.0.0.1:65501/v1".into();
        after.identity.quantization = Some("Q5_K_M".into());
        after.identity.cache = None;
        let result = drift(&before, &after);
        assert!(result.drifted);
        assert_eq!(
            result.changed,
            vec!["endpoint", "identity.quantization", "identity.cache"]
        );
        let mut renamed = before.clone();
        renamed.model = "fixture-model-y".into();
        assert_eq!(drift(&before, &renamed).changed, vec!["model"]);
    }

    #[test]
    fn recorded_runner_round_trips_and_extraction_stays_conservative() {
        let record = RunnerRecord::new(&runner());
        assert_eq!(record.kind, "local");
        assert_eq!(record.wire_api, WIRE_API);
        assert!(record.identity_missing.is_empty());
        assert_eq!(record.runner(), runner());

        let full = json!({"thread_id":"t-1","status":"completed",
            "observed_model_metadata_verified":true,"tool_operations":3,
            "runner":serde_json::to_value(&record).unwrap()});
        let attempt = qualification_attempt(&full, BTreeMap::new()).unwrap();
        assert_eq!(attempt.attempt_id, "t-1");
        assert!(attempt.completed);
        assert!(attempt.model_metadata_verified);
        assert_eq!(attempt.tool_operations, 3);
        assert_eq!(attempt.runner.as_ref(), Some(&record));

        let bare = json!({"thread_id":"t-2"});
        let attempt = qualification_attempt(&bare, BTreeMap::new()).unwrap();
        assert!(!attempt.completed);
        assert!(!attempt.model_metadata_verified);
        assert_eq!(attempt.tool_operations, 0);
        assert!(attempt.runner.is_none());
        assert!(qualification_attempt(&json!({}), BTreeMap::new()).is_err());
        assert!(
            qualification_attempt(&json!({"thread_id":"t-3","runner":42}), BTreeMap::new())
                .is_err()
        );
    }
}
