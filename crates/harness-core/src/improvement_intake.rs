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
//!   coverage/lost-use/consumption/restoration basis, usage volume alone
//!   defers to further investigation (invocation counts and catalogue or
//!   context exposure are distinct), and this module never applies a removal -
//!   the user's informed decision remains owned by the removal
//!   proposal/decision verbs;
//! - additional machinery is never the default: an addition is refused unless
//!   it records why no change, reuse of the smallest sufficient existing
//!   route, simplification and subtraction cannot satisfy the evidenced need,
//!   and reuse/no-change/removal conclusions are offered before admission;
//! - every treatment that selects an experiment declares its selection before
//!   dependent work - the effect path and claim, the required outcome, the
//!   smallest sufficient real unit/method with its applicability rationale,
//!   the controls, the projected use/cost, the admissible baseline basis and
//!   the stopping/escalation/deferral rules - and the declared unit must
//!   exercise the declared claim: a fixed command or retained replay never
//!   stands in for an unexercised agent, a broad strategy claim needs complete
//!   paired implementations, a repeated-use claim needs the sequence and
//!   state, and fewer lines, files or exposed names never establish benefit. A
//!   costly, low-value measurement is deferred with the missing fact and the
//!   reconsideration condition instead of running or being adopted;
//! - review reuses the existing owners instead of commissioning audits: usage
//!   and outcome records stay attributed to their reader, a coverage claim
//!   names the observed interval, task/environment mix, telemetry gaps and
//!   rare/explicit/indirect uses at risk, and low usage is a lead for
//!   investigation - never a finding of uselessness or an automatic deletion;
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

use crate::{
    board_hypothesis, build_identity, improvement_policy::ExperimentSelection, rollout_reader,
};
use serde::{Deserialize, Serialize, de::IgnoredAny};
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
pub const MAX_STATEMENT: usize = 2048;
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
        /// How each arm's actual consumption of the removed burden
        /// (invocation or catalogue/instruction/initialization exposure) is
        /// evidenced separately from invocation counts. A missing or blank
        /// value is refused by intake validation; older records stay readable
        /// so the gap is reported instead of silently ignored.
        #[serde(default)]
        consumption: String,
    },
}

/// One claimed removal target and its basis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovalClaim {
    pub target: String,
    pub basis: RemovalBasis,
}

/// The declared deferral of one selected experiment: the sufficient
/// experiment is not worth its cost, so it is deferred with the missing fact
/// and the condition under which the measurement is reconsidered. A deferral
/// never adopts without support and never describes a cheaper probe as the
/// result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionDeferral {
    /// The decision-changing fact the deferred measurement could not obtain.
    pub missing_fact: String,
    /// The condition under which the deferred measurement is reconsidered.
    pub reconsideration: String,
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
    /// Why no change, reuse of the smallest sufficient existing route,
    /// simplification or subtraction cannot satisfy the evidenced need.
    /// Required for an addition, so additional machinery is proposed only
    /// after the smaller routes were considered; a no-change, reuse or
    /// removal candidate states its conclusion instead.
    #[serde(default)]
    pub alternatives: Option<String>,
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
    /// The declared experiment selection: the effect path and claim, the
    /// required outcome, the selected experimental unit/method, its
    /// applicability rationale, the controls, the projected use/cost, the
    /// admissible baseline basis and the stopping/escalation/deferral rules.
    /// Required for every treatment that selects an experiment, so the
    /// smallest sufficient real unit is declared before dependent work.
    #[serde(default)]
    pub selection: Option<ExperimentSelection>,
    /// A declared deferral for a sufficient experiment whose cost is not
    /// worth its decision value; the candidate is deferred instead of run.
    #[serde(default)]
    pub deferral: Option<SelectionDeferral>,
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

/// Reads one bounded investigator report from a file. This is the strict
/// reader contract: the whole file is one JSON document. A retained terminal
/// message that frames the report in prose goes through
/// [`parse_terminal_report`] instead.
pub fn read_report(path: &Path) -> io::Result<InvestigatorReport> {
    crate::improvement_loop::read_json(path, MAX_REPORT_BYTES)
}

/// Parses one investigator report from a retained terminal message: either
/// the strict whole-message JSON document, or prose framing followed by
/// exactly one complete JSON report as the message's final payload. The
/// payload must start a line; a message with no parseable payload, with
/// trailing non-framing text after it, with more than one complete JSON
/// payload or with a payload that is not the schema-1 report shape is
/// refused as a whole. Nothing is salvaged from the framing prose, and the
/// payload is deserialized by the same strict contract as [`read_report`].
pub fn parse_terminal_report(bytes: &[u8]) -> io::Result<InvestigatorReport> {
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err(board_hypothesis::invalid(format!(
            "the terminal message is {} bytes, beyond the {MAX_REPORT_BYTES} byte investigator report bound",
            bytes.len()
        )));
    }
    // The strict reader contract stays first: a whole-message JSON document
    // needs no framing, and trailing non-whitespace still fails it.
    if let Ok(report) = serde_json::from_slice::<InvestigatorReport>(bytes) {
        return Ok(report);
    }
    // Terminal framing: the payload is the one complete JSON object that
    // begins a line in the message. Any other complete JSON object (even a
    // non-report one) makes the message ambiguous and is never skipped.
    let mut payloads = Vec::new();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte != b'{' || (index > 0 && bytes[index - 1] != b'\n') {
            continue;
        }
        let mut deserializer = serde_json::Deserializer::from_slice(&bytes[index..]);
        if IgnoredAny::deserialize(&mut deserializer).is_err() {
            continue;
        }
        payloads.push((index, deserializer.end().is_ok()));
    }
    match payloads.as_slice() {
        [] => Err(board_hypothesis::invalid(
            "the terminal message carries no complete JSON payload that starts a line; a missing, malformed or incomplete investigator report is not consumed",
        )),
        [(index, true)] => {
            serde_json::from_slice::<InvestigatorReport>(&bytes[*index..]).map_err(|error| {
                board_hypothesis::invalid(format!(
                    "the terminal JSON payload is not a schema-1 investigator report: {error}"
                ))
            })
        }
        [(_, false)] => Err(board_hypothesis::invalid(
            "the terminal message carries non-whitespace after its JSON payload; trailing non-framing text makes the report ambiguous and nothing is consumed",
        )),
        _ => Err(board_hypothesis::invalid(format!(
            "the terminal message carries {} complete JSON payloads; a multiple or ambiguous investigator report is not consumed",
            payloads.len()
        ))),
    }
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
    /// The declared sufficient experiment is not worth its cost; the missing
    /// fact and reconsideration condition are carried to the caller.
    Deferred {
        reason: String,
        next: String,
    },
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
        Ok(Prepared::Deferred { reason, next }) => Ok(IntakeOutcome::Deferred { reason, next }),
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
    match proposal.alternatives.as_deref() {
        Some(value) => {
            let _ = bounded_line(
                "alternatives consideration",
                value,
                MAX_STATEMENT,
                &mut issues,
            );
        }
        None if matches!(proposal.treatment, Treatment::Addition) => {
            issues.push(Issue::unsupported(
                "an addition needs an alternatives consideration: state why no change, reuse of the smallest sufficient existing route, simplification or subtraction cannot satisfy the evidenced need before proposing additional machinery",
            ));
        }
        None => {}
    }
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

    // The experiment-selection procedure is declared before dependent work:
    // the claimed effect path, the required outcome, the smallest sufficient
    // real unit/method with its applicability rationale, the controls, the
    // projected use/cost, the admissible baseline basis and the
    // stopping/escalation/deferral rules. Admission checks that the declared
    // unit can exercise the declared claim; a fixed command or retained replay
    // never stands in for an unexercised agent, a broad strategy claim needs
    // complete paired implementations, a repeated-use claim needs the
    // sequence and state, and fewer lines or files alone establish nothing.
    match (full, proposal.selection.as_ref()) {
        (true, Some(selection)) => {
            if let Some(problem) = selection.problem() {
                issues.push(Issue::unsupported(problem));
            }
        }
        (true, None) => issues.push(Issue::unsupported(
            "an experiment-selection declaration is required before admission: the effect path and claim, the required outcome, the experimental unit/method with its applicability rationale, the controls, the projected use/cost, the admissible baseline basis and the stopping/escalation/deferral rules",
        )),
        (false, _) => {}
    }
    // A costly, low-value measurement is deferred with the missing fact and
    // the reconsideration condition: the loop continues other eligible work
    // instead of running an unjustified experiment, adopting without support
    // or repeating an identically inconclusive one.
    let mut deferred: Option<(String, String)> = None;
    if let Some(declared) = &proposal.deferral {
        if !full {
            issues.push(Issue::unsupported(
                "a deferral belongs to a candidate that selects an experiment; a no-change or reuse conclusion runs none",
            ));
        } else {
            let missing = bounded_line(
                "deferral missing fact",
                &declared.missing_fact,
                MAX_STATEMENT,
                &mut issues,
            );
            let reconsideration = bounded_line(
                "deferral reconsideration",
                &declared.reconsideration,
                MAX_STATEMENT,
                &mut issues,
            );
            if let (Some(missing), Some(reconsideration)) = (missing, reconsideration) {
                deferred = Some((
                    format!("the selected experiment is deferred as not worth its cost: {missing}"),
                    reconsideration,
                ));
            }
        }
    }

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
            if let Some((reason, next)) = deferred {
                return Ok(Prepared::Deferred { reason, next });
            }
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
                    "removal of {} rests on usage volume ({invocations} invocation(s) over {}); low or absent use is a lead for investigation, not a finding of uselessness (invocation counts are not consumption: catalogue, instruction and initialization cost can exist with zero invocations), and removal needs the user's informed decision",
                    target.as_deref().unwrap_or("the claimed target"),
                    window.as_deref().unwrap_or("an unrecorded window")
                ),
                "record the observation interval, task/environment coverage, supported rare, explicit and indirect uses, how actual consumption of the removed burden is evidenced in each arm, lost scenarios and a restoration route in a removal proposal before dependent work",
            ));
            false
        }
        RemovalBasis::Coverage {
            interval,
            tasks,
            gaps,
            lost_uses,
            restoration,
            consumption,
        } => {
            let _ = bounded_line("coverage interval", interval, MAX_STATEMENT, issues);
            let _ = bounded_line("covered tasks", tasks, MAX_STATEMENT, issues);
            let _ = bounded_line("coverage gaps", gaps, MAX_STATEMENT, issues);
            let _ = bounded_line("lost uses", lost_uses, MAX_STATEMENT, issues);
            let _ = bounded_line("restoration", restoration, MAX_STATEMENT, issues);
            let _ = bounded_line("consumption evidence", consumption, MAX_STATEMENT, issues);
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
            alternatives: Some(
                "no change, reuse of the existing reader, simplification and subtraction each leave the measured burden in place"
                    .to_owned(),
            ),
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
            selection: Some(ExperimentSelection {
                method: crate::improvement_policy::ExperimentMethod::RealOperation,
                claim: crate::improvement_policy::EffectPath::LocalOperation,
                outcome: "the declared outcome measured through the real operation".to_owned(),
                rationale: "the unit exercises the claimed mechanism under the declared conditions"
                    .to_owned(),
                controls: "frozen inputs and the accepted baseline conditions".to_owned(),
                projection: "one bounded local cycle with the retention cost staying bounded".to_owned(),
                baseline: "the accepted revision, excluding the candidate edit".to_owned(),
                stopping:
                    "stop after the declared attempts and escalate only for a named missing observation"
                        .to_owned(),
            }),
            deferral: None,
        }
    }

    fn report(candidate: Proposal) -> InvestigatorReport {
        InvestigatorReport {
            schema: INTAKE_SCHEMA,
            candidates: vec![candidate],
            idle_reason: None,
        }
    }

    fn serialized(candidate: Proposal, pretty: bool) -> Vec<u8> {
        let report = report(candidate);
        if pretty {
            serde_json::to_vec_pretty(&report).unwrap()
        } else {
            serde_json::to_vec(&report).unwrap()
        }
    }

    #[test]
    fn terminal_reports_accept_strict_json_and_one_framed_payload() {
        // The strict whole-message contract stays accepted, compact or pretty.
        for pretty in [false, true] {
            let message = serialized(addition("bounded-output"), pretty);
            assert_eq!(parse_terminal_report(&message).unwrap().candidates.len(), 1);
        }
        // Investigator prose framing around exactly one final payload is
        // consumed through the same strict report contract.
        for pretty in [false, true] {
            let mut message =
                b"First paragraph of investigator prose.\n\nSecond paragraph.\n\n".to_vec();
            message.extend_from_slice(&serialized(addition("bounded-output"), pretty));
            message.push(b'\n');
            assert_eq!(parse_terminal_report(&message).unwrap().candidates.len(), 1);
        }
    }

    #[test]
    fn terminal_reports_refuse_absent_ambiguous_or_trailing_payloads() {
        assert!(
            parse_terminal_report(b"prose only\n").is_err(),
            "no payload"
        );
        let payload = serialized(addition("bounded-output"), false);

        let mut malformed = b"prose\n\n{\"schema\":1,".to_vec();
        malformed.extend_from_slice(&payload);
        assert!(parse_terminal_report(&malformed).is_err(), "malformed");

        let mut truncated = b"prose\n\n".to_vec();
        truncated.extend_from_slice(&payload[..payload.len() - 3]);
        assert!(parse_terminal_report(&truncated).is_err(), "incomplete");

        let mut ambiguous = b"prose\n\n".to_vec();
        ambiguous.extend_from_slice(&payload);
        ambiguous.push(b'\n');
        ambiguous.extend_from_slice(&payload);
        ambiguous.push(b'\n');
        let error = parse_terminal_report(&ambiguous).unwrap_err();
        assert!(
            error.to_string().contains("complete JSON payloads"),
            "{error}"
        );

        let mut trailing = b"prose\n\n".to_vec();
        trailing.extend_from_slice(&payload);
        trailing.extend_from_slice(b"\nnot framing\n");
        let error = parse_terminal_report(&trailing).unwrap_err();
        assert!(error.to_string().contains("trailing"), "{error}");

        assert!(
            parse_terminal_report(b"prose\n\n{\"not\":\"a report\"}\n").is_err(),
            "a JSON object that is not the report is refused"
        );
    }

    #[test]
    fn terminal_payload_with_wrong_schema_is_refused_by_intake() {
        let mut message = b"prose\n\n".to_vec();
        let mut document = report(addition("bounded-output"));
        document.schema = INTAKE_SCHEMA + 1;
        message.extend_from_slice(&serde_json::to_vec(&document).unwrap());
        let parsed = parse_terminal_report(&message).unwrap();
        let (bd, project) = unowned_board();
        let error = intake(bd, project, &parsed, &EvidenceIndex::default()).unwrap_err();
        assert!(error.to_string().contains("declares schema"), "{error}");
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
    fn removal_coverage_needs_consumption_evidence() {
        let (bd, project) = unowned_board();
        let mut candidate = addition("retire-dormant-helper");
        candidate.treatment = Treatment::Subtraction {
            removal: RemovalClaim {
                target: "dormant-helper".to_owned(),
                basis: RemovalBasis::Coverage {
                    interval: "180d".to_owned(),
                    tasks: "all recorded synthetic tasks".to_owned(),
                    gaps: "none observed".to_owned(),
                    lost_uses: "manual fallback remains available".to_owned(),
                    restoration: "restore from the retained source revision".to_owned(),
                    consumption: "  ".to_owned(),
                },
            },
        };
        let outcomes = intake(bd, project, &report(candidate), &index()).unwrap();
        assert!(
            matches!(&outcomes.outcomes[0], IntakeOutcome::Refused { reasons } if reasons.iter().any(|reason| reason.contains("consumption evidence is required"))),
            "{:?}",
            outcomes.outcomes[0]
        );
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
    fn an_addition_is_refused_without_the_smaller_route_consideration() {
        let (bd, project) = unowned_board();
        let mut candidate = addition("bounded-output");
        candidate.alternatives = None;
        let outcomes = intake(bd, project, &report(candidate), &index()).unwrap();
        let IntakeOutcome::Refused { reasons } = &outcomes.outcomes[0] else {
            panic!("expected refusal, got {:?}", outcomes.outcomes[0]);
        };
        let joined = reasons.join(" ");
        for needle in [
            "alternatives consideration",
            "no change",
            "reuse",
            "simplification",
            "subtraction",
        ] {
            assert!(joined.contains(needle), "{needle}: {reasons:?}");
        }

        let mut blank = addition("bounded-output");
        blank.alternatives = Some("   ".to_owned());
        let outcomes = intake(bd, project, &report(blank), &index()).unwrap();
        assert!(
            matches!(&outcomes.outcomes[0], IntakeOutcome::Refused { reasons } if reasons.iter().any(|reason| reason.contains("alternatives consideration"))),
            "{:?}",
            outcomes.outcomes[0]
        );

        // The smaller treatments carry their own conclusion instead of an
        // alternatives statement: a no-change reason, a retained reuse route
        // or a removal claim; none is refused for lacking the field.
        let mut no_change = addition("keep-current-route");
        no_change.alternatives = None;
        no_change.treatment = Treatment::NoChange {
            reason: "the measured burden is within the declared variation".to_owned(),
        };
        assert!(matches!(
            prepare(&no_change, &index()),
            Ok(Prepared::NoChange(_))
        ));

        let mut subtraction = addition("retire-dormant-helper");
        subtraction.alternatives = None;
        subtraction.treatment = Treatment::Subtraction {
            removal: RemovalClaim {
                target: "dormant-helper".to_owned(),
                basis: RemovalBasis::UsageVolume {
                    invocations: 0,
                    window: "90d".to_owned(),
                },
            },
        };
        let Err(failure) = prepare(&subtraction, &index()) else {
            panic!("the usage-volume removal defers instead of being refused");
        };
        assert!(
            !failure
                .issues
                .iter()
                .any(|issue| issue.message.contains("alternatives")),
            "{}",
            failure
                .issues
                .iter()
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
        assert!(matches!(
            failure.outcome(),
            IntakeOutcome::Deferred { reason, .. } if reason.contains("lead for investigation")
        ));
    }

    #[test]
    fn context_exposure_without_invocation_is_a_lead_not_a_finding() {
        let (bd, project) = unowned_board();
        // A skill loaded into the catalogue that consumes context on every
        // accepted task while recording zero invocations: the usage volume
        // cannot justify retirement, and this module applies no removal.
        let mut candidate = addition("retire-context-heavy-skill");
        candidate.treatment = Treatment::Subtraction {
            removal: RemovalClaim {
                target: "skill:context-heavy".to_owned(),
                basis: RemovalBasis::UsageVolume {
                    invocations: 0,
                    window: "365d".to_owned(),
                },
            },
        };
        let outcomes = intake(bd, project, &report(candidate), &index()).unwrap();
        let IntakeOutcome::Deferred { reason, next } = &outcomes.outcomes[0] else {
            panic!("expected deferral, got {:?}", outcomes.outcomes[0]);
        };
        assert!(reason.contains("lead for investigation"), "{reason}");
        assert!(reason.contains("catalogue"), "{reason}");
        assert!(reason.contains("0 invocation(s) over 365d"), "{reason}");
        assert!(next.contains("observation interval"), "{next}");
        assert!(next.contains("consumption"), "{next}");
        assert!(next.contains("rare"), "{next}");
    }

    #[test]
    fn incomplete_usage_coverage_defers_instead_of_removing() {
        let (bd, project) = unowned_board();
        // One workstation's usage records are unreadable: the retained item is
        // partial, so unresolved supported use stays explicit and removal
        // planning cannot start from it.
        let partial = EvidenceItem::new(
            "rollout:fleet#partial",
            EvidenceOwner::Rollout,
            ClaimKind::Observed,
            "lines=120 unrecognized=40; one workstation's session store was unreadable",
            &[
                "one workstation's usage records were unreadable; their use stays unknown"
                    .to_owned(),
            ],
            &[],
        )
        .unwrap();
        let partial_index = EvidenceIndex::new(vec![
            evidence(
                "outcome:cycle-1#task",
                ClaimKind::Observed,
                "rounds=2 attempts=1",
            ),
            partial,
        ])
        .unwrap();
        let coverage = |gaps: &str| {
            RemovalBasis::Coverage {
            interval: "180d".to_owned(),
            tasks: "every owned task on the covered workstations".to_owned(),
            gaps: gaps.to_owned(),
            lost_uses: "a rare manual recovery in a degraded environment".to_owned(),
            restoration: "restore from the retained revision".to_owned(),
            consumption:
                "each covered arm records the effective exposure of the capability, not just invocations"
                    .to_owned(),
        }
        };
        let mut incomplete = addition("retire-rarely-used-capability");
        incomplete.observation = "rollout:fleet#partial".to_owned();
        incomplete.basis = "rollout:fleet#partial".to_owned();
        incomplete.evidence = vec![EvidenceRef {
            locator: "rollout:fleet#partial".to_owned(),
            kind: ClaimKind::Observed,
        }];
        incomplete.treatment = Treatment::Subtraction {
            removal: RemovalClaim {
                target: "capability-x".to_owned(),
                basis: coverage(
                    "two workstations record no usage; their consumers are uncheckable",
                ),
            },
        };
        let outcomes = intake(bd, project, &report(incomplete), &partial_index).unwrap();
        let IntakeOutcome::Deferred { reason, next } = &outcomes.outcomes[0] else {
            panic!("expected deferral, got {:?}", outcomes.outcomes[0]);
        };
        assert!(reason.contains("partial"), "{reason}");
        assert!(next.contains("retain"), "{next}");

        // A blank gaps statement is refused: coverage gaps must stay explicit
        // rather than being read as absent.
        let mut hidden_gaps = addition("retire-rarely-used-capability");
        hidden_gaps.treatment = Treatment::Subtraction {
            removal: RemovalClaim {
                target: "capability-x".to_owned(),
                basis: coverage("   "),
            },
        };
        let outcomes = intake(bd, project, &report(hidden_gaps), &index()).unwrap();
        assert!(
            matches!(&outcomes.outcomes[0], IntakeOutcome::Refused { reasons } if reasons.iter().any(|reason| reason.contains("coverage gaps is required"))),
            "{:?}",
            outcomes.outcomes[0]
        );
    }

    #[test]
    fn a_rare_recovery_loss_stays_a_pending_candidate_with_restoration() {
        let coverage = |lost_uses: &str, restoration: &str| {
            RemovalBasis::Coverage {
            interval: "180d".to_owned(),
            tasks: "recorded owned tasks".to_owned(),
            gaps: "no machine-readable use from two workstations".to_owned(),
            lost_uses: lost_uses.to_owned(),
            restoration: restoration.to_owned(),
            consumption:
                "each arm records the effective catalogue identity and where the capability was consumed"
                    .to_owned(),
        }
        };
        // Ordinary-task nonuse cannot retire a still-required recovery path:
        // the candidate stays a pending hypothesis behind the informed
        // decision, and this module applies no removal.
        let mut candidate = addition("retire-rare-recovery-capability");
        candidate.alternatives = None;
        candidate.treatment = Treatment::Subtraction {
            removal: RemovalClaim {
                target: "capability-x".to_owned(),
                basis: coverage(
                    "a rare manual recovery in a degraded environment",
                    "restore from the pinned revision",
                ),
            },
        };
        assert!(
            matches!(
                prepare(&candidate, &index()),
                Ok(Prepared::Hypothesis {
                    removal_required: true,
                    ..
                })
            ),
            "the rare recovery use stays explicit with its restoration route"
        );

        // Without a restoration route the required behavior cannot be kept
        // recoverable, so the candidate is refused before admission.
        let mut unrecoverable = addition("retire-rare-recovery-capability");
        unrecoverable.alternatives = None;
        unrecoverable.treatment = Treatment::Subtraction {
            removal: RemovalClaim {
                target: "capability-x".to_owned(),
                basis: coverage("a rare manual recovery", "   "),
            },
        };
        let Err(failure) = prepare(&unrecoverable, &index()) else {
            panic!("a removal claim without a restoration route must be refused");
        };
        assert!(
            failure
                .issues
                .iter()
                .any(|issue| issue.message.contains("restoration is required")),
            "{}",
            failure
                .issues
                .iter()
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
        assert!(matches!(failure.outcome(), IntakeOutcome::Refused { .. }));
    }

    #[test]
    fn an_overlapping_route_concludes_reuse_without_a_card() {
        let (bd, project) = unowned_board();
        // The evidenced need is already satisfied by the smallest sufficient
        // existing route: intake concludes reuse instead of admitting a new
        // addition.
        let mut reuse = addition("overlap-with-existing-reader");
        reuse.treatment = Treatment::Reuse {
            existing: "outcome:cycle-1#task".to_owned(),
        };
        let outcomes = intake(bd, project, &report(reuse), &index()).unwrap();
        assert!(
            matches!(&outcomes.outcomes[0], IntakeOutcome::ReuseSuffices { existing } if existing == "outcome:cycle-1#task"),
            "{:?}",
            outcomes.outcomes[0]
        );

        // A reuse conclusion must name a retained attributable route; an
        // invented existing capability is refused.
        let mut unretained = addition("overlap-with-existing-reader");
        unretained.treatment = Treatment::Reuse {
            existing: "outcome:missing#route".to_owned(),
        };
        let outcomes = intake(bd, project, &report(unretained), &index()).unwrap();
        assert!(
            matches!(&outcomes.outcomes[0], IntakeOutcome::Refused { reasons } if reasons.iter().any(|reason| reason.contains("not retained in the evidence index"))),
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
