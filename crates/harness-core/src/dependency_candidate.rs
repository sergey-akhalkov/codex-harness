//! Bounded inspection and runtime validation of an owned staged candidate.
//!
//! The expected manifest SHA-256 is an explicit trusted-staging input. This
//! module never writes an active pointer, never installs a companion, and never
//! treats source manifests as authentication of a caller-forged stage.
#![cfg(windows)]

use crate::{
    dependency_audit, dependency_discovery, dependency_mcp_probe, native_build,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason},
    registration_native::ReadGuard,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Pipes::CreatePipe;

const MAX_MANIFEST: u64 = 8 * 1024 * 1024;
const MAX_REQUEST: u64 = 64 * 1024;
const MAX_FILE: u64 = 512 * 1024 * 1024;
const MAX_PAYLOAD: u64 = 512 * 1024 * 1024;
const MAX_OUTPUT: u64 = 64 * 1024;
const MAX_TREE_ENTRIES: usize = 16_384;
const MAX_DIRECTORIES: usize = 16_384;
const VERSION_TIMEOUT: Duration = Duration::from_secs(20);
const VERSION_CLEANUP: Duration = Duration::from_secs(5);

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "dependency candidate is invalid or incompatible",
    )
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn lowercase_digest(value: &str) -> io::Result<String> {
    if !digest(value) {
        return Err(invalid());
    }
    Ok(value.to_ascii_lowercase())
}

fn anonymous_pipe() -> io::Result<(File, File)> {
    let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 64 * 1024) } == 0 {
        return Err(invalid());
    }
    Ok(unsafe { (File::from_raw_handle(read), File::from_raw_handle(write)) })
}

fn drain_bounded(mut input: File, stop: &Cancellation) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        match input.read(&mut buffer) {
            Ok(0) => return Ok(bytes),
            Ok(count) => {
                if bytes.len() as u64 + count as u64 > MAX_OUTPUT {
                    stop.cancel();
                    return Err(invalid());
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            Err(_) => {
                stop.cancel();
                return Err(invalid());
            }
        }
    }
}

fn parse_basedpyright_version(stdout: &[u8], version: &str) -> io::Result<()> {
    let text = std::str::from_utf8(stdout).map_err(|_| invalid())?;
    let mut lines = text.lines();
    let first = lines.next().ok_or_else(invalid)?;
    if first != format!("basedpyright {version}") {
        return Err(invalid());
    }
    match lines.next() {
        Some(second) if second.starts_with("based on pyright ") && lines.next().is_none() => Ok(()),
        None => Ok(()),
        _ => Err(invalid()),
    }
}

fn relative_name(path: &str) -> io::Result<String> {
    if path.is_empty()
        || path.len() > 4096
        || path.contains('\\')
        || path.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with('.')
            || component.ends_with(char::is_whitespace)
            || component.contains([':', '*', '?', '"', '<', '>', '|'])
        {
            return Err(invalid());
        }
        let base = component.split('.').next().unwrap().to_ascii_uppercase();
        if matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            base.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        }) {
            return Err(invalid());
        }
    }
    Ok(path.to_owned())
}

fn lookalike(path: &str) -> String {
    path.to_uppercase().to_lowercase()
}

fn pin_directory(path: &Path) -> io::Result<File> {
    let handle = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let metadata = handle.metadata()?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !metadata.is_dir() {
        return Err(invalid());
    }
    Ok(handle)
}

fn read_guarded(path: &Path, limit: u64) -> io::Result<(ReadGuard, Vec<u8>)> {
    let mut guard = ReadGuard::open(&path.components().collect::<PathBuf>())?;
    let mut bytes = Vec::new();
    (&mut guard.file).take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok((guard, bytes))
}

fn hash_guard(guard: &mut ReadGuard, expected_size: u64) -> io::Result<String> {
    let mut count = 0;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = guard.file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count += read as u64;
        if count > MAX_FILE || count > expected_size {
            return Err(invalid());
        }
        hash.update(&buffer[..read]);
    }
    if count != expected_size {
        return Err(invalid());
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn hash_file(path: &Path, expected_size: Option<u64>) -> io::Result<(ReadGuard, u64, String)> {
    let mut guard = ReadGuard::open(&path.components().collect::<PathBuf>())?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(guard.file.as_raw_handle(), &mut info) } == 0
        || info.nNumberOfLinks != 1
    {
        return Err(invalid());
    }
    let size = guard.file.metadata()?.len();
    if size > MAX_FILE || expected_size.is_some_and(|expected| expected != size) {
        return Err(invalid());
    }
    let digest = hash_guard(&mut guard, size)?;
    guard.file.seek(SeekFrom::Start(0))?;
    Ok((guard, size, digest))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: u32,
    state: PathBuf,
    package: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct ManifestContents {
    files: Vec<ManifestFile>,
    #[serde(default)]
    directories: Vec<String>,
    file_count: u64,
    payload_bytes: u64,
}

#[derive(Deserialize)]
struct ManifestEnvelope {
    report: Map<String, Value>,
    contents: ManifestContents,
}

struct ObservedFile {
    path: String,
    size: u64,
    sha256: String,
    guard: ReadGuard,
}

/// Complete identity and integrity observation. No package process is started.
pub struct InspectedCandidate {
    report: Value,
    #[allow(dead_code)]
    files: Vec<ObservedFile>,
    #[allow(dead_code)]
    request: ReadGuard,
    #[allow(dead_code)]
    manifest: ReadGuard,
    #[allow(dead_code)]
    package_dir: File,
    #[allow(dead_code)]
    stage_dir: File,
    #[allow(dead_code)]
    node: Option<ReadGuard>,
    node_path: Option<PathBuf>,
}

impl InspectedCandidate {
    pub(crate) fn report(&self) -> &Value {
        &self.report
    }

    pub(crate) fn package(&self) -> &str {
        self.report["package"].as_str().unwrap_or_default()
    }

    pub(crate) fn version(&self) -> &str {
        self.report["version"].as_str().unwrap_or_default()
    }
}

/// Runtime-validated candidate. Construction is only through [`validate`].
pub struct ValidatedCandidate {
    report: Value,
    #[allow(dead_code)]
    inspected: InspectedCandidate,
}

impl ValidatedCandidate {
    pub fn report(&self) -> &Value {
        &self.report
    }

    pub fn package(&self) -> &str {
        self.report["package"].as_str().unwrap_or_default()
    }

    pub fn version(&self) -> &str {
        self.report["version"].as_str().unwrap_or_default()
    }
}

fn isolate_environment(command: &mut CommandSpec, root: &Path) -> io::Result<()> {
    for (key, _) in std::env::vars_os() {
        let name = key
            .to_str()
            .filter(|s| s.is_ascii() && !s.contains(['=', '\0']))
            .ok_or_else(invalid)?;
        command.env.insert(name.into(), None);
    }
    for name in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env.insert(name.into(), Some(value));
        }
    }
    for (variable, folder) in [
        ("HOME", "home"),
        ("USERPROFILE", "home"),
        ("APPDATA", "roaming"),
        ("LOCALAPPDATA", "local"),
        ("TEMP", "temp"),
        ("TMP", "temp"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("CODEX_HOME", "codex"),
        ("CBM_CACHE_DIR", "cbm"),
        ("CBM_RUNTIME_DIR", "ipc"),
        ("NUPHUS_MODELS_DIR", "models"),
        ("NPM_CONFIG_CACHE", "npm-cache"),
        ("npm_config_cache", "npm-cache"),
    ] {
        let path = root.join(folder);
        fs::create_dir_all(&path)?;
        command
            .env
            .insert(variable.into(), Some(path.into_os_string()));
    }
    command
        .env
        .insert("NUPHUS_MCP_NO_MODEL_DOWNLOAD".into(), Some("1".into()));
    Ok(())
}

fn bin_entry(manifest: &Value, command: &str) -> io::Result<String> {
    let path = match &manifest["bin"] {
        Value::String(bin) if command == manifest["name"].as_str().unwrap_or_default() => {
            bin.as_str()
        }
        Value::Object(bins) => bins
            .get(command)
            .and_then(Value::as_str)
            .ok_or_else(invalid)?,
        _ => return Err(invalid()),
    };
    relative_name(path.trim_start_matches("./"))
}

fn observe_tree(
    package: &Path,
    expected: &[ManifestFile],
    declared_directories: &[String],
) -> io::Result<Vec<ObservedFile>> {
    let expected_map: BTreeMap<String, &ManifestFile> = expected
        .iter()
        .map(|file| Ok((relative_name(&file.path)?, file)))
        .collect::<io::Result<_>>()?;
    if expected_map.len() != expected.len() {
        return Err(invalid());
    }
    let mut keys = BTreeSet::new();
    for path in expected_map.keys() {
        if !keys.insert(lookalike(path)) {
            return Err(invalid());
        }
    }
    let mut allowed_dirs = BTreeSet::new();
    allowed_dirs.insert(String::new());
    for path in expected_map.keys() {
        let mut prefix = String::new();
        for component in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if prefix != *path {
                allowed_dirs.insert(prefix.clone());
            }
        }
    }
    for directory in declared_directories {
        if directory.is_empty() {
            continue;
        }
        allowed_dirs.insert(relative_name(directory)?);
        let mut prefix = String::new();
        for component in directory.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            allowed_dirs.insert(prefix.clone());
        }
    }
    let mut observed = Vec::new();
    let mut seen = BTreeSet::new();
    let mut seen_dirs = BTreeSet::new();
    let mut entries = 0usize;
    let mut stack = vec![package.to_path_buf()];
    while let Some(directory) = stack.pop() {
        native_build::ordinary_ancestors(&directory)?;
        let _pin = pin_directory(&directory)?;
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            entries += 1;
            if entries > MAX_TREE_ENTRIES {
                return Err(invalid());
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(invalid());
            }
            if metadata.is_dir() {
                let relative = path
                    .strip_prefix(package)
                    .map_err(|_| invalid())?
                    .to_str()
                    .ok_or_else(invalid)?
                    .replace('\\', "/");
                let relative = relative_name(&relative)?;
                if !seen_dirs.insert(lookalike(&relative))
                    || !allowed_dirs.contains(&relative)
                    || seen_dirs.len() > MAX_DIRECTORIES
                {
                    return Err(invalid());
                }
                stack.push(path);
                continue;
            }
            if !metadata.is_file() {
                return Err(invalid());
            }
            let relative = path
                .strip_prefix(package)
                .map_err(|_| invalid())?
                .to_str()
                .ok_or_else(invalid)?
                .replace('\\', "/");
            let relative = relative_name(&relative)?;
            if !seen.insert(lookalike(&relative)) {
                return Err(invalid());
            }
            let Some(expected) = expected_map.get(&relative) else {
                return Err(invalid());
            };
            if expected.size != metadata.len() {
                return Err(invalid());
            }
            let (guard, size, sha256) = hash_file(&path, Some(expected.size))?;
            if sha256 != lowercase_digest(&expected.sha256)? {
                return Err(invalid());
            }
            observed.push(ObservedFile {
                path: relative,
                size,
                sha256,
                guard,
            });
        }
    }
    if observed.len() != expected.len() {
        return Err(invalid());
    }
    observed.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(observed)
}

/// Read-only complete integrity inspection. No packages, hooks or probes run.
pub(crate) fn inspect(
    stage: &Path,
    expected_manifest_sha256: &str,
    node: Option<(&Path, &str)>,
) -> io::Result<InspectedCandidate> {
    let expected_manifest_sha256 = lowercase_digest(expected_manifest_sha256)?;
    let stage = dependency_discovery::local_path(stage)?;
    native_build::ordinary_ancestors(&stage)?;
    let stage_name = stage
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(invalid)?;
    if !stage_name.starts_with("candidate-") {
        return Err(invalid());
    }
    let staging = stage.parent().ok_or_else(invalid)?;
    if staging.file_name().and_then(|name| name.to_str()) != Some("dependency-staging") {
        return Err(invalid());
    }
    let state = staging.parent().ok_or_else(invalid)?;
    native_build::verify_owned_state(state)?;
    let stage_dir = pin_directory(&stage)?;
    let package_dir = pin_directory(&stage.join("package"))?;

    let (request_guard, request_bytes) = read_guarded(&stage.join("request.json"), MAX_REQUEST)?;
    let request: Request = serde_json::from_slice(&request_bytes).map_err(|_| invalid())?;
    if request.schema != 1 {
        return Err(invalid());
    }
    dependency_audit::endpoints(&request.package, &request.version)?;
    let request_state = dependency_discovery::local_path(&request.state)?;
    if request_state != state {
        return Err(invalid());
    }

    let (manifest_guard, manifest_bytes) =
        read_guarded(&stage.join("manifest.json"), MAX_MANIFEST)?;
    let actual_manifest = format!("{:x}", Sha256::digest(&manifest_bytes));
    if actual_manifest != expected_manifest_sha256 {
        return Err(invalid());
    }
    let envelope: ManifestEnvelope =
        serde_json::from_slice(&manifest_bytes).map_err(|_| invalid())?;
    let report = envelope.report;
    if report.get("schema_version") != Some(&json!(1))
        || report.get("operation") != Some(&json!("npm-candidate-preparation"))
        || report.get("status") != Some(&json!("staged-unverified"))
        || report.get("activation_allowed") != Some(&json!(false))
        || report.get("package") != Some(&json!(request.package))
        || report.get("version") != Some(&json!(request.version))
        || report.get("request_sha256")
            != Some(&json!(format!("{:x}", Sha256::digest(&request_bytes))))
        || report.get("file_count") != Some(&json!(envelope.contents.file_count))
        || report.get("payload_bytes") != Some(&json!(envelope.contents.payload_bytes))
    {
        return Err(invalid());
    }
    let reported_stage = report
        .get("stage")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let reported_candidate = report
        .get("candidate")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    if dependency_discovery::local_path(Path::new(reported_stage))? != stage
        || dependency_discovery::local_path(Path::new(reported_candidate))? != stage.join("package")
    {
        return Err(invalid());
    }
    if envelope.contents.files.len() as u64 != envelope.contents.file_count {
        return Err(invalid());
    }
    let payload: u64 = envelope
        .contents
        .files
        .iter()
        .map(|file| file.size)
        .try_fold(0u64, |sum, size| sum.checked_add(size))
        .ok_or_else(invalid)?;
    if payload != envelope.contents.payload_bytes || payload > MAX_PAYLOAD {
        return Err(invalid());
    }
    let files = observe_tree(
        &stage.join("package"),
        &envelope.contents.files,
        &envelope.contents.directories,
    )?;
    let package_json = files
        .iter()
        .find(|file| file.path == "package.json")
        .ok_or_else(invalid)?;
    let mut identity_bytes = Vec::new();
    package_json
        .guard
        .file
        .try_clone()?
        .take(package_json.size + 1)
        .read_to_end(&mut identity_bytes)?;
    if identity_bytes.len() as u64 != package_json.size {
        return Err(invalid());
    }
    if dependency_audit::identity(&identity_bytes)?
        != (request.package.clone(), request.version.clone())
    {
        return Err(invalid());
    }

    let mut node_guard = None;
    let mut node_digest = None;
    let mut node_path = None;
    if let Some((path, expected)) = node {
        if request.package != "basedpyright" {
            return Err(invalid());
        }
        let expected = lowercase_digest(expected)?;
        let path = dependency_discovery::local_path(path)?;
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        {
            return Err(invalid());
        }
        let (guard, _, sha256) = hash_file(&path, None)?;
        if sha256 != expected {
            return Err(invalid());
        }
        node_digest = Some(sha256);
        node_path = Some(path);
        node_guard = Some(guard);
    } else if request.package == "basedpyright" {
        return Err(invalid());
    }

    let entry = match request.package.as_str() {
        "codebase-memory-mcp" => {
            let file = files
                .iter()
                .find(|file| file.path == "bin/codebase-memory-mcp.exe")
                .ok_or_else(invalid)?;
            json!({
                "kind": "native-executable",
                "path": file.path,
                "sha256": file.sha256,
                "size": file.size
            })
        }
        "@nuphus/nuphus-mcp-win32-x64" => {
            let executable = files
                .iter()
                .find(|file| file.path == "bin/nuphus-mcp.exe")
                .ok_or_else(invalid)?;
            let companions: Vec<_> = files
                .iter()
                .filter(|file| file.path.ends_with(".dll"))
                .map(|file| json!({"path": file.path, "sha256": file.sha256, "size": file.size}))
                .collect();
            if companions.len() != 2 {
                return Err(invalid());
            }
            json!({
                "kind": "native-executable",
                "path": executable.path,
                "sha256": executable.sha256,
                "size": executable.size,
                "companions": companions
            })
        }
        "basedpyright" => {
            let identity: Value = serde_json::from_slice(&identity_bytes).map_err(|_| invalid())?;
            let script = bin_entry(&identity, "basedpyright")?;
            let file = files
                .iter()
                .find(|file| file.path == script)
                .ok_or_else(invalid)?;
            json!({
                "kind": "node-cli",
                "path": file.path,
                "sha256": file.sha256,
                "size": file.size,
                "node_sha256": node_digest.ok_or_else(invalid)?
            })
        }
        _ => return Err(invalid()),
    };

    Ok(InspectedCandidate {
        report: json!({
            "schema_version": 1,
            "operation": "npm-candidate-inspection",
            "status": "integrity-verified",
            "package": request.package,
            "version": request.version,
            "stage": stage,
            "candidate": stage.join("package"),
            "manifest_sha256": expected_manifest_sha256,
            "file_count": files.len() as u64,
            "payload_bytes": payload,
            "entrypoint": entry,
            "activation_allowed": false,
            "runtime_compatibility": "not-probed",
            "package_code_executed": false,
            "model_calls": 0
        }),
        files,
        request: request_guard,
        manifest: manifest_guard,
        package_dir,
        stage_dir,
        node: node_guard,
        node_path,
    })
}

fn probe_mcp(inspected: &InspectedCandidate) -> io::Result<Value> {
    let path = inspected.report["entrypoint"]["path"]
        .as_str()
        .ok_or_else(invalid)?;
    let digest = inspected.report["entrypoint"]["sha256"]
        .as_str()
        .ok_or_else(invalid)?;
    let executable = inspected.report["candidate"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(invalid)?
        .join(path);
    let kind = match inspected.package() {
        "codebase-memory-mcp" => dependency_mcp_probe::ProbeKind::CodebaseMemory,
        "@nuphus/nuphus-mcp-win32-x64" => dependency_mcp_probe::ProbeKind::Nuphus,
        _ => return Err(invalid()),
    };
    dependency_mcp_probe::probe(&executable, kind, digest)
}

fn probe_basedpyright(inspected: &InspectedCandidate) -> io::Result<Value> {
    let _node = inspected.node.as_ref().ok_or_else(invalid)?;
    let script = inspected.report["entrypoint"]["path"]
        .as_str()
        .ok_or_else(invalid)?;
    let candidate = PathBuf::from(inspected.report["candidate"].as_str().ok_or_else(invalid)?);
    let script_path = candidate.join(script);
    let node_exe = inspected.node_path.as_ref().ok_or_else(invalid)?;
    let private = tempfile::Builder::new()
        .prefix("harness-dependency-candidate-")
        .tempdir()?;
    let mut command = CommandSpec::new(node_exe);
    isolate_environment(&mut command, private.path())?;
    command.current_dir = Some(private.path().to_owned());
    command.args = vec![script_path.into_os_string(), "--version".into()];
    let (stdout_read, stdout_write) = anonymous_pipe()?;
    let (stderr_read, stderr_write) = anonymous_pipe()?;
    command.stdout = Some(stdout_write);
    command.stderr = Some(stderr_write);
    let job = Job::new(Limits {
        memory_bytes: Some(512 * 1024 * 1024),
        cpu_percent: Some(25.0),
    })?;
    let child = job.spawn(&command)?;
    let stop = Cancellation::default();
    let stdout_stop = stop.clone();
    let stderr_stop = stop.clone();
    let stdout_worker = std::thread::Builder::new()
        .name("dependency-candidate-stdout".into())
        .spawn(move || drain_bounded(stdout_read, &stdout_stop))
        .map_err(|_| invalid())?;
    let stderr_worker = std::thread::Builder::new()
        .name("dependency-candidate-stderr".into())
        .spawn(move || drain_bounded(stderr_read, &stderr_stop))
        .map_err(|_| invalid())?;
    let outcome = job.wait(
        &child,
        Deadline::after(VERSION_TIMEOUT)?,
        &stop,
        VERSION_CLEANUP,
    );
    drop(command);
    let stdout = stdout_worker.join().map_err(|_| invalid())?;
    let stderr = stderr_worker.join().map_err(|_| invalid())?;
    let leftover = private.keep();
    let cleanup = fs::remove_dir_all(&leftover).or_else(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(io::Error::other(
                "dependency candidate temporary cleanup failed",
            ))
        }
    });
    let result = (|| {
        let outcome = outcome?;
        if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
            return Err(invalid());
        }
        let stdout = stdout?;
        let stderr = stderr?;
        if !stderr.is_empty() {
            return Err(invalid());
        }
        parse_basedpyright_version(&stdout, inspected.version())?;
        Ok(json!({
            "state": "cli-ready",
            "checked_operation": "--version",
            "stdout_bytes": stdout.len() as u64,
            "job_memory_bytes": 512 * 1024 * 1024u64,
            "owned_tree_stopped": outcome.job.active_processes == 0
        }))
    })();
    match (result, cleanup) {
        (Err(primary), Err(cleanup)) => Err(io::Error::new(
            primary.kind(),
            format!("{primary}; {cleanup}"),
        )),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Ok(summary), Ok(())) => Ok(summary),
    }
}

/// Complete integrity inspection plus the package's bounded runtime probe.
pub fn validate(
    stage: &Path,
    expected_manifest_sha256: &str,
    node: Option<(&Path, &str)>,
) -> io::Result<ValidatedCandidate> {
    let inspected = inspect(stage, expected_manifest_sha256, node)?;
    let runtime = match inspected.package() {
        "codebase-memory-mcp" | "@nuphus/nuphus-mcp-win32-x64" => probe_mcp(&inspected)?,
        "basedpyright" => probe_basedpyright(&inspected)?,
        _ => return Err(invalid()),
    };
    let mut report = inspected.report.clone();
    report["operation"] = json!("npm-candidate-validation");
    report["status"] = json!("runtime-verified");
    report["runtime_compatibility"] = json!("probed");
    report["package_code_executed"] = json!(true);
    report["runtime"] = runtime;
    Ok(ValidatedCandidate { report, inspected })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn owned_state() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        native_build::owner_root(&state).unwrap();
        (root, state)
    }

    pub(crate) fn write_stage(
        state: &Path,
        package: &str,
        version: &str,
        files: &[(&str, &[u8])],
        extra_report: Value,
    ) -> (PathBuf, String) {
        let staging = state.join("dependency-staging");
        fs::create_dir_all(&staging).unwrap();
        let stage = tempfile::Builder::new()
            .prefix("candidate-")
            .tempdir_in(&staging)
            .unwrap();
        let stage_path = stage.path().to_path_buf();
        let request = json!({
            "schema": 1,
            "state": state,
            "package": package,
            "version": version
        });
        let request_bytes = serde_json::to_vec(&request).unwrap();
        fs::write(stage_path.join("request.json"), &request_bytes).unwrap();
        let package_root = stage_path.join("package");
        fs::create_dir(&package_root).unwrap();
        let mut listing = Vec::new();
        let mut payload = 0u64;
        for (path, bytes) in files {
            let target = package_root.join(path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&target, bytes).unwrap();
            listing.push(json!({
                "path": path,
                "size": bytes.len() as u64,
                "sha256": format!("{:x}", Sha256::digest(bytes))
            }));
            payload += bytes.len() as u64;
        }
        let mut report = json!({
            "schema_version": 1,
            "operation": "npm-candidate-preparation",
            "status": "staged-unverified",
            "package": package,
            "version": version,
            "stage": stage_path,
            "candidate": package_root,
            "request_sha256": format!("{:x}", Sha256::digest(&request_bytes)),
            "file_count": listing.len() as u64,
            "payload_bytes": payload,
            "activation_allowed": false,
            "runtime_compatibility": "not-probed",
            "package_code_executed": false,
            "model_calls": 0
        });
        if let Value::Object(extra) = extra_report {
            for (key, value) in extra {
                report[key] = value;
            }
        }
        let manifest = serde_json::to_vec(&json!({"report": report, "contents": {
            "files": listing,
            "directories": [],
            "file_count": listing.len() as u64,
            "payload_bytes": payload
        }}))
        .unwrap();
        fs::write(stage_path.join("manifest.json"), &manifest).unwrap();
        let digest = format!("{:x}", Sha256::digest(&manifest));
        let _ = stage.keep();
        (stage_path, digest)
    }

    fn compile_node(root: &Path, source: &str, output: &Path) {
        fs::write(root.join("node.rs"), source).unwrap();
        let rustc = std::process::Command::new("where.exe")
            .arg("rustc.exe")
            .output()
            .unwrap();
        let rustc = String::from_utf8(rustc.stdout).unwrap();
        let rustc = PathBuf::from(rustc.lines().next().unwrap());
        let mut command = CommandSpec::new(rustc);
        command.args = vec![
            root.join("node.rs").into_os_string(),
            "--edition=2024".into(),
            "-o".into(),
            output.as_os_str().to_owned(),
        ];
        let log = File::create(root.join("compile.log")).unwrap();
        command.stdout = Some(log.try_clone().unwrap());
        command.stderr = Some(log);
        let job = Job::new(Limits {
            memory_bytes: Some(512 * 1024 * 1024),
            cpu_percent: Some(50.0),
        })
        .unwrap();
        let child = job.spawn(&command).unwrap();
        let outcome = job
            .wait(
                &child,
                Deadline::after(Duration::from_secs(30)).unwrap(),
                &Cancellation::default(),
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!((outcome.reason, outcome.exit_code), (StopReason::Exited, 0));
    }

    fn package_json(name: &str, version: &str, bin: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"name": name, "version": version, "bin": bin})).unwrap()
    }

    #[test]
    fn inspect_rejects_digest_mismatch_and_extra_files_without_execution() {
        let (_root, state) = owned_state();
        let manifest = package_json(
            "codebase-memory-mcp",
            "0.10.8",
            json!({"codebase-memory-mcp": "./bin.js"}),
        );
        let (stage, digest) = write_stage(
            &state,
            "codebase-memory-mcp",
            "0.10.8",
            &[
                ("package.json", manifest.as_slice()),
                ("bin.js", b"inert"),
                ("bin/codebase-memory-mcp.exe", b"not-a-real-exe-but-hashed"),
            ],
            json!({}),
        );
        assert!(inspect(&stage, "0".repeat(64).as_str(), None).is_err());
        fs::write(stage.join("package/extra.txt"), b"unexpected").unwrap();
        match inspect(&stage, &digest, None) {
            Err(error) => assert!(!error.to_string().to_ascii_lowercase().contains("probe")),
            Ok(_) => panic!("extra files were accepted"),
        }
    }

    #[test]
    fn unsupported_wrapper_and_missing_node_are_incompatible() {
        let (_root, state) = owned_state();
        let wrapper = package_json(
            "@nuphus/nuphus-mcp",
            "0.2.2",
            json!({"nuphus-mcp": "index.js"}),
        );
        let (stage, digest) = write_stage(
            &state,
            "@nuphus/nuphus-mcp",
            "0.2.2",
            &[("package.json", wrapper.as_slice()), ("index.js", b"shim")],
            json!({}),
        );
        assert!(inspect(&stage, &digest, None).is_err());
        let based = package_json(
            "basedpyright",
            "1.39.10",
            json!({"basedpyright": "index.js"}),
        );
        let (stage, digest) = write_stage(
            &state,
            "basedpyright",
            "1.39.10",
            &[("package.json", based.as_slice()), ("index.js", b"cli")],
            json!({}),
        );
        assert!(inspect(&stage, &digest, None).is_err());
    }

    #[test]
    fn inspect_holds_integrity_for_native_companion_layout() {
        let (_root, state) = owned_state();
        let manifest = package_json(
            "@nuphus/nuphus-mcp-win32-x64",
            "0.2.2",
            json!({"nuphus-mcp": "bin/nuphus-mcp.exe"}),
        );
        let (stage, digest) = write_stage(
            &state,
            "@nuphus/nuphus-mcp-win32-x64",
            "0.2.2",
            &[
                ("package.json", manifest.as_slice()),
                ("bin/nuphus-mcp.exe", b"native-bytes"),
                ("bin/onnxruntime.dll", b"dll-a"),
                ("bin/onnxruntime_providers_shared.dll", b"dll-b"),
            ],
            json!({}),
        );
        let inspected = inspect(&stage, &digest, None).unwrap();
        assert_eq!(inspected.package(), "@nuphus/nuphus-mcp-win32-x64");
        assert_eq!(inspected.version(), "0.2.2");
        assert_eq!(inspected.report()["runtime_compatibility"], "not-probed");
        assert_eq!(inspected.report()["package_code_executed"], false);
        assert_eq!(inspected.report()["activation_allowed"], false);
        assert_eq!(inspected.report()["file_count"], 4);
        assert_eq!(
            inspected.report()["entrypoint"]["path"],
            "bin/nuphus-mcp.exe"
        );
    }

    #[test]
    fn basedpyright_fake_node_version_probe_stays_private() {
        let (root, state) = owned_state();
        let based = package_json(
            "basedpyright",
            "1.39.10",
            json!({"basedpyright": "index.js"}),
        );
        let (stage, digest) = write_stage(
            &state,
            "basedpyright",
            "1.39.10",
            &[
                ("package.json", based.as_slice()),
                ("index.js", b"require('./dist/pyright');\n"),
            ],
            json!({}),
        );
        let node = root.path().join("node.exe");
        compile_node(
            root.path(),
            include_str!("../tests/fixtures/fake_node.rs"),
            &node,
        );
        let node_sha = format!("{:x}", Sha256::digest(fs::read(&node).unwrap()));
        let inspected = inspect(&stage, &digest, Some((&node, &node_sha))).unwrap();
        assert_eq!(inspected.package(), "basedpyright");
        assert_eq!(inspected.report()["package_code_executed"], false);
        let validated = validate(&stage, &digest, Some((&node, &node_sha))).unwrap();
        assert_eq!(validated.package(), "basedpyright");
        assert_eq!(validated.version(), "1.39.10");
        assert_eq!(validated.report()["status"], "runtime-verified");
        assert_eq!(
            validated.report()["runtime"]["checked_operation"],
            "--version"
        );
        assert_eq!(validated.report()["activation_allowed"], false);
        assert_eq!(validated.report()["manifest_sha256"], digest);
    }
    #[test]
    fn inspect_rejects_extra_empty_directories() {
        let (_root, state) = owned_state();
        let manifest = package_json(
            "@nuphus/nuphus-mcp-win32-x64",
            "0.2.2",
            json!({"nuphus-mcp": "bin/nuphus-mcp.exe"}),
        );
        let (stage, digest) = write_stage(
            &state,
            "@nuphus/nuphus-mcp-win32-x64",
            "0.2.2",
            &[
                ("package.json", manifest.as_slice()),
                ("bin/nuphus-mcp.exe", b"native-bytes"),
                ("bin/onnxruntime.dll", b"dll-a"),
                ("bin/onnxruntime_providers_shared.dll", b"dll-b"),
            ],
            json!({}),
        );
        fs::create_dir(stage.join("package/unexpected")).unwrap();
        assert!(inspect(&stage, &digest, None).is_err());
    }

    #[test]
    fn inspect_rejects_hardlinked_package_files() {
        let (_root, state) = owned_state();
        let manifest = package_json(
            "@nuphus/nuphus-mcp-win32-x64",
            "0.2.2",
            json!({"nuphus-mcp": "bin/nuphus-mcp.exe"}),
        );
        let (stage, digest) = write_stage(
            &state,
            "@nuphus/nuphus-mcp-win32-x64",
            "0.2.2",
            &[
                ("package.json", manifest.as_slice()),
                ("bin/nuphus-mcp.exe", b"native-bytes"),
                ("bin/onnxruntime.dll", b"dll-a"),
                ("bin/onnxruntime_providers_shared.dll", b"dll-b"),
            ],
            json!({}),
        );
        let original = stage.join("package/bin/onnxruntime.dll");
        let alias = stage.join("package/bin/alias.dll");
        fs::hard_link(&original, &alias).unwrap();
        assert!(inspect(&stage, &digest, None).is_err());
        assert_eq!(fs::read(&original).unwrap(), b"dll-a");
        assert_eq!(fs::read(&alias).unwrap(), b"dll-a");
    }

    #[test]
    fn basedpyright_fake_node_rejects_wrong_version_failure_and_flood() {
        let (root, state) = owned_state();
        let based = package_json(
            "basedpyright",
            "1.39.10",
            json!({"basedpyright": "index.js"}),
        );
        let (stage, digest) = write_stage(
            &state,
            "basedpyright",
            "1.39.10",
            &[
                ("package.json", based.as_slice()),
                ("index.js", b"require('./dist/pyright');\n"),
            ],
            json!({}),
        );
        let node = root.path().join("node.exe");
        compile_node(
            root.path(),
            include_str!("../tests/fixtures/fake_node.rs"),
            &node,
        );
        let node_sha = format!("{:x}", Sha256::digest(fs::read(&node).unwrap()));
        for mode in ["wrong-version", "fail", "flood"] {
            fs::write(root.path().join("fake-node-mode.txt"), mode).unwrap();
            assert!(
                validate(&stage, &digest, Some((&node, &node_sha))).is_err(),
                "{mode} accepted"
            );
        }
    }
    #[test]
    #[ignore = "explicit official staged package only; requires caller-audited stage, digest and Node"]
    fn official_staged_candidate_runtime_is_explicit_only() {
        let stage = PathBuf::from(
            std::env::var_os("HARNESS_DEPENDENCY_STAGE").expect("explicit stage required"),
        );
        let digest =
            std::env::var("HARNESS_DEPENDENCY_MANIFEST_SHA256").expect("explicit digest required");
        let node = std::env::var_os("HARNESS_DEPENDENCY_NODE").map(PathBuf::from);
        let node_sha = std::env::var("HARNESS_DEPENDENCY_NODE_SHA256").ok();
        let node = match (node.as_ref(), node_sha.as_deref()) {
            (Some(path), Some(hash)) => Some((path.as_path(), hash)),
            (None, None) => None,
            _ => panic!("node path and digest must be supplied together"),
        };
        let inspected = inspect(&stage, &digest, node).unwrap();
        assert_eq!(inspected.report()["package_code_executed"], false);
        let validated = validate(&stage, &digest, node).unwrap();
        assert_eq!(validated.report()["status"], "runtime-verified");
        assert_eq!(validated.report()["activation_allowed"], false);
        println!("{}", validated.report());
    }
}
