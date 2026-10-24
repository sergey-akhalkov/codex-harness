//! The executor MCP surface: dispatch, bounded status, native message/reply,
//! isolated stop, explicit continuation and explicit release. Authority is Codex request
//! `_meta.threadId`; runs are resolved from the existing MCP receipt and its
//! slot/app-server lease. Dispatch, delivery, stop containment and pool reset
//! stay in the existing executor owners; this module selects and validates the
//! binding.

use crate::{
    executor_assignment::{Assignment, AssignmentContext},
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
    task_worktree::{self, LaneDisposition, SlotDisposition, SlotState},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const DISPATCH_TOOL: &str = "executor_dispatch";
const LEAD_DISPATCH_TOOL: &str = "lead_dispatch";
const STATUS_TOOL: &str = "executor_status";
const WAIT_TOOL: &str = "executor_wait";
const MESSAGE_TOOL: &str = "executor_message";
const REPLY_TOOL: &str = "executor_reply";
const ASK_TOOL: &str = "executor_ask_lead";
const STOP_TOOL: &str = "executor_stop";
const RESUME_TOOL: &str = "executor_resume";
const RELEASE_TOOL: &str = "executor_release";
const APPROVAL_POLICY: &str = "never";
const MAX_RECEIPTS: usize = 256;
const MAX_STATUS_RUNS: usize = 8;
const MAX_STATUS_REQUESTS: usize = 16;
const MAX_PROGRESS_BYTES: usize = 4 * 1024;
const DEFAULT_WAIT_MS: u64 = 1_800_000;
const MAX_WAIT_MS: u64 = 1_800_000;
const WAIT_POLL: Duration = Duration::from_millis(100);
const WAIT_POLL_MAX: Duration = Duration::from_secs(1);
const STATUS_NATIVE_READ_BOUND: Duration = Duration::from_secs(1);
const MAX_RESULT_ACKS: usize = 8;
const MAX_WAIT_MESSAGE_BYTES: usize = 32 * 1024;
const MAX_WAIT_TURN_PAGES: usize = 64;
const EXECUTOR_TOOLS: &[&str] = &["executor_ask_lead", "executor_message", "executor_status"];
const LEAD_TOOLS: &[&str] = &[
    "executor_dispatch",
    "executor_status",
    "executor_wait",
    "executor_message",
    "executor_reply",
    "executor_ask_lead",
    "executor_stop",
    "executor_resume",
    "executor_release",
];

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
            "codex-harness mcp executors\nServe the installed executor MCP plugin over stdio. The caller identity comes only from Codex-injected request `_meta.threadId`. Tools dispatch one executor on the originating app-server, exchange addressed messages or questions, read bounded status, wait for one exact run's question or terminal result, stop one exact run through native turn interruption, explicitly continue that run, and explicitly release an accepted or discarded run through the existing pool owner. No run, thread, endpoint or recipient is accepted from tool arguments as authority."
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
            lead_dispatch_tool_definition(),
            status_tool_definition(),
            wait_tool_definition(),
            message_tool_definition(),
            reply_tool_definition(),
            ask_lead_tool_definition(),
            stop_tool_definition(),
            resume_tool_definition(),
            release_tool_definition(),
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

fn lead_dispatch_tool_definition() -> Value {
    json!({
        "name": LEAD_DISPATCH_TOOL,
        "description": "Director-only dispatch of one delegated Lead into a pooled workspace and a new thread on this Director's exact Codex app-server. The assignment must name a prepared OpenSpec change with its committed planning inputs, repositories, board task, objective and acceptance. No caller-supplied profile, model, parent, thread or endpoint is accepted.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "required": ["assignment"],
            "properties": {
                "owner": {
                    "type": "string",
                    "description": "Optional fresh run label (ASCII letters, digits, '-', '_' or '.', at most 128 characters). Omitted generates one from the caller suffix and a nanosecond timestamp."
                },
                "assignment": {
                    "type": "object",
                    "description": "Structured Lead assignment, schema 1. It must be a complete schema-1 Assignment (objective, inputs, outputs, invariants, acceptance, board_task) with lead_change naming the prepared OpenSpec change, planning root/base, directory and repository bases. The server validates it before any allocation or model work."
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

fn wait_tool_definition() -> Value {
    json!({
        "name": WAIT_TOOL,
        "description": "Wait up to timeoutMs across the calling lead's eligible unreleased runs. Omitted run observes all of them; an explicit run reads one. A pending question returns immediately with its exact request id. Otherwise this reads retained native state at a bounded interval and returns current terminal results, idle, a stable timeout, or an observation failure. Pass result ids in acknowledge only after receiving them; acknowledgement records receipt, not acceptance, release or question resolution. It starts no model turn and requires no picker navigation.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "run": {
                    "type": "string",
                    "description": "Optional exact run id returned by executor_status. Omit to observe every eligible run owned by the calling lead; an explicit run reads only that verified run."
                },
                "timeoutMs": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": MAX_WAIT_MS,
                    "default": DEFAULT_WAIT_MS,
                    "description": "Maximum wait in milliseconds. Omitted uses and caps at 1800000."
                },
                "acknowledge": {
                    "type": "array",
                    "items": {"type": "string"},
                    "maxItems": MAX_RESULT_ACKS,
                    "description": "Result ids returned by a previous executor_wait call. Acknowledges receipt only; retry-safe and never accepts work, releases a slot or resolves a question."
                }
            }
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

fn release_tool_definition() -> Value {
    json!({
        "name": RELEASE_TOOL,
        "description": "Explicitly release one executor run owned by the verified calling lead after its active work has ended. Requires a merged or discarded disposition and a nonempty reason. Refuses an active turn, unresolved request, stale or foreign lease, or ambiguous target before effects. Reuses the existing pool release owner: a clean or explicitly discarded slot is reset to the selected base with eligible ignored caches retained; dirty work under a merged disposition is preserved. Native thread archive alone never frees a slot.",
        "inputSchema": {
            "type": "object",
            "additionalProperties": false,
            "required": ["disposition", "reason"],
            "properties": {
                "run": {
                    "type": "string",
                    "description": "Optional exact run id returned by executor_status. Omitted selects the calling lead's sole recorded run; it never establishes authority by itself."
                },
                "disposition": {
                    "type": "string",
                    "enum": ["merged", "discarded"],
                    "description": "Explicit review disposition. merged resets a clean run to the selected base; discarded also authorizes deleting the run's unreviewed changes."
                },
                "reason": {
                    "type": "string",
                    "description": "Nonempty reason recorded with the disposition in the run receipt and slot state."
                },
                "base": {
                    "type": "string",
                    "description": "Optional committed base selected in the repository. Omitted uses the repository's current committed HEAD."
                }
            }
        }
    })
}

fn perform(operation: &Operation) -> io::Result<Value> {
    match operation.name.as_str() {
        DISPATCH_TOOL => dispatch(operation),
        LEAD_DISPATCH_TOOL => lead_dispatch(operation),
        STATUS_TOOL => status(operation),
        WAIT_TOOL => wait(operation),
        MESSAGE_TOOL => message(operation),
        REPLY_TOOL => reply(operation),
        ASK_TOOL => ask_lead(operation),
        STOP_TOOL => stop(operation),
        RESUME_TOOL => resume(operation),
        RELEASE_TOOL => release(operation),
        other => Err(invalid(format!("unknown executor MCP tool '{other}'"))),
    }
}

fn tool_response(result: io::Result<Value>, operation: &Operation) -> Value {
    let (structured, is_error) = match result {
        Ok(value) => {
            let is_error = value.get("error").is_some()
                || value.get("resolutionError").is_some()
                || value.get("cancellationError").is_some()
                || value.get("result").is_some_and(result_is_error)
                || value
                    .get("results")
                    .and_then(Value::as_array)
                    .is_some_and(|results| results.iter().any(result_is_error))
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

fn result_is_error(result: &Value) -> bool {
    result.get("error").is_some()
        || matches!(
            result["outcome"].as_str(),
            Some("partial") | Some("unsupported") | Some("error")
        )
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

/// A thread-local executors server table with the role's tool allowlist.
///
/// shortcut: Codex 0.164.0 rejects a leaf allowlist for a plugin-generated
/// server, so the full server table is repeated per thread and the filtered
/// thread loses `pluginId`; upgrade if a plugin-preserving per-thread filter
/// becomes available.
fn executors_tool_filter(source: &Path, tools: &[&str]) -> io::Result<Value> {
    let path = source.join(".agents/plugins/executors/.mcp.json");
    let value: Value = serde_json::from_slice(&fs::read(&path).map_err(|error| {
        invalid(format!(
            "the installed executors plugin manifest {} is unreadable: {error}",
            path.display()
        ))
    })?)
    .map_err(|error| {
        invalid(format!(
            "the installed executors plugin manifest {} is invalid: {error}",
            path.display()
        ))
    })?;
    let server = &value["mcpServers"]["executors"];
    let command = server["command"].as_str().ok_or_else(|| {
        invalid("the installed executors plugin manifest records no server command")
    })?;
    let args = server["args"].as_array().ok_or_else(|| {
        invalid("the installed executors plugin manifest records no server arguments")
    })?;
    let mut table = json!({
        "command": command,
        "args": args,
        "enabled_tools": tools,
    });
    if let Some(timeout) = server["tool_timeout_sec"].as_u64() {
        table["tool_timeout_sec"] = Value::from(timeout);
    }
    Ok(json!({"executors": table}))
}

/// Verify the started child's thread-local catalogue before any turn starts.
/// The full-table override is only useful if the native runtime actually
/// applies it; an absent or different catalogue is a capability failure that
/// preserves the created thread and pool claim for inspection.
fn verify_role_tools(
    conversation: &mut Conversation,
    thread_id: &str,
    allowed: &[&str],
) -> io::Result<()> {
    let answer = conversation.call(
        "mcpServerStatus/list",
        json!({
            "threadId": thread_id,
            "detail": "toolsAndAuthOnly",
            "limit": 100
        }),
    )?;
    if !answer["nextCursor"].is_null() {
        return Err(invalid(
            "the thread-local MCP catalogue returned more than one page; refusing an unverified role surface",
        ));
    }
    let data = answer["data"]
        .as_array()
        .ok_or_else(|| invalid("the thread-local MCP catalogue returned no data array"))?;
    let server = data
        .iter()
        .find(|server| server["name"] == "executors")
        .ok_or_else(|| {
            invalid("the thread-local MCP catalogue has no 'executors' server entry; the role filter was not applied")
        })?;
    let tools = server["tools"]
        .as_object()
        .ok_or_else(|| invalid("the thread-local 'executors' catalogue carries no tools object"))?;
    let actual: BTreeSet<&str> = tools.keys().map(String::as_str).collect();
    let expected: BTreeSet<&str> = allowed.iter().copied().collect();
    if actual != expected {
        return Err(invalid(format!(
            "the thread-local executors catalogue differs from the role allowlist: expected [{}], observed [{}]",
            expected.into_iter().collect::<Vec<_>>().join(", "),
            actual.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }
    Ok(())
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
struct LeadDispatchArgs {
    owner: Option<String>,
    assignment: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusArgs {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WaitArgs {
    #[serde(default)]
    run: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    acknowledge: Vec<String>,
}

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseArgs {
    #[serde(default)]
    run: Option<String>,
    disposition: String,
    reason: String,
    #[serde(default)]
    base: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunSide {
    Parent,
    Child,
}

struct RunReceipt {
    path: PathBuf,
    value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallerRole {
    Director,
    DelegatedLead,
    Executor,
}

struct CallerAuthority {
    role: CallerRole,
    root_thread: String,
}

/// Resolve the caller from Codex-injected identity and retained receipts. A
/// recorded child is an Executor or delegated Lead; an unrecorded thread is a
/// Director only when its registered root endpoint is live. The parent and
/// root of a new child come from this authority, never from tool arguments.
fn caller_authority(home: &Path, caller: &str) -> io::Result<CallerAuthority> {
    let own: Vec<Value> = mcp_receipts(home)?
        .into_iter()
        .filter(|receipt| receipt["executorThreadId"].as_str() == Some(caller))
        .collect();
    if own.len() > 1 {
        return Err(invalid(
            "the calling thread is recorded as the child of more than one run; refusing an ambiguous hierarchy identity",
        ));
    }
    if let Some(receipt) = own.first() {
        if !receipt["release"].is_null() {
            return Err(invalid(
                "the calling run is released and cannot dispatch descendants; resume or dispatch a fresh run",
            ));
        }
        return match receipt["role"].as_str().unwrap_or("executor") {
            "lead" => {
                let root = receipt["rootThreadId"]
                    .as_str()
                    .or_else(|| receipt["parentThreadId"].as_str())
                    .or_else(|| receipt["leadThreadId"].as_str())
                    .filter(|root| valid_thread_id(root))
                    .ok_or_else(|| {
                        invalid(
                            "the recorded delegated Lead run has no usable root thread; refusing to guess the hierarchy",
                        )
                    })?;
                Ok(CallerAuthority {
                    role: CallerRole::DelegatedLead,
                    root_thread: root.to_owned(),
                })
            }
            "executor" => Ok(CallerAuthority {
                role: CallerRole::Executor,
                root_thread: receipt["rootThreadId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            }),
            other => Err(invalid(format!(
                "the recorded run has unsupported child role {other:?}; refusing to infer authority"
            ))),
        };
    }
    match control::registered_lead(home, caller) {
        RegisteredLead::Live(_) => Ok(CallerAuthority {
            role: CallerRole::Director,
            root_thread: caller.to_owned(),
        }),
        RegisteredLead::Unavailable(reason) => Err(invalid(format!(
            "{reason}; no app-server was contacted. Remedy: restart this lead through the installed codex-harness launcher so its native thread endpoint is registered, then retry"
        ))),
    }
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
    child_role: RunRole,
    side: RunSide,
    run: String,
    lead_thread: String,
    executor_thread: String,
    parent_thread: String,
    root_thread: String,
    generation: String,
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
    let authority = caller_authority(&codex_home, &caller)?;
    if authority.role == CallerRole::Executor {
        return Err(invalid(
            "the calling thread is a recorded executor and cannot dispatch descendants; no endpoint, lease, thread or model work was touched",
        ));
    }
    let endpoint = match control::registered_lead(&codex_home, &caller) {
        RegisteredLead::Live(endpoint) => endpoint,
        RegisteredLead::Unavailable(reason) => {
            return Err(invalid(format!(
                "{reason}; no app-server was contacted. Remedy: restart this lead through the installed codex-harness launcher so its native thread endpoint is registered, then retry executor_dispatch from that live lead session"
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
    let filter = match executors_tool_filter(&source, EXECUTOR_TOOLS) {
        Ok(filter) => filter,
        Err(error) => return Err(prepared.release(error)),
    };
    plan.overrides.insert("mcp_servers".into(), filter);
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
    if let Err(error) = verify_role_tools(&mut conversation, &executor_thread, EXECUTOR_TOOLS) {
        return Err(io::Error::new(
            error.kind(),
            format!(
                "{error}; executor thread {executor_thread} was created and the pool claim is preserved because the role tool surface could not be verified"
            ),
        ));
    }
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
        "role": "executor",
        "parentThreadId": caller,
        "rootThreadId": authority.root_thread,
        "runGeneration": prepared.owner,
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
    if result["receiptError"].is_null()
        && let Err(error) = verified_receipt(&codex_home, &prepared.receipt, &caller)
            .and_then(|run| ensure_cache_guard(&codex_home, &run))
    {
        result["cacheGuardError"] = json!(error.to_string());
    }
    Ok(result)
}

/// Director-only dispatch of one prepared OpenSpec change to a delegated Lead.
/// The change is validated against the Director's repository before any slot
/// claim or model effect; the Lead thread runs on the Director's app-server
/// and its endpoint is registered so the Lead can dispatch Executors.
fn lead_dispatch(operation: &Operation) -> io::Result<Value> {
    let caller = caller_thread(&operation.metadata)?;
    let args: LeadDispatchArgs = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("lead_dispatch arguments are invalid: {error}")))?;
    let owner = args.owner.unwrap_or_else(|| generated_owner(&caller));
    let assignment_value = args.assignment.clone();
    let assignment = Assignment::from_value(args.assignment)?;
    let codex_home = native_launcher::codex_home()?;
    let authority = caller_authority(&codex_home, &caller)?;
    if authority.role != CallerRole::Director {
        return Err(invalid(
            "only a verified Director may dispatch a Lead; a delegated Lead or Executor cannot create another Lead and no model work was touched",
        ));
    }
    let endpoint = match control::registered_lead(&codex_home, &caller) {
        RegisteredLead::Live(endpoint) => endpoint,
        RegisteredLead::Unavailable(reason) => {
            return Err(invalid(format!(
                "{reason}; no app-server was contacted. Remedy: restart this Director through the installed codex-harness launcher, then retry lead_dispatch"
            )));
        }
    };
    let app_server = endpoint.process.clone().ok_or_else(|| {
        invalid(
            "the registered Director endpoint records no app-server process, so an exact run lease cannot be recorded; no thread was created",
        )
    })?;

    let mut director = Conversation::attach(&endpoint, DEFAULT_BOUND)?;
    let director_thread = director.read_thread_state(&caller)?;
    require_loaded_lead(&director_thread, &caller)?;
    let cwd = director_thread["cwd"]
        .as_str()
        .ok_or_else(|| invalid("the registered Director thread reports no working directory"))?;
    let source = crate::executor_cli::executor_input::source(None, &codex_home)?;
    let repository = repository_root(Path::new(cwd))?;
    let bd = crate::feedback_cli::discover_bd_in(&codex_home).map_err(|error| {
        invalid(format!(
            "Lead dispatch requires the consumer board executable: {error}"
        ))
    })?;
    let context = AssignmentContext {
        checkout: &repository,
        base: "",
        owner: &owner,
        source: &source,
    };
    let evidence =
        crate::executor_assignment::validate_lead_assignment(&assignment, &context, &bd)?;
    let prepared = executor_cli::prepare_mcp_lead_dispatch(
        &codex_home,
        &source,
        &repository,
        &owner,
        assignment,
        evidence,
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
    let filter = match executors_tool_filter(&source, LEAD_TOOLS) {
        Ok(filter) => filter,
        Err(error) => return Err(prepared.release(error)),
    };
    plan.overrides.insert("mcp_servers".into(), filter);
    plan.delegated_lead_session = true;
    if let Some(sandbox) = director_thread["sandbox"].as_str() {
        plan.sandbox = Some(sandbox.to_owned());
    }

    let mut conversation = match Conversation::start_on_endpoint(&endpoint, &plan) {
        Ok(conversation) => conversation,
        Err(error) => return Err(prepared.release(error)),
    };
    let lead_thread = conversation.thread_id().to_owned();
    let thread = match conversation.read_thread_state(&lead_thread) {
        Ok(thread) => thread,
        Err(error) => {
            return Err(prepared.release(io::Error::new(
                error.kind(),
                format!("delegated Lead thread {lead_thread} could not be confirmed: {error}"),
            )));
        }
    };
    let observed = match confirm_started(
        &thread,
        &lead_thread,
        &prepared.checkout,
        &identity,
        &prepared.title,
    ) {
        Ok(status) => status,
        Err(error) => return Err(prepared.release(error)),
    };
    if let Err(error) = verify_role_tools(&mut conversation, &lead_thread, LEAD_TOOLS) {
        return Err(io::Error::new(
            error.kind(),
            format!(
                "{error}; delegated Lead thread {lead_thread} was created and the pool claim is preserved because the role tool surface could not be verified"
            ),
        ));
    }
    if let Err(error) =
        control::publish_lead_endpoint(&codex_home, &endpoint.for_thread(&lead_thread)?)
    {
        return Err(io::Error::new(
            error.kind(),
            format!(
                "delegated Lead thread {lead_thread} was created but its endpoint could not be published: {error}; the run and pool claim are preserved"
            ),
        ));
    }
    let turn = match conversation.assign(&prepared.prompt) {
        Ok(turn) => turn,
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!(
                    "{error}; delegated Lead thread {lead_thread} was created and the pool claim is preserved because turn/start may have been accepted"
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
        "role": "lead",
        "parentThreadId": caller,
        "rootThreadId": caller,
        "runGeneration": prepared.owner,
        "leadThreadId": caller,
        "executorThreadId": lead_thread,
        "turnId": turn.turn_id,
        "assignment": assignment_value,
        "leadChange": prepared.lead_change,
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
    // A delegated Lead runs in a linked worktree. Its pool is the repository's
    // shared pool, so resolve the main worktree from the common Git directory
    // instead of treating the child worktree as a separate repository.
    let common = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "--git-common-dir"])
        .output();
    if let Ok(common) = common
        && common.status.success()
        && let Ok(text) = std::str::from_utf8(&common.stdout)
    {
        let mut path = PathBuf::from(text.trim());
        if path.is_relative() {
            path = root.join(path);
        }
        if let Some(main) = path.parent()
            && main.is_dir()
        {
            return main.canonicalize();
        }
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
            let parent = receipt.value["parentThreadId"]
                .as_str()
                .or_else(|| receipt.value["leadThreadId"].as_str());
            let child_role = receipt.value["role"].as_str().unwrap_or("executor");
            parent == Some(caller)
                || (child_role == "executor"
                    && receipt.value["executorThreadId"].as_str() == Some(caller))
        })
        .collect())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunView {
    Live,
    Release,
    Status,
}

fn verify_run(home: &Path, receipt: RunReceipt, caller: &str) -> io::Result<VerifiedRun> {
    verify_run_for(home, receipt, caller, RunView::Live)
}

fn verify_run_for(
    home: &Path,
    receipt: RunReceipt,
    caller: &str,
    view: RunView,
) -> io::Result<VerifiedRun> {
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
    let parent_thread = value["parentThreadId"]
        .as_str()
        .unwrap_or(&lead_thread)
        .to_owned();
    let root_thread = value["rootThreadId"]
        .as_str()
        .unwrap_or(&lead_thread)
        .to_owned();
    if !valid_thread_id(&parent_thread) || !valid_thread_id(&root_thread) {
        return Err(invalid(
            "the MCP run receipt records a malformed parent or root thread id",
        ));
    }
    let child_role = match value["role"].as_str() {
        None | Some("executor") => RunRole::Executor,
        Some("lead") => RunRole::Lead,
        Some(other) => {
            return Err(invalid(format!(
                "the MCP run receipt records unsupported child role {other:?}; no identity was accepted"
            )));
        }
    };
    let side = if caller == parent_thread {
        RunSide::Parent
    } else if caller == executor_thread {
        RunSide::Child
    } else {
        return Err(invalid(
            "the calling thread is not the recorded lead or executor of this run; no operation was performed",
        ));
    };
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
    let exact_owner = record.owner.as_deref() == Some(run.as_str());
    let exact_checkout = record.index == slot && same_path(&record.path, &checkout);
    let release_recorded = !value["release"].is_null();
    let released_with_record = release_recorded
        && exact_checkout
        && match record.state {
            SlotState::Released => {
                record.owner.is_none() && value["release"]["slotFreed"].as_bool() == Some(true)
            }
            SlotState::AwaitingReview => exact_owner,
            _ => false,
        };
    let allowed = match view {
        RunView::Live => exact_owner && matches!(record.state, SlotState::Occupied),
        RunView::Release => {
            (exact_owner
                && matches!(
                    record.state,
                    SlotState::Occupied | SlotState::AwaitingReview
                ))
                || released_with_record
        }
        RunView::Status => {
            (exact_owner
                && matches!(
                    record.state,
                    SlotState::Occupied | SlotState::AwaitingReview
                ))
                || released_with_record
        }
    };
    if !exact_checkout || !allowed {
        return Err(invalid(format!(
            "slot {slot} is not the recorded run's {}: owner {:?}, state {:?}, checkout {}; no operation was performed",
            match view {
                RunView::Live => "live lease",
                RunView::Release | RunView::Status => "recorded ownership",
            },
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
                // A released run keeps its receipt after the app-server that
                // hosted it is replaced. Status and repeated release may still
                // report that record; they must not reject it as a foreign live
                // lease.
                if view != RunView::Live && !value["release"].is_null() {
                    (
                        None,
                        Some(
                            "the registered lead endpoint no longer matches the released run lease; native status is unavailable"
                                .to_owned(),
                        ),
                    )
                } else {
                    return Err(invalid(
                        "the registered lead endpoint no longer matches the exact run lease; the run was replaced, resumed elsewhere or is stale, and no operation was performed",
                    ));
                }
            } else {
                (Some(*endpoint), None)
            }
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
    let generation = value["runGeneration"].as_str().unwrap_or(&run).to_owned();
    Ok(VerifiedRun {
        receipt: receipt.path,
        value: receipt.value,
        role,
        child_role,
        side,
        run,
        lead_thread,
        executor_thread,
        parent_thread,
        root_thread,
        generation,
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
    native_thread_bounded(run, thread_id, executor_side, DEFAULT_BOUND)
}

fn native_thread_bounded(
    run: &VerifiedRun,
    thread_id: &str,
    executor_side: bool,
    bound: Duration,
) -> io::Result<Value> {
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
    let mut conversation = Conversation::attach(&endpoint, bound)?;
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

fn native_status_or_release(
    run: &VerifiedRun,
    thread_id: &str,
    executor_side: bool,
    bound: Duration,
) -> Value {
    match native_thread_bounded(run, thread_id, executor_side, bound) {
        Ok(native) => native,
        Err(error) => json!({
            "threadId": thread_id,
            "status": "unavailable",
            "error": error.to_string(),
            "detail": if run.value["release"].is_null() {
                "native status could not be read within the status bound; the run receipt and lease remain authoritative"
            } else {
                "the run's explicit release record remains authoritative; native archive or a missing thread does not reopen or refill the slot"
            }
        }),
    }
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
                "contentId": entry["contentId"],
                "method": entry["method"],
                "turnId": entry["turnId"],
                "attempts": entry["attempts"],
                "requestedMs": entry["requestedMs"],
                "recordedMs": entry["recordedMs"],
                "deliveredMs": entry["deliveredMs"],
                "payloadBytes": entry["payloadBytes"].as_u64().or_else(|| entry["bytes"].as_u64()),
                "payloadPath": entry["payloadPath"],
                "detail": entry["detail"].as_str().map(|detail| excerpt(detail, 400)),
                "resolvedMs": entry["resolvedMs"],
                "resolvedBy": entry["resolvedBy"]
            })
        })
        .collect();
    let truncated = pending.len() > MAX_STATUS_REQUESTS;
    pending.truncate(MAX_STATUS_REQUESTS);
    (pending, truncated)
}

fn wait(operation: &Operation) -> io::Result<Value> {
    let caller = caller_thread(&operation.metadata)?;
    let args: WaitArgs = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("executor_wait arguments are invalid: {error}")))?;
    let timeout_ms = args.timeout_ms.unwrap_or(DEFAULT_WAIT_MS);
    if timeout_ms > MAX_WAIT_MS {
        return Err(invalid(format!(
            "executor_wait timeoutMs is {timeout_ms}; the limit is {MAX_WAIT_MS}. No wait was started"
        )));
    }
    if args.acknowledge.len() > MAX_RESULT_ACKS {
        return Err(invalid(format!(
            "executor_wait acknowledge has {} ids; the limit is {MAX_RESULT_ACKS}. No wait was started",
            args.acknowledge.len()
        )));
    }
    let total = Duration::from_millis(timeout_ms);
    let operation_budget = operation.deadline.remaining();
    if operation_budget < total.saturating_add(Duration::from_secs(1)) {
        return Err(invalid(format!(
            "executor_wait requested {timeout_ms} ms but only {} ms remain in this MCP request's budget; retry with a smaller timeoutMs. No wait was started",
            operation_budget.as_millis()
        )));
    }
    let home = native_launcher::codex_home()?;
    let targeted = args.run.is_some();
    let mut runs = match args.run.as_deref() {
        Some(run) => vec![lead_run_for_wait(&home, &caller, run)?],
        None => lead_runs_for_wait(&home, &caller)?,
    };
    if runs.is_empty() {
        return Ok(json!({"state":"idle"}));
    }
    for run in &runs {
        if let Err(error) = ensure_cache_guard(&home, run) {
            eprintln!(
                "codex-harness: cache guard for exact run {} could not start ({error}); no stop was attempted",
                run.run
            );
        }
    }
    let started = Instant::now();
    let deadline = started.checked_add(total).unwrap_or(started);
    let mut conversations = BTreeMap::new();
    let mut pending_acks = (!args.acknowledge.is_empty()).then_some(args.acknowledge);
    let mut acknowledged_this_call = BTreeSet::new();
    let mut poll = WAIT_POLL;
    loop {
        if operation.cancellation.is_cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "executor_wait was cancelled; no model work was started",
            ));
        }
        let mut refreshed = Vec::with_capacity(runs.len());
        for run in runs.drain(..) {
            match refresh_run_for(&home, &run, RunView::Status) {
                Ok(current) => {
                    if !targeted && !current.value["release"].is_null() {
                        continue;
                    }
                    refreshed.push(current);
                }
                Err(error) => {
                    return Ok(wait_observation_error(&run.run, error.to_string()));
                }
            }
        }
        runs = refreshed;
        if runs.is_empty() {
            return Ok(json!({"state":"idle"}));
        }
        let mut observed = Vec::with_capacity(runs.len());
        let mut pending = Vec::new();
        let mut pending_truncated = false;
        let mut in_progress = false;
        for run in &runs {
            let observation = match observe_wait_run(&mut conversations, run) {
                Ok(observation) => observation,
                Err(error) => return Ok(wait_observation_error(&run.run, error.to_string())),
            };
            pending_truncated |= observation.pending_truncated;
            for mut entry in observation.pending {
                if !targeted {
                    entry["run"] = json!(run.run);
                }
                pending.push(entry);
            }
            in_progress |= observation.in_progress;
            observed.push((run.run.clone(), observation.result));
        }
        if let Some(ids) = pending_acks.take() {
            let terminal: Vec<(&VerifiedRun, String, String)> = observed
                .iter()
                .filter_map(|(run_id, result)| {
                    let result = result.as_ref()?;
                    let run = runs.iter().find(|run| &run.run == run_id)?;
                    Some((
                        run,
                        result["resultId"].as_str()?.to_owned(),
                        result["turnId"].as_str().unwrap_or_default().to_owned(),
                    ))
                })
                .collect();
            for id in &ids {
                if id.is_empty() || id.len() > 256 {
                    return Err(invalid(
                        "executor_wait acknowledge contains an empty or oversized result id; no acknowledgement was written",
                    ));
                }
                let matches = terminal
                    .iter()
                    .filter(|(_, current, _)| current == id)
                    .count();
                if matches != 1 {
                    return Err(invalid(format!(
                        "executor_wait acknowledge id {id:?} is not exactly one current terminal result for the calling lead; no acknowledgement was written"
                    )));
                }
            }
            for id in &ids {
                let (run, _, turn_id) = terminal
                    .iter()
                    .find(|(_, current, _)| current == id)
                    .expect("validated acknowledgement");
                acknowledge_result(run, &caller, id, turn_id)?;
                acknowledged_this_call.insert(id.clone());
            }
        }
        let results: Vec<Value> = observed
            .into_iter()
            .filter_map(|(run_id, result)| {
                let result = result?;
                if targeted {
                    return Some(result);
                }
                let run = runs.iter().find(|run| run.run == run_id)?;
                let result_id = result["resultId"].as_str().unwrap_or_default();
                let turn_id = result["turnId"].as_str().unwrap_or_default();
                if acknowledged_this_call.contains(result_id)
                    || is_acknowledged(run, result_id, turn_id)
                {
                    return None;
                }
                Some(result)
            })
            .collect();
        if !pending.is_empty() || !results.is_empty() {
            return Ok(wait_event_response(
                targeted,
                timeout_ms,
                elapsed_ms(started),
                &runs,
                results,
                pending,
                pending_truncated,
            ));
        }
        if !targeted && !in_progress {
            return Ok(json!({"state":"idle"}));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(wait_timeout_response(targeted, &runs, timeout_ms));
        }
        std::thread::sleep(poll.min(remaining));
        poll = (poll * 2).min(WAIT_POLL_MAX);
    }
}

struct WaitObservation {
    result: Option<Value>,
    pending: Vec<Value>,
    pending_truncated: bool,
    in_progress: bool,
}

fn observe_wait_run(
    conversations: &mut BTreeMap<String, Conversation>,
    run: &VerifiedRun,
) -> io::Result<WaitObservation> {
    let (pending, pending_truncated) = pending_requests(run);
    let endpoint = run.endpoint.as_ref().ok_or_else(|| {
        invalid(format!(
            "the originating lead has no verified live endpoint ({})",
            run.endpoint_unavailable
                .as_deref()
                .unwrap_or("not recorded")
        ))
    })?;
    let conversation = conversation_for(conversations, endpoint)?;
    let result = observe_executor(conversation, run)?.map(|mut result| {
        add_result_identity(run, &mut result);
        result
    });
    Ok(WaitObservation {
        in_progress: result.is_none(),
        result,
        pending,
        pending_truncated,
    })
}

fn conversation_for<'a>(
    conversations: &'a mut BTreeMap<String, Conversation>,
    endpoint: &Endpoint,
) -> io::Result<&'a mut Conversation> {
    let key = format!("{}:{}", endpoint.port(), endpoint.token());
    if !conversations.contains_key(&key) {
        conversations.insert(key.clone(), Conversation::attach(endpoint, WAIT_POLL_MAX)?);
    }
    Ok(conversations
        .get_mut(&key)
        .expect("the endpoint conversation was inserted"))
}

fn add_result_identity(run: &VerifiedRun, result: &mut Value) {
    let result_id = result_identity(run, result);
    result["run"] = json!(run.run);
    result["resultId"] = json!(result_id);
}

fn result_identity(run: &VerifiedRun, result: &Value) -> String {
    let turn = result["turnId"]
        .as_str()
        .filter(|turn| !turn.is_empty())
        .unwrap_or("unknown");
    let status = result["status"].as_str().unwrap_or("unknown");
    let outcome = result["outcome"].as_str().unwrap_or("unknown");
    digest_id(
        "wait-result-",
        &[&run.run, &run.executor_thread, turn, status, outcome],
    )
}

fn is_acknowledged(run: &VerifiedRun, result_id: &str, turn_id: &str) -> bool {
    run.value["waitAcknowledgements"]
        .as_array()
        .is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry["resultId"].as_str() == Some(result_id)
                    && entry["turnId"].as_str() == Some(turn_id)
            })
        })
}

fn acknowledge_result(
    run: &VerifiedRun,
    caller: &str,
    result_id: &str,
    turn_id: &str,
) -> io::Result<()> {
    executor_message::update_receipt_document(&run.receipt, |document| {
        if document["run"].as_str() != Some(run.run.as_str())
            || document["leadThreadId"].as_str() != Some(caller)
            || document["executorThreadId"].as_str() != Some(run.executor_thread.as_str())
            || document["slot"].as_u64() != Some(u64::from(run.slot))
            || document["turnId"].as_str().unwrap_or_default() != turn_id
        {
            return Err(invalid(
                "the MCP run receipt changed before its wait acknowledgement could be recorded; no acknowledgement was written",
            ));
        }
        let mut acknowledgements = document["waitAcknowledgements"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !acknowledgements
            .iter()
            .any(|entry| entry["resultId"].as_str() == Some(result_id))
        {
            acknowledgements.push(json!({
                "resultId": result_id,
                "turnId": turn_id,
                "acknowledgedMs": executor_message_now()
            }));
        }
        if acknowledgements.len() > MAX_RESULT_ACKS {
            acknowledgements.drain(..acknowledgements.len() - MAX_RESULT_ACKS);
        }
        document["waitAcknowledgements"] = Value::Array(acknowledgements);
        Ok(())
    })
}

fn wait_event_response(
    targeted: bool,
    timeout_ms: u64,
    elapsed_ms: u64,
    runs: &[VerifiedRun],
    mut results: Vec<Value>,
    mut pending: Vec<Value>,
    pending_truncated: bool,
) -> Value {
    let pending_truncated = pending_truncated || pending.len() > MAX_STATUS_REQUESTS;
    pending.truncate(MAX_STATUS_REQUESTS);
    let state = if pending.is_empty() {
        "result"
    } else {
        "question"
    };
    if targeted {
        let result = if results.is_empty() {
            Value::Null
        } else {
            results.remove(0)
        };
        json!({
            "state": state,
            "run": runs.first().map(|run| run.run.clone()).unwrap_or_default(),
            "timeoutMs": timeout_ms,
            "elapsedMs": elapsed_ms,
            "result": result,
            "pendingRequests": pending,
            "pendingRequestsTruncated": pending_truncated
        })
    } else {
        let results_truncated = results.len() > MAX_STATUS_RUNS;
        results.truncate(MAX_STATUS_RUNS);
        json!({
            "state": state,
            "timeoutMs": timeout_ms,
            "elapsedMs": elapsed_ms,
            "results": results,
            "resultsTruncated": results_truncated,
            "pendingRequests": pending,
            "pendingRequestsTruncated": pending_truncated
        })
    }
}

fn wait_timeout_response(targeted: bool, runs: &[VerifiedRun], timeout_ms: u64) -> Value {
    if targeted {
        json!({
            "state": "timeout",
            "timeoutMs": timeout_ms,
            "run": runs.first().map(|run| run.run.clone()).unwrap_or_default()
        })
    } else {
        json!({"state":"timeout","timeoutMs":timeout_ms})
    }
}

fn wait_observation_error(run: &str, error: String) -> Value {
    json!({
        "state": "observation-error",
        "run": run,
        "error": format!("executor observation failed: {error}"),
        "detail": "the native thread, accepted turn or retained authority could not be read reliably; no completion, empty queue or normal timeout is claimed. Retry after the recorded run and lead endpoint are available."
    })
}

fn lead_runs_for_wait(home: &Path, caller: &str) -> io::Result<Vec<VerifiedRun>> {
    let receipts = read_receipts(home)?;
    let executor_only = receipts
        .iter()
        .any(|receipt| receipt.value["executorThreadId"].as_str() == Some(caller));
    let mut runs = Vec::new();
    for receipt in receipts {
        if receipt.value["leadThreadId"].as_str() != Some(caller)
            || !receipt.value["release"].is_null()
        {
            continue;
        }
        runs.push(verify_run_for(home, receipt, caller, RunView::Status)?);
    }
    if runs.is_empty() && executor_only {
        return Err(invalid(
            "executor_wait is available only to the verified lead of the exact run",
        ));
    }
    Ok(runs)
}

fn lead_run_for_wait(home: &Path, caller: &str, run_id: &str) -> io::Result<VerifiedRun> {
    if !valid_run_id(run_id) {
        return Err(invalid(format!(
            "run {run_id:?} is malformed; refusing before any run lookup or wait"
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
            "no recorded MCP executor run {run_id:?} is owned by the calling lead; no wait was started"
        ))),
        [receipt] => verify_run_for(
            home,
            RunReceipt {
                path: receipt.path.clone(),
                value: receipt.value.clone(),
            },
            caller,
            RunView::Status,
        ),
        _ => Err(invalid(format!(
            "run {run_id:?} matches more than one recorded run for the calling lead; refusing an ambiguous target"
        ))),
    }
}

fn refresh_run(home: &Path, previous: &VerifiedRun) -> io::Result<VerifiedRun> {
    refresh_run_for(home, previous, RunView::Live)
}

fn refresh_run_for(home: &Path, previous: &VerifiedRun, view: RunView) -> io::Result<VerifiedRun> {
    let bytes = fs::read(&previous.receipt).map_err(|error| {
        invalid(format!(
            "the MCP run receipt {} could not be refreshed: {error}",
            previous.receipt.display()
        ))
    })?;
    let value = serde_json::from_slice(&bytes).map_err(|error| {
        invalid(format!(
            "the MCP run receipt {} is not valid JSON: {error}",
            previous.receipt.display()
        ))
    })?;
    verify_run_for(
        home,
        RunReceipt {
            path: previous.receipt.clone(),
            value,
        },
        &previous.lead_thread,
        view,
    )
}

/// Reads only the current executor turn. The receipt's accepted turn must
/// still be in native history, while a later turn started by an addressed
/// message becomes the one that can finish this observation. A stale read that
/// lacks the accepted turn is an observation failure, never an older result.
fn observe_executor(
    conversation: &mut Conversation,
    run: &VerifiedRun,
) -> io::Result<Option<Value>> {
    let mut thread = conversation.read_thread_record(&run.executor_thread)?;
    if thread["id"].as_str() != Some(run.executor_thread.as_str()) {
        return Err(invalid(format!(
            "thread/read answered for {} instead of the recorded executor thread {}",
            thread["id"], run.executor_thread
        )));
    }
    if thread["turns"]
        .as_array()
        .is_none_or(|turns| turns.is_empty())
    {
        thread["turns"] = Value::Array(list_thread_turns(conversation, &run.executor_thread)?);
    }
    let thread_status = thread["status"]["type"]
        .as_str()
        .or_else(|| thread["status"].as_str())
        .unwrap_or("unknown");
    if thread_status == "systemError" {
        let cause = thread["error"]["message"]
            .as_str()
            .or_else(|| thread["status"]["message"].as_str())
            .unwrap_or("the native executor thread reported a system error");
        let turn_id = thread["turns"]
            .as_array()
            .and_then(|turns| turns.last())
            .and_then(|turn| turn["id"].as_str())
            .or_else(|| run.value["turnId"].as_str())
            .unwrap_or_default();
        let mut result = turn_result(run, turn_id, "systemError", "failed", "failed");
        result["error"] = Value::String(cause.to_owned());
        result["detail"] = Value::String(
            "the native thread reported a system error; no final result is claimed".into(),
        );
        return Ok(Some(result));
    }
    let turns = thread["turns"]
        .as_array()
        .filter(|turns| !turns.is_empty())
        .ok_or_else(|| {
            invalid(format!(
                "executor thread {} reports no turn history; its current turn cannot be observed",
                run.executor_thread
            ))
        })?;
    let accepted_turn = required_text(&run.value, "turnId")?;
    if !turns
        .iter()
        .any(|turn| turn["id"].as_str() == Some(accepted_turn))
    {
        return Err(invalid(format!(
            "executor thread {} no longer reports accepted turn {accepted_turn}; refusing to settle an older or unrelated turn",
            run.executor_thread
        )));
    }
    let turn = turns.last().expect("non-empty turn history");
    let turn_id = required_text(turn, "id")?;
    let status = required_text(turn, "status")?;
    match status {
        "completed" => {
            let final_message = turn["items"].as_array().and_then(|items| {
                items
                    .iter()
                    .rev()
                    .find(|item| item["type"] == "agentMessage")
            });
            let Some(final_message) = final_message else {
                let mut result =
                    turn_result(run, turn_id, status, "output-defect", "missing-output");
                result["output"] = Value::String("missing".into());
                result["error"] = Value::String(
                    "the current turn completed without an assistant message; this is a missing-output defect, not a provider failure or timeout"
                        .into(),
                );
                result["detail"] = Value::String(
                    "the native turn is terminal, but no final assistant message is available"
                        .into(),
                );
                return Ok(Some(result));
            };
            match final_message["text"].as_str() {
                Some(text) if !text.trim().is_empty() => {
                    let (text, truncated, bytes) = bounded_wait_text(text);
                    let mut result = turn_result(run, turn_id, status, "completed", "completed");
                    result["output"] = Value::String("present".into());
                    result["finalMessage"] = Value::String(text);
                    result["finalMessageBytes"] = Value::from(bytes);
                    result["finalMessageTruncated"] = Value::Bool(truncated);
                    result["detail"] = Value::String(
                        "the current turn completed with a final assistant message; this is observation, not lead acceptance or slot release"
                            .into(),
                    );
                    Ok(Some(result))
                }
                _ => {
                    let mut result =
                        turn_result(run, turn_id, status, "output-defect", "empty-output");
                    result["output"] = Value::String("empty".into());
                    result["error"] = Value::String(
                        "the current turn completed with an empty final assistant message; this is an output defect, not a provider failure or timeout"
                            .into(),
                    );
                    result["detail"] = Value::String(
                        "the native turn is terminal, but its final assistant message carries no text"
                            .into(),
                    );
                    Ok(Some(result))
                }
            }
        }
        "failed" => {
            let cause = turn["error"]["message"]
                .as_str()
                .unwrap_or("the native turn failed without a message");
            let mut result = turn_result(run, turn_id, status, "failed", "failed");
            result["error"] = Value::String(cause.to_owned());
            result["detail"] = Value::String(
                "the current native turn failed; partial work remains in the recorded checkout"
                    .into(),
            );
            Ok(Some(result))
        }
        "interrupted" => {
            let mut result = turn_result(run, turn_id, status, "stopped", "interrupted");
            result["detail"] = Value::String(
                "the current native turn was interrupted; the run was not completed or accepted"
                    .into(),
            );
            Ok(Some(result))
        }
        "inProgress" | "pending" | "queued" => Ok(None),
        other => Err(invalid(format!(
            "executor thread {} reported unknown turn status {other:?} for turn {turn_id}; refusing to guess a result",
            run.executor_thread
        ))),
    }
}

/// Reads native turn history through the paginated owner when `thread/read`
/// omits turns (the installed idle-thread fallback). Pagination is bounded; a
/// missing or malformed page is an observation failure.
fn list_thread_turns(conversation: &mut Conversation, thread_id: &str) -> io::Result<Vec<Value>> {
    let mut turns = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_WAIT_TURN_PAGES {
        let mut params = json!({
            "threadId": thread_id,
            "itemsView": "full",
            "sortDirection": "asc",
            "limit": 100
        });
        if let Some(cursor) = cursor.as_deref() {
            params["cursor"] = Value::String(cursor.to_owned());
        }
        let page = conversation.call("thread/turns/list", params)?;
        let data = page["data"].as_array().ok_or_else(|| {
            invalid(format!(
                "thread/turns/list for {thread_id} carried no data array; its history cannot be observed"
            ))
        })?;
        turns.extend(data.iter().cloned());
        match page["nextCursor"].as_str() {
            Some(next) if !next.is_empty() => cursor = Some(next.to_owned()),
            _ => return Ok(turns),
        }
    }
    Err(invalid(format!(
        "thread/turns/list for {thread_id} did not terminate within {MAX_WAIT_TURN_PAGES} pages; refusing an unbounded read"
    )))
}

fn turn_result(
    run: &VerifiedRun,
    turn_id: &str,
    status: &str,
    state: &str,
    outcome: &str,
) -> Value {
    json!({
        "state": state,
        "outcome": outcome,
        "status": status,
        "turnId": turn_id,
        "output": Value::Null,
        "finalMessage": Value::Null,
        "finalMessageBytes": Value::Null,
        "finalMessageTruncated": false,
        "native": {
            "threadId": run.executor_thread,
            "turnId": turn_id,
            "status": status
        },
        "cacheGuard": run.value["cacheGuard"],
        "detailLocator": {
            "threadId": run.executor_thread,
            "turnId": turn_id,
            "statusTool": STATUS_TOOL
        }
    })
}

fn bounded_wait_text(text: &str) -> (String, bool, usize) {
    let bytes = text.len();
    if bytes <= MAX_WAIT_MESSAGE_BYTES {
        return (text.to_owned(), false, bytes);
    }
    let mut end = MAX_WAIT_MESSAGE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true, bytes)
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn run_summary(run: &VerifiedRun) -> Value {
    json!({
        "run": run.run,
        "role": run.role.as_str(),
        "childRole": run.child_role.as_str(),
        "parentThreadId": run.parent_thread,
        "rootThreadId": run.root_thread,
        "runGeneration": run.generation,
        "title": run.title,
        "leadThreadId": run.lead_thread,
        "executorThreadId": run.executor_thread,
        "slot": run.slot,
        "source": run.source,
        "repository": run.repository,
        "checkout": run.checkout,
        "base": run.base,
        "profile": run.value["profile"],
        "cacheGuard": run.value["cacheGuard"],
        "stop": run.value["stop"],
        "continuation": run.value["continuation"],
        "release": run.value["release"],
        "predecessorThreadId": run.value["predecessorThreadId"],
        "receipt": run.receipt
    })
}

fn verified_receipt(home: &Path, receipt: &Path, caller: &str) -> io::Result<VerifiedRun> {
    verified_receipt_for(home, receipt, caller, RunView::Live)
}

fn verified_receipt_for(
    home: &Path,
    receipt: &Path,
    caller: &str,
    view: RunView,
) -> io::Result<VerifiedRun> {
    let bytes = fs::read(receipt).map_err(|error| {
        invalid(format!(
            "the MCP run receipt {} could not be re-read: {error}",
            receipt.display()
        ))
    })?;
    let value = serde_json::from_slice(&bytes).map_err(|error| {
        invalid(format!(
            "the MCP run receipt {} is not valid JSON: {error}",
            receipt.display()
        ))
    })?;
    verify_run_for(
        home,
        RunReceipt {
            path: receipt.to_owned(),
            value,
        },
        caller,
        view,
    )
}

/// Starts one background monitor for the exact current turn. The monitor uses
/// the existing detector and warning queue; this adapter supplies only the
/// exact receipt, thread, turn, lease and existing exact-thread stop.
fn ensure_cache_guard(home: &Path, run: &VerifiedRun) -> io::Result<()> {
    let stopped = run.value["stop"].is_object()
        && run.value["stop"]
            .get("continuedMs")
            .is_none_or(Value::is_null);
    if stopped {
        return Ok(());
    }
    let turn = required_text(&run.value, "turnId")?.to_owned();
    let cache_home = home.to_owned();
    let receipt = run.receipt.clone();
    let lead = run.lead_thread.clone();
    let thread = run.executor_thread.clone();
    let run_id = run.run.clone();
    let lease = run.value["lease"].clone();
    let guard_receipt = receipt.clone();
    let guard_lead = lead.clone();
    let guard_thread = thread.clone();
    let guard_turn = turn.clone();
    let guard_lease = lease.clone();
    executor_cli::cache::ensure_mcp_guard(
        home,
        &receipt,
        &run_id,
        &thread,
        &turn,
        lease,
        move |loss| {
            let current = verified_receipt(&cache_home, &guard_receipt, &guard_lead)?;
            if current.executor_thread != guard_thread
                || current.value["turnId"].as_str() != Some(guard_turn.as_str())
                || current.value["lease"] != guard_lease
            {
                return Err(invalid(
                    "the cache guard no longer names the exact current run, thread, turn and lease; no process was targeted",
                ));
            }
            let endpoint = current.endpoint.clone().ok_or_else(|| {
                invalid(format!(
                    "the exact run's registered app-server endpoint is unavailable ({}); no process was targeted",
                    current
                        .endpoint_unavailable
                        .as_deref()
                        .unwrap_or("not recorded")
                ))
            })?;
            let result = executor_cli::mcp_stop(
                &current.receipt,
                &endpoint,
                &current.executor_thread,
                DEFAULT_BOUND,
            )?;
            executor_cli::cache::record_stop_result(&current.receipt, loss, &result)
        },
    )
}

fn attach_cache_guard(home: &Path, previous: &VerifiedRun, response: &mut Value) {
    match refresh_run(home, previous).and_then(|current| {
        ensure_cache_guard(home, &current)?;
        Ok(current)
    }) {
        Ok(current) => response["cacheGuard"] = current.value["cacheGuard"].clone(),
        Err(error) => response["cacheGuardError"] = Value::String(error.to_string()),
    }
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
    let caller_role = caller_authority(&home, &caller)?;
    let truncated = receipts.len() > MAX_STATUS_RUNS;
    let mut runs = Vec::new();
    for receipt in receipts.into_iter().take(MAX_STATUS_RUNS) {
        let run = verify_run_for(&home, receipt, &caller, RunView::Status)?;
        let released = !run.value["release"].is_null();
        let cache_guard_error = (!released)
            .then(|| ensure_cache_guard(&home, &run))
            .and_then(Result::err)
            .map(|error| error.to_string());
        let (pending, pending_truncated) = pending_requests(&run);
        let mut summary = run_summary(&run);
        summary["pendingRequests"] = Value::Array(pending);
        summary["pendingRequestsTruncated"] = Value::Bool(pending_truncated);
        if let Some(error) = cache_guard_error {
            summary["cacheGuardError"] = Value::String(error);
        }
        runs.push((run, summary));
    }
    let mut natives = vec![vec![Value::Null, Value::Null]; runs.len()];
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for (index, (run, _)) in runs.iter().enumerate() {
            let caller_thread = match run.side {
                RunSide::Parent => &run.lead_thread,
                RunSide::Child => &run.executor_thread,
            };
            let caller_id = caller_thread.clone();
            handles.push((
                index,
                0,
                caller_id,
                scope.spawn(move || {
                    native_status_or_release(
                        run,
                        caller_thread,
                        run.side == RunSide::Child,
                        STATUS_NATIVE_READ_BOUND,
                    )
                }),
            ));
            if run.side == RunSide::Parent {
                let executor_id = run.executor_thread.clone();
                handles.push((
                    index,
                    1,
                    executor_id,
                    scope.spawn(move || {
                        native_status_or_release(
                            run,
                            &run.executor_thread,
                            true,
                            STATUS_NATIVE_READ_BOUND,
                        )
                    }),
                ));
            }
        }
        for (index, slot, thread_id, handle) in handles {
            let value = handle.join().unwrap_or_else(|_| {
                json!({
                    "threadId": thread_id,
                    "status": "unavailable",
                    "error": "native status worker panicked",
                    "detail": "native status is unavailable; the run receipt and lease remain authoritative"
                })
            });
            natives[index][slot] = value;
        }
    });
    let mut output = Vec::with_capacity(runs.len());
    for ((run, mut summary), native) in runs.into_iter().zip(natives) {
        summary["native"] = native[0].clone();
        summary["executorNative"] = if run.side == RunSide::Parent {
            native[1].clone()
        } else {
            Value::Null
        };
        output.push(summary);
    }
    Ok(json!({
        "callerThreadId": caller,
        "role": match caller_role.role {
            CallerRole::Director | CallerRole::DelegatedLead => "lead",
            CallerRole::Executor => "executor",
        },
        "callerRole": match caller_role.role {
            CallerRole::Director => "director",
            CallerRole::DelegatedLead => "lead",
            CallerRole::Executor => "executor",
        },
        "runs": output,
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
    result["cacheGuard"] = run.value["cacheGuard"].clone();
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

fn release(operation: &Operation) -> io::Result<Value> {
    let caller = caller_thread(&operation.metadata)?;
    let ReleaseArgs {
        run: selected,
        disposition,
        reason,
        base,
    } = serde_json::from_value(operation.arguments.clone())
        .map_err(|error| invalid(format!("executor_release arguments are invalid: {error}")))?;
    let disposition = match disposition.as_str() {
        "merged" => SlotDisposition::Merged,
        "discarded" => SlotDisposition::Discarded,
        other => {
            return Err(invalid(format!(
                "executor_release disposition must be 'merged' or 'discarded', not {other:?}; nothing was released"
            )));
        }
    };
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(invalid(
            "executor_release requires a nonempty reason; nothing was released",
        ));
    }
    let home = native_launcher::codex_home()?;
    let receipt = release_receipt(&home, &caller, selected.as_deref())?;
    let run = verify_run_for(&home, receipt, &caller, RunView::Release)?;
    let previous = run.value["release"]
        .as_object()
        .map(|_| run.value["release"].clone());
    if let Some(previous) = previous.as_ref() {
        if previous["run"].as_str() != Some(run.run.as_str())
            || previous["slot"].as_u64() != Some(u64::from(run.slot))
        {
            return Err(invalid(
                "the run receipt carries a release record for another run or slot; refusing an ambiguous release",
            ));
        }
        let recorded = previous["disposition"].as_str().ok_or_else(|| {
            invalid("the run receipt's release record names no disposition; nothing was released")
        })?;
        let slot_released = previous["slotFreed"].as_bool() == Some(true)
            || task_worktree::load_slot_record(&home, &run.repository, run.slot)?
                .is_some_and(|record| record.state == SlotState::Released);
        if slot_released {
            if recorded != executor_cli::disposition_name(disposition) {
                return Err(invalid(format!(
                    "run {} was already released as {recorded}; a repeated release cannot change the disposition and nothing was reset",
                    run.run
                )));
            }
            return Ok(release_response(&run, previous, true));
        }
    }
    require_release_ready(&run)?;
    let requested_base = match base {
        Some(base) if base.trim().is_empty() => {
            return Err(invalid(
                "executor_release base must not be empty when supplied; nothing was released",
            ));
        }
        Some(base) => Some(base.trim().to_owned()),
        None => None,
    };
    let base = match requested_base.as_deref() {
        Some(base) => base.to_owned(),
        None => executor_cli::committed_head(&run.repository)?,
    };

    // Re-read the exact receipt and run state after the native check. A slot
    // or thread replaced during release preparation must not be reset by this
    // caller, and a newer pending request must still refuse the release.
    let current = verified_receipt_for(&home, &run.receipt, &caller, RunView::Release)?;
    if current.run != run.run
        || current.executor_thread != run.executor_thread
        || current.slot != run.slot
    {
        return Err(invalid(
            "the exact run identity changed during release preparation; nothing was reset",
        ));
    }
    require_release_ready(&current)?;
    let pool_size = release_pool_size(&current)?;
    let pool = task_worktree::pool(&current.repository, pool_size)?;
    let slot = pool.slot(current.slot)?;
    if !same_path(&slot.path, &current.checkout) {
        return Err(invalid(format!(
            "slot {} resolves to {} instead of the recorded checkout {}; nothing was reset",
            current.slot,
            slot.path.display(),
            current.checkout.display()
        )));
    }
    let record = task_worktree::load_slot_record(&home, &current.repository, current.slot)?
        .ok_or_else(|| {
            invalid(format!(
                "slot {} has no recorded ownership; nothing was released",
                current.slot
            ))
        })?;
    if record.startup.is_some() {
        return Err(invalid(format!(
            "slot {} still carries an in-flight startup reservation; the slot was not reset",
            current.slot
        )));
    }
    if executor_cli::owner_live(&home, &current.repository, &current.run) {
        return Err(invalid(format!(
            "slot {} is still held by a live executor-session lease for run {}; stop it before release",
            current.slot, current.run
        )));
    }

    // A merged claim cannot justify deleting uncommitted or untracked work.
    // Preserve it with an explicit outcome; discarded is the caller's explicit
    // authorization for the existing pool owner to reset that work.
    if disposition == SlotDisposition::Merged && checkout_dirty(&current.checkout)? {
        let repeats = release_repeats(previous.as_ref());
        let release = release_record(
            &current,
            ReleaseFacts {
                disposition,
                reason,
                requested_base: requested_base.as_deref(),
                base: &base,
                outcome: "preserved",
                slot_state: "occupied",
                limitation: Some(
                    "the checkout holds local or untracked changes; merged does not authorize deleting them. Commit or merge the intended work, or repeat with disposition=discarded to authorize deletion",
                ),
                message_cleanup: None,
                repeats,
            },
        );
        record_release_result(&current, &caller, release.clone())?;
        return Ok(release_response(&current, &release, false));
    }

    // The slot is still held here, so no later occupant can publish a new
    // canonical receipt while this run's own payload paths are removed.
    let message_cleanup = executor_input::cleanup_recorded_messages(&home, &current.receipt);
    let is_live = |owner: &str| executor_cli::owner_live(&home, &current.repository, owner);
    let released = match record.task.as_deref() {
        Some(task) => task_worktree::release_task(
            &home,
            &current.repository,
            task,
            disposition,
            reason,
            &base,
            record.handle.as_deref(),
            &is_live,
        )?,
        None => task_worktree::release_slot(
            &home,
            &pool,
            current.slot,
            disposition,
            reason,
            &base,
            &is_live,
        )?,
    };
    let (outcome, slot_state, base, limitation) = match released {
        LaneDisposition::Reused { base } => ("reused", "released", base, None),
        LaneDisposition::Preserved { limitation } => {
            ("preserved", "awaitingReview", base, Some(limitation))
        }
    };
    let release = release_record(
        &current,
        ReleaseFacts {
            disposition,
            reason,
            requested_base: requested_base.as_deref(),
            base: &base,
            outcome,
            slot_state,
            limitation: limitation.as_deref(),
            message_cleanup: Some(&message_cleanup),
            repeats: release_repeats(previous.as_ref()),
        },
    );
    record_release_result(&current, &caller, release.clone())?;
    Ok(release_response(&current, &release, false))
}

fn release_receipt(home: &Path, caller: &str, selected: Option<&str>) -> io::Result<RunReceipt> {
    if let Some(run) = selected
        && !valid_run_id(run)
    {
        return Err(invalid(format!(
            "run {run:?} is malformed; refusing before any run lookup or release"
        )));
    }
    let matches: Vec<RunReceipt> = read_receipts(home)?
        .into_iter()
        .filter(|receipt| {
            receipt.value["leadThreadId"].as_str() == Some(caller)
                && selected.is_none_or(|run| receipt.value["run"].as_str() == Some(run))
        })
        .collect();
    match matches.as_slice() {
        [] => Err(invalid(match selected {
            Some(run) => format!(
                "no recorded MCP executor run {run:?} is owned by the calling lead; nothing was released"
            ),
            None => {
                "no recorded MCP executor run is owned by the calling lead; nothing was released"
                    .into()
            }
        })),
        [receipt] => Ok(RunReceipt {
            path: receipt.path.clone(),
            value: receipt.value.clone(),
        }),
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
                "the calling lead owns {} recorded MCP executor runs; the release target is ambiguous and nothing was reset. Choose one with executor_release run=<run>. runs: {}",
                matches.len(),
                Value::Array(runs)
            )))
        }
    }
}

fn require_release_ready(run: &VerifiedRun) -> io::Result<()> {
    let (pending, _) = pending_requests(run);
    if !pending.is_empty() {
        let ids: Vec<&Value> = pending
            .iter()
            .filter_map(|request| request.get("requestId"))
            .take(MAX_STATUS_REQUESTS)
            .collect();
        return Err(invalid(format!(
            "run {} has unresolved request(s) {}; answer them before release and nothing was reset",
            run.run,
            Value::Array(ids.into_iter().cloned().collect())
        )));
    }
    if run.value["stop"]["outcome"].as_str() == Some("partial") {
        return Err(invalid(format!(
            "run {} last reported partial stop containment; its owned work is not proved ended, so the slot was not released",
            run.run
        )));
    }
    if run.endpoint.is_none() {
        return Err(invalid(format!(
            "run {} has no verified live lead endpoint ({}); its native thread state cannot be proved idle and the slot was not released",
            run.run,
            run.endpoint_unavailable
                .as_deref()
                .unwrap_or("not recorded")
        )));
    }
    let native = native_thread(run, &run.executor_thread, true).map_err(|error| {
        invalid(format!(
            "the exact executor thread {} state cannot be read ({error}); native archive alone is not completion and slot {} remains occupied",
            run.executor_thread, run.slot
        ))
    })?;
    if let Some(turn) = native["activeTurnId"].as_str() {
        return Err(invalid(format!(
            "run {} still has active turn {turn} in executor thread {}; stop it before release and nothing was reset",
            run.run, run.executor_thread
        )));
    }
    let status = native["status"].as_str().unwrap_or("unknown");
    if status != "idle" {
        return Err(invalid(format!(
            "executor thread {} reports {status}, not an idle run whose active work has ended; release is refused and nothing was reset. Native archive or presentation alone does not free a slot",
            run.executor_thread
        )));
    }
    Ok(())
}

fn release_pool_size(run: &VerifiedRun) -> io::Result<u32> {
    run.value["poolSize"]
        .as_u64()
        .and_then(|size| u32::try_from(size).ok())
        .filter(|size| *size > 0)
        .ok_or_else(|| {
            invalid(
                "the MCP run receipt records no usable pool size; the exact slot layout cannot be resolved and nothing was reset",
            )
        })
}

fn checkout_dirty(checkout: &Path) -> io::Result<bool> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(checkout)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .map_err(|error| {
            invalid(format!(
                "the checkout {} could not be inspected before release: {error}",
                checkout.display()
            ))
        })?;
    if !output.status.success() {
        return Err(invalid(format!(
            "the checkout {} could not be inspected before release: {}",
            checkout.display(),
            excerpt(&String::from_utf8_lossy(&output.stderr), 300)
        )));
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn release_repeats(previous: Option<&Value>) -> u64 {
    previous
        .and_then(|release| release["repeats"].as_u64())
        .map(|repeats| repeats.saturating_add(1))
        .unwrap_or(0)
}

struct ReleaseFacts<'a> {
    disposition: SlotDisposition,
    reason: &'a str,
    requested_base: Option<&'a str>,
    base: &'a str,
    outcome: &'a str,
    slot_state: &'a str,
    limitation: Option<&'a str>,
    message_cleanup: Option<&'a io::Result<usize>>,
    repeats: u64,
}

fn release_record(run: &VerifiedRun, facts: ReleaseFacts<'_>) -> Value {
    let (removed, cleanup_error) = match facts.message_cleanup {
        Some(Ok(count)) => (json!(count), Value::Null),
        Some(Err(error)) => (Value::Null, json!(error.to_string())),
        None => (Value::Null, Value::Null),
    };
    let slot_freed = facts.outcome == "reused";
    json!({
        "schema": 1,
        "run": run.run,
        "leadThreadId": run.lead_thread,
        "executorThreadId": run.executor_thread,
        "slot": run.slot,
        "checkout": run.checkout,
        "disposition": executor_cli::disposition_name(facts.disposition),
        "reason": facts.reason,
        "requestedBase": facts.requested_base,
        "base": facts.base,
        "outcome": facts.outcome,
        "slotFreed": slot_freed,
        "slotState": facts.slot_state,
        "preserved": !slot_freed,
        "limitation": facts.limitation,
        "messagePayloadsRemoved": removed,
        "messageCleanupError": cleanup_error,
        "repeats": facts.repeats,
        "recordedMs": executor_message_now()
    })
}

fn record_release_result(run: &VerifiedRun, caller: &str, release: Value) -> io::Result<()> {
    executor_message::update_receipt_document(&run.receipt, |document| {
        if document["run"].as_str() != Some(run.run.as_str())
            || document["leadThreadId"].as_str() != Some(caller)
            || document["executorThreadId"].as_str() != Some(run.executor_thread.as_str())
            || document["slot"].as_u64() != Some(u64::from(run.slot))
        {
            return Err(invalid(
                "the MCP run receipt changed before its release could be recorded; the release record was not written",
            ));
        }
        document["release"] = release;
        Ok(())
    })
}

fn release_response(run: &VerifiedRun, release: &Value, idempotent: bool) -> Value {
    let slot_freed = release["slotFreed"].as_bool().unwrap_or(false);
    json!({
        "run": run.run,
        "leadThreadId": run.lead_thread,
        "executorThreadId": run.executor_thread,
        "slot": run.slot,
        "checkout": run.checkout,
        "receipt": run.receipt,
        "state": if slot_freed { "released" } else { "preserved" },
        "outcome": release["outcome"],
        "released": slot_freed,
        "slotFreed": slot_freed,
        "idempotent": idempotent,
        "disposition": release["disposition"],
        "reason": release["reason"],
        "base": release["base"],
        "requestedBase": release["requestedBase"],
        "preserved": release["preserved"],
        "slotState": release["slotState"],
        "limitation": release["limitation"],
        "messagePayloadsRemoved": release["messagePayloadsRemoved"],
        "messageCleanupError": release["messageCleanupError"],
        "release": release
    })
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
            let mut response = json!({
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
            });
            attach_cache_guard(&home, &run, &mut response);
            Ok(response)
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
            let mut response = json!({
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
            });
            attach_cache_guard(&home, &run, &mut response);
            Ok(response)
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
        return send_to_lead(&run, &args.text, KIND_NOTICE, args.client_message_id);
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
    let mut result = send_to_executor(&run, &args.text, args.client_message_id, None)?;
    attach_cache_guard(&home, &run, &mut result);
    Ok(result)
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
    let mut result = send_to_executor(&run, &args.text, None, Some(&args.request_id))?;
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
    attach_cache_guard(&home, &run, &mut result);
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
    send_to_lead(&run, &args.text, KIND_REQUEST, args.client_message_id)
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
    Ok(ComposedDelivery {
        text: format!("{header}{PAYLOAD_MARK}{text}"),
        spill: None,
    })
}

fn reply_envelope(
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
    Ok(ComposedDelivery {
        text: format!("{header}{PAYLOAD_MARK}{text}"),
        spill: None,
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
            let composed = lead_envelope(run, &id, kind, text)?;
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
    let composed = lead_envelope(run, &id, kind, text)?;
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
        Some(request_id) => reply_envelope(run, &id, request_id, text)?,
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
