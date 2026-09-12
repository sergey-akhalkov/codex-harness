//! Native token-workflow-only Check, Recover, Disconnect and Install.
//! Install reuses or acquires the pinned RTK archive and bounded adapter
//! build into CODEX_HOME/harness/rtk, then links only harness/bin/rtk.exe and
//! harness/bin/harness-rtk.exe. Feature edits use the recorded original CLI
//! from installation metadata and an ordinary config.toml; they are skipped
//! when that executable is absent so offline artifact tests stay model-free.
#![cfg(windows)]

use crate::{
    build_identity,
    config_file::ConfigSnapshot,
    dependency_archive, dependency_assets,
    dependency_fetch::Client,
    feature_edit::{self, Feature},
    installation_lock::InstallationLocks,
    installation_state::normal,
    inventory, native_build,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason},
    registration_native::{self as native, FileGuard, StagedFile, StagedLink},
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs, io,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub struct Request {
    pub source: PathBuf,
    pub codex_home: PathBuf,
    pub user_home: PathBuf,
    pub preview: bool,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub status: &'static str,
    pub model_calls: u32,
    pub rtk_version: Option<String>,
    pub source_identity: Option<String>,
}

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
}

fn read_json(path: &Path) -> io::Result<Option<Value>> {
    inventory::ordinary_parents(path)?;
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).map_err(|_| {
            conflict("token workflow state is not JSON; preserving it")
        })?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(conflict(&format!(
            "token workflow destination changed; preserving {}",
            path.display()
        ))),
    }
}

fn json_bytes(value: &Value) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    let after = json_bytes(value)?;
    match FileGuard::read_regular(path) {
        Ok((guard, current)) => {
            let identity = guard.object_identity()?;
            drop(guard);
            FileGuard::replace_regular(path, &identity, &current, &after)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            StagedFile::create(path, &after)?.commit()
        }
        Err(error) => Err(error),
    }
}

fn delete_regular(path: &Path) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    match FileGuard::read_regular(path) {
        Ok((guard, _)) => guard.remove(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn recorded_codex_command(home: &Path) -> io::Result<Option<PathBuf>> {
    let metadata = home.join("harness/installation.json");
    let Some(value) = read_json(&metadata)? else {
        return Ok(None);
    };
    let Some(text) = value["codexCommand"].as_str() else {
        return Ok(None);
    };
    let path = PathBuf::from(text);
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
    {
        return Err(conflict(
            "recorded original CLI is not an absolute native executable; preserving it",
        ));
    }
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
        Ok(_) => {
            build_identity::ordinary(&path)?;
            Ok(Some(path))
        }
    }
}

fn feature_timeout() -> Duration {
    Duration::from_secs(30)
}

fn ordinary_config(home: &Path) -> io::Result<Option<ConfigSnapshot>> {
    let config = home.join("config.toml");
    inventory::ordinary_parents(&config)?;
    match fs::symlink_metadata(&config) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
        Ok(metadata) if metadata.file_type().is_symlink() || metadata.is_dir() => Err(conflict(
            "Hook policy requires an ordinary base config; preserving the current path.",
        )),
        Ok(_) => Ok(Some(ConfigSnapshot::read(&config)?)),
    }
}

fn publish_feature(
    home: &Path,
    upstream: &Path,
    feature: Feature,
    enabled: bool,
) -> io::Result<()> {
    let config = home.join("config.toml");
    match ordinary_config(home)? {
        None => {
            let edit =
                feature_edit::prepare_feature(upstream, &[], feature, enabled, feature_timeout())?;
            if let Some(parent) = config.parent() {
                fs::create_dir_all(parent)?;
            }
            StagedFile::create(&config, edit.proposed_config())?.commit()
        }
        Some(snapshot) => {
            let edit = feature_edit::prepare_feature(
                upstream,
                snapshot.contents(),
                feature,
                enabled,
                feature_timeout(),
            )?;
            if snapshot.contents() == edit.proposed_config() {
                return Ok(());
            }
            edit.publish_to(&snapshot).map(drop)
        }
    }
}

fn apply_token_workflow_features(
    home: &Path,
    previous: Option<&Value>,
    next: &Value,
) -> io::Result<()> {
    let Some(upstream) = recorded_codex_command(home)? else {
        return Ok(());
    };
    let previous_enabled = previous.is_some_and(|value| value["enabled"] == true);
    let next_enabled = next["enabled"] == true;
    if next_enabled {
        if previous_enabled {
            return Ok(());
        }
        publish_feature(home, &upstream, Feature::CodeMode, true)?;
        publish_feature(home, &upstream, Feature::Hooks, true)?;
        return Ok(());
    }
    if previous_enabled {
        let features = feature_edit::discover_features(&upstream, home, feature_timeout())?;
        publish_feature(home, &upstream, Feature::Hooks, false)?;
        if features.code_mode {
            let previous_code_mode = previous
                .and_then(|value| value["previousCodeMode"].as_bool())
                .unwrap_or(false);
            publish_feature(home, &upstream, Feature::CodeMode, previous_code_mode)?;
        }
        return Ok(());
    }
    publish_feature(home, &upstream, Feature::Hooks, false)
}

fn planned_token_workflow_state(
    home: &Path,
    previous: Option<&Value>,
    mut next: Value,
) -> io::Result<Value> {
    if next["enabled"] == true {
        let previous_code_mode = if previous.is_some_and(|value| value["enabled"] == true) {
            previous
                .map(|value| value["previousCodeMode"].clone())
                .unwrap_or(Value::Null)
        } else if let Some(upstream) = recorded_codex_command(home)? {
            ordinary_config(home)?;
            Value::Bool(
                feature_edit::discover_features(&upstream, home, feature_timeout())?.code_mode,
            )
        } else {
            previous
                .map(|value| value["previousCodeMode"].clone())
                .unwrap_or(Value::Null)
        };
        if !previous_code_mode.is_null() {
            next["previousCodeMode"] = previous_code_mode;
        }
    }
    Ok(next)
}

fn owned_bin_link(home: &Path, destination: &Path) -> io::Result<PathBuf> {
    let destination = local_drive(destination)?;
    let bin = local_drive(&home.join("harness/bin"))?;
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if destination.parent() != Some(bin.as_path()) || !matches!(name, "rtk.exe" | "harness-rtk.exe")
    {
        return Err(conflict(
            "token workflow destination is outside harness/bin; preserving it",
        ));
    }
    Ok(destination)
}

fn optional_path(value: &Value) -> io::Result<Option<PathBuf>> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.is_empty() => Ok(None),
        Value::String(text) => Ok(Some(PathBuf::from(text))),
        _ => Err(conflict("token workflow pending path is not a string")),
    }
}

fn local_drive(path: &Path) -> io::Result<PathBuf> {
    let text = path
        .to_str()
        .ok_or_else(|| conflict("token workflow path is not UTF-8"))?;
    normal(Path::new(text.strip_prefix(r"\\?\").unwrap_or(text)))
}

fn path_text(path: &Path) -> io::Result<String> {
    local_drive(path)?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| conflict("token workflow path is not UTF-8"))
}

fn current_target(path: &Path) -> io::Result<Option<PathBuf>> {
    match FileGuard::capture_link(path) {
        Ok(guard) => {
            let (target, directory) = guard.link_description()?;
            if directory {
                return Err(conflict(
                    "token workflow destination is a directory link; preserving it",
                ));
            }
            Ok(Some(target))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn same_target(left: &Path, right: &Path) -> io::Result<bool> {
    native::targets_match(left, right)
}

fn restore_link(destination: &Path, old: Option<&Path>, new: Option<&Path>) -> io::Result<()> {
    let mut current = current_target(destination)?;
    if let (Some(cur), Some(new)) = (current.as_deref(), new)
        && same_target(cur, new)?
    {
        native::verified_link(destination, new, false)?.remove()?;
        current = None;
    }
    match (old, current) {
        (Some(old), None) => StagedLink::create(destination, old, false)?.commit(),
        (Some(old), Some(cur)) if same_target(&cur, old)? => Ok(()),
        (Some(_), Some(_)) => Err(conflict(&format!(
            "Cannot restore changed destination: {}",
            destination.display()
        ))),
        (None, None) => Ok(()),
        (None, Some(_)) => Err(conflict(&format!(
            "Cannot undo externally changed destination: {}",
            destination.display()
        ))),
    }
}

fn disconnect_link(destination: &Path, expected: &Path) -> io::Result<()> {
    match current_target(destination)? {
        None => Ok(()),
        Some(current) if same_target(&current, expected)? => {
            native::verified_link(destination, expected, false)?.remove()
        }
        Some(_) => Err(conflict(&format!(
            "token workflow destination changed; preserving {}",
            destination.display()
        ))),
    }
}

pub fn check(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let pending = home.join("harness/token-workflow-pending.json");
    inventory::ordinary_parents(&pending)?;
    if pending.exists() {
        return Err(conflict(
            "Token workflow transaction is pending; use --token-workflow-only Recover.",
        ));
    }
    if request.preview {
        return Ok(Report {
            status: "Preview token workflow Check",
            model_calls: 0,
            rtk_version: None,
            source_identity: None,
        });
    }
    let state = read_json(&home.join("harness/token-workflow.json"))?;
    let Some(state) = state else {
        return Ok(Report {
            status: "Token workflow disconnected",
            model_calls: 0,
            rtk_version: None,
            source_identity: None,
        });
    };
    if state["enabled"] != true {
        return Ok(Report {
            status: "Token workflow disconnected",
            model_calls: 0,
            rtk_version: None,
            source_identity: None,
        });
    }
    let links = state["links"]
        .as_array()
        .ok_or_else(|| conflict("token workflow links are missing"))?;
    for link in links {
        let destination = PathBuf::from(
            link["destination"]
                .as_str()
                .ok_or_else(|| conflict("token workflow destination is missing"))?,
        );
        let source = PathBuf::from(
            link["source"]
                .as_str()
                .ok_or_else(|| conflict("token workflow source is missing"))?,
        );
        let expected = link["sha256"]
            .as_str()
            .ok_or_else(|| conflict("token workflow hash is missing"))?;
        let target = fs::read_link(&destination).map_err(|_| {
            conflict(&format!(
                "Token workflow artifact mismatch: {}",
                destination.display()
            ))
        })?;
        if target != source || !source.is_file() {
            return Err(conflict(&format!(
                "Token workflow artifact mismatch: {}",
                destination.display()
            )));
        }
        if !build_identity::hash_file(&source)?.eq_ignore_ascii_case(expected) {
            return Err(conflict(&format!(
                "Token workflow artifact mismatch: {}",
                destination.display()
            )));
        }
    }
    let _ = request.source;
    Ok(Report {
        status: "Token workflow connected",
        model_calls: 0,
        rtk_version: state["rtkVersion"].as_str().map(str::to_owned),
        source_identity: state["sourceIdentity"].as_str().map(str::to_owned),
    })
}

pub fn recover(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let pending = home.join("harness/token-workflow-pending.json");
    inventory::ordinary_parents(&pending)?;
    if !pending.exists() {
        return Ok(Report {
            status: "No token workflow transaction",
            model_calls: 0,
            rtk_version: None,
            source_identity: None,
        });
    }
    if request.preview {
        return Ok(Report {
            status: "Preview token workflow recovery",
            model_calls: 0,
            rtk_version: None,
            source_identity: None,
        });
    }
    if recorded_codex_command(&home)?.is_some() {
        ordinary_config(&home)?;
    }
    let pending_value = read_json(&pending)?.ok_or_else(|| {
        conflict("token workflow pending transaction disappeared; preserving current state")
    })?;
    let operations = pending_value["operations"]
        .as_array()
        .ok_or_else(|| conflict("token workflow pending operations are missing"))?;
    for operation in operations.iter().rev() {
        let destination = owned_bin_link(
            &home,
            &PathBuf::from(
                operation["destination"]
                    .as_str()
                    .ok_or_else(|| conflict("token workflow pending destination is missing"))?,
            ),
        )?;
        restore_link(
            &destination,
            optional_path(&operation["oldSource"])?.as_deref(),
            optional_path(&operation["newSource"])?.as_deref(),
        )?;
    }
    if pending_value
        .get("previousState")
        .is_some_and(|value| !value.is_null())
    {
        write_json(
            &home.join("harness/token-workflow.json"),
            &pending_value["previousState"],
        )?;
    } else {
        delete_regular(&home.join("harness/token-workflow.json"))?;
    }
    let recovered = read_json(&home.join("harness/token-workflow.json"))?;
    apply_token_workflow_features(
        &home,
        pending_value.get("previousState"),
        recovered.as_ref().unwrap_or(&Value::Null),
    )?;
    delete_regular(&pending)?;
    Ok(Report {
        status: "Recovered token workflow",
        model_calls: 0,
        rtk_version: pending_value["previousState"]["rtkVersion"]
            .as_str()
            .map(str::to_owned),
        source_identity: pending_value["previousState"]["sourceIdentity"]
            .as_str()
            .map(str::to_owned),
    })
}

pub fn disconnect(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let pending = home.join("harness/token-workflow-pending.json");
    inventory::ordinary_parents(&pending)?;
    if pending.exists() {
        return Err(conflict(
            "Token workflow transaction is pending; use --token-workflow-only Recover.",
        ));
    }
    let state = read_json(&home.join("harness/token-workflow.json"))?;
    if state.as_ref().is_none_or(|value| value["enabled"] != true) {
        return Ok(Report {
            status: "Token workflow disconnected",
            model_calls: 0,
            rtk_version: None,
            source_identity: None,
        });
    }
    if request.preview {
        return Ok(Report {
            status: "Preview token workflow Disconnect",
            model_calls: 0,
            rtk_version: state
                .as_ref()
                .and_then(|value| value["rtkVersion"].as_str().map(str::to_owned)),
            source_identity: state
                .as_ref()
                .and_then(|value| value["sourceIdentity"].as_str().map(str::to_owned)),
        });
    }
    let state = state.expect("enabled token workflow state");
    if recorded_codex_command(&home)?.is_some() {
        ordinary_config(&home)?;
    }
    let mut operations = Vec::new();
    let links = state["links"]
        .as_array()
        .ok_or_else(|| conflict("token workflow links are missing"))?;
    for link in links {
        let destination = owned_bin_link(
            &home,
            &PathBuf::from(
                link["destination"]
                    .as_str()
                    .ok_or_else(|| conflict("token workflow destination is missing"))?,
            ),
        )?;
        let source = PathBuf::from(
            link["source"]
                .as_str()
                .ok_or_else(|| conflict("token workflow source is missing"))?,
        );
        operations.push(serde_json::json!({
            "destination": path_text(&destination)?,
            "oldSource": path_text(&source)?,
            "newSource": Value::Null
        }));
    }
    let next_state = serde_json::json!({"enabled": false, "links": []});
    write_json(
        &pending,
        &serde_json::json!({
            "previousState": state,
            "plannedState": next_state,
            "operations": operations
        }),
    )?;
    for operation in operations.iter().rev() {
        let destination = PathBuf::from(operation["destination"].as_str().unwrap());
        let old = PathBuf::from(operation["oldSource"].as_str().unwrap());
        disconnect_link(&destination, &old)?;
    }
    write_json(&home.join("harness/token-workflow.json"), &next_state)?;
    apply_token_workflow_features(&home, Some(&state), &next_state)?;
    delete_regular(&pending)?;
    Ok(Report {
        status: "Token workflow disconnected",
        model_calls: 0,
        rtk_version: state["rtkVersion"].as_str().map(str::to_owned),
        source_identity: state["sourceIdentity"].as_str().map(str::to_owned),
    })
}

fn definition(source: &Path) -> io::Result<Value> {
    let path = source.join("global/rtk.json");
    inventory::ordinary_parents(&path)?;
    let bytes = fs::read(&path).map_err(|_| conflict("RTK selection record is missing"))?;
    serde_json::from_slice(&bytes).map_err(|_| conflict("RTK selection record is not JSON"))
}

fn require_file(path: &Path, expected: &str, label: &str) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    build_identity::ordinary(path)?;
    if !path.is_file() {
        return Err(conflict(&format!(
            "{label} is missing after native RTK acquisition"
        )));
    }
    let actual = build_identity::hash_file(path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(conflict(&format!(
            "{label} identity mismatch; preserving the unexpected file"
        )));
    }
    Ok(())
}

fn rtk_url(definition: &Value) -> io::Result<&str> {
    let url = definition["url"]
        .as_str()
        .ok_or_else(|| conflict("RTK download URL is missing"))?;
    let version = definition["version"]
        .as_str()
        .ok_or_else(|| conflict("RTK version is missing"))?;
    let expected = format!(
        "https://github.com/rtk-ai/rtk/releases/download/v{version}/rtk-x86_64-pc-windows-msvc.zip"
    );
    if url != expected {
        return Err(conflict(
            "RTK download URL is not the pinned official Windows x64 archive",
        ));
    }
    Ok(url)
}

fn source_identity(source: &Path) -> io::Result<String> {
    let mut hashes = Vec::new();
    for relative in [
        "Cargo.toml",
        "Cargo.lock",
        "tools/rtk-adapter/Cargo.toml",
        "tools/rtk-adapter/src/main.rs",
        "global/rtk.json",
        "tools/token-workflow.psm1",
    ] {
        hashes.push(build_identity::hash_file(&source.join(relative))?);
    }
    Ok(build_identity::hash_bytes(hashes.join(":").as_bytes()))
}

struct Capture<'a> {
    input: &'a mut dyn Read,
    hash: Sha256,
}

impl Read for Capture<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(output)?;
        self.hash.update(&output[..count]);
        Ok(count)
    }
}

fn acquire_rtk(home: &Path, definition: &Value) -> io::Result<PathBuf> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err(conflict("RTK selection currently requires Windows x64."));
    }
    let version = definition["version"]
        .as_str()
        .ok_or_else(|| conflict("RTK version is missing"))?;
    let executable_hash = definition["executableSha256"]
        .as_str()
        .ok_or_else(|| conflict("RTK executable hash is missing"))?;
    let package = home.join(format!("harness/rtk/packages/{version}"));
    let rtk = package.join("rtk.exe");
    inventory::ordinary_parents(&rtk)?;
    native_build::ordinary_ancestors(&package)?;
    if rtk.is_file() {
        require_file(&rtk, executable_hash, "RTK binary")?;
        return Ok(rtk);
    }
    fs::create_dir_all(&package)?;
    let archive_hash = definition["sha256"]
        .as_str()
        .ok_or_else(|| conflict("RTK archive hash is missing"))?;
    let url = rtk_url(definition)?;
    let client = Client::new()
        .map_err(|error| conflict(&format!("RTK download client is unavailable: {error}")))?;
    let bytes = client
        .github_asset(&dependency_assets::Asset {
            url: url.to_owned(),
            sha256: archive_hash.to_ascii_lowercase(),
            size: 128 * 1024 * 1024,
            id: 1,
        })
        .map_err(|error| conflict(&format!("RTK archive download failed: {error}")))?;
    if build_identity::hash_bytes(&bytes) != archive_hash.to_ascii_lowercase() {
        return Err(conflict(
            "RTK release checksum mismatch; dependency not installed.",
        ));
    }
    extract_rtk_archive(&package, &rtk, &bytes, executable_hash)
}

fn extract_rtk_archive(
    package: &Path,
    rtk: &Path,
    bytes: &[u8],
    executable_hash: &str,
) -> io::Result<PathBuf> {
    let mut extracted = None;
    dependency_archive::visit_zip(bytes, |name, size, reader| {
        if Path::new(name).file_name() != Some(OsStr::new("rtk.exe")) {
            return Ok(());
        }
        if extracted.is_some() {
            return Err(conflict("RTK archive contains more than one rtk.exe"));
        }
        native_build::ordinary_ancestors(package)?;
        let mut capture = Capture {
            input: reader,
            hash: Sha256::new(),
        };
        StagedFile::from_reader(rtk, &mut capture, size)?.commit()?;
        extracted = Some(format!("{:x}", capture.hash.finalize()));
        Ok(())
    })?;
    let extracted = extracted.ok_or_else(|| conflict("RTK archive is missing rtk.exe"))?;
    if extracted != executable_hash.to_ascii_lowercase() {
        let _ = delete_regular(rtk);
        return Err(conflict(
            "RTK binary identity mismatch; extracted candidate was not kept.",
        ));
    }
    require_file(rtk, executable_hash, "RTK binary")?;
    Ok(rtk.to_path_buf())
}

fn build_adapter(source: &Path, home: &Path, rtk: &Path, identity: &str) -> io::Result<PathBuf> {
    let build = home.join(format!("harness/rtk/build/{identity}"));
    let adapter = build.join("harness-rtk.exe");
    let record_path = build.join("build.json");
    inventory::ordinary_parents(&adapter)?;
    native_build::ordinary_ancestors(&build)?;
    if adapter.is_file() {
        let record = read_json(&record_path)?.ok_or_else(|| {
            conflict("Unidentified RTK adapter build; preserving it for inspection.")
        })?;
        let recorded = record["binarySha256"]
            .as_str()
            .ok_or_else(|| conflict("RTK adapter build identity mismatch."))?;
        if record["sourceIdentity"].as_str() != Some(identity)
            || !build_identity::hash_file(&adapter)?.eq_ignore_ascii_case(recorded)
        {
            return Err(conflict("RTK adapter build identity mismatch."));
        }
        return Ok(adapter);
    }
    fs::create_dir_all(&build)?;
    let cargo = native_build::resolve_tool(OsStr::new("cargo"))?;
    let target = home.join("harness/rtk/cargo-target");
    fs::create_dir_all(&target)?;
    native_build::ordinary_ancestors(&target)?;
    let log_path = build.join("cargo.log");
    let log = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)?;
    let mut command = CommandSpec::new(cargo);
    command.args = [
        "build",
        "--release",
        "--locked",
        "--jobs",
        "1",
        "-p",
        "harness-rtk",
        "--manifest-path",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    command.args.extend([
        source.join("Cargo.toml").into_os_string(),
        "--target-dir".into(),
        target.as_os_str().to_owned(),
    ]);
    command.current_dir = Some(source.to_path_buf());
    command.env.insert(
        "CARGO_TARGET_DIR".into(),
        Some(target.as_os_str().to_owned()),
    );
    command.stdout = Some(log.try_clone()?);
    command.stderr = Some(log);
    let job = Job::new(Limits {
        memory_bytes: Some(2048 * 1024 * 1024),
        cpu_percent: Some(50.0),
    })?;
    let child = job.spawn(&command).map_err(|error| {
        conflict(&format!(
            "Starting bounded RTK adapter build failed: {error}"
        ))
    })?;
    let status = job.wait(
        &child,
        Deadline::after(Duration::from_secs(600))?,
        &Cancellation::default(),
        Duration::from_secs(5),
    )?;
    drop(command);
    if status.reason != StopReason::Exited || status.exit_code != 0 {
        return Err(conflict(&format!(
            "RTK adapter build failed (exit {}, {:?}); see {}",
            status.exit_code,
            status.reason,
            log_path.display()
        )));
    }
    let compiled = target.join("release/harness-rtk.exe");
    native_build::ordinary_ancestors(&compiled)?;
    fs::copy(&compiled, &adapter)?;
    write_json(
        &record_path,
        &serde_json::json!({
            "sourceIdentity": identity,
            "binarySha256": build_identity::hash_file(&adapter)?
        }),
    )?;
    let sibling = build.join("rtk.exe");
    match current_target(&sibling)? {
        Some(current) if same_target(&current, rtk)? => {}
        Some(_) => {
            return Err(conflict(
                "RTK build dependency link conflict; preserving it.",
            ));
        }
        None => StagedLink::create(&sibling, rtk, false)?.commit()?,
    }
    Ok(adapter)
}

pub fn install(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let source = normal(&request.source)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let pending = home.join("harness/token-workflow-pending.json");
    inventory::ordinary_parents(&pending)?;
    if pending.exists() {
        return Err(conflict(
            "Token workflow transaction is pending; use --token-workflow-only Recover.",
        ));
    }
    let state = read_json(&home.join("harness/token-workflow.json"))?;
    if request.preview {
        return Ok(Report {
            status: "Preview token workflow Install",
            model_calls: 0,
            rtk_version: state
                .as_ref()
                .and_then(|value| value["rtkVersion"].as_str().map(str::to_owned)),
            source_identity: state
                .as_ref()
                .and_then(|value| value["sourceIdentity"].as_str().map(str::to_owned)),
        });
    }
    let definition = definition(&source)?;
    let version = definition["version"]
        .as_str()
        .ok_or_else(|| conflict("RTK version is missing"))?;
    let executable_hash = definition["executableSha256"]
        .as_str()
        .ok_or_else(|| conflict("RTK executable hash is missing"))?;
    let identity = source_identity(&source)?;
    let rtk = acquire_rtk(&home, &definition)?;
    require_file(&rtk, executable_hash, "RTK binary")?;
    let adapter = build_adapter(&source, &home, &rtk, &identity)?;
    build_identity::ordinary(&adapter)?;
    let planned = [
        ("rtk.exe", rtk.clone()),
        ("harness-rtk.exe", adapter.clone()),
    ];
    let mut operations = Vec::new();
    let mut links = Vec::new();
    for (name, next) in planned {
        let destination = owned_bin_link(&home, &home.join("harness/bin").join(name))?;
        let current = current_target(&destination)?;
        let previous = state.as_ref().and_then(|value| {
            value["links"].as_array().and_then(|items| {
                items.iter().find_map(|item| {
                    item["destination"].as_str().and_then(|recorded| {
                        same_target(&PathBuf::from(recorded), &destination)
                            .ok()
                            .filter(|matched| *matched)
                            .and_then(|_| item["source"].as_str().map(PathBuf::from))
                    })
                })
            })
        });
        if let (Some(current), Some(previous)) = (current.as_ref(), previous.as_ref())
            && !same_target(current, previous)?
        {
            return Err(conflict(&format!(
                "Token workflow target conflict; preserving {}",
                destination.display()
            )));
        }
        if current.is_some() && previous.is_none() {
            return Err(conflict(&format!(
                "Token workflow target conflict; preserving {}",
                destination.display()
            )));
        }
        if current
            .as_ref()
            .is_none_or(|current| !same_target(current, &next).unwrap_or(false))
        {
            operations.push(serde_json::json!({
                "destination": path_text(&destination)?,
                "oldSource": current.as_ref().map(|path| path_text(path)).transpose()?,
                "newSource": path_text(&next)?
            }));
        }
        links.push(serde_json::json!({
            "destination": path_text(&destination)?,
            "source": path_text(&next)?,
            "sha256": build_identity::hash_file(&next)?
        }));
    }
    let next_state = planned_token_workflow_state(
        &home,
        state.as_ref(),
        serde_json::json!({
            "schemaVersion": 1,
            "enabled": true,
            "links": links,
            "sourceRoot": path_text(&source)?,
            "rtkVersion": version,
            "sourceIdentity": identity
        }),
    )?;
    write_json(
        &pending,
        &serde_json::json!({
            "previousState": state.clone().unwrap_or(Value::Null),
            "plannedState": next_state,
            "operations": operations
        }),
    )?;
    for operation in &operations {
        let destination = PathBuf::from(operation["destination"].as_str().unwrap());
        let old = optional_path(&operation["oldSource"])?;
        let new = optional_path(&operation["newSource"])?
            .ok_or_else(|| conflict("token workflow install source is missing"))?;
        if let Some(old) = old.as_deref() {
            disconnect_link(&destination, old)?;
        }
        StagedLink::create(&destination, &new, false)?.commit()?;
    }
    write_json(&home.join("harness/token-workflow.json"), &next_state)?;
    apply_token_workflow_features(&home, state.as_ref(), &next_state)?;
    delete_regular(&pending)?;
    Ok(Report {
        status: "Token workflow connected",
        model_calls: 0,
        rtk_version: Some(version.to_owned()),
        source_identity: next_state["sourceIdentity"].as_str().map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_mismatch_is_rejected_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        let bin = home.join("harness/bin");
        fs::create_dir_all(&bin).unwrap();
        let source = root.path().join("rtk.exe");
        fs::write(&source, b"original").unwrap();
        let destination = bin.join("rtk.exe");
        std::os::windows::fs::symlink_file(&source, &destination).unwrap();
        let hash = build_identity::hash_file(&source).unwrap();
        fs::write(&source, b"altered").unwrap();
        let state = home.join("harness/token-workflow.json");
        fs::write(
            &state,
            serde_json::to_vec(&serde_json::json!({
                "enabled": true,
                "links": [{
                    "destination": destination,
                    "source": source,
                    "sha256": hash
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let before = fs::read(&state).unwrap();
        let error = check(&Request {
            source: root.path().join("source"),
            codex_home: home,
            user_home: root.path().join("user"),
            preview: false,
        })
        .unwrap_err();
        assert!(error.to_string().contains("artifact mismatch"));
        assert_eq!(fs::read(&state).unwrap(), before);
    }

    #[test]
    fn recover_preview_preserves_pending_transaction() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        fs::create_dir_all(home.join("harness")).unwrap();
        let pending = home.join("harness/token-workflow-pending.json");
        fs::write(&pending, b"{\"operations\":[]}").unwrap();
        let before = fs::read(&pending).unwrap();
        let report = recover(&Request {
            source: root.path().join("source"),
            codex_home: home,
            user_home: root.path().join("user"),
            preview: true,
        })
        .unwrap();
        assert_eq!(report.status, "Preview token workflow recovery");
        assert_eq!(fs::read(&pending).unwrap(), before);
    }

    fn request(home: &Path, user: &Path) -> Request {
        Request {
            source: home.join("source"),
            codex_home: home.to_path_buf(),
            user_home: user.to_path_buf(),
            preview: false,
        }
    }

    #[test]
    fn disconnect_removes_owned_bin_link_and_preserves_foreign_file() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let bin = home.join("harness/bin");
        fs::create_dir_all(&bin).unwrap();
        let source = root.path().join("rtk.exe");
        fs::write(&source, b"owned-rtk").unwrap();
        let destination = bin.join("rtk.exe");
        std::os::windows::fs::symlink_file(&source, &destination).unwrap();
        let foreign = bin.join("foreign.txt");
        fs::write(&foreign, b"keep").unwrap();
        fs::write(
            home.join("harness/token-workflow.json"),
            serde_json::to_vec(&serde_json::json!({
                "enabled": true,
                "rtkVersion": "0.48.0",
                "links": [{
                    "destination": destination,
                    "source": source,
                    "sha256": build_identity::hash_file(&source).unwrap()
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let report = disconnect(&request(&home, &user)).unwrap();
        assert_eq!(report.status, "Token workflow disconnected");
        assert!(!destination.exists());
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        assert!(!home.join("harness/token-workflow-pending.json").exists());
        let state: Value =
            serde_json::from_slice(&fs::read(home.join("harness/token-workflow.json")).unwrap())
                .unwrap();
        assert_eq!(state["enabled"], false);
    }

    #[test]
    fn recover_restores_owned_bin_link_from_pending_journal() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let bin = home.join("harness/bin");
        fs::create_dir_all(&bin).unwrap();
        let old = root.path().join("old-rtk.exe");
        let new = root.path().join("new-rtk.exe");
        fs::write(&old, b"old").unwrap();
        fs::write(&new, b"new").unwrap();
        let destination = bin.join("harness-rtk.exe");
        std::os::windows::fs::symlink_file(&new, &destination).unwrap();
        fs::write(
            home.join("harness/token-workflow.json"),
            serde_json::to_vec(&serde_json::json!({"enabled": true, "links": []})).unwrap(),
        )
        .unwrap();
        fs::write(
            home.join("harness/token-workflow-pending.json"),
            serde_json::to_vec(&serde_json::json!({
                "previousState": {
                    "enabled": false,
                    "rtkVersion": "0.1.0",
                    "sourceIdentity": "abc"
                },
                "operations": [{
                    "destination": destination,
                    "oldSource": old,
                    "newSource": new
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let report = recover(&request(&home, &user)).unwrap();
        assert_eq!(report.status, "Recovered token workflow");
        assert_eq!(fs::read_link(&destination).unwrap(), old);
        assert!(!home.join("harness/token-workflow-pending.json").exists());
        let state: Value =
            serde_json::from_slice(&fs::read(home.join("harness/token-workflow.json")).unwrap())
                .unwrap();
        assert_eq!(state["enabled"], false);
        assert_eq!(state["rtkVersion"], "0.1.0");
    }

    #[test]
    fn recover_restores_dangling_owned_bin_link_without_following_its_target() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let bin = home.join("harness/bin");
        fs::create_dir_all(&bin).unwrap();
        let old = root.path().join("old-rtk.exe");
        let new = root.path().join("new-rtk.exe");
        fs::write(&old, b"old").unwrap();
        fs::write(&new, b"new").unwrap();
        let destination = bin.join("rtk.exe");
        std::os::windows::fs::symlink_file(&new, &destination).unwrap();
        fs::remove_file(&new).unwrap();
        fs::write(
            home.join("harness/token-workflow-pending.json"),
            serde_json::to_vec(&serde_json::json!({
                "previousState": {"enabled": false, "links": []},
                "operations": [{
                    "destination": destination,
                    "oldSource": old,
                    "newSource": new
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let report = recover(&request(&home, &user)).unwrap();
        assert_eq!(report.status, "Recovered token workflow");
        assert_eq!(fs::read_link(&destination).unwrap(), old);
        assert_eq!(fs::read(&old).unwrap(), b"old");
        assert!(!new.exists());
    }

    #[test]
    fn disconnect_preserves_foreign_regular_file_at_owned_destination() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let bin = home.join("harness/bin");
        fs::create_dir_all(&bin).unwrap();
        let source = root.path().join("rtk.exe");
        fs::write(&source, b"owned-rtk").unwrap();
        let destination = bin.join("rtk.exe");
        fs::write(&destination, b"foreign").unwrap();
        fs::write(
            home.join("harness/token-workflow.json"),
            serde_json::to_vec(&serde_json::json!({
                "enabled": true,
                "links": [{
                    "destination": destination,
                    "source": source,
                    "sha256": build_identity::hash_file(&source).unwrap()
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let error = disconnect(&request(&home, &user)).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("preserving") || message.contains("changed"),
            "{message}"
        );
        assert_eq!(fs::read(&destination).unwrap(), b"foreign");
        assert!(home.join("harness/token-workflow.json").exists());
    }

    #[test]
    fn disconnect_refuses_destination_outside_owned_bin() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        fs::create_dir_all(home.join("harness/bin")).unwrap();
        let foreign = home.join("harness/foreign.exe");
        fs::write(&foreign, b"keep").unwrap();
        fs::write(
            home.join("harness/token-workflow.json"),
            serde_json::to_vec(&serde_json::json!({
                "enabled": true,
                "links": [{
                    "destination": foreign,
                    "source": root.path().join("rtk.exe"),
                    "sha256": "abc"
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let error = disconnect(&request(&home, &user)).unwrap_err();
        assert!(error.to_string().contains("outside harness/bin"));
        assert_eq!(fs::read(home.join("harness/foreign.exe")).unwrap(), b"keep");
    }

    fn staged_request(root: &Path) -> (PathBuf, PathBuf, PathBuf, Request) {
        let home = std::path::absolute(root).unwrap().join("codex");
        let user = std::path::absolute(root).unwrap().join("user");
        let source = std::path::absolute(root).unwrap().join("source");
        fs::create_dir_all(source.join("global")).unwrap();
        fs::create_dir_all(home.join("harness/bin")).unwrap();
        fs::create_dir_all(home.join("harness/rtk/packages/0.48.0")).unwrap();
        let rtk = home.join("harness/rtk/packages/0.48.0/rtk.exe");
        fs::write(&rtk, b"staged-rtk").unwrap();
        fs::write(
            source.join("global/rtk.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": "0.48.0",
                "executableSha256": build_identity::hash_file(&rtk).unwrap()
            }))
            .unwrap(),
        )
        .unwrap();
        for relative in [
            "Cargo.toml",
            "Cargo.lock",
            "tools/rtk-adapter/Cargo.toml",
            "tools/rtk-adapter/src/main.rs",
            "tools/token-workflow.psm1",
        ] {
            let path = source.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, relative.as_bytes()).unwrap();
        }
        let identity = super::source_identity(&source).unwrap();
        let adapter = home.join(format!("harness/rtk/build/{identity}/harness-rtk.exe"));
        fs::create_dir_all(adapter.parent().unwrap()).unwrap();
        fs::write(&adapter, b"staged-adapter").unwrap();
        fs::write(
            adapter.parent().unwrap().join("build.json"),
            serde_json::to_vec(&serde_json::json!({
                "sourceIdentity": identity,
                "binarySha256": build_identity::hash_file(&adapter).unwrap()
            }))
            .unwrap(),
        )
        .unwrap();
        (
            home.clone(),
            rtk,
            adapter,
            Request {
                source,
                codex_home: home,
                user_home: user,
                preview: false,
            },
        )
    }

    #[test]
    fn install_preview_does_not_create_bin_links() {
        let root = tempfile::tempdir().unwrap();
        let (home, _, _, mut request) = staged_request(root.path());
        request.preview = true;
        let report = install(&request).unwrap();
        assert_eq!(report.status, "Preview token workflow Install");
        assert!(!home.join("harness/bin/rtk.exe").exists());
        assert!(!home.join("harness/token-workflow.json").exists());
    }

    #[test]
    fn install_links_pre_staged_artifacts_and_preserves_foreign_bin_file() {
        let root = tempfile::tempdir().unwrap();
        let (home, rtk, adapter, request) = staged_request(root.path());
        let foreign = home.join("harness/bin/foreign.txt");
        fs::write(&foreign, b"keep").unwrap();
        let report = install(&request).unwrap();
        assert_eq!(report.status, "Token workflow connected");
        assert_eq!(
            fs::read_link(home.join("harness/bin/rtk.exe")).unwrap(),
            rtk
        );
        assert_eq!(
            fs::read_link(home.join("harness/bin/harness-rtk.exe")).unwrap(),
            adapter
        );
        assert_eq!(fs::read(&foreign).unwrap(), b"keep");
        assert!(!home.join("harness/token-workflow-pending.json").exists());
        let connected = check(&request).unwrap();
        assert_eq!(connected.status, "Token workflow connected");
    }

    #[test]
    fn install_rejects_checksum_mismatch_without_creating_links() {
        let root = tempfile::tempdir().unwrap();
        let (home, rtk, _, request) = staged_request(root.path());
        fs::write(&rtk, b"altered").unwrap();
        let error = install(&request).unwrap_err();
        assert!(error.to_string().contains("identity mismatch"));
        assert!(!home.join("harness/bin/rtk.exe").exists());
        assert!(!home.join("harness/token-workflow.json").exists());
    }

    #[test]
    fn install_preserves_foreign_symlink_at_owned_destination() {
        let root = tempfile::tempdir().unwrap();
        let (home, _, _, request) = staged_request(root.path());
        let foreign = root.path().join("foreign.exe");
        fs::write(&foreign, b"foreign").unwrap();
        let destination = home.join("harness/bin/rtk.exe");
        std::os::windows::fs::symlink_file(&foreign, &destination).unwrap();
        let error = install(&request).unwrap_err();
        assert!(error.to_string().contains("target conflict"));
        assert_eq!(fs::read_link(&destination).unwrap(), foreign);
        assert!(!home.join("harness/token-workflow.json").exists());
    }

    #[test]
    fn install_refuses_missing_rtk_without_download_url() {
        let root = tempfile::tempdir().unwrap();
        let (home, rtk, _, request) = staged_request(root.path());
        fs::remove_file(&rtk).unwrap();
        let error = install(&request).unwrap_err();
        assert!(error.to_string().contains("RTK archive hash is missing"));
        assert!(!home.join("harness/bin/rtk.exe").exists());
        assert!(!home.join("harness/token-workflow.json").exists());
    }

    #[test]
    fn extract_rtk_archive_accepts_owned_zip_and_rejects_identity_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("package");
        fs::create_dir_all(&package).unwrap();
        let rtk = package.join("rtk.exe");
        let payload = b"owned-rtk-bytes";
        let expected = build_identity::hash_bytes(payload);
        let mut archive = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut archive));
            writer
                .start_file("rtk.exe", zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, payload).unwrap();
            writer.finish().unwrap();
        }
        let extracted = extract_rtk_archive(&package, &rtk, &archive, &expected).unwrap();
        assert_eq!(extracted, rtk);
        assert_eq!(fs::read(&rtk).unwrap(), payload);
        let other = root.path().join("other");
        fs::create_dir_all(&other).unwrap();
        let error =
            extract_rtk_archive(&other, &other.join("rtk.exe"), &archive, "deadbeef").unwrap_err();
        assert!(error.to_string().contains("identity mismatch"));
        assert!(!other.join("rtk.exe").exists());
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

    fn compile_codex_features(root: &Path) -> PathBuf {
        let output = std::path::absolute(root).unwrap().join("codex.exe");
        fs::write(
            root.join("codex.rs"),
            include_str!("../tests/fixtures/fake_codex_features.rs"),
        )
        .unwrap();
        let mut command = crate::process::CommandSpec::new(rustc());
        command.args = vec![
            root.join("codex.rs").into_os_string(),
            "--edition=2024".into(),
            "-o".into(),
            output.as_os_str().to_owned(),
        ];
        let log = fs::File::create(root.join("compile.log")).unwrap();
        command.stdout = Some(log.try_clone().unwrap());
        command.stderr = Some(log);
        let job = crate::process::Job::new(crate::process::Limits {
            memory_bytes: Some(512 * 1024 * 1024),
            cpu_percent: Some(50.0),
        })
        .unwrap();
        let child = job.spawn(&command).unwrap();
        let outcome = job
            .wait(
                &child,
                crate::process::Deadline::after(Duration::from_secs(30)).unwrap(),
                &crate::process::Cancellation::default(),
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(
            (outcome.reason, outcome.exit_code),
            (crate::process::StopReason::Exited, 0)
        );
        output
    }

    fn write_installation(home: &Path, upstream: &Path) {
        fs::create_dir_all(home.join("harness")).unwrap();
        fs::write(
            home.join("harness/installation.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "codexCommand": upstream
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn config_text(home: &Path) -> String {
        fs::read_to_string(home.join("config.toml")).unwrap_or_default()
    }

    #[test]
    fn install_enables_native_features_from_recorded_cli() {
        let root = tempfile::tempdir().unwrap();
        let (home, _, _, request) = staged_request(root.path());
        let upstream = compile_codex_features(root.path());
        write_installation(&home, &upstream);
        fs::write(
            home.join("config.toml"),
            b"[features]
hooks = false
code_mode = false
",
        )
        .unwrap();
        let report = install(&request).unwrap();
        assert_eq!(report.status, "Token workflow connected");
        let config = config_text(&home);
        assert!(config.contains("hooks = true"), "{config}");
        assert!(config.contains("code_mode = true"), "{config}");
        let state: Value =
            serde_json::from_slice(&fs::read(home.join("harness/token-workflow.json")).unwrap())
                .unwrap();
        assert_eq!(state["previousCodeMode"], false);
    }

    #[test]
    fn disconnect_restores_recorded_code_mode_and_disables_hooks() {
        let root = tempfile::tempdir().unwrap();
        let (home, rtk, _, request) = staged_request(root.path());
        let upstream = compile_codex_features(root.path());
        write_installation(&home, &upstream);
        fs::write(
            home.join("config.toml"),
            b"[features]
hooks = true
code_mode = true
",
        )
        .unwrap();
        let destination = home.join("harness/bin/rtk.exe");
        std::os::windows::fs::symlink_file(&rtk, &destination).unwrap();
        fs::write(
            home.join("harness/token-workflow.json"),
            serde_json::to_vec(&serde_json::json!({
                "enabled": true,
                "previousCodeMode": false,
                "links": [{
                    "destination": destination,
                    "source": rtk,
                    "sha256": build_identity::hash_file(&rtk).unwrap()
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let report = disconnect(&request).unwrap();
        assert_eq!(report.status, "Token workflow disconnected");
        let config = config_text(&home);
        assert!(config.contains("hooks = false"), "{config}");
        assert!(config.contains("code_mode = false"), "{config}");
    }

    #[test]
    fn recover_of_not_enabled_previous_state_disables_hooks() {
        let root = tempfile::tempdir().unwrap();
        let (home, _, _, request) = staged_request(root.path());
        let upstream = compile_codex_features(root.path());
        write_installation(&home, &upstream);
        fs::write(
            home.join("config.toml"),
            b"[features]
hooks = true
code_mode = true
",
        )
        .unwrap();
        fs::write(
            home.join("harness/token-workflow-pending.json"),
            serde_json::to_vec(&serde_json::json!({
                "previousState": {"enabled": false, "links": []},
                "operations": []
            }))
            .unwrap(),
        )
        .unwrap();
        let report = recover(&request).unwrap();
        assert_eq!(report.status, "Recovered token workflow");
        let config = config_text(&home);
        assert!(config.contains("hooks = false"), "{config}");
        assert!(config.contains("code_mode = true"), "{config}");
    }

    #[test]
    fn feature_edits_refuse_a_reparse_config_without_following_it() {
        let root = tempfile::tempdir().unwrap();
        let (home, _, _, request) = staged_request(root.path());
        let upstream = compile_codex_features(root.path());
        write_installation(&home, &upstream);
        let foreign = root.path().join("foreign-config.toml");
        fs::write(
            &foreign,
            b"keep = true
",
        )
        .unwrap();
        std::os::windows::fs::symlink_file(&foreign, home.join("config.toml")).unwrap();
        let error = install(&request).unwrap_err();
        assert!(error.to_string().contains("ordinary base config"));
        assert_eq!(
            fs::read(&foreign).unwrap(),
            b"keep = true
"
        );
        assert!(!home.join("harness/bin/rtk.exe").exists());
        assert!(!home.join("harness/token-workflow.json").exists());
        assert!(!home.join("harness/token-workflow-pending.json").exists());
    }

    #[test]
    fn install_preview_does_not_edit_features() {
        let root = tempfile::tempdir().unwrap();
        let (home, _, _, mut request) = staged_request(root.path());
        let upstream = compile_codex_features(root.path());
        write_installation(&home, &upstream);
        fs::write(
            home.join("config.toml"),
            b"[features]
hooks = false
",
        )
        .unwrap();
        request.preview = true;
        let report = install(&request).unwrap();
        assert_eq!(report.status, "Preview token workflow Install");
        assert_eq!(
            fs::read(home.join("config.toml")).unwrap(),
            b"[features]
hooks = false
"
        );
        assert!(!home.join("harness/bin/rtk.exe").exists());
    }
}
