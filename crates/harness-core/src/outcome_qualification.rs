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
//!
//! The selected API-observed identity policy qualifies an explicit local
//! runner through declared observable facts instead of unavailable full
//! material identity: bounded declared JSON fields are collected from explicit
//! local API sources, explicit effective client inputs are observed by digest,
//! and the collected set is bound to the qualification and re-checked before
//! dependent attempts. Unobservable weight hashes and hardware stay disclosed
//! limits of that policy; a required observation that fails, disappears or
//! changes suspends dependent comparisons instead of being dropped. The
//! full-material policy above keeps its existing gates and is never silently
//! replaced.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    time::Duration,
};

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
    validate_attempts(attempts)?;
    let (missing_outputs, divergent_outputs) = output_states(policy, attempts);
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
        missing_outputs,
        divergent_outputs,
        observed_repeats: attempts.len(),
        required_repeats: policy.repeats,
    })
}

/// Structural validation shared by both identity policies: attempt identities
/// and output keys/digests must be bounded and unique.
fn validate_attempts(attempts: &[QualificationAttempt]) -> std::io::Result<()> {
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
    Ok(())
}

/// Required outputs absent from at least one attempt and required outputs that
/// differed across attempts, each sorted by required-output name.
fn output_states(
    policy: &RepeatabilityPolicy,
    attempts: &[QualificationAttempt],
) -> (Vec<String>, Vec<String>) {
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
    (
        missing_outputs.into_iter().collect(),
        divergent_outputs.into_iter().collect(),
    )
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

// ---------------------------------------------------------------------------
// Declared API-observed identity policy
// ---------------------------------------------------------------------------

/// Identity policy a qualification was evaluated under. The mode is part of
/// the record: a full-material qualification and an API-observed qualification
/// are never interchangeable, and neither policy may change silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualificationMode {
    /// Every full-material identity fact is required (the stronger policy).
    FullMaterial,
    /// Declared API-observable facts plus effective client configuration.
    ApiObserved,
}

/// Facts API-observed identity cannot certify. They stay disclosed limits of
/// the selected policy instead of being invented or presented as verified
/// full-material identity.
pub const API_OBSERVED_LIMITS: [&str; 2] = [
    "unobservable weight or quantization artifacts are not certified by API-observed identity",
    "hardware identity and host state are not certified by API-observed identity",
];

/// Always-required identity facts. They are recorded separately from declared
/// observation fields and cannot be redeclared as one.
const API_OBSERVED_IDENTITY: [&str; 2] = ["endpoint", "model"];

/// One declared observable fact extracted from a local API document.
///
/// The declaration is fixed before any observation. `pointer` is a bounded
/// RFC 6901 JSON pointer into the observed document; `required` decides
/// whether absence or unreadability of the fact blocks the policy (required)
/// or stays a disclosed optional limit (not required).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredObservation {
    pub name: String,
    pub pointer: String,
    pub required: bool,
    /// How the observed fact is retained and compared.
    #[serde(default)]
    pub binding: ObservationBinding,
}

/// How one declared fact is retained once observed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationBinding {
    /// Retain the bounded scalar value itself (the default).
    #[default]
    Value,
    /// Retain a sha256 digest of the observed JSON value with its provenance:
    /// large or structured facts such as prompt templates and capability maps
    /// stay comparable without their full bodies entering the record.
    Digest,
}

/// One declared local API observation source.
///
/// `path` is an absolute path on the explicit endpoint's origin (`/props` for
/// a base URL `http://127.0.0.1:8080/v1`); collection performs one bounded
/// `GET` against that origin per declared source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationRequest {
    pub path: String,
    pub fields: Vec<DeclaredObservation>,
}

/// The declared observation plan: the observable server sources and the
/// effective client inputs whose explicit files are observed by digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiObservationPlan {
    pub requests: Vec<ObservationRequest>,
    /// Declared names of the effective client inputs (profile, catalogue)
    /// that must be supplied and observed; the exact paths stay private run
    /// inputs and are never part of a public error.
    pub required_client_inputs: Vec<String>,
}

/// The selected API-observed identity policy.
///
/// Fixed before qualification: the declared observable facts with their
/// requiredness and provenance rules plus the output-equality rule. The policy
/// itself is part of the qualification identity; it is bound to the collected
/// observations and can never be silently relaxed or replaced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiObservedPolicy {
    /// The output-equality rule fixed before repeated observations.
    pub output: RepeatabilityPolicy,
    pub plan: ApiObservationPlan,
}

impl ApiObservedPolicy {
    /// Checks the declared policy; a contradiction is a caller error, not a
    /// blocked qualification.
    pub fn validate(&self) -> Result<(), &'static str> {
        self.output.validate()?;
        self.plan.validate()
    }

    /// Canonical digest of the declared policy. It binds the qualification to
    /// this exact declaration: a changed, dropped or relaxed declaration
    /// refuses reuse.
    pub fn digest(&self) -> std::io::Result<String> {
        canonical_digest(self)
    }

    /// Declared fact names with their requiredness, including the required
    /// effective client inputs.
    fn declared_fields(&self) -> BTreeMap<&str, bool> {
        let mut fields = BTreeMap::new();
        for request in &self.plan.requests {
            for field in &request.fields {
                fields.insert(field.name.as_str(), field.required);
            }
        }
        for name in &self.plan.required_client_inputs {
            fields.insert(name.as_str(), true);
        }
        fields
    }

    /// Declared server fact names with their source path and requiredness.
    fn declared_sources(&self) -> BTreeMap<&str, (&str, bool)> {
        let mut sources = BTreeMap::new();
        for request in &self.plan.requests {
            for field in &request.fields {
                sources.insert(field.name.as_str(), (request.path.as_str(), field.required));
            }
        }
        sources
    }
}

impl ApiObservationPlan {
    /// Checks the declared plan; a contradiction is a caller error. Endpoint
    /// and model identity plus at least one effective client input are always
    /// required, and a declared optional fact only narrows what may be absent.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.requests.is_empty() {
            return Err("API-observed policy requires at least one observation source");
        }
        let mut names = BTreeSet::new();
        for request in &self.requests {
            if !observation_path_is_bounded(&request.path) {
                return Err("observation path must be an absolute bounded origin-relative path");
            }
            if request.fields.is_empty() {
                return Err("an observation source must declare at least one field");
            }
            for field in &request.fields {
                if !name_is_bounded(&field.name) {
                    return Err(
                        "observed field name is empty, oversized or contains control characters",
                    );
                }
                if API_OBSERVED_IDENTITY.contains(&field.name.as_str()) {
                    return Err(
                        "endpoint and model are always-required identity facts, not declared fields",
                    );
                }
                if !pointer_is_bounded(&field.pointer) {
                    return Err("observed field pointer is not a bounded JSON pointer");
                }
                if !names.insert(field.name.as_str()) {
                    return Err("observed field names must be unique across the declared plan");
                }
            }
        }
        if self.required_client_inputs.is_empty() {
            return Err("API-observed policy requires effective client configuration inputs");
        }
        let mut inputs = BTreeSet::new();
        for name in &self.required_client_inputs {
            if !name_is_bounded(name) || API_OBSERVED_IDENTITY.contains(&name.as_str()) {
                return Err("client input name is invalid or repeats an identity fact");
            }
            if !inputs.insert(name.as_str()) {
                return Err("required client input names must be unique");
            }
            if names.contains(name.as_str()) {
                return Err("a client input name cannot repeat an observed field name");
            }
        }
        Ok(())
    }

    /// Canonical digest of the declared plan. Observations bind to it, so a
    /// changed, dropped or relaxed declaration refuses reuse.
    pub fn digest(&self) -> std::io::Result<String> {
        canonical_digest(self)
    }
}

/// One explicit effective-client input: `name` is the declared policy name,
/// `path` is a private run input observed by digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientInput {
    pub name: String,
    pub path: PathBuf,
}

/// One bounded observed scalar value with its declared provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedFact {
    pub value: String,
    pub provenance: String,
}

/// Collected API-observed identity, bound to one declared plan and runner.
///
/// A required fact that could not be observed is a failure of collection, not
/// a record; only declared optional facts may be absent here, and they stay
/// disclosed limits (`unknown_optional`) instead of guessed values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiObservations {
    /// Digest of the declared plan these observations were collected under.
    pub plan_digest: String,
    /// The explicit local runner the observations were collected against.
    pub endpoint: String,
    pub model: String,
    pub fields: BTreeMap<String, ObservedFact>,
    /// Declared optional facts that were absent or unreadable, with a bounded
    /// reason. A missing optional fact is a disclosed limit, not a failure.
    pub unknown_optional: BTreeMap<String, String>,
    /// Full-material facts the declared configuration does not provide. They
    /// are disclosed limits under this policy instead of blockers.
    pub missing_identity: Vec<String>,
}

impl ApiObservations {
    /// Canonical digest of the observed identity; part of the qualification
    /// identity and of every pre-arm comparison.
    pub fn digest(&self) -> std::io::Result<String> {
        canonical_digest(self)
    }
}

/// The distinguishable causes of an unavailable required observation. Fetch,
/// read, status and parse failures are separate from a valid absent optional
/// field, which is never a failure of collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObservationFailureKind {
    /// The declared plan, client inputs or runner configuration are invalid.
    Declaration,
    /// The local API could not be reached or read (fetch failure).
    Transport,
    /// The local API answered with a non-success status.
    Status,
    /// The response or document could not be parsed within its bounds.
    Parse,
    /// A required declared field is absent from a readable document.
    Missing,
    /// A required declared field is present but is not a bounded scalar.
    Unreadable,
    /// A declared effective client input is absent or unreadable.
    ClientInput,
}

/// One failed explicit local API observation. `source` names the declared
/// source (path or client input name) and `detail` carries a bounded machine
/// description: never response bodies, observed values or credentials.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationFailure {
    pub kind: ObservationFailureKind,
    pub source: String,
    pub detail: String,
}

impl ObservationFailure {
    fn new(
        kind: ObservationFailureKind,
        source: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            source: source.into(),
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for ObservationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "local API observation {:?} at {}: {}",
            self.kind, self.source, self.detail
        )
    }
}

/// Bounds that keep observed facts inspectable and free of full private bodies.
pub const OBSERVED_VALUE_LIMIT: usize = 512;
const OBSERVATION_BODY_LIMIT: usize = 256 * 1024;
const OBSERVATION_HEAD_LIMIT: usize = 64 * 1024;
const CLIENT_INPUT_LIMIT: u64 = 64 * 1024 * 1024;
const OBSERVATION_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const OBSERVATION_IO_TIMEOUT: Duration = Duration::from_secs(15);

/// Collects the declared observations from the explicit local runner.
///
/// The transport performs one bounded `GET` per declared source against the
/// endpoint's origin through the standard library; effective client inputs
/// are hashed from their explicit local files (owned regular files only). A
/// required observation that cannot be fetched, read, parsed, or that is
/// absent or unreadable returns a distinguishable [`ObservationFailure`]. A
/// declared optional field that is absent stays a disclosed limit in the
/// returned observations instead of failing collection, and a fact declared
/// with [`ObservationBinding::Digest`] is retained as a content digest with
/// its provenance instead of its full body.
pub fn collect_observations(
    runner: &LocalRunner,
    plan: &ApiObservationPlan,
    client_inputs: &[ClientInput],
) -> Result<ApiObservations, ObservationFailure> {
    plan.validate().map_err(|message| {
        ObservationFailure::new(ObservationFailureKind::Declaration, "policy", message)
    })?;
    let plan_digest = plan.digest().map_err(|_| {
        ObservationFailure::new(
            ObservationFailureKind::Declaration,
            "policy",
            "the declared policy cannot be encoded",
        )
    })?;
    let origin = observation_origin(&runner.endpoint)?;
    let mut supplied: BTreeMap<&str, &Path> = BTreeMap::new();
    for input in client_inputs {
        if !name_is_bounded(&input.name)
            || supplied.insert(input.name.as_str(), &input.path).is_some()
        {
            return Err(ObservationFailure::new(
                ObservationFailureKind::ClientInput,
                "client input",
                "an input name is invalid or duplicated",
            ));
        }
    }
    let mut fields = BTreeMap::new();
    let mut unknown_optional = BTreeMap::new();
    for name in &plan.required_client_inputs {
        let path = supplied.get(name.as_str()).ok_or_else(|| {
            ObservationFailure::new(
                ObservationFailureKind::ClientInput,
                name.as_str(),
                "declared client input was not supplied",
            )
        })?;
        let value = client_input_digest(path, name)?;
        let provenance = format!("sha256 {}", path.to_string_lossy());
        if provenance.len() > OBSERVED_VALUE_LIMIT {
            return Err(ObservationFailure::new(
                ObservationFailureKind::ClientInput,
                name.as_str(),
                "input path exceeds the reporting bound",
            ));
        }
        fields.insert(name.clone(), ObservedFact { value, provenance });
    }
    for request in &plan.requests {
        let document = observation_document(&origin, &request.path)?;
        for field in &request.fields {
            match pointer_value(&document, &field.pointer) {
                None | Some(Value::Null) => {
                    if field.required {
                        return Err(ObservationFailure::new(
                            ObservationFailureKind::Missing,
                            &request.path,
                            format!("required field '{}' is absent", field.name),
                        ));
                    }
                    unknown_optional.insert(field.name.clone(), "not reported".to_owned());
                }
                Some(value) => {
                    let observed = match field.binding {
                        ObservationBinding::Value => observed_scalar(value),
                        ObservationBinding::Digest => digest_value(value),
                    };
                    match observed {
                        Some(value) => {
                            let provenance = match field.binding {
                                ObservationBinding::Value => format!("GET {}", request.path),
                                ObservationBinding::Digest => {
                                    format!("sha256 GET {} {}", request.path, field.pointer)
                                }
                            };
                            fields.insert(field.name.clone(), ObservedFact { value, provenance });
                        }
                        None => {
                            if field.required {
                                return Err(ObservationFailure::new(
                                    ObservationFailureKind::Unreadable,
                                    &request.path,
                                    format!(
                                        "required field '{}' is not a bounded scalar",
                                        field.name
                                    ),
                                ));
                            }
                            unknown_optional
                                .insert(field.name.clone(), "not a bounded scalar".to_owned());
                        }
                    }
                }
            }
        }
    }
    Ok(ApiObservations {
        plan_digest,
        endpoint: runner.endpoint.clone(),
        model: runner.model.clone(),
        fields,
        unknown_optional,
        missing_identity: runner.missing_identity(),
    })
}

/// The explicit local endpoint's origin (`scheme://authority`). Declared
/// observation paths are absolute on that origin. Only the plain HTTP
/// transport is available for observations; an endpoint that cannot use it is
/// a disclosed transport failure, not a silent skip.
fn observation_origin(endpoint: &str) -> Result<String, ObservationFailure> {
    let rest = endpoint.strip_prefix("http://").ok_or_else(|| {
        if endpoint.starts_with("https://") {
            ObservationFailure::new(
                ObservationFailureKind::Transport,
                "endpoint",
                "https observation transport is unsupported",
            )
        } else {
            ObservationFailure::new(
                ObservationFailureKind::Declaration,
                "endpoint",
                "not an explicit http origin",
            )
        }
    })?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || rest.contains(['?', '#']) {
        return Err(ObservationFailure::new(
            ObservationFailureKind::Declaration,
            "endpoint",
            "invalid explicit local endpoint",
        ));
    }
    Ok(format!("http://{authority}"))
}

fn observation_document(origin: &str, path: &str) -> Result<Value, ObservationFailure> {
    let body = observation_get(origin, path)?;
    serde_json::from_slice(&body).map_err(|_| {
        ObservationFailure::new(
            ObservationFailureKind::Parse,
            path,
            "response body is not valid JSON",
        )
    })
}

/// One bounded plain-HTTP GET. The whole response is read under a size bound
/// with explicit timeouts; content-length and chunked framings are decoded,
/// and everything else is a visible parse failure.
fn observation_get(origin: &str, path: &str) -> Result<Vec<u8>, ObservationFailure> {
    let transport =
        |detail: &str| ObservationFailure::new(ObservationFailureKind::Transport, path, detail);
    let authority = origin.strip_prefix("http://").unwrap_or(origin);
    let (host, port) = split_authority(authority).map_err(transport)?;
    let addresses: Vec<SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| transport("endpoint could not be resolved"))?
        .take(4)
        .collect();
    let mut stream = addresses
        .iter()
        .find_map(|address| TcpStream::connect_timeout(address, OBSERVATION_CONNECT_TIMEOUT).ok())
        .ok_or_else(|| transport("endpoint could not be reached"))?;
    stream
        .set_read_timeout(Some(OBSERVATION_IO_TIMEOUT))
        .map_err(|_| transport("endpoint read timeout could not be set"))?;
    stream
        .set_write_timeout(Some(OBSERVATION_IO_TIMEOUT))
        .map_err(|_| transport("endpoint write timeout could not be set"))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    )
    .map_err(|_| transport("observation request could not be sent"))?;
    let mut response = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                response.extend_from_slice(&buffer[..count]);
                if response.len() > OBSERVATION_BODY_LIMIT + OBSERVATION_HEAD_LIMIT {
                    return Err(ObservationFailure::new(
                        ObservationFailureKind::Parse,
                        path,
                        "response exceeds the size bound",
                    ));
                }
            }
            Err(_) => return Err(transport("observation response could not be read")),
        }
    }
    let head_end = find(&response, b"\r\n\r\n").ok_or_else(|| {
        ObservationFailure::new(
            ObservationFailureKind::Parse,
            path,
            "response head is malformed",
        )
    })?;
    let head = std::str::from_utf8(&response[..head_end]).map_err(|_| {
        ObservationFailure::new(
            ObservationFailureKind::Parse,
            path,
            "response head is malformed",
        )
    })?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            ObservationFailure::new(
                ObservationFailureKind::Parse,
                path,
                "response status line is malformed",
            )
        })?;
    if !(200..=299).contains(&status) {
        return Err(ObservationFailure::new(
            ObservationFailureKind::Status,
            path,
            format!("HTTP {status}"),
        ));
    }
    let mut content_length = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<usize>().ok();
        }
        if name.trim().eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
    }
    let body = &response[head_end + 4..];
    if chunked {
        return decode_chunked(body, path);
    }
    match content_length {
        Some(length) => {
            if length > OBSERVATION_BODY_LIMIT {
                return Err(ObservationFailure::new(
                    ObservationFailureKind::Parse,
                    path,
                    "response exceeds the size bound",
                ));
            }
            if body.len() < length {
                return Err(ObservationFailure::new(
                    ObservationFailureKind::Parse,
                    path,
                    "response body is truncated",
                ));
            }
            Ok(body[..length].to_vec())
        }
        None => {
            if body.len() > OBSERVATION_BODY_LIMIT {
                return Err(ObservationFailure::new(
                    ObservationFailureKind::Parse,
                    path,
                    "response exceeds the size bound",
                ));
            }
            Ok(body.to_vec())
        }
    }
}

fn split_authority(authority: &str) -> Result<(String, u16), &'static str> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or("endpoint authority is malformed")?;
        let port = match tail.strip_prefix(':') {
            Some(port) => port
                .parse::<u16>()
                .map_err(|_| "endpoint port is invalid")?,
            None if tail.is_empty() => 80,
            None => return Err("endpoint authority is malformed"),
        };
        return Ok((host.to_owned(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Ok((
            host.to_owned(),
            port.parse::<u16>()
                .map_err(|_| "endpoint port is invalid")?,
        )),
        None if !authority.is_empty() => Ok((authority.to_owned(), 80)),
        None => Err("endpoint authority is empty"),
    }
}

fn decode_chunked(mut data: &[u8], path: &str) -> Result<Vec<u8>, ObservationFailure> {
    let malformed = || {
        ObservationFailure::new(
            ObservationFailureKind::Parse,
            path,
            "chunked response is malformed",
        )
    };
    let mut body = Vec::new();
    loop {
        let line_end = find(data, b"\r\n").ok_or_else(malformed)?;
        let size = std::str::from_utf8(&data[..line_end]).map_err(|_| malformed())?;
        let size = usize::from_str_radix(size.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| malformed())?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(body);
        }
        if size > OBSERVATION_BODY_LIMIT || body.len() + size > OBSERVATION_BODY_LIMIT {
            return Err(ObservationFailure::new(
                ObservationFailureKind::Parse,
                path,
                "response exceeds the size bound",
            ));
        }
        if data.len() < size + 2 || &data[size..size + 2] != b"\r\n" {
            return Err(malformed());
        }
        body.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Resolves a bounded RFC 6901 JSON pointer against an observed document.
fn pointer_value<'a>(document: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer.is_empty() {
        return Some(document);
    }
    let mut current = document;
    for token in pointer.split('/').skip(1) {
        let token = token.replace("~1", "/").replace("~0", "~");
        current = match current {
            Value::Object(map) => map.get(&token)?,
            Value::Array(items) => items.get(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

/// A declared fact is observed only as a bounded scalar; objects, arrays,
/// `null`, blank text and oversized values stay absent or unreadable.
fn observed_scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => scalar_is_bounded(text).then_some(text.clone()),
        Value::Number(number) => {
            let text = number.to_string();
            scalar_is_bounded(&text).then_some(text)
        }
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// Bounded, control-free scalar text for one retained observed value.
fn scalar_is_bounded(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= OBSERVED_VALUE_LIMIT
        && !value.chars().any(char::is_control)
}

/// Binds any observed JSON value by content digest so large or structured
/// facts stay comparable without retaining their full bodies.
fn digest_value(value: &Value) -> Option<String> {
    serde_json::to_vec(value)
        .ok()
        .map(|bytes| crate::build_identity::hash_bytes(&bytes))
}

/// Hashes one explicit effective-client input from its owned regular file.
fn client_input_digest(path: &Path, name: &str) -> Result<String, ObservationFailure> {
    let input = |detail: &'static str| {
        ObservationFailure::new(ObservationFailureKind::ClientInput, name, detail)
    };
    crate::build_identity::ordinary(path)
        .map_err(|_| input("input is not an owned regular file"))?;
    let metadata = std::fs::metadata(path).map_err(|_| input("input is unreadable"))?;
    if !metadata.is_file() {
        return Err(input("input is not a regular file"));
    }
    if metadata.len() > CLIENT_INPUT_LIMIT {
        return Err(input("input exceeds the size bound"));
    }
    crate::build_identity::hash_file(path).map_err(|_| input("input is unreadable"))
}

fn observation_path_is_bounded(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && path.starts_with('/')
        && path
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'?' && byte != b'#')
}

fn pointer_is_bounded(pointer: &str) -> bool {
    if pointer.len() > 512 || pointer.chars().any(char::is_control) {
        return false;
    }
    if pointer.is_empty() {
        return true;
    }
    if !pointer.starts_with('/') {
        return false;
    }
    let bytes = pointer.as_bytes();
    bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| *byte != b'~' || matches!(bytes.get(index + 1), Some(b'0' | b'1')))
}

fn canonical_digest<T: Serialize>(value: &T) -> std::io::Result<String> {
    Ok(crate::build_identity::hash_bytes(&serde_json::to_vec(
        value,
    )?))
}

/// Result of one API-observed qualification. The record binds the declared
/// policy and its digest, the declared runner, the observed identity and the
/// per-attempt evidence; a consumer can never reuse it against a different
/// policy, runner or observation set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiObservedQualification {
    /// Identity policy this record was evaluated under.
    pub mode: QualificationMode,
    pub status: QualificationStatus,
    /// The declared policy this result was evaluated under.
    pub policy: ApiObservedPolicy,
    /// Digest of the declared policy (see [`ApiObservedPolicy::digest`]).
    pub policy_digest: String,
    /// The declared local runner at qualification time.
    pub runner: LocalRunner,
    /// The observed identity the attempts were qualified against.
    pub observations: ApiObservations,
    /// Digest of the observed identity (see [`ApiObservations::digest`]).
    pub observation_digest: String,
    /// Full-material facts the declaration does not provide; disclosed limits
    /// under this policy, never blockers.
    pub missing_identity: Vec<String>,
    /// Facts API-observed identity cannot certify (weights/artifacts, hardware).
    pub limits: Vec<String>,
    /// Required declared observations absent from the bound observed identity.
    pub missing_observations: Vec<String>,
    /// Attempts that did not complete successfully through the real entry point.
    pub unfinished_attempts: Vec<String>,
    /// Attempts whose observed model metadata was not verified.
    pub unverified_attempts: Vec<String>,
    /// Attempts with no observed tool round-trip.
    pub tool_exchange_missing: Vec<String>,
    /// Attempts whose recorded runner identity was absent or contradicted the
    /// declared configuration.
    pub runner_mismatch: Vec<AttemptDrift>,
    /// Required outputs absent from at least one repeat.
    pub missing_outputs: Vec<String>,
    /// Required outputs that differed across repeats.
    pub divergent_outputs: Vec<String>,
    pub observed_repeats: usize,
    pub required_repeats: usize,
}

impl ApiObservedQualification {
    pub fn qualified(&self) -> bool {
        self.status == QualificationStatus::Qualified
    }

    /// Dependent strict comparisons must not run while a qualification blocks.
    pub fn blocks_comparisons(&self) -> bool {
        !self.qualified()
    }
}

/// Evaluates the declared API-observed policy over controlled attempts.
///
/// The bound observations must belong to this declared policy and runner;
/// endpoint/model identity is always required, the effective client inputs
/// are required observations, and a required declared fact that is missing
/// from the bound set blocks instead of being dropped. Full-material facts the
/// declaration leaves unknown stay disclosed limits. Attempt evidence is
/// consumed under the same completion, verification, tool-round-trip and
/// output-equality gates as the stronger policy; a recorded attempt runner
/// record must confirm every material fact the declaration provides.
pub fn qualify_api_observed(
    runner: &LocalRunner,
    policy: &ApiObservedPolicy,
    observations: &ApiObservations,
    attempts: &[QualificationAttempt],
) -> std::io::Result<ApiObservedQualification> {
    policy.validate().map_err(invalid)?;
    let policy_digest = policy.digest()?;
    if observations.plan_digest != policy.plan.digest()?
        || observations.endpoint != runner.endpoint
        || observations.model != runner.model
    {
        return Err(invalid(
            "the observations do not belong to the declared policy and runner",
        ));
    }
    validate_attempts(attempts)?;
    let (missing_outputs, divergent_outputs) = output_states(&policy.output, attempts);
    let missing_observations: Vec<String> = policy
        .declared_fields()
        .into_iter()
        .filter(|(name, required)| {
            !observations.fields.contains_key(*name)
                && !(!required && observations.unknown_optional.contains_key(*name))
        })
        .map(|(name, _)| name.to_owned())
        .collect();
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
                let observed = declared_record_drift(&expected, record);
                observed.drifted.then_some(AttemptDrift {
                    attempt_id: attempt.attempt_id.clone(),
                    changed: observed.changed,
                })
            }
        })
        .collect();
    let blocked = !missing_observations.is_empty()
        || !unfinished_attempts.is_empty()
        || !unverified_attempts.is_empty()
        || !tool_exchange_missing.is_empty()
        || !runner_mismatch.is_empty()
        || !missing_outputs.is_empty()
        || !divergent_outputs.is_empty()
        || attempts.len() != policy.output.repeats;
    Ok(ApiObservedQualification {
        mode: QualificationMode::ApiObserved,
        status: if blocked {
            QualificationStatus::Blocked
        } else {
            QualificationStatus::Qualified
        },
        policy: policy.clone(),
        policy_digest,
        runner: runner.clone(),
        observations: observations.clone(),
        observation_digest: observations.digest()?,
        missing_identity: runner.missing_identity(),
        limits: API_OBSERVED_LIMITS
            .iter()
            .map(|line| (*line).to_owned())
            .collect(),
        missing_observations,
        unfinished_attempts,
        unverified_attempts,
        tool_exchange_missing,
        runner_mismatch,
        missing_outputs,
        divergent_outputs,
        observed_repeats: attempts.len(),
        required_repeats: policy.output.repeats,
    })
}

/// Compares an expected configuration against a runner identity recorded in
/// one attempt under the API-observed policy: kind, endpoint, model and wire
/// protocol must match, and every material fact the declared configuration
/// provides must be confirmed. Facts the declaration leaves unknown are not
/// required, and an attempt that records more material facts than declared
/// does not contradict the declaration.
fn declared_record_drift(expected: &RunnerRecord, observed: &RunnerRecord) -> Drift {
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
        let expected = identity_field(&expected.identity, name);
        let observed = identity_field(&observed.identity, name);
        if let Some(expected) = expected
            && observed.as_deref() != Some(expected.as_str())
        {
            changed.push(format!("identity.{name}"));
        }
    }
    Drift {
        drifted: !changed.is_empty(),
        changed,
    }
}

/// Outcome of a pre-arm reference comparison against retained observations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiObservedDrift {
    pub drifted: bool,
    /// Changed or unavailable binding facts, sorted names only (no values).
    pub changed: Vec<String>,
    /// The fresh collection failure when the recheck could not observe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<ObservationFailure>,
}

impl ApiObservedDrift {
    /// Dependent comparisons must not run while this check drifts or fails.
    pub fn blocks_comparisons(&self) -> bool {
        self.drifted
    }
}

/// Compares a retained qualification against the current declared runner, the
/// current policy and freshly observed identity. Any changed, dropped or
/// unavailable fact refuses dependent comparisons: the policy is bound to the
/// qualification and can never be silently downgraded or dropped.
pub fn observed_drift(
    qualification: &ApiObservedQualification,
    runner: &LocalRunner,
    policy: &ApiObservedPolicy,
    observations: &ApiObservations,
) -> ApiObservedDrift {
    let mut changed = Vec::new();
    if qualification.blocks_comparisons() {
        changed.push("qualification.blocked".to_owned());
    }
    let mut policy_changed = Vec::new();
    policy_differences(&qualification.policy, policy, &mut policy_changed);
    if policy_changed.is_empty()
        && let Ok(digest) = policy.digest()
        && digest != qualification.policy_digest
    {
        policy_changed.push("policy".to_owned());
    }
    changed.extend(policy_changed);
    changed.extend(drift(&qualification.runner, runner).changed);
    changed.extend(observation_changes(
        &qualification.observations,
        observations,
    ));
    changed.sort();
    changed.dedup();
    ApiObservedDrift {
        drifted: !changed.is_empty(),
        changed,
        failure: None,
    }
}

/// Names of the observed facts that changed between two collections,
/// including facts that appeared, disappeared or became unavailable. Values
/// never enter the names.
pub fn observation_changes(before: &ApiObservations, after: &ApiObservations) -> Vec<String> {
    let mut changed = Vec::new();
    if before.plan_digest != after.plan_digest {
        changed.push("observations.plan".to_owned());
    }
    if before.endpoint != after.endpoint {
        changed.push("observations.endpoint".to_owned());
    }
    if before.model != after.model {
        changed.push("observations.model".to_owned());
    }
    for (name, fact) in &before.fields {
        match after.fields.get(name) {
            Some(after) if after == fact => {}
            _ => changed.push(format!("observed.{name}")),
        }
    }
    for name in after.fields.keys() {
        if !before.fields.contains_key(name) {
            changed.push(format!("observed.{name}"));
        }
    }
    for (name, reason) in &before.unknown_optional {
        match after.unknown_optional.get(name) {
            Some(after) if after == reason => {}
            _ => changed.push(format!("optional.{name}")),
        }
    }
    for name in after.unknown_optional.keys() {
        if !before.unknown_optional.contains_key(name) {
            changed.push(format!("optional.{name}"));
        }
    }
    if before.missing_identity != after.missing_identity {
        changed.push("missing_identity".to_owned());
    }
    changed.sort();
    changed.dedup();
    changed
}

/// Re-collects the declared observations and compares them with the retained
/// qualification. A collection failure is itself a refusal with the
/// distinguishable cause retained.
pub fn recheck_observations(
    qualification: &ApiObservedQualification,
    runner: &LocalRunner,
    policy: &ApiObservedPolicy,
    client_inputs: &[ClientInput],
) -> ApiObservedDrift {
    match collect_observations(runner, &policy.plan, client_inputs) {
        Ok(observations) => observed_drift(qualification, runner, policy, &observations),
        Err(failure) => ApiObservedDrift {
            drifted: true,
            changed: vec![failure.to_string()],
            failure: Some(failure),
        },
    }
}

fn policy_differences(
    before: &ApiObservedPolicy,
    after: &ApiObservedPolicy,
    changed: &mut Vec<String>,
) {
    if before.output.repeats != after.output.repeats {
        changed.push("output.repeats".to_owned());
    }
    if before.output.required_outputs != after.output.required_outputs {
        changed.push("output.required_outputs".to_owned());
    }
    if before.output.ignored_metadata != after.output.ignored_metadata {
        changed.push("output.ignored_metadata".to_owned());
    }
    if before.plan.required_client_inputs != after.plan.required_client_inputs {
        changed.push("required_client_inputs".to_owned());
    }
    let before_sources = before.declared_sources();
    let after_sources = after.declared_sources();
    for name in before_sources.keys().chain(after_sources.keys()) {
        if before_sources.get(name) != after_sources.get(name) {
            changed.push(format!("declared.{name}"));
        }
    }
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

    fn api_policy() -> ApiObservedPolicy {
        ApiObservedPolicy {
            output: policy(),
            plan: ApiObservationPlan {
                requests: vec![ObservationRequest {
                    path: "/props".to_owned(),
                    fields: vec![
                        DeclaredObservation {
                            name: "server.build".to_owned(),
                            pointer: "/build".to_owned(),
                            required: true,
                            binding: ObservationBinding::Value,
                        },
                        DeclaredObservation {
                            name: "server.context".to_owned(),
                            pointer: "/settings/n_ctx".to_owned(),
                            required: true,
                            binding: ObservationBinding::Value,
                        },
                        DeclaredObservation {
                            name: "limits.weights".to_owned(),
                            pointer: "/weights".to_owned(),
                            required: false,
                            binding: ObservationBinding::Value,
                        },
                        DeclaredObservation {
                            name: "server.template".to_owned(),
                            pointer: "/template".to_owned(),
                            required: true,
                            binding: ObservationBinding::Digest,
                        },
                    ],
                }],
                required_client_inputs: vec!["profile".to_owned(), "catalogue".to_owned()],
            },
        }
    }

    fn api_observations(policy: &ApiObservedPolicy) -> ApiObservations {
        ApiObservations {
            plan_digest: policy.plan.digest().unwrap(),
            endpoint: runner().endpoint.clone(),
            model: runner().model.clone(),
            fields: BTreeMap::from([
                (
                    "server.build".to_owned(),
                    ObservedFact {
                        value: "b-synth".to_owned(),
                        provenance: "GET /props".to_owned(),
                    },
                ),
                (
                    "server.context".to_owned(),
                    ObservedFact {
                        value: "262144".to_owned(),
                        provenance: "GET /props".to_owned(),
                    },
                ),
                (
                    "profile".to_owned(),
                    ObservedFact {
                        value: "a".repeat(64),
                        provenance: "sha256 profile.toml".to_owned(),
                    },
                ),
                (
                    "catalogue".to_owned(),
                    ObservedFact {
                        value: "b".repeat(64),
                        provenance: "sha256 catalogue.json".to_owned(),
                    },
                ),
                (
                    "server.template".to_owned(),
                    ObservedFact {
                        value: "d".repeat(64),
                        provenance: "sha256 GET /props /template".to_owned(),
                    },
                ),
            ]),
            unknown_optional: BTreeMap::from([(
                "limits.weights".to_owned(),
                "not reported".to_owned(),
            )]),
            missing_identity: Vec::new(),
        }
    }

    #[test]
    fn api_observed_policy_requires_declared_sources_and_client_configuration() {
        assert!(api_policy().validate().is_ok());

        let mut empty_sources = api_policy();
        empty_sources.plan.requests.clear();
        assert!(empty_sources.validate().is_err());

        let mut empty_fields = api_policy();
        empty_fields.plan.requests[0].fields.clear();
        assert!(empty_fields.validate().is_err());

        let mut duplicate = api_policy();
        duplicate.plan.requests[0].fields[1].name = "server.build".to_owned();
        assert!(duplicate.validate().is_err());

        let mut reserved = api_policy();
        reserved.plan.requests[0].fields[0].name = "endpoint".to_owned();
        assert!(reserved.validate().is_err());

        let mut pointer = api_policy();
        pointer.plan.requests[0].fields[0].pointer = "build".to_owned();
        assert!(pointer.validate().is_err());
        pointer.plan.requests[0].fields[0].pointer = "/a~2".to_owned();
        assert!(pointer.validate().is_err());

        let mut path = api_policy();
        path.plan.requests[0].path = "props".to_owned();
        assert!(path.validate().is_err());
        path.plan.requests[0].path = "/props?token=private".to_owned();
        assert!(path.validate().is_err());
        path.plan.requests[0].path = "https://127.0.0.1/props".to_owned();
        assert!(path.validate().is_err());

        let mut inputs = api_policy();
        inputs.plan.required_client_inputs.clear();
        assert!(inputs.validate().is_err());
        inputs.plan.required_client_inputs = vec!["profile".to_owned(), "profile".to_owned()];
        assert!(inputs.validate().is_err());
        inputs.plan.required_client_inputs = vec!["server.build".to_owned()];
        assert!(inputs.validate().is_err());
        inputs.plan.required_client_inputs = vec!["model".to_owned()];
        assert!(inputs.validate().is_err());

        let mut output = api_policy();
        output.output.repeats = 1;
        assert!(output.validate().is_err());
    }

    #[test]
    fn api_observed_qualification_binds_policy_runner_and_observations() {
        let policy = api_policy();
        let observations = api_observations(&policy);
        let attempts = [attempt("a", 2, "d1"), attempt("b", 1, "d1")];
        let result = qualify_api_observed(&runner(), &policy, &observations, &attempts).unwrap();
        assert_eq!(result.mode, QualificationMode::ApiObserved);
        assert!(result.qualified(), "{result:?}");
        assert!(!result.blocks_comparisons());
        assert_eq!(result.policy_digest, policy.digest().unwrap());
        assert_eq!(result.observation_digest, observations.digest().unwrap());
        assert!(result.missing_identity.is_empty());
        assert_eq!(result.limits, API_OBSERVED_LIMITS.to_vec());
        assert!(result.missing_observations.is_empty());
        assert_eq!(result.observed_repeats, 2);
        assert_eq!(result.required_repeats, 2);

        // Unavailable weight hashes and hardware stay disclosed limits under
        // this policy instead of blocking qualification.
        let mut partial = runner();
        partial.identity.weights = None;
        partial.identity.quantization = None;
        let result = qualify_api_observed(&partial, &policy, &observations, &attempts).unwrap();
        assert!(result.qualified(), "{result:?}");
        assert_eq!(result.missing_identity, vec!["weights", "quantization"]);
        assert!(
            result
                .limits
                .iter()
                .any(|line| line.contains("weight or quantization"))
        );

        // Observations collected under another declared plan, endpoint or
        // model refuse reuse instead of silently re-binding.
        let mut other_policy = policy.clone();
        other_policy.plan.requests[0].path = "/other".to_owned();
        assert!(qualify_api_observed(&runner(), &other_policy, &observations, &attempts).is_err());
        let mut other_runner = runner();
        other_runner.endpoint = "http://127.0.0.1:65501/v1".to_owned();
        assert!(qualify_api_observed(&other_runner, &policy, &observations, &attempts).is_err());
        let mut other_runner = runner();
        other_runner.model = "fixture-model-y".to_owned();
        assert!(qualify_api_observed(&other_runner, &policy, &observations, &attempts).is_err());
    }

    #[test]
    fn api_observed_qualification_blocks_attempt_and_output_failures() {
        let policy = api_policy();
        let observations = api_observations(&policy);
        let full = vec![attempt("a", 2, "d1"), attempt("b", 2, "d1")];
        assert!(
            qualify_api_observed(&runner(), &policy, &observations, &full)
                .unwrap()
                .qualified()
        );

        let mut unfinished = full.clone();
        unfinished[1].completed = false;
        let result = qualify_api_observed(&runner(), &policy, &observations, &unfinished).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(result.unfinished_attempts, vec!["b"]);
        assert!(result.divergent_outputs.is_empty());

        let mut unverified = full.clone();
        unverified[1].model_metadata_verified = false;
        let result = qualify_api_observed(&runner(), &policy, &observations, &unverified).unwrap();
        assert_eq!(result.unverified_attempts, vec!["b"]);

        let mut text_only = full.clone();
        text_only[1].tool_operations = 0;
        let result = qualify_api_observed(&runner(), &policy, &observations, &text_only).unwrap();
        assert_eq!(result.tool_exchange_missing, vec!["b"]);

        let mut divergent = full.clone();
        divergent[1]
            .outputs
            .insert("solution.txt".to_owned(), "d2".to_owned());
        let result = qualify_api_observed(&runner(), &policy, &observations, &divergent).unwrap();
        assert_eq!(result.divergent_outputs, vec!["solution.txt"]);

        let mut missing = full.clone();
        missing[1].outputs.clear();
        let result = qualify_api_observed(&runner(), &policy, &observations, &missing).unwrap();
        assert_eq!(result.missing_outputs, vec!["solution.txt"]);

        let result = qualify_api_observed(&runner(), &policy, &observations, &full[..1]).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!((result.observed_repeats, result.required_repeats), (1, 2));

        // A recorded attempt runner that contradicts a declared material fact
        // blocks; a missing attempt record blocks; a record that adds a fact
        // the declaration leaves unknown does not contradict it.
        let mut drifted = full.clone();
        drifted[1].runner.as_mut().unwrap().identity.quantization = Some("Q5_K_M".to_owned());
        let result = qualify_api_observed(&runner(), &policy, &observations, &drifted).unwrap();
        assert_eq!(
            result.runner_mismatch,
            vec![AttemptDrift {
                attempt_id: "b".to_owned(),
                changed: vec!["identity.quantization".to_owned()],
            }]
        );
        let mut unrecorded = full.clone();
        unrecorded[1].runner = None;
        let result = qualify_api_observed(&runner(), &policy, &observations, &unrecorded).unwrap();
        assert_eq!(result.runner_mismatch.len(), 1);
        assert!(result.runner_mismatch[0].changed.is_empty());
        let mut partial = runner();
        partial.identity.quantization = None;
        let mut attempt_with_fact = attempt("a", 2, "d1");
        attempt_with_fact
            .runner
            .as_mut()
            .unwrap()
            .identity
            .quantization = Some("Q4_K_M".to_owned());
        let mut second = attempt_with_fact.clone();
        second.attempt_id = "b".to_owned();
        let result = qualify_api_observed(
            &partial,
            &policy,
            &observations,
            &[attempt_with_fact, second],
        )
        .unwrap();
        assert!(result.runner_mismatch.is_empty(), "{result:?}");
        assert!(result.qualified());
        // The declared fact must still be confirmed by the attempt record.
        let mut partial = runner();
        partial.identity.quantization = Some("Q4_K_M".to_owned());
        let mut facts_missing = attempt("a", 2, "d1");
        facts_missing.runner.as_mut().unwrap().identity.quantization = None;
        let mut second = facts_missing.clone();
        second.attempt_id = "b".to_owned();
        let result =
            qualify_api_observed(&partial, &policy, &observations, &[facts_missing, second])
                .unwrap();
        assert_eq!(result.runner_mismatch.len(), 2);
        assert_eq!(
            result.runner_mismatch[0].changed,
            vec!["identity.quantization"]
        );
    }

    #[test]
    fn api_observed_qualification_refuses_dropped_required_observations() {
        let policy = api_policy();
        let attempts = vec![attempt("a", 2, "d1"), attempt("b", 2, "d1")];

        let mut dropped = api_observations(&policy);
        dropped.fields.remove("server.build");
        dropped
            .unknown_optional
            .insert("server.build".to_owned(), "not reported".to_owned());
        let result = qualify_api_observed(&runner(), &policy, &dropped, &attempts).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(result.missing_observations, vec!["server.build"]);

        let mut optional_dropped = api_observations(&policy);
        optional_dropped.unknown_optional.clear();
        let result =
            qualify_api_observed(&runner(), &policy, &optional_dropped, &attempts).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(result.missing_observations, vec!["limits.weights"]);

        let mut no_client = api_observations(&policy);
        no_client.fields.remove("profile");
        let result = qualify_api_observed(&runner(), &policy, &no_client, &attempts).unwrap();
        assert!(result.blocks_comparisons());
        assert_eq!(result.missing_observations, vec!["profile"]);
    }

    #[test]
    fn api_observed_drift_names_changed_facts_and_refuses_reuse() {
        let policy = api_policy();
        let observations = api_observations(&policy);
        let attempts = vec![attempt("a", 2, "d1"), attempt("b", 2, "d1")];
        let qualification =
            qualify_api_observed(&runner(), &policy, &observations, &attempts).unwrap();
        let clean = observed_drift(&qualification, &runner(), &policy, &observations);
        assert!(!clean.drifted, "{clean:?}");
        assert!(clean.changed.is_empty());
        assert!(!clean.blocks_comparisons());

        let mut changed = observations.clone();
        changed.fields.get_mut("server.build").unwrap().value = "b-other".to_owned();
        let drift = observed_drift(&qualification, &runner(), &policy, &changed);
        assert_eq!(drift.changed, vec!["observed.server.build"]);

        let mut changed = observations.clone();
        changed.fields.remove("server.context");
        changed
            .unknown_optional
            .insert("server.context".to_owned(), "not reported".to_owned());
        let drift = observed_drift(&qualification, &runner(), &policy, &changed);
        assert_eq!(
            drift.changed,
            vec!["observed.server.context", "optional.server.context"]
        );

        let mut changed = observations.clone();
        changed.fields.get_mut("catalogue").unwrap().value = "c".repeat(64);
        let drift = observed_drift(&qualification, &runner(), &policy, &changed);
        assert_eq!(drift.changed, vec!["observed.catalogue"]);

        let mut changed_runner = runner();
        changed_runner.endpoint = "http://127.0.0.1:65501/v1".to_owned();
        let drift = observed_drift(&qualification, &changed_runner, &policy, &observations);
        assert_eq!(drift.changed, vec!["endpoint"]);

        let mut changed_runner = runner();
        changed_runner.model = "fixture-model-y".to_owned();
        let drift = observed_drift(&qualification, &changed_runner, &policy, &observations);
        assert_eq!(drift.changed, vec!["model"]);

        let mut changed_runner = runner();
        changed_runner.identity.quantization = None;
        let drift = observed_drift(&qualification, &changed_runner, &policy, &observations);
        assert!(drift.changed.contains(&"identity.quantization".to_owned()));

        // A dropped, relaxed or changed declaration never reuses a pass.
        let mut dropped_field = policy.clone();
        dropped_field.plan.requests[0]
            .fields
            .retain(|field| field.name != "server.context");
        let drift = observed_drift(&qualification, &runner(), &dropped_field, &observations);
        assert!(
            drift
                .changed
                .contains(&"declared.server.context".to_owned()),
            "{drift:?}"
        );

        let mut relaxed = policy.clone();
        relaxed.plan.requests[0]
            .fields
            .iter_mut()
            .find(|field| field.name == "server.build")
            .unwrap()
            .required = false;
        let drift = observed_drift(&qualification, &runner(), &relaxed, &observations);
        assert!(drift.changed.contains(&"declared.server.build".to_owned()));

        // A digest-bound fact is dropped and an effective client input is
        // removed: neither downgrade may reuse the earlier pass.
        let mut downgraded = policy.clone();
        downgraded.plan.requests[0]
            .fields
            .retain(|field| field.name != "server.template");
        downgraded.plan.required_client_inputs.pop();
        let drift = observed_drift(&qualification, &runner(), &downgraded, &observations);
        assert!(
            drift
                .changed
                .contains(&"declared.server.template".to_owned())
        );
        assert!(drift.changed.contains(&"required_client_inputs".to_owned()));

        let mut output = policy.clone();
        output.output.required_outputs = vec!["other.txt".to_owned()];
        let drift = observed_drift(&qualification, &runner(), &output, &observations);
        assert!(
            drift
                .changed
                .contains(&"output.required_outputs".to_owned())
        );

        let mut repeats = policy.clone();
        repeats.output.repeats = 3;
        let drift = observed_drift(&qualification, &runner(), &repeats, &observations);
        assert!(drift.changed.contains(&"output.repeats".to_owned()));

        let mut other_path = policy.clone();
        other_path.plan.requests[0].path = "/other".to_owned();
        let drift = observed_drift(&qualification, &runner(), &other_path, &observations);
        assert!(drift.changed.contains(&"declared.server.build".to_owned()));

        // Fresh observations collected under another plan are visible too.
        let mut other_plan = observations.clone();
        other_plan.plan_digest = "0000".to_owned();
        let drift = observed_drift(&qualification, &runner(), &policy, &other_plan);
        assert!(drift.changed.contains(&"observations.plan".to_owned()));

        // A blocked qualification never re-enters dependent comparisons.
        let mut incomplete = observations.clone();
        incomplete.fields.remove("profile");
        let blocked = qualify_api_observed(&runner(), &policy, &incomplete, &attempts).unwrap();
        assert!(blocked.blocks_comparisons());
        let drift = observed_drift(&blocked, &runner(), &policy, &incomplete);
        assert!(drift.changed.contains(&"qualification.blocked".to_owned()));
    }

    #[test]
    fn api_observed_collection_hashes_client_inputs_and_reports_failures() {
        let private = tempfile::tempdir().unwrap();
        let profile = private.path().join("profile.toml");
        let catalogue = private.path().join("catalogue.json");
        std::fs::write(&profile, "model = \"synth\"\n").unwrap();
        std::fs::write(&catalogue, "{\"servers\":[]}\n").unwrap();
        let inputs = [
            ClientInput {
                name: "profile".to_owned(),
                path: profile.clone(),
            },
            ClientInput {
                name: "catalogue".to_owned(),
                path: catalogue.clone(),
            },
        ];

        // A closed loopback port: fetch failure is a transport failure, not a
        // missing optional field.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut unreachable = runner();
        unreachable.endpoint = format!("http://127.0.0.1:{port}/v1");
        let failure = collect_observations(&unreachable, &api_policy().plan, &inputs).unwrap_err();
        assert_eq!(failure.kind, ObservationFailureKind::Transport);
        assert_eq!(failure.source, "/props");
        assert!(
            !failure
                .to_string()
                .contains(&private.path().display().to_string())
        );
        assert!(!failure.to_string().contains("127.0.0.1"));

        // A declared effective client input that is not supplied or not an
        // owned readable file is distinguishable from the transport failure.
        let failure =
            collect_observations(&unreachable, &api_policy().plan, &inputs[..1]).unwrap_err();
        assert_eq!(failure.kind, ObservationFailureKind::ClientInput);
        assert_eq!(failure.source, "catalogue");
        let missing = ClientInput {
            name: "catalogue".to_owned(),
            path: private.path().join("absent.json"),
        };
        let invalid_inputs = [inputs[0].clone(), missing];
        let failure =
            collect_observations(&unreachable, &api_policy().plan, &invalid_inputs).unwrap_err();
        assert_eq!(failure.kind, ObservationFailureKind::ClientInput);
        assert_eq!(failure.detail, "input is not an owned regular file");

        // Invalid declarations and unsupported transports refuse before any
        // document is read; nothing observed is invented.
        let mut invalid = api_policy().plan;
        invalid.requests[0].fields[0].pointer = "build".to_owned();
        let failure = collect_observations(&runner(), &invalid, &inputs).unwrap_err();
        assert_eq!(failure.kind, ObservationFailureKind::Declaration);
        let mut https = runner();
        https.endpoint = "https://127.0.0.1:65500/v1".to_owned();
        let failure = collect_observations(&https, &api_policy().plan, &inputs).unwrap_err();
        assert_eq!(failure.kind, ObservationFailureKind::Transport);
        assert!(failure.to_string().contains("https"));
    }
}
