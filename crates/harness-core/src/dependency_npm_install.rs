//! Native provisioning of missing shared npm MCP packages.
//!
//! Installs one explicitly selected, integrity-checked official package into
//! the shared user npm tree through a journaled create-directory transaction.
//! Existing installations, foreign provenance markers and shared lockfiles
//! are preserved; a failed post-activation check rolls the activation back.
#![cfg(windows)]

use crate::{
    build_identity,
    dependency_discovery::local_path,
    dependency_mcp_probe::{self, ProbeKind},
    dependency_package::{self, contained},
    dependency_stage,
    native_build::ordinary_ancestors,
    registration_native::{FileGuard, StagedFile},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

/// Audited unmodified official Nuphus native executables by version.
const AUDITED_NUPHUS_ORIGINALS: &[(&str, &str)] = &[(
    "0.2.2",
    "9a07112f17a964d9c0b1a54653af95559d7de33cce1cb3dffd60dfc4c85ccfb0",
)];

pub struct Request {
    pub manager: PathBuf,
    pub user_home: PathBuf,
    pub state: PathBuf,
    pub node: Option<PathBuf>,
}

struct Staged {
    candidate: PathBuf,
    executable: PathBuf,
    sources: Vec<Value>,
}

trait Ops {
    fn stage(&mut self, request: &Request, id: &str, version: &str) -> io::Result<Staged>;
    fn probe(
        &mut self,
        executable: &Path,
        kind: ProbeKind,
        expected_sha256: &str,
    ) -> io::Result<Value>;
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "dependency provisioning input or candidate was rejected",
    )
}

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
}

fn package_identity(id: &str) -> io::Result<(&'static str, &'static str, ProbeKind)> {
    match id {
        "codebase-memory" => Ok((
            "codebase-memory-mcp",
            "codebase-memory-mcp",
            ProbeKind::CodebaseMemory,
        )),
        "nuphus" => Ok(("@nuphus/nuphus-mcp", "nuphus-mcp", ProbeKind::Nuphus)),
        _ => Err(invalid()),
    }
}

fn stable_version(version: &str) -> bool {
    let date = |text: &str| {
        text.len() == 10
            && text.as_bytes()[4] == b'-'
            && text.as_bytes()[7] == b'-'
            && text
                .bytes()
                .enumerate()
                .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    };
    let dotted = |text: &str| {
        let text = text.strip_prefix('v').unwrap_or(text);
        !text.is_empty()
            && text.split('.').all(|part| {
                !part.is_empty()
                    && part.len() <= 10
                    && part.bytes().all(|byte| byte.is_ascii_digit())
                    && (part.len() == 1 || !part.starts_with('0'))
            })
    };
    dotted(version) || date(version)
}

fn hex_bytes(text: &str) -> io::Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    (text.bytes())
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|pair| {
            let hi = (pair[0] as char).to_digit(16).ok_or_else(invalid)?;
            let lo = (pair[1] as char).to_digit(16).ok_or_else(invalid)?;
            Ok((hi * 16 + lo) as u8)
        })
        .collect()
}

fn collect_tree(path: &Path, root: &Path, files: &mut BTreeMap<String, String>) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            return Err(conflict("Dependency tree contains an external link"));
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(io::Error::other)?
            .to_owned();
        let name = relative.to_string_lossy().replace('\\', "/");
        if metadata.is_dir() {
            collect_tree(&entry.path(), root, files)?;
        } else if metadata.is_file() {
            files.insert(name, build_identity::hash_file(&entry.path())?);
        }
    }
    Ok(())
}

fn tree_identity(path: &Path, overrides: &BTreeMap<String, String>) -> io::Result<String> {
    let mut files = BTreeMap::new();
    collect_tree(path, path, &mut files)?;
    for (name, digest) in overrides {
        files.insert(name.clone(), digest.clone());
    }
    let mut digest = Sha256::new();
    for (name, fingerprint) in files {
        let bytes = hex_bytes(&fingerprint)?;
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(&bytes);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn random_token() -> String {
    // Reuse tempfile's dependency-free randomness without adding a crate.
    let unique = tempfile::Builder::new()
        .prefix("harness-dependency-token-")
        .tempdir()
        .expect("temporary randomness source");
    let name = unique
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("temporary randomness name")
        .to_owned();
    unique.close().expect("temporary randomness cleanup");
    name
}

pub(crate) struct InstallationLock {
    path: PathBuf,
    token: String,
}

impl InstallationLock {
    pub(crate) fn acquire(state: &Path, installation: &Path) -> io::Result<Self> {
        let root = state.join("locks");
        ordinary_ancestors(&root)?;
        fs::create_dir_all(&root)?;
        build_identity::ordinary(&root)?;
        let key = format!(
            "{:x}",
            Sha256::digest(installation.to_string_lossy().as_bytes())
        );
        let path = root.join(format!("{key}.lock"));
        let token = random_token();
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(conflict(
                    "Another updater holds the installation lock; preserving shared state.",
                ));
            }
            Err(error) => return Err(error),
        };
        let record =
            json!({"pid": std::process::id(), "token": token, "installation": installation});
        serde_json::to_writer(&mut file, &record)?;
        Ok(Self { path, token })
    }
}

impl Drop for InstallationLock {
    fn drop(&mut self) {
        if fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|record| record["token"] == json!(self.token))
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct RealOps;

impl Ops for RealOps {
    fn stage(&mut self, request: &Request, id: &str, version: &str) -> io::Result<Staged> {
        match id {
            "codebase-memory" => {
                let report = dependency_stage::prepare(
                    &request.manager,
                    "codebase-memory-mcp",
                    version,
                    &request.state,
                )?;
                let candidate = report_path(&report, "candidate")?;
                let executable = candidate.join("bin/codebase-memory-mcp.exe");
                Ok(Staged {
                    executable,
                    candidate,
                    sources: report_sources(&report),
                })
            }
            "nuphus" => {
                let Some((_, expected)) = AUDITED_NUPHUS_ORIGINALS
                    .iter()
                    .find(|(audited, _)| *audited == version)
                else {
                    return Err(conflict(
                        "This Nuphus version has not passed the source adapter compatibility audit",
                    ));
                };
                if std::env::consts::ARCH != "x86_64" {
                    return Err(conflict(
                        "Only the audited Windows x64 Nuphus binary is supported",
                    ));
                }
                let package = dependency_stage::prepare(
                    &request.manager,
                    "@nuphus/nuphus-mcp",
                    version,
                    &request.state,
                )?;
                let platform = dependency_stage::prepare(
                    &request.manager,
                    "@nuphus/nuphus-mcp-win32-x64",
                    version,
                    &request.state,
                )?;
                let candidate = report_path(&package, "candidate")?;
                let platform_candidate = report_path(&platform, "candidate")?;
                let destination = candidate.join("node_modules/@nuphus/nuphus-mcp-win32-x64");
                let parent = destination.parent().ok_or_else(invalid)?;
                ordinary_ancestors(parent)?;
                fs::create_dir_all(parent)?;
                if fs::symlink_metadata(&destination).is_ok() {
                    return Err(conflict("Nuphus platform payload is already staged"));
                }
                fs::rename(&platform_candidate, &destination)?;
                let executable = destination.join("bin/nuphus-mcp.exe");
                if !executable.is_file() || build_identity::hash_file(&executable)? != *expected {
                    let _ = fs::remove_dir_all(&destination);
                    return Err(conflict(
                        "Nuphus native binary differs from the audited official executable",
                    ));
                }
                let mut sources = report_sources(&package);
                sources.extend(report_sources(&platform));
                Ok(Staged {
                    candidate,
                    executable,
                    sources,
                })
            }
            _ => Err(invalid()),
        }
    }

    fn probe(
        &mut self,
        executable: &Path,
        kind: ProbeKind,
        expected_sha256: &str,
    ) -> io::Result<Value> {
        dependency_mcp_probe::probe(executable, kind, expected_sha256)
    }
}

fn report_path(report: &Value, field: &str) -> io::Result<PathBuf> {
    report[field]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("native stage omitted its candidate root"))
}

fn report_sources(report: &Value) -> Vec<Value> {
    [
        report["source"]["metadata"].clone(),
        report["source"]["archive"].clone(),
    ]
    .into_iter()
    .filter(|value| value.is_string())
    .collect()
}

fn resolve_node(request: &Request) -> io::Result<Option<PathBuf>> {
    if let Some(node) = &request.node {
        let node = local_path(node)?;
        return if node.is_file() {
            Ok(Some(node))
        } else {
            Err(conflict(
                "The existing Node.js runtime is required; no duplicate Node is installed",
            ))
        };
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for (index, entry) in std::env::split_paths(&path).enumerate() {
        if index == 256 {
            return Err(invalid());
        }
        if !entry.is_absolute() {
            continue;
        }
        let candidate = entry.join("node.exe");
        if candidate.is_file() {
            return local_path(&candidate).map(Some);
        }
    }
    Ok(None)
}

pub(crate) fn replace_json(path: &Path, before: &[u8], after: &[u8]) -> io::Result<()> {
    let identity = {
        let (guard, current) = FileGuard::read_regular(path)?;
        if current != before {
            return Err(conflict("dependency journal changed during recovery"));
        }
        guard.object_identity()?
    };
    FileGuard::replace_regular(path, &identity, before, after)
}

/// Install one missing shared npm MCP package. Existing installations are
/// preserved; every mutation is journaled and reversible.
fn install(request: &Request, id: &str, version: &str, ops: &mut dyn Ops) -> io::Result<Value> {
    let (package, command, kind) = package_identity(id)?;
    if !stable_version(version) {
        return Err(conflict("A stable explicit dependency version is required"));
    }
    let home = local_path(&request.user_home)?;
    ordinary_ancestors(&request.state)?;
    fs::create_dir_all(&request.state)?;
    build_identity::ordinary(&request.state)?;
    let state = local_path(&request.state)?;
    let modules = home.join("AppData/Roaming/npm/node_modules");
    let installation = modules.join(package);
    if let Some(record) =
        dependency_package::npm(package, &modules, None, Some(command), "npm-global")?
    {
        let preserved = if record["status"] == "adopted" {
            "reused"
        } else {
            "pending"
        };
        return Ok(json!({
            "id": id,
            "state": preserved,
            "reason": "Existing shared installation is preserved.",
            "version": record["version"],
            "packages_acquired": false,
        }));
    }
    let Some(node) = resolve_node(request)? else {
        return Err(conflict(
            "The existing Node.js runtime is required; no duplicate Node is installed",
        ));
    };
    if modules.join(".package-lock.json").is_file() {
        return Err(conflict(
            "Existing shared npm lockfile requires a manager-aware transaction; preserve it",
        ));
    }
    let staged = ops.stage(request, id, version)?;
    let staged_evidence = ops.probe(
        &staged.executable,
        kind,
        &build_identity::hash_file(&staged.executable)?,
    )?;
    if !contained(&staged.candidate, &state.join("dependency-staging")) {
        return Err(conflict("Dependency activation requires owned staging"));
    }
    ordinary_ancestors(&state)?;
    let _lock = InstallationLock::acquire(&state, &installation)?;
    match fs::symlink_metadata(&installation) {
        Ok(_) => {
            return Err(conflict(
                "Shared package appeared before provisioning; repeat discovery",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    ordinary_ancestors(&installation)?;
    if let Some(parent) = installation.parent() {
        ordinary_ancestors(parent)?;
        fs::create_dir_all(parent)?;
    }
    let backup_root = state.join("rollback");
    ordinary_ancestors(&backup_root)?;
    fs::create_dir_all(&backup_root)?;
    build_identity::ordinary(&backup_root)?;
    let backup = {
        let unique = tempfile::Builder::new()
            .prefix(format!("{id}-new-").as_str())
            .tempdir_in(&backup_root)?
            .keep();
        fs::remove_dir(&unique)?;
        unique
    };
    let transactions = state.join("transactions");
    ordinary_ancestors(&transactions)?;
    fs::create_dir_all(&transactions)?;
    build_identity::ordinary(&transactions)?;
    let owner = tempfile::Builder::new()
        .prefix("standalone-")
        .tempdir_in(&transactions)?
        .keep();
    let journal_path = owner.join(format!("{id}.json"));
    let candidate_identity = tree_identity(&staged.candidate, &BTreeMap::new())?;
    let journal = json!({
        "schema_version": 1,
        "owner": "codex-harness-dependencies",
        "kind": "create-directory",
        "phase": "prepared",
        "installation": installation,
        "candidate": staged.candidate,
        "backup": backup,
        "candidate_identity": candidate_identity,
        "transaction_id": owner.file_name().and_then(|name| name.to_str()).unwrap_or("standalone"),
        "component": id,
    });
    let prepared = serde_json::to_vec_pretty(&journal)?;
    StagedFile::create(&journal_path, &prepared)?.commit()?;
    match (|| -> io::Result<()> {
        fs::rename(&staged.candidate, &installation)?;
        let installed_identity = tree_identity(&installation, &BTreeMap::new())?;
        let mut updated = journal.clone();
        updated["phase"] = json!("committed");
        updated["installed_identity"] = json!(installed_identity);
        let bytes = serde_json::to_vec_pretty(&updated)?;
        replace_json(&journal_path, &prepared, &bytes)?;
        Ok(())
    })() {
        Ok(()) => {}
        Err(error) => {
            rollback_committed(&journal_path)?;
            return Err(error);
        }
    };
    let installed = (|| -> io::Result<Value> {
        let Some(record) =
            dependency_package::npm(package, &modules, Some(&node), Some(command), "npm-global")?
        else {
            return Err(conflict(
                "Installed package was not rediscovered with its real native payload",
            ));
        };
        if record["status"] != "adopted" {
            return Err(conflict(
                "Installed package was not rediscovered as adopted",
            ));
        }
        let executable = match id {
            "nuphus" => record["paths"]["original_native_executable"]
                .as_str()
                .map(PathBuf::from),
            _ => record["paths"]["native_executable"]
                .as_str()
                .map(PathBuf::from),
        }
        .ok_or_else(|| conflict("Installed package omitted its native executable"))?;
        let evidence = ops.probe(&executable, kind, &build_identity::hash_file(&executable)?)?;
        write_provenance_marker(&journal_path, &installation, id, version, &executable)?;
        Ok(json!({
            "id": id,
            "state": "installed-unverified",
            "version": version,
            "record": record,
            "sources": staged.sources,
            "evidence": evidence,
            "staged_evidence": staged_evidence,
            "transaction_journal": journal_path,
            "verification": "Native protocol exercised; full language/desktop/browser behavior is separate acceptance.",
        }))
    })();
    match installed {
        Ok(result) => Ok(result),
        Err(error) => {
            rollback_committed(&journal_path)?;
            Err(error)
        }
    }
}

fn rollback_committed(journal_path: &Path) -> io::Result<()> {
    let result = recover_journal(journal_path, true)?;
    if result["state"] != "restored" {
        return Err(conflict(
            "Dependency rollback was refused; installation preserved for repair",
        ));
    }
    Ok(())
}

fn write_provenance_marker(
    journal_path: &Path,
    installation: &Path,
    id: &str,
    version: &str,
    executable: &Path,
) -> io::Result<()> {
    let marker = executable
        .parent()
        .ok_or_else(invalid)?
        .join(".harness-provisioning.json");
    let existing_marker = fs::read(&marker).ok();
    if let Some(existing) = &existing_marker {
        let owner: Value = serde_json::from_slice(existing).unwrap_or(Value::Null);
        if owner["owner"] != "codex-harness-dependencies" {
            return Err(conflict(
                "A foreign dependency provenance marker must not be overwritten",
            ));
        }
    }
    let record = json!({
        "owner": "codex-harness-dependencies",
        "id": id,
        "version": version,
        "digest": Value::Null,
        "executable_sha256": build_identity::hash_file(executable)?,
        "verification": "installation provenance only; operations require real consumer evidence",
    });
    let bytes = serde_json::to_vec_pretty(&record)?;
    ordinary_ancestors(&marker)?;
    match &existing_marker {
        Some(before) => replace_json(&marker, before, &bytes)?,
        None => StagedFile::create(&marker, &bytes)?.commit()?,
    }
    let journal_bytes = fs::read(journal_path)?;
    let mut journal: Value = serde_json::from_slice(&journal_bytes).map_err(|_| invalid())?;
    let relative = marker
        .strip_prefix(installation)
        .map_err(io::Error::other)?
        .to_string_lossy()
        .replace('\\', "/");
    let mut overrides = BTreeMap::new();
    overrides.insert(relative, format!("{:x}", Sha256::digest(&bytes)));
    journal["installed_identity"] = json!(tree_identity(installation, &overrides)?);
    replace_json(
        journal_path,
        &journal_bytes,
        &serde_json::to_vec_pretty(&journal)?,
    )?;
    Ok(())
}

/// Finish or roll back one journaled npm-tree activation. Foreign or damaged
/// instructions never mutate the filesystem; they stay pending for repair.
pub fn recover_journal(path: &Path, rollback_committed: bool) -> io::Result<Value> {
    let bytes = fs::read(path)?;
    let journal: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if journal["owner"] != "codex-harness-dependencies" {
        return Err(conflict("Unsupported or foreign dependency transaction"));
    }
    let pending = |reason: &str| Ok(json!({"state": "pending", "journal": path, "reason": reason}));
    if journal["kind"] != "create-directory" {
        return pending("Unsupported dependency transaction kind; preserve it for repair.");
    }
    if !matches!(
        journal["phase"].as_str(),
        Some("prepared" | "committed" | "restoring" | "restored")
    ) {
        return pending("Dependency transaction phase is absent or unrecognized.");
    }
    let identity_valid =
        |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !journal["candidate_identity"]
        .as_str()
        .is_some_and(identity_valid)
        || journal["installed_identity"]
            .as_str()
            .is_some_and(|value| !identity_valid(value))
    {
        return pending("Dependency recovery identity is missing or invalid.");
    }
    let field = |name: &str| -> Option<PathBuf> {
        journal[name]
            .as_str()
            .map(PathBuf::from)
            .filter(|value| value.is_absolute())
    };
    let (Some(installation), Some(candidate), Some(backup)) =
        (field("installation"), field("candidate"), field("backup"))
    else {
        return pending("Dependency recovery path is missing or relative.");
    };
    for path in [&installation, &candidate, &backup] {
        if fs::symlink_metadata(path)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false)
        {
            return pending("Dependency recovery path is a link; preserve it for repair.");
        }
    }
    if journal["phase"] == "restored" {
        return Ok(json!({"state": "restored", "journal": path}));
    }
    if journal["phase"] == "committed" && !rollback_committed {
        return Ok(json!({"state": "committed", "journal": path}));
    }
    if fs::symlink_metadata(&installation).is_err() {
        let mut updated = journal.clone();
        updated["phase"] = json!("restored");
        replace_json(path, &bytes, &serde_json::to_vec_pretty(&updated)?)?;
        return Ok(json!({
            "state": "restored",
            "journal": path,
            "reason": "Prior absence restored; candidate retained in lifecycle state.",
        }));
    }
    let current = tree_identity(&installation, &BTreeMap::new())?;
    let expected = journal["candidate_identity"].as_str().unwrap_or("");
    let identity_ok = current == expected
        || journal["installed_identity"]
            .as_str()
            .is_some_and(|value| current == value);
    if !identity_ok || backup.exists() {
        return pending("New installation changed or rollback path is occupied; preserve it.");
    }
    if let Some(parent) = backup.parent() {
        ordinary_ancestors(parent)?;
        fs::create_dir_all(parent)?;
    }
    let mut updated = journal.clone();
    updated["phase"] = json!("restoring");
    let restoring = serde_json::to_vec_pretty(&updated)?;
    replace_json(path, &bytes, &restoring)?;
    fs::rename(&installation, &backup)?;
    updated["phase"] = json!("restored");
    replace_json(path, &restoring, &serde_json::to_vec_pretty(&updated)?)?;
    Ok(json!({
        "state": "restored",
        "journal": path,
        "retained_candidate": backup,
    }))
}

/// Explicit provisioning entry point for apply/update.
pub fn provision(request: &Request, id: &str, version: &str) -> io::Result<Value> {
    install(request, id, version, &mut RealOps)
}

/// Recover every interrupted shared-npm activation journal in one state.
///
/// Committed journals stay installed unless `rollback_committed` is explicit;
/// interrupted preparations roll back to prior absence. Foreign or damaged
/// journals are reported and preserved without mutation.
pub fn recover_all(state: &Path, rollback_committed: bool) -> io::Result<Value> {
    let state = local_path(state)?;
    ordinary_ancestors(&state)?;
    let transactions = state.join("transactions");
    let mut journals = Vec::new();
    let owners = match fs::read_dir(&transactions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(json!({
                "schema_version": 1,
                "operation": "recover-npm",
                "journals": journals,
                "model_calls": 0,
            }));
        }
        Err(error) => return Err(error),
    };
    for (index, owner) in owners.enumerate() {
        if index == 4096 {
            return Err(conflict("Dependency transaction scan exceeded its bound"));
        }
        let owner = owner?.path();
        if !owner.is_dir() {
            continue;
        }
        for (count, journal) in fs::read_dir(&owner)?.enumerate() {
            if count == 4096 {
                return Err(conflict("Dependency transaction scan exceeded its bound"));
            }
            let journal = journal?.path();
            if journal
                .extension()
                .is_none_or(|extension| extension != "json")
            {
                continue;
            }
            let kind = fs::read(&journal)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|value| value["kind"].as_str().map(str::to_owned));
            if kind.as_deref() == Some("rustup-component-install") {
                let result =
                    crate::dependency_rust_component::recover_journal(&journal, rollback_committed);
                journals.push(match result {
                    Ok(value) => value,
                    Err(error) => json!({
                        "state": "failed",
                        "journal": journal,
                        "reason": error.to_string(),
                    }),
                });
                continue;
            }
            let result = match recover_journal(&journal, rollback_committed) {
                Ok(value) => value,
                Err(error) => json!({
                    "state": "failed",
                    "journal": journal,
                    "reason": error.to_string(),
                }),
            };
            journals.push(result);
        }
    }
    Ok(json!({
        "schema_version": 1,
        "operation": "recover-npm",
        "rollback_committed": rollback_committed,
        "journals": journals,
        "model_calls": 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeOps {
        fail_post_probe: bool,
        probes: usize,
    }

    impl FakeOps {
        fn new(fail_post_probe: bool) -> Self {
            Self {
                fail_post_probe,
                probes: 0,
            }
        }
    }

    impl Ops for FakeOps {
        fn stage(&mut self, request: &Request, _id: &str, version: &str) -> io::Result<Staged> {
            let staging = request.state.join("dependency-staging");
            fs::create_dir_all(&staging)?;
            let stage = tempfile::Builder::new()
                .prefix("candidate-")
                .tempdir_in(&staging)?
                .keep();
            let candidate = stage.join("package");
            fs::create_dir_all(candidate.join("bin"))?;
            fs::write(
                candidate.join("package.json"),
                format!(
                    r#"{{"name":"codebase-memory-mcp","version":"{version}","bin":{{"codebase-memory-mcp":"bin/tool.js"}}}}"#
                ),
            )?;
            fs::write(candidate.join("bin/tool.js"), b"fixture entry point")?;
            fs::write(
                candidate.join("bin/codebase-memory-mcp.exe"),
                b"owned fixture binary",
            )?;
            Ok(Staged {
                executable: candidate.join("bin/codebase-memory-mcp.exe"),
                candidate,
                sources: vec![json!("fixture://archive")],
            })
        }

        fn probe(
            &mut self,
            _executable: &Path,
            _kind: ProbeKind,
            _expected_sha256: &str,
        ) -> io::Result<Value> {
            self.probes += 1;
            if self.fail_post_probe && self.probes > 1 {
                return Err(conflict("fixture post-activation refusal"));
            }
            Ok(json!({"state": "protocol-ready", "fixture": true}))
        }
    }

    struct Fixture {
        // Keeps the disposable fixture tree alive for the assertions.
        #[allow(dead_code)]
        root: tempfile::TempDir,
        request: Request,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::Builder::new()
                .prefix("harness-npm-install-Юникод-")
                .tempdir()
                .unwrap();
            let node = root.path().join("node.exe");
            fs::write(&node, b"fixture node runtime").unwrap();
            let request = Request {
                manager: std::env::current_exe().unwrap(),
                user_home: root.path().join("user"),
                state: root.path().join("state"),
                node: Some(node),
            };
            fs::create_dir_all(&request.state).unwrap();
            Self { root, request }
        }

        fn modules(&self) -> PathBuf {
            self.request
                .user_home
                .join("AppData/Roaming/npm/node_modules")
        }

        fn installation(&self) -> PathBuf {
            self.modules().join("codebase-memory-mcp")
        }

        fn install(&self, ops: &mut FakeOps) -> io::Result<Value> {
            install(&self.request, "codebase-memory", "1.9.9", ops)
        }

        fn journal(&self) -> PathBuf {
            let mut found = Vec::new();
            let transactions = self.request.state.join("transactions");
            for owner in fs::read_dir(&transactions).unwrap() {
                let owner = owner.unwrap().path();
                if owner.is_dir() {
                    for journal in fs::read_dir(&owner).unwrap() {
                        found.push(journal.unwrap().path());
                    }
                }
            }
            assert_eq!(found.len(), 1, "one journal per installation");
            found.pop().unwrap()
        }
    }

    #[test]
    fn missing_package_installs_with_journal_marker_and_probes() {
        let fixture = Fixture::new();
        let mut ops = FakeOps::new(false);
        let result = fixture.install(&mut ops).unwrap();
        assert_eq!(result["state"], "installed-unverified");
        assert_eq!(result["record"]["status"], "adopted");
        assert_eq!(ops.probes, 2, "staged and installed executables are probed");
        let installation = fixture.installation();
        assert!(installation.join("package.json").is_file());
        let marker = installation.join("bin/.harness-provisioning.json");
        let marker_value: Value = serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
        assert_eq!(marker_value["owner"], "codex-harness-dependencies");
        assert_eq!(marker_value["id"], "codebase-memory");
        let journal: Value = serde_json::from_slice(&fs::read(fixture.journal()).unwrap()).unwrap();
        assert_eq!(journal["phase"], "committed");
        assert_eq!(
            journal["installed_identity"].as_str().unwrap().len(),
            64,
            "marker rewrite is recorded in the installed identity"
        );
        assert!(
            fixture
                .request
                .state
                .join("locks")
                .read_dir()
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn failed_post_activation_rolls_back_to_prior_absence() {
        let fixture = Fixture::new();
        let mut ops = FakeOps::new(true);
        let error = fixture.install(&mut ops).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("fixture post-activation refusal")
        );
        assert!(fs::symlink_metadata(fixture.installation()).is_err());
        let journal: Value = serde_json::from_slice(&fs::read(fixture.journal()).unwrap()).unwrap();
        assert_eq!(journal["phase"], "restored");
        let backup = PathBuf::from(journal["backup"].as_str().unwrap());
        assert!(backup.join("package.json").is_file());
        assert!(
            fixture
                .request
                .state
                .join("locks")
                .read_dir()
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn existing_installations_and_shared_lockfiles_are_preserved() {
        let fixture = Fixture::new();
        let installation = fixture.installation();
        fs::create_dir_all(installation.join("bin")).unwrap();
        fs::write(
            installation.join("package.json"),
            r#"{"name":"codebase-memory-mcp","version":"0.0.1"}"#,
        )
        .unwrap();
        let mut ops = FakeOps::new(false);
        let result = fixture.install(&mut ops).unwrap();
        assert_eq!(result["state"], "pending");
        assert_eq!(result["packages_acquired"], false);
        assert_eq!(ops.probes, 0);
        assert_eq!(
            fs::read_to_string(installation.join("package.json")).unwrap(),
            r#"{"name":"codebase-memory-mcp","version":"0.0.1"}"#
        );
        assert!(!fixture.request.state.join("transactions").exists());

        let lockfile = fixture.modules().join(".package-lock.json");
        fs::create_dir_all(fixture.modules()).unwrap();
        fs::write(&lockfile, b"shared manager lockfile").unwrap();
        fs::remove_dir_all(&installation).unwrap();
        let error = fixture.install(&mut FakeOps::new(false)).unwrap_err();
        assert!(error.to_string().contains("shared npm lockfile"));
        assert_eq!(fs::read(&lockfile).unwrap(), b"shared manager lockfile");
    }

    #[test]
    fn recovery_refuses_foreign_damage_and_honors_commitment() {
        let fixture = Fixture::new();
        fixture.install(&mut FakeOps::new(false)).unwrap();
        let journal_path = fixture.journal();
        let committed = recover_journal(&journal_path, false).unwrap();
        assert_eq!(committed["state"], "committed");
        assert!(fixture.installation().join("package.json").is_file());

        fs::write(
            fixture.installation().join("package.json"),
            r#"{"name":"codebase-memory-mcp","version":"9.9.9"}"#,
        )
        .unwrap();
        let pending = recover_journal(&journal_path, true).unwrap();
        assert_eq!(pending["state"], "pending");
        assert!(fixture.installation().join("package.json").is_file());

        let fresh = Fixture::new();
        fresh.install(&mut FakeOps::new(false)).unwrap();
        let fresh_journal = fresh.journal();
        let restored = recover_journal(&fresh_journal, true).unwrap();
        assert_eq!(restored["state"], "restored");
        assert!(fs::symlink_metadata(fresh.installation()).is_err());

        let mut foreign: Value =
            serde_json::from_slice(&fs::read(&fresh_journal).unwrap()).unwrap();
        foreign["owner"] = json!("foreign-owner");
        fs::write(&fresh_journal, serde_json::to_vec_pretty(&foreign).unwrap()).unwrap();
        assert!(recover_journal(&fresh_journal, true).is_err());
    }

    #[test]
    fn versions_and_identities_are_validated_before_any_mutation() {
        let fixture = Fixture::new();
        let error = install(
            &fixture.request,
            "codebase-memory",
            "1.0.0-beta",
            &mut FakeOps::new(false),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("stable explicit dependency version")
        );
        let error = install(
            &fixture.request,
            "serena",
            "1.9.9",
            &mut FakeOps::new(false),
        )
        .unwrap_err();
        assert!(!fixture.request.state.join("transactions").exists());
        assert!(error.to_string().contains("rejected"));
        assert!(stable_version("1.9.9"));
        assert!(stable_version("2026-09-14"));
        assert!(!stable_version("1.0.0-beta"));
        assert!(!stable_version(""));
    }

    #[test]
    fn foreign_provenance_marker_is_never_overwritten() {
        let fixture = Fixture::new();
        let installation = fixture.installation();
        fs::create_dir_all(installation.join("bin")).unwrap();
        let marker = installation.join("bin/.harness-provisioning.json");
        fs::write(&marker, br#"{"owner":"foreign-tool","note":"preserve me"}"#).unwrap();
        let executable = installation.join("bin/codebase-memory-mcp.exe");
        fs::write(&executable, b"owned fixture binary").unwrap();
        let error = write_provenance_marker(
            &fixture.request.state.join("transactions/unused.json"),
            &installation,
            "codebase-memory",
            "1.9.9",
            &executable,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("foreign dependency provenance marker")
        );
        assert_eq!(
            fs::read(&marker).unwrap(),
            br#"{"owner":"foreign-tool","note":"preserve me"}"#
        );
    }

    #[test]
    fn recover_all_keeps_commitments_and_rolls_back_explicitly() {
        let fixture = Fixture::new();
        fixture.install(&mut FakeOps::new(false)).unwrap();
        let report = recover_all(&fixture.request.state, false).unwrap();
        let journals = report["journals"].as_array().unwrap();
        assert_eq!(journals.len(), 1);
        assert_eq!(journals[0]["state"], "committed");
        assert!(fixture.installation().join("package.json").is_file());
        let report = recover_all(&fixture.request.state, true).unwrap();
        let journals = report["journals"].as_array().unwrap();
        assert_eq!(journals[0]["state"], "restored");
        assert!(fs::symlink_metadata(fixture.installation()).is_err());
    }

    #[test]
    fn recover_all_restores_interrupted_preparation_and_reports_foreign_journals() {
        let fixture = Fixture::new();
        let state = &fixture.request.state;
        let installation = fixture.installation();
        fs::create_dir_all(installation.join("bin")).unwrap();
        fs::write(
            installation.join("package.json"),
            r#"{"name":"codebase-memory-mcp","version":"1.9.9"}"#,
        )
        .unwrap();
        let owner = state.join("transactions/standalone-interrupted");
        fs::create_dir_all(&owner).unwrap();
        let journal = json!({
            "schema_version": 1,
            "owner": "codex-harness-dependencies",
            "kind": "create-directory",
            "phase": "prepared",
            "installation": installation,
            "candidate": state.join("staging/candidate-interrupted/package"),
            "backup": state.join("rollback/codebase-memory-new-interrupted"),
            "candidate_identity": tree_identity(&installation, &BTreeMap::new()).unwrap(),
            "transaction_id": "standalone-interrupted",
            "component": "codebase-memory",
        });
        fs::write(
            owner.join("codebase-memory.json"),
            serde_json::to_vec_pretty(&journal).unwrap(),
        )
        .unwrap();
        let foreign = state.join("transactions/foreign-owner");
        fs::create_dir_all(&foreign).unwrap();
        fs::write(
            foreign.join("foreign.json"),
            br#"{"owner":"foreign-tool","kind":"create-directory","phase":"prepared"}"#,
        )
        .unwrap();
        let report = recover_all(state, false).unwrap();
        let journals = report["journals"].as_array().unwrap();
        assert_eq!(journals.len(), 2);
        let restored = journals
            .iter()
            .find(|entry| entry["state"] == "restored")
            .expect("interrupted preparation is restored");
        let backup = PathBuf::from(restored["retained_candidate"].as_str().unwrap());
        assert!(backup.join("package.json").is_file());
        assert!(fs::symlink_metadata(&installation).is_err());
        let failed = journals
            .iter()
            .find(|entry| entry["state"] == "failed")
            .expect("foreign journal is reported");
        assert!(
            failed["journal"]
                .as_str()
                .unwrap()
                .ends_with("foreign.json")
        );
        assert_eq!(
            fs::read(foreign.join("foreign.json")).unwrap(),
            br#"{"owner":"foreign-tool","kind":"create-directory","phase":"prepared"}"#
        );
    }
}
