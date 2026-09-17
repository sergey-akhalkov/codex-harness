//! Native subscriptions-only Check/preview/Install/Recover/Disconnect. This
//! path never starts, stops or queries the live routing proxy. Install writes
//! owned routing files and source links, and may register an owned Task
//! Scheduler definition without starting it. Recover can restore an
//! interrupted restart-policy journal in place. configure-restart updates the
//! owned Task Scheduler restart policy without starting or stopping the live
//! proxy.
#![cfg(windows)]

use crate::{
    build_identity,
    installation_lock::InstallationLocks,
    installation_state::normal,
    inventory,
    registration_native::{self as native, FileGuard, StagedFile, StagedLink},
    task_scheduler,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub struct Request {
    pub source: PathBuf,
    pub codex_home: PathBuf,
    pub user_home: PathBuf,
    pub preview: bool,
    pub manager: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub status: &'static str,
    pub model_calls: u32,
    pub port: Option<u64>,
    pub note: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RestartPolicyReport {
    pub status: &'static str,
    pub model_calls: u32,
    pub port: Option<u64>,
    pub note: &'static str,
    pub task: String,
    pub restart_count: i32,
    pub restart_interval: &'static str,
    pub changed: bool,
}

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
}

fn read_json(path: &Path) -> io::Result<Option<Value>> {
    inventory::ordinary_parents(path)?;
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).map_err(|_| {
            conflict("subscription state is not JSON; preserving it")
        })?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn present(path: &Path) -> io::Result<bool> {
    inventory::ordinary_parents(path)?;
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
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
    write_bytes(path, &json_bytes(value)?)
}

fn write_bytes(path: &Path, after: &[u8]) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    match FileGuard::read_regular(path) {
        Ok((guard, current)) => {
            let identity = guard.object_identity()?;
            drop(guard);
            FileGuard::replace_regular(path, &identity, &current, after)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            StagedFile::create(path, after)?.commit()
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

fn local_drive(path: &Path) -> io::Result<PathBuf> {
    let text = path
        .to_str()
        .ok_or_else(|| conflict("subscription path is not UTF-8"))?;
    normal(Path::new(text.strip_prefix(r"\\?\").unwrap_or(text)))
}

fn path_text(path: &Path) -> io::Result<String> {
    local_drive(path)?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| conflict("subscription path is not UTF-8"))
}

fn optional_text(value: &Value) -> io::Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.is_empty() => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(conflict("subscription snapshot is not a string")),
    }
}

fn snapshot(path: &Path) -> io::Result<Option<String>> {
    inventory::ordinary_parents(path)?;
    match fs::read(path) {
        Ok(bytes) => Ok(Some(STANDARD.encode(bytes))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn restore_snapshot(path: &Path, before: &Value, after: &Value, native: &Value) -> io::Result<()> {
    let current = snapshot(path)?;
    let before = optional_text(before)?;
    let after = optional_text(after)?;
    let native = optional_text(native)?;
    if current != before && current != after && current != native {
        return Err(conflict(&format!(
            "Subscription file changed; preserving: {}",
            path.display()
        )));
    }
    match before {
        None => delete_regular(path),
        Some(bytes) => write_bytes(
            path,
            &STANDARD
                .decode(bytes)
                .map_err(|_| conflict("subscription snapshot is not Base64"))?,
        ),
    }
}

fn current_link(path: &Path) -> io::Result<Option<(PathBuf, bool)>> {
    inventory::ordinary_parents(path)?;
    match FileGuard::capture_link(path) {
        Ok(guard) => Ok(Some(guard.link_description()?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(conflict(&format!(
            "Foreign subscription source link preserved: {}",
            path.display()
        ))),
    }
}

fn same_target(left: &Path, right: &Path) -> io::Result<bool> {
    native::targets_match(&local_drive(left)?, &local_drive(right)?)
}

fn set_link(path: &Path, target: Option<&Path>, expected: Option<&Path>) -> io::Result<()> {
    let current = current_link(path)?;
    match (current.as_ref(), expected) {
        (None, None) => {}
        (Some((actual, _)), Some(expected)) if same_target(actual, expected)? => {}
        _ => {
            return Err(conflict(&format!(
                "Subscription link changed; preserving: {}",
                path.display()
            )));
        }
    }
    match (current.as_ref(), target) {
        (None, None) => Ok(()),
        (Some((actual, directory)), Some(wanted)) if same_target(actual, wanted)? => {
            let _ = directory;
            Ok(())
        }
        (Some((actual, directory)), None) => {
            native::verified_link(path, actual, *directory)?.remove()
        }
        (None, Some(wanted)) => {
            let directory = fs::metadata(wanted)
                .map(|meta| meta.is_dir())
                .unwrap_or(true);
            StagedLink::create(path, wanted, directory)?.commit()
        }
        (Some((actual, directory)), Some(wanted)) => {
            native::verified_link(path, actual, *directory)?.remove()?;
            let directory = fs::metadata(wanted)
                .map(|meta| meta.is_dir())
                .unwrap_or(*directory);
            StagedLink::create(path, wanted, directory)?.commit()
        }
    }
}

#[allow(dead_code)]
pub struct ServicePaths {
    pub source: PathBuf,
    pub user: PathBuf,
    pub home: PathBuf,
    pub state: PathBuf,
    pub pending: PathBuf,
    pub restart_pending: PathBuf,
    pub service: PathBuf,
    pub runtime: PathBuf,
    pub opencodex: PathBuf,
    pub config: PathBuf,
    pub config_link: PathBuf,
    pub config_source: PathBuf,
    pub role_link: PathBuf,
    pub role_source: PathBuf,
    pub task: String,
    pub zai_key: PathBuf,
    pub zai_profile: PathBuf,
    pub zai_catalog: PathBuf,
    pub xai_profile: PathBuf,
    pub xai_catalog: PathBuf,
    pub xai_profile_source: PathBuf,
    pub xai_catalog_source: PathBuf,
}

pub fn service_paths(source: &Path, user: &Path, home: &Path) -> io::Result<ServicePaths> {
    let identity = build_identity::hash_bytes(path_text(home)?.to_ascii_lowercase().as_bytes());
    Ok(ServicePaths {
        source: source.to_path_buf(),
        user: user.to_path_buf(),
        home: home.to_path_buf(),
        state: home.join("harness/subscription-routing.json"),
        pending: home.join("harness/subscription-routing-pending.json"),
        restart_pending: home.join("harness/subscription-restart-policy-pending.json"),
        service: home.join("harness/subscriptions/service.json"),
        runtime: home.join("harness/subscriptions/runs"),
        opencodex: user.join(".opencodex"),
        config: home.join("config.toml"),
        config_link: user.join(".opencodex/config.json"),
        config_source: source.join("global/opencodex/config.json"),
        role_link: home.join("agents/codex-harness-subscriptions"),
        role_source: source.join("global/opencodex/agents"),
        task: format!("codex-harness-subscriptions-{}", &identity[..16]),
        zai_key: home.join("harness/subscriptions/zai-key.txt"),
        zai_profile: home.join("zai.config.toml"),
        zai_catalog: home.join("zai.models.json"),
        xai_profile: home.join("xai.config.toml"),
        xai_catalog: home.join("xai.models.json"),
        xai_profile_source: source.join("global/codex-profiles/xai.config.toml"),
        xai_catalog_source: source.join("global/codex-profiles/xai.models.json"),
    })
}

fn paths(source: &Path, user: &Path, home: &Path) -> io::Result<ServicePaths> {
    service_paths(source, user, home)
}

pub fn assert_owned(state: &Value, paths: &ServicePaths) -> io::Result<()> {
    owned(state, paths)
}

pub fn current_link_target(path: &Path) -> io::Result<Option<PathBuf>> {
    Ok(current_link(path)?.map(|(target, _)| target))
}

pub fn set_owned_link(
    path: &Path,
    target: Option<&Path>,
    expected: Option<&Path>,
) -> io::Result<()> {
    set_link(path, target, expected)
}

pub fn path_display(path: &Path) -> io::Result<String> {
    path_text(path)
}

fn owned(state: &Value, paths: &ServicePaths) -> io::Result<()> {
    let version = state["schema_version"].as_u64();
    if !matches!(version, Some(1) | Some(2))
        || state["owner"] != "codex-harness-subscriptions"
        || state["codex"] != path_text(&paths.home)?
        || state["user"] != path_text(&paths.user)?
        || (version == Some(1) && state["task"] != paths.task)
    {
        return Err(conflict(
            "Subscription ownership record mismatch; preserving state.",
        ));
    }
    Ok(())
}

fn preserved_private(
    paths: &ServicePaths,
    before_key: Option<&[u8]>,
    before_profile: Option<&[u8]>,
    before_catalog: Option<&[u8]>,
) -> io::Result<()> {
    let read = |path: &Path| match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    };
    if read(&paths.zai_key)? != before_key.map(ToOwned::to_owned) {
        return Err(conflict("Local zai key store was created or modified."));
    }
    if read(&paths.zai_profile)? != before_profile.map(ToOwned::to_owned) {
        return Err(conflict(
            "Local zai profile files were created or modified.",
        ));
    }
    if read(&paths.zai_catalog)? != before_catalog.map(ToOwned::to_owned) {
        return Err(conflict(
            "Local zai profile files were created or modified.",
        ));
    }
    Ok(())
}

fn optional_path(value: &Value) -> io::Result<Option<PathBuf>> {
    match value {
        Value::Null => Ok(None),
        Value::String(text) if text.is_empty() => Ok(None),
        Value::String(text) => Ok(Some(PathBuf::from(text))),
        _ => Err(conflict("subscription link is not a string")),
    }
}

fn refuse_running(observed: Option<&task_scheduler::ObservedTask>) -> io::Result<()> {
    if observed.is_some_and(|task| task.running) {
        return Err(conflict(
            "Owned subscription task is running; live proxy was not stopped.",
        ));
    }
    Ok(())
}

fn xml_matches(actual: &str, expected: &str) -> io::Result<bool> {
    if actual == expected {
        return Ok(true);
    }
    task_scheduler::equivalent(actual, expected)
}

fn owned_task_present(
    observed: Option<&task_scheduler::ObservedTask>,
    expected: Option<&str>,
) -> io::Result<()> {
    match (observed, expected) {
        (None, _) => Ok(()),
        (Some(task), Some(expected)) if xml_matches(&task.xml, expected)? => Ok(()),
        _ => Err(conflict("Foreign or changed subscription task preserved.")),
    }
}

fn restore_idle_task(name: &str, before: Option<&str>, after: Option<&str>) -> io::Result<()> {
    let observed = task_scheduler::observe(name)?;
    refuse_running(observed.as_ref())?;
    let current = observed.as_ref().map(|task| task.xml.as_str());
    let matches_one = |expected: Option<&str>| -> io::Result<bool> {
        match (current, expected) {
            (None, None) => Ok(true),
            (Some(actual), Some(expected)) => xml_matches(actual, expected),
            _ => Ok(false),
        }
    };
    if matches_one(before)? {
        return Ok(());
    }
    if current.is_some() && !matches_one(after)? {
        return Err(conflict(
            "Subscription task changed after interruption; preserving.",
        ));
    }
    match before {
        None => {
            let _ = task_scheduler::remove(name, current)?;
            Ok(())
        }
        Some(xml) => {
            let _ = task_scheduler::register(name, xml, current)?;
            Ok(())
        }
    }
}

fn restore_journaled_task(pending: &Value, name: &str) -> io::Result<()> {
    if pending.get("task_before").is_none() && pending.get("task_after").is_none() {
        return Ok(());
    }
    restore_idle_task(
        name,
        pending["task_before"].as_str(),
        pending["task_after"].as_str(),
    )
}

fn assert_journaled_task_restorable(pending: &Value, name: &str) -> io::Result<()> {
    if pending.get("task_before").is_none() && pending.get("task_after").is_none() {
        return Ok(());
    }
    let observed = task_scheduler::observe(name)?;
    refuse_running(observed.as_ref())?;
    let current = observed.as_ref().map(|task| task.xml.as_str());
    let matches_one = |expected: Option<&str>| -> io::Result<bool> {
        match (current, expected) {
            (None, None) => Ok(true),
            (Some(actual), Some(expected)) => xml_matches(actual, expected),
            _ => Ok(false),
        }
    };
    if current.is_none()
        || matches_one(pending["task_before"].as_str())?
        || matches_one(pending["task_after"].as_str())?
    {
        return Ok(());
    }
    Err(conflict(
        "Subscription task changed after interruption; preserving.",
    ))
}

/// Write owned routing records, source links and an idle Task Scheduler
/// definition. Does not start OpenCodex or query the live proxy.
fn legacy_state(state: &Value) -> bool {
    state.get("schema_version").and_then(Value::as_u64) != Some(2)
}

fn remove_legacy_opencodex(
    paths: &ServicePaths,
    existing: &Value,
    pending_path: &Path,
) -> io::Result<Value> {
    let observed = task_scheduler::observe(&paths.task)?;
    refuse_running(observed.as_ref())?;
    owned_task_present(observed.as_ref(), existing["task_xml"].as_str())?;
    let task_before = observed.as_ref().map(|task| task.xml.clone());
    let expected_config = optional_path(&existing["links"]["configLink"])?;
    let expected_role = optional_path(&existing["links"]["roleLink"])?;
    let config_current = current_link(&paths.config_link)?;
    let role_current = current_link(&paths.role_link)?;
    match (config_current.as_ref(), expected_config.as_deref()) {
        (None, None) => {}
        (None, Some(_)) => {}
        (Some((actual, _)), Some(expected)) if same_target(actual, expected)? => {}
        _ => return Err(conflict("Foreign subscription source link preserved.")),
    }
    match (role_current.as_ref(), expected_role.as_deref()) {
        (None, None) => {}
        (None, Some(_)) => {}
        (Some((actual, _)), Some(expected)) if same_target(actual, expected)? => {}
        _ => return Err(conflict("Foreign subscription source link preserved.")),
    }
    let pending = serde_json::json!({
        "schema_version": 1,
        "owner": "codex-harness-subscriptions",
        "operation": "retire-opencodex",
        "source": path_text(&paths.source)?,
        "user": path_text(&paths.user)?,
        "codex": path_text(&paths.home)?,
        "task": paths.task,
        "task_before": task_before,
        "task_after": Value::Null,
        "links_before": {
            "configLink": expected_config.as_ref().map(|path| path_text(path)).transpose()?,
            "roleLink": expected_role.as_ref().map(|path| path_text(path)).transpose()?
        },
        "links_after": { "configLink": Value::Null, "roleLink": Value::Null },
        "state_before": snapshot(&paths.state)?,
        "state_after": Value::Null
    });
    write_json(pending_path, &pending)?;
    task_scheduler::remove(&paths.task, task_before.as_deref())?;
    if config_current.is_some() {
        set_link(&paths.config_link, None, expected_config.as_deref())?;
    }
    if role_current.is_some() {
        set_link(&paths.role_link, None, expected_role.as_deref())?;
    }
    Ok(pending)
}

fn subscription_state_v2(paths: &ServicePaths) -> io::Result<Value> {
    Ok(serde_json::json!({
        "schema_version": 2,
        "owner": "codex-harness-subscriptions",
        "source": path_text(&paths.source)?,
        "user": path_text(&paths.user)?,
        "codex": path_text(&paths.home)?
    }))
}

/// Write the native subscription records (xAI profile and catalog) and retire
/// any legacy OpenCodex task, links and state. OpenCodex is never started or
/// queried here.
/// Write the native subscription records (xAI profile and catalog) and retire
/// any legacy OpenCodex task, links and state. OpenCodex is never started or
/// queried here.
pub fn install(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let source = normal(&request.source)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let paths = paths(&source, &user, &home)?;
    let key = profile_bytes(&paths.zai_key)?;
    let profile = profile_bytes(&paths.zai_profile)?;
    let catalog = profile_bytes(&paths.zai_catalog)?;
    if present(&paths.restart_pending)? {
        return Err(conflict(
            "An interrupted restart policy update needs --subscriptions-only Recover.",
        ));
    }
    if present(&paths.pending)? {
        return Err(conflict(
            "An interrupted subscription operation needs Recover.",
        ));
    }
    let existing = read_json(&paths.state)?;
    if let Some(state) = &existing {
        owned(state, &paths)?;
    }
    if request.preview {
        preserved_private(
            &paths,
            key.as_deref(),
            profile.as_deref(),
            catalog.as_deref(),
        )?;
        return Ok(Report {
            status: "preview-subscriptions",
            model_calls: 0,
            port: existing.as_ref().and_then(|value| value["port"].as_u64()),
            note: if paths.xai_profile_source.is_file() {
                if existing.as_ref().is_some_and(legacy_state) {
                    "Preview: legacy OpenCodex records will be retired; the native xAI profile is written without secrets."
                } else {
                    "Preview: kit-owned xAI profile source is present and contains no secrets."
                }
            } else {
                "Preview: native xAI profile source is absent from this checkout."
            },
        });
    }
    let mut pending_legacy = None;
    if let Some(state) = existing.clone().filter(|state: &Value| legacy_state(state)) {
        pending_legacy = Some(remove_legacy_opencodex(&paths, &state, &paths.pending)?);
    }
    let result = (|| -> io::Result<()> {
        write_xai_profile(&paths)?;
        write_json(&paths.state, &subscription_state_v2(&paths)?)?;
        Ok(())
    })();
    match (&result, pending_legacy) {
        (Ok(()), Some(pending)) => {
            let mut pending = pending;
            pending["state_after"] = snapshot(&paths.state)?
                .map(Value::String)
                .unwrap_or(Value::Null);
            write_json(&paths.pending, &pending)?;
            delete_regular(&paths.pending)?;
        }
        (Err(error), Some(pending)) => {
            let _ = restore_journaled_task(&pending, &paths.task);
            if let Some(path) = pending["links_before"]["configLink"]
                .as_str()
                .map(Path::new)
            {
                let _ = set_link(&paths.config_link, Some(path), None);
            }
            if let Some(path) = pending["links_before"]["roleLink"].as_str().map(Path::new) {
                let _ = set_link(&paths.role_link, Some(path), None);
            }
            let _ = restore_snapshot(
                &paths.state,
                &pending["state_before"],
                &Value::Null,
                &pending["state_before"],
            );
            return Err(io::Error::other(error.to_string()));
        }
        _ => {}
    }
    result?;
    preserved_private(
        &paths,
        key.as_deref(),
        profile.as_deref(),
        catalog.as_deref(),
    )?;
    Ok(Report {
        status: "connected-native",
        model_calls: 0,
        port: None,
        note: "Native xAI profile and catalog are installed. OpenCodex records were retired without starting or querying a proxy.",
    })
}

pub fn check(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let source = normal(&request.source)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    if present(&home.join("harness/subscription-restart-policy-pending.json"))? {
        return Err(conflict(
            "An interrupted restart policy update needs --subscriptions-only Recover.",
        ));
    }
    if present(&home.join("harness/subscription-routing-pending.json"))? {
        return Err(conflict(
            "An interrupted subscription operation needs Recover.",
        ));
    }
    let paths = paths(&source, &user, &home)?;
    if request.preview {
        return Ok(Report {
            status: "preview-subscriptions",
            model_calls: 0,
            port: None,
            note: if paths.xai_profile_source.is_file() {
                "Preview: kit-owned xAI profile source is present and contains no secrets."
            } else {
                "Preview: native xAI profile source is absent from this checkout."
            },
        });
    }
    let state = read_json(&paths.state)?;
    let Some(state) = state else {
        return Ok(Report {
            status: "disconnected",
            model_calls: 0,
            port: None,
            note: "No owned subscription records.",
        });
    };
    owned(&state, &paths)?;
    if legacy_state(&state) {
        return Ok(Report {
            status: "degraded",
            model_calls: 0,
            port: state["port"].as_u64(),
            note: "Legacy OpenCodex records are present; run Update to retire them and keep the native xAI profile.",
        });
    }
    Ok(Report {
        status: "connected",
        model_calls: 0,
        port: None,
        note: xai_profile_note(&paths)?,
    })
}

fn auth_helper(paths: &ServicePaths) -> io::Result<PathBuf> {
    let direct = paths.home.join("harness/bin/codex-harness.exe");
    if direct.is_file() {
        return Ok(direct);
    }
    let absent = || conflict("xAI auth helper executable is absent; connect the kit core first.");
    let installation = paths.home.join("harness/installation.json");
    let bytes = match fs::read(&installation) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(absent()),
        Err(error) => return Err(error),
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| conflict("Kit installation record is not JSON."))?;
    let bridge = value
        .get("configBridge")
        .and_then(Value::as_str)
        .ok_or_else(absent)?;
    let bridge = local_drive(Path::new(bridge))?;
    if !bridge.is_file() {
        return Err(conflict(
            "Recorded kit manager executable is absent; run kit Update.",
        ));
    }
    Ok(bridge)
}

fn write_xai_profile(paths: &ServicePaths) -> io::Result<()> {
    if !paths.xai_profile_source.is_file() || !paths.xai_catalog_source.is_file() {
        return Err(conflict("Native xAI profile source is absent."));
    }
    let home = path_text(&paths.home)?.replace('\\', "/");
    let manager = path_text(&auth_helper(paths)?)?.replace('\\', "/");
    let template = fs::read_to_string(&paths.xai_profile_source)?;
    if template.contains("experimental_bearer_token")
        || template.to_ascii_lowercase().contains("eyj")
    {
        return Err(conflict("xAI profile source must not contain secrets."));
    }
    let rendered = template
        .replace("{{CODEX_HOME}}", &home)
        .replace("{{HARNESS_MANAGER}}", &manager);
    if rendered.contains("{{CODEX_HOME}}") || rendered.contains("{{HARNESS_MANAGER}}") {
        return Err(conflict("xAI profile template was not fully rendered."));
    }
    fs::write(&paths.xai_profile, rendered)?;
    let expected = current_link(&paths.xai_catalog)?.map(|(path, _)| path);
    set_link(
        &paths.xai_catalog,
        Some(&paths.xai_catalog_source),
        expected.as_deref(),
    )
}

fn xai_profile_note(paths: &ServicePaths) -> io::Result<&'static str> {
    if !paths.xai_profile.is_file() {
        return Ok("xAI profile is not installed");
    }
    let text = fs::read_to_string(&paths.xai_profile)?;
    if text.contains("experimental_bearer_token") || text.to_ascii_lowercase().contains("eyj") {
        return Err(conflict("Installed xAI profile contains a secret."));
    }
    let Some(command) = installed_auth_helper(&text) else {
        return Err(conflict("Installed xAI profile has no auth command."));
    };
    if !Path::new(command).is_file() {
        return Ok("xAI auth helper executable is absent");
    }
    if current_link(&paths.xai_catalog)?.is_none() {
        return Ok("xAI catalog link is absent");
    }
    Ok("xAI profile is kit-owned and contains no secrets")
}

fn installed_auth_helper(text: &str) -> Option<&str> {
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("command = \"")
            .and_then(|rest| rest.strip_suffix('"'))
    })
}

fn profile_bytes(path: &Path) -> io::Result<Option<Vec<u8>>> {
    inventory::ordinary_parents(path)?;
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn unfinished_restart_blockers(home: &Path) -> io::Result<()> {
    for name in [
        "activation-pending.json",
        "pending.json",
        "code-tools-registration-pending.json",
        "code-tools-files-pending.json",
        "bootstrap-pending.json",
        "bootstrap-runtime-pending.json",
        "subscription-routing-pending.json",
    ] {
        if present(&home.join("harness").join(name))? {
            return Err(conflict(&format!(
                "An unfinished harness operation prevents restart policy changes: {name}. Run the owning Recover first."
            )));
        }
    }
    Ok(())
}

fn restore_restart_policy(
    request: &Request,
    paths: &ServicePaths,
    key: Option<&[u8]>,
    profile: Option<&[u8]>,
    catalog: Option<&[u8]>,
) -> io::Result<Report> {
    unfinished_restart_blockers(&paths.home)?;
    let Some(pending) = read_json(&paths.restart_pending)? else {
        return Err(conflict(
            "An interrupted restart policy update needs --subscriptions-only Recover.",
        ));
    };
    owned(&pending, paths)?;
    if pending["source"] != path_text(&paths.source)? || pending["operation"] != "restart-policy" {
        return Err(conflict(
            "Restart policy journal ownership mismatch; preserving.",
        ));
    }
    let observed = task_scheduler::observe(&paths.task)?;
    let Some(task) = observed.as_ref() else {
        return Err(conflict(
            "Subscription task changed during policy recovery; preserving.",
        ));
    };
    let before = pending["task_before"].as_str();
    let after = pending["task_after"].as_str();
    let matches_before = before == Some(task.xml.as_str());
    let matches_after = match after {
        Some(xml) => xml_matches(&task.xml, xml)?,
        None => false,
    };
    if !matches_before && !matches_after {
        return Err(conflict(
            "Subscription task changed during policy recovery; preserving.",
        ));
    }
    let current_state = snapshot(&paths.state)?;
    if current_state != optional_text(&pending["state_before"])?
        && current_state != optional_text(&pending["state_after"])?
    {
        return Err(conflict(
            "Subscription ownership state changed during policy recovery; preserving.",
        ));
    }
    if request.preview {
        preserved_private(paths, key, profile, catalog)?;
        return Ok(Report {
            status: "preview-subscription-restart-policy-recovery",
            model_calls: 0,
            port: pending["port"].as_u64(),
            note: "Preview does not inspect or stop the live routing proxy.",
        });
    }
    if !matches_before {
        let xml =
            before.ok_or_else(|| conflict("Restart policy journal is missing task_before."))?;
        let restored = task_scheduler::update_in_place(&paths.task, xml, &task.xml)?;
        if restored != xml {
            return Err(conflict(
                "Restored task serialization changed; preserving policy recovery evidence.",
            ));
        }
    }
    restore_snapshot(
        &paths.state,
        &pending["state_before"],
        &pending["state_after"],
        &pending["state_before"],
    )?;
    delete_regular(&paths.restart_pending)?;
    preserved_private(paths, key, profile, catalog)?;
    Ok(Report {
        status: "subscriptions-restart-policy-recovered",
        model_calls: 0,
        port: pending["port"].as_u64(),
        note: "Owned restart policy was restored in place. Live proxy was not queried or stopped.",
    })
}

pub fn configure_restart(_request: &Request) -> io::Result<RestartPolicyReport> {
    Err(conflict(
        "The OpenCodex restart policy is retired: the subscription lifecycle no longer manages a task or proxy.",
    ))
}

pub fn recover(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let source = normal(&request.source)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let paths = paths(&source, &user, &home)?;
    let key = profile_bytes(&paths.zai_key)?;
    let profile = profile_bytes(&paths.zai_profile)?;
    let catalog = profile_bytes(&paths.zai_catalog)?;
    if present(&paths.restart_pending)? {
        return restore_restart_policy(
            request,
            &paths,
            key.as_deref(),
            profile.as_deref(),
            catalog.as_deref(),
        );
    }
    let Some(pending) = read_json(&paths.pending)? else {
        return Err(conflict(
            "An interrupted subscription operation needs Recover.",
        ));
    };
    owned(&pending, &paths)?;
    if pending["source"] != path_text(&paths.source)? || pending["operation"] != "retire-opencodex"
    {
        return Err(conflict(
            "Subscription journal ownership mismatch; preserving.",
        ));
    }
    assert_journaled_task_restorable(&pending, &paths.task)?;
    if request.preview {
        preserved_private(
            &paths,
            key.as_deref(),
            profile.as_deref(),
            catalog.as_deref(),
        )?;
        return Ok(Report {
            status: "preview-subscription-recovery",
            model_calls: 0,
            port: None,
            note: "Preview restores the interrupted OpenCodex retirement journal without writes.",
        });
    }
    // Complete the retirement idempotently: legacy records removed, native
    // profile present, v2 state committed, journal consumed.
    task_scheduler::remove(&paths.task, pending["task_before"].as_str())?;
    if current_link(&paths.config_link)?.is_some() {
        set_link(
            &paths.config_link,
            None,
            pending["links_before"]["configLink"]
                .as_str()
                .map(Path::new),
        )?;
    }
    if current_link(&paths.role_link)?.is_some() {
        set_link(
            &paths.role_link,
            None,
            pending["links_before"]["roleLink"].as_str().map(Path::new),
        )?;
    }
    write_xai_profile(&paths)?;
    write_json(&paths.state, &subscription_state_v2(&paths)?)?;
    delete_regular(&paths.pending)?;
    preserved_private(
        &paths,
        key.as_deref(),
        profile.as_deref(),
        catalog.as_deref(),
    )?;
    Ok(Report {
        status: "subscriptions-recovered-native",
        model_calls: 0,
        port: None,
        note: "OpenCodex retirement completed and the native xAI profile is installed.",
    })
}

pub fn disconnect(request: &Request) -> io::Result<Report> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let source = normal(&request.source)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let paths = paths(&source, &user, &home)?;
    let key = profile_bytes(&paths.zai_key)?;
    let profile = profile_bytes(&paths.zai_profile)?;
    let catalog = profile_bytes(&paths.zai_catalog)?;
    if present(&paths.restart_pending)? {
        return Err(conflict(
            "An interrupted restart policy update needs --subscriptions-only Recover.",
        ));
    }
    if present(&paths.pending)? {
        return Err(conflict(
            "An interrupted subscription operation needs Recover.",
        ));
    }
    let state = read_json(&paths.state)?;
    if let Some(state) = &state {
        owned(state, &paths)?;
    }
    if request.preview {
        preserved_private(
            &paths,
            key.as_deref(),
            profile.as_deref(),
            catalog.as_deref(),
        )?;
        return Ok(Report {
            status: "preview-subscriptions",
            model_calls: 0,
            port: state.as_ref().and_then(|value| value["port"].as_u64()),
            note: "Preview removes the owned native profile and any legacy OpenCodex records.",
        });
    }
    let Some(state) = state else {
        preserved_private(
            &paths,
            key.as_deref(),
            profile.as_deref(),
            catalog.as_deref(),
        )?;
        return Ok(Report {
            status: "disconnected",
            model_calls: 0,
            port: None,
            note: "No owned subscription records.",
        });
    };
    let mut pending = serde_json::json!({
        "schema_version": 1,
        "owner": "codex-harness-subscriptions",
        "operation": "disconnect",
        "source": path_text(&paths.source)?,
        "user": path_text(&paths.user)?,
        "codex": path_text(&paths.home)?,
        "task": paths.task,
        "task_before": Value::Null,
        "task_after": Value::Null,
        "links_before": { "configLink": Value::Null, "roleLink": Value::Null },
        "links_after": { "configLink": Value::Null, "roleLink": Value::Null },
        "state_before": snapshot(&paths.state)?,
        "state_after": Value::Null
    });
    write_json(&paths.pending, &pending)?;
    if legacy_state(&state) {
        let observed = task_scheduler::observe(&paths.task)?;
        refuse_running(observed.as_ref())?;
        owned_task_present(observed.as_ref(), state["task_xml"].as_str())?;
        let task_before = observed.as_ref().map(|task| task.xml.clone());
        let expected_config = optional_path(&state["links"]["configLink"])?;
        let expected_role = optional_path(&state["links"]["roleLink"])?;
        let config_current = current_link(&paths.config_link)?;
        let role_current = current_link(&paths.role_link)?;
        match (config_current.as_ref(), expected_config.as_deref()) {
            (None, None) => {}
            (None, Some(_)) => {}
            (Some((actual, _)), Some(expected)) if same_target(actual, expected)? => {}
            _ => return Err(conflict("Foreign subscription source link preserved.")),
        }
        match (role_current.as_ref(), expected_role.as_deref()) {
            (None, None) => {}
            (None, Some(_)) => {}
            (Some((actual, _)), Some(expected)) if same_target(actual, expected)? => {}
            _ => return Err(conflict("Foreign subscription source link preserved.")),
        }
        pending["task_before"] = task_before.map(Value::String).unwrap_or(Value::Null);
        pending["links_before"]["configLink"] = expected_config
            .as_ref()
            .map(|path| path_text(path))
            .transpose()?
            .map(Value::String)
            .unwrap_or(Value::Null);
        pending["links_before"]["roleLink"] = expected_role
            .as_ref()
            .map(|path| path_text(path))
            .transpose()?
            .map(Value::String)
            .unwrap_or(Value::Null);
        write_json(&paths.pending, &pending)?;
        task_scheduler::remove(&paths.task, observed.as_ref().map(|task| task.xml.as_str()))?;
        if config_current.is_some() {
            set_link(&paths.config_link, None, expected_config.as_deref())?;
        }
        if role_current.is_some() {
            set_link(&paths.role_link, None, expected_role.as_deref())?;
        }
    }
    let expected_catalog = current_link(&paths.xai_catalog)?;
    match expected_catalog.as_ref() {
        None => {}
        Some((actual, _)) if same_target(actual, &paths.xai_catalog_source)? => {}
        _ => return Err(conflict("Foreign xAI catalog link preserved.")),
    }
    set_link(
        &paths.xai_catalog,
        None,
        expected_catalog.as_ref().map(|(path, _)| path.as_path()),
    )?;
    if paths.xai_profile.is_file() {
        delete_regular(&paths.xai_profile)?;
    }
    delete_regular(&paths.state)?;
    delete_regular(&paths.pending)?;
    preserved_private(
        &paths,
        key.as_deref(),
        profile.as_deref(),
        catalog.as_deref(),
    )?;
    Ok(Report {
        status: "disconnected",
        model_calls: 0,
        port: None,
        note: "Owned native subscription records and legacy OpenCodex leftovers were removed.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn missing_state_is_disconnected_without_creating_homes() {
        let root = tempfile::tempdir().unwrap();
        let report = check(&Request {
            source: root.path().join("source"),
            codex_home: root.path().join("codex"),
            user_home: root.path().join("user"),
            preview: false,
            manager: None,
        })
        .unwrap();
        assert_eq!(report.status, "disconnected");
        assert!(!root.path().join("codex/harness").exists());
    }

    #[test]
    fn pending_transaction_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        fs::create_dir_all(home.join("harness")).unwrap();
        fs::write(
            home.join("harness/subscription-routing-pending.json"),
            b"{\"owner\":\"codex-harness-subscriptions\"}",
        )
        .unwrap();
        let error = check(&Request {
            source: root.path().join("source"),
            codex_home: home,
            user_home: root.path().join("user"),
            preview: false,
            manager: None,
        })
        .unwrap_err();
        assert!(error.to_string().contains("interrupted subscription"));
    }

    fn request(root: &Path) -> Request {
        Request {
            source: root.join("source"),
            codex_home: root.join("codex"),
            user_home: root.join("user"),
            preview: false,
            manager: None,
        }
    }

    fn write_source(root: &Path) {
        let source = std::path::absolute(root).unwrap().join("source");
        let kit = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        fs::create_dir_all(source.join("global/opencodex/agents")).unwrap();
        // Legacy retirement fixtures: the retired kit sources are gone, so the
        // link targets only need to exist as ordinary files.
        fs::write(
            source.join("global/opencodex/config.json"),
            b"{\"hostname\":\"127.0.0.1\",\"port\":10100}",
        )
        .unwrap();
        fs::write(source.join("global/opencodex/agents/middle.toml"), b"role").unwrap();
        fs::create_dir_all(source.join("global/codex-profiles")).unwrap();
        fs::copy(
            kit.join("global/codex-profiles/xai.config.toml"),
            source.join("global/codex-profiles/xai.config.toml"),
        )
        .unwrap();
        fs::copy(
            kit.join("global/codex-profiles/xai.models.json"),
            source.join("global/codex-profiles/xai.models.json"),
        )
        .unwrap();
    }

    fn write_manager(root: &Path) -> PathBuf {
        let home = std::path::absolute(root).unwrap().join("codex");
        let manager = home.join("harness/bin/codex-harness.exe");
        fs::create_dir_all(manager.parent().unwrap()).unwrap();
        fs::write(&manager, b"fixture-manager").unwrap();
        manager
    }

    fn absolute_paths(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let base = std::path::absolute(root).unwrap();
        (base.join("codex"), base.join("user"), base.join("source"))
    }

    /// Owned scheduled tasks are machine state; retire this fixture's
    /// definition even when a test finishes before its own disconnect.
    struct TaskCleanup(String);
    impl Drop for TaskCleanup {
        fn drop(&mut self) {
            let _ = task_scheduler::remove(&self.0, None);
        }
    }

    fn legacy_installed(root: &Path) -> (PathBuf, Value, TaskCleanup) {
        write_source(root);
        let manager = write_manager(root);
        let (home, user, source) = absolute_paths(root);
        fs::create_dir_all(home.join("harness/subscriptions")).unwrap();
        fs::write(home.join("harness/subscriptions/zai-key.txt"), b"secret").unwrap();
        fs::write(home.join("zai.config.toml"), b"keep-profile").unwrap();
        fs::write(home.join("zai.models.json"), b"keep-catalog").unwrap();
        let identity =
            build_identity::hash_bytes(path_text(&home).unwrap().to_ascii_lowercase().as_bytes());
        let task = format!("codex-harness-subscriptions-{}", &identity[..16]);
        let service = home.join("harness/subscriptions/service.json");
        fs::write(&service, b"{}").unwrap();
        let xml = task_scheduler::xml(&task, &home, &source, &service, &manager).unwrap();
        let registered = task_scheduler::register(&task, &xml, None).unwrap();
        let config_source = source.join("global/opencodex/config.json");
        let role_source = source.join("global/opencodex/agents");
        fs::create_dir_all(user.join(".opencodex")).unwrap();
        fs::create_dir_all(home.join("agents")).unwrap();
        set_link(
            &user.join(".opencodex/config.json"),
            Some(&config_source),
            None,
        )
        .unwrap();
        set_link(
            &home.join("agents/codex-harness-subscriptions"),
            Some(&role_source),
            None,
        )
        .unwrap();
        let state = serde_json::json!({
            "schema_version": 1,
            "owner": "codex-harness-subscriptions",
            "source": path_text(&source).unwrap(),
            "user": path_text(&user).unwrap(),
            "codex": path_text(&home).unwrap(),
            "task": task,
            "task_xml": registered,
            "port": 10100,
            "links": {
                "configLink": path_text(&config_source).unwrap(),
                "roleLink": path_text(&role_source).unwrap()
            }
        });
        write_json(&home.join("harness/subscription-routing.json"), &state).unwrap();
        (
            home.join("harness/subscription-routing.json"),
            state,
            TaskCleanup(task),
        )
    }

    #[test]
    fn install_writes_native_profile_and_v2_state_without_opencodex() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        write_manager(root.path());
        let (home, user, source) = absolute_paths(root.path());
        fs::create_dir_all(home.join("harness/subscriptions")).unwrap();
        fs::write(home.join("harness/subscriptions/zai-key.txt"), b"secret").unwrap();
        let report = install(&request(root.path())).unwrap();
        assert_eq!(report.status, "connected-native");
        let profile = fs::read_to_string(home.join("xai.config.toml")).unwrap();
        assert!(profile.contains("http://127.0.0.1:56122/v1"));
        assert!(profile.contains("xai-token"));
        assert_eq!(
            fs::read_link(home.join("xai.models.json")).unwrap(),
            source.join("global/codex-profiles/xai.models.json")
        );
        let state: Value = serde_json::from_slice(
            &fs::read(home.join("harness/subscription-routing.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(state["schema_version"], 2);
        assert!(state.get("task").is_none());
        assert!(state.get("links").is_none());
        let identity =
            build_identity::hash_bytes(path_text(&home).unwrap().to_ascii_lowercase().as_bytes());
        let task = format!("codex-harness-subscriptions-{}", &identity[..16]);
        assert!(task_scheduler::observe(&task).unwrap().is_none());
        assert!(!user.join(".opencodex/config.json").exists());
        assert!(!home.join("agents/codex-harness-subscriptions").exists());
        assert_eq!(
            fs::read(home.join("harness/subscriptions/zai-key.txt")).unwrap(),
            b"secret"
        );
    }

    #[test]
    fn install_retires_legacy_opencodex_records() {
        let root = tempfile::tempdir().unwrap();
        let (state_path, state, _task) = legacy_installed(root.path());
        let (home, user, source) = absolute_paths(root.path());
        let report = install(&request(root.path())).unwrap();
        assert_eq!(report.status, "connected-native");
        assert!(
            task_scheduler::observe(state["task"].as_str().unwrap())
                .unwrap()
                .is_none()
        );
        assert!(!user.join(".opencodex/config.json").exists());
        assert!(!home.join("agents/codex-harness-subscriptions").exists());
        let after: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        assert_eq!(after["schema_version"], 2);
        assert!(home.join("xai.config.toml").is_file());
        assert!(
            !home
                .join("harness/subscription-routing-pending.json")
                .exists()
        );
        assert_eq!(
            fs::read(home.join("harness/subscriptions/zai-key.txt")).unwrap(),
            b"secret"
        );
        assert_eq!(
            fs::read(home.join("zai.config.toml")).unwrap(),
            b"keep-profile"
        );
        assert_eq!(
            fs::read(home.join("zai.models.json")).unwrap(),
            b"keep-catalog"
        );
        let _ = source;
    }

    #[test]
    fn install_preserves_foreign_config_link() {
        let root = tempfile::tempdir().unwrap();
        let (_, state, _task) = legacy_installed(root.path());
        let (_home, user, _source) = absolute_paths(root.path());
        let foreign = std::path::absolute(root.path())
            .unwrap()
            .join("foreign.json");
        fs::write(&foreign, b"{}").unwrap();
        fs::remove_file(user.join(".opencodex/config.json")).unwrap();
        set_link(&user.join(".opencodex/config.json"), Some(&foreign), None).unwrap();
        let error = install(&request(root.path())).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Foreign subscription source link")
        );
        assert!(user.join(".opencodex/config.json").exists());
        assert!(
            task_scheduler::observe(state["task"].as_str().unwrap())
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn install_refuses_xai_profile_without_any_auth_helper() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let error = install(&request(root.path())).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("xAI auth helper executable is absent")
        );
    }

    #[test]
    fn install_renders_recorded_bridge_when_native_helper_link_is_absent() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let (home, _user, source) = absolute_paths(root.path());
        let bridge = source.join("global/codex-profiles/bridge-fixture.exe");
        fs::write(&bridge, b"fixture-bridge").unwrap();
        fs::create_dir_all(home.join("harness")).unwrap();
        fs::write(
            home.join("harness/installation.json"),
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "configBridge": format!("\\\\?\\{}", bridge.display())
            }))
            .unwrap(),
        )
        .unwrap();
        let report = install(&request(root.path())).unwrap();
        assert_eq!(report.status, "connected-native");
        assert!(!home.join("harness/bin/codex-harness.exe").exists());
        let profile = fs::read_to_string(home.join("xai.config.toml")).unwrap();
        let expected = bridge.to_str().unwrap().replace('\\', "/");
        assert!(
            profile.contains(&format!("command = \"{expected}\"")),
            "{profile}"
        );
        let observed = check(&request(root.path())).unwrap();
        assert_eq!(observed.status, "connected");
    }

    #[test]
    fn preview_and_check_report_kit_owned_xai_profile_without_secrets() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        write_manager(root.path());
        let (home, _user, _source) = absolute_paths(root.path());
        let mut preview_request = request(root.path());
        preview_request.preview = true;
        let preview = install(&preview_request).unwrap();
        assert_eq!(preview.status, "preview-subscriptions");
        assert!(!home.join("xai.config.toml").exists());
        install(&request(root.path())).unwrap();
        let observed = check(&request(root.path())).unwrap();
        assert_eq!(observed.status, "connected");
        assert_eq!(
            observed.note,
            "xAI profile is kit-owned and contains no secrets"
        );
        let profile = fs::read_to_string(home.join("xai.config.toml")).unwrap();
        assert!(profile.contains("wire_api = \"responses\""));
        assert!(!profile.to_ascii_lowercase().contains("eyj"));
    }

    #[test]
    fn check_reports_legacy_state_as_degraded() {
        let root = tempfile::tempdir().unwrap();
        let (_, _, _task) = legacy_installed(root.path());
        let observed = check(&request(root.path())).unwrap();
        assert_eq!(observed.status, "degraded");
        assert_eq!(observed.port, Some(10100));
        assert!(observed.note.contains("Legacy OpenCodex records"));
    }

    #[test]
    fn recover_preview_preserves_pending_and_private_files() {
        let root = tempfile::tempdir().unwrap();
        let (_, state, _task) = legacy_installed(root.path());
        let (home, _user, _source) = absolute_paths(root.path());
        let pending_path = home.join("harness/subscription-routing-pending.json");
        let pending = serde_json::json!({
            "schema_version": 1,
            "owner": "codex-harness-subscriptions",
            "operation": "retire-opencodex",
            "source": state["source"],
            "user": state["user"],
            "codex": state["codex"],
            "task": state["task"],
            "task_before": state["task_xml"],
            "task_after": Value::Null,
            "links_before": state["links"],
            "links_after": { "configLink": Value::Null, "roleLink": Value::Null },
            "state_before": snapshot(&home.join("harness/subscription-routing.json")).unwrap(),
            "state_after": Value::Null
        });
        write_json(&pending_path, &pending).unwrap();
        let before = fs::read(&pending_path).unwrap();
        let mut preview_request = request(root.path());
        preview_request.preview = true;
        let report = recover(&preview_request).unwrap();
        assert_eq!(report.status, "preview-subscription-recovery");
        assert_eq!(fs::read(&pending_path).unwrap(), before);
        assert_eq!(
            fs::read(home.join("harness/subscriptions/zai-key.txt")).unwrap(),
            b"secret"
        );
    }

    #[test]
    fn recover_completes_retirement_journal() {
        let root = tempfile::tempdir().unwrap();
        let (_, state, _task) = legacy_installed(root.path());
        let (home, user, _source) = absolute_paths(root.path());
        let pending_path = home.join("harness/subscription-routing-pending.json");
        let pending = serde_json::json!({
            "schema_version": 1,
            "owner": "codex-harness-subscriptions",
            "operation": "retire-opencodex",
            "source": state["source"],
            "user": state["user"],
            "codex": state["codex"],
            "task": state["task"],
            "task_before": state["task_xml"],
            "task_after": Value::Null,
            "links_before": state["links"],
            "links_after": { "configLink": Value::Null, "roleLink": Value::Null },
            "state_before": snapshot(&home.join("harness/subscription-routing.json")).unwrap(),
            "state_after": Value::Null
        });
        write_json(&pending_path, &pending).unwrap();
        let report = recover(&request(root.path())).unwrap();
        assert_eq!(report.status, "subscriptions-recovered-native");
        assert!(!pending_path.exists());
        assert!(
            task_scheduler::observe(state["task"].as_str().unwrap())
                .unwrap()
                .is_none()
        );
        assert!(!user.join(".opencodex/config.json").exists());
        assert!(home.join("xai.config.toml").is_file());
        let after: Value = serde_json::from_slice(
            &fs::read(home.join("harness/subscription-routing.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(after["schema_version"], 2);
    }

    #[test]
    fn disconnect_removes_native_profile_and_legacy_records() {
        let root = tempfile::tempdir().unwrap();
        let (_, state, _task) = legacy_installed(root.path());
        let (home, user, _source) = absolute_paths(root.path());
        install(&request(root.path())).unwrap();
        let report = disconnect(&request(root.path())).unwrap();
        assert_eq!(report.status, "disconnected");
        assert!(!home.join("xai.config.toml").exists());
        assert!(!home.join("xai.models.json").exists());
        assert!(!home.join("harness/subscription-routing.json").exists());
        assert!(
            task_scheduler::observe(state["task"].as_str().unwrap())
                .unwrap()
                .is_none()
        );
        assert!(!user.join(".opencodex").join("config.json").exists());
        assert_eq!(
            fs::read(home.join("harness/subscriptions/zai-key.txt")).unwrap(),
            b"secret"
        );
        assert_eq!(
            fs::read(home.join("zai.config.toml")).unwrap(),
            b"keep-profile"
        );
    }

    #[test]
    fn disconnect_refuses_running_owned_task_without_stopping_it() {
        let root = tempfile::tempdir().unwrap();
        let (_, state, _task) = legacy_installed(root.path());
        let (home, _user, _source) = absolute_paths(root.path());
        let task = state["task"].as_str().unwrap().to_string();
        let xml = crate::task_scheduler::with_exec_action(
            state["task_xml"].as_str().unwrap(),
            r"C:\Windows\System32\ping.exe",
            "-n 30 127.0.0.1",
        )
        .unwrap();
        let registered = crate::task_scheduler::update_in_place(
            &task,
            &xml,
            state["task_xml"].as_str().unwrap(),
        )
        .unwrap();
        let mut state = state;
        state["task_xml"] = Value::String(registered);
        write_json(&home.join("harness/subscription-routing.json"), &state).unwrap();
        crate::task_scheduler::run(&task).unwrap();
        let started = std::time::Instant::now();
        while started.elapsed() < Duration::from_secs(8) {
            if crate::task_scheduler::observe(&task)
                .unwrap()
                .is_some_and(|task| task.running)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let error = disconnect(&request(root.path())).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Owned subscription task is running; live proxy was not stopped."),
            "{error}"
        );
        assert!(home.join("harness/subscription-routing.json").is_file());
        assert!(
            crate::task_scheduler::observe(&task)
                .unwrap()
                .is_some_and(|task| task.running)
        );
        crate::task_scheduler::stop(&task).unwrap();
        crate::task_scheduler::remove(&task, None).unwrap();
    }

    #[test]
    fn configure_restart_reports_retirement() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        write_manager(root.path());
        install(&request(root.path())).unwrap();
        let error = configure_restart(&request(root.path())).unwrap_err();
        assert!(error.to_string().contains("restart policy is retired"));
    }
}
