//! Real isolated arm preparation on top of the experiment binding owner.
//!
//! A [`PreparedVariant`] is an immutable build identity; a home directory alone
//! is not an arm. This module makes one arm *ready to execute*: the prepared
//! build is published into a fresh owned executor home through the real
//! [`crate::core_install::connect`] owner, the shared explicit client inputs
//! are written into that home, and the effective launcher, instructions,
//! skills, tools, launch registration and configuration are read back and
//! retained as an [`ArmRuntime`] identity. [`verify_consumption`] re-verifies
//! that identity read-only before an attempt, and [`retire_arm`] /
//! [`discard_arm`] restore the homes through the existing disconnection owner.
//!
//! # Controller call order
//!
//! Per arm, between `improvement_experiment` preparation and the measured
//! attempt (the experiment owner stays authoritative for frozen copies,
//! candidate checkouts and variant selection):
//!
//! 1. `improvement_experiment::prepare_home` for the arm's `home`, `user_home`
//!    and `dependency_user_home` (fresh owned directories; an existing home is
//!    never merged).
//! 2. `improvement_experiment::prepare_variant` for the arm's immutable build.
//! 3. [`install_arm`] with the explicit private inputs: the kit source is
//!    derived from the build record (the two cannot disagree), the upstream
//!    client is explicit (never a PATH or provider fallback), and `protected`
//!    names every allocation the installation must stay disjoint from (frozen
//!    workloads, the other arm, prepared runtimes, the candidate checkout, the
//!    immutable supervisor/oracle). The declared client route and its explicit
//!    overlay are written into the arm configuration, and every declared
//!    setting is reconciled against the launch-time overrides the arm's own kit
//!    profile would apply, so an overridden declaration refuses instead of
//!    being recorded as consumed.
//! 4. `ExperimentBindings::validate` / `verify_pre_attempt` before the first
//!    measured attempt; [`verify_consumption`] additionally proves this arm's
//!    home still consumes exactly the prepared runtime and content.
//! 5. `improvement_experiment::select_variant` between attempts, with
//!    `attempt_active` while a measured attempt holds its frozen runtime.
//! 6. [`retire_arm`] when the arm's homes are no longer needed (an aborted
//!    preparation can be restored with [`discard_arm`]): both restore through
//!    the registered disconnection owner and touch only recorded owned links.
//!
//! # Boundaries
//!
//! Preparation is model-free: `connect` performs no model call, this module
//! never selects a provider and never fabricates model capability metadata.
//! It never copies ambient user-home state: only explicit [`PrivateInput`]
//! files are copied into an owned fresh home, with recorded hashes, and an
//! external catalogue stays external and drift-checked. Shared resources
//! (MCP workers, ports, caches, the model server) are *not* isolated by these
//! homes; the caller keeps them explicit and serialized. Disconnected homes
//! are never deleted here: reclamation stays with the owner of the run root.

use crate::{
    build_identity, core_disconnect, core_install,
    improvement_experiment::{Arm, PreparedVariant},
    installation_lock::InstallationLock,
    installation_state::PathScope,
    inventory,
    native_launcher::Registration as LaunchRegistration,
    outcome_qualification::{LocalRunner, RunnerRecord},
    registration_native,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path, PathBuf},
    time::Duration,
};

pub const RUNTIME_SCHEMA: u32 = 2;

/// Provider id of the explicit local route written into every prepared arm.
/// It matches the existing explicit local outcome route; no other provider is
/// ever selected or inherited implicitly.
pub const LOCAL_PROVIDER: &str = "local";

const CONFIG_LIMIT: usize = 1024 * 1024;
const FILE_LIMIT: u64 = 4 * 1024 * 1024;
const MAX_TIMEOUT: Duration = Duration::from_secs(300);

fn invalid(detail: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("improvement runtime: {detail}"),
    )
}

/// Ordinary (non-verbatim) spelling of an absolute path. The installation
/// owner rejects `\\?\` paths; `canonicalize` produces them on Windows.
fn plain(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let text = absolute.to_string_lossy().into_owned();
    Ok(PathBuf::from(
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned(),
    ))
}

/// Existing path in ordinary spelling.
fn resolved(path: &Path) -> io::Result<PathBuf> {
    plain(&fs::canonicalize(path)?)
}

/// Case-insensitive (Windows) canonical-or-absolute comparison key.
fn place(path: &Path) -> io::Result<String> {
    let plain = plain(path)?;
    let canonical = fs::canonicalize(&plain).unwrap_or(plain);
    let text = canonical.to_string_lossy();
    Ok(text
        .strip_prefix(r"\\?\")
        .unwrap_or(&text)
        .trim_end_matches('\\')
        .to_lowercase())
}

fn same_place(left: &Path, right: &Path) -> io::Result<bool> {
    Ok(place(left)? == place(right)?)
}

/// Case-insensitive absolute path components. `follow` resolves existing
/// aliases for allocation disjointness; link destinations are compared without
/// following their targets.
fn allocation_components(path: &Path, follow: bool) -> Vec<String> {
    let absolute = if follow {
        fs::canonicalize(path)
            .unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
    } else {
        std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
    };
    let text = absolute.to_string_lossy().into_owned();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned();
    Path::new(&text)
        .components()
        .map(|component| {
            let value = component.as_os_str().to_string_lossy().into_owned();
            if cfg!(windows) {
                value.to_ascii_lowercase()
            } else {
                value
            }
        })
        .collect()
}

/// True when either allocation is the other or is nested inside it, including
/// canonical aliases. Components compare case-insensitively on Windows.
fn overlaps(left: &Path, right: &Path) -> bool {
    let left = allocation_components(left, true);
    let right = allocation_components(right, true);
    let (short, long) = if left.len() <= right.len() {
        (left, right)
    } else {
        (right, left)
    };
    !short.is_empty() && long[..short.len()] == short[..]
}

/// True when `path` is `root` or is nested inside it.
fn inside(path: &Path, root: &Path) -> bool {
    let path = allocation_components(path, false);
    let root = allocation_components(root, false);
    !root.is_empty() && path.len() >= root.len() && path[..root.len()] == root[..]
}

fn bounded_value(name: &str, value: &str, limit: usize) -> io::Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > limit || trimmed.chars().any(char::is_control) {
        return Err(invalid(format!("{name} is missing or not a bounded value")));
    }
    Ok(())
}

fn bounded_token(name: &str, value: &str, limit: usize) -> io::Result<()> {
    bounded_value(name, value, limit)?;
    if value.trim().chars().any(char::is_whitespace) {
        return Err(invalid(format!("{name} must be a single token")));
    }
    Ok(())
}

/// Digest of one explicit bounded ordinary file; refuses reparse points.
fn hash_file(name: &str, path: &Path, limit: u64) -> io::Result<String> {
    build_identity::ordinary(path)?;
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(invalid(format!("{name} is not a regular file")));
    }
    if metadata.len() > limit {
        return Err(invalid(format!("{name} exceeds its size bound")));
    }
    build_identity::hash_file(path)
}

/// Deterministic digest of one source directory: every ordinary file's
/// relative path and content hash, ordered by path and bounded in count and
/// total bytes. Used for directory links (skills, agents) whose whole content
/// is consumed, not only their descriptor.
fn directory_digest(name: &str, root: &Path) -> io::Result<String> {
    const MAX_FILES: usize = 4096;
    const MAX_DEPTH: usize = 32;
    const MAX_BYTES: u64 = 64 * 1024 * 1024;
    let root = resolved(root)?;
    let mut files: Vec<(String, String)> = Vec::new();
    let mut total = 0u64;
    let mut stack = vec![(root.clone(), 0usize)];
    while let Some((directory, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            return Err(invalid(format!("{name} is nested beyond its bound")));
        }
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            build_identity::ordinary(&path)?;
            if path.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            if files.len() >= MAX_FILES {
                return Err(invalid(format!("{name} contains too many files")));
            }
            let metadata = fs::metadata(&path)?;
            total = total.saturating_add(metadata.len());
            if total > MAX_BYTES {
                return Err(invalid(format!("{name} exceeds its size bound")));
            }
            let relative = path
                .strip_prefix(&root)
                .map_err(io::Error::other)?
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, build_identity::hash_file(&path)?));
        }
    }
    files.sort();
    Ok(build_identity::hash_bytes(&serde_json::to_vec(&files)?))
}

fn ambient_codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(|value| PathBuf::from(value).join(".codex"))
        })
}

fn ambient_user_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

/// Explicit local client inputs shared by both arms.
///
/// The accepted model/provider/effort selection travels in [`LocalRunner`]
/// (endpoint, served model and declared material identity) plus the declared
/// reasoning effort. The catalogue is an external local file referenced from
/// the arm configuration (`model_catalog_json`), never copied, generated or
/// edited; it must stay resolvable and unchanged. The optional overlay is one
/// explicit TOML file carrying the qualified local client settings that the
/// route fields do not express (`features.code_mode = false`,
/// `model_context_window`, web/sandbox/approval settings, ...); it is merged
/// into the arm's base configuration, never into ambient state, and its file
/// must stay resolvable and unchanged. Settings the arm's own kit profile
/// would override at launch are refused instead of being silently rewritten,
/// and every declared setting is re-verified through the same native
/// precedence the launcher applies. The module writes no credentials, never
/// falls back to another provider and records exactly what was declared
/// (including missing identity facts) without inventing values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientInputs {
    pub runner: LocalRunner,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalogue: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<PathBuf>,
}

/// One explicit private runner file copied into the owned fresh home. The
/// source is an explicit caller-supplied path - never ambient home state - and
/// the recorded receipt keeps only the relative destination and its digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrivateInput {
    pub source: PathBuf,
    /// Relative destination inside the arm home, for example `auth.json`.
    pub destination: String,
}

/// Explicit private inputs for one arm's installation. Every path is named by
/// the caller; nothing is discovered from the ambient environment.
#[derive(Clone, Debug)]
pub struct ArmRequest {
    pub variant: PreparedVariant,
    /// Fresh owned executor home (`CODEX_HOME`) for this arm.
    pub home: PathBuf,
    /// Fresh owned user profile root that receives the skill links.
    pub user_home: PathBuf,
    /// Fresh owned dependency root; it may equal `user_home` as a real
    /// installation does, but never nests inside another arm allocation.
    pub dependency_user_home: PathBuf,
    /// Explicit native Codex client; no PATH lookup or implicit fallback.
    pub upstream: PathBuf,
    pub timeout: Duration,
    /// Accepted shared client inputs, when the run declares a local model.
    /// `None` prepares a model-free arm; a measured model attempt requires them.
    pub client: Option<ClientInputs>,
    pub private_inputs: Vec<PrivateInput>,
    /// Experiment allocations the installation state must stay disjoint from,
    /// for example the frozen workloads, the other arm's homes, prepared
    /// runtimes, the candidate checkout and the supervisor/oracle roots.
    pub protected: Vec<PathBuf>,
}

impl ArmRequest {
    /// Convenience constructor for a model-free arm whose fresh homes were
    /// prepared by the experiment owner.
    pub fn model_free(
        variant: PreparedVariant,
        home: PathBuf,
        user_home: PathBuf,
        dependency_user_home: PathBuf,
        upstream: PathBuf,
    ) -> Self {
        Self {
            variant,
            home,
            user_home,
            dependency_user_home,
            upstream,
            timeout: Duration::from_secs(60),
            client: None,
            private_inputs: Vec::new(),
            protected: Vec::new(),
        }
    }
}

/// One verified installed link recorded by its exact destination, source and
/// content digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstalledLink {
    pub name: String,
    pub destination: PathBuf,
    pub source: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileIdentity {
    pub path: PathBuf,
    pub sha256: String,
}

/// One declared client setting exactly as it was written into the arm
/// configuration, for drift checks and native-precedence reconciliation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclaredSetting {
    /// Dotted TOML path, for example `features.code_mode`.
    pub key: String,
    pub value: toml::Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrivateRecord {
    /// Relative destination inside the arm home.
    pub destination: String,
    pub sha256: String,
}

/// The declared client identity as written into the arm configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientRecord {
    pub runner: RunnerRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalogue: Option<FileIdentity>,
}

/// The effective arm configuration: the written base configuration, every
/// declared client setting, the explicit overlay and the arm kit profile
/// whose launch overrides were reconciled against those settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigurationRecord {
    pub path: PathBuf,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<ClientRecord>,
    pub settings: Vec<DeclaredSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<FileIdentity>,
    /// The arm kit profile whose launch overrides were reconciled against the
    /// declared settings; present whenever client inputs were declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kit_profile: Option<FileIdentity>,
}

/// Compact facts reported by the real installation owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallationFacts {
    pub status: String,
    pub links: usize,
    pub changed_links: usize,
    pub path_change: bool,
    /// Digest of the caller-selected client used by the owner's runtime check.
    pub runtime_executable_sha256: String,
    /// Private evidence directory retained by the runtime check.
    pub runtime_evidence: PathBuf,
}

/// Retained preparation and consumption identity of one ready arm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArmRuntime {
    pub schema: u32,
    pub arm: Arm,
    pub label: String,
    pub variant: PreparedVariant,
    /// Kit source root recorded by the build; content links resolve into it.
    pub source: PathBuf,
    pub home: PathBuf,
    pub user_home: PathBuf,
    pub dependency_user_home: PathBuf,
    /// The explicit client the runtime check executed; must stay unchanged.
    pub upstream: PathBuf,
    pub upstream_sha256: String,
    /// Effective launcher link: `home/harness/bin/codex.exe`.
    pub launcher: InstalledLink,
    /// Effective launch registration consumed by the launcher.
    pub launch_registration: PathBuf,
    pub launch_sha256: String,
    pub instructions: InstalledLink,
    pub agents: InstalledLink,
    pub skills: Vec<InstalledLink>,
    /// Native command links (`home/harness/bin`) excluding the launcher.
    pub commands: Vec<InstalledLink>,
    pub configuration: Option<ConfigurationRecord>,
    pub private: Vec<PrivateRecord>,
    pub installation: InstallationFacts,
    pub model_calls: u32,
}

impl ArmRuntime {
    /// True when the accepted local client inputs were written into this arm.
    pub fn client_configured(&self) -> bool {
        self.configuration
            .as_ref()
            .is_some_and(|record| record.client.is_some())
    }
}

/// Identity actually re-verified as consumed by one arm home.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Consumption {
    pub schema: u32,
    pub status: String,
    pub arm: Arm,
    pub label: String,
    pub build: PathBuf,
    pub record_sha256: String,
    pub launcher_sha256: String,
    pub model_ready: bool,
    pub model_calls: u32,
}

/// Result of restoring one arm's homes through the disconnection owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Retirement {
    pub schema: u32,
    pub status: String,
    pub removed_links: usize,
    pub already_missing: usize,
    pub preserved_adopted: usize,
    pub path_change: bool,
    pub model_calls: u32,
}

struct ValidatedClient {
    record: ClientRecord,
    config_bytes: Vec<u8>,
    settings: Vec<DeclaredSetting>,
    overlay: Option<FileIdentity>,
    kit_profile: FileIdentity,
}

struct ValidatedPrivate {
    source: PathBuf,
    destination: String,
}

struct Plan {
    variant: PreparedVariant,
    source: PathBuf,
    build: PathBuf,
    record: build_identity::BuildRecord,
    home: PathBuf,
    user_home: PathBuf,
    dependency_user_home: PathBuf,
    upstream: PathBuf,
    upstream_sha256: String,
    timeout: Duration,
    client: Option<ValidatedClient>,
    private_inputs: Vec<ValidatedPrivate>,
}

impl Plan {
    fn connect_request(&self) -> core_install::Request {
        core_install::Request {
            source: self.source.clone(),
            build: self.build.clone(),
            codex_home: self.home.clone(),
            user_home: self.user_home.clone(),
            dependency_user_home: self.dependency_user_home.clone(),
            upstream: Some(self.upstream.clone()),
            timeout: self.timeout,
            path_scope: Some(PathScope::Process),
        }
    }
}

/// Validate every explicit input without touching any installation state.
fn prepare_plan(request: &ArmRequest) -> io::Result<Plan> {
    let variant = &request.variant;
    bounded_value("arm label", &variant.label, 64)?;
    if variant.record_sha256.len() != 64 || variant.source_sha256.len() != 64 {
        return Err(invalid(
            "the prepared variant identity is not a full digest",
        ));
    }
    let build = resolved(&variant.build).map_err(|error| {
        invalid(format!(
            "the prepared {} runtime build is unavailable ({error}); prepare it again",
            variant.label
        ))
    })?;
    let record = build_identity::verify_record_integrity(&build).map_err(|error| {
        invalid(format!(
            "the prepared {} runtime is not intact ({error}); prepare it again",
            variant.label
        ))
    })?;
    if build_identity::hash_file(&build.join("build.json"))? != variant.record_sha256 {
        return Err(invalid(format!(
            "the prepared {} runtime changed since preparation; prepare it again",
            variant.label
        )));
    }
    if record.source.sha256 != variant.source_sha256 {
        return Err(invalid(format!(
            "the prepared {} runtime records a different source identity",
            variant.label
        )));
    }
    let source = resolved(&record.source_root).map_err(|error| {
        invalid(format!(
            "the recorded kit source of the {} runtime is unavailable ({error})",
            variant.label
        ))
    })?;
    if !source.is_dir() {
        return Err(invalid("the recorded kit source is not a directory"));
    }
    let check = build_identity::check(&build, Some(&source));
    if check.status != build_identity::Health::Healthy {
        return Err(invalid(format!(
            "the prepared {} runtime is not a verified unchanged build: {}",
            variant.label, check.action
        )));
    }

    let home = plain(&request.home)?;
    let user_home = plain(&request.user_home)?;
    let dependency_user_home = plain(&request.dependency_user_home)?;
    for (name, path) in [
        ("home", &home),
        ("user home", &user_home),
        ("dependency user home", &dependency_user_home),
    ] {
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(invalid(format!(
                "the {name} must be an absolute normalized directory"
            )));
        }
        if !path.is_dir() {
            return Err(invalid(format!(
                "the {name} {} is not a fresh owned directory; prepare the homes first",
                path.display()
            )));
        }
        build_identity::ordinary(path)?;
    }
    if home.join("harness/installation.json").exists() {
        return Err(invalid(format!(
            "{} already carries an installation; restore it through the lifecycle owner before preparing this arm",
            home.display()
        )));
    }
    if home.join("AGENTS.override.md").exists() {
        return Err(invalid(
            "the arm home carries an AGENTS override; a fresh arm home must not shadow the prepared instructions",
        ));
    }
    if let Some(ambient) = ambient_codex_home()
        && same_place(&home, &ambient)?
    {
        return Err(invalid(
            "refusing to install an experiment arm into the ambient Codex home",
        ));
    }
    if let Some(ambient) = ambient_user_home()
        && (same_place(&user_home, &ambient)? || same_place(&dependency_user_home, &ambient)?)
    {
        return Err(invalid(
            "refusing to install an experiment arm into the ambient user home",
        ));
    }
    if overlaps(&home, &user_home) || overlaps(&home, &dependency_user_home) {
        return Err(invalid(
            "the arm home must stay disjoint from its user and dependency homes",
        ));
    }
    if !same_place(&user_home, &dependency_user_home)?
        && overlaps(&user_home, &dependency_user_home)
    {
        return Err(invalid(
            "the arm user home and dependency user home must not nest inside each other",
        ));
    }
    for (name, path) in [
        ("home", &home),
        ("user home", &user_home),
        ("dependency user home", &dependency_user_home),
    ] {
        if overlaps(path, &build) {
            return Err(invalid(format!(
                "the arm {name} overlaps the prepared runtime; each experiment allocation must be separately owned"
            )));
        }
        if overlaps(path, &source) {
            return Err(invalid(format!(
                "the arm {name} overlaps its kit source checkout"
            )));
        }
    }
    for protected in &request.protected {
        for (name, path) in [
            ("home", &home),
            ("user home", &user_home),
            ("dependency user home", &dependency_user_home),
        ] {
            if overlaps(path, protected) {
                return Err(invalid(format!(
                    "the arm {name} overlaps a protected experiment allocation ({}); each allocation must be separately owned",
                    protected.display()
                )));
            }
        }
    }

    let upstream = plain(&request.upstream)?;
    if !upstream
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        || !upstream.is_absolute()
    {
        return Err(invalid(
            "the upstream client must be an absolute native executable",
        ));
    }
    let upstream_sha256 =
        hash_file("the upstream client", &upstream, FILE_LIMIT).map_err(|error| {
            invalid(format!(
                "the declared upstream client is missing or unreadable ({error})"
            ))
        })?;
    for (name, path) in [
        ("prepared runtime", &build),
        ("kit source", &source),
        ("arm home", &home),
        ("user home", &user_home),
        ("dependency user home", &dependency_user_home),
    ] {
        if overlaps(&upstream, path) {
            return Err(invalid(format!(
                "the upstream client overlaps the {name}; select the explicit original Codex client"
            )));
        }
    }
    for protected in &request.protected {
        if overlaps(&upstream, protected) {
            return Err(invalid(
                "the upstream client overlaps a protected experiment allocation",
            ));
        }
    }
    if record
        .binaries
        .values()
        .any(|digest| *digest == upstream_sha256)
    {
        return Err(invalid(
            "the declared upstream client is one of the prepared harness binaries; no launcher may stand in for the original client",
        ));
    }
    if request.timeout.is_zero() || request.timeout > MAX_TIMEOUT {
        return Err(invalid(
            "the installation timeout must be finite and within the owner's bound",
        ));
    }

    let client = request
        .client
        .as_ref()
        .map(validate_client)
        .transpose()?
        .map(|draft| -> io::Result<ValidatedClient> {
            let settings = declared_settings(&draft.table)?;
            let bytes = toml::to_string(&draft.table)
                .map_err(io::Error::other)?
                .into_bytes();
            if bytes.len() > CONFIG_LIMIT {
                return Err(invalid("the generated arm configuration exceeds its bound"));
            }
            let manifest = kit_manifest(&source)?;
            let kit_profile_path = plain(&source.join(&manifest.profile))?;
            let kit_profile = FileIdentity {
                sha256: hash_file(
                    "the arm kit profile",
                    &kit_profile_path,
                    CONFIG_LIMIT as u64,
                )
                .map_err(|error| invalid(format!("the arm kit profile is unresolved ({error})")))?,
                path: kit_profile_path,
            };
            // Refuse declared settings the arm's own kit profile would override
            // at launch: preparation never claims an unreachable value.
            let scratch = tempfile::Builder::new()
                .prefix("improvement-runtime-client-")
                .tempdir()?;
            fs::write(scratch.path().join("config.toml"), &bytes)?;
            reconcile_effective(&kit_profile.path, scratch.path(), &settings)?;
            Ok(ValidatedClient {
                record: draft.record,
                config_bytes: bytes,
                settings,
                overlay: draft.overlay,
                kit_profile,
            })
        })
        .transpose()?;
    let config_path = home.join("config.toml");
    match (&client, fs::symlink_metadata(&config_path)) {
        (Some(validated), Ok(_)) => {
            let bytes = fs::read(&config_path)?;
            let table = parse_config_bytes(&bytes, "the existing arm configuration")?;
            declared_settings_present(&table, &validated.settings).map_err(|error| {
                invalid(format!(
                    "an existing config.toml does not match the declared client inputs ({error}); a fresh arm home is never merged"
                ))
            })?;
            verify_client_values(&table, &validated.record)?;
            verify_file_identity(
                "the arm kit profile",
                &validated.kit_profile,
                CONFIG_LIMIT as u64,
            )?;
            if let Some(overlay) = &validated.overlay {
                verify_file_identity("the explicit client overlay", overlay, CONFIG_LIMIT as u64)?;
            }
            reconcile_effective(&validated.kit_profile.path, &home, &validated.settings)?;
        }
        (Some(_), Err(error)) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let private_inputs = validate_private_inputs(&home, &request.private_inputs)?;

    Ok(Plan {
        variant: variant.clone(),
        source,
        build,
        record,
        home,
        user_home,
        dependency_user_home,
        upstream,
        upstream_sha256,
        timeout: request.timeout,
        client,
        private_inputs,
    })
}

struct ClientDraft {
    record: ClientRecord,
    table: toml::Table,
    overlay: Option<FileIdentity>,
}

fn validate_client(client: &ClientInputs) -> io::Result<ClientDraft> {
    let runner = &client.runner;
    let endpoint = runner.endpoint.as_str();
    let authority = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or(""));
    let valid = authority.is_some_and(|authority| {
        !authority.is_empty()
            && !authority.contains('@')
            && !authority.chars().any(char::is_control)
    }) && endpoint.len() <= 2048
        && endpoint.bytes().all(|byte| byte.is_ascii_graphic());
    if !valid {
        return Err(invalid(
            "the declared local endpoint must be a credential-free http(s) URL",
        ));
    }
    bounded_token("the declared served model", &runner.model, 256)?;
    for name in crate::outcome_qualification::MATERIAL_FIELDS {
        if let Some(value) = runner.identity.declared(name)
            && (value.len() > 512 || value.chars().any(char::is_control))
        {
            return Err(invalid(format!(
                "the declared runner identity field {name} is not a bounded value"
            )));
        }
    }
    let declared_effort = client
        .reasoning_effort
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let identity_effort = runner
        .identity
        .declared("reasoning")
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("unknown"));
    let effort = match (declared_effort, identity_effort) {
        (Some(declared), Some(identity)) if !declared.eq_ignore_ascii_case(identity) => {
            return Err(invalid(
                "the declared reasoning effort contradicts the declared runner identity",
            ));
        }
        (Some(declared), _) => Some(declared.to_owned()),
        (None, Some(identity)) => Some(identity.to_owned()),
        (None, None) => None,
    };
    if let Some(effort) = &effort {
        bounded_token("the declared reasoning effort", effort, 64)?;
    }

    // Explicit overlay file: bounded ordinary TOML carrying the qualified
    // client settings beyond the route fields. Never ambient state.
    let (overlay_table, overlay) = match &client.overlay {
        Some(path) => {
            if !path.is_absolute() {
                return Err(invalid(
                    "the client overlay must be an explicit absolute TOML file",
                ));
            }
            let sha256 = hash_file("the client overlay", path, CONFIG_LIMIT as u64)
                .map_err(|error| invalid(format!("the client overlay is unresolved ({error})")))?;
            let bytes = fs::read(path)?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| invalid("the client overlay is not UTF-8"))?;
            let table: toml::Table = toml::from_str(text.trim_start_matches('\u{feff}'))
                .map_err(|_| invalid("the client overlay is not a valid TOML table"))?;
            (
                Some(table),
                Some(FileIdentity {
                    path: plain(path)?,
                    sha256,
                }),
            )
        }
        None => (None, None),
    };

    // Effective catalogue: declared by the client inputs and/or referenced by
    // the overlay; both must agree and the file stays external and hashed.
    let overlay_catalogue = overlay_table
        .as_ref()
        .and_then(|table| table.get("model_catalog_json"))
        .and_then(toml::Value::as_str)
        .map(PathBuf::from);
    let catalogue = match (client.catalogue.clone(), overlay_catalogue) {
        (Some(declared), Some(referenced)) => {
            if !same_place(&declared, &referenced)? {
                return Err(invalid(
                    "the client overlay names a different catalogue than the accepted client inputs",
                ));
            }
            Some(declared)
        }
        (Some(declared), None) => Some(declared),
        (None, Some(referenced)) => {
            if !referenced.is_absolute() {
                return Err(invalid(
                    "the catalogue reference must be an explicit absolute path",
                ));
            }
            Some(referenced)
        }
        (None, None) => None,
    };
    let catalogue = catalogue
        .as_ref()
        .map(|path| {
            let sha256 =
                hash_file("the external catalogue", path, FILE_LIMIT).map_err(|error| {
                    invalid(format!("the external catalogue is unresolved ({error})"))
                })?;
            Ok::<_, io::Error>(FileIdentity {
                path: plain(path)?,
                sha256,
            })
        })
        .transpose()?;

    // The accepted route is preserved exactly: overlay declarations that
    // contradict it are refused, never silently rewritten.
    if let Some(table) = &overlay_table {
        route_setting(table, "model", Some(runner.model.as_str()))?;
        route_setting(table, "model_provider", Some(LOCAL_PROVIDER))?;
        route_setting(table, "model_reasoning_effort", effort.as_deref())?;
        if let Some(value) = table.get("model_catalog_json") {
            let Some(accepted) = &catalogue else {
                return Err(invalid(
                    "the client overlay declares a catalogue but the accepted client inputs do not",
                ));
            };
            let referenced = value.as_str().ok_or_else(|| {
                invalid("the client overlay catalogue reference must be a string path")
            })?;
            if !same_place(Path::new(referenced), &accepted.path)? {
                return Err(invalid(
                    "the client overlay must not replace the accepted catalogue reference",
                ));
            }
        }
        for reserved in ["profile", "profiles", "projects"] {
            if table.contains_key(reserved) {
                return Err(invalid(format!(
                    "the client overlay must not carry the {reserved} layer; keep it an explicit dispatch input"
                )));
            }
        }
        if let Some(providers) = table.get("model_providers") {
            let providers = providers
                .as_table()
                .ok_or_else(|| invalid("the client overlay model_providers must be a table"))?;
            for (id, value) in providers {
                if id != LOCAL_PROVIDER {
                    return Err(invalid(
                        "the client overlay must not add alternate provider routes",
                    ));
                }
                let provider = value
                    .as_table()
                    .ok_or_else(|| invalid("the client overlay provider must be a table"))?;
                if let Some(base_url) = provider.get("base_url").and_then(toml::Value::as_str)
                    && base_url != endpoint
                {
                    return Err(invalid(
                        "the client overlay must not replace the accepted local endpoint",
                    ));
                }
                if let Some(wire) = provider.get("wire_api").and_then(toml::Value::as_str)
                    && wire != crate::outcome_qualification::WIRE_API
                {
                    return Err(invalid(
                        "the client overlay must not replace the supported wire API",
                    ));
                }
            }
        }
        if let Some(hooks) = table.get("features").and_then(|value| value.get("hooks"))
            && hooks.as_bool() != Some(false)
        {
            return Err(invalid(
                "a fresh arm home installs with hooks disabled; the client overlay must not request hooks",
            ));
        }
    }

    let mut table = route_table(&client.runner, effort.as_deref(), catalogue.as_ref())?;
    if let Some(overlay_table) = overlay_table {
        deep_merge(&mut table, overlay_table);
    }
    if toml::to_string(&table).map_err(io::Error::other)?.len() > CONFIG_LIMIT {
        return Err(invalid("the generated arm configuration exceeds its bound"));
    }
    let record = ClientRecord {
        runner: RunnerRecord::new(&client.runner),
        reasoning_effort: effort,
        catalogue,
    };
    Ok(ClientDraft {
        record,
        table,
        overlay,
    })
}

/// One overlay declaration of an accepted route key must agree with the
/// accepted inputs; an absent accepted value refuses a declared one instead of
/// letting the overlay choose it.
fn route_setting(table: &toml::Table, key: &str, accepted: Option<&str>) -> io::Result<()> {
    let Some(value) = table.get(key) else {
        return Ok(());
    };
    let Some(accepted) = accepted else {
        return Err(invalid(format!(
            "the client overlay declares {key} but the accepted client inputs do not; declare the accepted value instead"
        )));
    };
    if value.as_str() != Some(accepted) {
        return Err(invalid(format!(
            "the client overlay must not replace the accepted {key}"
        )));
    }
    Ok(())
}

/// The accepted route as a TOML table: model, provider, effort, catalogue
/// reference and the explicit local provider entry.
fn route_table(
    runner: &LocalRunner,
    effort: Option<&str>,
    catalogue: Option<&FileIdentity>,
) -> io::Result<toml::Table> {
    let mut table = toml::Table::new();
    table.insert("model".into(), toml::Value::String(runner.model.clone()));
    table.insert(
        "model_provider".into(),
        toml::Value::String(LOCAL_PROVIDER.into()),
    );
    if let Some(effort) = effort {
        table.insert(
            "model_reasoning_effort".into(),
            toml::Value::String(effort.to_owned()),
        );
    }
    if let Some(catalogue) = catalogue {
        table.insert(
            "model_catalog_json".into(),
            toml::Value::String(plain(&catalogue.path)?.to_string_lossy().into_owned()),
        );
    }
    let mut provider = toml::Table::new();
    provider.insert("name".into(), toml::Value::String("Local".into()));
    provider.insert(
        "base_url".into(),
        toml::Value::String(runner.endpoint.clone()),
    );
    provider.insert(
        "wire_api".into(),
        toml::Value::String(crate::outcome_qualification::WIRE_API.into()),
    );
    let mut providers = toml::Table::new();
    providers.insert(LOCAL_PROVIDER.into(), toml::Value::Table(provider));
    table.insert("model_providers".into(), toml::Value::Table(providers));
    Ok(table)
}

fn deep_merge(base: &mut toml::Table, overlay: toml::Table) {
    for (key, value) in overlay {
        let nested = matches!(
            (base.get(&key), &value),
            (Some(toml::Value::Table(_)), toml::Value::Table(_))
        );
        if nested {
            if let (Some(toml::Value::Table(existing)), toml::Value::Table(incoming)) =
                (base.get_mut(&key), value)
            {
                deep_merge(existing, incoming);
            }
            continue;
        }
        base.insert(key, value);
    }
}

/// Every declared client setting as a bounded dotted path with its exact
/// value, sorted for stable receipts.
fn declared_settings(table: &toml::Table) -> io::Result<Vec<DeclaredSetting>> {
    const MAX_SETTINGS: usize = 256;
    let mut settings = Vec::new();
    flatten_settings("", table, &mut settings)?;
    settings.sort_by(|left, right| left.key.cmp(&right.key));
    if settings.len() > MAX_SETTINGS {
        return Err(invalid(
            "the declared client configuration exceeds its bound",
        ));
    }
    Ok(settings)
}

fn flatten_settings(
    prefix: &str,
    table: &toml::Table,
    settings: &mut Vec<DeclaredSetting>,
) -> io::Result<()> {
    for (key, value) in table {
        if !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        {
            return Err(invalid(
                "declared client setting keys must be plain dotted components",
            ));
        }
        let dotted = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Table(inner) => flatten_settings(&dotted, inner, settings)?,
            leaf => settings.push(DeclaredSetting {
                key: dotted,
                value: leaf.clone(),
            }),
        }
    }
    Ok(())
}

fn lookup<'a>(table: &'a toml::Table, key: &str) -> Option<&'a toml::Value> {
    let mut parts = key.split('.');
    let mut current = table.get(parts.next()?)?;
    for part in parts {
        current = current.as_table()?.get(part)?;
    }
    Some(current)
}

fn parse_config_bytes(bytes: &[u8], name: &str) -> io::Result<toml::Table> {
    if bytes.len() > CONFIG_LIMIT {
        return Err(invalid(format!("{name} exceeds its bound")));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid(format!("{name} is not UTF-8")))?;
    toml::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|_| invalid(format!("{name} is not valid TOML")))
}

fn declared_settings_present(table: &toml::Table, settings: &[DeclaredSetting]) -> io::Result<()> {
    for setting in settings {
        match lookup(table, &setting.key) {
            Some(value) if *value == setting.value => {}
            _ => {
                return Err(invalid(format!(
                    "the declared client setting {} changed since preparation",
                    setting.key
                )));
            }
        }
    }
    Ok(())
}

fn verify_file_identity(name: &str, identity: &FileIdentity, limit: u64) -> io::Result<()> {
    let sha256 = hash_file(name, &identity.path, limit)
        .map_err(|error| invalid(format!("{name} is unresolved ({error})")))?;
    if sha256 != identity.sha256 {
        return Err(invalid(format!("{name} changed since preparation")));
    }
    Ok(())
}

/// Reconcile the declared client settings against the launch-time overrides
/// the installed launcher actually computes from the arm's own kit profile.
/// A declared setting the kit profile would override at launch is refused
/// instead of being counted as consumed.
fn reconcile_effective(
    kit_profile: &Path,
    home: &Path,
    settings: &[DeclaredSetting],
) -> io::Result<()> {
    let args = crate::portable_config::overrides(kit_profile, home, home)?;
    let mut overridden: BTreeMap<String, toml::Value> = BTreeMap::new();
    let (pairs, remainder) = args.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(invalid("native overrides are unbalanced"));
    }
    for pair in pairs {
        if pair[0] != "-c" {
            return Err(invalid("unexpected native override form"));
        }
        let encoded = pair[1]
            .to_str()
            .ok_or_else(|| invalid("a native override is not Unicode"))?;
        let (key, value) = encoded
            .split_once('=')
            .ok_or_else(|| invalid("a native override is malformed"))?;
        let parsed: toml::Table = toml::from_str(&format!("value={value}"))
            .map_err(|_| invalid("a native override value is not TOML"))?;
        overridden.insert(key.to_owned(), parsed["value"].clone());
    }
    for setting in settings {
        if let Some(value) = overridden.get(&setting.key)
            && *value != setting.value
        {
            return Err(invalid(format!(
                "the arm kit profile would override the declared client setting {} at launch",
                setting.key
            )));
        }
    }
    Ok(())
}

fn kit_manifest(source: &Path) -> io::Result<inventory::Manifest> {
    let bytes = fs::read(source.join("global/kit.json"))
        .map_err(|error| invalid(format!("the arm kit manifest is unavailable ({error})")))?;
    serde_json::from_slice(&bytes).map_err(|_| invalid("the arm kit manifest is invalid"))
}

fn validate_private_inputs(
    home: &Path,
    inputs: &[PrivateInput],
) -> io::Result<Vec<ValidatedPrivate>> {
    let mut seen = BTreeSet::new();
    let mut validated = Vec::new();
    for input in inputs {
        if !input.source.is_absolute() {
            return Err(invalid(
                "every private runner input needs an explicit absolute source file",
            ));
        }
        hash_file("a private runner input", &input.source, FILE_LIMIT).map_err(|error| {
            invalid(format!(
                "the private input {} is unresolved ({error})",
                input.destination
            ))
        })?;
        let relative = Path::new(&input.destination);
        let valid = !input.destination.is_empty()
            && !relative.is_absolute()
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)));
        let first = relative
            .components()
            .next()
            .and_then(|component| match component {
                Component::Normal(name) => Some(name.to_string_lossy().to_ascii_lowercase()),
                _ => None,
            });
        let reserved = first.as_deref() == Some("harness")
            || first.as_deref() == Some("agents")
            || matches!(
                input.destination.to_ascii_lowercase().as_str(),
                "config.toml" | "agents.md" | "agents.override.md"
            );
        if !valid || reserved {
            return Err(invalid(format!(
                "the private input destination {} is not a plain relative path outside installation-owned state",
                input.destination
            )));
        }
        if !seen.insert(input.destination.to_ascii_lowercase()) {
            return Err(invalid(
                "duplicate private input destination in one arm home",
            ));
        }
        let destination = home.join(&input.destination);
        match fs::symlink_metadata(&destination) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(_) => {
                if fs::read(&destination)? != fs::read(&input.source)? {
                    return Err(invalid(format!(
                        "the private input destination {} already exists with different content; a fresh arm home is never merged",
                        input.destination
                    )));
                }
            }
        }
        validated.push(ValidatedPrivate {
            source: input.source.clone(),
            destination: input.destination.clone(),
        });
    }
    Ok(validated)
}

/// Install one arm into its fresh owned homes through the real installation
/// owner and retain the verified consumption identity.
///
/// The build record's source root is authoritative for the kit content, so a
/// build and a source can never disagree. On any failure the error names the
/// arm and no receipt is returned: an unfinished or unverified arm is never
/// presented as ready. Installed state is restored through
/// [`retire_arm`]/[`discard_arm`], never rewritten in place.
pub fn install_arm(request: &ArmRequest) -> io::Result<ArmRuntime> {
    let plan = prepare_plan(request)?;
    let preview = core_install::connect(&plan.connect_request(), true).map_err(|error| {
        invalid(format!(
            "the {} arm failed its installation preflight ({error}); nothing was installed",
            plan.variant.label
        ))
    })?;
    if preview.status != "preview" || preview.model_calls != 0 || preview.runtime.is_some() {
        return Err(invalid(
            "the installation preflight did not report a model-free preview",
        ));
    }

    if let Some(client) = &plan.client {
        let path = plan.home.join("config.toml");
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::write(&path, &client.config_bytes)?;
            }
            Err(error) => return Err(error),
            Ok(_) => {}
        }
    }
    for input in &plan.private_inputs {
        let destination = plan.home.join(&input.destination);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::symlink_metadata(&destination) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::copy(&input.source, &destination)?;
            }
            Err(error) => return Err(error),
            Ok(_) => {
                // An identical destination from the same explicit input is
                // reused; changed content is never recorded as prepared.
                if fs::read(&destination)? != fs::read(&input.source)? {
                    return Err(invalid(format!(
                        "the private input destination {} changed during preparation; preserving it",
                        input.destination
                    )));
                }
            }
        }
    }

    let report = core_install::connect(&plan.connect_request(), false).map_err(|error| {
        invalid(format!(
            "the {} arm installation was not completed ({error}); the arm is not ready and its owned state can be restored with the lifecycle owner",
            plan.variant.label
        ))
    })?;
    if report.status != "connected" || report.model_calls != 0 {
        return Err(invalid(format!(
            "the {} arm installation did not report a completed model-free connection",
            plan.variant.label
        )));
    }
    let runtime = report
        .runtime
        .as_ref()
        .ok_or_else(|| invalid("the installation did not retain runtime validation evidence"))?;
    if !runtime.passed || runtime.model_calls != 0 {
        return Err(invalid(
            "the installation's runtime validation did not pass model-free",
        ));
    }
    let facts = InstallationFacts {
        status: report.status.to_owned(),
        links: report.links,
        changed_links: report.changed_links,
        path_change: report.path_change,
        runtime_executable_sha256: runtime.executable_sha256.clone(),
        runtime_evidence: runtime.evidence.clone(),
    };
    let observed = observe(&plan, facts)?;
    verify_consumption(&observed).map_err(|error| {
        invalid(format!(
            "the {} arm installation was applied but its consumption identity did not verify ({error}); it is not ready and must be restored through the lifecycle owner",
            plan.variant.label
        ))
    })?;
    Ok(observed)
}

fn checked_link(destination: &Path, source: &Path, directory: bool) -> io::Result<PathBuf> {
    let destination = plain(destination)?;
    let source = plain(source)?;
    registration_native::verified_link(&destination, &source, directory).map_err(|error| {
        invalid(format!(
            "the installed link {} does not point at its prepared source ({error})",
            destination.display()
        ))
    })?;
    Ok(source)
}

fn file_link(name: &str, destination: &Path, source: &Path) -> io::Result<InstalledLink> {
    let source = checked_link(destination, source, false)?;
    let sha256 = hash_file("an installed source file", &source, FILE_LIMIT)?;
    Ok(InstalledLink {
        name: name.to_owned(),
        destination: plain(destination)?,
        source,
        sha256,
    })
}

fn directory_link(name: &str, destination: &Path, source: &Path) -> io::Result<InstalledLink> {
    let source = checked_link(destination, source, true)?;
    let sha256 = directory_digest(name, &source)?;
    Ok(InstalledLink {
        name: name.to_owned(),
        destination: plain(destination)?,
        source,
        sha256,
    })
}

fn observe(plan: &Plan, facts: InstallationFacts) -> io::Result<ArmRuntime> {
    let inventory = inventory::read(&plan.source, &plan.home, &plan.user_home)?;
    let instructions = inventory
        .links
        .iter()
        .find(|link| link.kind == "instructions")
        .ok_or_else(|| invalid("the kit inventory has no instructions link"))?;
    let agents = inventory
        .links
        .iter()
        .find(|link| link.kind == "agents")
        .ok_or_else(|| invalid("the kit inventory has no agents link"))?;
    let instructions = file_link("AGENTS", &instructions.destination, &instructions.source)?;
    let agents = directory_link(&agents.name, &agents.destination, &agents.source)?;

    let mut skills = Vec::new();
    for link in inventory.links.iter().filter(|link| link.kind == "skill") {
        skills.push(directory_link(&link.name, &link.destination, &link.source)?);
    }
    let mut commands = Vec::new();
    for name in core_install::core_linked_binaries() {
        if name == "codex.exe" {
            continue;
        }
        let destination = plan.home.join("harness/bin").join(name);
        let source = plan.build.join(name);
        let link = file_link(name, &destination, &source)?;
        let expected = plan
            .record
            .binaries
            .get(name)
            .ok_or_else(|| invalid("the build record is missing a linked command"))?;
        if link.sha256 != *expected {
            return Err(invalid(format!(
                "the installed command {name} does not match the prepared build record"
            )));
        }
        commands.push(link);
    }
    let launcher = file_link(
        "codex",
        &plan.home.join("harness/bin/codex.exe"),
        &plan.build.join("codex.exe"),
    )?;
    let expected_launcher = plan
        .record
        .binaries
        .get("codex.exe")
        .ok_or_else(|| invalid("the build record is missing the launcher"))?;
    if launcher.sha256 != *expected_launcher {
        return Err(invalid(
            "the installed launcher does not match the prepared build record",
        ));
    }

    let launch_registration = plan.home.join("harness/native-launch.json");
    let launch_bytes = fs::read(&launch_registration)?;
    if launch_bytes.len() > CONFIG_LIMIT {
        return Err(invalid("the launch registration exceeds its bound"));
    }
    let launch_sha256 = build_identity::hash_bytes(&launch_bytes);
    let registration: LaunchRegistration = serde_json::from_slice(&launch_bytes)
        .map_err(|_| invalid("invalid launch registration"))?;
    let registered = registration
        .build
        .as_ref()
        .ok_or_else(|| invalid("the launch registration does not name a build"))?;
    if registration.schema != 2
        || registration.state.is_some()
        || registration.task_control
        || !same_place(registered, &plan.build)?
        || !same_place(&registration.upstream.executable, &plan.upstream)?
        || registration.upstream.sha256 != plan.upstream_sha256
    {
        return Err(invalid(
            "the launch registration does not consume this prepared runtime and client",
        ));
    }

    let config_path = plan.home.join("config.toml");
    let configuration = match fs::symlink_metadata(&config_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
        Ok(_) => {
            let sha256 = hash_file("the arm configuration", &config_path, CONFIG_LIMIT as u64)?;
            Some(ConfigurationRecord {
                path: plain(&config_path)?,
                sha256,
                client: plan.client.as_ref().map(|client| client.record.clone()),
                settings: plan
                    .client
                    .as_ref()
                    .map(|client| client.settings.clone())
                    .unwrap_or_default(),
                overlay: plan
                    .client
                    .as_ref()
                    .and_then(|client| client.overlay.clone()),
                kit_profile: plan
                    .client
                    .as_ref()
                    .map(|client| client.kit_profile.clone()),
            })
        }
    };
    if plan.client.is_some() && configuration.is_none() {
        return Err(invalid(
            "the declared client inputs were not published into the arm configuration",
        ));
    }

    let mut private = Vec::new();
    for input in &plan.private_inputs {
        let destination = plan.home.join(&input.destination);
        let sha256 = hash_file("a copied private runner input", &destination, FILE_LIMIT)?;
        private.push(PrivateRecord {
            destination: input.destination.clone(),
            sha256,
        });
    }

    Ok(ArmRuntime {
        schema: RUNTIME_SCHEMA,
        arm: plan.variant.arm,
        label: plan.variant.label.clone(),
        variant: plan.variant.clone(),
        source: plan.source.clone(),
        home: plan.home.clone(),
        user_home: plan.user_home.clone(),
        dependency_user_home: plan.dependency_user_home.clone(),
        upstream: plan.upstream.clone(),
        upstream_sha256: plan.upstream_sha256.clone(),
        launcher,
        launch_registration: plain(&launch_registration)?,
        launch_sha256,
        instructions,
        agents,
        skills,
        commands,
        configuration,
        private,
        installation: facts,
        model_calls: 0,
    })
}

fn verify_link_inside(
    link: &InstalledLink,
    source_root: &Path,
    destination_root: &Path,
) -> io::Result<()> {
    if !inside(&link.source, source_root) {
        return Err(invalid(format!(
            "the recorded link source {} escapes its source root",
            link.source.display()
        )));
    }
    if !inside(&link.destination, destination_root) {
        return Err(invalid(format!(
            "the recorded link destination {} escapes its owned root",
            link.destination.display()
        )));
    }
    Ok(())
}

/// Re-verify read-only that this arm home still consumes exactly the prepared
/// runtime, instructions, skills, tools and configuration. Any missing,
/// changed or redirected identity refuses the arm without falling back.
pub fn verify_consumption(runtime: &ArmRuntime) -> io::Result<Consumption> {
    if runtime.schema != RUNTIME_SCHEMA {
        return Err(invalid("unsupported arm runtime schema"));
    }
    if runtime.model_calls != 0 || runtime.installation.runtime_executable_sha256.is_empty() {
        return Err(invalid(
            "the retained arm identity does not describe a model-free installation",
        ));
    }
    if runtime.arm != runtime.variant.arm || runtime.label != runtime.variant.label {
        return Err(invalid(
            "the retained arm identity does not match its prepared variant",
        ));
    }
    let build = resolved(&runtime.variant.build)?;
    let record = build_identity::verify_record_integrity(&build).map_err(|error| {
        invalid(format!(
            "the prepared {} runtime is no longer intact ({error})",
            runtime.label
        ))
    })?;
    if build_identity::hash_file(&build.join("build.json"))? != runtime.variant.record_sha256 {
        return Err(invalid(format!(
            "the prepared {} runtime changed since preparation; prepare it again",
            runtime.label
        )));
    }
    if record.source.sha256 != runtime.variant.source_sha256 {
        return Err(invalid(
            "the prepared runtime records a different source identity",
        ));
    }
    let source = resolved(&record.source_root)?;
    if !same_place(&source, &runtime.source)? {
        return Err(invalid(
            "the prepared runtime no longer records this kit source",
        ));
    }
    let check = build_identity::check(&build, Some(&source));
    if check.status != build_identity::Health::Healthy {
        return Err(invalid(format!(
            "the prepared {} runtime is no longer a verified unchanged build: {}",
            runtime.label, check.action
        )));
    }

    if !runtime.upstream.is_file() || build_identity::ordinary(&runtime.upstream).is_err() {
        return Err(invalid(
            "the declared upstream client is missing or is not an ordinary file",
        ));
    }
    if build_identity::hash_file(&runtime.upstream)? != runtime.upstream_sha256 {
        return Err(invalid(
            "the declared upstream client changed since preparation",
        ));
    }

    let metadata = crate::installation_metadata::InstallationMetadata::read(
        &runtime.home,
        &runtime.user_home,
        &runtime.dependency_user_home,
    )?
    .ok_or_else(|| invalid("the arm home no longer carries an installation"))?;
    if !same_place(&metadata.settings().source_root, &runtime.source)? {
        return Err(invalid(
            "the arm home carries an installation from another source",
        ));
    }

    if !runtime.skills.is_empty() {
        let names: Vec<&str> = runtime
            .skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect();
        let lock = InstallationLock::acquire(&runtime.user_home)?;
        let owned = crate::installation_links::OwnedSkillLinks::read(
            &runtime.source,
            &runtime.home,
            &runtime.user_home,
            &runtime.dependency_user_home,
            &names,
            lock,
        )?;
        let receipt = owned.receipt();
        for recorded in &runtime.skills {
            let live = receipt
                .skills
                .iter()
                .find(|skill| skill.name == recorded.name)
                .ok_or_else(|| {
                    invalid(format!(
                        "the installed {} skill link is no longer owned by this installation",
                        recorded.name
                    ))
                })?;
            if !same_place(&live.source, &recorded.source)?
                || !same_place(&live.destination, &recorded.destination)?
            {
                return Err(invalid(format!(
                    "the installed {} skill link changed since preparation",
                    recorded.name
                )));
            }
        }
        owned.verify_unchanged()?;
    }

    verify_link_inside(&runtime.instructions, &runtime.source, &runtime.home)?;
    verify_link_inside(&runtime.agents, &runtime.source, &runtime.home)?;
    if !inside(&runtime.launcher.source, &build)
        || !inside(&runtime.launcher.destination, &runtime.home)
    {
        return Err(invalid("the launcher link escapes its owned roots"));
    }
    if !inside(&runtime.launch_registration, &runtime.home) {
        return Err(invalid("the launch registration escapes the arm home"));
    }
    for command in &runtime.commands {
        verify_link_inside(command, &build, &runtime.home)?;
        let expected = record.binaries.get(&command.name);
        if expected.is_some_and(|digest| *digest != command.sha256) {
            return Err(invalid(format!(
                "the installed {} command no longer matches the prepared build record",
                command.name
            )));
        }
    }

    file_link_unchanged(&runtime.instructions)?;
    directory_link_unchanged(&runtime.agents)?;
    for skill in &runtime.skills {
        verify_link_inside(skill, &runtime.source, &runtime.user_home)?;
        directory_link_unchanged(skill)?;
    }
    for command in &runtime.commands {
        file_link_unchanged(command)?;
    }
    file_link_unchanged(&runtime.launcher)?;
    let expected_launcher = record
        .binaries
        .get("codex.exe")
        .ok_or_else(|| invalid("the build record is missing the launcher"))?;
    if runtime.launcher.sha256 != *expected_launcher {
        return Err(invalid(
            "the installed launcher no longer matches the prepared build record",
        ));
    }

    let launch_bytes = fs::read(&runtime.launch_registration)?;
    if launch_bytes.len() > CONFIG_LIMIT {
        return Err(invalid("the launch registration exceeds its bound"));
    }
    if build_identity::hash_bytes(&launch_bytes) != runtime.launch_sha256 {
        return Err(invalid("the launch registration changed since preparation"));
    }
    let registration: LaunchRegistration = serde_json::from_slice(&launch_bytes)
        .map_err(|_| invalid("invalid launch registration"))?;
    if registration.schema != 2
        || registration.state.is_some()
        || registration.task_control
        || !registration
            .build
            .as_ref()
            .is_some_and(|registered| same_place(registered, &build).unwrap_or(false))
        || !same_place(&registration.upstream.executable, &runtime.upstream)?
        || registration.upstream.sha256 != runtime.upstream_sha256
    {
        return Err(invalid(
            "the launch registration no longer consumes this prepared runtime and client",
        ));
    }

    if let Some(configuration) = &runtime.configuration {
        let bytes = fs::read(&configuration.path)?;
        if build_identity::hash_bytes(&bytes) != configuration.sha256 {
            // The ordinary dispatch trusts its own bound workspace in this
            // same file before the first model request
            // (`executor_cli::ensure_workspace_trust` appends one
            // `[projects.'<workspace>'] trust_level = "trusted"` block per
            // slot), so a prepared arm legitimately gains exactly those
            // trailing entries after preparation. Accept nothing else:
            // removing them must reproduce the prepared bytes exactly.
            let restored = strip_appended_trust(&bytes)
                .filter(|stripped| build_identity::hash_bytes(stripped) == configuration.sha256);
            if restored.is_none() {
                return Err(invalid("the arm configuration changed since preparation"));
            }
        }
        let document = parse_config_bytes(&bytes, "the arm configuration")?;
        declared_settings_present(&document, &configuration.settings)?;
        if let Some(client) = &configuration.client {
            verify_client_values(&document, client)?;
            let kit_profile = configuration.kit_profile.as_ref().ok_or_else(|| {
                invalid("the arm configuration record lost its kit profile identity")
            })?;
            if !inside(&kit_profile.path, &runtime.source) {
                return Err(invalid("the recorded arm kit profile escapes its source"));
            }
            verify_file_identity("the arm kit profile", kit_profile, CONFIG_LIMIT as u64)?;
            reconcile_effective(&kit_profile.path, &runtime.home, &configuration.settings)?;
        }
        if let Some(overlay) = &configuration.overlay {
            verify_file_identity("the explicit client overlay", overlay, CONFIG_LIMIT as u64)?;
        }
    } else if runtime.client_configured() {
        return Err(invalid(
            "the declared client configuration is no longer recorded",
        ));
    }

    for private in &runtime.private {
        let destination = runtime.home.join(&private.destination);
        let sha256 = hash_file("a copied private runner input", &destination, FILE_LIMIT)?;
        if sha256 != private.sha256 {
            return Err(invalid(format!(
                "the private runner input {} changed since preparation",
                private.destination
            )));
        }
    }

    Ok(Consumption {
        schema: RUNTIME_SCHEMA,
        status: "consumed".into(),
        arm: runtime.arm,
        label: runtime.label.clone(),
        build,
        record_sha256: runtime.variant.record_sha256.clone(),
        launcher_sha256: runtime.launcher.sha256.clone(),
        model_ready: runtime.client_configured(),
        model_calls: 0,
    })
}

/// Removes the trailing trusted-project blocks the ordinary dispatch appends
/// to an arm configuration (`executor_cli::ensure_workspace_trust` writes
/// `\n[projects.'<workspace>']\ntrust_level = "trusted"\n` once per slot).
/// Anything else - including an appended comment or another projects entry -
/// keeps its bytes and therefore still fails the comparison.
fn strip_appended_trust(bytes: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut current = text;
    loop {
        let Some(rest) = current.strip_suffix("\ntrust_level = \"trusted\"\n") else {
            return Some(current.as_bytes().to_vec());
        };
        let section = rest.rsplit('\n').next()?;
        let workspace = section.strip_prefix("[projects.'")?.strip_suffix("']")?;
        if workspace.is_empty() || workspace.contains('\'') {
            return None;
        }
        current = &rest[..rest.len() - section.len() - 1];
    }
}

fn file_link_unchanged(link: &InstalledLink) -> io::Result<()> {
    let source = checked_link(&link.destination, &link.source, false)?;
    let sha256 = hash_file("an installed source file", &source, FILE_LIMIT)?;
    if sha256 != link.sha256 {
        return Err(invalid(format!(
            "the installed {} content changed since preparation",
            link.name
        )));
    }
    Ok(())
}

fn directory_link_unchanged(link: &InstalledLink) -> io::Result<()> {
    let source = checked_link(&link.destination, &link.source, true)?;
    let sha256 = directory_digest(&link.name, &source)?;
    if sha256 != link.sha256 {
        return Err(invalid(format!(
            "the installed {} content changed since preparation",
            link.name
        )));
    }
    Ok(())
}

fn verify_client_values(document: &toml::Table, client: &ClientRecord) -> io::Result<()> {
    let string = |name: &str| -> Option<String> {
        document
            .get(name)
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    };
    if string("model").as_deref() != Some(client.runner.model.as_str()) {
        return Err(invalid(
            "the arm configuration no longer selects the accepted served model",
        ));
    }
    if string("model_provider").as_deref() != Some(LOCAL_PROVIDER) {
        return Err(invalid(
            "the arm configuration no longer selects the explicit local provider",
        ));
    }
    if string("model_reasoning_effort").as_deref() != client.reasoning_effort.as_deref() {
        return Err(invalid(
            "the arm configuration reasoning effort does not match the accepted selection",
        ));
    }
    let provider = document
        .get("model_providers")
        .and_then(|value| value.get(LOCAL_PROVIDER))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| invalid("the arm configuration has no explicit local provider table"))?;
    if provider.get("base_url").and_then(toml::Value::as_str)
        != Some(client.runner.endpoint.as_str())
    {
        return Err(invalid(
            "the arm configuration endpoint does not match the accepted local endpoint",
        ));
    }
    if provider.get("wire_api").and_then(toml::Value::as_str)
        != Some(crate::outcome_qualification::WIRE_API)
    {
        return Err(invalid(
            "the arm configuration does not select the supported wire API",
        ));
    }
    match &client.catalogue {
        Some(catalogue) => {
            let declared = string("model_catalog_json")
                .ok_or_else(|| invalid("the arm configuration lost its catalogue reference"))?;
            if !same_place(Path::new(&declared), &catalogue.path)? {
                return Err(invalid(
                    "the arm configuration catalogue reference changed since preparation",
                ));
            }
            verify_file_identity("the external catalogue", catalogue, FILE_LIMIT)?;
        }
        None => {
            if string("model_catalog_json").is_some() {
                return Err(invalid(
                    "the arm configuration carries a catalogue that was never declared",
                ));
            }
        }
    }
    Ok(())
}

/// Restore one prepared arm through the disconnection owner: recorded owned
/// links and the process-local PATH entry are removed, unrelated configuration
/// and adopted objects are preserved, and no directory is deleted.
pub fn retire_arm(runtime: &ArmRuntime) -> io::Result<Retirement> {
    if runtime.schema != RUNTIME_SCHEMA {
        return Err(invalid("unsupported arm runtime schema"));
    }
    if let Some(metadata) = crate::installation_metadata::InstallationMetadata::read(
        &runtime.home,
        &runtime.user_home,
        &runtime.dependency_user_home,
    )? && !same_place(&metadata.settings().source_root, &runtime.source)?
    {
        return Err(invalid(
            "this home now carries another installation; preserving it",
        ));
    }
    disconnect_homes(
        &runtime.home,
        &runtime.user_home,
        &runtime.dependency_user_home,
    )
}

/// Restore the explicit homes of an arm whose preparation failed before a
/// receipt existed. An uninstalled home is a no-op.
pub fn discard_arm(
    home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
) -> io::Result<Retirement> {
    disconnect_homes(home, user_home, dependency_user_home)
}

fn disconnect_homes(
    home: &Path,
    user_home: &Path,
    dependency_user_home: &Path,
) -> io::Result<Retirement> {
    let report = core_disconnect::disconnect(
        &plain(home)?,
        &plain(user_home)?,
        &plain(dependency_user_home)?,
        false,
    )?;
    Ok(Retirement {
        schema: RUNTIME_SCHEMA,
        status: report.status.to_owned(),
        removed_links: report.removed_links,
        already_missing: report.already_missing,
        preserved_adopted: report.preserved_adopted,
        path_change: report.path_change,
        model_calls: 0,
    })
}
