//! Bounded UV venv staging for an owned, unactivated Python candidate.
//!
//! Trusted expected hashes pin the explicit uv.exe and python.exe identities
//! before execution. Executable hashing is not verification of the whole
//! Python/DLL tree. This module never installs packages, never relocates a
//! venv, never replaces an existing candidate, and never claims the candidate
//! is ready for activation.
#![cfg(windows)]

use crate::{
    dependency_discovery, native_build,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason},
    registration_native::{ReadGuard, StagedFile},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
};
use windows_sys::Win32::System::Pipes::CreatePipe;

const MAX_OUTPUT: u64 = 64 * 1024;
const MAX_MANIFEST: usize = 8 * 1024 * 1024;
const MAX_CFG: u64 = 16 * 1024;
const MAX_TREE_ENTRIES: usize = 16_384;
const MAX_FILE: u64 = 512 * 1024 * 1024;
const UV_TIMEOUT: Duration = Duration::from_secs(30);
const VERSION_TIMEOUT: Duration = Duration::from_secs(8);
const CLEANUP: Duration = Duration::from_secs(5);
const STORE_ALIAS: &str = "WindowsApps";

#[cfg(test)]
std::thread_local! {
    static FAIL_STDERR_READER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "python venv staging input or candidate was rejected",
    )
}

fn failed(code: &str) -> io::Error {
    io::Error::other(format!(
        "python venv staging {code}; installation preserved"
    ))
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
                    return Err(failed("output-bound-exceeded"));
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            Err(_) => {
                stop.cancel();
                return Err(failed("output-unavailable"));
            }
        }
    }
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

fn hash_file(path: &Path) -> io::Result<(ReadGuard, u64, String)> {
    let mut guard = ReadGuard::open(&path.components().collect::<PathBuf>())?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(guard.file.as_raw_handle(), &mut info) } == 0
        || info.nNumberOfLinks != 1
    {
        return Err(invalid());
    }
    let size = guard.file.metadata()?.len();
    if size > MAX_FILE {
        return Err(invalid());
    }
    let digest = hash_guard(&mut guard, size)?;
    guard.file.seek(SeekFrom::Start(0))?;
    Ok((guard, size, digest))
}

fn local_file(path: &Path) -> io::Result<PathBuf> {
    let path = dependency_discovery::local_path(path).map_err(|_| invalid())?;
    native_build::ordinary_ancestors(&path)?;
    crate::build_identity::ordinary(&path)?;
    if !path.is_file() {
        return Err(invalid());
    }
    Ok(path)
}

fn store_alias(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(STORE_ALIAS))
    })
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
        ("UV_CACHE_DIR", "uv-cache"),
        ("UV_PYTHON_INSTALL_DIR", "uv-python"),
        ("UV_TOOL_DIR", "uv-tools"),
        ("UV_TOOL_BIN_DIR", "uv-tool-bin"),
    ] {
        let path = root.join(folder);
        fs::create_dir_all(&path)?;
        command
            .env
            .insert(variable.into(), Some(path.into_os_string()));
    }
    command.env.insert("UV_NO_CONFIG".into(), Some("1".into()));
    command.env.insert("UV_OFFLINE".into(), Some("1".into()));
    command
        .env
        .insert("UV_PYTHON_DOWNLOADS".into(), Some("never".into()));
    Ok(())
}

fn run_bounded_with(
    mut command: CommandSpec,
    timeout: Duration,
    cancellation: &Cancellation,
) -> io::Result<(StopReason, u32, Vec<u8>, Vec<u8>)> {
    if cancellation.is_cancelled() {
        return Err(failed("cancelled-or-output-bound"));
    }
    let deadline = Deadline::after(timeout)?;
    let (stdout_read, stdout_write) = anonymous_pipe()?;
    let (stderr_read, stderr_write) = anonymous_pipe()?;
    command.stdout = Some(stdout_write);
    command.stderr = Some(stderr_write);
    let job = Job::new(Limits {
        memory_bytes: Some(512 * 1024 * 1024),
        cpu_percent: Some(25.0),
    })?;
    if cancellation.is_cancelled() {
        return Err(failed("cancelled-or-output-bound"));
    }
    let child = job.spawn(&command)?;
    // Close the parent's child endpoints before any fallible thread startup.
    // Otherwise a failed stderr reader can join stdout while our own retained
    // writer prevents EOF, even after the complete child Job was reclaimed.
    drop(command);
    let stop = Cancellation::default();
    let caller = cancellation.clone();
    let bridge_stop = stop.clone();
    let bridge = match std::thread::Builder::new()
        .name("python-venv-cancel-bridge".into())
        .spawn(move || {
            while !bridge_stop.is_cancelled() {
                if caller.is_cancelled() {
                    bridge_stop.cancel();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }) {
        Ok(worker) => worker,
        Err(_) => {
            let _ = job.terminate(130, CLEANUP);
            return Err(failed("cancelled-or-output-bound"));
        }
    };
    let stdout_stop = stop.clone();
    let stdout_worker = match std::thread::Builder::new()
        .name("python-venv-stdout".into())
        .spawn(move || drain_bounded(stdout_read, &stdout_stop))
    {
        Ok(worker) => worker,
        Err(_) => {
            stop.cancel();
            let _ = job.terminate(130, CLEANUP);
            let _ = bridge.join();
            return Err(failed("output-unavailable"));
        }
    };
    let stderr_stop = stop.clone();
    fn start_stderr(
        stream: File,
        stop: Cancellation,
    ) -> io::Result<std::thread::JoinHandle<io::Result<Vec<u8>>>> {
        #[cfg(test)]
        if FAIL_STDERR_READER.get() {
            return Err(io::Error::other("owned reader startup failure"));
        }
        std::thread::Builder::new()
            .name("python-venv-stderr".into())
            .spawn(move || drain_bounded(stream, &stop))
    }
    let stderr_worker = match start_stderr(stderr_read, stderr_stop) {
        Ok(worker) => worker,
        Err(_) => {
            stop.cancel();
            let _ = job.terminate(130, CLEANUP);
            let _ = bridge.join();
            let _ = stdout_worker.join();
            return Err(failed("output-unavailable"));
        }
    };
    let outcome = job.wait(&child, deadline, &stop, CLEANUP);
    stop.cancel();
    let _ = bridge.join();
    let stdout = stdout_worker.join().map_err(|_| invalid())?;
    let stderr = stderr_worker.join().map_err(|_| invalid())?;
    let outcome = outcome?;
    Ok((outcome.reason, outcome.exit_code, stdout?, stderr?))
}

fn stop_code(reason: StopReason, exit: u32) -> &'static str {
    match reason {
        StopReason::Timeout => "timeout",
        StopReason::Cancelled => "cancelled-or-output-bound",
        StopReason::MemoryLimit => "memory-limit",
        StopReason::Exited if exit == 0 => "exited",
        StopReason::Exited => "nonzero-exit",
    }
}

fn parse_cfg(text: &str) -> io::Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or_else(invalid)?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || values.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(invalid());
        }
    }
    Ok(values)
}

fn utf8_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_owned()
}

fn observe_candidate_files(root: &Path) -> io::Result<Vec<Value>> {
    let mut files = Vec::new();
    let mut entries = 0usize;
    let mut stack = vec![root.to_path_buf()];
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
                stack.push(path);
                continue;
            }
            if !metadata.is_file() {
                return Err(invalid());
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid())?
                .to_str()
                .ok_or_else(invalid)?
                .replace('\\', "/");
            let (_, size, sha256) = hash_file(&path)?;
            files.push(json!({
                "path": relative,
                "size": size,
                "sha256": sha256,
            }));
        }
    }
    files.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    Ok(files)
}

fn unique_stage(state: &Path) -> io::Result<PathBuf> {
    let staging_root = state.join("python-venv-staging");
    native_build::ordinary_ancestors(&staging_root)?;
    fs::create_dir_all(&staging_root)?;
    crate::build_identity::ordinary(&staging_root)?;
    let sequence = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    for extra in 0u32..32 {
        let id = format!("{}-{sequence}-{extra}", std::process::id());
        let stage = staging_root.join(id);
        match fs::create_dir(&stage) {
            Ok(()) => {
                native_build::ordinary_ancestors(&stage)?;
                return Ok(stage);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(failed("unique-staging-directory-unavailable"))
}

fn remove_tree(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(io::Error::other(
            "failed candidate cleanup requires recovery; installation preserved",
        )),
    }
}

fn retain_or_remove(path: &Path, cleanup: io::Result<()>, primary: io::Error) -> io::Error {
    match cleanup {
        Ok(()) => primary,
        Err(_) => io::Error::other(format!(
            "{primary}; failed candidate cleanup requires recovery; retained {}",
            path.display()
        )),
    }
}

fn owned_marker(state: &Path) -> io::Result<ReadGuard> {
    let mut guard = ReadGuard::open(&state.join("owner"))?;
    let mut bytes = Vec::new();
    (&mut guard.file)
        .take((native_build::OWNER.len() + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes != native_build::OWNER {
        return Err(failed("foreign-owner-marker"));
    }
    Ok(guard)
}

fn observed_version(result: io::Result<String>) -> (Option<String>, Option<String>) {
    match result {
        Ok(version) => (Some(version), None),
        Err(error) => (None, Some(error.to_string())),
    }
}

fn python_version_with(
    python: &Path,
    private: &Path,
    cancellation: &Cancellation,
) -> io::Result<String> {
    fs::create_dir_all(private)?;
    let mut command = CommandSpec::new(python);
    isolate_environment(&mut command, private)?;
    command.current_dir = Some(private.to_owned());
    command.args = vec!["--version".into()];
    let (reason, exit, stdout, stderr) = run_bounded_with(command, VERSION_TIMEOUT, cancellation)?;
    if reason != StopReason::Exited || exit != 0 || !stderr.is_empty() {
        return Err(failed(stop_code(reason, exit)));
    }
    let line = first_line(&utf8_text(&stdout));
    if !line.starts_with("Python ") {
        return Err(invalid());
    }
    Ok(line.trim_start_matches("Python ").trim().to_owned())
}

fn uv_version_with(uv: &Path, private: &Path, cancellation: &Cancellation) -> io::Result<String> {
    fs::create_dir_all(private)?;
    let mut command = CommandSpec::new(uv);
    isolate_environment(&mut command, private)?;
    command.current_dir = Some(private.to_owned());
    command.args = vec!["--version".into()];
    let (reason, exit, stdout, stderr) = run_bounded_with(command, VERSION_TIMEOUT, cancellation)?;
    if reason != StopReason::Exited || exit != 0 {
        return Err(failed(stop_code(reason, exit)));
    }
    let text = if stdout.is_empty() {
        utf8_text(&stderr)
    } else {
        utf8_text(&stdout)
    };
    let line = first_line(&text);
    let version = line
        .strip_prefix("uv ")
        .or_else(|| line.strip_prefix("uv.exe "))
        .unwrap_or(&line)
        .split_whitespace()
        .next()
        .ok_or_else(invalid)?;
    Ok(version.to_owned())
}

/// Explicit uv.exe, base python.exe, trusted hashes and owned native state.
#[derive(Clone, Debug)]
pub struct PythonVenvStageRequest<'a> {
    pub uv_exe: &'a Path,
    pub expected_uv_sha256: &'a str,
    pub python_exe: &'a Path,
    pub expected_python_sha256: &'a str,
    pub state: &'a Path,
}

/// Compact staged-unverified report plus the durable candidate/manifest paths.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StagedPythonVenv {
    pub report: Value,
    pub candidate: PathBuf,
    pub manifest: PathBuf,
}

/// Create a new empty UV venv at a unique owned candidate root.
///
/// The returned status is staged-unverified. Activation, wheel locks,
/// relocatable rewrite and registration remain parent-owned.
pub fn stage_uv_venv(request: PythonVenvStageRequest<'_>) -> io::Result<StagedPythonVenv> {
    stage_uv_venv_with_cancellation(request, &Cancellation::default())
}

/// Same as [`stage_uv_venv`], bridging caller cancellation through version and
/// `uv venv` waits. The owned job still stops its process tree.
pub fn stage_uv_venv_with_cancellation(
    request: PythonVenvStageRequest<'_>,
    cancellation: &Cancellation,
) -> io::Result<StagedPythonVenv> {
    if cancellation.is_cancelled() {
        return Err(failed("cancelled-or-output-bound"));
    }
    let expected_uv = lowercase_digest(request.expected_uv_sha256)?;
    let expected_python = lowercase_digest(request.expected_python_sha256)?;
    let state = dependency_discovery::local_path(request.state).map_err(|_| invalid())?;
    native_build::owner_root(&state)?;
    let _lock = native_build::lock_owned_state(&state)?;
    let _owner = owned_marker(&state)?;
    if cancellation.is_cancelled() {
        return Err(failed("cancelled-or-output-bound"));
    }
    if store_alias(request.uv_exe) || store_alias(request.python_exe) {
        return Err(failed("store-alias-rejected"));
    }
    let uv = local_file(request.uv_exe)?;
    let python = local_file(request.python_exe)?;
    if store_alias(&uv) || store_alias(&python) {
        return Err(failed("store-alias-rejected"));
    }
    let python_home = python.parent().ok_or_else(invalid)?.to_path_buf();
    native_build::ordinary_ancestors(&python_home)?;
    crate::build_identity::ordinary(&python_home)?;
    let (_uv_guard, uv_size, uv_sha) = hash_file(&uv)?;
    let (_python_guard, python_size, python_sha) = hash_file(&python)?;
    if uv_sha != expected_uv {
        return Err(failed("uv-hash-mismatch"));
    }
    if python_sha != expected_python {
        return Err(failed("python-hash-mismatch"));
    }
    if cancellation.is_cancelled() {
        return Err(failed("cancelled-or-output-bound"));
    }
    let stage = unique_stage(&state)?;
    let private = tempfile::Builder::new()
        .prefix("harness-python-venv-")
        .tempdir()?;
    let result = (|| {
        native_build::ordinary_ancestors(private.path())?;
        if cancellation.is_cancelled() {
            return Err(failed("cancelled-or-output-bound"));
        }
        let uv_version_result =
            uv_version_with(&uv, &private.path().join("uv-version"), cancellation);
        if cancellation.is_cancelled() {
            return Err(failed("cancelled-or-output-bound"));
        }
        let (observed_uv, uv_version_error) = observed_version(uv_version_result);
        let python_version_result = python_version_with(
            &python,
            &private.path().join("python-version"),
            cancellation,
        );
        if cancellation.is_cancelled() {
            return Err(failed("cancelled-or-output-bound"));
        }
        let (observed_python, python_version_error) = observed_version(python_version_result);
        if cancellation.is_cancelled() {
            return Err(failed("cancelled-or-output-bound"));
        }
        let work = private.path().join("work");
        fs::create_dir_all(&work)?;
        native_build::ordinary_ancestors(&work)?;
        let cache = work.join("uv-cache");
        fs::create_dir_all(&cache)?;
        let candidate = stage.join("candidate-venv");
        if candidate.exists() {
            return Err(failed("existing-candidate-refused"));
        }
        let mut command = CommandSpec::new(&uv);
        isolate_environment(&mut command, &work)?;
        command.current_dir = Some(work.clone());
        command.args = vec![
            "venv".into(),
            candidate.as_os_str().to_owned(),
            "--python".into(),
            python.as_os_str().to_owned(),
            "--no-project".into(),
            "--no-python-downloads".into(),
            "--offline".into(),
            "--no-config".into(),
            "--cache-dir".into(),
            cache.as_os_str().to_owned(),
            "--directory".into(),
            work.as_os_str().to_owned(),
            "--no-progress".into(),
        ];
        let (reason, exit, stdout, stderr) = run_bounded_with(command, UV_TIMEOUT, cancellation)?;
        if reason != StopReason::Exited || exit != 0 {
            return Err(failed(stop_code(reason, exit)));
        }
        if !candidate.is_dir() {
            return Err(failed("candidate-missing"));
        }
        native_build::ordinary_ancestors(&candidate)?;
        crate::build_identity::ordinary(&candidate)?;
        let cfg_path = candidate.join("pyvenv.cfg");
        let cfg_bytes = {
            let mut guard = ReadGuard::open(&cfg_path)?;
            let mut bytes = Vec::new();
            (&mut guard.file)
                .take(MAX_CFG + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_CFG {
                return Err(invalid());
            }
            bytes
        };
        let cfg_text = String::from_utf8(cfg_bytes.clone()).map_err(|_| invalid())?;
        let cfg = parse_cfg(&cfg_text)?;
        if cfg.contains_key("relocatable") {
            return Err(failed("relocatable-candidate-refused"));
        }
        let home = cfg.get("home").ok_or_else(invalid)?;
        let home_path = dependency_discovery::local_path(Path::new(home)).map_err(|_| invalid())?;
        if !crate::dependency_package::same_path(&home_path, &python_home) {
            return Err(failed("pyvenv-home-mismatch"));
        }
        if cfg.get("include-system-site-packages").map(String::as_str) != Some("false") {
            return Err(invalid());
        }
        let launcher = candidate.join("Scripts/python.exe");
        let launcher_w = candidate.join("Scripts/pythonw.exe");
        let (_launcher_guard, launcher_size, launcher_sha) = hash_file(&launcher)?;
        let (_launcher_w_guard, launcher_w_size, launcher_w_sha) = hash_file(&launcher_w)?;
        let files = observe_candidate_files(&candidate)?;
        if files.iter().any(|file| {
            file["path"]
                .as_str()
                .is_some_and(|path| path.contains(".dist-info/"))
        }) {
            return Err(failed("package-acquisition-refused"));
        }
        let cfg_sha = format!("{:x}", Sha256::digest(&cfg_bytes));
        let request_sha = format!(
            "{:x}",
            Sha256::digest(
                format!(
                    "{}\n{}\n{}\n{}\n{}",
                    uv.display(),
                    expected_uv,
                    python.display(),
                    expected_python,
                    state.display()
                )
                .as_bytes()
            )
        );
        let mut report = json!({
            "schema_version": 1,
            "operation": "uv-venv-candidate-preparation",
            "status": "staged-unverified",
            "activation_allowed": false,
            "ready_for_activation": false,
            "package_install": false,
            "runtime_tree_verified": false,
            "executable_hash_scope": "named-executables-only",
            "candidate": candidate,
            "stage": stage,
            "pyvenv": {
                "path": cfg_path,
                "sha256": cfg_sha,
                "home": home_path,
                "uv": cfg.get("uv").cloned(),
                "version_info": cfg.get("version_info").cloned(),
                "implementation": cfg.get("implementation").cloned(),
                "relocatable": false,
            },
            "tools": {
                "uv": {
                    "path": uv,
                    "sha256": uv_sha,
                    "size": uv_size,
                    "version": observed_uv,
                    "version_error": uv_version_error,
                    "hash_scope": "executable",
                },
                "python": {
                    "path": python,
                    "sha256": python_sha,
                    "size": python_size,
                    "version": observed_python,
                    "version_error": python_version_error,
                    "home": python_home,
                    "hash_scope": "executable",
                    "runtime_tree_verified": false,
                },
            },
            "launchers": {
                "python": { "path": launcher, "sha256": launcher_sha, "size": launcher_size },
                "pythonw": { "path": launcher_w, "sha256": launcher_w_sha, "size": launcher_w_size },
            },
            "files": files,
            "uv_stdout_bytes": stdout.len() as u64,
            "uv_stderr_bytes": stderr.len() as u64,
            "request_sha256": request_sha,
        });
        let manifest = serde_json::to_vec_pretty(&report)?;
        if manifest.len() > MAX_MANIFEST {
            return Err(invalid());
        }
        let manifest_path = stage.join("manifest.json");
        StagedFile::create(&manifest_path, &manifest)?.commit()?;
        report["manifest"] = json!(manifest_path);
        report["manifest_sha256"] = json!(format!("{:x}", Sha256::digest(&manifest)));
        Ok(StagedPythonVenv {
            report,
            candidate,
            manifest: manifest_path,
        })
    })();
    let leftover = private.keep();
    let private_cleanup = remove_tree(&leftover);
    match (result, private_cleanup) {
        (Ok(staged), Ok(())) => Ok(staged),
        (Ok(_), Err(cleanup)) => {
            let _ = remove_tree(&stage);
            Err(cleanup)
        }
        (Err(error), Ok(())) => Err(retain_or_remove(&stage, remove_tree(&stage), error)),
        (Err(error), Err(_)) => Err(io::Error::other(format!(
            "{error}; failed candidate cleanup requires recovery; retained {}",
            stage.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason};
    use sha2::{Digest, Sha256};
    use std::{
        fs::{self, File},
        path::{Path, PathBuf},
        time::Duration,
    };

    const STORE_ALIAS_COMPONENT: &str = "WindowsApps";

    fn owned_state() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        native_build::owner_root(&state).unwrap();
        (root, state)
    }

    fn rustc() -> PathBuf {
        let output = std::process::Command::new("where.exe")
            .arg("rustc.exe")
            .output()
            .unwrap();
        PathBuf::from(
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
    }

    fn compile(root: &Path, source: &str, output: &Path) {
        fs::write(root.join("fixture.rs"), source).unwrap();
        let mut command = CommandSpec::new(rustc());
        command.args = vec![
            root.join("fixture.rs").into_os_string(),
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

    fn compile_uv(root: &Path) -> PathBuf {
        let output = root.join("uv.exe");
        compile(root, include_str!("../tests/fixtures/fake_uv.rs"), &output);
        output
    }

    #[test]
    fn stderr_startup_failure_closes_parent_pipe_endpoints() {
        const CHILD: &str = "HARNESS_UV_READER_FAILURE_CHILD";
        if let Some(executable) = std::env::var_os(CHILD) {
            FAIL_STDERR_READER.set(true);
            let error = run_bounded_with(
                CommandSpec::new(executable),
                Duration::from_secs(1),
                &Cancellation::default(),
            )
            .unwrap_err();
            assert!(error.to_string().contains("output-unavailable"));
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let uv = compile_uv(root.path());
        let mut command = CommandSpec::new(std::env::current_exe().unwrap());
        command.args = vec![
            "dependency_python_stage::tests::stderr_startup_failure_closes_parent_pipe_endpoints"
                .into(),
            "--exact".into(),
            "--nocapture".into(),
        ];
        command.env.insert(CHILD.into(), Some(uv.into_os_string()));
        let output = root.path().join("reader-failure.log");
        let file = File::create(&output).unwrap();
        command.stdout = Some(file.try_clone().unwrap());
        command.stderr = Some(file);
        let job = Job::new(Limits {
            memory_bytes: Some(256 * 1024 * 1024),
            cpu_percent: Some(50.0),
        })
        .unwrap();
        let child = job.spawn(&command).unwrap();
        drop(command);
        let outcome = job
            .wait(
                &child,
                Deadline::after(Duration::from_secs(3)).unwrap(),
                &Cancellation::default(),
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(
            (outcome.reason, outcome.exit_code),
            (StopReason::Exited, 0),
            "{}",
            fs::read_to_string(output).unwrap()
        );
        assert_eq!(outcome.job.active_processes, 0);
    }

    fn write_python_stub(root: &Path) -> PathBuf {
        let python = root.join("python.exe");
        fs::write(&python, b"owned-python-stub").unwrap();
        python
    }

    fn sha(path: &Path) -> String {
        format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
    }

    fn request<'a>(
        uv: &'a Path,
        uv_sha: &'a str,
        python: &'a Path,
        python_sha: &'a str,
        state: &'a Path,
    ) -> PythonVenvStageRequest<'a> {
        PythonVenvStageRequest {
            uv_exe: uv,
            expected_uv_sha256: uv_sha,
            python_exe: python,
            expected_python_sha256: python_sha,
            state,
        }
    }

    fn set_mode(root: &Path, mode: &str) {
        fs::write(root.join("fake-uv-mode.txt"), mode).unwrap();
    }

    fn staging_children(state: &Path) -> Vec<PathBuf> {
        let root = state.join("python-venv-staging");
        if !root.exists() {
            return Vec::new();
        }
        fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect()
    }

    #[test]
    fn missing_and_incorrect_hashes_are_rejected_before_execution() {
        let (root, state) = owned_state();
        let uv = compile_uv(root.path());
        let python = write_python_stub(root.path());
        let uv_sha = sha(&uv);
        let python_sha = sha(&python);
        let missing = root.path().join("missing-python.exe");
        assert!(
            stage_uv_venv(request(&uv, &uv_sha, &missing, &python_sha, &state)).is_err(),
            "missing python accepted"
        );
        let wrong_uv = "0".repeat(64);
        assert!(
            stage_uv_venv(request(&uv, &wrong_uv, &python, &python_sha, &state)).is_err(),
            "wrong uv hash accepted"
        );
        let wrong_python = "1".repeat(64);
        assert!(
            stage_uv_venv(request(&uv, &uv_sha, &python, &wrong_python, &state)).is_err(),
            "wrong python hash accepted"
        );
        assert!(staging_children(&state).is_empty());
    }

    #[test]
    fn store_alias_python_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let store = root.path().join(STORE_ALIAS_COMPONENT).join("python.exe");
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        fs::write(&store, b"store-alias-stub").unwrap();
        let (root, state) = owned_state();
        let uv = compile_uv(root.path());
        let uv_sha = sha(&uv);
        let python_sha = "2".repeat(64);
        let error = stage_uv_venv(request(&uv, &uv_sha, &store, &python_sha, &state)).unwrap_err();
        assert!(error.to_string().contains("store-alias-rejected"));
        assert!(staging_children(&state).is_empty());
    }

    #[test]
    fn foreign_state_is_preserved() {
        let (root, _) = owned_state();
        let state = root.path().join("foreign");
        fs::create_dir_all(&state).unwrap();
        fs::write(state.join("owner"), b"other-owner\n").unwrap();
        let uv = compile_uv(root.path());
        let python = write_python_stub(root.path());
        assert!(stage_uv_venv(request(&uv, &sha(&uv), &python, &sha(&python), &state)).is_err());
        assert_eq!(fs::read(state.join("owner")).unwrap(), b"other-owner\n");
        assert!(!state.join("python-venv-staging").exists());
    }

    #[test]
    fn rust_uv_double_timeout_and_flood_are_contained() {
        let (root, state) = owned_state();
        let uv = compile_uv(root.path());
        let python = write_python_stub(root.path());
        let uv_sha = sha(&uv);
        let python_sha = sha(&python);
        set_mode(root.path(), "timeout");
        let timeout =
            stage_uv_venv(request(&uv, &uv_sha, &python, &python_sha, &state)).unwrap_err();
        assert!(timeout.to_string().contains("timeout"), "{timeout}");
        set_mode(root.path(), "flood");
        let flood_stop = Cancellation::default();
        let flood = stage_uv_venv_with_cancellation(
            request(&uv, &uv_sha, &python, &python_sha, &state),
            &flood_stop,
        )
        .unwrap_err();
        assert!(
            flood.to_string().contains("cancelled-or-output-bound")
                || flood.to_string().contains("output-bound-exceeded"),
            "{flood}"
        );
        assert!(
            !flood_stop.is_cancelled(),
            "output-bound failure cancelled the caller token"
        );
        set_mode(root.path(), "install-package");
        let packages =
            stage_uv_venv(request(&uv, &uv_sha, &python, &python_sha, &state)).unwrap_err();
        assert!(packages.to_string().contains("nonzero-exit"), "{packages}");
    }

    #[test]
    fn caller_cancellation_stops_the_owned_uv_tree() {
        let (root, state) = owned_state();
        let uv = root.path().join("uv.exe");
        let python = write_python_stub(root.path());
        fs::write(&uv, b"uncompiled-uv-stub").unwrap();
        let stop = Cancellation::default();
        stop.cancel();
        let error = stage_uv_venv_with_cancellation(
            request(&uv, &sha(&uv), &python, &sha(&python), &state),
            &stop,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("cancelled-or-output-bound"),
            "{error}"
        );
        assert!(!root.path().join("fixture.rs").exists());
        assert_eq!(fs::read(&uv).unwrap(), b"uncompiled-uv-stub");
    }

    #[test]
    fn rust_uv_double_stages_unverified_file_evidence() {
        let (root, state) = owned_state();
        let uv = compile_uv(root.path());
        let python = write_python_stub(root.path());
        let uv_sha = sha(&uv);
        let python_sha = sha(&python);
        let staged = stage_uv_venv(request(&uv, &uv_sha, &python, &python_sha, &state)).unwrap();
        assert_eq!(staged.report["status"], "staged-unverified");
        assert_eq!(staged.report["activation_allowed"], false);
        assert_eq!(staged.report["ready_for_activation"], false);
        assert_eq!(staged.report["runtime_tree_verified"], false);
        assert_eq!(staged.report["package_install"], false);
        assert_eq!(
            staged.report["executable_hash_scope"],
            "named-executables-only"
        );
        assert_eq!(staged.report["pyvenv"]["relocatable"], false);
        assert_eq!(
            PathBuf::from(staged.report["pyvenv"]["home"].as_str().unwrap()),
            python.parent().unwrap()
        );
        assert_eq!(staged.report["tools"]["uv"]["version"], "0.11.32");
        assert!(staged.report["tools"]["python"]["version"].is_null());
        assert!(staged.report["tools"]["python"]["version_error"].is_string());
        assert!(staged.candidate.join("pyvenv.cfg").is_file());
        assert!(staged.candidate.join("Scripts/python.exe").is_file());
        assert!(staged.manifest.is_file());
        assert!(
            !staged.report["manifest_sha256"]
                .as_str()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    #[ignore = "explicit owned uv/python only; set HARNESS_UV_EXECUTABLE, HARNESS_UV_SHA256, HARNESS_PYTHON_EXECUTABLE, HARNESS_PYTHON_SHA256"]
    fn actual_owned_uv_stages_empty_venv_without_package_install() {
        let uv = PathBuf::from(
            std::env::var_os("HARNESS_UV_EXECUTABLE").expect("HARNESS_UV_EXECUTABLE"),
        );
        let python = PathBuf::from(
            std::env::var_os("HARNESS_PYTHON_EXECUTABLE").expect("HARNESS_PYTHON_EXECUTABLE"),
        );
        let uv_sha = std::env::var("HARNESS_UV_SHA256").expect("HARNESS_UV_SHA256");
        let python_sha = std::env::var("HARNESS_PYTHON_SHA256").expect("HARNESS_PYTHON_SHA256");
        let (root, state) = owned_state();
        let before_python_dir = fs::read_dir(python.parent().unwrap()).unwrap().count();
        let staged = stage_uv_venv(request(&uv, &uv_sha, &python, &python_sha, &state)).unwrap();
        assert_eq!(staged.report["status"], "staged-unverified");
        assert_eq!(staged.report["activation_allowed"], false);
        assert_eq!(staged.report["ready_for_activation"], false);
        assert_eq!(staged.report["runtime_tree_verified"], false);
        assert_eq!(staged.report["package_install"], false);
        assert!(staged.report["tools"]["uv"]["version"].is_string());
        assert!(staged.report["tools"]["python"]["version"].is_string());
        assert_eq!(
            staged.report["pyvenv"]["uv"],
            staged.report["tools"]["uv"]["version"]
        );
        assert!(staged.report["pyvenv"]["version_info"].is_string());
        assert_eq!(
            PathBuf::from(staged.report["pyvenv"]["home"].as_str().unwrap()),
            python.parent().unwrap()
        );
        let cfg = fs::read_to_string(staged.candidate.join("pyvenv.cfg")).unwrap();
        assert!(cfg.contains("home = "));
        assert!(!cfg.contains("relocatable"));
        assert!(staged.candidate.join("Scripts/python.exe").is_file());
        assert!(staged.candidate.join("Scripts/pythonw.exe").is_file());
        assert!(
            !staged
                .candidate
                .join("Lib/site-packages")
                .read_dir()
                .unwrap()
                .any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .ends_with(".dist-info")
                })
        );
        assert_eq!(
            fs::read_dir(python.parent().unwrap()).unwrap().count(),
            before_python_dir
        );
        assert!(!root.path().join(".venv").exists());
    }
}
