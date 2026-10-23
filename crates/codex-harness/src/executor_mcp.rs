//! The executor MCP surface: dispatch, bounded status, native message/reply,
//! isolated stop and explicit continuation. Authority is Codex request
//! `_meta.threadId`; runs are resolved from the existing MCP receipt and its
//! slot/app-server lease. Dispatch, delivery and stop containment stay in the
//! existing executor owners; this module selects and validates the binding.

use crate::{
    executor_assignment::Assignment,
    executor_cli::control::{
        self, BoundIdentity, ControlPaths, ControlPlan, Conversation, DEFAULT_BOUND, Endpoint,
        EndpointProcess, RegisteredLead, active_turn,
    },
    executor_cli::{
        self,
        executor_input::{self, Delivery as ComposedDelivery},
        executor_message::{self, ExactDelivery, KIND_NOTICE, KIND_REQUEST, PAYLOAD_MARK},
    },
};
use harness_core::{
    mcp_session::{Operation, Session},
    mcp_stdio, native_launcher,
    process::Cancellation,
    task_worktree::{self, SlotState},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const DISPATCH_TOOL: &str = "executor_dispatch";
const STATUS_TOOL: &str = "executor_status";
const MESSAGE_TOOL: &str = "executor_message";
const REPLY_TOOL: &str = "executor_reply";
const ASK_TOOL: &str = "executor_ask_lead";
const STOP_TOOL: &str = "executor_stop";
const RESUME_TOOL: &str = "executor_resume";
const APPROVAL_POLICY: &str = "never";
const MAX_RECEIPTS: usize = 256;
const MAX_STATUS_RUNS: usize = 8;
const MAX_STATUS_REQUESTS: usize = 16;
const MAX_PROGRESS_BYTES: usize = 4 * 1024;

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
            "codex-harness mcp executors\nServe the installed executor MCP plugin over stdio. The caller identity comes only from Codex-injected request `_meta.threadId`. Tools dispatch one executor on the originating app-server, exchange addressed messages or questions, read bounded status, stop one exact run through native turn interruption, and explicitly continue that run. No run, thread, endpoint or recipient is accepted from tool arguments as authority."
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
    let session = Session::new(
        "executors",
        vec![
            dispatch_tool_definition(),
            status_tool_definition(),
            message_tool_definition(),
            reply_tool_definition(),
            ask_lead_tool_definition(),
            stop_tool_definition(),
            resume_tool_definition(),
        ],
    )?;
    mcp_stdio::serve_fallible_with_lifetime(
        session,
        input,
        output,
        &cancellation,
        None,
        |operation| Ok(tool_response(perform(&operation), &operation)),
    )?;
    Ok(0)
}

fn dispatch_tool_definition() -> Value {
    json!({
        "name": DISPATCH_TOOL,
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

fn status_tool_definition() -> Value {
    json!({
        "name": STATUS_TOOL,
        "description": "Read bounded native run, thread, slot and pending-request status for the verified calling thread. No identity argument is accepted.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "properties": {}
        }
    })
}

fn message_tool_definition() -> Value {
    json!({
        "name": MESSAGE_TOOL,
        "description": "Send one literal message over the native conversation. A lead addresses its sole recorded run, an exact run id returned by executor_status, or a run identified by requestId. If both run and requestId are supplied they must name the same run. An executor addresses only its originating lead. The clientMessageId makes retries idempotent; omit it for a new distinct message.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "required": ["text"],
            "properties": {
                "text": {"type": "string", "description": "Complete literal UTF-8 message."},
                "clientMessageId": {
                    "type": "string",
                    "description": "Optional caller-generated id, at most 128 ASCII letters, digits, '-', '_' or '.'. Reuse it only to reconcile the same payload."
                },
                "run": {
                    "type": "string",
                    "description": "Optional exact run id returned by executor_status. It is resolved only among the calling lead's verified runs; when requestId is also supplied both must name the same run."
                },
                "requestId": {
                    "type": "string",
                    "description": "Optional recorded request reference that identifies the lead's exact run. It is a correlation reference, not sender authority."
                }
            }
        }
    })
}

fn reply_tool_definition() -> Value {
    json!({
        "name": REPLY_TOOL,
        "description": "Answer one executor question by its recorded request id. The reply goes only to the exact executor run and thread that recorded the request; it starts no new conversation.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "required": ["requestId", "text"],
            "properties": {
                "requestId": {"type": "string", "description": "Request id returned by executor_ask_lead."},
                "text": {"type": "string", "description": "Complete literal UTF-8 answer."}
            }
        }
    })
}

fn ask_lead_tool_definition() -> Value {
    json!({
        "name": ASK_TOOL,
        "description": "Ask the originating lead one reply-required question through the same native app-server. Returns a stable request id without waiting for the answer. An omitted clientMessageId makes identical text a retry of the same request; a new id makes otherwise identical text a distinct question. Only an executor thread may call this tool.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "required": ["text"],
            "properties": {
                "text": {"type": "string", "description": "Complete literal UTF-8 question."},
                "clientMessageId": {
                    "type": "string",
                    "description": "Optional caller-generated id, at most 128 ASCII letters, digits, '-', '_' or '.'. Reuse it only for the same question. When omitted, identical question text on the same run is retry-stable; supply a new id to ask otherwise-identical text as a distinct question."
                }
            }
        }
    })
}

fn stop_tool_definition() -> Value {
    json!({
        "name": STOP_TOOL,
        "description": "Stop one executor run owned by the verified calling lead. Interrupts only the exact executor turn through the shared app-server, terminates only that thread's native background commands, verifies their inventory disappears, and preserves the checkout, slot, receipt and lease. The run's owned child command is not reported gone unless the exact-thread inventory proves it. No shared app-server or sibling conversation is targeted.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "run": {
                    "type": "string",
                    "description": "Optional exact run id returned by executor_status. Omitted selects the calling lead's sole recorded run; it never establishes authority by itself."
                }
            }
        }
    })
}

fn resume_tool_definition() -> Value {
    json!({
        "name": RESUME_TOOL,
        "description": "Explicitly continue one stopped executor run owned by the verified calling lead. exact resumes the same thread with the preserved assignment and bounded progress, without resubmitting the assignment or resetting the pool. fresh starts a new thread on the same shared app-server with the preserved assignment, retains the predecessor, and asks the model to inspect interrupted effects. An explicit stop is never restarted automatically.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "required": ["mode"],
            "properties": {
                "run": {
                    "type": "string",
                    "description": "Optional exact run id returned by executor_status. Omitted selects the calling lead's sole recorded run."
                },
                "mode": {
                    "type": "string",
                    "enum": ["exact", "fresh"],
                    "description": "exact continues the same native thread; fresh creates one new bound thread and retains the predecessor."
                },
                "progress": {
                    "type": "string",
                    "maxLength": 4096,
                    "description": "Optional bounded progress or new facts for the continuation. Omitted is reported as no progress supplied, never inferred from logs."
                }
            }
        }
    })
}

fn perform(operation: &Operation) -> io::Result<Value> {
    match operation.name.as_str() {
        DISPATCH_TOOL => dispatch(operation),
        STATUS_TOOL => status(operation),
        MESSAGE_TOOL => message(operation),
        REPLY_TOOL => reply(operation),
        ASK_TOOL => ask_lead(operation),
        STOP_TOOL => stop(operation),
        RESUME_TOOL => resume(operation),
        other => Err(invalid(format!("unknown executor MCP tool '{other}'"))),
    }
}

fn tool_response(result: io::Result<Value>, operation: &Operation) -> Value {
    let (structured, is_error) = match result {
        Ok(value) => {
            let is_error = value.get("error").is_some()
                || value.get("resolutionError").is_some()
                || value.get("cancellationError").is_some()
                || matches!(
                    value["outcome"].as_str(),
                    Some("partial") | Some("unsupported") | Some("error")
                )
                || value["containment"]["status"].as_str() == Some("partial")
                || matches!(
                    value["delivery"]["status"].as_str(),
                    Some("error") | Some("indeterminate")
                );
            (value, is_error)
        }
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusArgs {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct MessageArgs {
    text: String,
    #[serde(default)]
    client_message_id: Option<String>,
    #[serde(default)]
    run: Option<String>,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ReplyArgs {
    request_id: String,
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AskLeadArgs {
    text: String,
    #[serde(default)]
    client_message_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StopArgs {
    #[serde(default)]
    run: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResumeArgs {
    mode: String,
    #[serde(default)]
    run: Option<String>,
    #[serde(default)]
    progress: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunRole {
    Lead,
    Executor,
}

impl RunRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Lead => "lead",
            Self::Executor => "executor",
        }
    }
}

struct RunReceipt {
    path: PathBuf,
    value: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Lease {
    slot: u32,
    owner: String,
    checkout: PathBuf,
    app_server: EndpointProcess,
}

struct VerifiedRun {
    receipt: PathBuf,
    value: Value,
    role: RunRole,
    run: String,
    lead_thread: String,
    executor_thread: String,
    title: String,
    slot: u32,
    source: PathBuf,
    repository: PathBuf,
    checkout: PathBuf,
    base: String,
    identity: Option<BoundIdentity>,
    endpoint: Option<Endpoint>,
    endpoint_unavailable: Option<String>,
}

fn dispatch(operation: &Operation) -> io::Result<Value> {
    // Identity comes only from Codex's request metadata. Arguments are parsed
    // with deny_unknown_fields, so a caller-supplied lead/thread/run cannot
    // enter any authority decision.
    let caller = caller_thread(&operation.metadata)?;
    let args: DispatchArgs = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("executor_dispatch arguments are invalid: {error}")))?;
    let owner = args.owner.unwrap_or_else(|| generated_owner(&caller));
    let assignment_value = args.assignment.clone();
    let assignment = Assignment::from_value(args.assignment)?;
    let codex_home = native_launcher::codex_home()?;
    if mcp_receipts(&codex_home)?
        .iter()
        .any(|receipt| receipt["executorThreadId"].as_str() == Some(caller.as_str()))
    {
        return Err(invalid(
            "the calling thread is a recorded executor and cannot dispatch descendants; no endpoint, lease, thread or model work was touched",
        ));
    }
    let endpoint = match control::registered_lead(&codex_home, &caller) {
        RegisteredLead::Live(endpoint) => endpoint,
        RegisteredLead::Unavailable(reason) => {
            return Err(invalid(format!(
                "{reason}; no app-server was contacted. Remedy: start or recover the lead with `codex-harness lead start`, then retry from that live lead session"
            )));
        }
    };
    let app_server = endpoint.process.clone().ok_or_else(|| {
        invalid(
            "the registered lead endpoint records no app-server process, so an exact run lease cannot be recorded; no thread was created",
        )
    })?;

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
        "assignment": assignment_value,
        "prompt": prepared.prompt,
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
        "control": {
            "approvalPolicy": plan.approval_policy,
            "sandbox": plan.sandbox,
            "config": plan.overrides,
            "roleArgs": prepared.role_args,
            "profileOverrides": prepared.profile_overrides
        },
        "startupReservationReleased": startup_release.is_none(),
        "lease": {
            "slot": prepared.slot,
            "owner": prepared.owner,
            "checkout": prepared.checkout,
            "appServer": app_server
        }
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

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy()),
        _ => left
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(right.to_string_lossy().trim_end_matches(['\\', '/'])),
    }
}

fn required_text<'a>(value: &'a Value, field: &str) -> io::Result<&'a str> {
    value[field]
        .as_str()
        .filter(|text| !text.is_empty())
        .ok_or_else(|| invalid(format!("the MCP run receipt records no usable {field}")))
}

fn valid_thread_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id == id.trim()
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn valid_client_message_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn read_receipts(home: &Path) -> io::Result<Vec<RunReceipt>> {
    let root = home.join("harness/executor-pool");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut receipts = Vec::new();
    for directory in fs::read_dir(&root)? {
        let directory = directory?;
        if !directory.file_type()?.is_dir() {
            continue;
        }
        for entry in fs::read_dir(directory.path())? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("mcp-") || !name.ends_with(".json") {
                continue;
            }
            if receipts.len() >= MAX_RECEIPTS {
                return Err(invalid(format!(
                    "more than {MAX_RECEIPTS} MCP run receipts are present; refusing an unbounded status or message scan"
                )));
            }
            let path = entry.path();
            let bytes = fs::read(&path).map_err(|error| {
                invalid(format!(
                    "MCP run receipt {} could not be read: {error}",
                    path.display()
                ))
            })?;
            let value = serde_json::from_slice(&bytes).map_err(|error| {
                invalid(format!(
                    "MCP run receipt {} is not valid JSON: {error}",
                    path.display()
                ))
            })?;
            receipts.push(RunReceipt { path, value });
        }
    }
    receipts.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(receipts)
}

fn mcp_receipts(home: &Path) -> io::Result<Vec<Value>> {
    Ok(read_receipts(home)?
        .into_iter()
        .map(|receipt| receipt.value)
        .collect())
}

fn matching_receipts(home: &Path, caller: &str) -> io::Result<Vec<RunReceipt>> {
    Ok(read_receipts(home)?
        .into_iter()
        .filter(|receipt| {
            receipt.value["leadThreadId"].as_str() == Some(caller)
                || receipt.value["executorThreadId"].as_str() == Some(caller)
        })
        .collect())
}

fn verify_run(home: &Path, receipt: RunReceipt, caller: &str) -> io::Result<VerifiedRun> {
    let value = &receipt.value;
    if value["schema"].as_u64() != Some(1) {
        return Err(invalid(format!(
            "MCP run receipt {} has an unsupported schema; no identity was accepted",
            receipt.path.display()
        )));
    }
    let run = required_text(value, "run")?.to_owned();
    let lead_thread = required_text(value, "leadThreadId")?.to_owned();
    let executor_thread = required_text(value, "executorThreadId")?.to_owned();
    if !valid_thread_id(&lead_thread) || !valid_thread_id(&executor_thread) {
        return Err(invalid(
            "the MCP run receipt records a malformed lead or executor thread id",
        ));
    }
    if lead_thread == executor_thread {
        return Err(invalid(
            "the MCP run receipt records one thread as both lead and executor; refusing an ambiguous identity",
        ));
    }
    let role = if caller == lead_thread {
        RunRole::Lead
    } else if caller == executor_thread {
        RunRole::Executor
    } else {
        return Err(invalid(
            "the calling thread is not the recorded lead or executor of this run; no operation was performed",
        ));
    };
    let title = required_text(value, "title")?.to_owned();
    let base = required_text(value, "base")?.to_owned();
    let source = PathBuf::from(required_text(value, "source")?);
    let repository = PathBuf::from(required_text(value, "repository")?);
    let checkout = PathBuf::from(required_text(value, "checkout")?);
    let slot = value["slot"]
        .as_u64()
        .and_then(|slot| u32::try_from(slot).ok())
        .filter(|slot| *slot > 0)
        .ok_or_else(|| invalid("the MCP run receipt records no usable pool slot"))?;
    let lease: Lease = serde_json::from_value(value["lease"].clone())
        .map_err(|error| invalid(format!("the MCP run receipt has no exact lease: {error}")))?;
    if lease.slot != slot || lease.owner != run || !same_path(&lease.checkout, &checkout) {
        return Err(invalid(
            "the MCP run receipt's lease does not match its run, slot or checkout; no operation was performed",
        ));
    }
    let expected_parent = task_worktree::pool_state_dir(home, &repository)?;
    let parent = receipt
        .path
        .parent()
        .ok_or_else(|| invalid("the MCP run receipt has no parent directory"))?;
    let expected_name = format!("mcp-{slot}.json");
    if !same_path(parent, &expected_parent)
        || receipt.path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str())
    {
        return Err(invalid(format!(
            "MCP run receipt {} is not the canonical receipt for slot {} of {}; refusing a relocated or copied record",
            receipt.path.display(),
            slot,
            repository.display()
        )));
    }
    let record = task_worktree::load_slot_record(home, &repository, slot)?.ok_or_else(|| {
        invalid(format!(
            "slot {slot} has no recorded ownership; the MCP run has no exact lease"
        ))
    })?;
    if record.owner.as_deref() != Some(run.as_str())
        || record.index != slot
        || !same_path(&record.path, &checkout)
        || !matches!(record.state, SlotState::Occupied)
    {
        return Err(invalid(format!(
            "slot {slot} is not the recorded run's live lease: owner {:?}, state {:?}, checkout {}; no operation was performed",
            record.owner,
            record.state,
            record.path.display()
        )));
    }
    let (endpoint, endpoint_unavailable) = match control::registered_lead(home, &lead_thread) {
        RegisteredLead::Live(endpoint) => {
            if endpoint.thread_id.as_deref() != Some(lead_thread.as_str())
                || endpoint.process.as_ref() != Some(&lease.app_server)
            {
                return Err(invalid(
                    "the registered lead endpoint no longer matches the exact run lease; the run was replaced, resumed elsewhere or is stale, and no operation was performed",
                ));
            }
            (Some(*endpoint), None)
        }
        RegisteredLead::Unavailable(reason) => (None, Some(reason)),
    };
    let identity = if value["profile"].is_null() {
        None
    } else {
        Some(
            serde_json::from_value::<BoundIdentity>(value["profile"].clone()).map_err(|error| {
                invalid(format!(
                    "the MCP run receipt records no usable profile binding: {error}"
                ))
            })?,
        )
    };
    Ok(VerifiedRun {
        receipt: receipt.path,
        value: receipt.value,
        role,
        run,
        lead_thread,
        executor_thread,
        title,
        slot,
        source,
        repository,
        checkout,
        base,
        identity,
        endpoint,
        endpoint_unavailable,
    })
}

fn native_thread(run: &VerifiedRun, thread_id: &str, executor_side: bool) -> io::Result<Value> {
    let endpoint = run
        .endpoint
        .as_ref()
        .ok_or_else(|| {
            invalid(format!(
                "the originating lead thread has no verified live endpoint ({}) so native status is unavailable",
                run.endpoint_unavailable.as_deref().unwrap_or("not recorded")
            ))
        })?
        .for_thread(thread_id)?;
    let mut conversation = Conversation::attach(&endpoint, DEFAULT_BOUND)?;
    let thread = conversation.thread_state()?;
    if thread["id"].as_str() != Some(thread_id) {
        return Err(invalid(format!(
            "thread/read answered for {} instead of the recorded thread {thread_id}; status is unavailable",
            thread["id"]
        )));
    }
    let state = thread["status"]["type"]
        .as_str()
        .or_else(|| thread["status"].as_str())
        .ok_or_else(|| {
            invalid(format!(
                "thread {thread_id} reports no native status; status is unavailable rather than empty"
            ))
        })?;
    if executor_side {
        let cwd = thread["cwd"].as_str().ok_or_else(|| {
            invalid(format!(
                "executor thread {thread_id} reports no working directory; status is unavailable"
            ))
        })?;
        if !same_path(Path::new(cwd), &run.checkout) {
            return Err(invalid(format!(
                "executor thread {thread_id} runs at {cwd} instead of its recorded checkout {}; status is unavailable",
                run.checkout.display()
            )));
        }
        if let Some(identity) = &run.identity
            && let Err(mismatch) = identity.verify(&thread)
        {
            return Err(invalid(format!(
                "executor thread {thread_id} no longer carries its recorded profile binding: {mismatch}"
            )));
        }
    }
    Ok(json!({
        "threadId": thread_id,
        "status": state,
        "cwd": thread["cwd"],
        "activeTurnId": active_turn(&thread)
    }))
}

fn pending_requests(run: &VerifiedRun) -> (Vec<Value>, bool) {
    let Some(messages) = run.value["leadMessages"].as_array() else {
        return (Vec::new(), false);
    };
    let mut pending: Vec<Value> = messages
        .iter()
        .filter(|entry| {
            entry["kind"].as_str() == Some(KIND_REQUEST)
                && !matches!(
                    entry["status"].as_str(),
                    Some("resolved") | Some("cancelled")
                )
        })
        .map(|entry| {
            json!({
                "requestId": entry["id"],
                "kind": entry["kind"],
                "status": entry["status"],
                "method": entry["method"],
                "turnId": entry["turnId"],
                "attempts": entry["attempts"],
                "requestedMs": entry["requestedMs"],
                "recordedMs": entry["recordedMs"],
                "deliveredMs": entry["deliveredMs"],
                "payloadBytes": entry["payloadBytes"].as_u64().or_else(|| entry["bytes"].as_u64()),
                "resolvedMs": entry["resolvedMs"],
                "resolvedBy": entry["resolvedBy"]
            })
        })
        .collect();
    let truncated = pending.len() > MAX_STATUS_REQUESTS;
    pending.truncate(MAX_STATUS_REQUESTS);
    (pending, truncated)
}

fn run_summary(run: &VerifiedRun) -> Value {
    json!({
        "run": run.run,
        "role": run.role.as_str(),
        "title": run.title,
        "leadThreadId": run.lead_thread,
        "executorThreadId": run.executor_thread,
        "slot": run.slot,
        "source": run.source,
        "repository": run.repository,
        "checkout": run.checkout,
        "base": run.base,
        "profile": run.value["profile"],
        "stop": run.value["stop"],
        "continuation": run.value["continuation"],
        "predecessorThreadId": run.value["predecessorThreadId"],
        "receipt": run.receipt
    })
}

fn status(operation: &Operation) -> io::Result<Value> {
    let caller = caller_thread(&operation.metadata)?;
    let _: StatusArgs = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("executor_status arguments are invalid: {error}")))?;
    let home = native_launcher::codex_home()?;
    let receipts = matching_receipts(&home, &caller)?;
    if receipts.is_empty() {
        return Ok(json!({
            "callerThreadId": caller,
            "role": Value::Null,
            "runs": [],
            "state": "empty",
            "detail": "no recorded MCP executor run is bound to this calling thread"
        }));
    }
    let truncated = receipts.len() > MAX_STATUS_RUNS;
    let mut role = None;
    let mut runs = Vec::new();
    for receipt in receipts.into_iter().take(MAX_STATUS_RUNS) {
        let run = verify_run(&home, receipt, &caller)?;
        if let Some(previous) = role
            && previous != run.role
        {
            return Err(invalid(
                "the calling thread is recorded as both a lead and an executor; refusing an ambiguous status",
            ));
        }
        role = Some(run.role);
        let caller_thread = match run.role {
            RunRole::Lead => &run.lead_thread,
            RunRole::Executor => &run.executor_thread,
        };
        let native = native_thread(&run, caller_thread, run.role == RunRole::Executor)?;
        let executor_native = if run.role == RunRole::Lead {
            Some(native_thread(&run, &run.executor_thread, true)?)
        } else {
            None
        };
        let (pending, pending_truncated) = pending_requests(&run);
        let mut summary = run_summary(&run);
        summary["native"] = native;
        summary["executorNative"] = executor_native.unwrap_or(Value::Null);
        summary["pendingRequests"] = Value::Array(pending);
        summary["pendingRequestsTruncated"] = Value::Bool(pending_truncated);
        runs.push(summary);
    }
    Ok(json!({
        "callerThreadId": caller,
        "role": role.map(RunRole::as_str),
        "runs": runs,
        "truncated": truncated,
        "state": "ok"
    }))
}

fn stop(operation: &Operation) -> io::Result<Value> {
    let caller = caller_thread(&operation.metadata)?;
    let args: StopArgs = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("executor_stop arguments are invalid: {error}")))?;
    let home = native_launcher::codex_home()?;
    let run = match args.run.as_deref() {
        Some(run) => lead_run_by_id(&home, &caller, run)?,
        None => single_run(&home, &caller, RunRole::Lead)?,
    };
    let endpoint = run.endpoint.clone().ok_or_else(|| {
        invalid(format!(
            "the exact run's registered app-server endpoint is unavailable ({}); no process was targeted",
            run.endpoint_unavailable
                .as_deref()
                .unwrap_or("not recorded")
        ))
    })?;
    let mut result =
        executor_cli::mcp_stop(&run.receipt, &endpoint, &run.executor_thread, DEFAULT_BOUND)?;
    result["run"] = Value::String(run.run.clone());
    result["leadThreadId"] = Value::String(run.lead_thread.clone());
    result["executorThreadId"] = Value::String(run.executor_thread.clone());
    result["slot"] = Value::from(run.slot);
    result["lease"] = run.value["lease"].clone();
    result["preserved"] = json!({
        "checkout": run.checkout,
        "base": run.base,
        "slot": run.slot,
        "owner": run.run,
        "receipt": run.receipt,
        "appServerRetained": true,
        "reset": false,
        "released": false,
        "reassigned": false
    });
    Ok(result)
}

fn resume(operation: &Operation) -> io::Result<Value> {
    let caller = caller_thread(&operation.metadata)?;
    let args: ResumeArgs = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("executor_resume arguments are invalid: {error}")))?;
    let mode = match args.mode.as_str() {
        "exact" | "fresh" => args.mode.as_str(),
        other => {
            return Err(invalid(format!(
                "executor_resume mode must be 'exact' or 'fresh', not {other:?}; no run was resumed"
            )));
        }
    };
    let progress = args.progress.as_deref().unwrap_or_default();
    if progress.len() > MAX_PROGRESS_BYTES {
        return Err(invalid(format!(
            "executor_resume progress is {} bytes; the limit is {MAX_PROGRESS_BYTES}. Nothing was resumed",
            progress.len()
        )));
    }
    let home = native_launcher::codex_home()?;
    let run = match args.run.as_deref() {
        Some(run) => lead_run_by_id(&home, &caller, run)?,
        None => single_run(&home, &caller, RunRole::Lead)?,
    };
    let endpoint = run.endpoint.clone().ok_or_else(|| {
        invalid(format!(
            "the exact run's registered app-server endpoint is unavailable ({}); no model work was started",
            run.endpoint_unavailable
                .as_deref()
                .unwrap_or("not recorded")
        ))
    })?;
    if run.value["stop"]["outcome"].as_str() == Some("partial") {
        return Err(invalid(
            "the last stop reported partial containment; no continuation was started. Repeat executor_stop until the exact thread and its background terminals are verified gone, or preserve the run for explicit inspection",
        ));
    }
    let prompt = required_text(&run.value, "prompt")?.to_owned();
    let evidence = workspace_evidence(&run.checkout, &run.base);
    let from_turn = run.value["turnId"].as_str().map(str::to_owned);
    match mode {
        "exact" => {
            let mut conversation = Conversation::attach(&endpoint, DEFAULT_BOUND)?;
            conversation.adopt_thread(&run.executor_thread)?;
            let thread = conversation.thread_state()?;
            let turns = thread["turns"]
                .as_array()
                .filter(|turns| !turns.is_empty())
                .ok_or_else(|| {
                    invalid(format!(
                        "exact executor thread {} reports no pre-turn history, so the preserved assignment cannot be proved; the run was not resumed. Use executor_resume mode=fresh to retain this predecessor and start from the recorded assignment",
                        run.executor_thread
                    ))
                })?;
            if let Some(turn) = active_turn(&thread) {
                return Err(invalid(format!(
                    "exact executor thread {} still has active turn {turn}; stop it before explicit continuation. No model work was started",
                    run.executor_thread
                )));
            }
            let objective = run.value["assignment"]["objective"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| excerpt(&prompt, 1024));
            let text = exact_continuation_text(&objective, progress, &evidence, turns.len());
            let turn = conversation.assign(&text)?;
            record_continuation(
                &run,
                "exact",
                &run.executor_thread,
                from_turn.as_deref(),
                &run.executor_thread,
                &turn.turn_id,
                progress.len(),
            )?;
            Ok(json!({
                "mode": "exact",
                "run": run.run,
                "threadId": run.executor_thread,
                "turnId": turn.turn_id,
                "status": turn.status,
                "predecessorRetained": false,
                "preTurnTurns": turns.len(),
                "checkout": run.checkout,
                "base": run.base,
                "slot": run.slot,
                "receipt": run.receipt,
                "workspace": evidence,
                "detail": "resumed the exact native thread with the preserved assignment; the assignment was not resubmitted and the slot was not reset or reassigned"
            }))
        }
        "fresh" => {
            let mut current = Conversation::attach(&endpoint, DEFAULT_BOUND)?;
            current.adopt_thread(&run.executor_thread)?;
            let current_state = current.thread_state()?;
            if let Some(turn) = active_turn(&current_state) {
                return Err(invalid(format!(
                    "exact executor thread {} still has active turn {turn}; stop it before fresh recovery. No new thread was started",
                    run.executor_thread
                )));
            }
            drop(current);
            let identity = run.identity.clone().ok_or_else(|| {
                invalid(
                    "the run receipt records no exact profile binding; fresh recovery cannot preserve routing and no thread was started",
                )
            })?;
            let overrides: BTreeMap<String, Value> = match run.value["control"]["config"]
                .as_object()
            {
                Some(config) => config
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
                None => {
                    return Err(invalid(
                        "the run receipt records no exact control configuration; fresh recovery cannot preserve routing and no thread was started",
                    ));
                }
            };
            let paths = ControlPaths::for_slot(&home, &run.repository, run.slot)?;
            let mut plan = ControlPlan::new(
                home.join("harness/bin/codex.exe"),
                &home,
                &run.checkout,
                run.title.clone(),
                identity,
                paths,
            );
            plan.approval_policy = run.value["control"]["approvalPolicy"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| Some(APPROVAL_POLICY.into()));
            plan.sandbox = run.value["control"]["sandbox"].as_str().map(str::to_owned);
            plan.overrides = overrides;
            let mut conversation = Conversation::start_on_endpoint(&endpoint, &plan)?;
            let new_thread = conversation.thread_id().to_owned();
            let thread = conversation.thread_state()?;
            let observed = confirm_started(
                &thread,
                &new_thread,
                &run.checkout,
                run.identity.as_ref().expect("checked above"),
                &run.title,
            )?;
            let text = fresh_recovery_text(&prompt, progress, &evidence, &run.executor_thread);
            let turn = conversation.assign(&text)?;
            record_continuation(
                &run,
                "fresh",
                &run.executor_thread,
                from_turn.as_deref(),
                &new_thread,
                &turn.turn_id,
                progress.len(),
            )?;
            Ok(json!({
                "mode": "fresh",
                "run": run.run,
                "threadId": new_thread,
                "turnId": turn.turn_id,
                "status": turn.status,
                "observedStatus": observed,
                "predecessorThreadId": run.executor_thread,
                "predecessorRetained": true,
                "checkout": run.checkout,
                "base": run.base,
                "slot": run.slot,
                "receipt": run.receipt,
                "workspace": evidence,
                "detail": "created one new bound thread on the same shared app-server; the predecessor thread remains retained, the slot stayed bound, and the preserved assignment was submitted once in the new thread"
            }))
        }
        _ => unreachable!("validated above"),
    }
}

fn exact_continuation_text(
    objective: &str,
    progress: &str,
    evidence: &str,
    pre_turn_turns: usize,
) -> String {
    format!(
        "Continue the preserved assignment in this same thread. Do not replay or resubmit the assignment, reset or clean the checkout, or repeat completed commands. Inspect the current diff, commits, untracked files and interrupted effects before reusing any prior result; preserve completed work.\n\nPreserved assignment objective: {objective}\nRecorded pre-turn history: {pre_turn_turns} turn(s), retained in this thread.\nCheckout at continuation: {evidence}\nBounded progress supplied by the lead:\n{}\n\nReturn the compact result to the originating lead.",
        if progress.trim().is_empty() {
            "(none supplied; do not infer progress from logs)"
        } else {
            progress
        }
    )
}

fn fresh_recovery_text(
    assignment: &str,
    progress: &str,
    evidence: &str,
    predecessor: &str,
) -> String {
    format!(
        "Recover the preserved assignment in this new thread. The predecessor thread {predecessor} remains retained and is not this conversation. First inspect the current diff, commits, untracked files and interrupted effects; do not reset or clean the checkout and do not assume a prior command completed.\n\nCheckout at recovery: {evidence}\nBounded progress supplied by the lead:\n{}\n\nOriginal assignment:\n{assignment}\n\nReturn the compact result to the originating lead.",
        if progress.trim().is_empty() {
            "(none supplied; do not infer progress from logs)"
        } else {
            progress
        }
    )
}

fn workspace_evidence(checkout: &Path, base: &str) -> String {
    let status = Command::new("git")
        .args(["-C"])
        .arg(checkout)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output();
    let commits = Command::new("git")
        .args(["-C"])
        .arg(checkout)
        .args(["rev-list", "--count", &format!("{base}..HEAD")])
        .output();
    let changed = status
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).lines().count());
    let commits = commits
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        });
    match (commits, changed) {
        (Some(commits), Some(changed)) => format!(
            "{commits} commit(s) since the recorded base; {changed} changed or untracked path(s) currently present"
        ),
        _ => "workspace inspection unavailable; inspect the checkout directly before reusing prior effects"
            .into(),
    }
}

fn record_continuation(
    run: &VerifiedRun,
    mode: &str,
    from_thread: &str,
    from_turn: Option<&str>,
    thread_id: &str,
    turn_id: &str,
    progress_bytes: usize,
) -> io::Result<()> {
    executor_message::update_receipt_document(&run.receipt, |document| {
        let now = executor_message_now();
        let stop = document.get("stop").cloned().unwrap_or(Value::Null);
        document["executorThreadId"] = Value::String(thread_id.to_owned());
        document["turnId"] = Value::String(turn_id.to_owned());
        document["observedStatus"] = Value::String("active".into());
        document["continuation"] = json!({
            "schema": 1,
            "mode": mode,
            "fromThreadId": from_thread,
            "fromTurnId": from_turn,
            "threadId": thread_id,
            "turnId": turn_id,
            "progressBytes": progress_bytes,
            "recordedMs": now
        });
        if let Some(stop) = document["stop"].as_object_mut() {
            stop.insert("continuedMs".into(), Value::from(now));
            stop.insert("continuedBy".into(), Value::String(mode.to_owned()));
        }
        if mode == "fresh" {
            document["predecessorThreadId"] = Value::String(from_thread.to_owned());
            document["predecessor"] = json!({
                "threadId": from_thread,
                "turnId": from_turn,
                "retained": true,
                "stop": stop
            });
        }
        Ok(())
    })
}

fn digest_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    let hex: String = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{prefix}{hex}")
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn checked_client_id(provided: Option<String>, stable: bool, text: &str) -> io::Result<String> {
    if let Some(id) = provided {
        if !valid_client_message_id(&id) {
            return Err(invalid(
                "clientMessageId must be 1-128 ASCII letters, digits, '-', '_' or '.'",
            ));
        }
        return Ok(id);
    }
    if stable {
        Ok(digest_id("auto-", &[text]))
    } else {
        Ok(format!("auto-{}-{}", now_nanos(), std::process::id()))
    }
}

fn valid_request_id(id: &str) -> bool {
    let Some(hex) = id.strip_prefix("lead-") else {
        return false;
    };
    hex.len() == 24
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_run_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn single_run(home: &Path, caller: &str, role: RunRole) -> io::Result<VerifiedRun> {
    let field = match role {
        RunRole::Lead => "leadThreadId",
        RunRole::Executor => "executorThreadId",
    };
    let matches: Vec<RunReceipt> = read_receipts(home)?
        .into_iter()
        .filter(|receipt| receipt.value[field].as_str() == Some(caller))
        .collect();
    match matches.as_slice() {
        [] => Err(invalid(format!(
            "no recorded MCP executor run is bound to the calling {} thread; no operation was performed",
            role.as_str()
        ))),
        [receipt] => verify_run(
            home,
            RunReceipt {
                path: receipt.path.clone(),
                value: receipt.value.clone(),
            },
            caller,
        ),
        _ => {
            let runs: Vec<Value> = matches
                .iter()
                .map(|receipt| {
                    json!({
                        "run": receipt.value["run"],
                        "executorThreadId": receipt.value["executorThreadId"],
                        "slot": receipt.value["slot"]
                    })
                })
                .take(MAX_STATUS_RUNS)
                .collect();
            Err(invalid(format!(
                "the calling thread owns {} recorded MCP executor runs; the operation is ambiguous and no model effect occurred. Choose one with executor_message run=<run> or a recorded requestId. runs: {}",
                matches.len(),
                Value::Array(runs)
            )))
        }
    }
}

fn lead_run_by_id(home: &Path, caller: &str, run_id: &str) -> io::Result<VerifiedRun> {
    if !valid_run_id(run_id) {
        return Err(invalid(format!(
            "run {run_id:?} is malformed; refusing before any run lookup or send"
        )));
    }
    let matches: Vec<RunReceipt> = read_receipts(home)?
        .into_iter()
        .filter(|receipt| {
            receipt.value["leadThreadId"].as_str() == Some(caller)
                && receipt.value["run"].as_str() == Some(run_id)
        })
        .collect();
    match matches.as_slice() {
        [] => Err(invalid(format!(
            "no recorded MCP executor run {run_id:?} is owned by the calling lead; nothing was sent"
        ))),
        [receipt] => verify_run(
            home,
            RunReceipt {
                path: receipt.path.clone(),
                value: receipt.value.clone(),
            },
            caller,
        ),
        _ => Err(invalid(format!(
            "run {run_id:?} matches more than one recorded run for the calling lead; refusing an ambiguous target"
        ))),
    }
}

fn run_for_request(
    home: &Path,
    request_id: &str,
    caller: &str,
) -> io::Result<(VerifiedRun, Value)> {
    if !valid_request_id(request_id) {
        return Err(invalid(format!(
            "requestId {request_id:?} is malformed; refusing before any run lookup or send"
        )));
    }
    let mut matches = Vec::new();
    for receipt in read_receipts(home)? {
        if let Some(entry) = receipt.value["leadMessages"]
            .as_array()
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["id"].as_str() == Some(request_id))
            })
            .cloned()
        {
            matches.push((receipt, entry));
        }
    }
    match matches.len() {
        0 => Err(invalid(format!(
            "requestId {request_id} is not recorded on any MCP executor run; nothing was sent"
        ))),
        1 => {
            let (receipt, entry) = matches.remove(0);
            let run = verify_run(home, receipt, caller)?;
            if run.role != RunRole::Lead {
                return Err(invalid(
                    "only the recorded originating lead may answer this request; nothing was sent",
                ));
            }
            Ok((run, entry))
        }
        _ => Err(invalid(format!(
            "requestId {request_id} is recorded on more than one run; refusing an ambiguous correlation"
        ))),
    }
}

fn message(operation: &Operation) -> io::Result<Value> {
    let caller = caller_thread(&operation.metadata)?;
    let args: MessageArgs = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("executor_message arguments are invalid: {error}")))?;
    if args.text.is_empty() {
        return Err(invalid("the message is empty; nothing was sent"));
    }
    let home = native_launcher::codex_home()?;
    let receipts = read_receipts(&home)?;
    let is_executor = receipts
        .iter()
        .any(|receipt| receipt.value["executorThreadId"].as_str() == Some(caller.as_str()));
    let is_lead = receipts
        .iter()
        .any(|receipt| receipt.value["leadThreadId"].as_str() == Some(caller.as_str()));
    if is_executor && is_lead {
        return Err(invalid(
            "the calling thread is recorded as both a lead and an executor; refusing an ambiguous message direction",
        ));
    }
    if is_executor {
        if args.request_id.is_some() || args.run.is_some() {
            return Err(invalid(
                "an executor addresses only its originating lead and cannot select a run; nothing was sent",
            ));
        }
        let run = single_run(&home, &caller, RunRole::Executor)?;
        return send_to_lead(&home, &run, &args.text, KIND_NOTICE, args.client_message_id);
    }
    if !is_lead {
        return Err(invalid(
            "the calling thread is not a recorded MCP executor lead or executor; no operation was performed",
        ));
    }
    let run = match (args.run.as_deref(), args.request_id.as_deref()) {
        (Some(run_id), Some(request_id)) => {
            let selected = lead_run_by_id(&home, &caller, run_id)?;
            let (requested, _) = run_for_request(&home, request_id, &caller)?;
            if selected.run != requested.run || !same_path(&selected.receipt, &requested.receipt) {
                return Err(invalid(format!(
                    "run {run_id:?} and requestId {request_id} name different runs; nothing was sent"
                )));
            }
            selected
        }
        (Some(run_id), None) => lead_run_by_id(&home, &caller, run_id)?,
        (None, Some(request_id)) => run_for_request(&home, request_id, &caller)?.0,
        (None, None) => single_run(&home, &caller, RunRole::Lead)?,
    };
    if run.role != RunRole::Lead {
        return Err(invalid(
            "the selected request does not belong to the calling lead; nothing was sent",
        ));
    }
    send_to_executor(&home, &run, &args.text, args.client_message_id, None)
}

fn reply(operation: &Operation) -> io::Result<Value> {
    let caller = caller_thread(&operation.metadata)?;
    let args: ReplyArgs = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("executor_reply arguments are invalid: {error}")))?;
    if args.text.is_empty() {
        return Err(invalid("the reply is empty; nothing was sent"));
    }
    let home = native_launcher::codex_home()?;
    let (run, entry) = run_for_request(&home, &args.request_id, &caller)?;
    if entry["kind"].as_str() != Some(KIND_REQUEST) {
        return Err(invalid(
            "the referenced record is not a reply-required question; nothing was sent",
        ));
    }
    if entry["status"].as_str() == Some("resolved") {
        let mut result = delivery_result(
            &run,
            "lead-to-executor",
            &args.request_id,
            Some(KIND_REQUEST),
            &delivery_from_entry(&entry),
            None,
            false,
        );
        result["resolved"] = Value::Bool(true);
        result["replyMessageId"] = entry["resolvedBy"].clone();
        return Ok(result);
    }
    let mut result = send_to_executor(&home, &run, &args.text, None, Some(&args.request_id))?;
    result["requestId"] = Value::String(args.request_id.clone());
    result["replyMessageId"] = result["messageId"].clone();
    if result["delivery"]["status"].as_str() == Some("delivered")
        && result["resolved"].as_bool() != Some(true)
    {
        result["resolutionError"] = Value::String(
            "the reply was observed in the executor conversation but its recorded request did not resolve"
                .to_owned(),
        );
    }
    Ok(result)
}

fn ask_lead(operation: &Operation) -> io::Result<Value> {
    let caller = caller_thread(&operation.metadata)?;
    let args: AskLeadArgs = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("executor_ask_lead arguments are invalid: {error}")))?;
    if args.text.is_empty() {
        return Err(invalid("the question is empty; nothing was sent"));
    }
    let home = native_launcher::codex_home()?;
    let run = single_run(&home, &caller, RunRole::Executor)?;
    send_to_lead(
        &home,
        &run,
        &args.text,
        KIND_REQUEST,
        args.client_message_id,
    )
}

fn delivery_from_entry(entry: &Value) -> Value {
    let status = entry["status"].as_str().unwrap_or("indeterminate");
    json!({
        "status": if status == "resolved" { "delivered" } else { status },
        "method": entry["method"],
        "turnId": entry["turnId"],
        "evidence": entry["evidence"],
        "attempts": entry["attempts"],
        "detail": entry["detail"]
    })
}

fn delivery_result(
    run: &VerifiedRun,
    direction: &str,
    id: &str,
    kind: Option<&str>,
    delivery: &Value,
    client_message_id: Option<&str>,
    awaiting_reply: bool,
) -> Value {
    let mut result = json!({
        "direction": direction,
        "id": id,
        "requestId": if kind == Some(KIND_REQUEST) { Value::String(id.to_owned()) } else { Value::Null },
        "messageId": if kind == Some(KIND_REQUEST) { Value::Null } else { Value::String(id.to_owned()) },
        "kind": kind,
        "clientMessageId": client_message_id,
        "delivery": delivery,
        "awaitingReply": awaiting_reply,
        "run": run_summary(run)
    });
    if matches!(
        delivery["status"].as_str(),
        Some("error") | Some("indeterminate")
    ) {
        result["error"] = delivery["detail"].clone();
    }
    result
}

/// A stopped run is never restarted by a message or reply. Exact continuation
/// is the only operation that starts model work again, so delivery is refused
/// until that same stop record carries an explicit continuation marker.
fn reject_stopped_delivery(run: &VerifiedRun) -> io::Result<()> {
    let stopped = matches!(
        run.value["stop"]["outcome"].as_str(),
        Some("stopped") | Some("partial") | Some("already-stopped")
    );
    if stopped && run.value["stop"]["continuedMs"].is_null() {
        return Err(invalid(
            "the exact run is stopped; this delivery was not sent and no turn was started. Call executor_resume with mode exact or fresh for an explicit continuation",
        ));
    }
    Ok(())
}

fn content_digest(kind: &str, text: &str) -> String {
    digest_id("content-", &[kind, text])
}

fn question_id(run: &VerifiedRun, kind: &str, client_message_id: &str) -> String {
    digest_id(
        "lead-",
        &[&run.run, &run.executor_thread, kind, client_message_id],
    )
}

fn message_id(run: &VerifiedRun, direction: &str, client_message_id: &str) -> String {
    digest_id(
        "msg-",
        &[&run.run, &run.executor_thread, direction, client_message_id],
    )
}

fn lead_envelope(
    home: &Path,
    run: &VerifiedRun,
    id: &str,
    kind: &str,
    text: &str,
) -> io::Result<ComposedDelivery> {
    let reply = if kind == KIND_REQUEST {
        format!("executor_reply requestId={id} text=<literal answer>")
    } else {
        "not-requested".to_owned()
    };
    let header = json!({
        "id": id,
        "kind": kind,
        "owner": run.run,
        "runGeneration": run.run,
        "executorThreadId": run.executor_thread,
        "leadThreadId": run.lead_thread,
        "slot": run.slot,
        "checkout": run.checkout,
        "reply": reply
    });
    let header = serde_json::to_string(&header).map_err(|error| {
        invalid(format!(
            "the executor message header could not be encoded: {error}"
        ))
    })?;
    executor_input::compose(home, id, text, |body| {
        format!("{header}{PAYLOAD_MARK}{body}")
    })
}

fn reply_envelope(
    home: &Path,
    run: &VerifiedRun,
    id: &str,
    request_id: &str,
    text: &str,
) -> io::Result<ComposedDelivery> {
    let header = json!({
        "id": id,
        "kind": "reply",
        "replyTo": request_id,
        "owner": run.run,
        "runGeneration": run.run,
        "executorThreadId": run.executor_thread,
        "leadThreadId": run.lead_thread,
        "slot": run.slot,
        "checkout": run.checkout
    });
    let header = serde_json::to_string(&header).map_err(|error| {
        invalid(format!(
            "the lead reply header could not be encoded: {error}"
        ))
    })?;
    executor_input::compose(home, id, text, |body| {
        format!("{header}{PAYLOAD_MARK}{body}")
    })
}

fn question_entry(run: &VerifiedRun, id: &str) -> Option<Value> {
    run.value["leadMessages"]
        .as_array()?
        .iter()
        .find(|entry| entry["id"].as_str() == Some(id))
        .cloned()
}

struct QuestionRecord<'a> {
    kind: &'a str,
    content: &'a str,
    client_message_id: &'a str,
    text: &'a str,
    status: &'a str,
    method: &'a str,
    turn_id: Option<&'a str>,
    evidence: Option<&'a str>,
    detail: &'a str,
    attempts: u64,
    spill: Option<&'a executor_input::Spill>,
}

fn upsert_question(run: &VerifiedRun, id: &str, record: &QuestionRecord<'_>) -> io::Result<Value> {
    executor_message::update_receipt_document(&run.receipt, |document| {
        let now = executor_message_now();
        let mut messages = document["leadMessages"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let index = messages
            .iter()
            .position(|entry| entry["id"].as_str() == Some(id));
        if index.is_none() && messages.len() >= 64 {
            if let Some(disposable) = messages.iter().position(|entry| {
                matches!(entry["status"].as_str(), Some("resolved") | Some("error"))
                    && entry["kind"].as_str() == Some(KIND_NOTICE)
            }) {
                messages.remove(disposable);
            } else {
                return Err(invalid(
                    "the receipt already records 64 unresolved communication entries; refusing to evict a question, so nothing was sent",
                ));
            }
        }
        let requested_ms = index
            .and_then(|index| messages[index]["requestedMs"].as_u64())
            .unwrap_or(now);
        let mut entry = index
            .map(|index| messages[index].clone())
            .unwrap_or_else(|| {
                json!({
                    "schema": 1,
                    "id": id,
                    "requestedMs": now
                })
            });
        entry["requestedMs"] = Value::from(requested_ms);
        entry["contentId"] = Value::String(record.content.to_owned());
        entry["clientMessageId"] = Value::String(record.client_message_id.to_owned());
        entry["kind"] = Value::String(record.kind.to_owned());
        entry["status"] = Value::String(record.status.to_owned());
        entry["method"] = Value::String(record.method.to_owned());
        entry["turnId"] = json!(record.turn_id);
        entry["evidence"] = json!(record.evidence);
        entry["detail"] = Value::String(record.detail.to_owned());
        entry["attempts"] = Value::from(record.attempts);
        entry["bytes"] = Value::from(record.text.len());
        entry["lines"] = Value::from(record.text.lines().count().max(1));
        entry["payloadBytes"] = Value::from(record.text.len());
        entry["leadThreadId"] = Value::String(run.lead_thread.clone());
        entry["executorThreadId"] = Value::String(run.executor_thread.clone());
        entry["runGeneration"] = Value::String(run.run.clone());
        entry["slot"] = Value::from(run.slot);
        entry["owner"] = Value::String(run.run.clone());
        entry["recordedMs"] = Value::from(now);
        if record.status == "delivered" {
            entry["deliveredMs"] = Value::from(now);
        }
        if let Some(spill) = record.spill {
            entry["delivery"] = Value::String("spill".into());
            entry["payloadPath"] = Value::String(spill.path.display().to_string());
            entry["payloadBytes"] = Value::from(spill.bytes);
        } else {
            entry["delivery"] = Value::Null;
            entry["payloadPath"] = Value::Null;
        }
        match index {
            Some(index) => messages[index] = entry.clone(),
            None => messages.push(entry.clone()),
        }
        document["leadMessages"] = Value::Array(messages);
        Ok(entry)
    })
}

fn executor_message_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn send_to_lead(
    home: &Path,
    run: &VerifiedRun,
    text: &str,
    kind: &str,
    client_message_id: Option<String>,
) -> io::Result<Value> {
    if run.role != RunRole::Executor {
        return Err(invalid(
            "only a recorded executor thread may send an executor message to its lead",
        ));
    }
    reject_stopped_delivery(run)?;
    let client = checked_client_id(client_message_id, kind == KIND_REQUEST, text)?;
    let id = question_id(run, kind, &client);
    let content = content_digest(kind, text);
    let previous = question_entry(run, &id);
    if let Some(previous) = previous.as_ref() {
        if previous["kind"].as_str() != Some(kind) {
            return Err(invalid(
                "the recorded clientMessageId names a different communication kind; nothing was sent",
            ));
        }
        if let Some(recorded) = previous["contentId"].as_str()
            && recorded != content
        {
            return Err(invalid(
                "the recorded clientMessageId was used for different content; use a new id, and nothing was sent",
            ));
        }
    }
    if let Some(previous) = previous.as_ref() {
        let status = previous["status"].as_str().unwrap_or_default();
        if matches!(status, "resolved" | "delivered") {
            let delivery = delivery_from_entry(previous);
            let awaiting = kind == KIND_REQUEST && status != "resolved";
            return Ok(delivery_result(
                run,
                "executor-to-lead",
                &id,
                Some(kind),
                &delivery,
                Some(&client),
                awaiting,
            ));
        }
        if status == "cancelled" {
            let delivery = delivery_from_entry(previous);
            let mut result = delivery_result(
                run,
                "executor-to-lead",
                &id,
                Some(kind),
                &delivery,
                Some(&client),
                false,
            );
            result["error"] = Value::String(
                "the exact run was stopped, so this pending delivery was cancelled; explicit executor_resume is required before new work"
                    .into(),
            );
            return Ok(result);
        }
        if matches!(status, "pending" | "queued" | "indeterminate") {
            let Some(endpoint) = run.endpoint.as_ref() else {
                let delivery = delivery_from_entry(previous);
                let mut result = delivery_result(
                    run,
                    "executor-to-lead",
                    &id,
                    Some(kind),
                    &delivery,
                    Some(&client),
                    kind == KIND_REQUEST,
                );
                result["error"] = Value::String(format!(
                    "the originating lead has no verified live endpoint ({}); the recorded attempt was not resent",
                    run.endpoint_unavailable
                        .as_deref()
                        .unwrap_or("not recorded")
                ));
                return Ok(result);
            };
            let composed = lead_envelope(home, run, &id, kind, text)?;
            let observed = executor_message::observe_exact_thread(
                endpoint,
                &run.lead_thread,
                None,
                None,
                &id,
                &composed.text,
            );
            if let Ok(Some(evidence)) = observed {
                let entry = upsert_question(
                    run,
                    &id,
                    &QuestionRecord {
                        kind,
                        content: &content,
                        client_message_id: &client,
                        text,
                        status: evidence.status,
                        method: &evidence.method,
                        turn_id: evidence.turn_id.as_deref(),
                        evidence: evidence.evidence.as_deref(),
                        detail: &evidence.detail,
                        attempts: previous["attempts"].as_u64().unwrap_or(0),
                        spill: composed.spill.as_ref(),
                    },
                )?;
                let delivery = delivery_from_entry(&entry);
                return Ok(delivery_result(
                    run,
                    "executor-to-lead",
                    &id,
                    Some(kind),
                    &delivery,
                    Some(&client),
                    kind == KIND_REQUEST,
                ));
            }
            if status == "pending" {
                let entry = upsert_question(
                    run,
                    &id,
                    &QuestionRecord {
                        kind,
                        content: &content,
                        client_message_id: &client,
                        text,
                        status: "indeterminate",
                        method: previous["method"].as_str().unwrap_or_default(),
                        turn_id: previous["turnId"].as_str(),
                        evidence: previous["evidence"].as_str(),
                        detail: "an earlier attempt stopped before its outcome was recorded and the lead conversation's own items do not show it",
                        attempts: previous["attempts"].as_u64().unwrap_or(0),
                        spill: composed.spill.as_ref(),
                    },
                )?;
                let delivery = delivery_from_entry(&entry);
                return Ok(delivery_result(
                    run,
                    "executor-to-lead",
                    &id,
                    Some(kind),
                    &delivery,
                    Some(&client),
                    kind == KIND_REQUEST,
                ));
            }
            let delivery = delivery_from_entry(previous);
            return Ok(delivery_result(
                run,
                "executor-to-lead",
                &id,
                Some(kind),
                &delivery,
                Some(&client),
                kind == KIND_REQUEST,
            ));
        }
    }
    let composed = lead_envelope(home, run, &id, kind, text)?;
    let attempts = previous
        .as_ref()
        .and_then(|entry| entry["attempts"].as_u64())
        .unwrap_or(0)
        + 1;
    let Some(endpoint) = run.endpoint.as_ref() else {
        let detail = format!(
            "the originating lead has no verified live endpoint ({}); the {} was recorded but not sent",
            run.endpoint_unavailable
                .as_deref()
                .unwrap_or("not recorded"),
            if kind == KIND_REQUEST {
                "question"
            } else {
                "notification"
            }
        );
        let entry = upsert_question(
            run,
            &id,
            &QuestionRecord {
                kind,
                content: &content,
                client_message_id: &client,
                text,
                status: "error",
                method: "",
                turn_id: None,
                evidence: None,
                detail: &detail,
                attempts,
                spill: composed.spill.as_ref(),
            },
        )?;
        let delivery = delivery_from_entry(&entry);
        let mut result = delivery_result(
            run,
            "executor-to-lead",
            &id,
            Some(kind),
            &delivery,
            Some(&client),
            kind == KIND_REQUEST,
        );
        result["error"] = Value::String(detail);
        return Ok(result);
    };
    let mut conversation =
        executor_message::prepare_exact_thread(endpoint, &run.lead_thread, None, None)?;
    upsert_question(
        run,
        &id,
        &QuestionRecord {
            kind,
            content: &content,
            client_message_id: &client,
            text,
            status: "pending",
            method: "",
            turn_id: None,
            evidence: None,
            detail: "the input is being submitted to the lead conversation and its own items are being read",
            attempts,
            spill: composed.spill.as_ref(),
        },
    )?;
    let delivery = executor_message::send_exact_thread(
        &mut conversation,
        &run.lead_thread,
        &id,
        &composed.text,
    )
    .unwrap_or_else(|error| ExactDelivery {
        status: "indeterminate",
        method: String::new(),
        turn_id: None,
        evidence: None,
        detail: format!(
            "the native send could not be completed before the request was submitted: {error}; whether it reached the lead is unknown"
        ),
    });
    let entry = upsert_question(
        run,
        &id,
        &QuestionRecord {
            kind,
            content: &content,
            client_message_id: &client,
            text,
            status: delivery.status,
            method: &delivery.method,
            turn_id: delivery.turn_id.as_deref(),
            evidence: delivery.evidence.as_deref(),
            detail: &delivery.detail,
            attempts,
            spill: composed.spill.as_ref(),
        },
    )?;
    let status = entry["status"].as_str().unwrap_or_default();
    let delivery = delivery_from_entry(&entry);
    Ok(delivery_result(
        run,
        "executor-to-lead",
        &id,
        Some(kind),
        &delivery,
        Some(&client),
        kind == KIND_REQUEST && status != "resolved",
    ))
}

fn send_to_executor(
    home: &Path,
    run: &VerifiedRun,
    text: &str,
    client_message_id: Option<String>,
    reply_to: Option<&str>,
) -> io::Result<Value> {
    if run.role != RunRole::Lead {
        return Err(invalid(
            "only the recorded originating lead may message this executor",
        ));
    }
    reject_stopped_delivery(run)?;
    let client = match client_message_id {
        Some(client) => {
            if !valid_client_message_id(&client) {
                return Err(invalid(
                    "clientMessageId must be 1-128 ASCII letters, digits, '-', '_' or '.'",
                ));
            }
            client
        }
        None => format!("auto-{}-{}", now_nanos(), std::process::id()),
    };
    let id = match reply_to {
        Some(request_id) => {
            executor_message::attempt_identity(&run.executor_thread, text, Some(request_id))
        }
        None => message_id(run, "lead-to-executor", &client),
    };
    let composed = match reply_to {
        Some(request_id) => reply_envelope(home, run, &id, request_id, text)?,
        None => ComposedDelivery {
            text: text.to_owned(),
            spill: None,
        },
    };
    let previous = executor_message::read_exact_attempt(&run.receipt, &id)?;
    if let Some(previous) = previous.as_ref() {
        if let Some(recorded) = previous["contentId"].as_str()
            && recorded != executor_message::content_id(&id, &composed.text)
        {
            return Err(invalid(
                "the recorded clientMessageId was used for different content; use a new id, and nothing was sent",
            ));
        }
        let status = previous["status"].as_str().unwrap_or_default();
        if status == "delivered" {
            let delivery = delivery_from_entry(previous);
            let mut result = delivery_result(
                run,
                "lead-to-executor",
                &id,
                None,
                &delivery,
                Some(&client),
                false,
            );
            if let Some(request_id) = reply_to {
                result["resolved"] =
                    Value::Bool(receipt_question_status(run, request_id)? == "resolved");
            }
            return Ok(result);
        }
        if status == "cancelled" {
            let delivery = delivery_from_entry(previous);
            let mut result = delivery_result(
                run,
                "lead-to-executor",
                &id,
                None,
                &delivery,
                Some(&client),
                false,
            );
            result["error"] = Value::String(
                "the exact run was stopped, so this pending delivery was cancelled; call executor_resume before sending new work"
                    .into(),
            );
            return Ok(result);
        }
        if matches!(status, "pending" | "queued" | "indeterminate") {
            let Some(endpoint) = run.endpoint.as_ref() else {
                let delivery = delivery_from_entry(previous);
                let mut result = delivery_result(
                    run,
                    "lead-to-executor",
                    &id,
                    None,
                    &delivery,
                    Some(&client),
                    false,
                );
                result["error"] = Value::String(format!(
                    "the originating lead has no verified live endpoint ({}); the recorded attempt was not resent",
                    run.endpoint_unavailable
                        .as_deref()
                        .unwrap_or("not recorded")
                ));
                return Ok(result);
            };
            let observed = executor_message::observe_exact_thread(
                endpoint,
                &run.executor_thread,
                Some(&run.checkout),
                run.identity.as_ref(),
                &id,
                &composed.text,
            );
            if let Ok(Some(evidence)) = observed {
                let attempts = previous["attempts"].as_u64().unwrap_or(0);
                executor_message::finish_exact_attempt(
                    &run.receipt,
                    &id,
                    &composed.text,
                    &evidence,
                    previous["expectedTurnId"].as_str(),
                    attempts,
                    reply_to,
                )?;
                let current = executor_message::read_exact_attempt(&run.receipt, &id)?
                    .unwrap_or_else(|| previous.clone());
                let delivery = delivery_from_entry(&current);
                let mut result = delivery_result(
                    run,
                    "lead-to-executor",
                    &id,
                    None,
                    &delivery,
                    Some(&client),
                    false,
                );
                if let Some(request_id) = reply_to {
                    result["resolved"] =
                        Value::Bool(receipt_question_status(run, request_id)? == "resolved");
                }
                return Ok(result);
            }
            if status == "pending" {
                let attempts = previous["attempts"].as_u64().unwrap_or(0);
                let unknown = ExactDelivery {
                    status: "indeterminate",
                    method: previous["method"].as_str().unwrap_or_default().to_owned(),
                    turn_id: previous["turnId"].as_str().map(str::to_owned),
                    evidence: None,
                    detail:
                        "an earlier attempt stopped before its outcome was recorded and the executor conversation's own items do not show it"
                            .to_owned(),
                };
                executor_message::finish_exact_attempt(
                    &run.receipt,
                    &id,
                    &composed.text,
                    &unknown,
                    previous["expectedTurnId"].as_str(),
                    attempts,
                    reply_to,
                )?;
                let current = executor_message::read_exact_attempt(&run.receipt, &id)?
                    .unwrap_or_else(|| unknown_value(&unknown));
                let delivery = delivery_from_entry(&current);
                return Ok(delivery_result(
                    run,
                    "lead-to-executor",
                    &id,
                    None,
                    &delivery,
                    Some(&client),
                    false,
                ));
            }
            let delivery = delivery_from_entry(previous);
            return Ok(delivery_result(
                run,
                "lead-to-executor",
                &id,
                None,
                &delivery,
                Some(&client),
                false,
            ));
        }
    }
    let endpoint = run.endpoint.as_ref().ok_or_else(|| {
        invalid(format!(
            "the originating lead has no verified live endpoint ({}); nothing was sent",
            run.endpoint_unavailable
                .as_deref()
                .unwrap_or("not recorded")
        ))
    })?;
    let mut conversation = executor_message::prepare_exact_thread(
        endpoint,
        &run.executor_thread,
        Some(&run.checkout),
        run.identity.as_ref(),
    )?;
    let attempts = previous
        .as_ref()
        .and_then(|entry| entry["attempts"].as_u64())
        .unwrap_or(0)
        + 1;
    executor_message::begin_exact_attempt(&run.receipt, &id, &composed.text, attempts, reply_to)?;
    let delivery = executor_message::send_exact_thread(
        &mut conversation,
        &run.executor_thread,
        &id,
        &composed.text,
    )
    .unwrap_or_else(|error| ExactDelivery {
        status: "indeterminate",
        method: String::new(),
        turn_id: None,
        evidence: None,
        detail: format!(
            "the native send could not be completed before the request was submitted: {error}; whether it reached the executor is unknown"
        ),
    });
    executor_message::finish_exact_attempt(
        &run.receipt,
        &id,
        &composed.text,
        &delivery,
        None,
        attempts,
        reply_to,
    )?;
    let delivery_value = serde_json::to_value(&delivery)?;
    let mut result = delivery_result(
        run,
        "lead-to-executor",
        &id,
        None,
        &delivery_value,
        Some(&client),
        false,
    );
    if let Some(request_id) = reply_to {
        result["resolved"] = Value::Bool(
            delivery.status == "delivered"
                && receipt_question_status(run, request_id)? == "resolved",
        );
    }
    Ok(result)
}

fn receipt_question_status(run: &VerifiedRun, id: &str) -> io::Result<String> {
    let bytes = fs::read(&run.receipt)?;
    let value: Value = serde_json::from_slice(&bytes)?;
    Ok(value["leadMessages"]
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["id"].as_str() == Some(id))
        })
        .and_then(|entry| entry["status"].as_str())
        .unwrap_or_default()
        .to_owned())
}

fn unknown_value(delivery: &ExactDelivery) -> Value {
    json!({
        "status": delivery.status,
        "method": delivery.method,
        "turnId": delivery.turn_id,
        "evidence": delivery.evidence,
        "attempts": Value::Null,
        "detail": delivery.detail
    })
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
