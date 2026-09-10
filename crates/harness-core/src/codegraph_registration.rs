//! Owned CodeGraph MCP registration journal for an explicit Codex home.
//! Install/Update retire only owned CBM entries. Check is read-only. Recover
//! restores interrupted activation. Disconnect removes the owned native block.
//! Callers pass an owned home; this module does not search for or mutate a
//! default global Codex home.
#![cfg(windows)]

use crate::{
    build_identity,
    dependency_discovery::local_path,
    inventory,
    registration_native::{FileGuard, StagedFile},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

const BEGIN: &str = "# BEGIN codex-harness MCP registrations\n";
const END: &str = "# END codex-harness MCP registrations\n";
const CBM: &str = "codebase-memory";
const GRAPH: &str = "codegraph";
const LSP: &str = "harness-lsp";
const READINESS: &str = "mcp_optional_startup_grace_ms";

#[path = "codegraph_registration_toml.rs"]
mod statements;

pub struct RegistrationRequest {
    pub codex_home: PathBuf,
    pub mode: String,
    pub command: Option<PathBuf>,
    pub package_root: Option<PathBuf>,
    pub defer_commit: bool,
    pub preview: bool,
    pub retained_registrations: Option<Value>,
}

pub fn apply(request: &RegistrationRequest) -> io::Result<Value> {
    if !matches!(
        request.mode.as_str(),
        "Install" | "Update" | "Check" | "Disconnect" | "Recover"
    ) {
        return Err(invalid("unsupported CodeGraph registration mode"));
    }
    let home = local_path(&request.codex_home)?;
    let config = home.join("config.toml");
    let state_path = home.join("harness/code-tools-registration.json");
    let pending_path = home.join("harness/code-tools-registration-pending.json");
    inventory::ordinary_parents(&config)?;
    inventory::ordinary_parents(&state_path)?;
    inventory::ordinary_parents(&pending_path)?;
    if request.mode == "Recover" {
        return recover(&config, &state_path, &pending_path, request.preview);
    }
    if pending_path.exists() {
        return Err(conflict(
            "Interrupted MCP registration: run install.ps1 -Mode Recover.",
        ));
    }
    let before = read_bytes(&config)?;
    let state_bytes = read_bytes(&state_path)?;
    let state = if state_bytes.is_empty() {
        json!({"schema_version": 1, "registrations": {}})
    } else {
        serde_json::from_slice(&state_bytes)?
    };
    if state["schema_version"] != 1 {
        return Err(conflict(
            "Unknown registration state schema; preserving configuration.",
        ));
    }
    let graph = codegraph_spec(request, &home)?;
    let desired = desired_registrations(
        &state,
        graph.as_ref(),
        request.mode.as_str(),
        request.retained_registrations.as_ref(),
    )?;
    let names = all_names(&state, &desired);
    let parsed = parse_config(&before)?;
    let servers = mcp_servers(&parsed);
    refuse_conflicts(&servers, &state, &names)?;
    let mut ops = operations(&servers, &desired, &names)?;
    let readiness = readiness_plan(&parsed, &state, &request.mode)?;
    if let Some(operation) = &readiness.operation {
        ops.push(operation.clone());
    }
    if request.mode == "Check" {
        return Ok(json!({
            "status": if ops.is_empty() { "connected" } else { "degraded" },
            "callable": Value::Null,
            "operations": ops,
            "note": "This checks registrations; real MCP calls are separate acceptance evidence.",
            "model_calls": 0
        }));
    }
    if request.preview {
        return Ok(json!({
            "status": if request.mode == "Disconnect" { "preview-disconnection" } else { "preview-registration" },
            "operations": ops,
            "mutated": false,
            "model_calls": 0
        }));
    }
    if ops.is_empty() {
        return Ok(json!({"status": "unchanged-registration", "model_calls": 0}));
    }
    let prepared = adjust_readiness(&before, readiness.target.as_ref())?;
    let untouched = without_owned(&prepared, &state)?;
    let block = render_block(&desired)?;
    let after = [untouched.as_slice(), block.as_slice()].concat();
    verify_result(&prepared, &after, &names, &desired)?;
    if read_bytes(&config)? != before {
        return Err(conflict(
            "Config changed during preparation; preserving concurrent changes.",
        ));
    }
    let after_state = if request.mode == "Disconnect" {
        Vec::new()
    } else {
        json_bytes(&json!({
            "schema_version": 1,
            "registrations": json_map(&desired),
            "block": String::from_utf8(block.clone()).map_err(|error| invalid(&error.to_string()))?,
            "connection_policy": readiness.policy
        }))?
    };
    let pending = json!({
        "schema_version": 1,
        "before": STANDARD.encode(&before),
        "after_hash": build_identity::hash_bytes(&after),
        "previous_state": if state_bytes.is_empty() { Value::Null } else { serde_json::from_slice::<Value>(&state_bytes)? },
        "previous_state_bytes": if state_bytes.is_empty() { Value::Null } else { Value::String(STANDARD.encode(&state_bytes)) },
        "after_state": if after_state.is_empty() { Value::Null } else { serde_json::from_slice::<Value>(&after_state)? },
        "after_state_hash": build_identity::hash_bytes(&after_state),
        "config_existed": config.is_file()
    });
    fs::create_dir_all(home.join("harness"))?;
    write_bytes(&pending_path, &json_bytes(&pending)?)?;
    let result = (|| {
        if read_bytes(&config)? != before {
            return Err(conflict(
                "Config changed before activation; preserving concurrent changes.",
            ));
        }
        replace_bytes(&config, &before, &after)?;
        if request.mode == "Disconnect" {
            if !state_bytes.is_empty() {
                delete_regular(&state_path)?;
            }
        } else {
            replace_bytes(&state_path, &state_bytes, &after_state)?;
        }
        if !request.defer_commit {
            delete_regular(&pending_path)?;
        }
        Ok(json!({
            "status": if request.mode == "Disconnect" { "disconnected" } else { "connected" },
            "servers": desired.keys().cloned().collect::<Vec<_>>(),
            "callable": Value::Null,
            "retired": if request.mode == "Disconnect" { Value::Array(vec![]) } else { json!([CBM]) },
            "model_calls": 0
        }))
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let _ = recover(&config, &state_path, &pending_path, false);
            Err(error)
        }
    }
}

fn recover(
    config: &Path,
    state_path: &Path,
    pending_path: &Path,
    preview: bool,
) -> io::Result<Value> {
    let Some(pending_bytes) = read_optional(pending_path)? else {
        return Ok(json!({"status": "no-pending-registration", "model_calls": 0}));
    };
    let pending: Value = serde_json::from_slice(&pending_bytes)?;
    let before = decode_b64(
        pending["before"]
            .as_str()
            .ok_or_else(|| invalid("pending before"))?,
    )?;
    let current = read_bytes(config)?;
    let after_hash = pending["after_hash"]
        .as_str()
        .ok_or_else(|| invalid("pending after_hash"))?;
    let mut previous_state = match pending.get("previous_state_bytes") {
        Some(Value::String(value)) => decode_b64(value)?,
        Some(Value::Null) | None => Vec::new(),
        Some(_) => return Err(invalid("pending previous_state_bytes")),
    };
    if previous_state.is_empty()
        && let Some(previous) = pending
            .get("previous_state")
            .filter(|value| value.is_object())
    {
        previous_state = json_bytes(previous)?;
    }
    let actual_state = read_bytes(state_path)?;
    let restored = restore_config(
        &current,
        &before,
        after_hash,
        &previous_state,
        &pending,
        &actual_state,
    )?;
    if let Some(expected) = pending["after_state_hash"].as_str()
        && build_identity::hash_bytes(&actual_state) != build_identity::hash_bytes(&previous_state)
        && build_identity::hash_bytes(&actual_state) != expected
    {
        return Err(conflict(
            "Registration state changed after interruption; preserving concurrent changes.",
        ));
    }
    if preview {
        return Ok(json!({"status": "preview-recovery", "config": config, "model_calls": 0}));
    }
    apply_restored_config(
        config,
        &current,
        &restored,
        pending["config_existed"] == false,
    )?;
    if pending
        .get("previous_state_bytes")
        .and_then(Value::as_str)
        .is_some()
        || !previous_state.is_empty()
    {
        replace_bytes(state_path, &read_bytes(state_path)?, &previous_state)?;
    } else if state_path.exists() {
        delete_regular(state_path)?;
    }
    delete_regular(pending_path)?;
    Ok(json!({"status": "registration-recovered", "model_calls": 0}))
}

fn restore_config(
    current: &[u8],
    before: &[u8],
    after_hash: &str,
    previous_state: &[u8],
    pending: &Value,
    actual_state: &[u8],
) -> io::Result<Vec<u8>> {
    if build_identity::hash_bytes(current) == build_identity::hash_bytes(before)
        || build_identity::hash_bytes(current) == after_hash
    {
        return Ok(before.to_vec());
    }
    let previous = if previous_state.is_empty() {
        json!({"schema_version": 1, "registrations": {}})
    } else {
        serde_json::from_slice(previous_state)?
    };
    let after_state = match pending.get("after_state") {
        Some(value) if !value.is_null() => value.clone(),
        _ => matching_after_state(actual_state, pending)?,
    };
    restore_owned_block(current, before, &previous, &after_state)
}

fn apply_restored_config(
    config: &Path,
    current: &[u8],
    restored: &[u8],
    config_existed: bool,
) -> io::Result<()> {
    if !config_existed && restored.is_empty() {
        if config.exists() {
            delete_regular(config)?;
        }
        return Ok(());
    }
    if current != restored {
        replace_bytes(config, current, restored)?;
    }
    Ok(())
}

fn restore_owned_block(
    current: &[u8],
    before: &[u8],
    previous: &Value,
    after_state: &Value,
) -> io::Result<Vec<u8>> {
    let parsed = parse_config(current)?;
    let servers = mcp_servers(&parsed);
    let mut names = owned_names(previous);
    names.extend(owned_names(after_state));
    for name in &names {
        let expected = after_state["registrations"]
            .get(name)
            .map(registration_table)
            .transpose()?
            .map(toml::Value::Table);
        if servers.get(name) != expected.as_ref() {
            return Err(conflict(
                "Owned MCP setting changed after interruption; preserving current settings.",
            ));
        }
    }
    let before_ready = parse_config(before)?.get(READINESS).cloned();
    let expected_ready = if readiness_policy(after_state)?.is_some() {
        Some(toml::Value::Integer(0))
    } else if readiness_policy(previous)?.is_some_and(|policy| policy["previous_present"] == false)
    {
        None
    } else {
        before_ready.clone()
    };
    if parsed.get(READINESS) != expected_ready.as_ref() {
        return Err(conflict(
            "Native MCP readiness changed after interruption; preserving current settings.",
        ));
    }
    let previous_block = owned_block(previous)?;
    let untouched = without_owned(current, after_state)?;
    let restored = [untouched.as_slice(), previous_block.as_slice()].concat();
    let desired: BTreeMap<String, Value> = previous["registrations"]
        .as_object()
        .ok_or_else(|| invalid("previous registrations"))?
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    verify_result(current, &restored, &names, &desired)?;
    adjust_readiness(&restored, before_ready.as_ref())
}

fn owned_block(state: &Value) -> io::Result<Vec<u8>> {
    match state.get("block") {
        Some(Value::String(block)) if !block.is_empty() => Ok(block.as_bytes().to_vec()),
        Some(Value::Null) | None => Ok(Vec::new()),
        Some(_) => Err(invalid("registration block")),
    }
}

fn matching_after_state(actual_state: &[u8], pending: &Value) -> io::Result<Value> {
    if actual_state.is_empty() {
        return Ok(json!({"schema_version": 1, "registrations": {}}));
    }
    if let Some(expected) = pending["after_state_hash"].as_str()
        && build_identity::hash_bytes(actual_state) != expected
    {
        return Err(conflict(
            "Owned MCP registration block changed after interruption; preserving concurrent changes.",
        ));
    }
    serde_json::from_slice(actual_state).map_err(|error| invalid(&error.to_string()))
}

fn codegraph_spec(request: &RegistrationRequest, home: &Path) -> io::Result<Option<Value>> {
    if request.mode == "Disconnect" {
        return Ok(None);
    }
    if request.mode == "Check" && request.command.is_none() && request.package_root.is_none() {
        return Ok(None);
    }
    let command = request
        .command
        .as_deref()
        .ok_or_else(|| invalid("CodeGraph registration needs --command"))?;
    let package = request
        .package_root
        .as_deref()
        .ok_or_else(|| invalid("CodeGraph registration needs --package-root"))?;
    Ok(Some(json!({
        "command": path_text(command)?,
        "args": ["mcp", "codegraph", "--package-root", path_text(package)?],
        "env": {"CODEX_HOME": path_text(home)?},
        "startup_timeout_sec": 30,
        "tool_timeout_sec": 660
    })))
}

fn desired_registrations(
    state: &Value,
    graph: Option<&Value>,
    mode: &str,
    retained: Option<&Value>,
) -> io::Result<BTreeMap<String, Value>> {
    let mut desired = BTreeMap::new();
    if mode == "Disconnect" {
        return Ok(desired);
    }
    if let Some(retained) = retained {
        for (name, spec) in retained_map(retained)? {
            if name == CBM || name == GRAPH || name == LSP {
                continue;
            }
            registration_table(spec)?;
            desired.insert(name.clone(), spec.clone());
        }
    } else if let Some(existing) = state["registrations"].as_object() {
        for (name, spec) in existing {
            if name != CBM {
                desired.insert(name.clone(), spec.clone());
            }
        }
    }
    if let Some(graph) = graph {
        desired.insert(GRAPH.into(), graph.clone());
    }
    Ok(desired)
}

fn retained_map(value: &Value) -> io::Result<&Map<String, Value>> {
    let Value::Object(map) = value else {
        return Err(invalid("retained registrations JSON"));
    };
    if let Some(registrations) = map.get("registrations") {
        return registrations
            .as_object()
            .ok_or_else(|| invalid("retained registrations JSON"));
    }
    Ok(map)
}

fn all_names(state: &Value, desired: &BTreeMap<String, Value>) -> BTreeSet<String> {
    let mut names = BTreeSet::from([
        CBM.into(),
        GRAPH.into(),
        LSP.into(),
        "serena".into(),
        "graphify".into(),
        "nuphus".into(),
    ]);
    if let Some(existing) = state["registrations"].as_object() {
        names.extend(existing.keys().cloned());
    }
    names.extend(desired.keys().cloned());
    names
}

fn refuse_conflicts(
    servers: &toml::Table,
    state: &Value,
    names: &BTreeSet<String>,
) -> io::Result<()> {
    for name in names {
        let Some(actual) = servers.get(name) else {
            continue;
        };
        let Some(old) = state["registrations"].get(name) else {
            return Err(conflict(&format!(
                "MCP name/ownership conflict: {name}; preserving current registration."
            )));
        };
        if actual != &toml::Value::Table(registration_table(old)?) {
            return Err(conflict(&format!(
                "MCP name/ownership conflict: {name}; preserving current registration."
            )));
        }
    }
    Ok(())
}

fn operations(
    servers: &toml::Table,
    desired: &BTreeMap<String, Value>,
    names: &BTreeSet<String>,
) -> io::Result<Vec<Value>> {
    let mut ops = Vec::new();
    for name in names {
        let actual = servers.get(name);
        let target = desired.get(name);
        let matches = match (actual, target) {
            (None, None) => true,
            (Some(actual), Some(spec)) => actual == &toml::Value::Table(registration_table(spec)?),
            _ => false,
        };
        if !matches {
            ops.push(json!({
                "name": name,
                "action": if target.is_none() { "remove" } else { "register" }
            }));
        }
    }
    Ok(ops)
}

struct ReadinessPlan {
    policy: Value,
    target: Option<toml::Value>,
    operation: Option<Value>,
}

fn readiness_policy(state: &Value) -> io::Result<Option<&Value>> {
    let Some(policy) = state
        .get("connection_policy")
        .filter(|value| !value.is_null())
    else {
        return Ok(None);
    };
    if policy["key"] != READINESS
        || policy["value"].as_i64() != Some(0)
        || !policy["previous_present"].is_boolean()
    {
        return Err(conflict(
            "Unknown native MCP readiness ownership; preserving configuration.",
        ));
    }
    Ok(Some(policy))
}

fn readiness_plan(parsed: &toml::Table, state: &Value, mode: &str) -> io::Result<ReadinessPlan> {
    let prior = readiness_policy(state)?;
    let actual = parsed.get(READINESS);
    let zero = toml::Value::Integer(0);
    if prior.is_some() && actual != Some(&zero) {
        return Err(conflict(
            "Native MCP readiness ownership conflict; preserving current settings.",
        ));
    }
    if mode == "Disconnect" {
        let target = if prior.is_some_and(|policy| policy["previous_present"] == false) {
            None
        } else {
            actual.cloned()
        };
        return Ok(ReadinessPlan {
            policy: Value::Null,
            target,
            operation: prior.map(|_| json!({"name": READINESS, "action": "restore"})),
        });
    }
    if actual.is_some() && actual != Some(&zero) {
        return Err(conflict(
            "Native MCP readiness conflict: explicit setting differs from 0; preserving configuration.",
        ));
    }
    Ok(ReadinessPlan {
        policy: prior.cloned().unwrap_or_else(|| {
            json!({
                "key": READINESS, "value": 0, "previous_present": actual.is_some()
            })
        }),
        target: Some(zero),
        operation: prior
            .is_none()
            .then(|| json!({"name": READINESS, "action": "register"})),
    })
}

fn adjust_readiness(before: &[u8], target: Option<&toml::Value>) -> io::Result<Vec<u8>> {
    let mut expected = parse_config(before)?;
    if expected.get(READINESS) == target {
        return Ok(before.to_vec());
    }
    let remaining = statements::remove(before, &BTreeSet::new(), Some(READINESS))?;
    expected.remove(READINESS);
    let after = if let Some(value) = target {
        expected.insert(READINESS.into(), value.clone());
        let mut assignment = toml::Table::new();
        assignment.insert(READINESS.into(), value.clone());
        let rendered =
            toml::to_string(&assignment).map_err(|_| invalid("native MCP readiness value"))?;
        let bom = if remaining.starts_with(&[0xef, 0xbb, 0xbf]) {
            3
        } else {
            0
        };
        [&remaining[..bom], rendered.as_bytes(), &remaining[bom..]].concat()
    } else {
        remaining
    };
    if parse_config(&after)? != expected {
        return Err(conflict(
            "Cannot change native MCP readiness without changing unrelated settings.",
        ));
    }
    Ok(after)
}

fn without_owned(before: &[u8], state: &Value) -> io::Result<Vec<u8>> {
    let owned = owned_names(state);
    if owned.is_empty() {
        return Ok(before.to_vec());
    }
    if let Some(block) = state["block"].as_str()
        && let Some(at) = find_unique(before, block.as_bytes()).ok().flatten()
    {
        let mut out = Vec::with_capacity(before.len() - block.len());
        out.extend_from_slice(&before[..at]);
        out.extend_from_slice(&before[at + block.len()..]);
        if verify_unrelated(before, &out, &owned).is_ok()
            && mcp_servers(&parse_config(&out)?)
                == unmanaged(&mcp_servers(&parse_config(before)?), &owned)
        {
            return Ok(out);
        }
    }
    let after = statements::remove(before, &owned, None)?;
    verify_unrelated(before, &after, &owned)?;
    if mcp_servers(&parse_config(&after)?)
        != unmanaged(&mcp_servers(&parse_config(before)?), &owned)
    {
        return Err(conflict(
            "Cannot isolate owned MCP statements; preserving configuration.",
        ));
    }
    Ok(after)
}

fn render_block(desired: &BTreeMap<String, Value>) -> io::Result<Vec<u8>> {
    if desired.is_empty() {
        return Ok(Vec::new());
    }
    let mut servers = toml::Table::new();
    for (name, spec) in desired {
        servers.insert(name.clone(), toml::Value::Table(registration_table(spec)?));
    }
    let mut document = toml::Table::new();
    document.insert("mcp_servers".into(), toml::Value::Table(servers));
    let rendered = toml::to_string(&document).map_err(|error| invalid(&error.to_string()))?;
    Ok(format!("\n{BEGIN}{rendered}{END}").into_bytes())
}

fn verify_result(
    before: &[u8],
    after: &[u8],
    names: &BTreeSet<String>,
    desired: &BTreeMap<String, Value>,
) -> io::Result<()> {
    verify_unrelated(before, after, names)?;
    let new_servers = mcp_servers(&parse_config(after)?);
    for name in names {
        let actual = new_servers.get(name);
        match desired.get(name) {
            None => {
                if actual.is_some() {
                    return Err(conflict(&format!(
                        "Native editor produced an unexpected registration for {name}."
                    )));
                }
            }
            Some(spec) => {
                if actual != Some(&toml::Value::Table(registration_table(spec)?)) {
                    return Err(conflict(&format!(
                        "Native editor produced an unexpected registration for {name}."
                    )));
                }
            }
        }
    }
    Ok(())
}

fn verify_unrelated(before: &[u8], after: &[u8], owned: &BTreeSet<String>) -> io::Result<()> {
    let (old_rest, old_servers) = split_servers(parse_config(before)?);
    let (new_rest, new_servers) = split_servers(parse_config(after)?);
    if old_rest != new_rest {
        return Err(conflict(
            "Native editor changed unrelated settings; live configuration unchanged.",
        ));
    }
    if unmanaged(&old_servers, owned) != unmanaged(&new_servers, owned) {
        return Err(conflict(
            "Native editor changed unrelated settings; live configuration unchanged.",
        ));
    }
    Ok(())
}

fn registration_table(value: &Value) -> io::Result<toml::Table> {
    let mut table = toml::Table::new();
    table.insert(
        "command".into(),
        toml::Value::String(text_field(value, "command")?),
    );
    let args = value["args"]
        .as_array()
        .ok_or_else(|| invalid("registration args"))?;
    let mut rendered = Vec::new();
    for arg in args {
        rendered.push(toml::Value::String(
            arg.as_str()
                .ok_or_else(|| invalid("registration arg"))?
                .into(),
        ));
    }
    table.insert("args".into(), toml::Value::Array(rendered));
    if let Some(env) = value.get("env") {
        let mut env_table = toml::Table::new();
        let object = env.as_object().ok_or_else(|| invalid("registration env"))?;
        for (key, item) in object {
            env_table.insert(
                key.clone(),
                toml::Value::String(
                    item.as_str()
                        .ok_or_else(|| invalid("registration env value"))?
                        .into(),
                ),
            );
        }
        table.insert("env".into(), toml::Value::Table(env_table));
    }
    for key in ["startup_timeout_sec", "tool_timeout_sec"] {
        if let Some(number) = value.get(key).and_then(Value::as_i64) {
            table.insert(key.into(), toml::Value::Integer(number));
        }
    }
    Ok(table)
}

fn parse_config(bytes: &[u8]) -> io::Result<toml::Table> {
    if bytes.is_empty() {
        return Ok(toml::Table::new());
    }
    let text = std::str::from_utf8(bytes).map_err(|error| invalid(&error.to_string()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.trim().is_empty() {
        return Ok(toml::Table::new());
    }
    toml::from_str(text)
        .map_err(|_| conflict("Cannot parse MCP configuration; preserving current settings."))
}

fn split_servers(mut document: toml::Table) -> (toml::Table, toml::Table) {
    let servers = mcp_servers(&document);
    document.remove("mcp_servers");
    (document, servers)
}

fn mcp_servers(document: &toml::Table) -> toml::Table {
    match document.get("mcp_servers") {
        Some(toml::Value::Table(table)) => table.clone(),
        _ => toml::Table::new(),
    }
}

fn unmanaged(servers: &toml::Table, owned: &BTreeSet<String>) -> toml::Table {
    servers
        .iter()
        .filter(|(name, _)| !owned.contains(name.as_str()))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

fn owned_names(state: &Value) -> BTreeSet<String> {
    state["registrations"]
        .as_object()
        .map(|values| values.keys().cloned().collect())
        .unwrap_or_default()
}

fn json_map(desired: &BTreeMap<String, Value>) -> Map<String, Value> {
    desired
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn find_unique(haystack: &[u8], needle: &[u8]) -> io::Result<Option<usize>> {
    if needle.is_empty() {
        return Ok(None);
    }
    let mut found = None;
    let mut offset = 0;
    while offset + needle.len() <= haystack.len() {
        if &haystack[offset..offset + needle.len()] == needle {
            if found.is_some() {
                return Err(conflict(
                    "Owned MCP registration block is not unique; preserving configuration.",
                ));
            }
            found = Some(offset);
            offset += needle.len();
        } else {
            offset += 1;
        }
    }
    Ok(found)
}

fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    match FileGuard::read_regular(path) {
        Ok((_guard, bytes)) => Ok(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn read_optional(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match FileGuard::read_regular(path) {
        Ok((_guard, bytes)) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace_bytes(path, &[], bytes)
}

fn replace_bytes(path: &Path, before: &[u8], after: &[u8]) -> io::Result<()> {
    match FileGuard::read_regular(path) {
        Ok((guard, current)) => {
            if current != before && !before.is_empty() {
                return Err(conflict(
                    "registration record bytes changed; preserving current settings.",
                ));
            }
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
    match FileGuard::read_regular(path) {
        Ok((guard, _)) => guard.remove(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
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

fn decode_b64(value: &str) -> io::Result<Vec<u8>> {
    STANDARD
        .decode(value.trim())
        .map_err(|error| invalid(&error.to_string()))
}

fn text_field(value: &Value, key: &str) -> io::Result<String> {
    value[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid(&format!("registration {key}")))
}

fn path_text(path: &Path) -> io::Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("path is not UTF-8"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}

fn conflict(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}
