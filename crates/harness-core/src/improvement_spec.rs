//! Native OpenSpec prerequisites for improvement hypotheses and workloads.
//!
//! OpenSpec remains the artifact owner. These receipts bind its actual resolved
//! files to a declared initial measurement scope and experiment contract; they
//! do not certify implementation, task completion, benefit, or user removal
//! authority. The adapter also reads the change's actual completion state for
//! decision reconciliation and archives an unadopted completed change through
//! the installed CLI's supported non-synchronizing path, so a rejected delta
//! stays referencable without reaching the main specifications.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Component, Path, PathBuf},
    process::Command,
};

const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024;
/// Bound on one task description retained in a completion receipt.
const MAX_TASK_DESCRIPTION: usize = 240;
/// Bound on the main-specification files inspected for an accidental sync.
const MAX_SPEC_FILES: usize = 4096;
/// Bound on one reviewable removal-proposal clause value.
const MAX_REMOVAL_CLAUSE_BYTES: usize = 1024;

/// The exact Markdown heading under which a hypothesis's own OpenSpec change
/// states its reviewable removal proposal. The proposal is authored in the
/// change before the user's decision is requested and stays unapplied;
/// OpenSpec remains its artifact owner.
pub const REMOVAL_PROPOSAL_HEADING: &str = "## Removal proposal";

/// The clause labels a reviewable removal proposal states exactly once, each
/// non-empty and on one line: the target and source references, the unapplied
/// preview, evidence and its gaps, measured versus predicted benefit, lost
/// scenarios, consumer/configuration/installation impact, alternatives,
/// retained checks and restoration.
pub const REMOVAL_PROPOSAL_CLAUSES: [&str; 13] = [
    "Target:",
    "Source:",
    "Evidence:",
    "Gaps:",
    "Measured:",
    "Predicted:",
    "Loss:",
    "Lost scenarios:",
    "Impact:",
    "Alternatives:",
    "Retained checks:",
    "Restoration:",
    "Preview:",
];

/// What each removal-proposal clause must carry. The planner brief renders
/// this text; the validator enforces the exact labels above.
pub const REMOVAL_PROPOSAL_GUIDE: &str = "Target: the named removal target as a single token. Source: the owning change or source reference. Evidence: the retained evidence locator the proposal rests on. Gaps: the evidence gaps, unverified dependencies and uncheckable consumer access, disclosed rather than treated as absent. Measured: the benefit actually measured so far, stated explicitly as not yet measured when none exists - never a prediction presented as a measurement. Predicted: the predicted benefit. Loss: the reviewed behavior-loss label as a single token. Lost scenarios: what users lose and in which scenarios. Impact: the affected callers, configuration and installations. Alternatives: the alternatives considered, including no change. Retained checks: the requirements and checks that remain binding. Restoration: the recovery route that restores the capability. Preview: the locator of the unapplied preview.";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Specification {
    /// Source project, including when planning lives in a registered store.
    pub project: PathBuf,
    pub change: String,
    pub store: Option<String>,
    /// Expected resolved planning root. Refuse an accidental nearest project.
    pub planning_root: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentContract {
    /// An artifact returned by OpenSpec, relative to the resolved change root.
    pub acceptance_artifact: PathBuf,
    /// Exact Markdown heading of the acceptance section in that artifact.
    pub acceptance_heading: String,
    pub mechanism: String,
    pub counterexample: String,
    pub applicability: String,
    pub independent_acceptance: String,
    pub meaningful_effect: String,
    pub operating_conditions: String,
    pub comparison_policy: String,
    pub stopping_rule: String,
}

impl ExperimentContract {
    pub fn validate(&self) -> io::Result<()> {
        validate_heading("acceptance_heading", &self.acceptance_heading)?;
        for (name, value) in [
            ("mechanism", &self.mechanism),
            ("counterexample", &self.counterexample),
            ("applicability", &self.applicability),
            ("independent_acceptance", &self.independent_acceptance),
            ("meaningful_effect", &self.meaningful_effect),
            ("operating_conditions", &self.operating_conditions),
            ("comparison_policy", &self.comparison_policy),
            ("stopping_rule", &self.stopping_rule),
        ] {
            if value.trim().is_empty() || value.len() > 8192 {
                return Err(invalid(format!(
                    "missing or oversized experiment field: {name}"
                )));
            }
        }
        relative_path("acceptance artifact", &self.acceptance_artifact)
    }
}

/// The operation selected for one hypothesis's targeted measurement.
///
/// An existing build, search or diagnostic operation is linked from the
/// hypothesis's own change through this declaration; the adapter never creates
/// or requires a second hypothesis card or change for it. Only an operation
/// that independently becomes an improvement hypothesis owns its own change.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MeasurementWorkload {
    /// Bounded identity of the operation the measurement runs.
    pub operation: String,
    /// Where the operation's existing contract and evidence are linked from.
    pub contract: String,
}

/// The declared scope of one hypothesis's targeted initial measurement, stated
/// in the hypothesis's own OpenSpec change before any measurement is directed.
/// A hypothesis without a complete declared scope stays undispatched:
/// [`OpenSpec::begin_measurement`] refuses it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MeasurementScope {
    /// The observed problem or friction that started the investigation.
    pub observed_problem: String,
    /// What the targeted measurement covers, including its intended-use horizon.
    pub investigation_scope: String,
    /// The measurement question the targeted baseline answers.
    pub measurement_question: String,
    /// The selected existing operation and where its contract is linked.
    pub workload: MeasurementWorkload,
    /// Retained evidence or authorized real-work references the investigation
    /// is seeded from.
    pub evidence_references: Vec<String>,
    /// Known limits of the evidence basis and the targeted measurement.
    pub limits: String,
    /// The change artifact that states this scope, relative to the change root.
    pub declaration_artifact: PathBuf,
    /// Exact Markdown heading of the scope section in that artifact.
    pub declaration_heading: String,
}

impl MeasurementScope {
    pub fn validate(&self) -> io::Result<()> {
        validate_heading("declaration_heading", &self.declaration_heading)?;
        relative_path("declaration artifact", &self.declaration_artifact)?;
        for (name, value) in [
            ("observed_problem", &self.observed_problem),
            ("investigation_scope", &self.investigation_scope),
            ("measurement_question", &self.measurement_question),
            ("workload.operation", &self.workload.operation),
            ("workload.contract", &self.workload.contract),
            ("limits", &self.limits),
        ] {
            if value.trim().is_empty() || value.len() > 8192 {
                return Err(invalid(format!(
                    "missing or oversized measurement field: {name}"
                )));
            }
        }
        if self.evidence_references.is_empty() || self.evidence_references.len() > 64 {
            return Err(invalid(
                "missing or oversized measurement field: evidence_references",
            ));
        }
        for reference in &self.evidence_references {
            if reference.trim().is_empty() || reference.len() > 8192 {
                return Err(invalid(
                    "missing or oversized measurement field: evidence_references",
                ));
            }
        }
        Ok(())
    }
}

/// Proof that the hypothesis's own change resolved through the installed CLI
/// and stated the declared measurement scope before directed measurement. It
/// does not certify the later candidate-implementation prerequisites; those
/// stay with [`OpenSpec::qualify`].
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MeasurementReceipt {
    pub specification: Specification,
    pub scope: MeasurementScope,
    pub change_root: PathBuf,
    pub schema: String,
    /// Canonical artifact paths and content digests of the change as it stood
    /// when the measurement was directed.
    pub artifacts: BTreeMap<PathBuf, String>,
    /// Fingerprint of the declared scope the measurement is bound to.
    pub scope_digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanningReceipt {
    pub specification: Specification,
    pub contract: ExperimentContract,
    pub change_root: PathBuf,
    pub schema: String,
    /// Canonical artifact paths and content digests, including all spec deltas.
    pub artifacts: BTreeMap<PathBuf, String>,
    pub contract_digest: String,
    pub implementation_state: String,
}

/// The reviewable removal proposal stated in the hypothesis's own OpenSpec
/// change before any removal effect. The receipt binds the exact section body
/// of the change artifact that states it and carries the clause values the
/// bounded board record and the user's decision mirror; resolving it applies
/// nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovalProposalReceipt {
    pub specification: Specification,
    pub change_root: PathBuf,
    /// The resolved change artifact that states the proposal.
    pub artifact: PathBuf,
    pub heading: String,
    /// Digest of the exact extracted section body, so any later proposal
    /// change is detectable and needs a fresh decision.
    pub section_digest: String,
    pub target: String,
    pub source: String,
    pub evidence: String,
    pub gaps: String,
    pub measured: String,
    pub predicted: String,
    pub loss: String,
    pub lost_scenarios: String,
    pub impact: String,
    pub alternatives: String,
    pub retained_checks: String,
    pub restoration: String,
    pub preview: String,
}

impl RemovalProposalReceipt {
    /// One clause value by its exact label, so callers can re-read the
    /// reviewed content without a second parser.
    pub fn clause(&self, label: &str) -> Option<&str> {
        Some(match label {
            "Target:" => &self.target,
            "Source:" => &self.source,
            "Evidence:" => &self.evidence,
            "Gaps:" => &self.gaps,
            "Measured:" => &self.measured,
            "Predicted:" => &self.predicted,
            "Loss:" => &self.loss,
            "Lost scenarios:" => &self.lost_scenarios,
            "Impact:" => &self.impact,
            "Alternatives:" => &self.alternatives,
            "Retained checks:" => &self.retained_checks,
            "Restoration:" => &self.restoration,
            "Preview:" => &self.preview,
            _ => return None,
        })
    }
}

/// The change's actual task state, read from the installed CLI and its own
/// resolved artifacts. Completion reconciliation compares this state; a
/// decision token never stands in for a required task the artifact still
/// reports open. Reading it is read-only, so retention keeps the change and
/// its evidence referencable without any write.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompletionReceipt {
    pub specification: Specification,
    pub change_root: PathBuf,
    pub schema: String,
    /// The CLI's own apply state; `all_done` exactly when the change's
    /// artifact reports every task complete.
    pub state: String,
    pub total: u64,
    pub complete: u64,
    pub remaining: u64,
    /// Bounded descriptions of the tasks the change's artifact still reports
    /// open, in artifact order.
    pub unfinished_tasks: Vec<String>,
    /// Canonical artifact paths and content digests of the change as it stood
    /// when completion was read.
    pub artifacts: BTreeMap<PathBuf, String>,
}

impl CompletionReceipt {
    /// True exactly when the change's own artifact reports every task done.
    pub fn is_complete(&self) -> bool {
        self.remaining == 0 && self.state == "all_done"
    }
}

/// Proof that an unadopted change was archived through the installed CLI's
/// supported non-synchronizing path: the change and its reconciled artifacts
/// moved to the change archive with their content intact, while the main
/// specifications kept their exact files and content.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArchiveReceipt {
    pub specification: Specification,
    /// The change's location before the archive.
    pub change_root: PathBuf,
    pub archived_as: String,
    pub archive_root: PathBuf,
    pub artifacts: BTreeMap<PathBuf, String>,
    pub main_specs_files: usize,
    /// Digest of the main specification tree before and after the archive.
    pub main_specs_digest: String,
}

/// Only the installed CLI creates and resolves changes. No templates, workflow
/// packages or store registries are written directly by this adapter.
#[derive(Default)]
pub struct OpenSpec {
    /// Child-only environment overrides, useful for an explicitly isolated
    /// configuration home. The parent environment is never mutated.
    pub environment: BTreeMap<String, String>,
}

impl OpenSpec {
    fn command(&self, target: &Specification, arguments: &[&str]) -> io::Result<Vec<u8>> {
        validate_target(target)?;
        #[cfg(windows)]
        let mut command = {
            // CommandWithArgs passes literal native argv, including spaces and
            // metacharacters. It also refuses legacy Windows PowerShell 5.1.
            let mut command = Command::new("pwsh");
            command.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-CommandWithArgs",
                "& openspec @args; exit $LASTEXITCODE",
            ]);
            command
        };
        #[cfg(not(windows))]
        let mut command = Command::new("openspec");
        command
            .args(arguments)
            .current_dir(&target.project)
            .envs(&self.environment)
            .env("OPENSPEC_TELEMETRY", "0");
        if let Some(store) = &target.store {
            command.args(["--store", store]);
        }
        let output = command.output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "OpenSpec {} failed ({}): {}{}",
                arguments.first().copied().unwrap_or("command"),
                output.status,
                bounded_text(&output.stdout),
                bounded_text(&output.stderr),
            )));
        }
        if output.stdout.len() > MAX_ARTIFACT_BYTES as usize {
            return Err(invalid(
                "OpenSpec response exceeds the bounded planning contract",
            ));
        }
        Ok(output.stdout)
    }

    fn json(&self, target: &Specification, arguments: &[&str]) -> io::Result<Value> {
        serde_json::from_slice(&self.command(target, arguments)?)
            .map_err(|error| invalid(format!("OpenSpec returned invalid JSON: {error}")))
    }

    pub fn scaffold(&self, target: &Specification) -> io::Result<Value> {
        let context = self.json(target, &["context", "--json"])?;
        let resolved = PathBuf::from(required(&context["root"], "path")?).canonicalize()?;
        if resolved != target.planning_root.canonicalize()? {
            return Err(invalid(
                "OpenSpec resolved a different planning root; no change was created",
            ));
        }
        self.json(
            target,
            &[
                "new",
                "change",
                &target.change,
                "--schema",
                "spec-driven",
                "--json",
            ],
        )
    }

    /// Return native planning instructions. A caller must complete the artifacts
    /// and call qualify before dispatching implementation.
    pub fn instructions(&self, target: &Specification, artifact: &str) -> io::Result<Value> {
        if !["proposal", "specs", "design", "tasks"].contains(&artifact) {
            return Err(invalid("unsupported improvement planning artifact"));
        }
        self.json(
            target,
            &[
                "instructions",
                artifact,
                "--change",
                &target.change,
                "--json",
            ],
        )
    }

    /// Resolve the hypothesis's own OpenSpec change before any directed initial
    /// measurement. The change must state the declared measurement scope and
    /// carry its tasks; it need not yet invent a solution, so only `status` is
    /// consulted here. A later [`OpenSpec::qualify`] still gates candidate
    /// implementation on the complete validated artifacts.
    pub fn begin_measurement(
        &self,
        target: &Specification,
        scope: &MeasurementScope,
    ) -> io::Result<MeasurementReceipt> {
        scope.validate()?;
        let status = self.json(target, &["status", "--change", &target.change, "--json"])?;
        let root = PathBuf::from(required(&status["planningHome"], "root")?).canonicalize()?;
        if root != target.planning_root.canonicalize()? {
            return Err(invalid(
                "OpenSpec resolved a different planning root; no directed measurement is eligible",
            ));
        }
        if required(&status, "changeName")? != target.change {
            return Err(invalid("OpenSpec resolved a different change"));
        }
        let change_root = PathBuf::from(required(&status, "changeRoot")?).canonicalize()?;
        if !change_root.starts_with(&root) || change_root == root {
            return Err(invalid(
                "OpenSpec change escapes its declared planning root",
            ));
        }
        let mut artifacts = BTreeMap::new();
        let absent = Vec::new();
        for kind in ["proposal", "specs", "design", "tasks"] {
            let paths = match status["artifactPaths"][kind]["existingOutputPaths"].as_array() {
                Some(paths) => paths,
                None => &absent,
            };
            if paths.is_empty() && matches!(kind, "proposal" | "tasks") {
                return Err(invalid(format!(
                    "missing OpenSpec initial-measurement prerequisite: {kind}"
                )));
            }
            for path in paths {
                let path = PathBuf::from(
                    path.as_str()
                        .ok_or_else(|| invalid("invalid artifact path"))?,
                )
                .canonicalize()?;
                if !path.starts_with(&change_root) || path == change_root {
                    return Err(invalid("planning artifact escapes the selected change"));
                }
                artifacts.insert(path.clone(), digest_artifact(&path)?);
            }
        }
        let declaration = change_root
            .join(&scope.declaration_artifact)
            .canonicalize()
            .map_err(|_| {
                invalid(
                    "the measurement scope artifact is missing from the hypothesis's own OpenSpec change",
                )
            })?;
        if !artifacts.contains_key(&declaration) {
            return Err(invalid(
                "the measurement scope must be stated in a resolved artifact of the hypothesis's own OpenSpec change",
            ));
        }
        if !section_present(
            &fs::read_to_string(&declaration)?,
            &scope.declaration_heading,
        ) {
            return Err(invalid(
                "missing or empty measurement scope section in the linked OpenSpec artifact; no directed measurement is eligible",
            ));
        }
        Ok(MeasurementReceipt {
            specification: target.clone(),
            scope: scope.clone(),
            change_root,
            schema: required(&status, "schemaName")?.to_owned(),
            artifacts,
            scope_digest: digest_bytes(&serde_json::to_vec(scope)?),
        })
    }

    pub fn qualify(
        &self,
        target: &Specification,
        contract: &ExperimentContract,
    ) -> io::Result<PlanningReceipt> {
        contract.validate()?;
        let status = self.json(target, &["status", "--change", &target.change, "--json"])?;
        let root = PathBuf::from(required(&status["planningHome"], "root")?).canonicalize()?;
        if root != target.planning_root.canonicalize()? {
            return Err(invalid(
                "OpenSpec resolved a different planning root; no implementation is eligible",
            ));
        }
        if required(&status, "changeName")? != target.change {
            return Err(invalid("OpenSpec resolved a different change"));
        }
        let change_root = PathBuf::from(required(&status, "changeRoot")?).canonicalize()?;
        if !change_root.starts_with(&root) || change_root == root {
            return Err(invalid(
                "OpenSpec change escapes its declared planning root",
            ));
        }
        let reported = status["artifacts"]
            .as_array()
            .ok_or_else(|| invalid("OpenSpec did not report artifact completeness"))?;
        let mut artifacts = BTreeMap::new();
        for kind in ["proposal", "specs", "design", "tasks"] {
            if !reported
                .iter()
                .any(|artifact| artifact["id"] == kind && artifact["status"] == "done")
            {
                return Err(invalid(format!(
                    "missing OpenSpec planning prerequisite: {kind}"
                )));
            }
            let paths = status["artifactPaths"][kind]["existingOutputPaths"]
                .as_array()
                .filter(|paths| !paths.is_empty())
                .ok_or_else(|| invalid(format!("OpenSpec reports no files for {kind}")))?;
            for path in paths {
                let path = PathBuf::from(
                    path.as_str()
                        .ok_or_else(|| invalid("invalid artifact path"))?,
                )
                .canonicalize()?;
                if !path.starts_with(&change_root) || path == change_root {
                    return Err(invalid("planning artifact escapes the selected change"));
                }
                artifacts.insert(path.clone(), digest_artifact(&path)?);
            }
        }
        let acceptance = change_root
            .join(&contract.acceptance_artifact)
            .canonicalize()?;
        if !artifacts.contains_key(&acceptance) {
            return Err(invalid(
                "experiment acceptance must reference a resolved OpenSpec artifact",
            ));
        }
        if !section_present(
            &fs::read_to_string(&acceptance)?,
            &contract.acceptance_heading,
        ) {
            return Err(invalid(
                "missing or empty experiment acceptance section in the linked OpenSpec artifact",
            ));
        }
        // The native validator owns syntax and requirement/scenario checks.
        self.command(
            target,
            &["validate", &target.change, "--strict", "--no-interactive"],
        )?;
        let apply = self.json(
            target,
            &[
                "instructions",
                "apply",
                "--change",
                &target.change,
                "--json",
            ],
        )?;
        let state = required(&apply, "state")?;
        if !["ready", "all_done"].contains(&state) {
            return Err(invalid(format!(
                "OpenSpec implementation prerequisite is {state}"
            )));
        }
        // Detect changes while the external commands were checking the files.
        for (path, digest) in &artifacts {
            if digest_artifact(path)? != *digest {
                return Err(invalid("planning artifacts changed during qualification"));
            }
        }
        let contract_digest = digest_bytes(&serde_json::to_vec(contract)?);
        Ok(PlanningReceipt {
            specification: target.clone(),
            contract: contract.clone(),
            change_root,
            schema: required(&status, "schemaName")?.to_owned(),
            artifacts,
            contract_digest,
            implementation_state: state.to_owned(),
        })
    }

    /// Resolves the reviewable removal proposal of an already qualified
    /// change. The proposal must be stated exactly once under
    /// [`REMOVAL_PROPOSAL_HEADING`] in one resolved artifact of the change,
    /// with every declared clause present exactly once; a missing,
    /// duplicated or empty clause is refused by name. The resolved artifacts
    /// must still match the receipt, so a proposal is never read from a
    /// drifted change, and resolving it applies nothing.
    pub fn removal_proposal(
        &self,
        receipt: &PlanningReceipt,
    ) -> io::Result<RemovalProposalReceipt> {
        let mut stated: Option<(PathBuf, String)> = None;
        for (path, digest) in &receipt.artifacts {
            if !path.starts_with(&receipt.change_root) || path == &receipt.change_root {
                return Err(invalid("a planning artifact escapes the selected change"));
            }
            let relative = path
                .strip_prefix(&receipt.change_root)
                .map_err(|_| invalid("a planning artifact escapes the selected change"))?;
            let bytes = fs::read(path).map_err(|error| {
                invalid(format!(
                    "the planning artifact {} is unreadable: {error}",
                    relative.display()
                ))
            })?;
            if bytes.len() as u64 > MAX_ARTIFACT_BYTES {
                return Err(invalid(format!(
                    "the planning artifact {} exceeds the bounded planning contract",
                    relative.display()
                )));
            }
            if digest_bytes(&bytes) != *digest {
                return Err(invalid(format!(
                    "the planning artifact {} changed after qualification; re-qualify the change before resolving its removal proposal",
                    relative.display()
                )));
            }
            let text = String::from_utf8(bytes).map_err(|_| {
                invalid(format!(
                    "the planning artifact {} is not UTF-8 text",
                    relative.display()
                ))
            })?;
            if let Some(section) = extract_section(&text, REMOVAL_PROPOSAL_HEADING) {
                if let Some((first, _)) = &stated {
                    let first = first.strip_prefix(&receipt.change_root).unwrap_or(first);
                    return Err(invalid(format!(
                        "the removal proposal is stated in more than one resolved artifact ({} and {}); state it exactly once under '{REMOVAL_PROPOSAL_HEADING}'",
                        first.display(),
                        relative.display()
                    )));
                }
                stated = Some((path.clone(), section));
            }
        }
        let Some((artifact, section)) = stated else {
            return Err(invalid(format!(
                "the change states no reviewable removal proposal: missing the section '{REMOVAL_PROPOSAL_HEADING}' in its resolved artifacts"
            )));
        };
        let clauses = removal_clauses(&section)?;
        let clause = |label: &str| clauses.get(label).cloned().unwrap_or_default();
        Ok(RemovalProposalReceipt {
            specification: receipt.specification.clone(),
            change_root: receipt.change_root.clone(),
            artifact,
            heading: REMOVAL_PROPOSAL_HEADING.to_owned(),
            section_digest: digest_bytes(section.as_bytes()),
            target: clause("Target:"),
            source: clause("Source:"),
            evidence: clause("Evidence:"),
            gaps: clause("Gaps:"),
            measured: clause("Measured:"),
            predicted: clause("Predicted:"),
            loss: clause("Loss:"),
            lost_scenarios: clause("Lost scenarios:"),
            impact: clause("Impact:"),
            alternatives: clause("Alternatives:"),
            retained_checks: clause("Retained checks:"),
            restoration: clause("Restoration:"),
            preview: clause("Preview:"),
        })
    }

    /// Re-resolve stores and validate content before dependent effects. An old
    /// receipt cannot authorize a changed contract or another registered root.
    pub fn revalidate(&self, receipt: &PlanningReceipt) -> io::Result<()> {
        let current = self.qualify(&receipt.specification, &receipt.contract)?;
        if current.artifacts != receipt.artifacts
            || current.change_root != receipt.change_root
            || current.schema != receipt.schema
            || current.contract_digest != receipt.contract_digest
        {
            return Err(invalid(
                "frozen planning inputs changed; dependent comparison requires preparation again",
            ));
        }
        Ok(())
    }

    /// Re-resolve the same change and measurement scope before a retained
    /// baseline is reused. The change may legitimately grow into its complete
    /// artifacts, but its identity and declared scope must not change; drift
    /// requires a fresh directed measurement.
    pub fn revalidate_measurement(&self, receipt: &MeasurementReceipt) -> io::Result<()> {
        let current = self.begin_measurement(&receipt.specification, &receipt.scope)?;
        if current.change_root != receipt.change_root || current.schema != receipt.schema {
            return Err(invalid(
                "the OpenSpec change identity changed; a retained baseline cannot be reused",
            ));
        }
        Ok(())
    }

    /// Reads the change's actual completion state from the installed CLI and
    /// its own resolved artifacts. Completion is reconciled from this task
    /// state alone, so an experiment outcome cannot close a required task the
    /// change still reports open. The operation writes nothing and is safe to
    /// repeat, which is what makes retention referencable.
    pub fn completion(&self, target: &Specification) -> io::Result<CompletionReceipt> {
        let status = self.json(target, &["status", "--change", &target.change, "--json"])?;
        let root = PathBuf::from(required(&status["planningHome"], "root")?).canonicalize()?;
        if root != target.planning_root.canonicalize()? {
            return Err(invalid(
                "OpenSpec resolved a different planning root; no completion state is eligible",
            ));
        }
        if required(&status, "changeName")? != target.change {
            return Err(invalid("OpenSpec resolved a different change"));
        }
        let change_root = PathBuf::from(required(&status, "changeRoot")?).canonicalize()?;
        if !change_root.starts_with(&root) || change_root == root {
            return Err(invalid(
                "OpenSpec change escapes its declared planning root",
            ));
        }
        let mut artifacts = BTreeMap::new();
        for kind in ["proposal", "specs", "design", "tasks"] {
            let paths = status["artifactPaths"][kind]["existingOutputPaths"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if paths.is_empty() && kind == "tasks" {
                return Err(invalid(
                    "missing OpenSpec task artifact; completion cannot be reconciled",
                ));
            }
            for path in &paths {
                let path = PathBuf::from(
                    path.as_str()
                        .ok_or_else(|| invalid("invalid artifact path"))?,
                )
                .canonicalize()?;
                if !path.starts_with(&change_root) || path == change_root {
                    return Err(invalid("planning artifact escapes the selected change"));
                }
                artifacts.insert(path.clone(), digest_artifact(&path)?);
            }
        }
        let apply = self.json(
            target,
            &[
                "instructions",
                "apply",
                "--change",
                &target.change,
                "--json",
            ],
        )?;
        if required(&apply, "changeName")? != target.change {
            return Err(invalid("OpenSpec resolved a different change"));
        }
        if PathBuf::from(required(&apply, "changeDir")?).canonicalize()? != change_root {
            return Err(invalid("OpenSpec resolved a different change directory"));
        }
        let state = required(&apply, "state")?.to_owned();
        let progress = &apply["progress"];
        let total = whole(progress, "total")?;
        let complete = whole(progress, "complete")?;
        let remaining = whole(progress, "remaining")?;
        if complete + remaining != total {
            return Err(invalid(
                "OpenSpec reported inconsistent task progress; completion cannot be reconciled",
            ));
        }
        if state == "all_done" && remaining != 0 {
            return Err(invalid(
                "OpenSpec reported an all-done state with unfinished tasks; completion cannot be reconciled",
            ));
        }
        let tasks = apply["tasks"]
            .as_array()
            .ok_or_else(|| invalid("OpenSpec did not report the change's tasks"))?;
        let mut open = 0u64;
        let mut unfinished_tasks = Vec::new();
        for task in tasks {
            if task["done"].as_bool() != Some(false) {
                continue;
            }
            open += 1;
            let text = task["description"].as_str().unwrap_or_default().trim();
            if !text.is_empty() {
                unfinished_tasks.push(bounded_task(text));
            }
        }
        if open != remaining {
            return Err(invalid(
                "OpenSpec reported a task list inconsistent with its own progress; completion cannot be reconciled",
            ));
        }
        Ok(CompletionReceipt {
            specification: target.clone(),
            change_root,
            schema: required(&status, "schemaName")?.to_owned(),
            state,
            total,
            complete,
            remaining,
            unfinished_tasks,
            artifacts,
        })
    }

    /// Archives a completed change whose behavior was not adopted through the
    /// installed CLI's supported `--skip-specs` path: the change and its
    /// reconciled artifacts move to the change archive while the main
    /// specifications keep their exact files and content. Refuses while the
    /// change's own artifact still reports an unfinished required task - an
    /// experiment outcome cannot close it - and refuses a stale `expected`
    /// receipt whose resolved artifacts no longer match.
    pub fn archive_unadopted(
        &self,
        target: &Specification,
        expected: &CompletionReceipt,
    ) -> io::Result<ArchiveReceipt> {
        let current = self.completion(target)?;
        if !current.is_complete() {
            let detail = if current.remaining > 0 {
                let tasks = if current.unfinished_tasks.is_empty() {
                    "the change reports no usable task description".to_owned()
                } else {
                    current.unfinished_tasks.join("; ")
                };
                format!(
                    "the change still reports {} unfinished required task(s): {tasks}; an experiment outcome cannot close them",
                    current.remaining
                )
            } else {
                format!(
                    "the change's apply state is {} although its task list reports no open task; completion cannot be confirmed",
                    current.state
                )
            };
            return Err(invalid(format!(
                "{detail}, so the unadopted change is retained"
            )));
        }
        if current.change_root != expected.change_root || current.artifacts != expected.artifacts {
            return Err(invalid(
                "the change's resolved artifacts changed since it was reconciled; re-read completion and reconcile again before archiving",
            ));
        }
        let before = main_specs_digest(&target.planning_root)?;
        let archived = self.json(
            target,
            &["archive", &target.change, "--skip-specs", "-y", "--json"],
        )?;
        let archive = &archived["archive"];
        if archive["specsUpdated"].as_bool() != Some(false) {
            return Err(invalid(format!(
                "the installed CLI did not confirm a non-synchronizing archive (specsUpdated={}); the main specifications may have been changed by the unadopted delta and must be inspected and restored from their retained state",
                archive["specsUpdated"]
            )));
        }
        let archived_as = required(archive, "archivedAs")?.to_owned();
        let archive_root = PathBuf::from(required(archive, "path")?).canonicalize()?;
        if !archive_root.is_dir() {
            return Err(invalid(
                "the installed CLI reported an archive path that is not a directory",
            ));
        }
        let after = main_specs_digest(&target.planning_root)?;
        if before != after {
            return Err(invalid(format!(
                "the main specifications changed during the non-synchronizing archive ({} files before, {} after); inspect {} and restore the affected main specifications from their retained state",
                before.0,
                after.0,
                target.planning_root.join("openspec/specs").display()
            )));
        }
        for (path, digest) in &current.artifacts {
            let relative = path
                .strip_prefix(&current.change_root)
                .map_err(|_| invalid("a reconciled artifact escapes its change"))?
                .to_path_buf();
            let retained = archive_root.join(&relative);
            let retained_digest = digest_artifact(&retained).map_err(|error| {
                invalid(format!(
                    "the archived change does not retain {}: {error}",
                    relative.display()
                ))
            })?;
            if retained_digest != *digest {
                return Err(invalid(format!(
                    "the archived change does not retain the reconciled content of {}; the unadopted artifacts are not fully accessible",
                    relative.display()
                )));
            }
        }
        Ok(ArchiveReceipt {
            specification: target.clone(),
            change_root: current.change_root,
            archived_as,
            archive_root,
            artifacts: current.artifacts,
            main_specs_files: before.0,
            main_specs_digest: before.1,
        })
    }
}

fn validate_target(target: &Specification) -> io::Result<()> {
    if !target.project.is_absolute()
        || !target.project.is_dir()
        || !target.planning_root.is_absolute()
        || !target.planning_root.is_dir()
    {
        return Err(invalid(
            "source project and planning root must be existing absolute directories",
        ));
    }
    for (name, value) in std::iter::once(("change", target.change.as_str()))
        .chain(target.store.as_deref().map(|value| ("store", value)))
    {
        if value.is_empty()
            || value.len() > 200
            || value.starts_with('-')
            || !value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
            || matches!(value, "." | "..")
        {
            return Err(invalid(format!("invalid OpenSpec {name} identifier")));
        }
    }
    Ok(())
}

fn validate_heading(field: &str, heading: &str) -> io::Result<()> {
    let heading = heading.trim();
    let level = heading.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&level)
        || !heading[level..].starts_with(' ')
        || heading[level..].trim().is_empty()
        || heading.contains(['\n', '\r'])
        || heading.len() > 512
    {
        return Err(invalid(format!("{field} must identify a Markdown section")));
    }
    Ok(())
}

/// Extracts one Markdown section body: the lines after the exact heading line
/// until the next heading of the same or higher level. A heading deeper than
/// the section heading stays inside it.
fn extract_section(text: &str, heading: &str) -> Option<String> {
    let heading = heading.trim();
    let level = heading.bytes().take_while(|byte| *byte == b'#').count();
    let mut lines = text.lines().skip_while(|line| line.trim() != heading);
    lines.next()?;
    let body: Vec<&str> = lines
        .take_while(|line| {
            let line = line.trim();
            let next_level = line.bytes().take_while(|byte| *byte == b'#').count();
            next_level == 0 || next_level > level || !line[next_level..].starts_with(' ')
        })
        .collect();
    Some(body.join("\n"))
}

/// True when the exact Markdown heading exists and its section has content.
fn section_present(text: &str, heading: &str) -> bool {
    extract_section(text, heading)
        .is_some_and(|body| body.lines().any(|line| !line.trim().is_empty()))
}

/// Parses the clause values of one reviewable removal proposal section. Every
/// declared clause must be stated exactly once, non-empty, on one line and
/// within the bounded clause size; a clause line may carry a Markdown bullet.
fn removal_clauses(section: &str) -> io::Result<BTreeMap<&'static str, String>> {
    let mut clauses: BTreeMap<&'static str, String> = BTreeMap::new();
    for line in section.lines() {
        let line = line.trim();
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line)
            .trim_start();
        for label in REMOVAL_PROPOSAL_CLAUSES {
            let Some(value) = line.strip_prefix(label) else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                return Err(invalid(format!(
                    "the removal proposal clause {label} is empty"
                )));
            }
            if value.len() > MAX_REMOVAL_CLAUSE_BYTES {
                return Err(invalid(format!(
                    "the removal proposal clause {label} exceeds {MAX_REMOVAL_CLAUSE_BYTES} bytes"
                )));
            }
            if clauses.insert(label, value.to_owned()).is_some() {
                return Err(invalid(format!(
                    "the removal proposal clause {label} is stated more than once"
                )));
            }
            break;
        }
    }
    for label in REMOVAL_PROPOSAL_CLAUSES {
        if !clauses.contains_key(label) {
            return Err(invalid(format!(
                "the reviewable removal proposal is incomplete: missing the clause {label}"
            )));
        }
    }
    Ok(clauses)
}

fn relative_path(field: &str, path: &Path) -> io::Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        Err(invalid(format!(
            "{field} must be a nonempty relative path without traversal"
        )))
    } else {
        Ok(())
    }
}

fn digest_artifact(path: &Path) -> io::Result<String> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ARTIFACT_BYTES {
        return Err(invalid(
            "planning artifact is empty, not a file, or exceeds the size bound",
        ));
    }
    let bytes = fs::read(path)?;
    if std::str::from_utf8(&bytes)
        .map_err(|_| invalid("planning artifact is not UTF-8"))?
        .trim()
        .is_empty()
    {
        return Err(invalid("planning artifact contains no content"));
    }
    Ok(digest_bytes(&bytes))
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A bounded, single-line task description for a completion receipt.
fn bounded_task(text: &str) -> String {
    let text = text.replace(['\n', '\r'], " ");
    if text.chars().count() <= MAX_TASK_DESCRIPTION {
        return text;
    }
    let truncated: String = text.chars().take(MAX_TASK_DESCRIPTION).collect();
    format!("{truncated}...")
}

/// Digest of the main specification tree under a resolved planning root over
/// canonical relative paths and file content. A missing tree is the empty
/// tree, so an accidental sync of an unadopted delta changes this digest.
fn main_specs_digest(planning_root: &Path) -> io::Result<(usize, String)> {
    fn collect(root: &Path, directory: &Path, files: &mut Vec<(String, String)>) -> io::Result<()> {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            if !file_type.is_dir() && !file_type.is_file() {
                continue;
            }
            if files.len() >= MAX_SPEC_FILES {
                return Err(invalid(
                    "the main specification tree exceeds the inspection bound",
                ));
            }
            let path = entry.path();
            if file_type.is_dir() {
                collect(root, &path, files)?;
            } else {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| invalid("a main specification escapes its root"))?
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push((relative, digest_file(&path)?));
            }
        }
        Ok(())
    }
    let root = planning_root.join("openspec/specs");
    let mut files = Vec::new();
    collect(&root, &root, &mut files)?;
    files.sort();
    Ok((files.len(), digest_bytes(&serde_json::to_vec(&files)?)))
}

/// Digest of one main-specification file. Unlike a planning artifact it may
/// legitimately be empty (a store keeps placeholder files), so only the size
/// bound applies here.
fn digest_file(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    if bytes.len() > MAX_ARTIFACT_BYTES as usize {
        return Err(invalid("a main specification file exceeds the size bound"));
    }
    Ok(digest_bytes(&bytes))
}

/// A required whole-number field of one CLI JSON object.
fn whole(value: &Value, field: &str) -> io::Result<u64> {
    value[field]
        .as_u64()
        .ok_or_else(|| invalid(format!("OpenSpec did not report {field}")))
}

fn required<'a>(value: &'a Value, field: &str) -> io::Result<&'a str> {
    value[field]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid(format!("OpenSpec did not report {field}")))
}

fn bounded_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).into_owned()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}
