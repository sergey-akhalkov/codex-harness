//! Explicit candidate preparation. This module never changes active registrations.
//! Compiler concurrency stays within the installation's fixed resource budget.
use crate::build_identity::{self, BINARIES, Profile};
use crate::heavy_command;
use crate::process::{
    Cancellation, CommandSpec, Deadline, ExclusiveFileLock, Job, Limits, StopReason,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(crate) const OWNER: &[u8] = b"codex-harness-native-state-v1\n";
const TARGET: &str = "x86_64-pc-windows-msvc";
/// Dedicated owned scratch root beneath the process temp directory. The name
/// stays short because nested compiler paths contend for the MSVC path limit.
const SCRATCH_ROOT_NAME: &str = "chx";
/// Ownership record file written into the owned root and every entry.
const SCRATCH_OWNER_FILE: &str = "owner";
/// Lease object held exclusively while an owned entry is in use. Keeping it
/// separate from the record leaves the ownership identity readable even while
/// a live operation holds the lease.
const SCRATCH_LEASE_FILE: &str = "lease";
/// Leading identity line of an ownership record. Ownership is this content,
/// never a directory name, timestamp or process identifier.
const SCRATCH_OWNER: &str = "codex-harness-native-scratch-v1\n";
/// A persistent fast target has distinct ownership, so scratch hygiene never
/// retires it just because a build released its lease.
const FAST_OWNER: &str = "codex-harness-native-fast-target-v1\n";
const FAST_RECORD_FILE: &str = "last.json";
const FAST_USED_FILE: &str = "used";
const FAST_SCHEMA: u32 = 1;
/// Unused fast targets are retired only after a full quiet period. The lease,
/// not this age, decides whether a target is currently active.
const FAST_INACTIVITY: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const DELIVERY_SELECTION_FILE: &str = "delivery-selection.json";
pub const COMPILED_INPUT_SELECTIONS: &str = "compiled-input-selections";
const COMPILED_INPUT_RELATIONSHIP: &str = "compiled-inputs-match";
const CURRENT_CHECKOUT_RELATIONSHIP: &str = "current-checkout";

#[path = "native_handoff.rs"]
mod handoff;

// Shared bounded execution for explicit management operations. Configuration
// preparation uses the same job/output protections as build handoff.
pub(crate) use handoff::invoke as invoke_management;

/// Versioned internal command executed only by the fresh compiler output.
pub fn finalize(request: &Path) -> io::Result<()> {
    if answer_delivery_probe(request)? {
        return Ok(());
    }
    handoff::finalize(request)
}

/// Internal delivery query answered by a freshly compiled manager. The request
/// shape is deliberately distinct from a finalization request, so a manager
/// built before this protocol rejects it and the caller completes the build the
/// pre-query way instead.
const DELIVERY_PROBE_KIND: &str = "delivery-binaries-v1";

#[derive(Deserialize)]
struct DeliveryProbe {
    schema: u32,
    kind: String,
}

#[derive(Deserialize)]
struct DeliveryProbeResponse {
    schema: u32,
    delivery: Vec<String>,
}

/// Answer a delivery query from a producer, or report that this is a normal
/// finalization request. Read-only: no state is read or written either way.
fn answer_delivery_probe(request: &Path) -> io::Result<bool> {
    let bytes = handoff::bounded_bytes(request)?;
    let Ok(probe) = serde_json::from_slice::<DeliveryProbe>(&bytes) else {
        return Ok(false);
    };
    if probe.schema != 1 || probe.kind != DELIVERY_PROBE_KIND {
        return Ok(false);
    }
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "schema": 1,
            "delivery": BINARIES,
        }))?
    );
    Ok(true)
}

/// Read the source-owned delivery declaration before a manager exists. An
/// absent, malformed or unsafe declaration is unavailable, not an empty or
/// inferred set; the caller retains the two-phase manager query.
fn declared_delivery(manifest: &Path) -> Option<Vec<String>> {
    let text = fs::read_to_string(manifest).ok()?;
    let document: toml::Value = toml::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let entries = document
        .get("workspace")?
        .get("metadata")?
        .get("harness")?
        .get("delivery")?
        .as_array()?;
    let mut delivery = Vec::with_capacity(entries.len());
    let mut seen = BTreeSet::new();
    for entry in entries {
        let name = entry.as_str()?;
        let stem = name.strip_suffix(".exe")?;
        if stem.is_empty()
            || !stem
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || !seen.insert(name)
        {
            return None;
        }
        delivery.push(name.to_owned());
    }
    (!delivery.is_empty()).then_some(delivery)
}

/// One Cargo invocation for the complete declared set. Package selection stays
/// unchanged, so a declaration cannot widen the build to unrelated members.
fn declared_delivery_cargo(
    source: &Path,
    target: &Path,
    cargo: &OsStr,
    profile: Profile,
    delivery: &[String],
) -> io::Result<CommandSpec> {
    let mut command = publication_cargo(source, target, cargo, profile)?;
    command
        .args
        .extend(publication_bin_selection(Some(delivery)));
    Ok(command)
}

fn write_delivery_selection(staging: &Path, declaration: Option<&[String]>) -> io::Result<()> {
    let evidence = match declaration {
        Some(delivery) => serde_json::json!({
            "schema": 1,
            "selection": "source-declaration",
            "one_cargo_invocation": true,
            "delivery": delivery,
        }),
        None => serde_json::json!({
            "schema": 1,
            "selection": "two-phase-manager-query",
            "one_cargo_invocation": false,
        }),
    };
    fs::write(
        staging.join(DELIVERY_SELECTION_FILE),
        serde_json::to_vec_pretty(&evidence)?,
    )
}

fn dep_info_paths(path: &Path) -> io::Result<Vec<String>> {
    ordinary_ancestors(path)?;
    if fs::metadata(path)?.len() > 8 * 1024 * 1024 {
        return Err(io::Error::other(
            "Compiled-input evidence is missing or oversized.",
        ));
    }
    let dependencies = fs::read_to_string(path)?;
    let dependencies = dependencies.replace("\\\r\n", "").replace("\\\n", "");
    let mut lines = dependencies.lines().filter(|line| !line.trim().is_empty());
    let line = lines
        .next()
        .ok_or_else(|| io::Error::other("Missing Cargo compiled-input evidence."))?;
    if lines.next().is_some() {
        return Err(io::Error::other(
            "Ambiguous Cargo dep-info; candidate not accepted.",
        ));
    }
    let (_, paths) = line
        .split_once(": ")
        .ok_or_else(|| io::Error::other("Unrecognized Cargo dep-info; candidate not accepted."))?;
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut chars = paths.chars().peekable();
    while let Some(ch) = chars.next() {
        if (ch == '\\' && chars.peek().is_some_and(|c| c.is_whitespace() || *c == '#'))
            || (ch == '$' && chars.peek() == Some(&'$'))
        {
            token.push(chars.next().unwrap());
        } else if ch.is_whitespace() {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    if tokens.is_empty() {
        return Err(io::Error::other("Missing Cargo compiled-input evidence."));
    }
    Ok(tokens)
}

fn verify_compiled_inputs(
    target: &Path,
    source: &Path,
    inputs: &build_identity::SourceIdentity,
    profile: Profile,
) -> io::Result<build_identity::CompiledInputs> {
    let canonical_target = target.canonicalize()?;
    let mut binaries = BTreeMap::new();
    let mut files = BTreeMap::new();
    for binary in BINARIES {
        let depfile = target
            .join(TARGET)
            .join(profile.output_dir())
            .join(binary)
            .with_extension("d");
        let mut source_inputs = BTreeSet::new();
        for token in dep_info_paths(&depfile)? {
            let path = PathBuf::from(token);
            let path = if path.is_absolute() {
                path
            } else {
                source.join(path)
            };
            ordinary_ancestors(&path)?;
            let path = path.canonicalize()?;
            if let Ok(relative) = path.strip_prefix(source) {
                let key = relative.to_string_lossy().replace('\\', "/");
                if !inputs.files.contains_key(&key) {
                    return Err(io::Error::other(format!(
                        "Compiled input {key} is outside the native input inventory; place executable resources under the crate's src directory before explicit build."
                    )));
                }
                files.insert(key.clone(), inputs.files[&key].clone());
                source_inputs.insert(key);
            } else if !path.starts_with(&canonical_target) {
                return Err(io::Error::other(
                    "Compiled input is outside owned source/build roots; candidate not accepted.",
                ));
            }
        }
        binaries.insert(
            (*binary).to_owned(),
            source_inputs.into_iter().collect::<Vec<_>>(),
        );
    }
    Ok(build_identity::CompiledInputs {
        schema: build_identity::COMPILED_INPUT_SCHEMA,
        source_sha256: inputs.sha256.clone(),
        build_record_sha256: String::new(),
        binaries,
        files,
        build_affecting: build_identity::build_affecting_identity(&inputs.files),
    })
}

/// Serialize mutations without adopting an unknown or missing state directory.
pub(crate) fn lock_owned_state(state: &Path) -> io::Result<ExclusiveFileLock> {
    verify_owned_state(state)?;
    let path = state.join("build.lock");
    ordinary_ancestors(&path)?;
    ExclusiveFileLock::try_acquire(&path)?.ok_or_else(|| {
        io::Error::other("Another native operation owns this state; wait for it to finish.")
    })
}

fn owner_matches(marker: &Path) -> io::Result<bool> {
    let mut bytes = Vec::new();
    fs::File::open(marker)?
        .take((OWNER.len() + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes == OWNER)
}

pub(crate) fn verify_owned_state(state: &Path) -> io::Result<()> {
    ordinary_ancestors(state)?;
    let marker = state.join("owner");
    build_identity::ordinary(&marker)?;
    if !owner_matches(&marker)? {
        return Err(io::Error::other(
            "Native state has foreign ownership; preserving it.",
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct PreparedBuild {
    pub build: PathBuf,
    pub reused: bool,
    pub source_identity: String,
}

/// An immutable record of why a verified fast candidate is still applicable
/// to the current checkout. It references the original build; it never
/// rewrites its record or binaries.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledInputSelection {
    pub schema: u32,
    pub build: String,
    pub profile: String,
    pub build_record_sha256: String,
    pub binaries: BTreeMap<String, String>,
    pub compiled_inputs_sha256: String,
    pub source_root: PathBuf,
    pub source_sha256: String,
    pub built_source_sha256: String,
    pub relationship: String,
}

fn tool_version(tool: &OsStr, args: &[&str], source: &Path) -> io::Result<String> {
    let executable = resolve_tool(tool)?;
    let temp = owned_scratch()?;
    let output_path = temp.path().join("stdout");
    let error_path = temp.path().join("stderr");
    let mut command = CommandSpec::new(&executable);
    command.args = args.iter().map(|value| (*value).into()).collect();
    command.current_dir = Some(source.to_owned());
    command.stdout = Some(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)?,
    );
    command.stderr = Some(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&error_path)?,
    );
    let job = Job::new(Limits {
        memory_bytes: Some(256 * 1024 * 1024),
        cpu_percent: Some(25.0),
    })?;
    let child = job.spawn(&command)?;
    let outcome = job.wait(
        &child,
        Deadline::after(Duration::from_secs(15))?,
        &Cancellation::default(),
        Duration::from_secs(5),
    )?;
    drop(command);
    if outcome.reason != StopReason::Exited
        || outcome.exit_code != 0
        || fs::metadata(&output_path)?.len() > 16384
    {
        let retained = temp.keep();
        return Err(io::Error::other(format!(
            "Native build tool identity could not be established; bounded probe evidence: {}",
            retained.display()
        )));
    }
    fs::read_to_string(output_path).map(|s| s.trim().to_owned())
}

pub(crate) fn resolve_tool(tool: &OsStr) -> io::Result<PathBuf> {
    let requested = Path::new(tool);
    if requested.is_absolute() || requested.components().count() > 1 {
        // rustup dispatches by argv[0]: resolving rustc.exe/cargo.exe symlinks
        // to rustup.exe would silently select a different command.
        let resolved = std::path::absolute(requested)?;
        if resolved.is_file()
            && resolved
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            return Ok(resolved);
        }
    } else if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths).filter(|p| p.is_absolute()) {
            let mut candidate = directory.join(requested);
            if candidate.extension().is_none() {
                candidate.set_extension("exe");
            }
            if candidate.is_file()
                && candidate
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            {
                return Ok(candidate);
            }
        }
    }
    Err(io::Error::other(
        "Required native build executable is unavailable; install Rust/Cargo and the MSVC linker before explicit bootstrap.",
    ))
}

pub fn ordinary_ancestors(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(ancestor) {
            Ok(_) => build_identity::ordinary(ancestor)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Canonical spelling even when the leaf does not yet exist. On Windows,
/// comparing a verbatim canonical path against an ordinary absolute path would
/// miss ancestor/descendant overlap and permit build state inside the checkout.
fn state_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    ordinary_ancestors(&absolute)?;
    for ancestor in absolute.ancestors() {
        if ancestor.exists() {
            return Ok(ancestor
                .canonicalize()?
                .join(absolute.strip_prefix(ancestor).map_err(io::Error::other)?));
        }
    }
    Err(io::Error::other("Native state has no accessible ancestor."))
}

pub(crate) fn directory(path: &Path) -> io::Result<()> {
    ordinary_ancestors(path)?;
    fs::create_dir_all(path)?;
    build_identity::ordinary(path)?;
    if !path.is_dir() {
        return Err(io::Error::other(
            "Expected an ordinary native state directory.",
        ));
    }
    Ok(())
}

/// Start the admitted compiler tree. Held admissions use the aggregate owner;
/// nested admissions do not acquire a second aggregate limit.
pub(crate) fn start_admitted_compiler(
    admission: &heavy_command::Admission,
    job: &Job,
    command: &CommandSpec,
) -> io::Result<crate::process::OwnedProcess> {
    admission.spawn_in_envelope(job, command)?.resume()
}

fn scratch_root() -> PathBuf {
    std::env::temp_dir().join(SCRATCH_ROOT_NAME)
}

/// Ownership record for a fresh entry: the identity line plus the diagnostic
/// details an operator can use during bounded recovery. Only the identity line
/// is ever compared; the details never decide anything.
fn scratch_record() -> String {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    format!(
        "{SCRATCH_OWNER}pid={} started={started}\n",
        std::process::id()
    )
}

/// True when `path` is an ordinary file that begins with the ownership record.
fn carries_scratch_ownership(path: &Path) -> bool {
    if build_identity::ordinary(path).is_err() {
        return false;
    }
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut identity = [0u8; SCRATCH_OWNER.len()];
    file.read_exact(&mut identity).is_ok() && identity == SCRATCH_OWNER.as_bytes()
}

/// Establish the ownership record for an empty or fresh directory.
fn write_scratch_record(dir: &Path) -> io::Result<()> {
    let record = dir.join(SCRATCH_OWNER_FILE);
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&record)
    {
        Ok(mut file) => {
            file.write_all(SCRATCH_OWNER.as_bytes())?;
            file.sync_all()
        }
        // A concurrent creator may have won the race; adopt only its identity.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if carries_scratch_ownership(&record) {
                Ok(())
            } else {
                Err(io::Error::other(
                    "Scratch root ownership record is foreign; preserving it.",
                ))
            }
        }
        Err(error) => Err(error),
    }
}

/// Verify or establish the dedicated owned scratch root.
///
/// A same-named directory is adopted only when it is empty or already carries
/// this harness's ownership record. Anything else is unproven foreign data and
/// is preserved untouched instead of being swept or reused.
fn owned_scratch_root(root: &Path, create: bool) -> io::Result<Option<PathBuf>> {
    match fs::symlink_metadata(root) {
        Ok(metadata) => {
            if !metadata.is_dir() || build_identity::ordinary(root).is_err() {
                return Ok(None);
            }
            if carries_scratch_ownership(&root.join(SCRATCH_OWNER_FILE)) {
                return Ok(Some(root.to_owned()));
            }
            if fs::read_dir(root)?.next().is_some() {
                return Ok(None);
            }
            if create {
                write_scratch_record(root)?;
            }
            Ok(Some(root.to_owned()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if !create {
                return Ok(None);
            }
            match fs::create_dir(root) {
                Ok(()) => {
                    build_identity::ordinary(root)?;
                    write_scratch_record(root)?;
                    Ok(Some(root.to_owned()))
                }
                // Another creator won the race; adopt only verified data.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    owned_scratch_root(root, create)
                }
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

/// One owned scratch tree with a live lease.
///
/// The entry is a short random directory beneath the owned root, so nested
/// compiler paths stay short. Its record file carries the ownership identity
/// and its lease object carries the exclusive operating-system lock held while
/// the entry is in use: the kernel releases that lock when the owning process
/// ends, so an interrupted operation leaves proven ownership with a dead lease
/// for the next explicit operation. The field order releases the lease before
/// `TempDir` removes the tree.
struct ScratchEntry {
    lease: Option<ExclusiveFileLock>,
    dir: tempfile::TempDir,
}

impl ScratchEntry {
    /// Create one leased entry beneath an already verified owned root.
    fn leased(root: &Path) -> io::Result<Self> {
        let dir = tempfile::Builder::new().prefix("").tempdir_in(root)?;
        // Publish the identity before taking the lease: a sweep verifies the
        // identity and then takes only an existing released lease, so it can
        // never reclaim a directory whose lease is not yet held.
        let mut owner = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.path().join(SCRATCH_OWNER_FILE))?;
        owner.write_all(scratch_record().as_bytes())?;
        owner.sync_all()?;
        drop(owner);
        let lease = ExclusiveFileLock::try_acquire(&dir.path().join(SCRATCH_LEASE_FILE))?
            .ok_or_else(|| {
                io::Error::other("A fresh scratch lease is already held; retry the explicit build.")
            })?;
        Ok(Self {
            lease: Some(lease),
            dir,
        })
    }

    /// Unproven short-lived scratch when the owned root cannot be claimed.
    fn unproven() -> io::Result<Self> {
        Ok(Self {
            lease: None,
            dir: tempfile::Builder::new().prefix("hcb-").tempdir()?,
        })
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Retain the tree for diagnostics. The released lease keeps it
    /// reclaimable by the next explicit operation instead of leaking unproven
    /// data.
    fn keep(self) -> PathBuf {
        let Self { lease, dir } = self;
        drop(lease);
        dir.keep()
    }
}

/// Create one scratch tree in the dedicated owned root.
///
/// A same-named unproven root is preserved, so the build falls back to
/// unproven short-lived scratch instead of failing an otherwise valid build
/// over a name collision in the process temp directory.
fn owned_scratch() -> io::Result<ScratchEntry> {
    match owned_scratch_root(&scratch_root(), true)? {
        Some(root) => ScratchEntry::leased(&root),
        None => ScratchEntry::unproven(),
    }
}

/// Reclaim owned scratch trees abandoned by an interrupted explicit operation.
///
/// `TempDir` cleanup only runs when the manager unwinds normally, so a killed
/// install/update leaks its scratch. Reclamation requires both the ownership
/// record and a released lease: a matching name, an age or a process
/// identifier never authorizes removal, and active or foreign data survives.
/// Reparse points are skipped rather than followed, and every step stays
/// best-effort so hygiene can never fail or delay a valid build.
fn sweep_owned_scratch(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir())
            || build_identity::ordinary(&path).is_err()
            || !carries_scratch_ownership(&path.join(SCRATCH_OWNER_FILE))
        {
            continue;
        }
        // Missing or linked lease evidence is never authoritative, and a
        // missing lease must not be created: a directory whose lease is not yet
        // held belongs to an operation that is still being created.
        let lease = path.join(SCRATCH_LEASE_FILE);
        if build_identity::ordinary(&lease).is_err() {
            continue;
        }
        match ExclusiveFileLock::try_acquire_existing(&lease) {
            // The lease outlives its operation only while that process lives;
            // reclaim the entry while holding the released lease. A live or
            // unprovable lease preserves the entry untouched.
            Ok(Some(lease)) => reclaim_owned_entry(&path, lease),
            Ok(None) => {}
            Err(_) => {}
        }
    }
}

/// Remove one abandoned owned entry. The ownership record is removed last and
/// kept whenever any part of the tree resists removal, so a failed cleanup
/// leaves a still-reclaimable owned entry instead of unproven debris.
fn reclaim_owned_entry(path: &Path, lease: ExclusiveFileLock) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    let mut blocked = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name == SCRATCH_OWNER_FILE || name == SCRATCH_LEASE_FILE {
            continue;
        }
        let child = entry.path();
        let removed = if build_identity::ordinary(&child).is_err() {
            // A reparse point is skipped, never followed or unlinked.
            Err(io::Error::other("reparse point"))
        } else if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            fs::remove_dir_all(&child)
        } else {
            fs::remove_file(&child)
        };
        if removed.is_err() {
            blocked = true;
        }
    }
    if blocked {
        return;
    }
    let Ok(mut remaining) = fs::read_dir(path) else {
        return;
    };
    if remaining.any(|entry| {
        entry.is_ok_and(|entry| {
            let name = entry.file_name();
            name != SCRATCH_OWNER_FILE && name != SCRATCH_LEASE_FILE
        })
    }) {
        return;
    }
    // Release the lease before removing the lease object and record; a fresh
    // operation never adopts this directory name, so no new owner is
    // displaced here. The identity is removed last: a failed removal therefore
    // stays owned and the next sweep can finish the job.
    drop(lease);
    let _ = fs::remove_file(path.join(SCRATCH_LEASE_FILE));
    let _ = fs::remove_file(path.join(SCRATCH_OWNER_FILE));
    let _ = fs::remove_dir(path);
}

/// Evidence from the last successful build in a persistent fast target.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FastBuildRecord {
    schema: u32,
    key: String,
    state: String,
    built_epoch_ns: u128,
    source: build_identity::SourceIdentity,
}

/// A leased persistent compiler target. Dropping it releases the lease but
/// deliberately leaves the target for the next explicit fast build.
struct FastTarget {
    _lease: ExclusiveFileLock,
    path: PathBuf,
    key: String,
    state: String,
}

impl FastTarget {
    fn path(&self) -> &Path {
        &self.path
    }

    fn record_success(&self, source: &build_identity::SourceIdentity) -> io::Result<()> {
        let built_epoch_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let record = FastBuildRecord {
            schema: FAST_SCHEMA,
            key: self.key.clone(),
            state: self.state.clone(),
            built_epoch_ns,
            source: source.clone(),
        };
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(self.path.join(FAST_RECORD_FILE))?;
        file.write_all(&serde_json::to_vec_pretty(&record)?)?;
        file.sync_all()?;
        fs::write(self.path.join(FAST_USED_FILE), built_epoch_ns.to_string())
    }
}

fn fast_target_key(
    identity: &build_identity::SourceIdentity,
    rustc: &str,
    cargo: &str,
    profile: Profile,
) -> io::Result<String> {
    let files: BTreeMap<_, _> = identity
        .files
        .iter()
        .filter(|(path, _)| build_identity::build_affecting_path(path))
        .map(|(path, hash)| (path.clone(), hash.clone()))
        .collect();
    Ok(build_identity::hash_bytes(&serde_json::to_vec(
        &serde_json::json!({
            "schema": FAST_SCHEMA,
            "rustc": rustc,
            "cargo": cargo,
            "target": TARGET,
            "profile": profile.as_str(),
            "files": files,
        }),
    )?))
}

fn fast_target_path(root: &Path, key: &str) -> PathBuf {
    root.join(format!("f{}", &key[..16]))
}

fn fast_owner_record(key: &str, state: &str) -> String {
    format!("{FAST_OWNER}key={key}\nstate={state}\n")
}

fn fast_owner_identity(path: &Path) -> Option<(String, String)> {
    if build_identity::ordinary(path).is_err() {
        return None;
    }
    if fs::metadata(path).ok()?.len() > 1024 {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    if lines.next()? != FAST_OWNER.trim_end() {
        return None;
    }
    let key = lines.next()?.strip_prefix("key=")?.to_owned();
    let state = lines.next()?.strip_prefix("state=")?.to_owned();
    (lines.next().is_none()).then_some((key, state))
}

fn fast_state_id(state: &Path) -> io::Result<String> {
    Ok(build_identity::hash_bytes(
        state
            .canonicalize()?
            .to_string_lossy()
            .to_ascii_lowercase()
            .as_bytes(),
    ))
}

fn fast_target_is_empty(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| {
        entries.all(|entry| {
            entry.is_ok_and(|entry| {
                matches!(
                    entry.file_name().to_str(),
                    Some(SCRATCH_OWNER_FILE | SCRATCH_LEASE_FILE | FAST_USED_FILE)
                )
            })
        })
    })
}

fn fast_file_or_absent(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) => metadata.is_file() && build_identity::ordinary(path).is_ok(),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

/// Reuse is allowed only when every changed identity input is newer than the
/// last successful build. A restored timestamp therefore falls back to fresh
/// scratch instead of trusting Cargo's mtime comparison.
fn fast_record_allows(
    record: &FastBuildRecord,
    source: &Path,
    current: &build_identity::SourceIdentity,
) -> bool {
    if record.source.sha256 == current.sha256 {
        return true;
    }
    let mut paths = BTreeSet::new();
    paths.extend(record.source.files.keys().cloned());
    paths.extend(current.files.keys().cloned());
    for path in paths {
        if record.source.files.get(&path) == current.files.get(&path) {
            continue;
        }
        let Ok(metadata) = fs::metadata(source.join(&path)) else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        let Ok(modified) = metadata.modified() else {
            return false;
        };
        let Ok(modified) = modified.duration_since(UNIX_EPOCH) else {
            return false;
        };
        if modified.as_nanos() <= record.built_epoch_ns {
            return false;
        }
    }
    true
}

/// Acquire one keyed fast target. Any foreign, active, mismatched or unproven
/// state returns `None` without mutation so the caller uses fresh scratch.
fn fast_target_in(
    root: &Path,
    source: &Path,
    state: &Path,
    identity: &build_identity::SourceIdentity,
    rustc: &str,
    cargo: &str,
    profile: Profile,
) -> io::Result<Option<FastTarget>> {
    if profile != Profile::DeliveryFast {
        return Ok(None);
    }
    let key = fast_target_key(identity, rustc, cargo, profile)?;
    let state = fast_state_id(state)?;
    let path = fast_target_path(root, &key);
    let created = match fs::create_dir(&path) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(error),
    };
    if created {
        fs::write(
            path.join(SCRATCH_OWNER_FILE),
            fast_owner_record(&key, &state),
        )?;
        let target = FastTarget {
            _lease: ExclusiveFileLock::try_acquire(&path.join(SCRATCH_LEASE_FILE))?
                .ok_or_else(|| io::Error::other("Fresh fast target lease is already held."))?,
            path,
            key,
            state,
        };
        fs::write(target.path.join(FAST_USED_FILE), "created")?;
        return Ok(Some(target));
    }
    let lease = path.join(SCRATCH_LEASE_FILE);
    if !path.is_dir()
        || build_identity::ordinary(&path).is_err()
        || fast_owner_identity(&path.join(SCRATCH_OWNER_FILE)) != Some((key.clone(), state.clone()))
        || !fast_file_or_absent(&lease)
    {
        return Ok(None);
    }
    let _lease = match ExclusiveFileLock::try_acquire_existing(&lease) {
        Ok(Some(lease)) => lease,
        _ => return Ok(None),
    };
    let record_path = path.join(FAST_RECORD_FILE);
    let used = path.join(FAST_USED_FILE);
    if !fast_file_or_absent(&record_path) || !fast_file_or_absent(&used) {
        return Ok(None);
    }
    let record = match fs::read(&record_path) {
        Ok(bytes) => serde_json::from_slice::<FastBuildRecord>(&bytes).ok(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let accepted = match record {
        Some(record)
            if record.schema == FAST_SCHEMA && record.key == key && record.state == state =>
        {
            fast_record_allows(&record, source, identity)
        }
        None => fast_target_is_empty(&path),
        _ => false,
    };
    if !accepted {
        return Ok(None);
    }
    fs::write(
        used,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs()
            .to_string(),
    )?;
    Ok(Some(FastTarget {
        _lease,
        path,
        key,
        state,
    }))
}

fn fast_target(
    source: &Path,
    state: &Path,
    identity: &build_identity::SourceIdentity,
    rustc: &str,
    cargo: &str,
    profile: Profile,
) -> io::Result<Option<FastTarget>> {
    if profile != Profile::DeliveryFast {
        return Ok(None);
    }
    let root = owned_scratch_root(&scratch_root(), true)?.ok_or_else(|| {
        io::Error::other(
            "Fast compiler-cache root is foreign or ambiguous; preserving it and refusing to inherit its target.",
        )
    })?;
    retire_inactive_fast_targets(&root, &fast_state_id(state)?, SystemTime::now());
    fast_target_in(&root, source, state, identity, rustc, cargo, profile)
}

/// Retire only released, owned fast targets that have been quiet for the full
/// inactivity period for this installation. Active, malformed, linked and
/// foreign entries survive.
fn retire_inactive_fast_targets(root: &Path, state: &str, now: SystemTime) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir())
            || build_identity::ordinary(&path).is_err()
        {
            continue;
        }
        let Some((_, owner_state)) = fast_owner_identity(&path.join(SCRATCH_OWNER_FILE)) else {
            continue;
        };
        if owner_state != state {
            continue;
        }
        let used = path.join(FAST_USED_FILE);
        if build_identity::ordinary(&used).is_err() {
            continue;
        }
        let Ok(modified) = fs::metadata(&used).and_then(|metadata| metadata.modified()) else {
            continue;
        };
        if now
            .duration_since(modified)
            .is_ok_and(|quiet| quiet >= FAST_INACTIVITY)
            && let Ok(Some(lease)) =
                ExclusiveFileLock::try_acquire_existing(&path.join(SCRATCH_LEASE_FILE))
        {
            reclaim_owned_entry(&path, lease);
        }
    }
}

pub(crate) fn owner_root(state: &Path) -> io::Result<()> {
    ordinary_ancestors(state)?;
    let marker = state.join("owner");
    if state.exists() {
        build_identity::ordinary(state)?;
        if !state.is_dir() {
            return Err(io::Error::other(
                "Native state target is not a directory; preserving it.",
            ));
        }
        if marker.exists() {
            build_identity::ordinary(&marker)?;
            if !owner_matches(&marker)? {
                return Err(io::Error::other(
                    "Native state has foreign ownership; preserving it.",
                ));
            }
            return Ok(());
        }
        if fs::read_dir(state)?.next().is_some() {
            return Err(io::Error::other(
                "Native state has no ownership record and is nonempty; preserving it.",
            ));
        }
    } else {
        directory(state)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)?;
    file.write_all(OWNER)?;
    file.sync_all()
}

/// Publication order of one `builds/<identity>` directory name. Names are
/// `<source>-<nanos>-<pid>`; a foreign or malformed name is skipped instead of
/// being delivered.
fn published_sequence(build: &Path) -> Option<(u128, u32)> {
    let name = build.file_name()?.to_str()?;
    let mut parts = name.split('-');
    let source = parts.next()?;
    let sequence = parts.next()?.parse::<u128>().ok()?;
    let pid = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some()
        || source.len() != 16
        || !source.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some((sequence, pid))
}

fn candidate_builds(state: &Path, profile: Profile) -> io::Result<Vec<((u128, u32), PathBuf)>> {
    owner_root(state)?;
    let builds = state.join("builds");
    let missing = || {
        io::Error::other(
            "No integrity-verified native build is available in the owned state; run an explicit build first.",
        )
    };
    if !builds.exists() {
        return Err(missing());
    }
    directory(&builds)?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(&builds)? {
        let path = entry?.path();
        let Some(sequence) = published_sequence(&path) else {
            continue;
        };
        let Ok(record) = build_identity::verify_record_integrity(&path) else {
            continue;
        };
        if build_identity::record_profile(&record).ok() != Some(profile)
            || !BINARIES
                .iter()
                .all(|name| record.binaries.contains_key(*name))
        {
            continue;
        }
        candidates.push((sequence, path));
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
    Ok(candidates)
}

/// The freshest published build whose full record and every recorded binary
/// still verify. An unverified, altered or foreign directory is never chosen.
pub fn freshest_verified(state: &Path) -> io::Result<PathBuf> {
    freshest_verified_for(state, Profile::Release)
}

fn freshest_verified_for(state: &Path, profile: Profile) -> io::Result<PathBuf> {
    candidate_builds(state, profile)?
        .into_iter()
        .next()
        .map(|(_, path)| path)
        .ok_or_else(|| {
            io::Error::other(
                "No integrity-verified native build is available in the owned state; run an explicit build first.",
            )
        })
}

/// Select within the existing integrity-verified publication history. Applying
/// delivery requires current source; a planning preview may use an older build
/// from the same checkout and reports its source relationship separately.
fn freshest_for_source(
    state: &Path,
    source: &Path,
    require_current_source: bool,
    profile: Profile,
) -> io::Result<Option<PathBuf>> {
    let source = source.canonicalize()?;
    Ok(candidate_builds(state, profile)?
        .into_iter()
        .map(|(_, path)| path)
        .find(|path| {
            build_identity::read_record(path)
                .ok()
                .and_then(|record| record.source_root.canonicalize().ok())
                .is_some_and(|root| root == source)
                && (!require_current_source
                    || build_identity::check_profile(path, Some(&source), profile).status
                        == build_identity::Health::Healthy)
        }))
}

/// A planning-only selection from an already owned state, without preparing
/// or activating anything. The caller retains normal preparation when no
/// eligible build is available.
pub fn preview_build(state: &Path, source: &Path) -> io::Result<Option<PathBuf>> {
    preview_build_for(state, source, Profile::Release)
}

pub fn preview_build_for(
    state: &Path,
    source: &Path,
    profile: Profile,
) -> io::Result<Option<PathBuf>> {
    verify_owned_state(state)?;
    freshest_for_source(state, source, false, profile)
}

/// The build delivered by an Install/Update that names none: the freshest
/// verified build of the selected source in the owned state this manager
/// itself runs from. A manager outside such a state keeps requiring an
/// explicit build instead of guessing, and a stale or foreign-source build is
/// never delivered silently.
pub fn delivery_build(source: &Path) -> io::Result<PathBuf> {
    delivery_from(&std::env::current_exe()?, source)
}

/// Spelling matters: every lifecycle check rejects verbatim (`\\?\`) paths, so
/// the delivered build is returned in its ordinary local form.
fn delivery_from(executable: &Path, source: &Path) -> io::Result<PathBuf> {
    let executable = crate::dependency_package::plain(&executable.canonicalize()?);
    let build = executable
        .parent()
        .ok_or_else(|| io::Error::other("Native manager has no build directory."))?;
    let outside = || {
        io::Error::other(
            "This manager does not run from an owned native state; pass --build DIRECTORY.",
        )
    };
    let builds = build.parent().ok_or_else(outside)?;
    if builds.file_name() != Some(OsStr::new("builds")) {
        return Err(outside());
    }
    let state = builds.parent().ok_or_else(outside)?;
    let profile = build_identity::record_profile(&build_identity::read_record(build)?)?;
    freshest_for_source(state, source, true, profile)?
        .ok_or_else(|| {
            io::Error::other(
                "No integrity-verified native build matches the selected source; run an explicit build first.",
            )
        })
}

/// Release candidates require the full checkout identity. Fast candidates
/// additionally require complete compiled-input evidence bound to the same
/// immutable record.
fn reusable_candidates(
    state: &Path,
    source: &Path,
    rustc: &str,
    cargo: &str,
    expected: &build_identity::SourceIdentity,
    profile: Profile,
) -> io::Result<Vec<(PathBuf, build_identity::BuildRecord)>> {
    let builds = state.join("builds");
    if !builds.exists() {
        return Ok(Vec::new());
    }
    directory(&builds)?;
    let mut candidates = Vec::new();
    for entry in fs::read_dir(builds)? {
        let path = entry?.path();
        let Ok(record) = build_identity::read_record(&path) else {
            continue;
        };
        if record.source_root != source
            || record.rustc != rustc
            || record.cargo != cargo
            || record.target != TARGET
            || build_identity::record_profile(&record).ok() != Some(profile)
        {
            continue;
        }
        if profile == Profile::Release {
            if record.source != *expected {
                continue;
            }
            candidates.push((path, record));
            continue;
        }
        let Ok(evidence) = build_identity::read_compiled_inputs(&path) else {
            continue;
        };
        if evidence.validate_against(&path, &record).is_err() || !evidence.matches(expected) {
            continue;
        }
        candidates.push((path, record));
    }
    Ok(candidates)
}

fn find_reusable(
    state: &Path,
    source: &Path,
    rustc: &str,
    cargo: &str,
    expected: &build_identity::SourceIdentity,
    profile: Profile,
) -> io::Result<Option<PreparedBuild>> {
    find_reusable_with(state, source, rustc, cargo, expected, profile, |path| {
        consumer_check(path, Some(source)).map(|check| reusable_status(profile, &check.status))
    })
}

fn reusable_status(profile: Profile, status: &build_identity::Health) -> bool {
    *status == build_identity::Health::Healthy
        || (profile == Profile::DeliveryFast && *status == build_identity::Health::SourceStale)
}

fn find_reusable_with(
    state: &Path,
    source: &Path,
    rustc: &str,
    cargo: &str,
    expected: &build_identity::SourceIdentity,
    profile: Profile,
    mut verify: impl FnMut(&Path) -> io::Result<bool>,
) -> io::Result<Option<PreparedBuild>> {
    for (path, record) in reusable_candidates(state, source, rustc, cargo, expected, profile)? {
        if !verify(&path).is_ok_and(|verified| verified) {
            continue;
        }
        if profile == Profile::DeliveryFast {
            let current = build_identity::source_identity(source)?;
            if current != *expected {
                continue;
            }
            let Ok(record) = build_identity::verify_record_integrity(&path) else {
                continue;
            };
            let Ok(evidence) = build_identity::read_compiled_inputs(&path) else {
                continue;
            };
            if evidence.validate_against(&path, &record).is_err() || !evidence.matches(&current) {
                continue;
            }
            record_reuse_selection(state, source, &path, &record, &current)?;
            return Ok(Some(PreparedBuild {
                build: path,
                reused: true,
                source_identity: record.source.sha256,
            }));
        }
        return Ok(Some(PreparedBuild {
            build: path,
            reused: true,
            source_identity: record.source.sha256,
        }));
    }
    Ok(None)
}

fn record_reuse_selection(
    state: &Path,
    source: &Path,
    build: &Path,
    record: &build_identity::BuildRecord,
    current: &build_identity::SourceIdentity,
) -> io::Result<PathBuf> {
    let selection = CompiledInputSelection {
        schema: 1,
        build: build
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("Non-Unicode build directory"))?
            .into(),
        profile: build_identity::record_profile(record)?.as_str().into(),
        build_record_sha256: build_identity::hash_file(&build.join("build.json"))?,
        binaries: record.binaries.clone(),
        compiled_inputs_sha256: build_identity::hash_file(
            &build.join(build_identity::COMPILED_INPUTS_FILE),
        )?,
        source_root: source.canonicalize()?,
        source_sha256: current.sha256.clone(),
        built_source_sha256: record.source.sha256.clone(),
        relationship: if current.sha256 == record.source.sha256 {
            CURRENT_CHECKOUT_RELATIONSHIP
        } else {
            COMPILED_INPUT_RELATIONSHIP
        }
        .into(),
    };
    let bytes = serde_json::to_vec_pretty(&selection)?;
    let selections = state.join(COMPILED_INPUT_SELECTIONS);
    directory(&selections)?;
    let destination = selections.join(format!("{}.json", build_identity::hash_bytes(&bytes)));
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            build_identity::ordinary(&destination)?;
            if fs::read(&destination)? != bytes {
                return Err(io::Error::other(
                    "Compiled-input selection record conflict; preserving it.",
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos();
            let temporary = selections.join(format!(".{}-{stamp}.tmp", std::process::id()));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            if let Err(error) = fs::rename(&temporary, &destination) {
                let _ = fs::remove_file(&temporary);
                return Err(error);
            }
        }
        Err(error) => return Err(error),
    }
    Ok(destination)
}

/// Run an integrity-verified candidate's model-free Check during explicit
/// management. Ordinary launch and public Check never call this function.
pub fn consumer_check(
    build: &Path,
    source: Option<&Path>,
) -> io::Result<build_identity::BuildCheck> {
    let before = build_identity::verify_record_integrity(build)?;
    let receipt_hash = build_identity::hash_file(&build.join("build.json"))?;
    let evidence = owned_scratch()?;
    let mut command = CommandSpec::new(build.join("codex-harness.exe"));
    command.args = vec![
        "check".into(),
        "--build".into(),
        build.as_os_str().to_owned(),
    ];
    if let Some(source) = source {
        command
            .args
            .extend(["--source".into(), source.as_os_str().to_owned()]);
    }
    let output = evidence.path().join("stdout.json");
    let result = handoff::invoke(
        command,
        &evidence.path().join("stderr.log"),
        Some(&output),
        Duration::from_secs(10),
    );
    let verified = (|| {
        let result = result?;
        let report: build_identity::BuildCheck =
            serde_json::from_slice(&handoff::bounded_bytes(&output)?)?;
        if result.reason != StopReason::Exited
            || result.exit_code != u32::from(report.status != build_identity::Health::Healthy)
            || report.runtime_allowed
                != matches!(
                    report.status,
                    build_identity::Health::Healthy
                        | build_identity::Health::SourceStale
                        | build_identity::Health::SourceUnavailable
                )
            || (report.runtime_allowed && !report.management_allowed)
            || receipt_hash != build_identity::hash_file(&build.join("build.json"))?
            || build_identity::verify_record_integrity(build)?.binaries != before.binaries
        {
            return Err(io::Error::other(
                "Candidate Check response or integrity is inconsistent.",
            ));
        }
        Ok(report)
    })();
    verified.map_err(|error| {
        let path = evidence.keep();
        io::Error::other(format!(
            "Actual candidate Check failed: {error}; see {}",
            path.display()
        ))
    })
}

/// Hand selection to the actual verified consumer before it acquires the lock.
/// A different release's compiled validator cannot authorize this candidate.
pub fn activate_candidate(
    state: &Path,
    build: &Path,
) -> io::Result<crate::build_selection::Selection> {
    verify_owned_state(state)?;
    ordinary_ancestors(build)?;
    let build = build.canonicalize()?;
    let state = state.canonicalize()?;
    if build.parent() != Some(state.join("builds").canonicalize()?.as_path()) {
        return Err(io::Error::other(
            "Candidate must be published in the selected owned state.",
        ));
    }
    let manager = build.join("codex-harness.exe");
    // The manager comparison needs a usable image; the self path verifies
    // through build_selection::activate, and the consumer path verifies the
    // candidate exactly once before it runs (no separate throwaway pass).
    if manager.canonicalize()? == std::env::current_exe()?.canonicalize()? {
        return crate::build_selection::activate(&state, &build);
    }
    let check = consumer_check(&build, None)?;
    if !check.runtime_allowed {
        return Err(io::Error::other(check.action));
    }
    let evidence = owned_scratch()?;
    let mut command = CommandSpec::new(manager);
    command.args = vec![
        "activate-build".into(),
        "--state".into(),
        state.as_os_str().to_owned(),
        "--build".into(),
        build.as_os_str().to_owned(),
    ];
    let output = evidence.path().join("stdout.json");
    let result = handoff::invoke(
        command,
        &evidence.path().join("stderr.log"),
        Some(&output),
        Duration::from_secs(15),
    );
    let verified = (|| {
        let result = result?;
        if result.reason != StopReason::Exited || result.exit_code != 0 {
            return Err(io::Error::other(
                "Candidate selection did not complete; inspect retained state before recovery.",
            ));
        }
        let report: crate::build_selection::Selection =
            serde_json::from_slice(&handoff::bounded_bytes(&output)?)?;
        let pointer: serde_json::Value =
            serde_json::from_slice(&handoff::bounded_bytes(&state.join("active-build.json"))?)?;
        if report.build.as_ref() != Some(&build)
            || pointer["schema"] != 1
            || pointer["build"].as_str() != build.file_name().and_then(|s| s.to_str())
            || pointer["record_sha256"] != build_identity::hash_file(&build.join("build.json"))?
        {
            return Err(io::Error::other(
                "Candidate selection response does not match its actual pointer.",
            ));
        }
        Ok(report)
    })();
    verified.map_err(|error| {
        let path = evidence.keep();
        io::Error::other(format!(
            "Native selection handoff failed: {error}; see {}",
            path.display()
        ))
    })
}

/// Base publication Cargo command. Both phases share source, target and
/// limits; only the binary selection differs.
fn publication_cargo(
    source: &Path,
    target: &Path,
    cargo: &OsStr,
    profile: Profile,
) -> io::Result<CommandSpec> {
    let mut command = CommandSpec::new(resolve_tool(cargo)?);
    // Retained matched fast-profile evidence: four jobs, while release keeps
    // its fresh-target single-job publication limit.
    let jobs = if profile == Profile::DeliveryFast {
        "4"
    } else {
        "1"
    };
    command.args = [
        "build",
        "--profile",
        profile.as_str(),
        "--locked",
        // Cargo otherwise scales rustc concurrency to host CPU count, which
        // does not reflect the fixed memory budget of the compiler job.
        "--jobs",
        jobs,
        "--target",
        TARGET,
        "-p",
        "codex-harness",
        "-p",
        "harness-rtk",
        "-p",
        "token-audit",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    command.args.extend([
        "--manifest-path".into(),
        source.join("Cargo.toml").into_os_string(),
        "--target-dir".into(),
        target.as_os_str().to_owned(),
    ]);
    command.current_dir = Some(source.to_owned());
    Ok(command)
}

/// Cargo selection for the completion phase: exactly the source-owned delivery
/// set, or the pre-query whole-package selection when the compiled manager
/// predates the delivery query.
fn publication_bin_selection(delivery: Option<&[String]>) -> Vec<OsString> {
    match delivery {
        Some(names) => names
            .iter()
            .flat_map(|name| {
                [
                    OsString::from("--bin"),
                    OsString::from(name.strip_suffix(".exe").unwrap_or(name)),
                ]
            })
            .collect(),
        None => vec![OsString::from("--bins")],
    }
}

/// Ask the freshly compiled manager for the delivery programs its own source
/// declares. `None` means the compiled manager predates the query.
fn probe_delivery(
    target: &Path,
    staging: &Path,
    source: &Path,
    profile: Profile,
) -> io::Result<Option<Vec<String>>> {
    let request = staging.join("delivery-probe-request.json");
    ordinary_ancestors(&request)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&request)?;
    file.write_all(&serde_json::to_vec(&serde_json::json!({
        "schema": 1,
        "kind": DELIVERY_PROBE_KIND,
    }))?)?;
    file.sync_all()?;
    drop(file);
    let mut command = CommandSpec::new(
        target
            .join(TARGET)
            .join(profile.output_dir())
            .join("codex-harness.exe"),
    );
    command.args = vec!["finalize-build-v1".into(), request.as_os_str().to_owned()];
    command.current_dir = Some(source.to_owned());
    let log = staging.join("delivery-probe.log");
    let response = staging.join("delivery-probe.json");
    let outcome = handoff::invoke(command, &log, Some(&response), Duration::from_secs(60))?;
    classify_delivery_probe(outcome.reason, outcome.exit_code, &log, &response)
}

/// Classify one delivery-query outcome. An ordinary nonzero exit is a manager
/// that predates the query and keeps the compatibility fallback with its log
/// retained; a timeout, kill, or unusable successful answer is a real failure
/// and must not be mistaken for an unsupported protocol.
fn classify_delivery_probe(
    reason: StopReason,
    exit_code: u32,
    log: &Path,
    response: &Path,
) -> io::Result<Option<Vec<String>>> {
    match (reason, exit_code) {
        (StopReason::Exited, 0) => {
            let bytes = match handoff::bounded_bytes(response) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Err(io::Error::other(format!(
                        "Fresh manager delivery query produced no readable response ({error}); see {} and {}",
                        log.display(),
                        response.display()
                    )));
                }
            };
            match serde_json::from_slice::<DeliveryProbeResponse>(&bytes) {
                Ok(parsed) if parsed.schema == 1 && !parsed.delivery.is_empty() => {
                    Ok(Some(parsed.delivery))
                }
                Ok(parsed) => Err(io::Error::other(format!(
                    "Fresh manager delivery query answered an unusable set (schema {}, {} entries); see {} and {}",
                    parsed.schema,
                    parsed.delivery.len(),
                    log.display(),
                    response.display()
                ))),
                Err(error) => Err(io::Error::other(format!(
                    "Fresh manager delivery query answered an unreadable response ({error}); see {} and {}",
                    log.display(),
                    response.display()
                ))),
            }
        }
        // A manager built before the query rejects the request with an ordinary
        // nonzero exit; its log stays next to the retained staging directory.
        (StopReason::Exited, _) => {
            eprintln!(
                "native build: the freshly compiled manager rejected the delivery query (exit {exit_code}); completing with the pre-query whole-package selection. See {}",
                log.display()
            );
            Ok(None)
        }
        (reason, exit_code) => Err(io::Error::other(format!(
            "Fresh manager delivery query did not complete ({reason:?}, exit {exit_code}); an incomplete query does not establish a pre-query manager. See {} and {}",
            log.display(),
            response.display()
        ))),
    }
}

/// Run one bounded Cargo phase in the admitted compiler tree. Phases reuse the
/// same fresh target, so the completion phase only adds what the bootstrap
/// phase still lacks.
fn run_publication_phase(
    admission: &heavy_command::Admission,
    budget: &heavy_command::Budget,
    account: &Path,
    deadline: Deadline,
    log: &fs::File,
    log_path: &Path,
    command: &mut CommandSpec,
) -> io::Result<()> {
    // One owned lifecycle Job per phase: the Job is consumed by its wait, no
    // compiler tree survives between phases, and its name is released for the
    // completion phase. The admitted aggregate and CPU policy stay unchanged.
    let (job, marker) = admission.owned_job(budget, account)?;
    eprintln!("{}", admission.job_line(&job)?);
    command.stdout = Some(log.try_clone()?);
    command.stderr = Some(log.try_clone()?);
    command
        .env
        .insert(heavy_command::LEASE_ENV.into(), Some(marker));
    let child = start_admitted_compiler(admission, &job, command)
        .map_err(|e| io::Error::other(format!("Starting bounded Cargo failed: {e}")))?;
    let status = job.wait(
        &child,
        // A cold release build includes optimized manager variants and native
        // dependencies. Keep it finite without applying an indexing deadline
        // to compilation; the compiler Job still bounds memory and CPU.
        deadline,
        &Cancellation::default(),
        Duration::from_secs(5),
    )?;
    command.stdout = None;
    command.stderr = None;
    if status.reason != StopReason::Exited || status.exit_code != 0 {
        return Err(io::Error::other(format!(
            "Candidate build failed (exit {}, {:?}); active installation preserved. See {}",
            status.exit_code,
            status.reason,
            log_path.display()
        )));
    }
    Ok(())
}

/// Build into installation-owned state, then publish an immutable candidate.
/// No Cargo command is reachable from `build_identity::check`.
pub fn prepare(source: &Path, state: &Path, cargo: &OsStr) -> io::Result<PreparedBuild> {
    prepare_for(source, state, cargo, Profile::Release)
}

/// Prepare an immutable candidate from the explicitly selected compiler profile.
pub fn prepare_for(
    source: &Path,
    state: &Path,
    cargo: &OsStr,
    profile: Profile,
) -> io::Result<PreparedBuild> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err(io::Error::other(
            "Native bootstrap currently supports Windows x64 with MSVC.",
        ));
    }
    let source = source.canonicalize()?;
    let state = state_path(state)?;
    // Runtime data must not be mixed into the checkout or encompass its source.
    if state.starts_with(&source) || source.starts_with(&state) {
        return Err(io::Error::other(
            "Native build state must be outside the source checkout.",
        ));
    }
    let before = build_identity::source_identity(&source)?;
    let rustc = tool_version(OsStr::new("rustc"), &["-vV"], &source)?;
    if !rustc.lines().any(|line| line == format!("host: {TARGET}")) {
        return Err(io::Error::other(
            "Native bootstrap requires the x86_64-pc-windows-msvc Rust toolchain.",
        ));
    }
    let cargo_version = tool_version(cargo, &["--version"], &source)?;
    // Custom compiler wrappers and flags must get an explicit identity contract
    // before they can participate in an installation build.
    for (key, value) in std::env::vars_os() {
        let key = key.to_string_lossy().to_ascii_uppercase();
        let override_input = key.starts_with("RUSTC")
            || key == "RUSTFLAGS"
            || key.starts_with("CARGO_BUILD_")
            || key.starts_with("CARGO_TARGET_")
            || key.starts_with("CARGO_PROFILE_")
            || key.starts_with("CARGO_ALIAS_")
            || matches!(
                key.as_str(),
                "CARGO_ENCODED_RUSTFLAGS" | "CARGO_INCREMENTAL"
            );
        if override_input && !value.is_empty() {
            return Err(io::Error::other(format!(
                "Unsupported native bootstrap override {key}; use the declared toolchain without ambient overrides."
            )));
        }
    }
    // One global order for every native caller that both queues heavy work and
    // mutates build state: account admission first, then the state lock. A
    // nested native caller inside an admitted tree is a verified member of the
    // admitted Job, so this order cannot deadlock with the shared queue.
    // Compilation consumes the same machine-local budget as every other heavy
    // command, and applies it only when this process owns the lease: a nested
    // Windows Job's CPU rate is a proportion of its parent's rate, so an
    // inherited build adds containment without a second cap.
    let account = heavy_command::account_dir(None)?;
    let budget = heavy_command::Budget::read(&account)?;
    let admission = heavy_command::Admission::acquire(
        &account,
        &budget,
        &format!("native build {}", source.display()),
        &Cancellation::default(),
    )?;
    let deadline = budget.deadline()?;
    owner_root(&state)?;
    let lock = lock_owned_state(&state)?;
    // Best-effort hygiene: a foreign or unavailable root is preserved, never
    // swept, and never fails the build.
    if let Ok(Some(root)) = owned_scratch_root(&scratch_root(), false) {
        sweep_owned_scratch(&root);
    }
    if let Some(reused) = find_reusable(&state, &source, &rustc, &cargo_version, &before, profile)?
    {
        return Ok(reused);
    }
    let sequence = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let id = format!("{}-{sequence}-{}", &before.sha256[..16], std::process::id());
    let staging_root = state.join("staging");
    directory(&staging_root)?;
    let staging = staging_root.join(&id);
    fs::create_dir(&staging)?;
    let log_path = staging.join("cargo.log");
    let log = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&log_path)?;
    // Never let Cargo's mtime cache certify a different content identity, and
    // never write into pre-existing cache descendants (which may be links).
    // Unchanged builds reuse their verified immutable artifacts above. Only
    // explicit fast delivery may adopt a persistent target; publication always
    // receives a fresh scratch tree.
    let fast = fast_target(&source, &state, &before, &rustc, &cargo_version, profile)?;
    let scratch = fast.is_none().then(owned_scratch).transpose()?;
    let target = match &fast {
        Some(fast) => fast.path().to_owned(),
        None => scratch
            .as_ref()
            .ok_or_else(|| io::Error::other("Native build target was not prepared."))?
            .path()
            .to_owned(),
    };
    let declaration = declared_delivery(&source.join("Cargo.toml"));
    write_delivery_selection(&staging, declaration.as_deref())?;
    ordinary_ancestors(&target)?;
    // Release LTO of every manager binary exceeds a 4 GiB Job. The compiler Job
    // is the lifecycle job inside the admitted envelope. A held admission places
    // the compiler in the shared CPU job when present, then the account
    // aggregate, then this lifecycle job. A nested build stays on the inherited
    // path and does not acquire a second slot or aggregate limit. The compilation
    // tree inherits the same marker, so a nested native consumer reuses this
    // admission instead of queueing on it.
    // A source-owned declaration selects the complete delivery set before the
    // manager exists. Legacy or unusable declarations retain the two-phase
    // manager query and its whole-package compatibility fallback.
    if let Some(delivery) = declaration.as_deref() {
        let mut command = declared_delivery_cargo(&source, &target, cargo, profile, delivery)?;
        run_publication_phase(
            &admission,
            &budget,
            &account,
            deadline,
            &log,
            &log_path,
            &mut command,
        )?;
    } else {
        let mut command = publication_cargo(&source, &target, cargo, profile)?;
        command.args.push("--bin".into());
        command.args.push("codex-harness".into());
        run_publication_phase(
            &admission,
            &budget,
            &account,
            deadline,
            &log,
            &log_path,
            &mut command,
        )?;
        let delivery = probe_delivery(&target, &staging, &source, profile)?;
        let mut command = publication_cargo(&source, &target, cargo, profile)?;
        command
            .args
            .extend(publication_bin_selection(delivery.as_deref()));
        run_publication_phase(
            &admission,
            &budget,
            &account,
            deadline,
            &log,
            &log_path,
            &mut command,
        )?;
    }
    // Both phases are done; closing the shared log before finalization keeps
    // the staging directory renamable on Windows.
    drop(log);
    if build_identity::source_identity(&source)? != before {
        return Err(io::Error::other(
            "Native sources changed during the build; candidate not accepted. Retry explicit build with stable inputs.",
        ));
    }
    let compiled_manager = target
        .join(TARGET)
        .join(profile.output_dir())
        .join("codex-harness.exe");
    ordinary_ancestors(&compiled_manager)?;
    let record = handoff::run(handoff::Request {
        schema: 1,
        source: source.clone(),
        target: target.clone(),
        staging: staging.clone(),
        before: before.clone(),
        rustc,
        cargo: cargo_version,
        build_target: TARGET.into(),
        profile: profile.as_str().into(),
        manager_sha256: build_identity::hash_file(&compiled_manager)?,
    })?;
    if build_identity::source_identity(&source)? != before {
        return Err(io::Error::other(
            "Native sources changed during finalization; active installation preserved.",
        ));
    }
    let builds = state.join("builds");
    directory(&builds)?;
    let published = builds.join(id);
    fs::rename(&staging, &published).map_err(|e| {
        io::Error::other(format!(
            "Publishing native candidate failed; retained {}: {e}",
            staging.display()
        ))
    })?;
    if let Some(fast) = &fast
        && let Err(error) = fast.record_success(&before)
    {
        eprintln!(
            "native build: fast target evidence could not be recorded ({error}); the immutable candidate is valid but the target will not be reused without fresh evidence."
        );
    }
    drop(lock);
    Ok(PreparedBuild {
        build: published,
        reused: false,
        source_identity: record.source.sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_state_is_preserved_and_owned_lock_serializes() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("foreign"), "keep").unwrap();
        assert!(owner_root(temp.path()).is_err());
        assert_eq!(
            fs::read_to_string(temp.path().join("foreign")).unwrap(),
            "keep"
        );
        let owned = temp.path().join("owned");
        owner_root(&owned).unwrap();
        owner_root(&owned).unwrap();
        let path = owned.join("build.lock");
        let a = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let b = OpenOptions::new().write(true).open(&path).unwrap();
        a.try_lock().unwrap();
        assert!(b.try_lock().is_err());
        drop(a);
        b.try_lock().unwrap();
    }

    fn source_fixture(root: &Path, name: &str) -> PathBuf {
        let source = root.join(name);
        for directory in ["crates/one/src", "crates/harness-rtk/src"] {
            fs::create_dir_all(source.join(directory)).unwrap();
        }
        for file in [
            "Cargo.toml",
            "Cargo.lock",
            "crates/one/src/lib.rs",
            "crates/harness-rtk/src/lib.rs",
        ] {
            fs::write(source.join(file), "fixture").unwrap();
        }
        source
    }

    fn published_from(state: &Path, name: &str, source: &Path, binaries: &[&str]) -> PathBuf {
        published_from_profile(state, name, source, binaries, Profile::Release)
    }

    fn published_from_profile(
        state: &Path,
        name: &str,
        source: &Path,
        binaries: &[&str],
        profile: Profile,
    ) -> PathBuf {
        let build = state.join("builds").join(name);
        fs::create_dir_all(&build).unwrap();
        let mut hashes = std::collections::BTreeMap::new();
        for binary in binaries {
            fs::write(build.join(binary), binary).unwrap();
            hashes.insert(
                (*binary).to_owned(),
                build_identity::hash_file(&build.join(binary)).unwrap(),
            );
        }
        let record = build_identity::BuildRecord {
            schema: build_identity::SCHEMA,
            source_root: source.to_path_buf(),
            source: build_identity::source_identity(source).unwrap(),
            rustc: "fixture".into(),
            cargo: "fixture".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: profile.as_str().into(),
            binaries: hashes,
        };
        fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        build
    }

    fn published(state: &Path, name: &str, binaries: &[&str]) -> PathBuf {
        let source = source_fixture(state.parent().unwrap(), "source");
        published_from(state, name, &source, binaries)
    }

    fn dep_token(path: &Path) -> String {
        let mut token = String::new();
        for character in path.to_string_lossy().chars() {
            if character == '$' {
                token.push_str("$$");
            } else {
                if character.is_whitespace() || character == '#' {
                    token.push('\\');
                }
                token.push(character);
            }
        }
        token
    }

    fn write_dep_files(
        target: &Path,
        source: &Path,
        profile: Profile,
        overrides: &[(&str, PathBuf)],
    ) {
        let directory = target.join(TARGET).join(profile.output_dir());
        fs::create_dir_all(&directory).unwrap();
        for name in BINARIES {
            let input = overrides
                .iter()
                .find(|(binary, _)| *binary == *name)
                .map(|(_, path)| path.clone())
                .unwrap_or_else(|| source.join("crates/one/src/lib.rs"));
            let binary = directory.join(name);
            fs::write(
                binary.with_extension("d"),
                format!("{}: {}\n", dep_token(&binary), dep_token(&input)),
            )
            .unwrap();
        }
    }

    fn build_with_compiled_evidence(
        state: &Path,
        name: &str,
        source: &Path,
        target: &Path,
        profile: Profile,
        overrides: &[(&str, PathBuf)],
    ) -> (PathBuf, build_identity::CompiledInputs) {
        write_dep_files(target, source, profile, overrides);
        let build = published_from_profile(state, name, source, BINARIES, profile);
        let record_bytes = fs::read(build.join("build.json")).unwrap();
        let mut evidence = verify_compiled_inputs(
            target,
            source,
            &build_identity::source_identity(source).unwrap(),
            profile,
        )
        .unwrap();
        evidence.bind_record(&record_bytes).unwrap();
        fs::write(
            build.join(build_identity::COMPILED_INPUTS_FILE),
            serde_json::to_vec(&evidence).unwrap(),
        )
        .unwrap();
        (build, evidence)
    }

    #[test]
    fn delivery_uses_the_freshest_verified_published_build() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        owner_root(&state).unwrap();
        let older = published(&state, "aaaa0000aaaa0000-1500-1", BINARIES);
        let newer = published(&state, "bbbb0000bbbb0000-2500-2", BINARIES);
        assert_eq!(freshest_verified(&state).unwrap(), newer);
        // A newer directory that cannot verify is skipped, never delivered.
        let altered = published(&state, "cccc0000cccc0000-3500-3", BINARIES);
        fs::write(altered.join("codex-harness.exe"), "altered").unwrap();
        assert_eq!(freshest_verified(&state).unwrap(), newer);
        // A record missing this consumer's binaries is not a delivery candidate.
        let partial = published(&state, "dddd0000dddd0000-4500-4", &["codex-harness.exe"]);
        assert_eq!(freshest_verified(&state).unwrap(), newer);
        fs::remove_dir_all(&altered).unwrap();
        fs::remove_dir_all(&partial).unwrap();
        fs::remove_dir_all(&newer).unwrap();
        assert_eq!(freshest_verified(&state).unwrap(), older);
    }

    #[test]
    fn preview_and_delivery_never_cross_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        owner_root(&state).unwrap();
        let source = source_fixture(temp.path(), "source");
        let release = published_from(&state, "aaaa0000aaaa0000-1500-1", &source, BINARIES);
        let fast = published_from_profile(
            &state,
            "bbbb0000bbbb0000-2500-2",
            &source,
            BINARIES,
            Profile::DeliveryFast,
        );
        assert_eq!(
            preview_build(&state, &source).unwrap(),
            Some(release.clone())
        );
        assert_eq!(
            preview_build_for(&state, &source, Profile::DeliveryFast).unwrap(),
            Some(fast.clone())
        );
        assert_eq!(freshest_verified(&state).unwrap(), release);
        assert_eq!(
            freshest_verified_for(&state, Profile::DeliveryFast).unwrap(),
            fast.clone()
        );

        let newer_release = published_from(&state, "cccc0000cccc0000-3500-3", &source, BINARIES);
        assert_eq!(preview_build(&state, &source).unwrap(), Some(newer_release));
        assert_eq!(
            preview_build_for(&state, &source, Profile::DeliveryFast).unwrap(),
            Some(fast.clone())
        );
        fs::remove_dir_all(fast).unwrap();
        assert_eq!(
            preview_build_for(&state, &source, Profile::DeliveryFast).unwrap(),
            None
        );
    }

    #[test]
    fn publication_cargo_uses_the_selected_profile_only() {
        let source = Path::new("D:/source");
        let target = Path::new("D:/target");
        let release =
            publication_cargo(source, target, OsStr::new("cargo"), Profile::Release).unwrap();
        let fast =
            publication_cargo(source, target, OsStr::new("cargo"), Profile::DeliveryFast).unwrap();
        let args = |command: &CommandSpec| -> Vec<String> {
            command
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect()
        };
        assert!(
            args(&release)
                .windows(2)
                .any(|pair| pair == ["--profile", "release"])
        );
        assert!(
            args(&fast)
                .windows(2)
                .any(|pair| pair == ["--profile", "delivery-fast"])
        );
        assert!(!args(&fast).iter().any(|arg| arg == "--release"));
    }

    #[test]
    fn publication_jobs_are_four_for_fast_and_one_for_release() {
        let source = Path::new("D:/source");
        let target = Path::new("D:/target");
        let jobs = |profile| {
            let command = publication_cargo(source, target, OsStr::new("cargo"), profile).unwrap();
            let args: Vec<_> = command
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            args.windows(2)
                .find(|pair| pair[0] == "--jobs")
                .map(|pair| pair[1].clone())
        };
        assert_eq!(jobs(Profile::Release).as_deref(), Some("1"));
        assert_eq!(jobs(Profile::DeliveryFast).as_deref(), Some("4"));
    }

    #[test]
    fn fast_reuse_accepts_source_stale_but_release_does_not() {
        use build_identity::Health;
        assert!(reusable_status(Profile::DeliveryFast, &Health::Healthy));
        assert!(reusable_status(Profile::DeliveryFast, &Health::SourceStale));
        assert!(!reusable_status(Profile::Release, &Health::SourceStale));
        assert!(!reusable_status(
            Profile::DeliveryFast,
            &Health::SourceUnavailable
        ));
    }

    #[test]
    fn reuse_selection_rejects_nonmatching_candidates_before_consumer_validation() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        owner_root(&state).unwrap();
        let source = source_fixture(temp.path(), "source")
            .canonicalize()
            .unwrap();
        let identity = build_identity::source_identity(&source).unwrap();
        let matching = published_from(&state, "aaaa0000aaaa0000-1500-1", &source, BINARIES);
        let other = source_fixture(temp.path(), "other").canonicalize().unwrap();
        let _foreign_checkout = published_from(&state, "bbbb0000bbbb0000-2500-2", &other, BINARIES);
        let wrong_toolchain = published_from(&state, "cccc0000cccc0000-3500-3", &source, BINARIES);
        let mut record = build_identity::read_record(&wrong_toolchain).unwrap();
        record.cargo = "another cargo".into();
        fs::write(
            wrong_toolchain.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        // Build recorded from a changed checkout; the live identity no longer
        // matches it once the checkout is restored.
        let changed = source.join("crates/one/src/lib.rs");
        fs::write(&changed, "changed").unwrap();
        let _changed_checkout =
            published_from(&state, "dddd0000dddd0000-4500-4", &source, BINARIES);
        fs::write(&changed, "fixture").unwrap();
        let fast = published_from_profile(
            &state,
            "eeee0000eeee0000-5500-5",
            &source,
            BINARIES,
            Profile::DeliveryFast,
        );

        let candidates = reusable_candidates(
            &state,
            &source,
            "fixture",
            "fixture",
            &identity,
            Profile::Release,
        )
        .unwrap();
        let names: Vec<_> = candidates
            .iter()
            .map(|(path, _)| path.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, vec!["aaaa0000aaaa0000-1500-1".to_owned()]);
        assert_eq!(candidates[0].0, matching);
        let fast_candidates = reusable_candidates(
            &state,
            &source,
            "fixture",
            "fixture",
            &identity,
            Profile::DeliveryFast,
        )
        .unwrap();
        assert!(
            fast_candidates.is_empty(),
            "missing compiled-input evidence must force a fast build"
        );
        assert!(fast.join("build.json").is_file());
        // A candidate whose record is intact but whose recorded bytes changed
        // still reaches consumer validation: filtering is identity, not trust.
        fs::write(matching.join("harness-rtk.exe"), "changed").unwrap();
        let candidates = reusable_candidates(
            &state,
            &source,
            "fixture",
            "fixture",
            &identity,
            Profile::Release,
        )
        .unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(
            consumer_check(&candidates[0].0, Some(&source)).is_err(),
            "a candidate with changed recorded bytes must fail its consumer check"
        );
    }

    /// The compiled-input gate accepts every path the source identity covers,
    /// including fixture and documentation paths, and refuses a compiled input
    /// under the checkout that the inventory cannot name.
    #[test]
    fn compiled_input_gate_covers_identity_paths_and_refuses_uncovered_ones() {
        let temp = tempfile::tempdir().unwrap();
        let source = source_fixture(temp.path(), "source");
        let target = temp.path().join("target");
        let release = target.join(TARGET).join("release");
        let delivery_fast = target.join(TARGET).join(Profile::DeliveryFast.output_dir());
        fs::create_dir_all(source.join("crates/one/tests/fixtures")).unwrap();
        fs::create_dir_all(&release).unwrap();
        fs::create_dir_all(&delivery_fast).unwrap();
        let covered = source.join("crates/one/src/lib.rs");
        let fixture = source.join("crates/one/tests/fixtures/message.txt");
        fs::write(&fixture, "fixture").unwrap();
        let banner = source.join("crates/one/src/banner.md");
        fs::write(&banner, "banner").unwrap();
        let outside = source.join("crates/one/README.md");
        fs::write(&outside, "readme").unwrap();
        let generated = release.join("generated.rs");
        fs::write(&generated, "generated").unwrap();
        let generated_fast = delivery_fast.join("generated.rs");
        fs::write(&generated_fast, "generated fast").unwrap();
        let foreign = temp.path().join("foreign.rs");
        fs::write(&foreign, "foreign").unwrap();
        let identity = build_identity::source_identity(&source).unwrap();
        assert!(
            identity
                .files
                .contains_key("crates/one/tests/fixtures/message.txt")
        );
        assert!(identity.files.contains_key("crates/one/src/banner.md"));
        assert!(!identity.files.contains_key("crates/one/README.md"));
        // Production passes the canonical checkout and target roots; the gate
        // compares canonicalized input paths against them.
        let source = source.canonicalize().unwrap();
        let target = target.canonicalize().unwrap();
        let write = |directory: &Path, binary: &str, input: &Path| {
            for &name in BINARIES {
                fs::write(directory.join(name), "binary").unwrap();
                let chosen = if name == binary {
                    input
                } else {
                    covered.as_path()
                };
                fs::write(
                    directory.join(name).with_extension("d"),
                    format!("{}: {}\n", directory.join(name).display(), chosen.display()),
                )
                .unwrap();
            }
        };
        write(&release, "", &covered);
        verify_compiled_inputs(&target, &source, &identity, Profile::Release).unwrap();
        write(&release, "harness-rtk.exe", &fixture);
        verify_compiled_inputs(&target, &source, &identity, Profile::Release).unwrap();
        write(&release, "codex.exe", &banner);
        verify_compiled_inputs(&target, &source, &identity, Profile::Release).unwrap();
        write(&release, "token-audit.exe", &generated);
        verify_compiled_inputs(&target, &source, &identity, Profile::Release).unwrap();
        write(&delivery_fast, "token-audit.exe", &generated_fast);
        verify_compiled_inputs(&target, &source, &identity, Profile::DeliveryFast).unwrap();
        write(&release, "harness-inspect.exe", &outside);
        let refused = verify_compiled_inputs(&target, &source, &identity, Profile::Release)
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("outside the native input inventory"),
            "{refused}"
        );
        write(&release, "harness-observe.exe", &foreign);
        let refused = verify_compiled_inputs(&target, &source, &identity, Profile::Release)
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("outside owned source/build roots"),
            "{refused}"
        );
    }

    #[test]
    fn compiled_input_evidence_covers_every_binary_and_refuses_missing_or_ambiguous_dep_info() {
        let temp = tempfile::tempdir().unwrap();
        let source = source_fixture(temp.path(), "source")
            .canonicalize()
            .unwrap();
        let target = temp.path().join("target");
        let identity = build_identity::source_identity(&source).unwrap();
        write_dep_files(&target, &source, Profile::DeliveryFast, &[]);
        let release = target.join(TARGET).join("delivery-fast");
        let evidence =
            verify_compiled_inputs(&target, &source, &identity, Profile::DeliveryFast).unwrap();
        assert_eq!(evidence.binaries.len(), BINARIES.len());
        assert!(
            BINARIES
                .iter()
                .all(|binary| evidence.binaries.contains_key(*binary))
        );
        assert!(evidence.binaries.values().all(|paths| !paths.is_empty()));
        assert_eq!(
            evidence.files.get("crates/one/src/lib.rs"),
            identity.files.get("crates/one/src/lib.rs")
        );
        assert_eq!(
            evidence.build_affecting,
            build_identity::build_affecting_identity(&identity.files)
        );

        let missing = release.join("codex.exe").with_extension("d");
        fs::remove_file(&missing).unwrap();
        assert!(
            verify_compiled_inputs(&target, &source, &identity, Profile::DeliveryFast).is_err(),
            "missing dep-info must not authorize reuse"
        );
        write_dep_files(&target, &source, Profile::DeliveryFast, &[]);
        fs::write(&missing, "not dep-info\n").unwrap();
        assert!(
            verify_compiled_inputs(&target, &source, &identity, Profile::DeliveryFast).is_err(),
            "malformed dep-info must not authorize reuse"
        );
        fs::write(
            &missing,
            format!(
                "{}: {}\n{}: {}\n",
                release.join("codex.exe").display(),
                source.join("crates/one/src/lib.rs").display(),
                release.join("second.exe").display(),
                source.join("crates/one/src/lib.rs").display()
            ),
        )
        .unwrap();
        let ambiguous = verify_compiled_inputs(&target, &source, &identity, Profile::DeliveryFast)
            .unwrap_err()
            .to_string();
        assert!(ambiguous.contains("Ambiguous"), "{ambiguous}");
    }

    #[test]
    fn compiled_input_evidence_requires_nonempty_binary_paths_and_exact_union() {
        let temp = tempfile::tempdir().unwrap();
        let source = source_fixture(temp.path(), "source")
            .canonicalize()
            .unwrap();
        let state = temp.path().join("state");
        owner_root(&state).unwrap();
        let target = temp.path().join("target");
        let extra = source.join("crates/one/unrelated.txt");
        fs::write(&extra, "unused source data").unwrap();
        let (build, evidence) = build_with_compiled_evidence(
            &state,
            "aaaa0000aaaa0000-1500-1",
            &source,
            &target,
            Profile::DeliveryFast,
            &[],
        );
        let record = build_identity::read_record(&build).unwrap();
        let evidence_path = build.join(build_identity::COMPILED_INPUTS_FILE);
        let original = fs::read(&evidence_path).unwrap();
        let refused = |candidate: &build_identity::CompiledInputs, label: &str| {
            assert!(
                candidate.validate_against(&build, &record).is_err(),
                "{label} must be refused"
            );
            fs::write(&evidence_path, serde_json::to_vec(candidate).unwrap()).unwrap();
            let identity = build_identity::source_identity(&source).unwrap();
            assert!(
                reusable_candidates(
                    &state,
                    &source,
                    "fixture",
                    "fixture",
                    &identity,
                    Profile::DeliveryFast,
                )
                .unwrap()
                .is_empty(),
                "{label} must not authorize reuse"
            );
        };

        let mut empty_binary = evidence.clone();
        empty_binary.binaries.get_mut("codex.exe").unwrap().clear();
        refused(&empty_binary, "an empty per-binary path list");

        let mut empty_files = evidence.clone();
        for paths in empty_files.binaries.values_mut() {
            paths.clear();
        }
        empty_files.files.clear();
        refused(&empty_files, "empty per-binary and union evidence");

        let mut extra_file = evidence.clone();
        extra_file.files.insert(
            "crates/one/unrelated.txt".into(),
            build_identity::hash_file(&extra).unwrap(),
        );
        refused(&extra_file, "an unreferenced union entry");

        fs::write(&evidence_path, original).unwrap();
        assert!(evidence.validate_against(&build, &record).is_ok());
        let identity = build_identity::source_identity(&source).unwrap();
        assert_eq!(
            reusable_candidates(
                &state,
                &source,
                "fixture",
                "fixture",
                &identity,
                Profile::DeliveryFast,
            )
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn fast_reuse_records_same_binaries_for_the_current_checkout_without_rewriting_the_build() {
        let temp = tempfile::tempdir().unwrap();
        let source = source_fixture(temp.path(), "source")
            .canonicalize()
            .unwrap();
        let state = temp.path().join("state");
        owner_root(&state).unwrap();
        let target = temp.path().join("target");
        let test_file = source.join("crates/one/tests/only_test.rs");
        fs::create_dir_all(test_file.parent().unwrap()).unwrap();
        fs::write(&test_file, "first").unwrap();
        let prior = build_identity::source_identity(&source).unwrap();
        let release = published_from(&state, "aaaa0000aaaa0000-1500-1", &source, BINARIES);
        let (build, _) = build_with_compiled_evidence(
            &state,
            "bbbb0000bbbb0000-2500-2",
            &source,
            &target,
            Profile::DeliveryFast,
            &[],
        );
        let record = build_identity::read_record(&build).unwrap();
        let record_bytes = fs::read(build.join("build.json")).unwrap();
        let evidence_bytes = fs::read(build.join(build_identity::COMPILED_INPUTS_FILE)).unwrap();
        let binaries = record.binaries.clone();

        fs::write(&test_file, "second").unwrap();
        let current = build_identity::source_identity(&source).unwrap();
        assert_ne!(
            current, prior,
            "the test-only change must alter the checkout"
        );
        assert!(
            reusable_candidates(
                &state,
                &source,
                "fixture",
                "fixture",
                &current,
                Profile::Release,
            )
            .unwrap()
            .iter()
            .all(|(path, _)| path != &release),
            "release selection must keep requiring the full checkout identity"
        );

        let reused = find_reusable_with(
            &state,
            &source,
            "fixture",
            "fixture",
            &current,
            Profile::DeliveryFast,
            |_| Ok(true),
        )
        .unwrap()
        .expect("matching compiled inputs permit fast reuse");
        assert!(reused.reused);
        assert_eq!(reused.build, build);

        let selections = state.join(COMPILED_INPUT_SELECTIONS);
        let entries: Vec<_> = fs::read_dir(&selections).unwrap().collect();
        assert_eq!(entries.len(), 1, "one immutable reuse-selection record");
        let selection: CompiledInputSelection =
            serde_json::from_slice(&fs::read(entries[0].as_ref().unwrap().path()).unwrap())
                .unwrap();
        assert_eq!(
            selection.build,
            build.file_name().unwrap().to_str().unwrap()
        );
        assert_eq!(selection.profile, Profile::DeliveryFast.as_str());
        assert_eq!(selection.binaries, binaries);
        assert_eq!(selection.source_root, source);
        assert_eq!(selection.source_sha256, current.sha256);
        assert_eq!(selection.built_source_sha256, prior.sha256);
        assert_eq!(
            selection.build_record_sha256,
            build_identity::hash_bytes(&record_bytes)
        );
        assert_eq!(
            selection.compiled_inputs_sha256,
            build_identity::hash_bytes(&evidence_bytes)
        );
        assert_eq!(selection.relationship, COMPILED_INPUT_RELATIONSHIP);

        assert_eq!(
            fs::read(build.join("build.json")).unwrap(),
            record_bytes,
            "the prior build record must not be rewritten"
        );
        assert_eq!(
            fs::read(build.join(build_identity::COMPILED_INPUTS_FILE)).unwrap(),
            evidence_bytes
        );
        for (name, digest) in binaries {
            assert_eq!(
                build_identity::hash_file(&build.join(name)).unwrap(),
                digest
            );
        }
    }

    #[test]
    fn compiled_and_build_affecting_changes_invalidate_fast_reuse_but_unrelated_files_do_not() {
        let temp = tempfile::tempdir().unwrap();
        let source = source_fixture(temp.path(), "source")
            .canonicalize()
            .unwrap();
        let state = temp.path().join("state");
        owner_root(&state).unwrap();
        let target = temp.path().join("target");
        let compiled_doc = source.join("crates/one/src/fixture.md");
        fs::write(&compiled_doc, "compiled resource").unwrap();
        let (build, _) = build_with_compiled_evidence(
            &state,
            "aaaa0000aaaa0000-1500-1",
            &source,
            &target,
            Profile::DeliveryFast,
            &[("codex.exe", compiled_doc.clone())],
        );
        let evidence_path = build.join(build_identity::COMPILED_INPUTS_FILE);
        let evidence = fs::read(&evidence_path).unwrap();
        let candidates = || {
            let identity = build_identity::source_identity(&source).unwrap();
            reusable_candidates(
                &state,
                &source,
                "fixture",
                "fixture",
                &identity,
                Profile::DeliveryFast,
            )
            .unwrap()
        };
        assert_eq!(candidates().len(), 1, "baseline compiled evidence matches");

        fs::write(&compiled_doc, "changed compiled resource").unwrap();
        assert!(
            candidates().is_empty(),
            "a compiled documentation-like file must invalidate"
        );
        fs::write(&compiled_doc, "compiled resource").unwrap();
        assert_eq!(candidates().len(), 1);

        let unrelated = source.join("crates/one/unrelated.txt");
        fs::write(&unrelated, "not compiled").unwrap();
        assert_eq!(
            candidates().len(),
            1,
            "an unrelated new file must not force a build"
        );
        fs::remove_file(&unrelated).unwrap();

        fs::remove_file(&evidence_path).unwrap();
        assert!(
            candidates().is_empty(),
            "missing compiled-input evidence must force a build"
        );
        fs::write(&evidence_path, &evidence).unwrap();
        let mut tampered: serde_json::Value = serde_json::from_slice(&evidence).unwrap();
        tampered["binaries"]
            .as_object_mut()
            .unwrap()
            .remove("codex.exe");
        fs::write(&evidence_path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        assert!(
            candidates().is_empty(),
            "incomplete per-binary evidence must force a build"
        );
        fs::write(&evidence_path, &evidence).unwrap();

        for path in ["Cargo.lock", "Cargo.toml"] {
            let file = source.join(path);
            let original = fs::read(&file).unwrap();
            fs::write(&file, b"changed build input").unwrap();
            assert!(
                candidates().is_empty(),
                "{path} must require new compatible evidence"
            );
            fs::write(&file, original).unwrap();
            assert_eq!(candidates().len(), 1);
        }

        let toolchain = source.join("rust-toolchain.toml");
        fs::write(&toolchain, "[toolchain]\nchannel='stable'\n").unwrap();
        assert!(candidates().is_empty(), "toolchain identity changed");
        fs::remove_file(&toolchain).unwrap();
        assert_eq!(candidates().len(), 1);

        let cargo_config = source.join(".cargo/config.toml");
        fs::create_dir_all(cargo_config.parent().unwrap()).unwrap();
        fs::write(&cargo_config, "[build]\nrustflags=[]\n").unwrap();
        assert!(candidates().is_empty(), "Cargo host configuration changed");
        fs::remove_dir_all(cargo_config.parent().unwrap()).unwrap();
        assert_eq!(candidates().len(), 1);
    }

    #[test]
    fn delivery_refuses_a_state_without_verified_builds() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        owner_root(&state).unwrap();
        let error = freshest_verified(&state).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("integrity-verified native build"),
            "{error}"
        );
        let foreign = temp.path().join("foreign");
        fs::create_dir_all(&foreign).unwrap();
        fs::write(foreign.join("keep"), "keep").unwrap();
        assert!(freshest_verified(&foreign).is_err());
        assert_eq!(fs::read_to_string(foreign.join("keep")).unwrap(), "keep");
    }

    #[test]
    fn delivery_ignores_builds_from_another_or_changed_source() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        owner_root(&state).unwrap();
        let first = source_fixture(temp.path(), "one");
        let second = source_fixture(temp.path(), "two");
        let from_first = published_from(&state, "aaaa0000aaaa0000-1500-1", &first, BINARIES);
        let from_second = published_from(&state, "bbbb0000bbbb0000-2500-2", &second, BINARIES);
        assert_eq!(freshest_verified(&state).unwrap(), from_second);
        assert_eq!(
            freshest_for_source(&state, &first, true, Profile::Release).unwrap(),
            Some(from_first.clone())
        );
        // A build whose live source no longer matches is not deliverable.
        fs::write(first.join("crates/one/src/lib.rs"), "changed").unwrap();
        assert_eq!(
            freshest_for_source(&state, &first, true, Profile::Release).unwrap(),
            None
        );
        assert_eq!(
            preview_build(&state, &first).unwrap(),
            Some(from_first.clone())
        );
        fs::write(from_first.join(BINARIES[0]), "altered").unwrap();
        assert_eq!(preview_build(&state, &first).unwrap(), None);
        let missing = temp.path().join("missing");
        assert!(preview_build(&missing, &first).is_err());
        assert!(!missing.exists());
    }

    #[test]
    fn delivery_build_requires_a_manager_inside_an_owned_state() {
        let source = std::env::current_dir().unwrap();
        let error = delivery_build(&source).unwrap_err();
        assert!(error.to_string().contains("--build DIRECTORY"), "{error}");
    }

    #[test]
    fn delivery_returns_a_spelling_the_lifecycle_accepts() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        owner_root(&state).unwrap();
        let source = source_fixture(temp.path(), "source");
        let older = published_from(&state, "aaaa0000aaaa0000-1500-1", &source, BINARIES);
        let newer = published_from(&state, "bbbb0000bbbb0000-2500-2", &source, BINARIES);
        // The manager runs from the older build while a fresher one exists.
        let manager = older.join("codex-harness.exe");
        let delivered = delivery_from(&manager, &source).unwrap();
        assert_eq!(delivered, newer);
        assert!(
            !delivered.to_string_lossy().starts_with(r"\\?\"),
            "verbatim spelling is rejected by the installer: {delivered:?}"
        );
        assert!(crate::installation_state::normal(&delivered).is_ok());
        // A source without a matching build is refused instead of guessed.
        let other = source_fixture(temp.path(), "other");
        let error = delivery_from(&manager, &other).unwrap_err();
        assert!(
            error.to_string().contains("matches the selected source"),
            "{error}"
        );
    }

    #[test]
    fn delivery_from_uses_the_manager_profile() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("state");
        owner_root(&state).unwrap();
        let source = source_fixture(temp.path(), "source");
        let fast_old = published_from_profile(
            &state,
            "aaaa0000aaaa0000-1500-1",
            &source,
            BINARIES,
            Profile::DeliveryFast,
        );
        let fast_new = published_from_profile(
            &state,
            "bbbb0000bbbb0000-2500-2",
            &source,
            BINARIES,
            Profile::DeliveryFast,
        );
        let _release_new = published_from(&state, "cccc0000cccc0000-3500-3", &source, BINARIES);
        let delivered = delivery_from(&fast_old.join("codex-harness.exe"), &source).unwrap();
        assert_eq!(delivered, fast_new);
    }

    /// Create an NTFS junction without requiring the symlink privilege.
    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(not(windows))]
    fn junction(link: &Path, target: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    /// Give a file a clearly stale timestamp.
    fn backdate(path: &Path) {
        let old = SystemTime::now() - Duration::from_secs(30 * 24 * 60 * 60);
        OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }

    /// Plant an owned entry whose lease is not held, like a killed operation.
    /// The recorded process identifier is diagnostic and proves nothing.
    fn abandoned_entry(root: &Path, pid: u32) -> PathBuf {
        let dir = root.join(format!("hcb-abandoned-{pid:08x}"));
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join(SCRATCH_OWNER_FILE),
            format!("{SCRATCH_OWNER}pid={pid} started=1\n"),
        )
        .unwrap();
        fs::write(dir.join(SCRATCH_LEASE_FILE), b"").unwrap();
        dir
    }

    fn owned_root_fixture(outer: &Path) -> PathBuf {
        let root = outer.join("chx");
        fs::create_dir(&root).unwrap();
        write_scratch_record(&root).unwrap();
        root
    }

    fn fast_fixture(outer: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let root = owned_root_fixture(outer);
        let state = outer.join("state");
        owner_root(&state).unwrap();
        let source = source_fixture(outer, "source").canonicalize().unwrap();
        (root, source, state)
    }

    fn open_fast_target(
        root: &Path,
        source: &Path,
        state: &Path,
        profile: Profile,
    ) -> io::Result<Option<FastTarget>> {
        let identity = build_identity::source_identity(source)?;
        fast_target_in(
            root,
            source,
            state,
            &identity,
            "rustc fixture",
            "cargo fixture",
            profile,
        )
    }

    #[test]
    fn scratch_root_is_adopted_only_with_its_ownership_record() {
        let outer = tempfile::tempdir().unwrap();
        let foreign = outer.path().join("chx");
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("foreign"), "keep").unwrap();
        assert!(owned_scratch_root(&foreign, true).unwrap().is_none());
        assert_eq!(fs::read_to_string(foreign.join("foreign")).unwrap(), "keep");
        assert!(!foreign.join(SCRATCH_OWNER_FILE).exists());
        let file = outer.path().join("chx-file");
        fs::write(&file, "keep").unwrap();
        assert!(owned_scratch_root(&file, true).unwrap().is_none());
        assert_eq!(fs::read_to_string(&file).unwrap(), "keep");
        let empty = outer.path().join("chx-empty");
        fs::create_dir(&empty).unwrap();
        assert!(owned_scratch_root(&empty, true).unwrap().is_some());
        assert!(carries_scratch_ownership(&empty.join(SCRATCH_OWNER_FILE)));
        assert!(owned_scratch_root(&empty, false).unwrap().is_some());
    }

    #[test]
    fn scratch_entries_hold_a_live_lease_and_keep_paths_short() {
        let outer = tempfile::tempdir().unwrap();
        let root = owned_root_fixture(outer.path());
        let entry = ScratchEntry::leased(&root).unwrap();
        let path = entry.path().to_owned();
        // One short component beneath the owned root keeps compiler paths short.
        assert_eq!(path.parent(), Some(root.as_path()));
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.len() <= 8, "unexpected scratch name {name}");
        assert!(carries_scratch_ownership(&path.join(SCRATCH_OWNER_FILE)));
        // A second operation cannot take a live lease.
        let lease = path.join(SCRATCH_LEASE_FILE);
        let probe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lease)
            .unwrap();
        assert!(probe.try_lock().is_err(), "lease is live");
        drop(probe);
        // Normal unwinding removes the whole tree and its record.
        drop(entry);
        assert!(
            !path.exists(),
            "normal cleanup failed for {}",
            path.display()
        );
    }

    #[test]
    fn sweep_reclaims_only_abandoned_owned_entries() {
        let outer = tempfile::tempdir().unwrap();
        let root = owned_root_fixture(outer.path());

        // Old foreign prefixed data inside the owned root has no record.
        let foreign = root.join("hcb-old-foreign");
        fs::create_dir_all(foreign.join("nested")).unwrap();
        fs::write(foreign.join("nested/keep"), "keep").unwrap();
        backdate(&foreign.join("nested/keep"));
        // A legacy directory beside the root is a neighboring namespace.
        let neighbor = outer.path().join("hcc-neighbor");
        fs::create_dir(&neighbor).unwrap();
        fs::write(neighbor.join("keep"), "keep").unwrap();
        backdate(&neighbor.join("keep"));
        // An outside sentinel reachable only through a reparse point.
        let sentinel = outer.path().join("outside");
        fs::create_dir(&sentinel).unwrap();
        fs::write(sentinel.join("keep"), "keep").unwrap();
        let link = root.join("hcb-reparse-entry");
        assert!(junction(&link, &sentinel), "junction creation failed");

        // An active operation keeps a live lease even while it looks ancient.
        let active = ScratchEntry::leased(&root).unwrap();
        let active_lease = active.path().join(SCRATCH_LEASE_FILE);
        backdate(&active.path().join(SCRATCH_OWNER_FILE));
        let probe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&active_lease)
            .unwrap();
        assert!(probe.try_lock().is_err(), "lease is live");

        // Abandoned owned scratch: no lease, whatever its name or age.
        let abandoned = abandoned_entry(&root, std::process::id());
        // Abandoned owned scratch whose removal a foreign handle blocks.
        // Non-Windows removal does not fail on open files, so this case is
        // Windows-only: it is the platform whose cleanup semantics matter here.
        #[cfg(windows)]
        let blocked = abandoned_entry(&root, 0);
        #[cfg(windows)]
        fs::write(blocked.join("held"), b"x").unwrap();
        #[cfg(windows)]
        let hold = {
            use std::os::windows::fs::OpenOptionsExt;
            OpenOptions::new()
                .write(true)
                .share_mode(0)
                .open(blocked.join("held"))
                .unwrap()
        };

        sweep_owned_scratch(&root);

        assert!(
            !abandoned.exists(),
            "an abandoned owned entry is reclaimed whatever it records"
        );
        assert!(active.path().is_dir(), "an active lease survives");
        #[cfg(windows)]
        assert!(
            blocked.join("held").is_file(),
            "blocked removal is preserved"
        );
        #[cfg(windows)]
        assert!(
            blocked.join(SCRATCH_OWNER_FILE).is_file(),
            "a failed cleanup keeps the entry reclaimable"
        );
        assert!(
            foreign.join("nested/keep").is_file(),
            "foreign data survives"
        );
        assert!(link.is_dir(), "a reparse entry is skipped");
        assert_eq!(fs::read_to_string(sentinel.join("keep")).unwrap(), "keep");
        assert_eq!(fs::read_to_string(neighbor.join("keep")).unwrap(), "keep");

        // Once the block is gone the next sweep reclaims the entry.
        #[cfg(windows)]
        drop(hold);
        sweep_owned_scratch(&root);
        #[cfg(windows)]
        assert!(!blocked.exists());
        assert!(active.path().is_dir());
        assert!(link.is_dir());
        assert_eq!(fs::read_to_string(sentinel.join("keep")).unwrap(), "keep");
    }

    #[test]
    fn fast_target_is_keyed_leased_and_reacquired() {
        let outer = tempfile::tempdir().unwrap();
        let (root, source, state) = fast_fixture(outer.path());
        let identity = build_identity::source_identity(&source).unwrap();
        let key = fast_target_key(
            &identity,
            "rustc fixture",
            "cargo fixture",
            Profile::DeliveryFast,
        )
        .unwrap();
        let target = open_fast_target(&root, &source, &state, Profile::DeliveryFast)
            .unwrap()
            .unwrap();
        let path = target.path().to_owned();
        assert_eq!(path.parent(), Some(root.as_path()));
        assert_eq!(
            path.file_name().unwrap(),
            format!("f{}", &key[..16]).as_str()
        );
        assert_eq!(
            fast_owner_identity(&path.join(SCRATCH_OWNER_FILE)).map(|(key, _)| key),
            Some(key.clone())
        );
        let probe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join(SCRATCH_LEASE_FILE))
            .unwrap();
        assert!(probe.try_lock().is_err(), "fast target lease is live");
        drop(probe);
        assert!(
            open_fast_target(&root, &source, &state, Profile::DeliveryFast)
                .unwrap()
                .is_none(),
            "a live fast target is not acquired twice"
        );
        drop(target);
        let acquired = open_fast_target(&root, &source, &state, Profile::DeliveryFast)
            .unwrap()
            .unwrap();
        assert_eq!(acquired.path(), path);
        drop(acquired);

        fs::write(source.join("crates/one/src/lib.rs"), "changed source").unwrap();
        let changed = build_identity::source_identity(&source).unwrap();
        assert_eq!(
            fast_target_key(
                &changed,
                "rustc fixture",
                "cargo fixture",
                Profile::DeliveryFast
            )
            .unwrap(),
            key,
            "ordinary source bytes do not key the target"
        );
        fs::write(source.join("Cargo.lock"), "changed lock").unwrap();
        let relocked = build_identity::source_identity(&source).unwrap();
        assert_ne!(
            fast_target_key(
                &relocked,
                "rustc fixture",
                "cargo fixture",
                Profile::DeliveryFast
            )
            .unwrap(),
            key,
            "a build-affecting input changes the target key"
        );
    }

    #[test]
    fn fast_target_preserves_foreign_and_ambiguous_entries() {
        let outer = tempfile::tempdir().unwrap();
        let (root, source, state) = fast_fixture(outer.path());
        let identity = build_identity::source_identity(&source).unwrap();
        let key = fast_target_key(
            &identity,
            "rustc fixture",
            "cargo fixture",
            Profile::DeliveryFast,
        )
        .unwrap();
        let path = fast_target_path(&root, &key);
        fs::create_dir(&path).unwrap();
        fs::write(path.join("foreign"), "keep").unwrap();
        assert!(
            open_fast_target(&root, &source, &state, Profile::DeliveryFast)
                .unwrap()
                .is_none()
        );
        assert_eq!(fs::read_to_string(path.join("foreign")).unwrap(), "keep");
        fs::remove_dir_all(&path).unwrap();

        let state_id = build_identity::hash_bytes(
            state
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase()
                .as_bytes(),
        );
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join(SCRATCH_OWNER_FILE),
            fast_owner_record(&key, &state_id),
        )
        .unwrap();
        fs::write(path.join(SCRATCH_LEASE_FILE), "").unwrap();
        fs::write(path.join(FAST_USED_FILE), "interrupted").unwrap();
        fs::write(path.join("partial-target"), "keep").unwrap();
        assert!(
            open_fast_target(&root, &source, &state, Profile::DeliveryFast)
                .unwrap()
                .is_none(),
            "a target without successful evidence is ambiguous"
        );
        assert_eq!(
            fs::read_to_string(path.join("partial-target")).unwrap(),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(path.join(FAST_USED_FILE)).unwrap(),
            "interrupted"
        );
        assert!(path.join(SCRATCH_OWNER_FILE).is_file());
    }

    #[test]
    fn fast_target_rejects_changed_bytes_with_a_restored_timestamp() {
        let outer = tempfile::tempdir().unwrap();
        let (root, source, state) = fast_fixture(outer.path());
        let identity = build_identity::source_identity(&source).unwrap();
        let target = open_fast_target(&root, &source, &state, Profile::DeliveryFast)
            .unwrap()
            .unwrap();
        target.record_success(&identity).unwrap();
        drop(target);

        let changed = source.join("crates/one/src/lib.rs");
        fs::write(&changed, "changed bytes").unwrap();
        OpenOptions::new()
            .write(true)
            .open(&changed)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(5))
            .unwrap();
        assert!(
            open_fast_target(&root, &source, &state, Profile::DeliveryFast)
                .unwrap()
                .is_none(),
            "an older changed input must use fresh scratch"
        );

        OpenOptions::new()
            .write(true)
            .open(&changed)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        assert!(
            open_fast_target(&root, &source, &state, Profile::DeliveryFast)
                .unwrap()
                .is_some(),
            "a provably newer input may use Cargo's rebuild path"
        );
    }

    #[test]
    fn ordinary_release_never_acquires_a_fast_target() {
        let outer = tempfile::tempdir().unwrap();
        let (root, source, state) = fast_fixture(outer.path());
        let identity = build_identity::source_identity(&source).unwrap();
        let target = open_fast_target(&root, &source, &state, Profile::DeliveryFast)
            .unwrap()
            .unwrap();
        target.record_success(&identity).unwrap();
        let used = target.path().join(FAST_USED_FILE);
        let before = fs::read_to_string(&used).unwrap();
        assert!(
            open_fast_target(&root, &source, &state, Profile::Release)
                .unwrap()
                .is_none(),
            "release never inherits a live fast target"
        );
        drop(target);
        assert!(
            open_fast_target(&root, &source, &state, Profile::Release)
                .unwrap()
                .is_none(),
            "release never inherits successful fast evidence"
        );
        assert_eq!(fs::read_to_string(used).unwrap(), before);
    }

    #[test]
    fn inactive_fast_target_retires_only_after_the_quiet_period() {
        let outer = tempfile::tempdir().unwrap();
        let (root, source, state) = fast_fixture(outer.path());
        let first = open_fast_target(&root, &source, &state, Profile::DeliveryFast)
            .unwrap()
            .unwrap();
        let first_path = first.path().to_owned();
        drop(first);
        fs::write(source.join("Cargo.lock"), "second").unwrap();
        let second = open_fast_target(&root, &source, &state, Profile::DeliveryFast)
            .unwrap()
            .unwrap();
        let second_path = second.path().to_owned();
        drop(second);
        fs::write(source.join("Cargo.lock"), "third").unwrap();
        let active = open_fast_target(&root, &source, &state, Profile::DeliveryFast)
            .unwrap()
            .unwrap();
        backdate(&first_path.join(FAST_USED_FILE));
        backdate(&active.path().join(FAST_USED_FILE));
        let other = root.join("fother-state");
        fs::create_dir(&other).unwrap();
        fs::write(
            other.join(SCRATCH_OWNER_FILE),
            fast_owner_record("other-key", "other-state"),
        )
        .unwrap();
        fs::write(other.join(SCRATCH_LEASE_FILE), "").unwrap();
        fs::write(other.join(FAST_USED_FILE), "quiet").unwrap();
        backdate(&other.join(FAST_USED_FILE));

        retire_inactive_fast_targets(&root, &fast_state_id(&state).unwrap(), SystemTime::now());

        assert!(!first_path.exists(), "quiet owned target is retired");
        assert!(second_path.exists(), "recent owned target is preserved");
        assert!(active.path().is_dir(), "live leased target is preserved");
        assert!(other.is_dir(), "another installation's target is preserved");
    }

    #[test]
    fn delivery_declaration_matches_runtime_and_builds_in_one_command() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifest = root.join("Cargo.toml");
        let delivery = declared_delivery(&manifest).expect("root delivery declaration");
        assert_eq!(
            delivery,
            BINARIES
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>(),
            "source declaration and runtime delivery list drifted"
        );
        assert!(
            !delivery.iter().any(|name| name.contains("fixture")),
            "test fixtures are not delivery programs"
        );
        let command = declared_delivery_cargo(
            &root,
            Path::new("D:/target"),
            OsStr::new("cargo"),
            Profile::DeliveryFast,
            &delivery,
        )
        .unwrap();
        let args: Vec<String> = command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args.first().map(String::as_str), Some("build"));
        assert_eq!(args.iter().filter(|arg| *arg == "--profile").count(), 1);
        assert!(!args.iter().any(|arg| arg == "--bins"));
        let selected: Vec<&str> = args
            .windows(2)
            .filter(|pair| pair[0] == "--bin")
            .map(|pair| pair[1].as_str())
            .collect();
        assert_eq!(
            selected,
            BINARIES
                .iter()
                .map(|name| name.strip_suffix(".exe").unwrap())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn unavailable_delivery_declaration_keeps_the_two_phase_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let manifest = temp.path().join("Cargo.toml");
        assert!(declared_delivery(&manifest).is_none());
        for invalid in [
            "[workspace]\nmembers = []\n",
            "[workspace.metadata.harness]\ndelivery = []\n",
            "[workspace.metadata.harness]\ndelivery = [\"bad/name.exe\"]\n",
            "[workspace.metadata.harness]\ndelivery = [\"one.exe\", \"one.exe\"]\n",
            "[workspace.metadata.harness]\ndelivery = [1]\n",
        ] {
            fs::write(&manifest, invalid).unwrap();
            assert!(declared_delivery(&manifest).is_none(), "{invalid}");
        }
        fs::write(
            &manifest,
            "[workspace.metadata.harness]\ndelivery = [\"one.exe\", \"new-program.exe\"]\n",
        )
        .unwrap();
        assert_eq!(
            declared_delivery(&manifest).unwrap(),
            ["one.exe", "new-program.exe"]
        );
        write_delivery_selection(temp.path(), None).unwrap();
        let evidence: serde_json::Value =
            serde_json::from_slice(&fs::read(temp.path().join(DELIVERY_SELECTION_FILE)).unwrap())
                .unwrap();
        assert_eq!(evidence["selection"], "two-phase-manager-query");
        assert_eq!(evidence["one_cargo_invocation"], false);
    }

    #[test]
    fn admitted_compiler_spawn_uses_the_aggregate_owner() {
        let temp = tempfile::tempdir().unwrap();
        let account = temp.path().join("account");
        heavy_command::prepare(&account).unwrap();
        let budget = heavy_command::Budget {
            memory_bytes: 2 * 1024 * 1024 * 1024,
            aggregate_memory_limit_bytes: 2 * 1024 * 1024 * 1024,
            max_concurrent_trees: 1,
            deadline_seconds: 60,
            queue_wait_seconds: 30,
            cpu_percent: None,
        };
        let admission = heavy_command::Admission::acquire(
            &account,
            &budget,
            "compiler",
            &Cancellation::default(),
        )
        .unwrap();
        let (job, _) = admission.owned_job(&budget, &account).unwrap();
        let mut command = CommandSpec::new(std::env::current_exe().unwrap());
        command.args = [
            "--exact",
            "heavy_command::tests::direct_exit_probe",
            "--ignored",
            "--nocapture",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        let child = start_admitted_compiler(&admission, &job, &command)
            .unwrap_or_else(|error| panic!("admitted compiler spawn failed: {error}"));
        assert!(
            admission.payload_is_in_aggregate(&child).unwrap(),
            "the compiler payload is outside the account aggregate"
        );

        let source = include_str!("native_build.rs");
        let phase = source
            .find("fn run_publication_phase(")
            .expect("publication phase runner");
        let window = &source[phase..phase + 1600];
        assert!(
            window.contains("let (job, marker) = admission.owned_job"),
            "{window}"
        );
        assert!(window.contains("start_admitted_compiler("), "{window}");
        assert!(!window.contains(".spawn("), "{window}");
        let prepare = source
            .find("pub fn prepare(")
            .expect("publication entry point");
        assert!(
            source[prepare..].contains("run_publication_phase("),
            "publication compiles through the admitted phase runner"
        );
    }

    #[test]
    fn completion_selection_is_exact_or_falls_back_to_whole_package_bins() {
        let delivery = vec!["codex-harness.exe".to_owned(), "token-audit.exe".to_owned()];
        let args: Vec<String> = publication_bin_selection(Some(&delivery))
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["--bin", "codex-harness", "--bin", "token-audit"]);
        let fallback: Vec<String> = publication_bin_selection(None)
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(fallback, ["--bins"]);
    }

    #[test]
    fn delivery_probe_answers_only_its_own_request_shape() {
        let temp = tempfile::tempdir().unwrap();
        let probe = temp.path().join("probe.json");
        fs::write(&probe, br#"{"schema":1,"kind":"delivery-binaries-v1"}"#).unwrap();
        assert!(answer_delivery_probe(&probe).unwrap());
        let other = temp.path().join("other.json");
        fs::write(&other, br#"{"schema":1,"source":"x"}"#).unwrap();
        assert!(!answer_delivery_probe(&other).unwrap());
    }

    #[test]
    fn delivery_probe_classification_distinguishes_legacy_from_failure() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("probe.log");
        fs::write(&log, "probe").unwrap();
        let response = temp.path().join("probe.json");
        fs::write(
            &response,
            br#"{"schema":1,"delivery":["codex-harness.exe","token-audit.exe"]}"#,
        )
        .unwrap();
        let names = classify_delivery_probe(StopReason::Exited, 0, &log, &response)
            .unwrap()
            .unwrap();
        assert_eq!(names, ["codex-harness.exe", "token-audit.exe"]);
        // An ordinary nonzero exit is the pre-query manager signature.
        assert!(
            classify_delivery_probe(StopReason::Exited, 2, &log, &response)
                .unwrap()
                .is_none()
        );
        // An incomplete query is a failure, not a legacy signature.
        let failed = classify_delivery_probe(StopReason::Timeout, 124, &log, &response)
            .unwrap_err()
            .to_string();
        assert!(
            failed.contains("Timeout") && failed.contains("probe.log"),
            "{failed}"
        );
        // A successful answer must still be a usable delivery set.
        fs::write(&response, br#"{"schema":1,"delivery":[]}"#).unwrap();
        let empty = classify_delivery_probe(StopReason::Exited, 0, &log, &response)
            .unwrap_err()
            .to_string();
        assert!(empty.contains("unusable set"), "{empty}");
        fs::write(&response, b"not json").unwrap();
        let unreadable = classify_delivery_probe(StopReason::Exited, 0, &log, &response)
            .unwrap_err()
            .to_string();
        assert!(unreadable.contains("unreadable response"), "{unreadable}");
    }
}
