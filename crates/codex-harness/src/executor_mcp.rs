//! The executor MCP slice: one dispatch tool whose authority is the Codex
//! request metadata and whose conversation runs on the calling lead's exact
//! app-server. Pool/profile/assignment preparation stays in the existing
//! executor owners; this module only validates the trusted attachment, starts
//! one thread, submits the brief and records the pool receipt.

use crate::{
    executor_assignment::Assignment,
    executor_cli,
    executor_cli::control::{
        self, BoundIdentity, ControlPaths, ControlPlan, Conversation, DEFAULT_BOUND, RegisteredLead,
    },
};
use harness_core::{
    mcp_session::{Operation, Session},
    mcp_stdio, native_launcher,
    process::Cancellation,
    task_worktree,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const TOOL: &str = "executor_dispatch";
const APPROVAL_POLICY: &str = "never";

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn excerpt(text: &str, limit: usize) -> String {
    text.chars()
        .take(limit)
        .collect::<String>()
        .replace(['\r', '\n', '\t'], " ")
}

/// Starts the stdio server for the installed plugin. It owns no app-server and
/// no conversation until a trusted request arrives.
pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    if args == ["--help"] {
        println!(
            "codex-harness mcp executors\nServe the installed executor MCP plugin over stdio. The caller identity comes only from Codex-injected request `_meta.threadId`; the tool resolves that lead's registered app-server endpoint and creates one executor thread there."
        );
        return Ok(0);
    }
    if !args.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid native MCP command options",
        ));
    }
    let (input, output) = mcp_stdio::standard_files()?;
    let cancellation = Cancellation::default();
    let session = Session::new("executors", vec![tool_definition()])?;
    mcp_stdio::serve_fallible_with_lifetime(
        session,
        input,
        output,
        &cancellation,
        None,
        |operation| Ok(tool_response(dispatch(&operation), &operation)),
    )?;
    Ok(0)
}

fn tool_definition() -> Value {
    json!({
        "name": TOOL,
        "description": "Dispatch one executor into a pooled worktree and a new thread on this lead's exact Codex app-server. The configured default executor profile, or an allowed requested profile, is used; no model, provider, effort, thread or endpoint argument is accepted.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "required": ["assignment"],
            "properties": {
                "owner": {
                    "type": "string",
                    "description": "Optional fresh run label (ASCII letters, digits, '-', '_' or '.', at most 128 characters). Omitted generates one from the caller suffix and a nanosecond timestamp."
                },
                "base": {
                    "type": "string",
                    "description": "Optional committed base. Omitted uses the consumer checkout's configured upstream."
                },
                "profile": {
                    "type": "string",
                    "description": "Optional allowed executor profile. Omitted uses the configured default."
                },
                "assignment": {
                    "type": "object",
                    "description": "Structured executor assignment, schema 1. Inputs are relative to the allocated pool slot.",
                    "additionalProperties": false,
                    "required": ["schema", "objective", "inputs", "outputs", "invariants", "acceptance"],
                    "properties": {
                        "schema": {"const": 1},
                        "objective": {"type": "string"},
                        "inputs": {"type": "array", "items": {"type": "string"}},
                        "outputs": {"type": "array", "items": {"type": "string"}},
                        "invariants": {"type": "array", "items": {"type": "string"}},
                        "acceptance": {"type": "array", "items": {"type": "string"}},
                        "consumer": {"type": "string"},
                        "purpose": {"type": "string"},
                        "board_task": {"type": "string"},
                        "escalate": {"type": "array", "items": {"type": "string"}}
                    }
                }
            }
        }
    })
}

fn tool_response(result: io::Result<Value>, operation: &Operation) -> Value {
    let (structured, is_error) = match result {
        Ok(value) => (value, false),
        Err(error) => (
            json!({
                "tool": operation.name,
                "error": error.to_string()
            }),
            true,
        ),
    };
    let text = serde_json::to_string_pretty(&structured)
        .unwrap_or_else(|_| "{\"error\":\"result serialization failed\"}".to_owned());
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": is_error
    })
}

/// A bounded owner for a caller that did not choose one. The caller's last
/// thread-id segment identifies the lead without copying the full identity;
/// the nanosecond timestamp keeps a fresh run distinct.
fn generated_owner(caller: &str) -> String {
    let suffix = caller.rsplit('-').next().unwrap_or("lead");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("mcp-{suffix}-{nanos}")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DispatchArgs {
    owner: Option<String>,
    assignment: Value,
    #[serde(default)]
    base: Option<String>,
    #[serde(default)]
    profile: Option<String>,
}

fn dispatch(operation: &Operation) -> io::Result<Value> {
    // Identity comes only from Codex's request metadata. Arguments are parsed
    // with deny_unknown_fields, so a caller-supplied lead/thread/run cannot
    // enter any authority decision.
    let caller = caller_thread(&operation.metadata)?;
    let args: DispatchArgs = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("executor_dispatch arguments are invalid: {error}")))?;
    let owner = args.owner.unwrap_or_else(|| generated_owner(&caller));
    let assignment = Assignment::from_value(args.assignment)?;
    let codex_home = native_launcher::codex_home()?;
    let endpoint = match control::registered_lead(&codex_home, &caller) {
        RegisteredLead::Live(endpoint) => endpoint,
        RegisteredLead::Unavailable(reason) => {
            return Err(invalid(format!(
                "{reason}; no app-server was contacted. Remedy: start or recover the lead with `codex-harness lead start`, then retry from that live lead session"
            )));
        }
    };

    let mut lead = Conversation::attach(&endpoint, DEFAULT_BOUND)?;
    let lead_thread = lead.read_thread_state(&caller)?;
    require_loaded_lead(&lead_thread, &caller)?;
    let cwd = lead_thread["cwd"]
        .as_str()
        .ok_or_else(|| invalid("the registered lead thread reports no working directory"))?;
    let source = crate::executor_cli::executor_input::source(None, &codex_home)?;
    let repository = repository_root(Path::new(cwd))?;
    let prepared = executor_cli::prepare_mcp_dispatch(
        &codex_home,
        &source,
        &repository,
        &owner,
        args.profile.as_deref(),
        args.base.as_deref(),
        assignment,
    )?;

    let mut overrides = parse_assignments(&prepared.profile_overrides)?;
    overrides.extend(parse_role_args(&prepared.role_args)?);
    overrides.insert("agents.enabled".into(), json!(false));
    let identity = BoundIdentity::resolve(&prepared.bound);
    let mut plan = ControlPlan::new(
        codex_home.join("harness/bin/codex.exe"),
        &codex_home,
        &prepared.checkout,
        prepared.title.clone(),
        identity.clone(),
        ControlPaths::for_slot(&codex_home, &prepared.repository, prepared.slot)?,
    );
    plan.approval_policy = Some(APPROVAL_POLICY.into());
    plan.overrides = overrides;
    if let Some(sandbox) = lead_thread["sandbox"].as_str() {
        plan.sandbox = Some(sandbox.to_owned());
    }

    let mut conversation = match Conversation::start_on_endpoint(&endpoint, &plan) {
        Ok(conversation) => conversation,
        Err(error) => return Err(prepared.release(error)),
    };
    let executor_thread = conversation.thread_id().to_owned();
    let thread = match conversation.read_thread_state(&executor_thread) {
        Ok(thread) => thread,
        Err(error) => {
            return Err(prepared.release(io::Error::new(
                error.kind(),
                format!("executor thread {executor_thread} could not be confirmed: {error}"),
            )));
        }
    };
    let observed = match confirm_started(
        &thread,
        &executor_thread,
        &prepared.checkout,
        &identity,
        &prepared.title,
    ) {
        Ok(status) => status,
        Err(error) => return Err(prepared.release(error)),
    };
    let turn = match conversation.assign(&prepared.prompt) {
        Ok(turn) => turn,
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "{error}; executor thread {executor_thread} was created and the pool claim is preserved because turn/start may have been accepted"
                ),
            ));
        }
    };
    let startup_release = task_worktree::release_startup_reservation(
        &codex_home,
        &prepared.repository,
        prepared.slot,
        &prepared.owner,
    )
    .err()
    .map(|error| error.to_string());
    let receipt = json!({
        "schema": 1,
        "run": prepared.owner,
        "leadThreadId": caller,
        "executorThreadId": executor_thread,
        "turnId": turn.turn_id,
        "title": prepared.title,
        "slot": prepared.slot,
        "source": prepared.source,
        "repository": prepared.repository,
        "poolSize": prepared.pool_size,
        "checkout": prepared.checkout,
        "base": prepared.base,
        "profile": identity,
        "observedStatus": observed,
        "allowProviderModelFallback": false,
        "startupReservationReleased": startup_release.is_none()
    });
    let receipt_error =
        control::write_staged(&prepared.receipt, &serde_json::to_vec_pretty(&receipt)?)
            .err()
            .map(|error| error.to_string());
    let mut result = receipt;
    result["receipt"] = json!(prepared.receipt);
    if let Some(error) = receipt_error {
        result["receiptError"] = json!(error);
    }
    if let Some(error) = startup_release {
        result["startupReleaseError"] = json!(error);
    }
    Ok(result)
}

fn caller_thread(metadata: &Value) -> io::Result<String> {
    let id = metadata
        .as_object()
        .and_then(|metadata| metadata.get("threadId"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            invalid(
                "the MCP request carries no `_meta.threadId`; refusing before any endpoint, lease, thread or model effect",
            )
        })?;
    if id.is_empty()
        || id.len() > 128
        || id != id.trim()
        || !id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(invalid(
            "the MCP request `_meta.threadId` is malformed; refusing before any endpoint, lease, thread or model effect",
        ));
    }
    Ok(id.to_owned())
}

fn require_loaded_lead(thread: &Value, caller: &str) -> io::Result<()> {
    let status = thread["status"]["type"].as_str().unwrap_or_default();
    if !matches!(status, "idle" | "active") {
        return Err(invalid(format!(
            "lead thread {caller} is {status} on the registered endpoint, not a loaded live thread; refusing to dispatch"
        )));
    }
    if thread["cwd"].as_str().is_none() {
        return Err(invalid(format!(
            "lead thread {caller} reports no working directory; refusing to choose a source checkout"
        )));
    }
    Ok(())
}

fn repository_root(cwd: &Path) -> io::Result<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|error| {
            invalid(format!(
                "the lead thread directory {} is not in an inspectable Git checkout: {error}",
                cwd.display()
            ))
        })?;
    if !output.status.success() {
        return Err(invalid(format!(
            "the lead thread directory {} is not a Git checkout: {}",
            cwd.display(),
            excerpt(&String::from_utf8_lossy(&output.stderr), 300)
        )));
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| invalid("git returned a non-UTF-8 repository root"))?;
    let root = PathBuf::from(text.trim());
    if !root.is_dir() {
        return Err(invalid(format!(
            "git reported repository root {} but it is not a directory",
            root.display()
        )));
    }
    root.canonicalize()
}

fn parse_assignments(entries: &[String]) -> io::Result<BTreeMap<String, Value>> {
    entries
        .iter()
        .map(|entry| parse_assignment(entry))
        .collect()
}

fn parse_role_args(args: &[String]) -> io::Result<BTreeMap<String, Value>> {
    let mut parsed = BTreeMap::new();
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = match flag.as_str() {
            "-c" | "--config" => args.next().ok_or_else(|| {
                invalid("the executor role carries a config flag without a value")
            })?,
            other if other.starts_with("-c") && other.len() > 2 => &other[2..],
            other => {
                return Err(invalid(format!(
                    "the executor role carries unsupported launcher argument '{other}'"
                )));
            }
        };
        let (key, value) = parse_assignment(value)?;
        parsed.insert(key, value);
    }
    Ok(parsed)
}

fn parse_assignment(entry: &str) -> io::Result<(String, Value)> {
    let (key, value) = entry.split_once('=').ok_or_else(|| {
        invalid("an executor configuration override is not an exact key=value pair")
    })?;
    if key.is_empty() {
        return Err(invalid("an executor configuration override has no key"));
    }
    let document: toml::Table = toml::from_str(&format!("value = {value}")).map_err(|error| {
        invalid(format!(
            "executor configuration '{key}' is invalid: {error}"
        ))
    })?;
    let value = serde_json::to_value(
        document
            .get("value")
            .ok_or_else(|| invalid("executor configuration override is empty"))?,
    )?;
    Ok((key.to_owned(), value))
}

fn confirm_started(
    thread: &Value,
    expected_thread: &str,
    expected_cwd: &Path,
    identity: &BoundIdentity,
    expected_title: &str,
) -> io::Result<String> {
    if thread["id"].as_str() != Some(expected_thread) {
        return Err(invalid(format!(
            "thread/read answered for {} instead of the executor thread {expected_thread}",
            thread["id"]
        )));
    }
    if let Err(mismatch) = identity.verify(thread) {
        return Err(invalid(format!(
            "the executor thread did not preserve the configured ds routing: {mismatch}; no turn was started"
        )));
    }
    if thread["name"].as_str() != Some(expected_title) {
        return Err(invalid(format!(
            "the executor thread is named {} instead of '{}'; no turn was started",
            thread["name"], expected_title
        )));
    }
    let cwd = thread["cwd"]
        .as_str()
        .ok_or_else(|| invalid("the executor thread reports no working directory"))?;
    let actual = Path::new(cwd).canonicalize().map_err(|error| {
        invalid(format!(
            "the executor thread directory {cwd} cannot be resolved: {error}"
        ))
    })?;
    let expected = expected_cwd.canonicalize().map_err(|error| {
        invalid(format!(
            "the allocated checkout {} cannot be resolved: {error}",
            expected_cwd.display()
        ))
    })?;
    if actual != expected {
        return Err(invalid(format!(
            "the executor thread runs in {} instead of the allocated checkout {}; no turn was started",
            actual.display(),
            expected.display()
        )));
    }
    let status = thread["status"]["type"].as_str().unwrap_or_default();
    if !matches!(status, "idle" | "active") {
        return Err(invalid(format!(
            "the executor thread reports status '{status}', not a loaded thread; no turn was started"
        )));
    }
    Ok(status.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_owner_uses_the_caller_suffix_and_stays_bounded() {
        let first = generated_owner("01a00000-0000-7000-8000-0000000000ab");
        std::thread::sleep(std::time::Duration::from_millis(1));
        let second = generated_owner("01a00000-0000-7000-8000-0000000000ab");
        assert!(first.starts_with("mcp-0000000000ab-"), "{first}");
        assert!(first.len() <= 128, "{first}");
        assert!(
            first
                .chars()
                .all(|character| character.is_ascii_alphanumeric()
                    || matches!(character, '-' | '_' | '.'))
        );
        assert_ne!(first, second);
    }
}
