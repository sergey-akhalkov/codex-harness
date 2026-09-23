//! Control-backed executor conversations: one harness-owned `codex app-server`
//! child per pooled exec run instead of the inbound-incapable `codex exec`.
//!
//! [`Conversation::start`] performs the whole preparation of one run before the
//! first model request: it reserves a loopback port, writes a capability token
//! into the run's kit-local state, spawns `codex app-server --listen
//! ws://127.0.0.1:PORT --ws-auth capability-token --ws-token-file PATH` inside
//! the caller's Windows Job with the executor session environment, waits for the
//! endpoint, initializes the connection with the experimental API, starts the
//! conversation thread with `cwd` at the bound slot and the resolved profile
//! binding pinned through `model`/`modelProvider`/`config`, verifies that the
//! started thread reports that binding, names the thread with the assignment
//! title, and records the endpoint (port, token, thread id, plus the
//! app-server child's exact process identity) in `endpoint-N.json` beside the
//! dispatch receipt. That file is the address `executor message` and
//! `executor stop` read, so the recorded process identity is the one the stop
//! path can interrupt and end without trusting a bare pid.
//!
//! The tab host then submits the assignment with [`Conversation::assign`],
//! renders what [`Conversation::pump`] returns (each record carries the
//! [`Lifecycle`] state it establishes and one readable line from
//! [`ControlEvent::render`], including the lead inputs addressed to the run,
//! which are rendered once as `input:` lines), and records the outcome from
//! [`Conversation::final_message`]. `executor message` and `executor stop`
//! address the same conversation through [`Endpoint::read`] plus
//! [`Conversation::attach`], and use [`Conversation::request`] for one bounded
//! native request (`turn/start`, `turn/steer`, `turn/interrupt`) without
//! inventing state; its [`Reply`] distinguishes the server's own refusal from
//! a request it never answered, which delivery classification depends on.
//!
//! Fail-closed rules:
//!
//! - A child that exits, or that never serves its endpoint within the bound,
//!   fails startup with the log locator and the child identity, never a
//!   fabricated readiness.
//! - A started thread that does not report the bound model, provider and
//!   reasoning effort fails the dispatch instead of silently running with
//!   different routing.
//! - A response carrying another request identity, no `result`, an `error`, or
//!   a `thread/read` answer for another thread fails the call.
//! - Notifications that cannot establish a state (an unknown turn status, a
//!   missing item type) record a deviation in [`Conversation::defect`] instead
//!   of inventing a state. An unknown notification method stays unclassified,
//!   because a newer server may add methods this driver does not need.
//!
//! Bounds: one control record is at most 1 MiB (the transport's own limit),
//! [`Conversation::pump`] returns at most [`MAX_EVENTS_PER_PUMP`] records per
//! call, and every request waits at most the session bound.
//!
//! Verified baseline: every native shape used here (initialize plus
//! initialized, `thread/start` with `cwd`/`model`/`modelProvider`/
//! `allowProviderModelFallback`/`approvalPolicy`/`sandbox`/`config`,
//! `thread/name/set`, `turn/start` input parts, `thread/read includeTurns`,
//! `thread/started`, `thread/status/changed`, `item/*`, `turn/completed`
//! statuses, `error`) is one the change's contract probes observed on the
//! installed codex-cli 0.156.1. `thread/read includeTurns` currently answers
//! with a deprecation notice pointing at paginated `thread/turns/list` plus
//! `thread/items/list` reads; those paginated reads are the later migration for
//! this driver, not a silent change of the final-message contract.

use harness_core::{
    orchestration_config::{EXECUTOR_AGENT_TOOLS_OFF, EXECUTOR_SESSION_ENV, ProfileBinding},
    process::{CommandSpec, Job, OwnedProcess, ProcessIdentity},
    task_control::ControlConnection,
    task_worktree,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::OsString,
    fmt, fs,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{FARPROC, HMODULE},
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW},
};

/// One control record and one machine-readable state file never exceed this.
const MAX_RECORD: usize = 1024 * 1024;
/// One pump call never returns more records than this; the rest stay queued.
pub const MAX_EVENTS_PER_PUMP: usize = 256;
/// Capability tokens are 32 random bytes as lowercase hex, the shape the
/// control transport requires of its bearer.
const TOKEN_BYTES: usize = 32;
/// Record schema of the endpoint file written beside the dispatch receipt.
const SCHEMA: u32 = 1;
/// Default bound for readiness, requests and response shaping.
pub const DEFAULT_BOUND: Duration = Duration::from_secs(20);
/// Default record-read timeout of [`Conversation::pump`].
pub const DEFAULT_POLL: Duration = Duration::from_millis(200);
/// Bounded wait used while draining records that are already arriving, so one
/// pump returns a burst instead of one record per poll interval.
const DRAIN: Duration = Duration::from_millis(25);
/// Bounded pause between readiness attempts.
const RETRY: Duration = Duration::from_millis(50);

fn invalid(reason: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason.into())
}

/// Bounded single-line excerpt for error text and rendering.
fn excerpt(text: &str, limit: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for character in text.chars() {
        if count >= limit {
            out.push_str("...");
            break;
        }
        match character {
            '\r' => continue,
            '\n' | '\t' => out.push(' '),
            other => out.push(other),
        }
        count += 1;
    }
    out
}

/// Replaces a state file through a staged write, so a reader, the lead reading
/// the endpoint record while the run is live, never observes a partial record,
/// and a failed staging leaves the previous bytes in place.
fn write_staged(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| invalid("control state path has no directory"))?;
    fs::create_dir_all(directory)?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid("control state path has no file name"))?;
    let mut staged = OsString::from(name);
    staged.push(format!(".new-{}", std::process::id()));
    let staged = directory.join(staged);
    fs::write(&staged, bytes)?;
    match fs::rename(&staged, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&staged);
            Err(error)
        }
    }
}

fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid(format!(
            "control state file {} exceeds its bound",
            path.display()
        )));
    }
    Ok(bytes)
}

/// Kit-local control state of one pooled run: the endpoint record beside the
/// dispatch receipt, plus the capability-token file and the app-server log the
/// run owns. All three live in the pool state directory the receipt uses, so
/// nothing lands in the executor's checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlPaths {
    /// `endpoint-N.json` beside `spawn-N.json`.
    pub endpoint: PathBuf,
    /// The capability token the child reads through `--ws-token-file`.
    pub token: PathBuf,
    /// The child's combined stdout/stderr, named when anything fails.
    pub log: PathBuf,
}

impl ControlPaths {
    /// The control state of one pool slot, beside its dispatch receipt.
    ///
    /// The endpoint record keeps the name the stop path reads
    /// (`endpoint-<index>.json`, task 2.1's convention): a record under any
    /// other name would leave a live run unaddressable for `executor stop`
    /// and `executor message`.
    pub fn for_slot(codex_home: &Path, source: &Path, index: u32) -> io::Result<Self> {
        let directory = task_worktree::pool_state_dir(codex_home, source)?;
        Ok(Self {
            endpoint: directory.join(format!("endpoint-{index}.json")),
            token: directory.join(format!("endpoint-{index}.token")),
            log: directory.join(format!("endpoint-{index}.log")),
        })
    }
}

/// The identity the harness bound for one executor run: the resolved profile
/// binding of the dispatch, against which the started thread is verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundIdentity {
    pub profile: String,
    pub model: Option<String>,
    pub model_provider: Option<String>,
    pub reasoning_effort: Option<String>,
}

impl BoundIdentity {
    /// The binding of one resolved profile, as the dispatch receipt records it.
    pub fn resolve(binding: &ProfileBinding) -> Self {
        Self {
            profile: binding.profile.clone(),
            model: binding.model.clone(),
            model_provider: binding.model_provider.clone(),
            reasoning_effort: binding.reasoning_effort.clone(),
        }
    }

    /// Verifies a `thread/start` result against every field the profile bound.
    ///
    /// Fields the profile leaves open are not compared: an unbound model or
    /// effort is the native default, not a mismatch. Comparison is
    /// case-insensitive because the CLI reports model, provider and effort
    /// identifiers in normalized case.
    pub fn verify(&self, started: &Value) -> Result<(), String> {
        if let Some(model) = &self.model {
            compare_field(&started["model"], model, "model")?;
        }
        if let Some(provider) = &self.model_provider {
            compare_field(&started["modelProvider"], provider, "modelProvider")?;
        }
        if let Some(effort) = &self.reasoning_effort {
            compare_field(&started["reasoningEffort"], effort, "reasoningEffort")?;
        }
        Ok(())
    }
}

fn compare_field(reported: &Value, expected: &str, field: &str) -> Result<(), String> {
    match reported.as_str() {
        Some(value) if value.eq_ignore_ascii_case(expected) => Ok(()),
        Some(value) => Err(format!(
            "the started thread reports {field}={value} instead of the bound {expected}"
        )),
        None => Err(format!(
            "the started thread reports no {field} to compare with the bound {expected}"
        )),
    }
}

/// One planned control-backed executor conversation: the process identity, the
/// conversation identity and the state files of a single pooled run.
#[derive(Debug, Clone)]
pub struct ControlPlan {
    /// The installed Codex launcher the app-server child runs from.
    pub launcher: PathBuf,
    /// The kit `CODEX_HOME` the child loads its configuration from.
    pub home: PathBuf,
    /// The bound pool slot: the child's working directory and the thread `cwd`.
    pub slot: PathBuf,
    /// The assignment title the thread is named with.
    pub title: String,
    /// The resolved profile binding the thread must report.
    pub identity: BoundIdentity,
    /// Kit-local control state of this run.
    pub paths: ControlPaths,
    /// The native approval policy the bound profile resolves to, when it
    /// resolves one; passed through as a `thread/start` override.
    pub approval_policy: Option<String>,
    /// The native sandbox mode the bound profile resolves to, when the executor
    /// shell preflight resolved one; passed through as a `thread/start`
    /// override, because the app-server has no `--profile` flag and would
    /// otherwise run the thread under the default policy.
    pub sandbox: Option<String>,
    /// Additional `thread/start` config overrides the resolved profile needs
    /// beyond the binding, for example a provider route the profile carries.
    pub overrides: BTreeMap<String, Value>,
    /// Additional `codex app-server` arguments, appended after the subcommand.
    pub args: Vec<OsString>,
    /// The loopback port the child must serve. The default reserves a free
    /// port; a caller that owns the endpoint - an acceptance check that
    /// provides the server, or an operator who pins the port - names it here.
    /// The child must still serve it before any conversation starts.
    pub port: Option<u16>,
    /// Environment overrides for the child; `None` removes a variable. The
    /// caller passes the prepared executor shell `PATH` here.
    pub env: BTreeMap<OsString, Option<OsString>>,
    /// Bound for readiness and for every request's answer.
    pub bound: Duration,
    /// Record-read timeout of one pump step.
    pub poll: Duration,
}

impl ControlPlan {
    /// A plan with the default bounds and no overrides.
    pub fn new(
        launcher: impl Into<PathBuf>,
        home: impl Into<PathBuf>,
        slot: impl Into<PathBuf>,
        title: impl Into<String>,
        identity: BoundIdentity,
        paths: ControlPaths,
    ) -> Self {
        Self {
            launcher: launcher.into(),
            home: home.into(),
            slot: slot.into(),
            title: title.into(),
            identity,
            paths,
            approval_policy: None,
            sandbox: None,
            overrides: BTreeMap::new(),
            args: Vec::new(),
            port: None,
            env: BTreeMap::new(),
            bound: DEFAULT_BOUND,
            poll: DEFAULT_POLL,
        }
    }
}

/// The `codex app-server` command one control session spawns: the loopback
/// listener, the capability-token file, the executor agent-tool override, the
/// caller's extra arguments, the bound slot as working directory and the
/// executor session environment.
pub fn app_server_spec(plan: &ControlPlan, port: u16) -> CommandSpec {
    let mut command = CommandSpec::new(&plan.launcher);
    command.current_dir = Some(plan.slot.clone());
    command.args = app_server_args(plan, port);
    command.env.insert(
        "CODEX_HOME".into(),
        Some(plan.home.clone().into_os_string()),
    );
    command
        .env
        .insert(EXECUTOR_SESSION_ENV.into(), Some("1".into()));
    for (key, value) in &plan.env {
        command.env.insert(key.clone(), value.clone());
    }
    command
}

fn app_server_args(plan: &ControlPlan, port: u16) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("app-server"),
        OsString::from("--listen"),
        OsString::from(format!("ws://127.0.0.1:{port}")),
        OsString::from("--ws-auth"),
        OsString::from("capability-token"),
        OsString::from("--ws-token-file"),
        plan.paths.token.clone().into_os_string(),
    ];
    args.extend(EXECUTOR_AGENT_TOOLS_OFF.map(OsString::from));
    args.extend(plan.args.iter().cloned());
    args
}

/// The control endpoint of one run, recorded beside the dispatch receipt: the
/// exact address and bearer later `executor message` and `executor stop` calls
/// address, and the thread that conversation runs on.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Endpoint {
    schema: u32,
    port: u16,
    token: String,
    /// Absent until the conversation thread exists; an endpoint without it is
    /// recorded state, not an addressable conversation.
    pub thread_id: Option<String>,
    /// The exact identity of the app-server child serving this endpoint, when
    /// this endpoint was recorded by the host that spawned it. The stop path
    /// verifies and ends that child through this record; a pid alone would be
    /// a guess about a reused process.
    #[serde(default)]
    pub process: Option<EndpointProcess>,
}

/// The app-server child one recorded endpoint belongs to: the process
/// identity (pid, creation time and image) the owning host spawned inside its
/// own Job, in the shape the stop path reads (`process.pid`,
/// `process.creationTime`, `process.program`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EndpointProcess {
    pub pid: u32,
    pub creation_time: u64,
    pub program: PathBuf,
}

impl fmt::Debug for Endpoint {
    /// The bearer never reaches logs, receipts or panics.
    fn fmt(&self, format: &mut fmt::Formatter<'_>) -> fmt::Result {
        format
            .debug_struct("Endpoint")
            .field("port", &self.port)
            .field("thread_id", &self.thread_id)
            .field("process", &self.process)
            .finish_non_exhaustive()
    }
}

impl Endpoint {
    fn new(port: u16, token: String, process: EndpointProcess) -> Self {
        Self {
            schema: SCHEMA,
            port,
            token,
            thread_id: None,
            process: Some(process),
        }
    }

    /// The bearer of this endpoint. Only the transport needs it.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The loopback port of this endpoint.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Writes the record where the lead reads it: beside the dispatch receipt,
    /// replacing any previous record of the same slot in one step.
    pub fn record(&self, path: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self)?;
        if bytes.len() > MAX_RECORD {
            return Err(invalid("control endpoint record exceeds its bound"));
        }
        write_staged(path, &bytes)
    }

    /// Reads and validates a recorded endpoint. A malformed record fails
    /// closed: an unusable address must never be treated as a live one.
    pub fn read(path: &Path) -> io::Result<Self> {
        let bytes = read_bounded(path, MAX_RECORD as u64)?;
        let endpoint: Self = serde_json::from_slice(&bytes).map_err(|error| {
            invalid(format!(
                "control endpoint record {} is not readable: {error}",
                path.display()
            ))
        })?;
        if endpoint.schema != SCHEMA
            || endpoint.port == 0
            || !endpoint
                .token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
            || endpoint.token.len() < TOKEN_BYTES * 2
            || endpoint
                .thread_id
                .as_deref()
                .is_some_and(|id| id.is_empty())
            || endpoint.process.as_ref().is_some_and(|process| {
                process.pid == 0
                    || process.creation_time == 0
                    || process.program.as_os_str().is_empty()
            })
        {
            return Err(invalid(format!(
                "control endpoint record {} is malformed or unsupported",
                path.display()
            )));
        }
        Ok(endpoint)
    }
}

/// The lifecycle vocabulary the dispatch receipt records for a control-backed
/// run. The names are the ones `executor_observation` uses, so the host keeps
/// one receipt vocabulary across both exec backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lifecycle {
    /// The native thread identity was observed.
    NativeStart,
    /// Work is running on the thread.
    Running,
    /// The turn completed with native status `completed`.
    Completed,
    /// The turn completed with native status `failed`, or the thread reported
    /// a system error.
    Failed,
    /// A protocol deviation makes the state unknown; nothing may report this
    /// run as a clean completion.
    Defect,
    /// The turn completed with native status `interrupted`.
    Interrupted,
}

impl Lifecycle {
    /// The receipt state name recorded for this state.
    pub fn receipt_state(self) -> &'static str {
        match self {
            Self::NativeStart => "native-start",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Defect => "defect",
            Self::Interrupted => "interrupted",
        }
    }

    /// A terminal state is established by the thread's own turn status; later
    /// item activity (an interrupted tool call finishing) never reopens it.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Interrupted)
    }
}

/// One accepted turn of the bound thread, as `turn/start` answered it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnStart {
    pub turn_id: String,
    pub status: String,
}

/// The answer to one bounded native request, exactly as the protocol delivered
/// it.
///
/// A caller that must classify delivery reads this instead of
/// [`Conversation::call`]: a [`Reply::Rejected`] is the server's own refusal
/// (nothing was applied), while [`Reply::Unanswered`] or a transport error says
/// nothing about whether the input reached the conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// The server answered with a `result`.
    Result(Value),
    /// The server answered with an `error`.
    Rejected(Value),
    /// No answer arrived within the request bound.
    Unanswered,
}

/// The final assistant message of a thread, or why it is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalMessage {
    /// The last turn's last assistant message carries text.
    Present(String),
    /// The last assistant message is empty: an output defect, never evidence
    /// about the model, authentication or quota.
    Empty,
    /// The thread has no assistant message at all.
    Missing,
}

/// One control record as observed, with the lifecycle meaning it establishes.
///
/// The raw record is retained for the receipt and the bounded detail file, so a
/// rendering decision or a later reconciliation never has to re-read the
/// conversation.
#[derive(Debug, Clone)]
pub struct ControlEvent {
    /// The raw control record.
    pub raw: Value,
    /// The notification method, when this record was a notification.
    pub method: Option<String>,
    /// The lifecycle state this record establishes, when it establishes one.
    pub lifecycle: Option<Lifecycle>,
    /// The protocol deviation recorded while mapping this record, when the
    /// record cannot establish a state.
    pub deviation: Option<String>,
    /// True when this record repeats a lead input the surface already
    /// rendered; [`Self::render`] skips it so one delivered message shows once
    /// even when the server announces it both as started and as completed.
    pub repeat: bool,
}

impl ControlEvent {
    /// Writes one readable line for the session's own visible surface.
    ///
    /// Reasoning summaries are intentionally not rendered: the surface carries
    /// decisions, activity and lifecycle, not opaque model state.
    pub fn render(&self, out: &mut dyn Write) -> io::Result<()> {
        if self.repeat {
            return Ok(());
        }
        let Some(line) = self.line() else {
            return Ok(());
        };
        writeln!(out, "{line}")
    }

    fn line(&self) -> Option<String> {
        let params = &self.raw["params"];
        match self.method.as_deref() {
            Some("thread/started") => Some(format!(
                "state: native start (session {})",
                params["thread"]["id"].as_str().unwrap_or("unidentified")
            )),
            Some("thread/status/changed") => Some(format!(
                "state: thread {}",
                params["status"]["type"].as_str().unwrap_or("unreported")
            )),
            Some("turn/started") => Some("state: turn started".to_owned()),
            Some("turn/completed") => match params["turn"]["status"].as_str() {
                Some("failed") => Some(format!(
                    "turn failed: {}",
                    excerpt(
                        params["turn"]["error"]["message"]
                            .as_str()
                            .unwrap_or("no native message"),
                        400
                    )
                )),
                Some(status) => Some(format!("state: turn completed ({status})")),
                None => Some("state: turn completed (unreported)".to_owned()),
            },
            Some("error") => Some(format!(
                "error: {}{}",
                excerpt(
                    params["error"]["message"]
                        .as_str()
                        .unwrap_or("native error without a message"),
                    400
                ),
                if params["willRetry"] == true {
                    " (retrying)"
                } else {
                    ""
                }
            )),
            Some(method @ ("item/started" | "item/updated" | "item/completed")) => {
                item_line(method, &params["item"])
            }
            Some(method) => Some(format!("event: {method}")),
            None => None,
        }
    }
}

fn item_line(method: &str, item: &Value) -> Option<String> {
    let phase = method.trim_start_matches("item/");
    match item["type"].as_str()? {
        "reasoning" => None,
        "agentMessage" => Some(format!(
            "assistant: {}",
            excerpt(item["text"].as_str().unwrap_or_default(), 4000)
        )),
        // The addressed `executor message` path delivers lead input into
        // this conversation; the run's own surface shows it, so the lead
        // can see that the correction arrived.
        "userMessage" => Some(format!(
            "input: {}",
            excerpt(&user_message_text(item), 4000)
        )),
        "commandExecution" => Some(format!(
            "command: {}{}",
            excerpt(item["command"].as_str().unwrap_or_default(), 400),
            match item["exitCode"].as_i64() {
                Some(code) => format!(" (exit {code})"),
                None => String::new(),
            }
        )),
        "fileChange" => Some(format!(
            "files changed: {}",
            item["changes"].as_array().map_or(0, Vec::len)
        )),
        "mcpToolCall" => Some(format!(
            "tool: {}.{}",
            item["server"].as_str().unwrap_or("server"),
            item["tool"].as_str().unwrap_or("tool")
        )),
        "webSearch" => Some(format!(
            "search: {}",
            excerpt(item["query"].as_str().unwrap_or_default(), 400)
        )),
        other => Some(format!("item {phase}: {other}")),
    }
}

/// The literal text of one `userMessage` item: its `content` parts, or the
/// `text` field some builds carry directly. Parts this driver does not know
/// (images, audio) are not invented as text. The addressed-message path
/// correlates a delivered input through this same reading.
pub(crate) fn user_message_text(item: &Value) -> String {
    if let Some(content) = item["content"].as_array() {
        let text: Vec<&str> = content
            .iter()
            .filter(|part| part["type"] == "text")
            .filter_map(|part| part["text"].as_str())
            .collect();
        if !text.is_empty() {
            return text.join("\n");
        }
    }
    item["text"].as_str().unwrap_or_default().to_owned()
}

/// One control-backed executor conversation: the app-server child, its
/// authenticated connection, the named thread, and the observed state.
///
/// The child belongs to the caller's [`Job`] for its whole life; this type
/// never reaps, restarts or releases anything, and a failure after the child
/// started leaves it to the caller's Job, which is the sole cleanup authority.
pub struct Conversation {
    connection: ControlConnection,
    process: Option<OwnedProcess>,
    endpoint: Endpoint,
    thread_id: String,
    identity: Option<BoundIdentity>,
    lifecycle: Option<Lifecycle>,
    defect: Option<String>,
    failure: Option<String>,
    turn: Option<TurnStart>,
    pending: VecDeque<Value>,
    /// Item ids of lead inputs this surface has already rendered, so one
    /// delivered message is not printed twice when the server announces it as
    /// both started and completed. Bounded: dedupe is best effort above it.
    rendered_inputs: BTreeSet<String>,
    next_id: u64,
    bound: Duration,
    poll: Duration,
}

/// Bound on remembered lead-input item ids; beyond it the surface re-renders
/// rather than growing without limit.
const RENDERED_INPUTS_LIMIT: usize = 256;

impl fmt::Debug for Conversation {
    /// Diagnostics carry the conversation identity and the observed state; the
    /// capability bearer stays out through [`Endpoint`]'s own redaction.
    fn fmt(&self, format: &mut fmt::Formatter<'_>) -> fmt::Result {
        format
            .debug_struct("Conversation")
            .field("thread_id", &self.thread_id)
            .field("endpoint", &self.endpoint)
            .field("identity", &self.identity)
            .field("lifecycle", &self.lifecycle)
            .field("defect", &self.defect)
            .field("failure", &self.failure)
            .field("turn", &self.turn)
            .field("process", &self.process_identity())
            .field("queued", &self.pending.len())
            .finish_non_exhaustive()
    }
}

impl Conversation {
    /// Starts one harness-owned app-server child inside the caller's Job and
    /// prepares the conversation on it.
    ///
    /// Returns only after the endpoint answers, the connection is initialized
    /// with the experimental API, the thread runs at the bound slot with the
    /// resolved profile binding verified, and the endpoint record is written.
    /// On any failure the child, if it started at all, stays inside the
    /// caller's Job, and the error names the log to read.
    pub fn start(job: &Job, plan: &ControlPlan) -> io::Result<Self> {
        if plan.launcher.as_os_str().is_empty() || plan.slot.as_os_str().is_empty() {
            return Err(invalid(
                "a control session requires the launcher, the kit home and the bound slot",
            ));
        }
        let (process, connection, endpoint) = spawn_app_server(job, plan)?;
        let mut conversation = Self {
            connection,
            process: Some(process),
            endpoint,
            thread_id: String::new(),
            identity: Some(plan.identity.clone()),
            lifecycle: None,
            defect: None,
            failure: None,
            turn: None,
            pending: VecDeque::new(),
            rendered_inputs: BTreeSet::new(),
            next_id: 0,
            bound: plan.bound,
            poll: plan.poll,
        };
        conversation.initialize()?;
        conversation.start_thread(plan)?;
        conversation
            .endpoint
            .record(&plan.paths.endpoint)
            .map_err(|error| {
                invalid(format!(
                    "control endpoint record {}: {error}",
                    plan.paths.endpoint.display()
                ))
            })?;
        Ok(conversation)
    }

    /// Attaches to a conversation that already runs, through its recorded
    /// endpoint: the path `executor message` and `executor stop` use.
    ///
    /// Attaching changes nothing: it connects, initializes the connection with
    /// the experimental API and adopts the recorded thread identity. The
    /// caller keeps owning every decision about delivery and lifecycle.
    // The host that started a conversation drives it directly; attaching is
    // for the later command that addresses a live run from outside.
    #[allow(dead_code)]
    pub fn attach(endpoint: &Endpoint, bound: Duration) -> io::Result<Self> {
        let thread_id = endpoint.thread_id.clone().ok_or_else(|| {
            invalid(
                "the recorded control endpoint names no thread; the run never started a conversation to address",
            )
        })?;
        let connection = ControlConnection::connect(endpoint.port, endpoint.token(), bound)?;
        let mut conversation = Self {
            connection,
            process: None,
            endpoint: endpoint.clone(),
            thread_id,
            identity: None,
            lifecycle: None,
            defect: None,
            failure: None,
            turn: None,
            pending: VecDeque::new(),
            rendered_inputs: BTreeSet::new(),
            next_id: 0,
            bound,
            poll: DEFAULT_POLL,
        };
        conversation.initialize()?;
        Ok(conversation)
    }

    /// The endpoint of this conversation, with its thread identity.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// The bound native thread every control request addresses.
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    /// The identity the started thread was verified against, when this session
    /// started its own thread.
    #[allow(dead_code)] // Read back through the receipt by the addressing commands.
    pub fn identity(&self) -> Option<&BoundIdentity> {
        self.identity.as_ref()
    }

    /// The app-server child of this session, when the session started one.
    pub fn process(&self) -> Option<&OwnedProcess> {
        self.process.as_ref()
    }

    /// The identity of the app-server child, for the receipt.
    pub fn process_identity(&self) -> Option<ProcessIdentity> {
        self.process.as_ref().map(OwnedProcess::identity)
    }

    /// The lifecycle state the conversation has reached so far.
    pub fn lifecycle(&self) -> Option<Lifecycle> {
        self.lifecycle
    }

    /// The first protocol deviation observed. A run with a recorded deviation
    /// must never be reported as a clean completion.
    pub fn defect(&self) -> Option<&str> {
        self.defect.as_deref()
    }

    /// The last native failure message observed, if any. This is evidence for
    /// the receipt's cause, not a terminal state by itself.
    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// The turn this session submitted, as the native answer described it.
    #[allow(dead_code)] // Recorded on the surface; the addressing commands read it back.
    pub fn turn(&self) -> Option<&TurnStart> {
        self.turn.as_ref()
    }

    /// Submits one text input to the bound thread through `turn/start`.
    ///
    /// The answer is transport acceptance, not delivery: a `turn/start` that
    /// arrives while another turn is active is answered with the already-active
    /// turn, so only the conversation's own evidence can establish that an
    /// input was applied. Callers that must classify delivery do so from the
    /// rendered items and the recorded turn identity, never from this answer.
    pub fn assign(&mut self, text: &str) -> io::Result<TurnStart> {
        if text.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "an assignment input must not be empty",
            ));
        }
        self.assign_input(json!([{"type":"text","text":text}]))
    }

    /// Submits one already-shaped input array (text and image parts) to the
    /// bound thread through `turn/start`.
    pub fn assign_input(&mut self, input: Value) -> io::Result<TurnStart> {
        let params = json!({"threadId": self.thread_id, "input": input});
        let answer = self.call("turn/start", params)?;
        let turn = TurnStart {
            turn_id: answer["turn"]["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| {
                    invalid(
                        "turn/start answered without a turn identity; the submitted input has no recorded turn",
                    )
                })?
                .to_owned(),
            status: answer["turn"]["status"]
                .as_str()
                .unwrap_or("unknown")
                .to_owned(),
        };
        self.turn = Some(turn.clone());
        Ok(turn)
    }

    /// Sends one bounded native request and returns its `result`.
    ///
    /// The caller owns the meaning of the answer. Nothing is retried and no
    /// state is invented: an error answer, an answer carrying another request
    /// identity, an answer without a result, or a deadline is an error.
    pub fn call(&mut self, method: &str, params: Value) -> io::Result<Value> {
        match self.request(method, params)? {
            Reply::Result(result) => Ok(result),
            Reply::Rejected(error) => Err(io::Error::other(format!(
                "the native request {method} was rejected: {}",
                excerpt(&error.to_string(), 400)
            ))),
            Reply::Unanswered => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!(
                    "the native request {method} was not answered within {:?}; the conversation state is unknown and must not be reported as progress",
                    self.bound
                ),
            )),
        }
    }

    /// Sends one bounded native request and returns its answer as the protocol
    /// delivered it, or an error when no answer can be trusted (an answer
    /// carrying another request identity, a non-object record, a record with
    /// neither result nor error, or a transport failure).
    ///
    /// The distinction matters to a caller that must classify delivery: the
    /// server's own error answer is a definite refusal, while an unanswered
    /// request says nothing about whether the input was applied.
    pub fn request(&mut self, method: &str, params: Value) -> io::Result<Reply> {
        if method.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a control request requires a method",
            ));
        }
        self.next_id += 1;
        let id = self.next_id;
        self.connection.send(
            &json!({"id": id, "method": method, "params": params}),
            self.bound,
        )?;
        let until = Instant::now() + self.bound;
        loop {
            if Instant::now() >= until {
                return Ok(Reply::Unanswered);
            }
            let Some(value) = self.connection.receive(self.poll)? else {
                continue;
            };
            if value.get("method").is_some() {
                // Notifications that arrive while a request is in flight are
                // observed by the next pump, in arrival order.
                if self.pending.len() >= MAX_EVENTS_PER_PUMP {
                    return Err(invalid(
                        "the control event queue exceeded its bound while awaiting an answer",
                    ));
                }
                self.pending.push_back(value);
                continue;
            }
            if !value.is_object() {
                return Err(invalid(
                    "a control record is not a JSON object; refusing to guess conversation state",
                ));
            }
            if value["id"] != json!(id) {
                return Err(invalid(format!(
                    "the answer to {method} carried request identity {}",
                    value["id"]
                )));
            }
            if let Some(error) = value.get("error") {
                return Ok(Reply::Rejected(error.clone()));
            }
            if value.get("result").is_none() {
                return Err(invalid(format!("the answer to {method} carried no result")));
            }
            return Ok(Reply::Result(value["result"].clone()));
        }
    }

    /// Returns the conversation's control records as they arrive, waiting at
    /// most one poll interval for the first one and draining a burst after it.
    ///
    /// A closed connection is an error, never an empty batch: the host must see
    /// that the conversation can no longer be observed.
    pub fn pump(&mut self) -> io::Result<Vec<ControlEvent>> {
        let mut events = Vec::new();
        let mut wait = self.poll;
        loop {
            if let Some(value) = self.pending.pop_front() {
                if events.len() >= MAX_EVENTS_PER_PUMP {
                    self.pending.push_front(value);
                    break;
                }
                events.push(self.observe(value));
                wait = DRAIN;
                continue;
            }
            let Some(value) = self.connection.receive(wait)? else {
                break;
            };
            if events.len() >= MAX_EVENTS_PER_PUMP {
                self.pending.push_back(value);
                break;
            }
            events.push(self.observe(value));
            wait = DRAIN;
        }
        Ok(events)
    }

    /// Reads the thread's own state (`thread/read includeTurns`), refusing an
    /// answer that describes another thread.
    pub fn thread_state(&mut self) -> io::Result<Value> {
        if self.thread_id.is_empty() {
            return Err(invalid("this control session has no thread identity"));
        }
        let read = self.call(
            "thread/read",
            json!({"threadId": self.thread_id, "includeTurns": true}),
        )?;
        let thread = &read["thread"];
        if thread["id"].as_str() != Some(self.thread_id.as_str()) {
            return Err(invalid(format!(
                "thread/read answered for thread {} instead of {}; refusing to read another conversation",
                thread["id"], self.thread_id
            )));
        }
        Ok(thread.clone())
    }

    /// The final assistant message of the thread's last turn, taken from the
    /// thread items rather than from any transport acknowledgement.
    pub fn final_message(&mut self) -> io::Result<FinalMessage> {
        let thread = self.thread_state()?;
        Ok(final_message_from(&thread))
    }

    fn initialize(&mut self) -> io::Result<()> {
        self.call(
            "initialize",
            json!({
                "clientInfo": {"name": "harness-executor-control", "version": "1"},
                "capabilities": {"experimentalApi": true}
            }),
        )?;
        self.connection
            .send(&json!({"method": "initialized"}), self.bound)
    }

    fn start_thread(&mut self, plan: &ControlPlan) -> io::Result<()> {
        let started = self.call("thread/start", thread_params(plan))?;
        let id = started["thread"]["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                invalid(
                    "thread/start answered without a thread identity; no conversation exists to drive",
                )
            })?
            .to_owned();
        if let Err(mismatch) = plan.identity.verify(&started) {
            return Err(invalid(format!(
                "the executor profile binding was not preserved: {mismatch}; the conversation was refused instead of running with different routing"
            )));
        }
        self.call(
            "thread/name/set",
            json!({"threadId": id, "name": plan.title}),
        )?;
        self.thread_id = id.clone();
        self.endpoint.thread_id = Some(id);
        Ok(())
    }

    fn observe(&mut self, value: Value) -> ControlEvent {
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut event = ControlEvent {
            raw: value,
            method: method.clone(),
            lifecycle: None,
            deviation: None,
            repeat: false,
        };
        let Some(method) = method else {
            // A response that no request awaits is a deviation from the
            // request/response contract: its meaning must not be guessed.
            event.deviation = self.deviation(
                "a control record arrived without a method and without an outstanding request",
            );
            return event;
        };
        // One lead input arrives as a started and a completed record of the
        // same item; the surface renders its text once.
        if method.starts_with("item/")
            && event.raw["params"]["item"]["type"] == "userMessage"
            && !user_message_text(&event.raw["params"]["item"]).is_empty()
            && let Some(id) = event.raw["params"]["item"]["id"].as_str()
        {
            if self.rendered_inputs.len() >= RENDERED_INPUTS_LIMIT {
                self.rendered_inputs.clear();
            }
            event.repeat = !self.rendered_inputs.insert(id.to_owned());
        }
        let (lifecycle, deviation) = self.classify(&method, &event.raw);
        event.lifecycle = lifecycle;
        event.deviation = deviation;
        event
    }

    /// Maps one notification onto the lifecycle state it establishes, or onto
    /// a recorded deviation when it establishes nothing.
    fn classify(&mut self, method: &str, raw: &Value) -> (Option<Lifecycle>, Option<String>) {
        let bound = self.thread_id.clone();
        let params = &raw["params"];
        let addressed = params["threadId"].as_str().is_some_and(|id| id == bound);
        match method {
            "thread/started" => match params["thread"]["id"].as_str() {
                Some(id) if id == bound => (self.set(Lifecycle::NativeStart), None),
                // A dedicated child may still announce other threads; they are
                // not this conversation.
                Some(_) => (None, None),
                None => (
                    None,
                    self.deviation("thread/started carried no thread identity"),
                ),
            },
            "thread/status/changed" => {
                if !addressed {
                    return (None, None);
                }
                match params["status"]["type"].as_str() {
                    Some("active") => (self.set(Lifecycle::Running), None),
                    // Settling alone establishes nothing: the turn's own status
                    // is the outcome.
                    Some("idle" | "notLoaded") => (None, None),
                    Some("systemError") => {
                        let message = "the native thread reported a system error".to_owned();
                        self.failure = Some(message.clone());
                        (self.set(Lifecycle::Failed), Some(message))
                    }
                    Some(other) => (
                        None,
                        self.deviation(&format!(
                            "thread/status/changed reported the unknown status type {other}"
                        )),
                    ),
                    None => (
                        None,
                        self.deviation("thread/status/changed carried no status type"),
                    ),
                }
            }
            "turn/started" => {
                if !addressed {
                    return (None, None);
                }
                (self.progress(), None)
            }
            "turn/completed" => {
                if !addressed {
                    return (None, None);
                }
                match params["turn"]["status"].as_str() {
                    Some("completed") => (self.set(Lifecycle::Completed), None),
                    Some("failed") => {
                        let message = params["turn"]["error"]["message"]
                            .as_str()
                            .unwrap_or("the native turn failed without a message")
                            .to_owned();
                        self.failure = Some(message.clone());
                        (self.set(Lifecycle::Failed), Some(message))
                    }
                    Some("interrupted") => (self.set(Lifecycle::Interrupted), None),
                    Some(other) => (
                        self.set(Lifecycle::Defect),
                        self.deviation(&format!(
                            "turn/completed reported the unknown turn status {other}"
                        )),
                    ),
                    None => (
                        self.set(Lifecycle::Defect),
                        self.deviation("turn/completed carried no turn status"),
                    ),
                }
            }
            "item/started" | "item/updated" | "item/completed" => {
                if !addressed {
                    return (None, None);
                }
                if params["item"].get("type").and_then(Value::as_str).is_none() {
                    return (
                        None,
                        self.deviation(&format!("{method} carried no item type")),
                    );
                }
                (self.progress(), None)
            }
            "error" => {
                let retrying = params["willRetry"] == true;
                let message = params["error"]["message"]
                    .as_str()
                    .unwrap_or("the native control stream reported an error");
                // A retrying error is not a state; a terminal one is recorded as
                // the cause, while the turn's own status stays the state.
                if !retrying {
                    self.failure = Some(message.to_owned());
                }
                (None, None)
            }
            // Unknown methods are not deviations: a newer server may add
            // notifications this driver does not need to understand.
            _ => (None, None),
        }
    }

    fn set(&mut self, state: Lifecycle) -> Option<Lifecycle> {
        self.lifecycle = Some(state);
        Some(state)
    }

    /// Progress from a non-terminal state; a terminal or unknown state stays.
    /// The bound thread exists by construction, so its own item activity is
    /// evidence of running work even before a `thread/started` was observed.
    fn progress(&mut self) -> Option<Lifecycle> {
        match self.lifecycle {
            Some(state) if state.is_terminal() || state == Lifecycle::Defect => None,
            _ => self.set(Lifecycle::Running),
        }
    }

    fn deviation(&mut self, reason: &str) -> Option<String> {
        if self.defect.is_none() {
            self.defect = Some(reason.to_owned());
        }
        Some(reason.to_owned())
    }
}

fn thread_params(plan: &ControlPlan) -> Value {
    let mut params = Map::new();
    params.insert("cwd".into(), json!(plan.slot));
    // A fallback would silently change the routed model the binding recorded.
    params.insert("allowProviderModelFallback".into(), json!(false));
    if let Some(model) = &plan.identity.model {
        params.insert("model".into(), json!(model));
    }
    if let Some(provider) = &plan.identity.model_provider {
        params.insert("modelProvider".into(), json!(provider));
    }
    if let Some(policy) = &plan.approval_policy {
        params.insert("approvalPolicy".into(), json!(policy));
    }
    if let Some(sandbox) = &plan.sandbox {
        params.insert("sandbox".into(), json!(sandbox));
    }
    let mut config = Map::new();
    if let Some(effort) = &plan.identity.reasoning_effort {
        config.insert("model_reasoning_effort".into(), json!(effort));
    }
    for (key, value) in &plan.overrides {
        config.insert(key.clone(), value.clone());
    }
    if !config.is_empty() {
        params.insert("config".into(), Value::Object(config));
    }
    Value::Object(params)
}

/// The final assistant message of the last turn of one thread record.
fn final_message_from(thread: &Value) -> FinalMessage {
    let Some(turn) = thread["turns"].as_array().and_then(|turns| turns.last()) else {
        return FinalMessage::Missing;
    };
    let Some(last) = turn["items"].as_array().and_then(|items| {
        items
            .iter()
            .rev()
            .find(|item| item["type"] == "agentMessage")
    }) else {
        return FinalMessage::Missing;
    };
    match last["text"].as_str() {
        Some(text) if !text.trim().is_empty() => FinalMessage::Present(text.to_owned()),
        _ => FinalMessage::Empty,
    }
}

/// Binds a free loopback port for the child that will serve it.
fn free_loopback_port() -> io::Result<u16> {
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Spawns the app-server child and waits until its endpoint answers.
///
/// The child is spawned through the caller's Job, so its process tree stays
/// owned by the host exactly as the launcher tree did. Readiness is a served
/// endpoint, never a scheduling guess: a child that exits, and a child that
/// keeps running without serving, both fail closed with the log locator.
fn spawn_app_server(
    job: &Job,
    plan: &ControlPlan,
) -> io::Result<(OwnedProcess, ControlConnection, Endpoint)> {
    let token = capability_token()?;
    write_staged(&plan.paths.token, token.as_bytes())?;
    let port = match plan.port {
        Some(port) if port != 0 => port,
        _ => free_loopback_port()?,
    };
    let mut spec = app_server_spec(plan, port);
    let log = fs::File::create(&plan.paths.log).map_err(|error| {
        invalid(format!(
            "control app-server log {}: {error}",
            plan.paths.log.display()
        ))
    })?;
    spec.stdout = Some(log.try_clone()?);
    spec.stderr = Some(log);
    let process = job.spawn(&spec)?;
    let until = Instant::now() + plan.bound;
    let connection = loop {
        match ControlConnection::connect(port, &token, plan.poll) {
            Ok(connection) => break connection,
            Err(error) => {
                if !process.is_running()? {
                    let code = match process.exit_code()? {
                        Some(code) => format!("exit code {code}"),
                        None => "an unrecorded exit status".to_owned(),
                    };
                    return Err(io::Error::other(format!(
                        "the app-server child exited with {code} before serving its endpoint ({error}); log {}",
                        plan.paths.log.display()
                    )));
                }
                if Instant::now() >= until {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "the app-server child did not serve ws://127.0.0.1:{port} within {:?}; child pid {}; log {}",
                            plan.bound,
                            process.identity().pid,
                            plan.paths.log.display()
                        ),
                    ));
                }
                std::thread::sleep(RETRY);
            }
        }
    };
    let identity = process.identity();
    let endpoint = Endpoint::new(
        port,
        token,
        EndpointProcess {
            pid: identity.pid,
            creation_time: identity.creation_time,
            program: plan.launcher.clone(),
        },
    );
    Ok((process, connection, endpoint))
}

/// One capability token: 32 bytes of OS randomness as lowercase hex, the shape
/// the control transport requires of its bearer.
fn capability_token() -> io::Result<String> {
    let mut bytes = [0u8; TOKEN_BYTES];
    os_random(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Fills a buffer with the Windows system generator (`SystemFunction036`, the
/// documented API behind `rand_s` and `RtlGenRandom`).
///
/// The kit's window API feature set does not expose the entry point statically,
/// so it is resolved once from `advapi32.dll`. An unresolvable or failing
/// generator fails closed: a capability token is never taken from a weaker
/// source.
fn os_random(buffer: &mut [u8]) -> io::Result<()> {
    type Generator = unsafe extern "system" fn(*mut core::ffi::c_void, u32) -> u8;
    static GENERATOR: OnceLock<Option<Generator>> = OnceLock::new();
    let generator = *GENERATOR.get_or_init(|| unsafe {
        let mut module: HMODULE = GetModuleHandleW(windows_sys::core::w!("advapi32.dll"));
        if module.is_null() {
            module = LoadLibraryW(windows_sys::core::w!("advapi32.dll"));
        }
        if module.is_null() {
            return None;
        }
        let address: FARPROC = GetProcAddress(module, c"SystemFunction036".as_ptr().cast());
        address.map(|address| {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, Generator>(address)
        })
    });
    let generator = generator.ok_or_else(|| {
        io::Error::other(
            "the system random generator is unavailable; no control token was generated",
        )
    })?;
    let filled = unsafe { generator(buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if filled == 0 {
        return Err(io::Error::other(
            "the system random generator failed; no control token was generated",
        ));
    }
    Ok(())
}
