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
struct Paths {
    source: PathBuf,
    user: PathBuf,
    home: PathBuf,
    state: PathBuf,
    pending: PathBuf,
    restart_pending: PathBuf,
    service: PathBuf,
    config: PathBuf,
    config_link: PathBuf,
    config_source: PathBuf,
    role_link: PathBuf,
    role_source: PathBuf,
    task: String,
    zai_key: PathBuf,
    zai_profile: PathBuf,
    zai_catalog: PathBuf,
}

fn paths(source: &Path, user: &Path, home: &Path) -> io::Result<Paths> {
    let identity = build_identity::hash_bytes(path_text(home)?.to_ascii_lowercase().as_bytes());
    Ok(Paths {
        source: source.to_path_buf(),
        user: user.to_path_buf(),
        home: home.to_path_buf(),
        state: home.join("harness/subscription-routing.json"),
        pending: home.join("harness/subscription-routing-pending.json"),
        restart_pending: home.join("harness/subscription-restart-policy-pending.json"),
        service: home.join("harness/subscriptions/service.json"),
        config: home.join("config.toml"),
        config_link: user.join(".opencodex/config.json"),
        config_source: source.join("global/opencodex/config.json"),
        role_link: home.join("agents/codex-harness-subscriptions"),
        role_source: source.join("global/opencodex/agents"),
        task: format!("codex-harness-subscriptions-{}", &identity[..16]),
        zai_key: home.join("harness/subscriptions/zai-key.txt"),
        zai_profile: home.join("zai.config.toml"),
        zai_catalog: home.join("zai.models.json"),
    })
}

fn owned(state: &Value, paths: &Paths) -> io::Result<()> {
    if state["schema_version"] != 1
        || state["owner"] != "codex-harness-subscriptions"
        || state["codex"] != path_text(&paths.home)?
        || state["user"] != path_text(&paths.user)?
        || state["task"] != paths.task
    {
        return Err(conflict(
            "Subscription ownership record mismatch; preserving state.",
        ));
    }
    Ok(())
}

fn preserved_private(
    paths: &Paths,
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

fn source_config(paths: &Paths) -> io::Result<Value> {
    let Some(config) = read_json(&paths.config_source)? else {
        return Err(conflict(
            "Subscription source must select loopback and preserve the ordinary Codex launcher.",
        ));
    };
    let hostname = config["hostname"].as_str();
    let port = config["port"].as_u64();
    if hostname != Some("127.0.0.1")
        || !matches!(port, Some(value) if (1024..=65535).contains(&value))
        || config["codexAutoStart"] != false
        || config["codexShimAutoRestore"] != false
    {
        return Err(conflict(
            "Subscription source must select loopback and preserve the ordinary Codex launcher.",
        ));
    }
    Ok(config)
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
            port: existing
                .as_ref()
                .and_then(|value| value["port"].as_u64())
                .or_else(|| {
                    read_json(&paths.config_source)
                        .ok()
                        .flatten()
                        .and_then(|config| config["port"].as_u64())
                }),
            note: "Preview does not inspect or stop the live routing proxy.",
        });
    }
    if !paths.role_source.is_dir() {
        return Err(conflict("Subscription role source is absent."));
    }
    let config = source_config(&paths)?;
    let port = config["port"].as_u64();
    let config_current = current_link(&paths.config_link)?;
    let role_current = current_link(&paths.role_link)?;
    let expected_config = existing
        .as_ref()
        .map(|state| optional_path(&state["links"]["configLink"]))
        .transpose()?
        .flatten();
    let expected_role = existing
        .as_ref()
        .map(|state| optional_path(&state["links"]["roleLink"]))
        .transpose()?
        .flatten();
    match (config_current.as_ref(), expected_config.as_deref()) {
        (None, None) => {}
        (Some((actual, _)), Some(expected)) if same_target(actual, expected)? => {}
        (Some((actual, _)), None) if same_target(actual, &paths.config_source)? => {}
        _ => return Err(conflict("Foreign subscription source link preserved.")),
    }
    match (role_current.as_ref(), expected_role.as_deref()) {
        (None, None) => {}
        (Some((actual, _)), Some(expected)) if same_target(actual, expected)? => {}
        (Some((actual, _)), None) if same_target(actual, &paths.role_source)? => {}
        _ => return Err(conflict("Foreign subscription source link preserved.")),
    }
    let observed = task_scheduler::observe(&paths.task)?;
    refuse_running(observed.as_ref())?;
    owned_task_present(
        observed.as_ref(),
        existing
            .as_ref()
            .and_then(|state| state["task_xml"].as_str()),
    )?;
    let task_before = observed.as_ref().map(|task| task.xml.clone());
    let descriptor = serde_json::json!({
        "schema_version": 1,
        "owner": "codex-harness-subscriptions",
        "source": path_text(&paths.source)?,
        "user": path_text(&paths.user)?,
        "codex": path_text(&paths.home)?,
        "task": paths.task,
        "port": port,
        "dependency": Value::Null,
        "links": {
            "configLink": path_text(&paths.config_source)?,
            "roleLink": path_text(&paths.role_source)?
        }
    });
    let pending = serde_json::json!({
        "schema_version": 1,
        "owner": "codex-harness-subscriptions",
        "source": path_text(&paths.source)?,
        "user": path_text(&paths.user)?,
        "codex": path_text(&paths.home)?,
        "task": paths.task,
        "port": port,
        "state_before": snapshot(&paths.state)?,
        "state_after": snapshot(&paths.state)?,
        "service_before": snapshot(&paths.service)?,
        "service_after": snapshot(&paths.service)?,
        "config_before": snapshot(&paths.config)?,
        "config_after": snapshot(&paths.config)?,
        "config_native": snapshot(&paths.config)?,
        "task_before": task_before,
        "task_after": task_before,
        "links_before": {
            "configLink": expected_config.as_ref().map(|path| path_text(path)).transpose()?,
            "roleLink": expected_role.as_ref().map(|path| path_text(path)).transpose()?
        },
        "links_after": {
            "configLink": path_text(&paths.config_source)?,
            "roleLink": path_text(&paths.role_source)?
        }
    });
    write_json(&paths.pending, &pending)?;
    if let Some(parent) = paths.config_link.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = paths.role_link.parent() {
        fs::create_dir_all(parent)?;
    }
    set_link(
        &paths.config_link,
        Some(&paths.config_source),
        expected_config
            .as_deref()
            .or(config_current.as_ref().map(|(path, _)| path.as_path())),
    )?;
    write_json(&paths.service, &descriptor)?;
    set_link(
        &paths.role_link,
        Some(&paths.role_source),
        expected_role
            .as_deref()
            .or(role_current.as_ref().map(|(path, _)| path.as_path())),
    )?;
    let powershell = task_scheduler::resolve_powershell()?;
    let desired_xml = task_scheduler::xml(
        &paths.task,
        &paths.home,
        &paths.source,
        &paths.service,
        &powershell,
    )?;
    let registered_xml =
        task_scheduler::register(&paths.task, &desired_xml, task_before.as_deref())?;
    let mut descriptor = descriptor;
    descriptor["task_xml"] = serde_json::Value::String(registered_xml);
    write_json(&paths.state, &descriptor)?;
    let mut pending = pending;
    pending["task_after"] = descriptor["task_xml"].clone();
    pending["state_after"] = snapshot(&paths.state)?.into();
    pending["service_after"] = snapshot(&paths.service)?.into();
    write_json(&paths.pending, &pending)?;
    delete_regular(&paths.pending)?;
    preserved_private(
        &paths,
        key.as_deref(),
        profile.as_deref(),
        catalog.as_deref(),
    )?;
    Ok(Report {
        status: "connected-files",
        model_calls: 0,
        port,
        note: "Owned routing files, source links and Task Scheduler definition were written. The live proxy was not queried or started.",
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
    if request.preview {
        return Ok(Report {
            status: "preview-subscriptions",
            model_calls: 0,
            port: None,
            note: "Preview does not inspect or stop the live routing proxy.",
        });
    }
    let Some(state) = read_json(&home.join("harness/subscription-routing.json"))? else {
        return Ok(Report {
            status: "disconnected",
            model_calls: 0,
            port: None,
            note: "No owned subscription records. Live proxy was not queried.",
        });
    };
    if state["owner"] != "codex-harness-subscriptions" {
        return Err(conflict("Foreign subscription ownership preserved."));
    }
    let config_source = source.join("global/opencodex/config.json");
    let role_source = source.join("global/opencodex/agents");
    if state["links"]["configLink"] != config_source.to_string_lossy().as_ref()
        && PathBuf::from(state["links"]["configLink"].as_str().unwrap_or_default()) != config_source
    {
        return Err(conflict("Foreign subscription source link preserved."));
    }
    if PathBuf::from(state["links"]["roleLink"].as_str().unwrap_or_default()) != role_source {
        return Err(conflict("Foreign subscription source link preserved."));
    }
    Ok(Report {
        status: "degraded",
        model_calls: 0,
        port: state["port"].as_u64(),
        note: "Owned records were checked; the live routing proxy was not queried or stopped.",
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
        "bootstrap-graphify-pending.json",
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
    paths: &Paths,
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

pub fn configure_restart(request: &Request) -> io::Result<RestartPolicyReport> {
    let home = normal(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let source = normal(&request.source)?;
    let _locks = InstallationLocks::acquire(&user, &user)?;
    let paths = paths(&source, &user, &home)?;
    let key = profile_bytes(&paths.zai_key)?;
    let profile = profile_bytes(&paths.zai_profile)?;
    let catalog = profile_bytes(&paths.zai_catalog)?;
    unfinished_restart_blockers(&home)?;
    if present(&paths.restart_pending)? {
        return Err(conflict(
            "An interrupted restart policy update needs --subscriptions-only Recover.",
        ));
    }
    let Some(mut state) = read_json(&paths.state)? else {
        return Err(conflict(
            "Restart policy requires a committed subscription installation.",
        ));
    };
    owned(&state, &paths)?;
    if state["source"] != path_text(&paths.source)? {
        return Err(conflict(
            "Subscription source changed; preserving restart policy.",
        ));
    }
    let observed = task_scheduler::observe(&paths.task)?;
    let Some(task) = observed.as_ref() else {
        return Err(conflict("Foreign or changed subscription task preserved."));
    };
    owned_task_present(Some(task), state["task_xml"].as_str())?;
    let xml = task_scheduler::with_restart_policy(&task.xml, 3, "PT1M")?;
    if request.preview {
        preserved_private(
            &paths,
            key.as_deref(),
            profile.as_deref(),
            catalog.as_deref(),
        )?;
        return Ok(RestartPolicyReport {
            status: "preview-subscription-restart-policy",
            model_calls: 0,
            port: state["port"].as_u64(),
            note: "Preview does not inspect or stop the live routing proxy.",
            task: paths.task.clone(),
            restart_count: 3,
            restart_interval: "PT1M",
            changed: xml != task.xml,
        });
    }
    if xml == task.xml {
        preserved_private(
            &paths,
            key.as_deref(),
            profile.as_deref(),
            catalog.as_deref(),
        )?;
        return Ok(RestartPolicyReport {
            status: "subscriptions-restart-policy-configured",
            model_calls: 0,
            port: state["port"].as_u64(),
            note: "Owned restart policy already matched; live proxy was not queried or stopped.",
            task: paths.task.clone(),
            restart_count: 3,
            restart_interval: "PT1M",
            changed: false,
        });
    }
    let before = snapshot(&paths.state)?;
    state["task_xml"] = Value::String(xml.clone());
    let after = STANDARD.encode(json_bytes(&state)?);
    let pending = serde_json::json!({
        "schema_version": 1,
        "owner": "codex-harness-subscriptions",
        "operation": "restart-policy",
        "source": path_text(&paths.source)?,
        "user": path_text(&paths.user)?,
        "codex": path_text(&paths.home)?,
        "task": paths.task,
        "port": state["port"],
        "task_before": task.xml,
        "task_after": xml,
        "state_before": before,
        "state_after": after
    });
    write_json(&paths.restart_pending, &pending)?;
    if snapshot(&paths.state)? != before {
        return Err(conflict(
            "Subscription ownership state changed before policy write; preserving.",
        ));
    }
    let registered = task_scheduler::update_in_place(&paths.task, &xml, &task.xml)?;
    state["task_xml"] = Value::String(registered.clone());
    let mut pending = pending;
    pending["task_after"] = Value::String(registered);
    pending["state_after"] = Value::String(STANDARD.encode(json_bytes(&state)?));
    write_json(&paths.restart_pending, &pending)?;
    write_json(&paths.state, &state)?;
    delete_regular(&paths.restart_pending)?;
    preserved_private(
        &paths,
        key.as_deref(),
        profile.as_deref(),
        catalog.as_deref(),
    )?;
    Ok(RestartPolicyReport {
        status: "subscriptions-restart-policy-configured",
        model_calls: 0,
        port: state["port"].as_u64(),
        note: "Owned restart policy was updated in place. Live proxy was not queried or stopped.",
        task: paths.task,
        restart_count: 3,
        restart_interval: "PT1M",
        changed: true,
    })
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
        preserved_private(
            &paths,
            key.as_deref(),
            profile.as_deref(),
            catalog.as_deref(),
        )?;
        return Ok(Report {
            status: "no-pending-subscriptions",
            model_calls: 0,
            port: None,
            note: "No owned pending journal. Live proxy was not queried or stopped.",
        });
    };
    owned(&pending, &paths)?;
    assert_journaled_task_restorable(&pending, &paths.task)?;
    if pending.get("phase").and_then(Value::as_str) == Some("recovered") {
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
                port: pending["port"].as_u64(),
                note: "Preview does not inspect or stop the live routing proxy.",
            });
        }
        delete_regular(&paths.pending)?;
        preserved_private(
            &paths,
            key.as_deref(),
            profile.as_deref(),
            catalog.as_deref(),
        )?;
        return Ok(Report {
            status: "subscriptions-recovered",
            model_calls: 0,
            port: pending["port"].as_u64(),
            note: "File-phase recovery completed without querying or stopping the live proxy.",
        });
    }
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
            port: pending["port"].as_u64(),
            note: "Preview does not inspect or stop the live routing proxy.",
        });
    }
    restore_snapshot(
        &paths.config,
        &pending["config_before"],
        &pending["config_after"],
        &pending["config_native"],
    )?;
    for name in ["configLink", "roleLink"] {
        let destination = if name == "configLink" {
            &paths.config_link
        } else {
            &paths.role_link
        };
        let expected = current_link(destination)?.map(|(target, _)| target);
        set_link(
            destination,
            optional_path(&pending["links_before"][name])?.as_deref(),
            expected.as_deref(),
        )?;
    }
    restore_snapshot(
        &paths.service,
        &pending["service_before"],
        &pending["service_after"],
        &pending["service_before"],
    )?;
    restore_snapshot(
        &paths.state,
        &pending["state_before"],
        &pending["state_after"],
        &pending["state_before"],
    )?;
    restore_journaled_task(&pending, &paths.task)?;
    let mut recovered = pending;
    recovered["phase"] = Value::String("recovered".into());
    write_json(&paths.pending, &recovered)?;
    delete_regular(&paths.pending)?;
    preserved_private(
        &paths,
        key.as_deref(),
        profile.as_deref(),
        catalog.as_deref(),
    )?;
    Ok(Report {
        status: "subscriptions-recovered",
        model_calls: 0,
        port: recovered["port"].as_u64(),
        note: "Owned files, source links and idle Task Scheduler definition were restored. Live proxy was not queried or stopped.",
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
            note: "Preview does not inspect or stop the live routing proxy.",
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
            note: "No owned subscription records. Live proxy was not queried or stopped.",
        });
    };
    let config_current = current_link(&paths.config_link)?;
    let role_current = current_link(&paths.role_link)?;
    let expected_config = optional_path(&state["links"]["configLink"])?;
    let expected_role = optional_path(&state["links"]["roleLink"])?;
    match (config_current.as_ref(), expected_config.as_deref()) {
        (None, None) => {}
        (Some((actual, _)), Some(expected)) if same_target(actual, expected)? => {}
        _ => return Err(conflict("Foreign subscription source link preserved.")),
    }
    match (role_current.as_ref(), expected_role.as_deref()) {
        (None, None) => {}
        (Some((actual, _)), Some(expected)) if same_target(actual, expected)? => {}
        _ => return Err(conflict("Foreign subscription source link preserved.")),
    }
    let observed = task_scheduler::observe(&paths.task)?;
    refuse_running(observed.as_ref())?;
    owned_task_present(observed.as_ref(), state["task_xml"].as_str())?;
    let task_before = observed.as_ref().map(|task| task.xml.clone());
    let pending = serde_json::json!({
        "schema_version": 1,
        "owner": "codex-harness-subscriptions",
        "source": path_text(&paths.source)?,
        "user": path_text(&paths.user)?,
        "codex": path_text(&paths.home)?,
        "task": paths.task,
        "port": state["port"],
        "state_before": snapshot(&paths.state)?,
        "state_after": Value::Null,
        "service_before": snapshot(&paths.service)?,
        "service_after": Value::Null,
        "config_before": snapshot(&paths.config)?,
        "config_after": snapshot(&paths.config)?,
        "config_native": snapshot(&paths.config)?,
        "task_before": task_before,
        "task_after": Value::Null,
        "links_before": {
            "configLink": expected_config.as_ref().map(|path| path_text(path)).transpose()?,
            "roleLink": expected_role.as_ref().map(|path| path_text(path)).transpose()?
        },
        "links_after": {
            "configLink": Value::Null,
            "roleLink": Value::Null
        }
    });
    write_json(&paths.pending, &pending)?;
    set_link(&paths.config_link, None, expected_config.as_deref())?;
    set_link(&paths.role_link, None, expected_role.as_deref())?;
    delete_regular(&paths.service)?;
    delete_regular(&paths.state)?;
    task_scheduler::remove(&paths.task, task_before.as_deref())?;
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
        port: state["port"].as_u64(),
        note: "Owned records, source links and idle Task Scheduler definition were removed. Live proxy was not queried or stopped.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_state_is_disconnected_without_creating_homes() {
        let root = tempfile::tempdir().unwrap();
        let report = check(&Request {
            source: root.path().join("source"),
            codex_home: root.path().join("codex"),
            user_home: root.path().join("user"),
            preview: false,
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
        }
    }

    fn owned_state(root: &Path) -> Value {
        let home = std::path::absolute(root).unwrap().join("codex");
        let user = std::path::absolute(root).unwrap().join("user");
        let source = std::path::absolute(root).unwrap().join("source");
        let identity =
            build_identity::hash_bytes(path_text(&home).unwrap().to_ascii_lowercase().as_bytes());
        serde_json::json!({
            "schema_version": 1,
            "owner": "codex-harness-subscriptions",
            "source": path_text(&source).unwrap(),
            "user": path_text(&user).unwrap(),
            "codex": path_text(&home).unwrap(),
            "task": format!("codex-harness-subscriptions-{}", &identity[..16]),
            "port": 10100,
            "links": {
                "configLink": path_text(&source.join("global/opencodex/config.json")).unwrap(),
                "roleLink": path_text(&source.join("global/opencodex/agents")).unwrap()
            }
        })
    }

    #[test]
    fn recover_preview_preserves_pending_and_private_files() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let source = std::path::absolute(root.path()).unwrap().join("source");
        fs::create_dir_all(home.join("harness/subscriptions")).unwrap();
        fs::create_dir_all(source.join("global/opencodex/agents")).unwrap();
        fs::write(home.join("harness/subscriptions/zai-key.txt"), b"secret").unwrap();
        let pending = home.join("harness/subscription-routing-pending.json");
        fs::write(
            &pending,
            serde_json::to_vec(&owned_state(root.path())).unwrap(),
        )
        .unwrap();
        let before = fs::read(&pending).unwrap();
        let report = recover(&Request {
            source,
            codex_home: home.clone(),
            user_home: user,
            preview: true,
        })
        .unwrap();
        assert_eq!(report.status, "preview-subscription-recovery");
        assert_eq!(fs::read(&pending).unwrap(), before);
        assert_eq!(
            fs::read(home.join("harness/subscriptions/zai-key.txt")).unwrap(),
            b"secret"
        );
    }

    #[test]
    fn disconnect_removes_owned_links_and_preserves_foreign_and_private_files() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let source = std::path::absolute(root.path()).unwrap().join("source");
        let config_source = source.join("global/opencodex/config.json");
        let role_source = source.join("global/opencodex/agents");
        fs::create_dir_all(&role_source).unwrap();
        fs::create_dir_all(home.join("agents")).unwrap();
        fs::create_dir_all(home.join("harness/subscriptions")).unwrap();
        fs::create_dir_all(user.join(".opencodex")).unwrap();
        fs::write(&config_source, br#"{"port":10100}"#).unwrap();
        std::os::windows::fs::symlink_file(&config_source, user.join(".opencodex/config.json"))
            .unwrap();
        std::os::windows::fs::symlink_dir(
            &role_source,
            home.join("agents/codex-harness-subscriptions"),
        )
        .unwrap();
        fs::write(home.join("harness/subscriptions/zai-key.txt"), b"keep-key").unwrap();
        fs::write(home.join("zai.config.toml"), b"keep-profile").unwrap();
        fs::write(home.join("config.toml"), b"foreign-config").unwrap();
        fs::write(
            home.join("harness/subscription-routing.json"),
            serde_json::to_vec_pretty(&owned_state(root.path())).unwrap(),
        )
        .unwrap();
        fs::write(
            home.join("harness/subscriptions/service.json"),
            b"{\"owner\":\"codex-harness-subscriptions\"}",
        )
        .unwrap();
        let report = disconnect(&Request {
            source,
            codex_home: home.clone(),
            user_home: user.clone(),
            preview: false,
        })
        .unwrap();
        assert_eq!(report.status, "disconnected");
        assert!(!user.join(".opencodex/config.json").exists());
        assert!(!home.join("agents/codex-harness-subscriptions").exists());
        assert!(!home.join("harness/subscription-routing.json").exists());
        assert!(
            !home
                .join("harness/subscription-routing-pending.json")
                .exists()
        );
        assert_eq!(
            fs::read(home.join("harness/subscriptions/zai-key.txt")).unwrap(),
            b"keep-key"
        );
        assert_eq!(
            fs::read(home.join("zai.config.toml")).unwrap(),
            b"keep-profile"
        );
        assert_eq!(
            fs::read(home.join("config.toml")).unwrap(),
            b"foreign-config"
        );
    }

    #[test]
    fn recover_restores_owned_files_from_pending_journal_without_touching_private_state() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let source = std::path::absolute(root.path()).unwrap().join("source");
        let config_source = source.join("global/opencodex/config.json");
        let role_source = source.join("global/opencodex/agents");
        fs::create_dir_all(&role_source).unwrap();
        fs::create_dir_all(home.join("agents")).unwrap();
        fs::create_dir_all(home.join("harness/subscriptions")).unwrap();
        fs::create_dir_all(user.join(".opencodex")).unwrap();
        fs::write(&config_source, b"source-config").unwrap();
        let previous = owned_state(root.path());
        let previous_bytes = serde_json::to_vec_pretty(&previous).unwrap();
        fs::write(home.join("config.toml"), b"after-config").unwrap();
        fs::write(home.join("harness/subscriptions/zai-key.txt"), b"secret").unwrap();
        std::os::windows::fs::symlink_file(&config_source, user.join(".opencodex/config.json"))
            .unwrap();
        let pending = serde_json::json!({
            "schema_version": 1,
            "owner": "codex-harness-subscriptions",
            "source": previous["source"],
            "user": previous["user"],
            "codex": previous["codex"],
            "task": previous["task"],
            "port": 10100,
            "state_before": STANDARD.encode(&previous_bytes),
            "state_after": Value::Null,
            "service_before": Value::Null,
            "service_after": Value::Null,
            "config_before": STANDARD.encode(b"before-config"),
            "config_after": STANDARD.encode(b"after-config"),
            "config_native": STANDARD.encode(b"after-config"),
            "links_before": {
                "configLink": Value::Null,
                "roleLink": Value::Null
            },
            "links_after": {
                "configLink": previous["links"]["configLink"].clone(),
                "roleLink": previous["links"]["roleLink"].clone()
            }
        });
        fs::write(
            home.join("harness/subscription-routing-pending.json"),
            serde_json::to_vec_pretty(&pending).unwrap(),
        )
        .unwrap();
        let report = recover(&Request {
            source,
            codex_home: home.clone(),
            user_home: user.clone(),
            preview: false,
        })
        .unwrap();
        assert_eq!(report.status, "subscriptions-recovered");
        assert_eq!(
            fs::read(home.join("config.toml")).unwrap(),
            b"before-config"
        );
        assert!(!user.join(".opencodex/config.json").exists());
        assert!(
            !home
                .join("harness/subscription-routing-pending.json")
                .exists()
        );
        let restored: Value = serde_json::from_slice(
            &fs::read(home.join("harness/subscription-routing.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(restored["owner"], "codex-harness-subscriptions");
        assert_eq!(
            fs::read(home.join("harness/subscriptions/zai-key.txt")).unwrap(),
            b"secret"
        );
        let _ = role_source;
    }

    #[test]
    fn recover_restores_idle_task_without_starting_live_proxy() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let source = std::path::absolute(root.path()).unwrap().join("source");
        let report = install(&Request {
            source: source.clone(),
            codex_home: home.clone(),
            user_home: user.clone(),
            preview: false,
        })
        .unwrap();
        assert_eq!(report.status, "connected-files");
        let state: Value = serde_json::from_slice(
            &fs::read(home.join("harness/subscription-routing.json")).unwrap(),
        )
        .unwrap();
        let task = state["task"].as_str().unwrap().to_string();
        let xml = state["task_xml"].as_str().unwrap().to_string();
        crate::task_scheduler::remove(&task, Some(&xml)).unwrap();
        assert!(crate::task_scheduler::observe(&task).unwrap().is_none());
        let pending = serde_json::json!({
            "schema_version": 1,
            "owner": "codex-harness-subscriptions",
            "source": state["source"],
            "user": state["user"],
            "codex": state["codex"],
            "task": task,
            "port": 10100,
            "state_before": snapshot(&home.join("harness/subscription-routing.json")).unwrap(),
            "state_after": snapshot(&home.join("harness/subscription-routing.json")).unwrap(),
            "service_before": snapshot(&home.join("harness/subscriptions/service.json")).unwrap(),
            "service_after": snapshot(&home.join("harness/subscriptions/service.json")).unwrap(),
            "config_before": Value::Null,
            "config_after": Value::Null,
            "config_native": Value::Null,
            "task_before": xml,
            "task_after": Value::Null,
            "links_before": state["links"].clone(),
            "links_after": state["links"].clone()
        });
        fs::write(
            home.join("harness/subscription-routing-pending.json"),
            serde_json::to_vec_pretty(&pending).unwrap(),
        )
        .unwrap();
        let recovered = recover(&Request {
            source,
            codex_home: home.clone(),
            user_home: user,
            preview: false,
        })
        .unwrap();
        assert_eq!(recovered.status, "subscriptions-recovered");
        let observed = crate::task_scheduler::observe(&task).unwrap();
        assert!(observed.as_ref().is_some_and(|task| !task.running));
        crate::task_scheduler::remove(&task, observed.as_ref().map(|task| task.xml.as_str()))
            .unwrap();
        assert!(crate::task_scheduler::observe(&task).unwrap().is_none());
    }

    #[test]
    fn foreign_link_is_preserved_on_disconnect() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let source = std::path::absolute(root.path()).unwrap().join("source");
        fs::create_dir_all(source.join("global/opencodex/agents")).unwrap();
        fs::create_dir_all(user.join(".opencodex")).unwrap();
        fs::create_dir_all(home.join("harness")).unwrap();
        fs::write(user.join(".opencodex/config.json"), b"foreign").unwrap();
        fs::write(
            home.join("harness/subscription-routing.json"),
            serde_json::to_vec_pretty(&owned_state(root.path())).unwrap(),
        )
        .unwrap();
        let error = disconnect(&request(root.path())).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Foreign subscription source link")
        );
        assert_eq!(
            fs::read(user.join(".opencodex/config.json")).unwrap(),
            b"foreign"
        );
        assert!(home.join("harness/subscription-routing.json").exists());
    }

    #[test]
    fn disconnect_preview_preserves_owned_records() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let source = std::path::absolute(root.path()).unwrap().join("source");
        fs::create_dir_all(source.join("global/opencodex/agents")).unwrap();
        fs::create_dir_all(home.join("harness")).unwrap();
        let state = home.join("harness/subscription-routing.json");
        fs::write(
            &state,
            serde_json::to_vec_pretty(&owned_state(root.path())).unwrap(),
        )
        .unwrap();
        let before = fs::read(&state).unwrap();
        let report = disconnect(&Request {
            source,
            codex_home: home.clone(),
            user_home: user,
            preview: true,
        })
        .unwrap();
        assert_eq!(report.status, "preview-subscriptions");
        assert_eq!(fs::read(&state).unwrap(), before);
        assert!(
            !home
                .join("harness/subscription-routing-pending.json")
                .exists()
        );
    }

    #[test]
    fn disconnect_refuses_pending_transaction() {
        let root = tempfile::tempdir().unwrap();
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        fs::create_dir_all(home.join("harness")).unwrap();
        let pending = home.join("harness/subscription-routing-pending.json");
        fs::write(&pending, b"{\"owner\":\"codex-harness-subscriptions\"}").unwrap();
        let before = fs::read(&pending).unwrap();
        let error = disconnect(&request(root.path())).unwrap_err();
        assert!(error.to_string().contains("interrupted subscription"));
        assert_eq!(fs::read(&pending).unwrap(), before);
    }

    #[test]
    fn recover_restores_restart_policy_in_place_without_stopping_proxy() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let source = std::path::absolute(root.path()).unwrap().join("source");
        let report = install(&Request {
            source: source.clone(),
            codex_home: home.clone(),
            user_home: user.clone(),
            preview: false,
        })
        .unwrap();
        assert_eq!(report.status, "connected-files");
        let state_path = home.join("harness/subscription-routing.json");
        let before_bytes = fs::read(&state_path).unwrap();
        let state: Value = serde_json::from_slice(&before_bytes).unwrap();
        let task = state["task"].as_str().unwrap().to_string();
        let xml = state["task_xml"].as_str().unwrap().to_string();
        let pending = serde_json::json!({
            "schema_version": 1,
            "owner": "codex-harness-subscriptions",
            "operation": "restart-policy",
            "source": state["source"],
            "user": state["user"],
            "codex": state["codex"],
            "task": task,
            "port": 10100,
            "task_before": xml,
            "task_after": xml,
            "state_before": STANDARD.encode(&before_bytes),
            "state_after": STANDARD.encode(&before_bytes)
        });
        fs::write(
            home.join("harness/subscription-restart-policy-pending.json"),
            serde_json::to_vec_pretty(&pending).unwrap(),
        )
        .unwrap();
        let preview = recover(&Request {
            source: source.clone(),
            codex_home: home.clone(),
            user_home: user.clone(),
            preview: true,
        })
        .unwrap();
        assert_eq!(
            preview.status,
            "preview-subscription-restart-policy-recovery"
        );
        assert!(
            home.join("harness/subscription-restart-policy-pending.json")
                .exists()
        );
        let recovered = recover(&Request {
            source,
            codex_home: home.clone(),
            user_home: user,
            preview: false,
        })
        .unwrap();
        assert_eq!(recovered.status, "subscriptions-restart-policy-recovered");
        assert!(
            !home
                .join("harness/subscription-restart-policy-pending.json")
                .exists()
        );
        assert_eq!(fs::read(&state_path).unwrap(), before_bytes);
        let observed = crate::task_scheduler::observe(&task).unwrap();
        assert!(observed.as_ref().is_some_and(|task| !task.running));
        crate::task_scheduler::remove(&task, observed.as_ref().map(|task| task.xml.as_str()))
            .unwrap();
    }

    #[test]
    fn configure_restart_updates_owned_policy_in_place_and_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let source = std::path::absolute(root.path()).unwrap().join("source");
        let request = Request {
            source: source.clone(),
            codex_home: home.clone(),
            user_home: user.clone(),
            preview: false,
        };
        assert_eq!(install(&request).unwrap().status, "connected-files");
        let state_path = home.join("harness/subscription-routing.json");
        let mut state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        let task = state["task"].as_str().unwrap().to_string();
        let xml = state["task_xml"].as_str().unwrap().to_string();
        let reduced = crate::task_scheduler::with_restart_policy(&xml, 1, "PT1M").unwrap();
        let registered = crate::task_scheduler::update_in_place(&task, &reduced, &xml).unwrap();
        state["task_xml"] = Value::String(registered);
        fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
        let preview = configure_restart(&Request {
            source: source.clone(),
            codex_home: home.clone(),
            user_home: user.clone(),
            preview: true,
        })
        .unwrap();
        assert_eq!(preview.status, "preview-subscription-restart-policy");
        assert!(preview.changed);
        assert_eq!(preview.restart_count, 3);
        let first = configure_restart(&request).unwrap();
        assert_eq!(first.status, "subscriptions-restart-policy-configured");
        assert!(first.changed);
        let second = configure_restart(&request).unwrap();
        assert_eq!(second.status, "subscriptions-restart-policy-configured");
        assert!(!second.changed);
        assert!(
            !home
                .join("harness/subscription-restart-policy-pending.json")
                .exists()
        );
        let observed = crate::task_scheduler::observe(&task).unwrap();
        assert!(observed.as_ref().is_some_and(|task| !task.running));
        crate::task_scheduler::remove(&task, observed.as_ref().map(|task| task.xml.as_str()))
            .unwrap();
    }

    #[test]
    fn configure_restart_refuses_foreign_task_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let source = std::path::absolute(root.path()).unwrap().join("source");
        let request = Request {
            source: source.clone(),
            codex_home: home.clone(),
            user_home: user.clone(),
            preview: false,
        };
        assert_eq!(install(&request).unwrap().status, "connected-files");
        let state_path = home.join("harness/subscription-routing.json");
        let mut state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
        let task = state["task"].as_str().unwrap().to_string();
        let xml = state["task_xml"].as_str().unwrap().to_string();
        state["task_xml"] = Value::String("foreign-task-xml".into());
        fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
        let before = fs::read(&state_path).unwrap();
        let error = configure_restart(&request).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Foreign or changed subscription task")
        );
        assert_eq!(fs::read(&state_path).unwrap(), before);
        assert!(
            !home
                .join("harness/subscription-restart-policy-pending.json")
                .exists()
        );
        crate::task_scheduler::remove(&task, Some(&xml)).unwrap();
    }

    fn write_source(root: &Path) {
        let source = std::path::absolute(root).unwrap().join("source");
        fs::create_dir_all(source.join("global/opencodex/agents")).unwrap();
        fs::write(
            source.join("global/opencodex/config.json"),
            br#"{"hostname":"127.0.0.1","port":10100,"codexAutoStart":false,"codexShimAutoRestore":false}"#,
        )
        .unwrap();
    }

    #[test]
    fn install_preview_does_not_create_homes_or_query_proxy() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let report = install(&Request {
            source: std::path::absolute(root.path()).unwrap().join("source"),
            codex_home: std::path::absolute(root.path()).unwrap().join("codex"),
            user_home: std::path::absolute(root.path()).unwrap().join("user"),
            preview: true,
        })
        .unwrap();
        assert_eq!(report.status, "preview-subscriptions");
        assert!(!root.path().join("codex/harness").exists());
        assert!(!root.path().join("user/.opencodex").exists());
    }

    #[test]
    fn install_writes_owned_files_and_preserves_private_and_foreign_config() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let home = std::path::absolute(root.path()).unwrap().join("codex");
        let user = std::path::absolute(root.path()).unwrap().join("user");
        let source = std::path::absolute(root.path()).unwrap().join("source");
        fs::create_dir_all(home.join("harness/subscriptions")).unwrap();
        fs::write(home.join("harness/subscriptions/zai-key.txt"), b"keep-key").unwrap();
        fs::write(home.join("config.toml"), b"foreign-config").unwrap();
        let report = install(&Request {
            source: source.clone(),
            codex_home: home.clone(),
            user_home: user.clone(),
            preview: false,
        })
        .unwrap();
        assert_eq!(report.status, "connected-files");
        assert_eq!(report.port, Some(10100));
        assert_eq!(
            fs::read_link(user.join(".opencodex/config.json")).unwrap(),
            source.join("global/opencodex/config.json")
        );
        assert_eq!(
            fs::read_link(home.join("agents/codex-harness-subscriptions")).unwrap(),
            source.join("global/opencodex/agents")
        );
        assert!(home.join("harness/subscription-routing.json").is_file());
        assert!(
            !home
                .join("harness/subscription-routing-pending.json")
                .exists()
        );
        assert_eq!(
            fs::read(home.join("harness/subscriptions/zai-key.txt")).unwrap(),
            b"keep-key"
        );
        assert_eq!(
            fs::read(home.join("config.toml")).unwrap(),
            b"foreign-config"
        );
        let state: Value = serde_json::from_slice(
            &fs::read(home.join("harness/subscription-routing.json")).unwrap(),
        )
        .unwrap();
        assert!(state["task_xml"].as_str().is_some());
        let observed = crate::task_scheduler::observe(state["task"].as_str().unwrap()).unwrap();
        assert!(observed.as_ref().is_some_and(|task| !task.running));
        let repeat = install(&Request {
            source: source.clone(),
            codex_home: home.clone(),
            user_home: user.clone(),
            preview: false,
        })
        .unwrap();
        assert_eq!(repeat.status, "connected-files");
        crate::task_scheduler::remove(state["task"].as_str().unwrap(), state["task_xml"].as_str())
            .unwrap();
        assert!(
            crate::task_scheduler::observe(state["task"].as_str().unwrap())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn install_preserves_foreign_config_link() {
        let root = tempfile::tempdir().unwrap();
        write_source(root.path());
        let user = std::path::absolute(root.path()).unwrap().join("user");
        fs::create_dir_all(user.join(".opencodex")).unwrap();
        fs::write(user.join(".opencodex/config.json"), b"foreign").unwrap();
        let before = fs::read(user.join(".opencodex/config.json")).unwrap();
        let error = install(&Request {
            source: std::path::absolute(root.path()).unwrap().join("source"),
            codex_home: std::path::absolute(root.path()).unwrap().join("codex"),
            user_home: user.clone(),
            preview: false,
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Foreign subscription source link")
        );
        assert_eq!(
            fs::read(user.join(".opencodex/config.json")).unwrap(),
            before
        );
        assert!(
            !std::path::absolute(root.path())
                .unwrap()
                .join("codex/harness/subscription-routing.json")
                .exists()
        );
    }
}
