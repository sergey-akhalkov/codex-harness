//! Grounded improvement intake for the sequential self-improvement loop.
//!
//! The controller asks a bounded investigator for candidate improvements only
//! after retained native evidence and an authorized real work item exist. This
//! module turns the investigator's bounded report into board outcomes: an
//! admitted hypothesis card, a reused prior conclusion, an explicit
//! deferred/idle state or a refusal. It never calls a model, applies no
//! treatment, deletes nothing, records no benefit verdict and starts no run.
//!
//! Call order inside the controller:
//!
//! 1. build an [`EvidenceIndex`] from retained native evidence (for example
//!    [`EvidenceItem::read_rollout`] over a retained rollout file, or
//!    caller-supplied items attributed to an existing owner);
//! 2. read the investigator's [`InvestigatorReport`] from its bounded result;
//! 3. call [`intake`] with the board inputs, the report and the index;
//! 4. consume the [`IntakeReport`]: `admitted`/`reconsidered` cards continue to
//!    planning, `existing`/`reused-*`/`no-change`/`reuse-suffices`/`idle`/
//!    `deferred` do not, and `refused` stays unadmitted with exact reasons.
//!
//! Contract guarantees:
//!
//! - observations, inferences and predictions stay distinguishable: evidence
//!   items and references carry [`ClaimKind`], the expected effect stays
//!   `predicted`, and a prediction can never be cited as retained evidence;
//! - a repeated-read claim must match retained structured read facts captured
//!   by an existing owner ([`RetainedRead::capture`] over the native file
//!   change identity and the content identities of
//!   [`crate::build_identity`]); investigator-supplied tokens alone never
//!   establish the repetition. Missing retained facts defer, changed or
//!   non-matching retained facts refuse, and only two retained reads with the
//!   same file, content and context identity admit the claim;
//! - retention, not use counts, grounds a hypothesis: a subtraction or
//!   simplification proposal is only admitted with a bounded
//!   coverage/lost-use/restoration basis, usage volume alone defers to further
//!   investigation, and this module never applies a removal - the user's
//!   informed decision remains owned by the removal proposal/decision verbs;
//! - prior results participate in admission through
//!   [`crate::board_hypothesis::admit_hypothesis`]: open, closed and deferred
//!   same-condition cards are reused, and only a recorded new evidential basis
//!   links a reconsideration while preserving the earlier conclusion;
//! - evaluation workloads use the nonblocking `related` edge; an unreadable
//!   workload never blocks the candidate admission;
//! - missing contract parts, unretained evidence, overclaimed evidence
//!   strength and partial retained evidence are refused or deferred before any
//!   board mutation; an empty report is idle and performs no board or model
//!   work.
//!
//! The validity of a prose mechanism is the investigator's and the lead's
//! judgment; intake enforces the bounded contract, the evidence attribution
//! and the board consequences, and records that limit instead of certifying a
//! benefit.
//!
//! ```no_run
//! use harness_core::improvement_intake::{
//!     EvidenceIndex, EvidenceItem, InvestigatorReport, intake,
//! };
//! use std::path::{Path, PathBuf};
//!
//! # fn main() -> std::io::Result<()> {
//! let evidence = EvidenceIndex::new(vec![EvidenceItem::read_rollout(
//!     "rollout:cycle-1#seed",
//!     Path::new("retained/rollout.jsonl"),
//! )?])?;
//! let report = InvestigatorReport {
//!     schema: 1,
//!     candidates: Vec::new(),
//!     idle_reason: Some("the bounded evidence shows no attributable burden".to_owned()),
//! };
//! let outcomes = intake(
//!     Path::new("bd.exe"),
//!     &PathBuf::from("project"),
//!     &report,
//!     &evidence,
//! )?;
//! assert_eq!(outcomes.outcomes.len(), 1);
//! # Ok(())
//! # }
//! ```

use crate::{board_hypothesis, build_identity, rollout_reader};
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

/// Version of the bounded investigator report this intake reads.
pub const INTAKE_SCHEMA: u32 = 1;
/// Bound on candidates in one investigator report.
pub const MAX_CANDIDATES: usize = 8;
/// Bound on retained-evidence references in one candidate.
pub const MAX_EVIDENCE_REFERENCES: usize = 8;
/// Bound on evidence items in one intake index.
pub const MAX_EVIDENCE_ITEMS: usize = 64;
/// Bound on one locator, identity value or token reference.
pub const MAX_LOCATOR: usize = 128;
/// Bound on one token field (mechanism, conditions, target).
pub const MAX_TOKEN: usize = 96;
/// Bound on one statement line (effect, counterexample, acceptance, reason).
pub const MAX_STATEMENT: usize = 256;
/// Bound on the coverage description of one evidence item.
pub const MAX_COVERAGE: usize = 192;
/// Bound on the error and warning notes of one evidence item.
pub const MAX_NOTES: usize = 16;
/// Bound on the retained read facts of one evidence item.
pub const MAX_READS: usize = 16;
/// Bound on refusal reasons reported for one candidate.
pub const MAX_REASONS: usize = 8;
/// Bound on one serialized investigator report.
pub const MAX_REPORT_BYTES: u64 = 256 * 1024;

/// Whether a statement is an observed fact, an inference or a prediction.
///
/// The ordering is meaningful: a claim can never be stronger than the
/// retained evidence it cites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimKind {
    Predicted,
    Inferred,
    Observed,
}

impl ClaimKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Predicted => "predicted",
            Self::Inferred => "inferred",
            Self::Observed => "observed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "predicted" => Some(Self::Predicted),
            "inferred" => Some(Self::Inferred),
            "observed" => Some(Self::Observed),
            _ => None,
        }
    }
}

/// The existing owner that produced one retained evidence record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceOwner {
    /// An existing rollout/telemetry reader such as [`rollout_reader`].
    Rollout,
    /// Outcome accounting retained by the outcome owner.
    Outcome,
    /// A board record, decision or card.
    Board,
    /// The retained source/file identity owner (native file change identity
    /// and the content identities of [`crate::build_identity`]).
    Source,
    /// An observation of authorized real work outside this checkout.
    AuthorizedWork,
}

impl EvidenceOwner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rollout => "rollout",
            Self::Outcome => "outcome",
            Self::Board => "board",
            Self::Source => "source",
            Self::AuthorizedWork => "authorized-work",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "rollout" => Some(Self::Rollout),
            "outcome" => Some(Self::Outcome),
            "board" => Some(Self::Board),
            "source" => Some(Self::Source),
            "authorized-work" => Some(Self::AuthorizedWork),
            _ => None,
        }
    }
}

/// One retained read observation of a file, captured for one actual read by
/// an existing owner: the native file change identity, the content identity
/// and the task/attempt context identity the read happened under.
///
/// This is the only accepted identity source for a repeated-read claim.
/// Investigator-supplied tokens are assertions; they are matched against
/// these retained facts, never trusted on their own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedRead {
    pub file: String,
    pub content: String,
    pub context: String,
}

impl RetainedRead {
    /// Captures one read observation from an actual file, reusing the
    /// existing owners: [`crate::build_identity::ordinary`] and
    /// [`crate::build_identity::hash_file`] for the owned-input check and the
    /// content identity, and [`rollout_reader::FileIdentity`] for the native
    /// file change identity. `context` names the existing task, attempt or
    /// run identity under which the read happened. The file is verified
    /// unchanged across the capture so the two identities describe the same
    /// bytes.
    pub fn capture(path: &Path, context: &str) -> io::Result<Self> {
        build_identity::ordinary(path)?;
        let before = rollout_reader::FileIdentity::of_path(path)?;
        let content = build_identity::hash_file(path)?;
        let after = rollout_reader::FileIdentity::of_path(path)?;
        if before != after {
            return Err(board_hypothesis::invalid(
                "the file changed while its read identity was captured; capture again under stable content",
            ));
        }
        let id_hex: String = before
            .file_id
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Self {
            file: format!(
                "file:v{:x}.i{}.c{:x}.s{:x}",
                before.volume_serial, id_hex, before.change_time, before.size
            ),
            content: format!("sha256.{content}"),
            context: context.to_owned(),
        }
        .bounded()
    }

    fn bounded(self) -> io::Result<Self> {
        Ok(Self {
            file: board_hypothesis::require_token("read file identity", &self.file, MAX_LOCATOR)?,
            content: board_hypothesis::require_token(
                "read content identity",
                &self.content,
                MAX_LOCATOR,
            )?,
            context: board_hypothesis::require_token(
                "read context identity",
                &self.context,
                MAX_LOCATOR,
            )?,
        })
    }
}

/// One bounded retained-evidence record with its attribution, coverage and
/// deficits.
///
/// `errors` carry measured deficits (unreadable input, corrupt or oversized
/// lines, conflicting or identity-less usage records); an item with any error
/// is partial and cannot ground an observed claim. `warnings` carry the
/// reader's own notes, including missing counters - they stay visible instead
/// of being turned into zero values. `reads` carry the structured read
/// observations captured by an existing owner; merely labelling an item
/// observed supplies no identities it does not carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceItem {
    /// A bounded drill-down reference cited by candidate proposals.
    pub locator: String,
    /// The existing owner that produced this record.
    pub owner: EvidenceOwner,
    /// Whether the record is an observed fact or an inference over one.
    pub kind: ClaimKind,
    /// The reader's bounded coverage description.
    pub coverage: String,
    /// Measured deficits that make the evidence partial.
    #[serde(default)]
    pub errors: Vec<String>,
    /// The reader's own notes (stale or missing counters stay explicit).
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Retained structured read observations for this evidence.
    #[serde(default)]
    pub reads: Vec<RetainedRead>,
}

impl EvidenceItem {
    /// Builds one bounded evidence item without retained read facts. A
    /// prediction is not retained evidence and is refused here; keep it in
    /// the predicted effect. Attach captured read observations through
    /// [`EvidenceItem::with_reads`] or [`EvidenceItem::read_source`].
    pub fn new(
        locator: &str,
        owner: EvidenceOwner,
        kind: ClaimKind,
        coverage: &str,
        errors: &[String],
        warnings: &[String],
    ) -> io::Result<Self> {
        Self::validated(locator, owner, kind, coverage, errors, warnings, &[])
    }

    fn validated(
        locator: &str,
        owner: EvidenceOwner,
        kind: ClaimKind,
        coverage: &str,
        errors: &[String],
        warnings: &[String],
        reads: &[RetainedRead],
    ) -> io::Result<Self> {
        if kind == ClaimKind::Predicted {
            return Err(board_hypothesis::invalid(
                "a predicted statement is not retained evidence; keep predictions in the predicted effect field",
            ));
        }
        let locator = board_hypothesis::require_token("evidence locator", locator, MAX_LOCATOR)?;
        let coverage = board_hypothesis::require_line("coverage", coverage, MAX_COVERAGE)?;
        let errors = bounded_notes("evidence error", errors)?;
        let warnings = bounded_notes("evidence warning", warnings)?;
        if reads.len() > MAX_READS {
            return Err(board_hypothesis::invalid(format!(
                "the evidence item carries {} read facts; at most {MAX_READS} are accepted",
                reads.len()
            )));
        }
        let reads = reads
            .iter()
            .cloned()
            .map(RetainedRead::bounded)
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Self {
            locator,
            owner,
            kind,
            coverage,
            errors,
            warnings,
            reads,
        })
    }

    /// Attaches retained structured read observations to this item and
    /// re-validates the bounds.
    pub fn with_reads(mut self, reads: Vec<RetainedRead>) -> io::Result<Self> {
        self.reads = reads;
        self.bounded()
    }

    /// Captures one retained read observation from an actual file through the
    /// existing owners and builds the evidence item for it. A repeated-read
    /// claim needs two such observations on one item, each captured for its
    /// own read.
    pub fn read_source(locator: &str, context: &str, path: &Path) -> io::Result<Self> {
        let read = RetainedRead::capture(path, context)?;
        Self::validated(
            locator,
            EvidenceOwner::Source,
            ClaimKind::Observed,
            "retained_reads=1 native-file-identity content=sha256",
            &[],
            &[],
            std::slice::from_ref(&read),
        )
    }

    /// Reads one retained rollout file through the existing reader and retains
    /// its attribution, coverage counters, deficits and notes. Token counters
    /// are not aggregated here: the drill-down locator keeps the reader's own
    /// measured/missing/inferred semantics authoritative.
    pub fn read_rollout(locator: &str, path: &Path) -> io::Result<Self> {
        let summary = rollout_reader::read(path);
        let coverage = format!(
            "lines={} events={} recognized={} unrecognized={} corrupt={} oversized={} responses={} turns={} cumulative_snapshots={}",
            summary.coverage.lines,
            summary.coverage.events,
            summary.coverage.recognized_events,
            summary.coverage.unrecognized_events,
            summary.coverage.corrupt_lines,
            summary.coverage.oversized_lines,
            summary
                .row
                .get("response_count")
                .and_then(|value| value.as_u64())
                .unwrap_or(0),
            summary.turns.len(),
            summary.cumulative.len(),
        );
        let mut errors = Vec::new();
        if summary.warnings.contains("unreadable_input") {
            errors.push("the rollout file was unreadable; no counters were measured".to_owned());
        }
        if summary.coverage.corrupt_lines > 0 {
            errors.push(format!(
                "{} line(s) were not valid JSON and stayed outside the measured counters",
                summary.coverage.corrupt_lines
            ));
        }
        if summary.coverage.oversized_lines > 0 {
            errors.push(format!(
                "{} line(s) exceeded the per-record size bound and stayed outside the measured counters",
                summary.coverage.oversized_lines
            ));
        }
        if !summary.conflicts.is_empty() {
            errors.push(format!(
                "{} response identity/identities disagreed; their usage stays unknown",
                summary.conflicts.len()
            ));
        }
        if !summary.unidentified.is_empty() {
            errors.push(format!(
                "{} usage record(s) carried no stable identity; interval allocation stays unknown",
                summary.unidentified.len()
            ));
        }
        let warnings: Vec<String> = summary.warnings.into_iter().collect();
        Self::new(
            locator,
            EvidenceOwner::Rollout,
            ClaimKind::Observed,
            &coverage,
            &errors,
            &warnings,
        )
    }

    /// True when a measured deficit keeps this item partial.
    pub fn is_partial(&self) -> bool {
        !self.errors.is_empty()
    }

    fn bounded(self) -> io::Result<Self> {
        Self::validated(
            &self.locator,
            self.owner,
            self.kind,
            &self.coverage,
            &self.errors,
            &self.warnings,
            &self.reads,
        )
    }
}

fn bounded_notes(name: &str, notes: &[String]) -> io::Result<Vec<String>> {
    if notes.len() > MAX_NOTES {
        return Err(board_hypothesis::invalid(format!(
            "{name} notes exceed {MAX_NOTES} entries"
        )));
    }
    notes
        .iter()
        .map(|note| board_hypothesis::require_line(name, note, MAX_STATEMENT))
        .collect()
}

/// The bounded retained evidence a report may cite, indexed by locator.
#[derive(Debug, Clone, Default)]
pub struct EvidenceIndex {
    items: Vec<EvidenceItem>,
}

impl EvidenceIndex {
    /// Builds an index. Duplicate locators and items outside the bounds are
    /// refused so a citation can never resolve ambiguously.
    pub fn new(items: Vec<EvidenceItem>) -> io::Result<Self> {
        if items.len() > MAX_EVIDENCE_ITEMS {
            return Err(board_hypothesis::invalid(format!(
                "the evidence index carries {} items; at most {MAX_EVIDENCE_ITEMS} are accepted",
                items.len()
            )));
        }
        let mut bounded = Vec::with_capacity(items.len());
        for item in items {
            let item = item.bounded()?;
            if bounded
                .iter()
                .any(|existing: &EvidenceItem| existing.locator == item.locator)
            {
                return Err(board_hypothesis::invalid(format!(
                    "the evidence locator {} appears more than once; a citation must resolve uniquely",
                    item.locator
                )));
            }
            bounded.push(item);
        }
        Ok(Self { items: bounded })
    }

    /// The retained item for one locator.
    pub fn resolve(&self, locator: &str) -> Option<&EvidenceItem> {
        self.items.iter().find(|item| item.locator == locator)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// One bounded reference from a candidate to retained evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub locator: String,
    pub kind: ClaimKind,
}

/// An asserted identity of one read of a file within a task context. It is
/// matched against retained read facts ([`RetainedRead`]); the assertion alone
/// never establishes the repetition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadIdentity {
    /// The structured file identity (for example a reader file id or path).
    pub file: String,
    /// The structured content identity (a digest of the read bytes).
    pub content: String,
    /// The task/context identity under which the read happened.
    pub context: String,
}

/// A claim that one repeated read was avoidable. Both asserted identities must
/// equal two retained structured read observations ([`RetainedRead`]) with one
/// file, content and context identity; unchanged retained identity is
/// necessary, and the mechanism must still explain why the repetition was
/// unnecessary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatedReadClaim {
    /// What was repeated (for example `read:src/lib.rs`).
    pub operation: String,
    /// The retained structured record that carries the two reads.
    pub evidence: String,
    pub first: ReadIdentity,
    pub second: ReadIdentity,
}

/// What a subtraction or simplification claim rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemovalBasis {
    /// Low or absent use. This is a lead for investigation, never a finding
    /// of uselessness: intake defers it to a coverage investigation.
    UsageVolume { invocations: u64, window: String },
    /// The bounded analysis a reviewable removal proposal needs.
    Coverage {
        interval: String,
        tasks: String,
        gaps: String,
        lost_uses: String,
        restoration: String,
    },
}

/// One claimed removal target and its basis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovalClaim {
    pub target: String,
    pub basis: RemovalBasis,
}

/// The investigator's route: addition, no change, reuse of an existing route,
/// simplification or subtraction. No-change and reuse are concluded outcomes;
/// simplification and subtraction stay behind the separate removal authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Treatment {
    Addition,
    NoChange { reason: String },
    Reuse { existing: String },
    Simplification { removal: RemovalClaim },
    Subtraction { removal: RemovalClaim },
}

/// The workload one candidate is intended to be evaluated on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadRef {
    /// The workload hypothesis card.
    pub item: String,
    /// The experiment identity the relationship belongs to.
    pub experiment: String,
    #[serde(default)]
    pub evidence: Option<String>,
}

/// One bounded investigator candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub mechanism: String,
    /// Applicability conditions under which the mechanism is claimed to hold.
    pub conditions: String,
    /// The retained observation locator that grounds the candidate.
    pub observation: String,
    #[serde(default)]
    pub predicted: Option<String>,
    #[serde(default)]
    pub counterexample: Option<String>,
    #[serde(default)]
    pub acceptance: Option<String>,
    /// The exact OpenSpec change reference.
    #[serde(default)]
    pub spec: Option<String>,
    /// The fresh evidential basis; it must be a retained evidence locator.
    pub basis: String,
    pub treatment: Treatment,
    /// The retained evidence this candidate cites, with claim strengths.
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub repeated_read: Option<RepeatedReadClaim>,
    #[serde(default)]
    pub workload: Option<WorkloadRef>,
    /// The useful next check when the candidate cannot be grounded yet.
    #[serde(default)]
    pub next_check: Option<String>,
}

/// The investigator's bounded output for one intake round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigatorReport {
    pub schema: u32,
    #[serde(default)]
    pub candidates: Vec<Proposal>,
    /// Why no candidate is proposed; used verbatim by the idle outcome.
    #[serde(default)]
    pub idle_reason: Option<String>,
}

/// Reads one bounded investigator report from a file.
pub fn read_report(path: &Path) -> io::Result<InvestigatorReport> {
    crate::improvement_loop::read_json(path, MAX_REPORT_BYTES)
}

/// The recorded outcome of one candidate or one empty intake round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum IntakeOutcome {
    /// A card was created for this candidate; planning may proceed.
    Admitted {
        id: String,
        /// The retained evidence locators this admission cited.
        evidence: Vec<String>,
        /// The candidate is a removal treatment; no removal is applied and the
        /// controller's removal gate must find a recorded user decision before
        /// dependent work.
        removal_required: bool,
        workload: Option<WorkloadLink>,
    },
    /// A matching card is already active; no duplicate card was created.
    Existing {
        id: String,
        status: String,
        workload: Option<WorkloadLink>,
    },
    /// A prior same-condition rejection is reused as recorded.
    ReusedRejection {
        id: String,
        experiment: Option<String>,
        reason: Option<String>,
        basis: Option<String>,
    },
    /// A prior same-condition inconclusive result is reused as recorded.
    ReusedInconclusive {
        id: String,
        experiment: Option<String>,
        reason: Option<String>,
        basis: Option<String>,
    },
    /// A fresh retained basis was recorded on the existing card, the card was
    /// reactivated and the earlier conclusion is preserved.
    Reconsidered {
        id: String,
        basis: String,
        prior_outcome: String,
        prior_experiment: Option<String>,
        workload: Option<WorkloadLink>,
    },
    /// The evidence supports making no change at this time.
    NoChange { reason: String },
    /// An existing attributable route satisfies the evidenced need.
    ReuseSuffices { existing: String },
    /// Grounding is incomplete and a useful next check exists.
    Deferred { reason: String, next: String },
    /// The candidate is unsupported; nothing was admitted.
    Refused { reasons: Vec<String> },
    /// No grounded candidate remains; no board or model work was performed.
    Idle { reason: String },
}

/// How the nonblocking candidate/workload relationship resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum WorkloadLink {
    /// The trial comment and the nonblocking `related` edge were established.
    Recorded {
        item: String,
        experiment: String,
        /// A new trial comment was written by this call.
        comment: bool,
        /// The `related` edge was established by this call.
        related: bool,
    },
    /// The reference could not be linked; the candidate admission is
    /// unaffected and the workload never blocks it.
    Unlinked {
        item: String,
        experiment: String,
        reason: String,
    },
}

/// The intake results for one investigator report, parallel to its candidates
/// (a single [`IntakeOutcome::Idle`] for an empty report).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeReport {
    pub schema: u32,
    pub outcomes: Vec<IntakeOutcome>,
}

/// Runs one grounded intake round against the board and the retained evidence.
///
/// Structural validation, evidence grounding and candidate admission are
/// separated: a refused or deferred candidate performs no board mutation, and
/// admission additionally searches every prior card through
/// [`board_hypothesis::admit_hypothesis`], so a repeated candidate reuses its
/// recorded conclusion instead of creating a duplicate card.
pub fn intake(
    bd: &Path,
    project: &Path,
    report: &InvestigatorReport,
    evidence: &EvidenceIndex,
) -> io::Result<IntakeReport> {
    if report.schema != INTAKE_SCHEMA {
        return Err(board_hypothesis::invalid(format!(
            "the investigator report declares schema {}; intake reads schema {INTAKE_SCHEMA}",
            report.schema
        )));
    }
    if report.candidates.len() > MAX_CANDIDATES {
        return Err(board_hypothesis::invalid(format!(
            "the investigator report carries {} candidates; at most {MAX_CANDIDATES} are accepted",
            report.candidates.len()
        )));
    }
    if report.candidates.is_empty() {
        let reason = match report.idle_reason.as_deref() {
            Some(value) => board_hypothesis::require_line("idle reason", value, MAX_STATEMENT)?,
            None => {
                "no grounded candidate was proposed; awaiting fresh evidence or authorized real work"
                    .to_owned()
            }
        };
        return Ok(IntakeReport {
            schema: INTAKE_SCHEMA,
            outcomes: vec![IntakeOutcome::Idle { reason }],
        });
    }
    let mut outcomes = Vec::with_capacity(report.candidates.len());
    for candidate in &report.candidates {
        outcomes.push(evaluate(bd, project, candidate, evidence)?);
    }
    Ok(IntakeReport {
        schema: INTAKE_SCHEMA,
        outcomes,
    })
}

/// One validation problem. `unsupported` problems refuse the candidate;
/// problems that only find missing grounding may defer it to the recorded
/// next check.
struct Issue {
    unsupported: bool,
    message: String,
    next: Option<String>,
}

impl Issue {
    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            unsupported: true,
            message: message.into(),
            next: None,
        }
    }

    fn missing(message: impl Into<String>, next: impl Into<String>) -> Self {
        Self {
            unsupported: false,
            message: message.into(),
            next: Some(next.into()),
        }
    }
}

struct Failure {
    issues: Vec<Issue>,
    next_check: Option<String>,
}

impl Failure {
    fn outcome(&self) -> IntakeOutcome {
        if self.issues.iter().any(|issue| issue.unsupported) {
            let mut reasons: Vec<String> = self
                .issues
                .iter()
                .map(|issue| issue.message.clone())
                .collect();
            if reasons.len() > MAX_REASONS {
                let extra = reasons.len() - MAX_REASONS + 1;
                reasons.truncate(MAX_REASONS - 1);
                reasons.push(format!("and {extra} more issue(s)"));
            }
            return IntakeOutcome::Refused { reasons };
        }
        let reason = self
            .issues
            .iter()
            .map(|issue| issue.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        let next = self
            .next_check
            .clone()
            .or_else(|| self.issues.iter().find_map(|issue| issue.next.clone()))
            .unwrap_or_else(|| {
                "retain the missing evidence and resubmit the candidate through intake".to_owned()
            });
        IntakeOutcome::Deferred { reason, next }
    }
}

enum Prepared {
    Hypothesis {
        bounded: board_hypothesis::BoundedHypothesis,
        removal_required: bool,
    },
    NoChange(String),
    Reuse(String),
}

enum TreatmentBuild {
    Hypothesis,
    NoChange(String),
    Reuse(String),
}

fn evaluate(
    bd: &Path,
    project: &Path,
    proposal: &Proposal,
    evidence: &EvidenceIndex,
) -> io::Result<IntakeOutcome> {
    match prepare(proposal, evidence) {
        Ok(Prepared::Hypothesis {
            bounded,
            removal_required,
        }) => admit(bd, project, proposal, &bounded, removal_required),
        Ok(Prepared::NoChange(reason)) => Ok(IntakeOutcome::NoChange { reason }),
        Ok(Prepared::Reuse(existing)) => Ok(IntakeOutcome::ReuseSuffices { existing }),
        Err(failure) => Ok(failure.outcome()),
    }
}

fn prepare(proposal: &Proposal, index: &EvidenceIndex) -> Result<Prepared, Failure> {
    let mut issues: Vec<Issue> = Vec::new();
    let next_check = match proposal.next_check.as_deref() {
        None => None,
        Some(value) => bounded_line("next check", value, MAX_STATEMENT, &mut issues),
    };
    let full = matches!(
        proposal.treatment,
        Treatment::Addition | Treatment::Simplification { .. } | Treatment::Subtraction { .. }
    );

    let mechanism = bounded_token("mechanism", &proposal.mechanism, MAX_TOKEN, &mut issues);
    let conditions = bounded_token("conditions", &proposal.conditions, MAX_TOKEN, &mut issues);
    let observation = bounded_token(
        "observation",
        &proposal.observation,
        MAX_LOCATOR,
        &mut issues,
    );
    let basis = bounded_token("basis", &proposal.basis, MAX_LOCATOR, &mut issues);
    let predicted = bounded_statement(
        "predicted effect",
        proposal.predicted.as_deref(),
        full,
        &mut issues,
    );
    let counterexample = bounded_statement(
        "counterexample",
        proposal.counterexample.as_deref(),
        full,
        &mut issues,
    );
    let acceptance = bounded_statement(
        "acceptance",
        proposal.acceptance.as_deref(),
        full,
        &mut issues,
    );
    let spec = match proposal.spec.as_deref() {
        Some(value) => bounded_token("spec reference", value, MAX_LOCATOR, &mut issues),
        None if full => {
            issues.push(Issue::unsupported(
                "spec reference is required: every candidate names its own OpenSpec change before implementation",
            ));
            None
        }
        None => None,
    };

    if proposal.evidence.is_empty() {
        issues.push(Issue::unsupported(
            "at least one retained evidence reference is required; an ungrounded idea cannot be admitted",
        ));
    } else if proposal.evidence.len() > MAX_EVIDENCE_REFERENCES {
        issues.push(Issue::unsupported(format!(
            "{} evidence references exceed the {MAX_EVIDENCE_REFERENCES} accepted",
            proposal.evidence.len()
        )));
    }
    let mut observed = false;
    for reference in &proposal.evidence {
        let Some(locator) = bounded_token(
            "evidence locator",
            &reference.locator,
            MAX_LOCATOR,
            &mut issues,
        ) else {
            continue;
        };
        if reference.kind == ClaimKind::Predicted {
            issues.push(Issue::unsupported(format!(
                "the locator {locator} is cited as a prediction; predictions are not retained evidence"
            )));
            continue;
        }
        let Some(item) = index.resolve(&locator) else {
            issues.push(Issue::unsupported(format!(
                "the evidence locator {locator} is not retained in the supplied evidence index"
            )));
            continue;
        };
        if reference.kind > item.kind {
            issues.push(Issue::unsupported(format!(
                "the retained item {locator} is {} but is cited as {}; a claim cannot be stronger than its evidence",
                item.kind.as_str(),
                reference.kind.as_str()
            )));
            continue;
        }
        if reference.kind == ClaimKind::Observed {
            observed = true;
            if item.is_partial() {
                issues.push(Issue::missing(
                    format!(
                        "the retained evidence {locator} is partial: {}",
                        item.errors.join("; ")
                    ),
                    "retain evidence without the recorded deficits or cite an item whose coverage is complete",
                ));
            }
        }
    }
    if !proposal.evidence.is_empty() && !observed {
        issues.push(Issue::unsupported(
            "at least one observed evidence reference is required; an inference alone cannot ground a hypothesis",
        ));
    }

    if let Some(locator) = &observation {
        match index.resolve(locator) {
            None => issues.push(Issue::unsupported(format!(
                "the observation {locator} is not retained in the supplied evidence index"
            ))),
            Some(item) if item.kind != ClaimKind::Observed => {
                issues.push(Issue::unsupported(format!(
                    "the observation {locator} is an inference rather than an observed fact"
                )));
            }
            Some(item) if item.is_partial() => issues.push(Issue::missing(
                format!(
                    "the observation evidence {locator} is partial: {}",
                    item.errors.join("; ")
                ),
                "retain complete evidence for the observation and resubmit",
            )),
            Some(_) => {}
        }
    }
    if let Some(locator) = &basis
        && index.resolve(locator).is_none()
    {
        issues.push(Issue::unsupported(format!(
            "the basis {locator} is not retained in the supplied evidence index; a fresh basis must name retained evidence"
        )));
    }

    if let Some(claim) = &proposal.repeated_read {
        validate_repeated_read(claim, index, &mut issues);
    }

    if let Some(workload) = &proposal.workload {
        let _ = bounded_token("workload item", &workload.item, MAX_TOKEN, &mut issues);
        let _ = bounded_token(
            "workload experiment",
            &workload.experiment,
            MAX_LOCATOR,
            &mut issues,
        );
        if let Some(reference) = &workload.evidence {
            let _ = bounded_token("workload evidence", reference, MAX_LOCATOR, &mut issues);
        }
    }

    let mut removal_required = false;
    let build = match &proposal.treatment {
        Treatment::Addition => Some(TreatmentBuild::Hypothesis),
        Treatment::NoChange { reason } => {
            bounded_line("no-change reason", reason, MAX_STATEMENT, &mut issues)
                .map(TreatmentBuild::NoChange)
        }
        Treatment::Reuse { existing } => {
            let existing = bounded_token("reuse reference", existing, MAX_LOCATOR, &mut issues);
            if let Some(existing) = &existing
                && index.resolve(existing).is_none()
            {
                issues.push(Issue::unsupported(format!(
                    "the reuse reference {existing} is not retained in the evidence index; the existing route must be attributable"
                )));
            }
            existing.map(TreatmentBuild::Reuse)
        }
        Treatment::Simplification { removal } | Treatment::Subtraction { removal } => {
            removal_required = validate_removal(removal, &mut issues);
            Some(TreatmentBuild::Hypothesis)
        }
    };

    if !issues.is_empty() {
        return Err(Failure { issues, next_check });
    }
    let Some(build) = build else {
        return Err(Failure {
            issues: vec![Issue::unsupported(
                "the proposal treatment is incomplete; resubmit the candidate through intake",
            )],
            next_check,
        });
    };
    match build {
        TreatmentBuild::NoChange(reason) => Ok(Prepared::NoChange(reason)),
        TreatmentBuild::Reuse(existing) => Ok(Prepared::Reuse(existing)),
        TreatmentBuild::Hypothesis => {
            let (Some(mechanism), Some(conditions), Some(observation), Some(basis)) =
                (mechanism, conditions, observation, basis)
            else {
                return Err(Failure {
                    issues: vec![Issue::unsupported(
                        "mechanism, conditions, observation and basis are required before admission",
                    )],
                    next_check,
                });
            };
            let (Some(predicted), Some(counterexample), Some(acceptance), Some(spec)) =
                (predicted, counterexample, acceptance, spec)
            else {
                return Err(Failure {
                    issues: vec![Issue::unsupported(
                        "predicted effect, counterexample, acceptance and spec reference are required before admission",
                    )],
                    next_check,
                });
            };
            let bounded = board_hypothesis::BoundedHypothesis::try_from_draft(
                board_hypothesis::HypothesisDraft {
                    mechanism,
                    conditions,
                    observation,
                    predicted,
                    counterexample,
                    acceptance,
                    spec,
                    basis,
                },
            )
            .map_err(|error| Failure {
                issues: vec![Issue::unsupported(error.to_string())],
                next_check,
            })?;
            Ok(Prepared::Hypothesis {
                bounded,
                removal_required,
            })
        }
    }
}

fn admit(
    bd: &Path,
    project: &Path,
    proposal: &Proposal,
    bounded: &board_hypothesis::BoundedHypothesis,
    removal_required: bool,
) -> io::Result<IntakeOutcome> {
    let admission = board_hypothesis::admit_hypothesis(bd, project, bounded, Some(&bounded.basis))?;
    let evidence = proposal
        .evidence
        .iter()
        .map(|reference| reference.locator.trim().to_owned())
        .collect::<Vec<_>>();
    match admission {
        board_hypothesis::Admission::Created { id } => Ok(IntakeOutcome::Admitted {
            workload: link_workload(bd, project, &id, proposal)?,
            id,
            evidence,
            removal_required,
        }),
        board_hypothesis::Admission::Existing { id, status } => Ok(IntakeOutcome::Existing {
            workload: link_workload(bd, project, &id, proposal)?,
            id,
            status,
        }),
        board_hypothesis::Admission::ReusedRejection {
            id,
            experiment,
            reason,
            basis,
        } => Ok(IntakeOutcome::ReusedRejection {
            id,
            experiment,
            reason,
            basis,
        }),
        board_hypothesis::Admission::ReusedInconclusive {
            id,
            experiment,
            reason,
            basis,
        } => Ok(IntakeOutcome::ReusedInconclusive {
            id,
            experiment,
            reason,
            basis,
        }),
        board_hypothesis::Admission::Reconsidered {
            id,
            basis,
            prior_outcome,
            prior_experiment,
        } => Ok(IntakeOutcome::Reconsidered {
            workload: link_workload(bd, project, &id, proposal)?,
            id,
            basis,
            prior_outcome,
            prior_experiment,
        }),
    }
}

fn link_workload(
    bd: &Path,
    project: &Path,
    candidate: &str,
    proposal: &Proposal,
) -> io::Result<Option<WorkloadLink>> {
    let Some(workload) = &proposal.workload else {
        return Ok(None);
    };
    let item = board_hypothesis::require_token("workload item", &workload.item, MAX_TOKEN)?;
    let experiment =
        board_hypothesis::require_token("workload experiment", &workload.experiment, MAX_LOCATOR)?;
    let readable = board_hypothesis::load_card(bd, project, &item)
        .ok()
        .filter(|snapshot| {
            snapshot
                .labels
                .iter()
                .any(|label| label == board_hypothesis::HYPOTHESIS_LABEL)
        });
    if readable.is_none() {
        return Ok(Some(WorkloadLink::Unlinked {
            item,
            experiment,
            reason: "the referenced workload is not a readable hypothesis card; the candidate was still admitted and the queue stays nonblocking"
                .to_owned(),
        }));
    }
    let trial = match board_hypothesis::BoundedTrial::try_from_draft(board_hypothesis::TrialDraft {
        experiment: experiment.clone(),
        role: board_hypothesis::HypothesisRole::Candidate,
        counterpart: item.clone(),
        evidence: workload.evidence.clone(),
    }) {
        Ok(trial) => trial,
        Err(error) => {
            return Ok(Some(WorkloadLink::Unlinked {
                item,
                experiment,
                reason: bounded_text(&error.to_string(), MAX_STATEMENT),
            }));
        }
    };
    match board_hypothesis::record_trial(bd, project, candidate, &trial) {
        Ok(record) => Ok(Some(WorkloadLink::Recorded {
            item,
            experiment,
            comment: record.recorded,
            related: record.related,
        })),
        Err(error) => Ok(Some(WorkloadLink::Unlinked {
            item,
            experiment,
            reason: bounded_text(&error.to_string(), MAX_STATEMENT),
        })),
    }
}

fn validate_repeated_read(
    claim: &RepeatedReadClaim,
    index: &EvidenceIndex,
    issues: &mut Vec<Issue>,
) {
    let _ = bounded_line(
        "repeated-read operation",
        &claim.operation,
        MAX_STATEMENT,
        issues,
    );
    let locator = claim.evidence.trim();
    match index.resolve(locator) {
        None => issues.push(Issue::unsupported(format!(
            "the repeated-read evidence {locator} is not retained in the supplied evidence index"
        ))),
        Some(item) if item.kind != ClaimKind::Observed => issues.push(Issue::unsupported(format!(
            "the repeated-read evidence {locator} is an inference; structured observed identity is required"
        ))),
        Some(item) if item.is_partial() => issues.push(Issue::missing(
            format!(
                "the repeated-read evidence {locator} is partial: {}",
                item.errors.join("; ")
            ),
            "retain the read identities from evidence without recorded deficits and resubmit",
        )),
        Some(_) => {}
    }
    let first = read_identity("first", &claim.first, issues);
    let second = read_identity("second", &claim.second, issues);
    let (Some(first), Some(second)) = (&first, &second) else {
        return;
    };
    let Some(item) = index.resolve(locator) else {
        return;
    };
    // The claim's tokens are assertions; the conclusion must come from the
    // retained structured read facts the cited evidence owner captured.
    if item.reads.len() < 2 {
        issues.push(Issue::missing(
            format!(
                "no two retained structured read facts for {} are recorded on {locator}",
                claim.operation.trim()
            ),
            "have the read owner capture both reads (native file identity, content digest and context) and retain them on the evidence item, then resubmit",
        ));
        return;
    }
    let matching = |read: &RetainedRead, identity: &(String, String, String)| {
        read.file == identity.0 && read.content == identity.1 && read.context == identity.2
    };
    let supported = item.reads.iter().enumerate().any(|(left_index, left)| {
        item.reads
            .iter()
            .skip(left_index + 1)
            .any(|right| left == right && matching(left, first) && matching(right, second))
    });
    if supported {
        return;
    }
    let retained_pair = item.reads.iter().enumerate().any(|(left_index, left)| {
        item.reads
            .iter()
            .skip(left_index + 1)
            .any(|right| left == right)
    });
    let first_retained = item.reads.iter().any(|read| matching(read, first));
    let second_retained = item.reads.iter().any(|read| matching(read, second));
    let operation = claim.operation.trim();
    if !retained_pair {
        issues.push(Issue::unsupported(format!(
            "the retained read facts for {operation} record no two reads with the same file, content and context identity on {locator}; a repetition across changed identity is not avoidable rereading"
        )));
    } else if !(first_retained && second_retained) {
        issues.push(Issue::unsupported(format!(
            "the claimed read identities do not match the retained read facts for {operation} on {locator}; investigator-supplied tokens alone do not establish the repetition"
        )));
    } else {
        issues.push(Issue::unsupported(format!(
            "the retained read facts for {operation} do not record two distinct matching reads on {locator}; the claimed repetition is not supported"
        )));
    }
}

fn read_identity(
    side: &str,
    identity: &ReadIdentity,
    issues: &mut Vec<Issue>,
) -> Option<(String, String, String)> {
    let file = identity_value(&format!("{side} file"), &identity.file, issues)?;
    let content = identity_value(&format!("{side} content"), &identity.content, issues)?;
    let context = identity_value(&format!("{side} context"), &identity.context, issues)?;
    Some((file, content, context))
}

fn identity_value(name: &str, value: &str, issues: &mut Vec<Issue>) -> Option<String> {
    match board_hypothesis::require_token(name, value, MAX_LOCATOR) {
        Ok(token) if token.eq_ignore_ascii_case("unknown") => {
            issues.push(Issue::missing(
                format!("the repeated-read {name} identity is unknown"),
                "capture the structured file, content and context identity of both reads from an existing reader before treating the repetition as avoidable",
            ));
            None
        }
        Ok(token) => Some(token),
        Err(error) => {
            issues.push(Issue::unsupported(error.to_string()));
            None
        }
    }
}

/// Validates one removal claim. Returns `true` only for a complete coverage
/// analysis; usage volume is a lead for investigation and defers.
fn validate_removal(removal: &RemovalClaim, issues: &mut Vec<Issue>) -> bool {
    let target = bounded_token("removal target", &removal.target, MAX_TOKEN, issues);
    match &removal.basis {
        RemovalBasis::UsageVolume {
            invocations,
            window,
        } => {
            let window = bounded_line("usage window", window, MAX_STATEMENT, issues);
            issues.push(Issue::missing(
                format!(
                    "removal of {} rests on usage volume ({invocations} invocation(s) over {}); low or absent use is a lead for investigation, not a finding of uselessness, and removal needs the user's informed decision",
                    target.as_deref().unwrap_or("the claimed target"),
                    window.as_deref().unwrap_or("an unrecorded window")
                ),
                "record the observation interval, task/environment coverage, supported rare, explicit and indirect uses, lost scenarios and a restoration route in a removal proposal before dependent work",
            ));
            false
        }
        RemovalBasis::Coverage {
            interval,
            tasks,
            gaps,
            lost_uses,
            restoration,
        } => {
            let _ = bounded_line("coverage interval", interval, MAX_STATEMENT, issues);
            let _ = bounded_line("covered tasks", tasks, MAX_STATEMENT, issues);
            let _ = bounded_line("coverage gaps", gaps, MAX_STATEMENT, issues);
            let _ = bounded_line("lost uses", lost_uses, MAX_STATEMENT, issues);
            let _ = bounded_line("restoration", restoration, MAX_STATEMENT, issues);
            true
        }
    }
}

fn bounded_token(name: &str, value: &str, max: usize, issues: &mut Vec<Issue>) -> Option<String> {
    match board_hypothesis::require_token(name, value, max) {
        Ok(token) => Some(token),
        Err(error) => {
            issues.push(Issue::unsupported(error.to_string()));
            None
        }
    }
}

fn bounded_line(name: &str, value: &str, max: usize, issues: &mut Vec<Issue>) -> Option<String> {
    match board_hypothesis::require_line(name, value, max) {
        Ok(line) => Some(line),
        Err(error) => {
            issues.push(Issue::unsupported(error.to_string()));
            None
        }
    }
}

fn bounded_statement(
    name: &str,
    value: Option<&str>,
    required: bool,
    issues: &mut Vec<Issue>,
) -> Option<String> {
    match value {
        Some(value) => bounded_line(name, value, MAX_STATEMENT, issues),
        None if required => {
            issues.push(Issue::unsupported(format!(
                "{name} is required before admission"
            )));
            None
        }
        None => None,
    }
}

fn bounded_text(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = 0;
    for character in value.chars() {
        let next = end + character.len_utf8();
        if next > max {
            break;
        }
        end = next;
    }
    format!("{}...", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path that cannot lead to a board: refusal, deferral, idle and
    /// concluded no-change/reuse outcomes must not touch it.
    fn unowned_board() -> (&'static Path, &'static Path) {
        (Path::new("no-such-bd"), Path::new("no-such-project"))
    }

    fn evidence(locator: &str, kind: ClaimKind, coverage: &str) -> EvidenceItem {
        EvidenceItem::new(locator, EvidenceOwner::Outcome, kind, coverage, &[], &[]).unwrap()
    }

    fn index() -> EvidenceIndex {
        EvidenceIndex::new(vec![
            evidence(
                "outcome:cycle-1#task",
                ClaimKind::Observed,
                "rounds=2 attempts=1",
            ),
            evidence(
                "outcome:cycle-1#interval",
                ClaimKind::Inferred,
                "derived interval cost",
            ),
        ])
        .unwrap()
    }

    fn addition(mechanism: &str) -> Proposal {
        Proposal {
            mechanism: mechanism.to_owned(),
            conditions: "local-tool-runs".to_owned(),
            observation: "outcome:cycle-1#task".to_owned(),
            predicted: Some("less repeated context loading".to_owned()),
            counterexample: Some("diagnostics vanish on failure".to_owned()),
            acceptance: Some("the independent oracle passes".to_owned()),
            spec: Some("openspec/changes/add-synthetic".to_owned()),
            basis: "outcome:cycle-1#task".to_owned(),
            treatment: Treatment::Addition,
            evidence: vec![EvidenceRef {
                locator: "outcome:cycle-1#task".to_owned(),
                kind: ClaimKind::Observed,
            }],
            repeated_read: None,
            workload: None,
            next_check: None,
        }
    }

    fn report(candidate: Proposal) -> InvestigatorReport {
        InvestigatorReport {
            schema: INTAKE_SCHEMA,
            candidates: vec![candidate],
            idle_reason: None,
        }
    }

    #[test]
    fn empty_report_is_idle_without_touching_the_board() {
        let (bd, project) = unowned_board();
        let outcomes = intake(
            bd,
            project,
            &InvestigatorReport {
                schema: INTAKE_SCHEMA,
                candidates: Vec::new(),
                idle_reason: None,
            },
            &EvidenceIndex::default(),
        )
        .unwrap();
        assert_eq!(outcomes.outcomes.len(), 1);
        let IntakeOutcome::Idle { reason } = &outcomes.outcomes[0] else {
            panic!("expected idle, got {:?}", outcomes.outcomes[0]);
        };
        assert!(reason.contains("grounded candidate"));
    }

    #[test]
    fn missing_contract_parts_refuse_before_any_board_access() {
        let (bd, project) = unowned_board();
        let mut candidate = addition("bounded-output");
        candidate.mechanism = "  ".to_owned();
        candidate.counterexample = None;
        candidate.acceptance = None;
        let outcomes = intake(bd, project, &report(candidate), &index()).unwrap();
        let IntakeOutcome::Refused { reasons } = &outcomes.outcomes[0] else {
            panic!("expected refusal, got {:?}", outcomes.outcomes[0]);
        };
        assert!(
            reasons.iter().any(|reason| reason.contains("mechanism")),
            "{reasons:?}"
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("acceptance is required")),
            "{reasons:?}"
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("counterexample is required")),
            "{reasons:?}"
        );

        let mut ungrounded = addition("bounded-output");
        ungrounded.evidence = Vec::new();
        let outcomes = intake(bd, project, &report(ungrounded), &index()).unwrap();
        let IntakeOutcome::Refused { reasons } = &outcomes.outcomes[0] else {
            panic!("expected refusal, got {:?}", outcomes.outcomes[0]);
        };
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("at least one retained evidence reference")),
            "{reasons:?}"
        );
    }

    #[test]
    fn unretained_evidence_and_strength_overclaims_refuse() {
        let (bd, project) = unowned_board();
        let mut candidate = addition("bounded-output");
        candidate.evidence = vec![EvidenceRef {
            locator: "outcome:cycle-1#missing".to_owned(),
            kind: ClaimKind::Observed,
        }];
        let outcomes = intake(bd, project, &report(candidate), &index()).unwrap();
        let IntakeOutcome::Refused { reasons } = &outcomes.outcomes[0] else {
            panic!("expected refusal, got {:?}", outcomes.outcomes[0]);
        };
        assert!(
            reasons.iter().any(|reason| reason.contains("not retained")),
            "{reasons:?}"
        );

        let mut overclaim = addition("bounded-output");
        overclaim.evidence = vec![EvidenceRef {
            locator: "outcome:cycle-1#interval".to_owned(),
            kind: ClaimKind::Observed,
        }];
        overclaim.observation = "outcome:cycle-1#interval".to_owned();
        let outcomes = intake(bd, project, &report(overclaim), &index()).unwrap();
        let IntakeOutcome::Refused { reasons } = &outcomes.outcomes[0] else {
            panic!("expected refusal, got {:?}", outcomes.outcomes[0]);
        };
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("cannot be stronger")),
            "{reasons:?}"
        );
    }

    #[test]
    fn predictions_cannot_be_cited_as_evidence() {
        let (bd, project) = unowned_board();
        let mut candidate = addition("bounded-output");
        candidate.evidence = vec![EvidenceRef {
            locator: "outcome:cycle-1#task".to_owned(),
            kind: ClaimKind::Predicted,
        }];
        let outcomes = intake(bd, project, &report(candidate), &index()).unwrap();
        let IntakeOutcome::Refused { reasons } = &outcomes.outcomes[0] else {
            panic!("expected refusal, got {:?}", outcomes.outcomes[0]);
        };
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("predictions are not retained evidence")),
            "{reasons:?}"
        );
    }

    fn read(file: &str, content: &str, context: &str) -> RetainedRead {
        RetainedRead {
            file: file.to_owned(),
            content: content.to_owned(),
            context: context.to_owned(),
        }
    }

    fn reads_item(locator: &str, reads: Vec<RetainedRead>) -> EvidenceItem {
        EvidenceItem::new(
            locator,
            EvidenceOwner::Source,
            ClaimKind::Observed,
            "retained_reads",
            &[],
            &[],
        )
        .unwrap()
        .with_reads(reads)
        .unwrap()
    }

    fn read_claim(evidence: &str, first: &str, second: &str) -> RepeatedReadClaim {
        RepeatedReadClaim {
            operation: "read:src/lib.rs".to_owned(),
            evidence: evidence.to_owned(),
            first: ReadIdentity {
                file: "file:a".to_owned(),
                content: first.to_owned(),
                context: "context:task-1".to_owned(),
            },
            second: ReadIdentity {
                file: "file:a".to_owned(),
                content: second.to_owned(),
                context: "context:task-1".to_owned(),
            },
        }
    }

    #[test]
    fn repeated_read_claims_require_matching_retained_facts() {
        let (bd, project) = unowned_board();

        // A complete generic item carries no read facts: equal claimed tokens
        // still cannot establish the repetition.
        let mut generic = addition("avoid-repeated-read");
        generic.repeated_read = Some(read_claim(
            "outcome:cycle-1#task",
            "digest-aaa",
            "digest-aaa",
        ));
        let outcomes = intake(bd, project, &report(generic), &index()).unwrap();
        let IntakeOutcome::Deferred { reason, next } = &outcomes.outcomes[0] else {
            panic!("expected deferral, got {:?}", outcomes.outcomes[0]);
        };
        assert!(
            reason.contains("no two retained structured read facts"),
            "{reason}"
        );
        assert!(next.contains("capture both reads"), "{next}");

        // Retained facts that differ in content refuse even when the claim
        // repeats fabricated equal tokens.
        let changed = EvidenceIndex::new(vec![
            evidence(
                "outcome:cycle-1#task",
                ClaimKind::Observed,
                "rounds=2 attempts=1",
            ),
            reads_item(
                "source:lib#reads",
                vec![
                    read("file:a", "digest-aaa", "context:task-1"),
                    read("file:a", "digest-bbb", "context:task-1"),
                ],
            ),
        ])
        .unwrap();
        let mut fabricated = addition("avoid-repeated-read");
        fabricated.repeated_read = Some(read_claim("source:lib#reads", "digest-aaa", "digest-aaa"));
        let outcomes = intake(bd, project, &report(fabricated), &changed).unwrap();
        let IntakeOutcome::Refused { reasons } = &outcomes.outcomes[0] else {
            panic!("expected refusal, got {:?}", outcomes.outcomes[0]);
        };
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("record no two reads with the same")),
            "{reasons:?}"
        );

        // A claim that does not match the retained facts is a mismatch, not
        // observed support.
        let matching = EvidenceIndex::new(vec![
            evidence(
                "outcome:cycle-1#task",
                ClaimKind::Observed,
                "rounds=2 attempts=1",
            ),
            reads_item(
                "source:lib#reads",
                vec![
                    read("file:a", "digest-aaa", "context:task-1"),
                    read("file:a", "digest-aaa", "context:task-1"),
                ],
            ),
        ])
        .unwrap();
        let mut mismatch = addition("avoid-repeated-read");
        mismatch.repeated_read = Some(read_claim("source:lib#reads", "digest-ccc", "digest-ccc"));
        let outcomes = intake(bd, project, &report(mismatch), &matching).unwrap();
        let IntakeOutcome::Refused { reasons } = &outcomes.outcomes[0] else {
            panic!("expected refusal, got {:?}", outcomes.outcomes[0]);
        };
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("do not match the retained read facts")),
            "{reasons:?}"
        );

        // Two retained reads with the same identity and a matching claim pass
        // grounding; the owned-board admission is covered by the integration
        // test.
        let mut supported = addition("avoid-repeated-read");
        supported.repeated_read = Some(read_claim("source:lib#reads", "digest-aaa", "digest-aaa"));
        assert!(matches!(
            prepare(&supported, &matching),
            Ok(Prepared::Hypothesis { .. })
        ));
    }

    #[test]
    fn unknown_repeated_read_identity_defers() {
        let (bd, project) = unowned_board();
        let index = EvidenceIndex::new(vec![
            evidence(
                "outcome:cycle-1#task",
                ClaimKind::Observed,
                "rounds=2 attempts=1",
            ),
            reads_item(
                "source:lib#reads",
                vec![
                    read("file:a", "digest-aaa", "context:task-1"),
                    read("file:a", "digest-aaa", "context:task-1"),
                ],
            ),
        ])
        .unwrap();
        let mut unknown = addition("avoid-repeated-read");
        unknown.repeated_read = Some(read_claim("source:lib#reads", "digest-aaa", "unknown"));
        let outcomes = intake(bd, project, &report(unknown), &index).unwrap();
        let IntakeOutcome::Deferred { reason, next } = &outcomes.outcomes[0] else {
            panic!("expected deferral, got {:?}", outcomes.outcomes[0]);
        };
        assert!(reason.contains("identity is unknown"), "{reason}");
        assert!(next.contains("structured"), "{next}");
    }

    #[test]
    fn read_source_captures_native_identity_and_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("observed.txt");
        std::fs::write(&path, "observed contents\n").unwrap();
        let item = EvidenceItem::read_source("source:observed#1", "context:task-1", &path).unwrap();
        assert_eq!(item.owner, EvidenceOwner::Source);
        assert_eq!(item.kind, ClaimKind::Observed);
        assert_eq!(item.reads.len(), 1);
        assert!(item.reads[0].content.starts_with("sha256."));
        assert!(item.reads[0].file.starts_with("file:v"));
        assert_eq!(item.reads[0].context, "context:task-1");
        let repeat =
            EvidenceItem::read_source("source:observed#2", "context:task-1", &path).unwrap();
        assert_eq!(
            item.reads[0], repeat.reads[0],
            "an unchanged file captures the same read identity"
        );
        std::fs::write(&path, "different contents\n").unwrap();
        let changed =
            EvidenceItem::read_source("source:observed#3", "context:task-1", &path).unwrap();
        assert_ne!(item.reads[0].content, changed.reads[0].content);
    }

    #[test]
    fn usage_volume_removal_defers_to_coverage_investigation() {
        let (bd, project) = unowned_board();
        let mut candidate = addition("retire-dormant-helper");
        candidate.treatment = Treatment::Subtraction {
            removal: RemovalClaim {
                target: "dormant-helper".to_owned(),
                basis: RemovalBasis::UsageVolume {
                    invocations: 0,
                    window: "90d".to_owned(),
                },
            },
        };
        let outcomes = intake(bd, project, &report(candidate), &index()).unwrap();
        let IntakeOutcome::Deferred { reason, next } = &outcomes.outcomes[0] else {
            panic!("expected deferral, got {:?}", outcomes.outcomes[0]);
        };
        assert!(reason.contains("lead for investigation"), "{reason}");
        assert!(next.contains("removal proposal"), "{next}");
    }

    #[test]
    fn grounded_no_change_and_reuse_conclude_without_a_board() {
        let (bd, project) = unowned_board();
        let mut no_change = addition("keep-current-route");
        no_change.treatment = Treatment::NoChange {
            reason: "the measured burden is within the declared variation".to_owned(),
        };
        let outcomes = intake(bd, project, &report(no_change), &index()).unwrap();
        assert!(
            matches!(&outcomes.outcomes[0], IntakeOutcome::NoChange { reason } if reason.contains("declared variation")),
            "{:?}",
            outcomes.outcomes[0]
        );

        let mut reuse = addition("reuse-existing-route");
        reuse.treatment = Treatment::Reuse {
            existing: "outcome:cycle-1#task".to_owned(),
        };
        let outcomes = intake(bd, project, &report(reuse), &index()).unwrap();
        assert!(
            matches!(&outcomes.outcomes[0], IntakeOutcome::ReuseSuffices { existing } if existing == "outcome:cycle-1#task"),
            "{:?}",
            outcomes.outcomes[0]
        );
    }

    #[test]
    fn evidence_items_stay_bounded_and_reject_predictions() {
        assert!(
            EvidenceItem::new(
                "outcome:cycle-1#task",
                EvidenceOwner::Outcome,
                ClaimKind::Predicted,
                "rounds=2",
                &[],
                &[],
            )
            .is_err()
        );
        assert!(
            EvidenceItem::new(
                "outcome:cycle-1#task",
                EvidenceOwner::Outcome,
                ClaimKind::Observed,
                "",
                &[],
                &[],
            )
            .is_err()
        );
        let duplicate = vec![
            evidence("outcome:cycle-1#task", ClaimKind::Observed, "rounds=2"),
            evidence("outcome:cycle-1#task", ClaimKind::Observed, "rounds=3"),
        ];
        assert!(EvidenceIndex::new(duplicate).is_err());
        assert_eq!(ClaimKind::parse("observed"), Some(ClaimKind::Observed));
        assert_eq!(ClaimKind::parse("predicted"), Some(ClaimKind::Predicted));
        assert!(ClaimKind::parse("assumed").is_none());
    }
}
