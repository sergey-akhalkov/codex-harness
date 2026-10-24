//! Explicit prerequisite preparation for the independent Serena plugin.
//!
//! `inspect` is a read-only observation. `prepare` is the only entry point that
//! acquires packages, and it publishes the plugin registry only after the
//! prepared Serena and BasedPyright identities pass discovery.
#![cfg(windows)]
use crate::{
    dependency_discovery::{self, Request as DiscoveryRequest, local_path},
    dependency_runtime::{self, Needs, Runtime},
    native_build,
    registration_native::{FileGuard, StagedFile},
};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

pub const REGISTRY_NAME: &str = "code-tools.json";

#[derive(Clone, Debug)]
pub struct Request {
    pub catalogue: PathBuf,
    pub user_home: PathBuf,
    pub state: PathBuf,
}

pub fn registry_path(state: &Path) -> PathBuf {
    state.join(REGISTRY_NAME)
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Serena setup input or registry was rejected",
    )
}

fn effective_path() -> Option<OsString> {
    #[cfg(debug_assertions)]
    if let Some(path) = std::env::var_os("HARNESS_SERENA_SETUP_PATH") {
        return Some(path);
    }
    std::env::var_os("PATH")
}

fn find_node(path: Option<&OsString>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|part| part.is_absolute())
        .map(|part| part.join("node.exe"))
        .find(|candidate| candidate.is_file())
}

fn discovery_request(request: &Request, runtime: Option<&Runtime>) -> io::Result<DiscoveryRequest> {
    let mut discovery = DiscoveryRequest {
        catalogue: local_path(&request.catalogue)?,
        user_home: local_path(&request.user_home)?,
        full_records: true,
        ..DiscoveryRequest::default()
    };
    if let Some(runtime) = runtime {
        discovery.uv_tools_dir = runtime.uv_tools().map(Path::to_owned);
        if let Some(prefix) = runtime.npm_prefix() {
            discovery.npm_prefixes.push(prefix.to_owned());
        }
    }
    let mut paths = Vec::new();
    if let Some(node) = runtime.and_then(|runtime| runtime.node.as_deref())
        && let Some(parent) = node.parent()
    {
        paths.push(parent.to_owned());
    }
    if let Some(path) = effective_path() {
        paths.extend(std::env::split_paths(&path).filter(|part| part.is_absolute()));
    }
    if !paths.is_empty() {
        discovery.path = Some(std::env::join_paths(paths).map_err(io::Error::other)?);
    }
    Ok(discovery)
}

fn inventory(request: &Request, runtime: Option<&Runtime>) -> io::Result<Value> {
    dependency_discovery::discover(&discovery_request(request, runtime)?)
}

fn row<'a>(inventory: &'a Value, group: &str, id: &str) -> Option<&'a Value> {
    inventory[group]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["id"] == id)
}

fn adopted(record: Option<&Value>) -> bool {
    record.is_some_and(|record| {
        matches!(
            record["status"].as_str(),
            Some("adopted" | "present" | "installed" | "ready" | "verified")
        ) && record["version"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    })
}

fn file(value: &Value) -> Option<PathBuf> {
    let path = PathBuf::from(value.as_str()?);
    path.is_file().then_some(path)
}

fn identities(value: &Value) -> Value {
    let serena = row(value, "mcp", "serena");
    let python = row(value, "languages", "python");
    json!({
        "serena": serena.and_then(|row| row["version"].as_str()),
        "python": python.and_then(|row| row["version"].as_str()),
    })
}

fn registry_ready(value: &Value) -> bool {
    let serena = row(value, "mcp", "serena");
    let python = row(value, "languages", "python");
    let (Some(serena), Some(python)) = (serena, python) else {
        return false;
    };
    adopted(Some(serena))
        && file(&serena["paths"]["console_entrypoint"]).is_some()
        && adopted(Some(python))
        && python["serena_id"] == "python_basedpyright"
        && file(&python["paths"]["node"]).is_some()
        && file(&python["paths"]["entrypoint"]).is_some()
}

fn read_registry(state: &Path) -> io::Result<Option<Value>> {
    let path = registry_path(state);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(invalid());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| invalid())
}

/// One verified language row from the prepared registry, for the dependency
/// apply owner to reuse instead of staging a duplicate installation.
pub(crate) fn prepared_language(state: &Path, id: &str) -> io::Result<Option<Value>> {
    let Some(value) = read_registry(state)? else {
        return Ok(None);
    };
    if !registry_ready(&value) {
        return Ok(None);
    }
    Ok(row(&value, "languages", id).cloned())
}

fn missing(inventory: &Value) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if !adopted(row(inventory, "mcp", "serena")) {
        missing.push("serena");
    }
    if !adopted(row(inventory, "languages", "python")) {
        missing.push("basedpyright");
    }
    missing
}

fn ready_report(registry: &Path, value: &Value, mutated: bool) -> Value {
    json!({
        "schema_version": 1,
        "operation": "serena-setup-inspect",
        "status": "ready",
        "ready": true,
        "registry": registry,
        "identities": identities(value),
        "model_calls": 0,
        "mutated": mutated,
    })
}

/// Read-only readiness observation. This never creates state or acquires a package.
pub fn inspect(request: &Request) -> io::Result<Value> {
    let catalogue = local_path(&request.catalogue)?;
    if !catalogue.is_file() {
        return Err(invalid());
    }
    let state = local_path(&request.state)?;
    if let Some(value) = read_registry(&state)? {
        if registry_ready(&value) {
            return Ok(ready_report(&registry_path(&state), &value, false));
        }
        return Ok(json!({
            "schema_version": 1,
            "operation": "serena-setup-inspect",
            "status": "preparation-required",
            "ready": false,
            "registry": registry_path(&state),
            "reason": "The prepared registry is incomplete or its selected files changed; explicit preparation is required.",
            "model_calls": 0,
            "mutated": false,
        }));
    }
    let owned = dependency_runtime::prepared(
        &state,
        Needs {
            serena: true,
            python: true,
        },
    )?;
    let inventory = inventory(request, owned.as_ref())?;
    let missing = missing(&inventory);
    let reusable = missing.is_empty();
    Ok(json!({
        "schema_version": 1,
        "operation": "serena-setup-inspect",
        "status": if reusable { "reuse-available" } else { "preparation-required" },
        "ready": false,
        "registry": registry_path(&state),
        "reason": if reusable {
            "Compatible shared Serena and BasedPyright installations are available; explicit preparation will adopt them."
        } else {
            "Semantic prerequisites have not been prepared."
        },
        "missing": missing,
        "reusable": reusable,
        "model_calls": 0,
        "mutated": false,
    }))
}

fn registry_from_inventory(inventory: &Value) -> io::Result<Value> {
    let serena = row(inventory, "mcp", "serena").filter(|row| adopted(Some(row)));
    let python = row(inventory, "languages", "python").filter(|row| adopted(Some(row)));
    let (Some(serena), Some(_python)) = (serena, python) else {
        return Err(invalid());
    };
    let mut languages = Vec::new();
    for language in inventory["languages"].as_array().into_iter().flatten() {
        if adopted(Some(language)) && language["serena_id"].is_string() {
            languages.push(language.clone());
        }
    }
    if !languages.iter().any(|row| row["id"] == "python") {
        return Err(invalid());
    }
    Ok(json!({
        "schema_version": 1,
        "generated_by": "serena-setup",
        "catalogue": inventory["catalogue"],
        "mcp": [serena.clone()],
        "languages": languages,
    }))
}

fn validate_registry(value: &Value) -> io::Result<()> {
    let serena = row(value, "mcp", "serena").ok_or_else(invalid)?;
    let python = row(value, "languages", "python").ok_or_else(invalid)?;
    if !adopted(Some(serena))
        || !adopted(Some(python))
        || python["serena_id"] != "python_basedpyright"
        || file(&serena["paths"]["console_entrypoint"]).is_none()
        || file(&python["paths"]["node"]).is_none()
        || file(&python["paths"]["entrypoint"]).is_none()
    {
        return Err(invalid());
    }
    Ok(())
}

fn write_registry(state: &Path, bytes: &[u8]) -> io::Result<bool> {
    let path = registry_path(state);
    match FileGuard::read_regular(&path) {
        Ok((guard, before)) => {
            let identity = guard.object_identity()?;
            drop(guard);
            if before == bytes {
                return Ok(false);
            }
            FileGuard::replace_regular(&path, &identity, &before, bytes)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            StagedFile::create(&path, bytes)?.commit()?;
            Ok(true)
        }
        Err(error) => Err(error),
    }
}

fn prepared_report(
    registry: &Path,
    value: &Value,
    runtime: &Runtime,
    registry_changed: bool,
) -> Value {
    json!({
        "schema_version": 1,
        "operation": "serena-setup-prepare",
        "status": "ready",
        "ready": true,
        "registry": registry,
        "identities": identities(value),
        "model_calls": 0,
        "mutated": registry_changed || !runtime.acquired.is_empty(),
        "packages_acquired": !runtime.acquired.is_empty(),
        "acquired": runtime.acquired,
        "reused": runtime.reused,
        "prepared": {
            "uv": runtime.uv,
            "python": runtime.python,
            "node": runtime.node,
            "serena": runtime.serena,
            "serena_console": runtime.serena_console,
            "basedpyright": runtime.basedpyright,
        },
        "pinned": dependency_runtime::pinned(),
    })
}

/// Explicitly prepare or adopt the Serena and BasedPyright dependencies.
pub fn prepare(request: &Request) -> io::Result<Value> {
    let catalogue = local_path(&request.catalogue)?;
    if !catalogue.is_file() {
        return Err(invalid());
    }
    let state = local_path(&request.state)?;
    if let Some(value) = read_registry(&state)?
        && registry_ready(&value)
    {
        return Ok(prepared_report(
            &registry_path(&state),
            &value,
            &Runtime::default(),
            false,
        ));
    }
    let both = Needs {
        serena: true,
        python: true,
    };
    native_build::owner_root(&state)?;
    let _lock = native_build::lock_owned_state(&state)?;
    if let Some(value) = read_registry(&state)?
        && registry_ready(&value)
    {
        return Ok(prepared_report(
            &registry_path(&state),
            &value,
            &Runtime::default(),
            false,
        ));
    }

    let existing = dependency_runtime::prepared(&state, both)?;
    let observed = inventory(request, existing.as_ref())?;
    let ambient_node = find_node(effective_path().as_ref());
    let mut runtime = existing.unwrap_or_default();
    let needs = missing(&observed);
    if !needs.is_empty() {
        runtime = dependency_runtime::provision(
            &state,
            Needs {
                serena: needs.contains(&"serena"),
                python: needs.contains(&"basedpyright"),
            },
            ambient_node.as_deref(),
        )
        .map_err(|error| {
            io::Error::other(format!(
                "{error}; the previous registry and completed runtime pieces were preserved; correct the reported cause and retry explicit preparation"
            ))
        })?;
    }
    let final_inventory = inventory(request, Some(&runtime))?;
    let missing = missing(&final_inventory);
    if !missing.is_empty() {
        let detail = missing
            .iter()
            .map(|id| {
                let group = if *id == "serena" { "mcp" } else { "languages" };
                let record = row(&final_inventory, group, id);
                let status = record
                    .and_then(|row| row["status"].as_str())
                    .unwrap_or("missing");
                let path = record
                    .and_then(|row| {
                        row["paths"]["console_entrypoint"]
                            .as_str()
                            .or_else(|| row["paths"]["entrypoint"].as_str())
                    })
                    .unwrap_or("no-path");
                format!("{id}={status}:{path}")
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(io::Error::other(format!(
            "Serena preparation finished without an adopted Serena and BasedPyright identity ({detail}); the previous registry was preserved."
        )));
    }
    let value = registry_from_inventory(&final_inventory)?;
    validate_registry(&value)?;
    let bytes = serde_json::to_vec_pretty(&value)?;
    let changed = write_registry(&state, &bytes)?;
    Ok(prepared_report(
        &registry_path(&state),
        &value,
        &runtime,
        changed,
    ))
}
