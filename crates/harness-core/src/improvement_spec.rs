//! Native OpenSpec prerequisites for improvement hypotheses and workloads.
//!
//! OpenSpec remains the artifact owner. These receipts bind its actual resolved
//! files to a declared initial measurement scope and experiment contract; they
//! do not certify implementation, task completion, benefit, or user removal
//! authority.

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

/// True when the exact Markdown heading exists and its section has content.
fn section_present(text: &str, heading: &str) -> bool {
    let heading = heading.trim();
    let level = heading.bytes().take_while(|byte| *byte == b'#').count();
    let mut lines = text.lines().skip_while(|line| line.trim() != heading);
    lines.next().is_some()
        && lines
            .take_while(|line| {
                let line = line.trim();
                let next_level = line.bytes().take_while(|byte| *byte == b'#').count();
                next_level == 0 || next_level > level || !line[next_level..].starts_with(' ')
            })
            .any(|line| !line.trim().is_empty())
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
