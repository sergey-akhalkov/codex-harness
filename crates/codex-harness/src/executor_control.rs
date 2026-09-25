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
    process::{
        Cancellation, CommandSpec, Deadline, ExclusiveFileLock, Job, OwnedProcess, ProcessIdentity,
    },
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

    /// The working state of one managed lead session host, named by the host
    /// process: the conversation's thread identity does not exist yet when the
    /// app-server command is built. The registry record that other commands
    /// resolve is published under that identity once `thread/start` or
    /// `thread/resume` established it.
    pub fn for_lead_host(codex_home: &Path, pid: u32) -> Self {
        let directory = lead_host_dir(codex_home);
        Self {
            endpoint: directory.join(format!("{pid}.json")),
            token: directory.join(format!("{pid}.token")),
            log: directory.join(format!("{pid}.log")),
        }
    }
}

/// Identity of the process that dispatched one executor run. A pid alone is
/// not an identity: the creation time and image must match too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct DispatcherIdentity {
    pub pid: u32,
    pub creation_time: u64,
    pub program: PathBuf,
}

/// Immutable originating relationship captured from the dispatching process
/// before its session environment is cleared for the host. This record is not
/// a lead endpoint, and a caller-supplied id or copied marker is not authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct OriginatingLead {
    pub schema: u32,
    pub thread_id: String,
    pub run_generation: String,
    pub dispatcher: DispatcherIdentity,
}

pub(crate) const ORIGINATING_LEAD_SCHEMA: u32 = 1;
/// Opaque per-run context carried to the executor host. It is never a lead
/// thread id and never authority for a later dispatch.
pub(crate) const EXECUTOR_RUN_ENV: &str = "HARNESS_EXECUTOR_RUN";

const LEAD_OVERRIDE_ENV: [&str; 3] = [
    "HARNESS_ORIGINATING_LEAD",
    "HARNESS_LEAD_THREAD",
    "HARNESS_LEAD_RECIPIENT",
];

pub(crate) fn supplied_lead_option(key: &str) -> bool {
    matches!(
        key,
        "--lead"
            | "--lead-thread"
            | "--lead-thread-id"
            | "--originating-lead"
            | "--recipient"
            | "--thread-id"
    )
}

/// Captures the dispatching process's own `CODEX_THREAD_ID`. The working
/// directory is not an input: a descendant that changes directory still
/// records this process's thread id. A copied run marker, a sibling run, a
/// stale reference, or a caller-supplied lead id fails closed. This does not
/// open a lead endpoint; a thread id that is merely present cannot be proved
/// to name a live lead conversation without one.
pub(crate) fn establish_originating_lead(state_dir: &Path) -> io::Result<OriginatingLead> {
    if LEAD_OVERRIDE_ENV
        .iter()
        .any(|name| std::env::var_os(name).is_some())
    {
        return Err(invalid(
            "caller-supplied lead id or recipient is not authority; refusing before a model request",
        ));
    }
    if let Some(marker) = std::env::var_os(EXECUTOR_RUN_ENV) {
        let marker = marker.to_string_lossy();
        return Err(invalid(copied_or_sibling_reason(state_dir, marker.trim())));
    }
    let thread_id = match std::env::var("CODEX_THREAD_ID") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => {
            return Err(invalid(
                "unverifiable originating lead: CODEX_THREAD_ID is missing; refusing before a model request",
            ));
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(invalid(
                "unverifiable originating lead: CODEX_THREAD_ID is not Unicode; refusing before a model request",
            ));
        }
    };
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Err(invalid(
            "unverifiable originating lead: CODEX_THREAD_ID is blank; refusing before a model request",
        ));
    }
    let program = std::env::current_exe().map_err(|error| {
        invalid(format!(
            "unverifiable originating lead: dispatching process image is unavailable: {error}; refusing before a model request"
        ))
    })?;
    let user = harness_core::process_service::current_user().map_err(|error| {
        invalid(format!(
            "unverifiable originating lead: dispatching process account is unavailable: {error}; refusing before a model request"
        ))
    })?;
    let identity = harness_core::process_service::ServiceProcess::observe(
        std::process::id(),
        &program,
        0,
        &user,
    )
    .map_err(|error| {
        invalid(format!(
            "unverifiable originating lead: dispatching process identity cannot be verified: {error}; refusing before a model request"
        ))
    })?
    .identity();
    Ok(OriginatingLead {
        schema: ORIGINATING_LEAD_SCHEMA,
        thread_id: thread_id.to_owned(),
        run_generation: run_generation()?,
        dispatcher: DispatcherIdentity {
            pid: identity.pid,
            creation_time: identity.creation_time,
            program,
        },
    })
}

fn copied_or_sibling_reason(state_dir: &Path, marker: &str) -> String {
    if marker.is_empty() {
        return "copied run marker is not authority; refusing before a model request".to_owned();
    }
    match marker_disposition(state_dir, marker) {
        MarkerDisposition::Stale => {
            "stale run reference is not authority; refusing before a model request".to_owned()
        }
        MarkerDisposition::Sibling => {
            "sibling run marker is not the originating lead; refusing before a model request"
                .to_owned()
        }
        MarkerDisposition::Copied => {
            "copied run marker is not authority; refusing before a model request".to_owned()
        }
    }
}

enum MarkerDisposition {
    Copied,
    Sibling,
    Stale,
}

fn marker_disposition(state_dir: &Path, marker: &str) -> MarkerDisposition {
    let Ok(entries) = fs::read_dir(state_dir) else {
        return MarkerDisposition::Copied;
    };
    let mut sibling = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("spawn-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(bytes) = fs::read(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        let Some(recorded) = value["originatingLead"]["runGeneration"].as_str() else {
            continue;
        };
        if recorded != marker {
            continue;
        }
        if dispatcher_is_stale(&value["originatingLead"]["dispatcher"]) {
            return MarkerDisposition::Stale;
        }
        sibling = true;
    }
    if sibling {
        MarkerDisposition::Sibling
    } else {
        MarkerDisposition::Copied
    }
}

fn dispatcher_is_stale(value: &Value) -> bool {
    let Some(pid) = value["pid"].as_u64() else {
        return true;
    };
    let Some(creation) = value["creationTime"].as_u64() else {
        return true;
    };
    if pid == 0 || creation == 0 || pid > u64::from(u32::MAX) {
        return true;
    }
    let Some(program) = value["program"].as_str().map(PathBuf::from) else {
        return true;
    };
    if !program.is_file() {
        return true;
    }
    let Ok(user) = harness_core::process_service::current_user() else {
        return true;
    };
    !matches!(
        harness_core::process_service::ServiceProcess::inspect(
            ProcessIdentity {
                pid: pid as u32,
                creation_time: creation,
            },
            &program,
            &user,
        ),
        Ok(Some(_))
    )
}

fn run_generation() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    os_random(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
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
    /// Whether this plan serves an executor conversation. An executor
    /// conversation is marked with the executor session environment and runs
    /// the single-agent override; a managed lead session keeps the native agent
    /// capability and must not be mistaken for an executor by the launcher or
    /// by the kit's own nested-dispatch refusal.
    pub executor_session: bool,
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
            executor_session: true,
            bound: DEFAULT_BOUND,
            poll: DEFAULT_POLL,
        }
    }
}

/// The `codex app-server` command one control session spawns: the loopback
/// listener, the capability-token file, the caller's extra arguments, the bound
/// slot as working directory, and - for an executor conversation - the executor
/// session environment and the agent-tool override. A managed lead session gets
/// neither, because the lead keeps the native agent capability. The parent's
/// session and thread id are removed in both cases, so the child cannot attach
/// to a conversation it does not serve.
pub fn app_server_spec(plan: &ControlPlan, port: u16) -> CommandSpec {
    let mut command = CommandSpec::new(&plan.launcher);
    command.current_dir = Some(plan.slot.clone());
    command.args = app_server_args(plan, port);
    command.env.insert(
        "CODEX_HOME".into(),
        Some(plan.home.clone().into_os_string()),
    );
    if plan.executor_session {
        command
            .env
            .insert(EXECUTOR_SESSION_ENV.into(), Some("1".into()));
    }
    for (key, value) in &plan.env {
        command.env.insert(key.clone(), value.clone());
    }
    // After plan overrides, so a leaked lead id cannot ride along as the
    // child's native identity.
    command.env.insert("CODEX_SESSION_ID".into(), None);
    command.env.insert("CODEX_THREAD_ID".into(), None);
    command
}

/// The turn currently in progress on one thread record, when a turn is
/// running. Native requests that address an active turn (`turn/steer`,
/// `turn/interrupt`) name it by id, not by thread alone.
pub fn active_turn(thread: &Value) -> Option<String> {
    thread["turns"]
        .as_array()?
        .iter()
        .rev()
        .find(|turn| turn["status"] == "inProgress")
        .and_then(|turn| turn["id"].as_str())
        .map(str::to_owned)
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
    if plan.executor_session {
        // A managed lead session keeps the native agent capability: neither
        // this override nor the session marker reaches its app-server child.
        args.extend(EXECUTOR_AGENT_TOOLS_OFF.map(OsString::from));
    }
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

/// Kit-local registry of managed lead sessions: one endpoint record per exact
/// native lead thread, so an executor spawn running in that thread's own shell
/// can inherit the address of the conversation that dispatched it. The registry
/// holds addresses and their bearers only - never message text, transcripts or
/// board state - and its records are local session state, not shared artifacts.
pub fn lead_registry_dir(codex_home: &Path) -> PathBuf {
    codex_home.join("harness/lead-endpoints")
}

/// Working directory of one managed lead session host: the app-server's
/// capability-token file, its combined log, and the transient endpoint record
/// the driver writes before the conversation has a thread identity. It is not
/// the registry: only the thread-keyed records beside it are addresses other
/// commands resolve.
pub fn lead_host_dir(codex_home: &Path) -> PathBuf {
    lead_registry_dir(codex_home).join("host")
}

/// The exact thread id is the registry key. A value that cannot be a file name
/// (a path, a reserved name, an unreasonable length) is refused before any
/// record is written or read, so a spoofed `CODEX_THREAD_ID` cannot name state
/// this registry does not own.
fn lead_key(thread_id: &str) -> io::Result<String> {
    let key = thread_id.trim();
    if key.is_empty()
        || key.len() > 128
        || key == "."
        || key == ".."
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(invalid(
            "a lead session identity that is not a bounded registry key cannot name a registered endpoint",
        ));
    }
    Ok(key.to_owned())
}

/// The registry file of one exact lead thread.
pub fn lead_endpoint_path(codex_home: &Path, thread_id: &str) -> io::Result<PathBuf> {
    Ok(lead_registry_dir(codex_home).join(format!("{}.json", lead_key(thread_id)?)))
}

/// What the registry reports for one exact lead thread.
pub enum RegisteredLead {
    /// A validated record whose recorded app-server process is still live.
    Live(Box<Endpoint>),
    /// No usable endpoint. The reason is bounded and names no bearer.
    Unavailable(String),
}

/// Resolves the registered endpoint of one exact lead thread, failing closed: a
/// missing, unreadable, malformed, mismatched or stale record is never reported
/// as a usable address. Spawn inherits only a live record, and `lead message`
/// refuses with this reason instead of guessing an address.
pub fn registered_lead(codex_home: &Path, thread_id: &str) -> RegisteredLead {
    let path = match lead_endpoint_path(codex_home, thread_id) {
        Ok(path) => path,
        Err(error) => return RegisteredLead::Unavailable(error.to_string()),
    };
    let endpoint = match Endpoint::read(&path) {
        Ok(endpoint) => endpoint,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return RegisteredLead::Unavailable(
                "no lead endpoint is registered for the dispatching lead session".to_owned(),
            );
        }
        // Every other failure is reported as the bounded fact: a malformed
        // record's parse detail can echo record fields, and an unusable record
        // is not an address in any case.
        Err(_) => {
            return RegisteredLead::Unavailable(
                "the registered lead endpoint record is unreadable or malformed".to_owned(),
            );
        }
    };
    if endpoint.thread_id.as_deref() != Some(thread_id.trim()) {
        return RegisteredLead::Unavailable(
            "the registered lead endpoint belongs to another conversation".to_owned(),
        );
    }
    let Some(process) = endpoint.process.as_ref() else {
        return RegisteredLead::Unavailable(
            "the registered lead endpoint records no app-server process".to_owned(),
        );
    };
    if !endpoint_process_live(process) {
        return RegisteredLead::Unavailable(
            "the process registered for this lead session is no longer live".to_owned(),
        );
    }
    RegisteredLead::Live(Box::new(endpoint))
}

/// Whether the recorded endpoint process is still the process that serves it:
/// pid, creation time and image must match the current process table. A bare
/// pid, a missing image, an unverifiable identity or a reused pid is not
/// liveness, so a stale address can never be inherited or delivered to.
pub fn endpoint_process_live(process: &EndpointProcess) -> bool {
    if process.pid == 0 || process.creation_time == 0 || !process.program.is_file() {
        return false;
    }
    let Ok(user) = harness_core::process_service::current_user() else {
        return false;
    };
    matches!(
        harness_core::process_service::ServiceProcess::inspect(
            ProcessIdentity {
                pid: process.pid,
                creation_time: process.creation_time,
            },
            &process.program,
            &user,
        ),
        Ok(Some(_))
    )
}

/// Publishes one managed lead session's verified endpoint under its exact
/// thread id, replacing an earlier record of that thread in one step. Refusing
/// a thread without an identity keeps an unaddressable record out of the
/// registry, where every resolver would have to guess what it belongs to.
pub fn publish_lead_endpoint(codex_home: &Path, endpoint: &Endpoint) -> io::Result<PathBuf> {
    let thread = endpoint
        .thread_id
        .clone()
        .filter(|thread| !thread.trim().is_empty())
        .ok_or_else(|| {
            invalid(
                "a lead endpoint can only be recorded for a conversation with a thread identity",
            )
        })?;
    let path = lead_endpoint_path(codex_home, &thread)?;
    endpoint.record(&path)?;
    Ok(path)
}

/// Removes one thread's registry record. A missing record is the normal case of
/// a session that never published, or already retired, its address.
pub fn retire_lead_endpoint(codex_home: &Path, thread_id: &str) -> io::Result<()> {
    let path = lead_endpoint_path(codex_home, thread_id)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io::Error::other(format!(
            "the lead endpoint record {} could not be retired: {error}",
            path.display()
        ))),
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
/// conversation; token-level delta records are the one exception, because
/// their completed item already carries their content.
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
    /// True for a token-level delta record (`…/delta`, `…textDelta`) or a
    /// repeated MCP startup progress record. Its content is fully derivable
    /// from the completed item or from the server's subsequent behavior, so it
    /// is neither rendered on the visible surface nor retained in the bounded
    /// detail file; one such record per model token or per server would
    /// otherwise flood both.
    pub transient: bool,
}

impl ControlEvent {
    /// Writes one readable line for the session's own visible surface.
    ///
    /// Reasoning summaries are intentionally not rendered: the surface carries
    /// decisions, activity and lifecycle, not opaque model state.
    pub fn render(&self, out: &mut dyn Write) -> io::Result<()> {
        if self.repeat || self.transient {
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
            // A method this driver does not interpret is not surface noise:
            // it stays in the bounded detail record (unless transient), and
            // the readable surface keeps only the records it can name.
            Some(_) => None,
            None => None,
        }
    }
}

fn is_transient(method: &str) -> bool {
    method.ends_with("/delta")
        || method.ends_with("Delta")
        || method == "mcpServer/startupStatus/updated"
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
    /// The turn whose terminal status can finish this run. Assignment
    /// submission and later accepted input update it. A historical turn cannot.
    turn: Option<TurnStart>,
    /// Turns whose terminal status was already observed. A replay cannot finish
    /// this run.
    settled_turns: BTreeSet<String>,
    /// Set once further input is no longer accepted. The host may still be
    /// rendering the tail; closure of the frontend is a later, separate step.
    closure_begun: bool,
    /// Terminal outcome of the current turn, committed only after a quiet pump
    /// so a correction in the same or next burst can supersede it.
    pending_terminal: Option<(Lifecycle, Option<String>)>,
    /// A user input arrived without a turn id and the next `turn/started` may
    /// be that input's turn.
    input_awaits_turn: bool,
    /// Dispatch receipt beside the endpoint, when this conversation was started
    /// by the host. Reply holds and undelivered input live there.
    receipt: Option<PathBuf>,
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
        let mut conversation = Self::open(job, plan)?;
        conversation.start_thread(plan)?;
        conversation.record_endpoint(plan)?;
        Ok(conversation)
    }

    /// Starts the app-server and resumes one exact existing thread.
    ///
    /// This does not call `thread/start`. The conversation identity is the
    /// requested session, so attaching the native frontend cannot replace the
    /// recorded session with a new one. A resume answer for another thread is
    /// refused before the endpoint is published.
    // The host uses this. The fixture test binary compiles this file alone.
    #[allow(dead_code)]
    pub fn start_resuming(job: &Job, plan: &ControlPlan, session: &str) -> io::Result<Self> {
        if session.is_empty() {
            return Err(invalid(
                "exact-session resume requires the recorded session id; refusing to start another conversation",
            ));
        }
        let mut conversation = Self::open(job, plan)?;
        conversation.resume_exact(session, plan)?;
        conversation.record_endpoint(plan)?;
        Ok(conversation)
    }

    /// Starts the app-server and resumes one exact existing thread while
    /// keeping the routing and the directory that thread's own record already
    /// established.
    ///
    /// This is the lead's own conversation: the kit gives it a delivery
    /// endpoint and must not rewrite the model, provider, effort or working
    /// directory a native session already has. Only the exact thread identity
    /// and the selected checkout are verified, so a second conversation can
    /// never be attached or described in this thread's place.
    pub fn start_resuming_own(job: &Job, plan: &ControlPlan, session: &str) -> io::Result<Self> {
        if session.is_empty() {
            return Err(invalid(
                "exact-session resume requires the recorded session id; refusing to start another conversation",
            ));
        }
        let mut conversation = Self::open(job, plan)?;
        conversation.resume_exact_own(session, plan)?;
        conversation.record_endpoint(plan)?;
        Ok(conversation)
    }

    fn open(job: &Job, plan: &ControlPlan) -> io::Result<Self> {
        if plan.launcher.as_os_str().is_empty() || plan.slot.as_os_str().is_empty() {
            return Err(invalid(
                "a control session requires the launcher, the kit home and the bound slot",
            ));
        }
        if plan.identity.model_provider.as_deref() == Some("xai") {
            // app-server receives profile settings through -c and thread/start,
            // so the launcher's --profile xai preparation never runs. Start the
            // shared transport here, outside the app-server's owned Job.
            let prepared = (|| -> io::Result<()> {
                let launcher = plan.launcher.canonicalize()?;
                let manager = launcher
                    .parent()
                    .ok_or_else(|| invalid("launcher has no build directory"))?
                    .join("codex-harness.exe")
                    .canonicalize()?;
                harness_core::native_launcher::ensure_xai_shim(
                    &manager,
                    harness_core::xai_responses_shim::DEFAULT_PORT,
                )
            })();
            prepared.map_err(|error| {
                invalid(format!(
                    "xAI compatibility shim preparation failed before app-server startup: {error}; check the installed launcher and its sibling codex-harness.exe"
                ))
            })?;
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
            settled_turns: BTreeSet::new(),
            closure_begun: false,
            pending_terminal: None,
            input_awaits_turn: false,
            receipt: receipt_beside_endpoint(&plan.paths.endpoint),
            pending: VecDeque::new(),
            rendered_inputs: BTreeSet::new(),
            next_id: 0,
            bound: plan.bound,
            poll: plan.poll,
        };
        conversation.initialize()?;
        Ok(conversation)
    }

    fn record_endpoint(&self, plan: &ControlPlan) -> io::Result<()> {
        self.endpoint.record(&plan.paths.endpoint).map_err(|error| {
            invalid(format!(
                "control endpoint record {}: {error}",
                plan.paths.endpoint.display()
            ))
        })
    }

    /// Adopts `session` through `thread/resume` and refuses any other thread.
    fn resume_exact(&mut self, session: &str, plan: &ControlPlan) -> io::Result<()> {
        let resumed = self.call("thread/resume", json!({"threadId": session}))?;
        if resumed["thread"]["id"].as_str() != Some(session) {
            return Err(invalid(
                "thread/resume returned another thread; refusing to attach a frontend to a different conversation",
            ));
        }
        if let Err(mismatch) = confirm_resumed(&plan.identity, &resumed, &plan.slot) {
            return Err(invalid(format!(
                "resuming the exact session changed the bound routing: {mismatch}"
            )));
        }
        self.thread_id = session.to_owned();
        self.endpoint.thread_id = Some(session.to_owned());
        Ok(())
    }

    /// Adopts `session` through `thread/resume` for a conversation that keeps
    /// its own routing, and refuses any other thread. The reported directory
    /// must be the selected checkout or inside it: a session that runs outside
    /// the selected source would make this command's own record wrong.
    fn resume_exact_own(&mut self, session: &str, plan: &ControlPlan) -> io::Result<()> {
        let resumed = self.call("thread/resume", json!({"threadId": session}))?;
        if resumed["thread"]["id"].as_str() != Some(session) {
            return Err(invalid(
                "thread/resume returned another thread; refusing to attach a frontend to a different conversation",
            ));
        }
        if let Some(cwd) = resumed["thread"]["cwd"]
            .as_str()
            .or_else(|| resumed["cwd"].as_str())
            && !inside_checkout(cwd, &plan.slot)
        {
            return Err(invalid(format!(
                "the resumed conversation runs in {cwd}, outside the selected checkout {}; name the checkout that conversation runs in as --source instead of exposing a session of another directory",
                plan.slot.display()
            )));
        }
        self.thread_id = session.to_owned();
        self.endpoint.thread_id = Some(session.to_owned());
        Ok(())
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
            settled_turns: BTreeSet::new(),
            closure_begun: false,
            pending_terminal: None,
            input_awaits_turn: false,
            receipt: None,
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
        if !self.closure_begun {
            self.pending_terminal = None;
            self.turn = Some(turn.clone());
        }
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
    /// that the conversation can no longer be observed. A pending terminal
    /// outcome is committed at the end of the burst, after a correction in the
    /// same burst has had a chance to supersede it.
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
        self.settle_batch(&mut events);
        Ok(events)
    }

    /// Reads the thread's own state (`thread/read includeTurns`), refusing an
    /// answer that describes another thread.
    pub fn thread_state(&mut self) -> io::Result<Value> {
        if self.thread_id.is_empty() {
            return Err(invalid("this control session has no thread identity"));
        }
        let read = match self.request(
            "thread/read",
            json!({"threadId": self.thread_id, "includeTurns": true}),
        )? {
            Reply::Result(result) => result,
            // codex-cli 0.156.1 rejects includeTurns on an idle thread with
            // `list_turns is not supported yet`. A plain read still names the
            // thread, so an idle conversation can receive turn/start.
            Reply::Rejected(error) if error.to_string().contains("list_turns") => {
                self.call("thread/read", json!({"threadId": self.thread_id}))?
            }
            Reply::Rejected(error) => {
                return Err(io::Error::other(format!(
                    "the native request thread/read was rejected: {}",
                    excerpt(&error.to_string(), 400)
                )));
            }
            Reply::Unanswered => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "the native request thread/read was not answered within {:?}; the conversation state is unknown and must not be reported as progress",
                        self.bound
                    ),
                ));
            }
        };
        let thread = &read["thread"];
        if thread["id"].as_str() != Some(self.thread_id.as_str()) {
            return Err(invalid(format!(
                "thread/read answered for thread {} instead of {}; refusing to read another conversation",
                thread["id"], self.thread_id
            )));
        }
        Ok(thread.clone())
    }

    /// The final assistant message of the accepted turn, taken from the thread
    /// items rather than from any transport acknowledgement or an older turn.
    pub fn final_message(&mut self) -> io::Result<FinalMessage> {
        let thread = self.thread_state()?;
        let turn_id = self.turn.as_ref().map(|turn| turn.turn_id.as_str());
        Ok(final_message_for(&thread, turn_id))
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
                "the profile binding was not preserved: {mismatch}; the conversation was refused instead of running with different routing"
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

    /// Materializes the named empty thread through `thread/resume` before any
    /// model request. Remote resume has no rollout to open until this returns
    /// the same thread, model and provider the start pinned.
    // The host uses these. The fixture test binary compiles this file alone.
    #[allow(dead_code)]
    pub fn prepare_named_empty(&mut self, slot: &Path) -> io::Result<()> {
        let resumed = self.call("thread/resume", json!({"threadId": self.thread_id}))?;
        if resumed["thread"]["id"].as_str() != Some(self.thread_id.as_str()) {
            return Err(invalid(
                "thread/resume returned another thread; refusing to attach a frontend to a different conversation",
            ));
        }
        if let Some(identity) = &self.identity
            && let Err(mismatch) = confirm_resumed(identity, &resumed, slot)
        {
            return Err(invalid(format!(
                "preparing the named empty thread changed the bound routing: {mismatch}"
            )));
        }
        Ok(())
    }

    /// Stops the turn that is actually in progress. A thread with no active
    /// turn is already idle; this does not start a model request.
    #[allow(dead_code)]
    pub fn interrupt_active(&mut self) -> io::Result<bool> {
        let thread = self.thread_state()?;
        let Some(turn) = active_turn(&thread) else {
            return Ok(false);
        };
        self.call(
            "turn/interrupt",
            json!({"threadId": self.thread_id, "turnId": turn}),
        )?;
        Ok(true)
    }

    /// Whether the bound thread already contains a turn. The assignment must
    /// not be submitted again when it does.
    #[allow(dead_code)]
    pub fn has_turns(&mut self) -> io::Result<bool> {
        let thread = self.thread_state()?;
        Ok(thread["turns"]
            .as_array()
            .is_some_and(|turns| !turns.is_empty()))
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
            transient: method.as_deref().is_some_and(is_transient),
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
        // same item; the surface renders its text once, and acceptance is once.
        if method.starts_with("item/")
            && event.raw["params"]["item"]["type"] == "userMessage"
            && !user_message_text(&event.raw["params"]["item"]).is_empty()
            && let Some(id) = event.raw["params"]["item"]["id"].as_str()
        {
            if self.rendered_inputs.len() >= RENDERED_INPUTS_LIMIT {
                self.rendered_inputs.clear();
            }
            let fresh = self.rendered_inputs.insert(id.to_owned());
            event.repeat = !fresh;
            if fresh {
                let text = user_message_text(&event.raw["params"]["item"]);
                let turn_id = item_turn_id(&event.raw["params"]).map(str::to_owned);
                if let Some(client_id) = item_client_id(&event.raw["params"]["item"]) {
                    self.resolve_observed_reply(client_id, id, turn_id.as_deref());
                }
                self.note_user_input(id, turn_id.as_deref(), &text);
            }
        }
        let (lifecycle, deviation) = self.classify(&method, &event.raw);
        event.lifecycle = lifecycle;
        event.deviation = deviation;
        event
    }

    /// Maps one notification onto the lifecycle state it establishes, or onto
    /// a recorded deviation when it establishes nothing.
    ///
    /// A terminal turn status finishes this run only when it names the current
    /// accepted turn and no newer input superseded it in this burst. A previous
    /// session's completion, or an older turn's completion after a correction,
    /// is history. The lifecycle is not committed here: [`Self::settle_pending`]
    /// does that on a quiet pump so the two cannot be confused.
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
                self.note_turn_started(turn_id_of(params));
                (self.progress(), None)
            }
            "turn/completed" => {
                if !addressed {
                    return (None, None);
                }
                let Some(id) = turn_id_of(params) else {
                    if self.turn.is_none() || self.closure_begun {
                        return (None, None);
                    }
                    // No identity, no correlation. Do not treat it as success.
                    let cause = "turn/completed carried no turn identity; refusing to treat it as this run's completion"
                        .to_owned();
                    self.deviation(&cause);
                    self.arm_terminal(Lifecycle::Defect, Some(cause.clone()));
                    return (None, Some(cause));
                };
                // A driver that is only observing adopts the first addressed
                // turn. Once a turn is accepted, another id is history.
                if self.turn.is_none() {
                    self.turn = Some(TurnStart {
                        turn_id: id.to_owned(),
                        status: params["turn"]["status"]
                            .as_str()
                            .unwrap_or("unknown")
                            .to_owned(),
                    });
                }
                if !self.is_current(id) {
                    self.settled_turns.insert(id.to_owned());
                    return (None, None);
                }
                self.settled_turns.insert(id.to_owned());
                match params["turn"]["status"].as_str() {
                    Some("completed") => {
                        if self.reply_hold_unresolved() {
                            self.pending_terminal = None;
                            return (None, None);
                        }
                        self.arm_terminal(Lifecycle::Completed, None);
                        (None, None)
                    }
                    Some("failed") => {
                        let message = params["turn"]["error"]["message"]
                            .as_str()
                            .unwrap_or("the native turn failed without a message")
                            .to_owned();
                        self.arm_terminal(Lifecycle::Failed, Some(message.clone()));
                        (None, Some(message))
                    }
                    Some("interrupted") => {
                        self.arm_terminal(Lifecycle::Interrupted, None);
                        (None, None)
                    }
                    Some(other) => {
                        let cause =
                            format!("turn/completed reported the unknown turn status {other}");
                        self.deviation(&cause);
                        self.arm_terminal(Lifecycle::Defect, Some(cause.clone()));
                        (None, Some(cause))
                    }
                    None => {
                        let cause = "turn/completed carried no turn status".to_owned();
                        self.deviation(&cause);
                        self.arm_terminal(Lifecycle::Defect, Some(cause.clone()));
                        (None, Some(cause))
                    }
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

    fn is_current(&self, id: &str) -> bool {
        self.turn.as_ref().is_some_and(|turn| turn.turn_id == id)
    }

    fn note_turn_started(&mut self, id: Option<&str>) {
        let Some(id) = id else {
            return;
        };
        if self.closure_begun || self.settled_turns.contains(id) {
            return;
        }
        let current = self.turn.as_ref().map(|turn| turn.turn_id.as_str());
        if current == Some(id) {
            self.input_awaits_turn = false;
            return;
        }
        let current_settled = current.is_some_and(|current| self.settled_turns.contains(current));
        if self.input_awaits_turn || current_settled {
            self.accept_observed_turn(id);
        }
        self.input_awaits_turn = false;
    }

    fn note_user_input(&mut self, id: &str, turn_id: Option<&str>, text: &str) {
        if self.closure_begun {
            self.report_undelivered(id, text);
            return;
        }
        if let Some(turn_id) = turn_id {
            if !self.settled_turns.contains(turn_id)
                && self
                    .turn
                    .as_ref()
                    .is_some_and(|turn| turn.turn_id != turn_id)
            {
                self.accept_observed_turn(turn_id);
            }
            self.input_awaits_turn = false;
        } else {
            self.input_awaits_turn = true;
        }
    }

    /// Upgrades the one queued reply whose client id the conversation now
    /// observes, then resolves only the lead request that reply names. This is
    /// the late half of a queued `turn/steer`: transport acceptance alone could
    /// not establish delivery, but the conversation's own item can do so after
    /// the in-flight tool returns.
    fn resolve_observed_reply(&self, client_id: &str, item_id: &str, turn_id: Option<&str>) {
        let Some(receipt) = &self.receipt else {
            return;
        };
        let _ = resolve_observed_reply(receipt, client_id, item_id, turn_id);
    }

    fn accept_observed_turn(&mut self, id: &str) {
        self.pending_terminal = None;
        if !self.closure_begun
            && self
                .lifecycle
                .is_some_and(|state| state.is_terminal() || state == Lifecycle::Defect)
        {
            self.lifecycle = Some(Lifecycle::Running);
        }
        self.turn = Some(TurnStart {
            turn_id: id.to_owned(),
            status: "inProgress".to_owned(),
        });
    }

    fn arm_terminal(&mut self, state: Lifecycle, cause: Option<String>) {
        self.pending_terminal = Some((state, cause));
    }

    /// Commits a terminal outcome that this burst did not supersede, and stamps
    /// it on the turn event so callers see the same lifecycle the conversation
    /// recorded. A reply hold keeps a completed turn nonterminal.
    fn settle_batch(&mut self, events: &mut [ControlEvent]) {
        let Some((state, cause)) = self.pending_terminal.take() else {
            return;
        };
        if state == Lifecycle::Completed && self.reply_hold_unresolved() {
            return;
        }
        if let Some(cause) = cause.as_deref() {
            match state {
                Lifecycle::Failed => self.failure = Some(cause.to_owned()),
                Lifecycle::Defect => {
                    self.deviation(cause);
                }
                _ => {}
            }
        }
        self.lifecycle = Some(state);
        self.closure_begun = true;
        if let Some(receipt) = self.receipt.clone() {
            let _ = mark_input_closure(&receipt);
        }
        if let Some(event) = events
            .iter_mut()
            .rev()
            .find(|event| event.method.as_deref() == Some("turn/completed"))
        {
            event.lifecycle = Some(state);
            if event.deviation.is_none()
                && let Some(cause) = cause
            {
                event.deviation = Some(cause);
            }
        }
    }

    fn reply_hold_unresolved(&self) -> bool {
        let Some(receipt) = &self.receipt else {
            return false;
        };
        // A receipt that cannot be read does not prove the hold is gone.
        // Leaving the run nonterminal is safer than reporting completion.
        unresolved_reply_hold_at(receipt).unwrap_or(true)
    }

    fn report_undelivered(&self, id: &str, text: &str) {
        let Some(receipt) = &self.receipt else {
            return;
        };
        let detail = format!(
            "input arrived after closure began and was not delivered: {}",
            excerpt(text, 200)
        );
        let _ = record_undelivered_input(receipt, id, &detail);
    }

    fn set(&mut self, state: Lifecycle) -> Option<Lifecycle> {
        let closed = self.closure_begun
            || self
                .lifecycle
                .is_some_and(|current| current.is_terminal() || current == Lifecycle::Defect);
        if closed && !state.is_terminal() && state != Lifecycle::Defect {
            return self.lifecycle;
        }
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

#[allow(dead_code)]
fn confirm_resumed(identity: &BoundIdentity, resumed: &Value, slot: &Path) -> Result<(), String> {
    if let Some(model) = &identity.model {
        compare_field(&resumed["model"], model, "model")?;
    }
    if let Some(provider) = &identity.model_provider {
        compare_field(&resumed["modelProvider"], provider, "modelProvider")?;
    }
    if let Some(effort) = &identity.reasoning_effort
        && resumed
            .get("reasoningEffort")
            .is_some_and(|value| !value.is_null())
    {
        compare_field(&resumed["reasoningEffort"], effort, "reasoningEffort")?;
    }
    if let Some(cwd) = resumed["thread"]["cwd"]
        .as_str()
        .or_else(|| resumed["cwd"].as_str())
        && !same_slot(cwd, slot)
    {
        return Err(format!(
            "the resumed thread cwd is {cwd}, not the bound slot {}",
            slot.display()
        ));
    }
    Ok(())
}

#[allow(dead_code)]
fn same_slot(reported: &str, slot: &Path) -> bool {
    normalize_slot(reported).eq_ignore_ascii_case(&normalize_slot(&slot.to_string_lossy()))
}

#[allow(dead_code)]
fn normalize_slot(path: &str) -> String {
    let mut text = path.replace('/', "\\");
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        text = format!(r"\\{rest}");
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        text = rest.to_owned();
    }
    text.trim_end_matches('\\').to_owned()
}

/// Whether the reported directory is the selected checkout or one of its
/// subdirectories. A lead session may work in a subdirectory of its checkout;
/// a session outside the selected checkout is a different conversation than
/// this command would record.
#[allow(dead_code)]
fn inside_checkout(reported: &str, checkout: &Path) -> bool {
    let reported = normalize_slot(reported).to_ascii_lowercase();
    let checkout = normalize_slot(&checkout.to_string_lossy()).to_ascii_lowercase();
    reported == checkout
        || reported
            .strip_prefix(&checkout)
            .is_some_and(|rest| rest.starts_with('\\'))
}

/// The final assistant message of one thread record.
///
/// When `turn_id` is set, only that accepted turn is read. Falling back to
/// another turn would let a stale completion supply this run's result.
fn final_message_for(thread: &Value, turn_id: Option<&str>) -> FinalMessage {
    let turns = thread["turns"].as_array();
    let turn = match (turns, turn_id) {
        (Some(turns), Some(id)) => turns.iter().find(|turn| turn["id"].as_str() == Some(id)),
        (Some(turns), None) => turns.last(),
        _ => None,
    };
    let Some(turn) = turn else {
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

const RECEIPT_LOCK_WAIT: Duration = Duration::from_secs(5);

/// Same hold record the observation owner reads, including an unanswered
/// `leadMessages` reply-request. Kept here because this file is also compiled
/// alone by the control fixture tests.
fn unresolved_reply_hold(receipt: &Value) -> bool {
    reply_request_hold(receipt.get("replyRequests"))
        || lead_message_hold(receipt.get("leadMessages"))
}

/// `replyRequests` is the explicit hold record. A missing field is not a hold.
fn reply_request_hold(requests: Option<&Value>) -> bool {
    let Some(requests) = requests.and_then(Value::as_array) else {
        return false;
    };
    requests.iter().any(|request| {
        request.get("status").and_then(Value::as_str) == Some("unresolved")
            && request.get("kind").and_then(Value::as_str) != Some("notify")
            && request.get("requiresReply").and_then(Value::as_bool) != Some(false)
    })
}

/// `lead message` writes `leadMessages`, not `replyRequests`. A reply-request
/// that was not refused and not resolved is the same hold. A notification is not.
fn lead_message_hold(messages: Option<&Value>) -> bool {
    let Some(messages) = messages.and_then(Value::as_array) else {
        return false;
    };
    messages.iter().any(|message| {
        message.get("kind").and_then(Value::as_str) == Some("reply-request")
            && message.get("requiresReply").and_then(Value::as_bool) != Some(false)
            && !matches!(
                message.get("status").and_then(Value::as_str),
                Some("resolved" | "error" | "refused")
            )
    })
}

fn unresolved_reply_hold_at(receipt: &Path) -> io::Result<bool> {
    if !receipt.is_file() {
        return Ok(false);
    }
    with_control_receipt_lock(receipt, || {
        let value =
            serde_json::from_slice::<Value>(&fs::read(receipt)?).map_err(io::Error::other)?;
        Ok(unresolved_reply_hold(&value))
    })
}

fn item_client_id(item: &Value) -> Option<&str> {
    item.get("clientId")
        .or_else(|| item.get("client_id"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
}

fn receipt_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default()
}

/// Reconciles one late queued reply from native evidence. Correlation is by the
/// recorded client message id and the reply's named request; an ordinary
/// steering input has no `replyTo` and never resolves a question.
fn resolve_observed_reply(
    receipt: &Path,
    client_id: &str,
    item_id: &str,
    turn_id: Option<&str>,
) -> io::Result<()> {
    with_control_receipt_lock(receipt, || {
        let mut value: Value =
            serde_json::from_slice(&fs::read(receipt)?).map_err(io::Error::other)?;
        let messages = value
            .get_mut("messages")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| io::Error::other("dispatch receipt messages field is not an array"))?;
        let Some(index) = messages.iter().position(|message| {
            message
                .get("clientMessageId")
                .or_else(|| message.get("id"))
                .and_then(Value::as_str)
                == Some(client_id)
        }) else {
            return Ok(());
        };
        if messages[index]["status"] != Value::String("delivered".to_owned()) {
            messages[index]["status"] = Value::String("delivered".to_owned());
            messages[index]["evidence"] = Value::String(format!(
                "observed as userMessage {item_id} of turn {}, correlated by the recorded client message id",
                turn_id.unwrap_or("unrecorded")
            ));
            let now = receipt_now_ms();
            messages[index]["deliveredMs"] = Value::from(now);
            messages[index]["recordedMs"] = Value::from(now);
            messages[index]["detail"] = Value::String(format!(
                "the late queued input is in the conversation's own items (userMessage {item_id}); whether the executor applied it is the executor's own report"
            ));
        }
        let Some(reply_to) = messages[index]
            .get("replyTo")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            let bytes = serde_json::to_vec_pretty(&value)?;
            let temp = receipt.with_extension(format!("{}.tmp", std::process::id()));
            fs::write(&temp, bytes)?;
            fs::rename(&temp, receipt)?;
            return Ok(());
        };
        let now = receipt_now_ms();
        if let Some(requests) = value.get_mut("leadMessages").and_then(Value::as_array_mut) {
            for request in requests {
                if request.get("id").and_then(Value::as_str) == Some(reply_to.as_str()) {
                    request["status"] = Value::String("resolved".to_owned());
                    request["resolvedMs"] = Value::from(now);
                    request["resolvedBy"] = Value::String(client_id.to_owned());
                    request["replyEvidence"] = Value::String(format!(
                        "observed as userMessage {item_id} in the executor conversation"
                    ));
                }
            }
        }
        let bytes = serde_json::to_vec_pretty(&value)?;
        let temp = receipt.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&temp, bytes)?;
        fs::rename(&temp, receipt)
    })
}

fn record_undelivered_input(receipt: &Path, id: &str, detail: &str) -> io::Result<()> {
    if id.is_empty() {
        return Ok(());
    }
    with_control_receipt_lock(receipt, || {
        let mut value =
            serde_json::from_slice::<Value>(&fs::read(receipt)?).map_err(io::Error::other)?;
        let messages = value
            .as_object_mut()
            .ok_or_else(|| io::Error::other("dispatch receipt is not an object"))?
            .entry("messages")
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(messages) = messages.as_array_mut() else {
            return Err(io::Error::other(
                "dispatch receipt messages field is not an array",
            ));
        };
        if messages
            .iter()
            .any(|message| message.get("id").and_then(Value::as_str) == Some(id))
        {
            return Ok(());
        }
        messages.push(json!({
            "schema": 1,
            "id": id,
            "status": "undelivered",
            "detail": detail,
        }));
        let bytes = serde_json::to_vec_pretty(&value).map_err(io::Error::other)?;
        let temp = receipt.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&temp, bytes)?;
        fs::rename(&temp, receipt)
    })
}

fn mark_input_closure(receipt: &Path) -> io::Result<()> {
    with_control_receipt_lock(receipt, || {
        let mut value =
            serde_json::from_slice::<Value>(&fs::read(receipt)?).map_err(io::Error::other)?;
        if let Some(object) = value.as_object_mut() {
            object.insert("inputClosure".to_owned(), Value::String("begun".to_owned()));
        }
        let bytes = serde_json::to_vec_pretty(&value).map_err(io::Error::other)?;
        let temp = receipt.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&temp, bytes)?;
        fs::rename(&temp, receipt)
    })
}

fn with_control_receipt_lock<T>(
    receipt: &Path,
    action: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let path = receipt.with_extension("lock");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let deadline = Deadline::after(RECEIPT_LOCK_WAIT)?;
    let _lock = ExclusiveFileLock::acquire(&path, deadline, &Cancellation::default())?;
    action()
}

/// `spawn-N.json` beside `endpoint-N.json`, the dispatch receipt this run owns.
fn receipt_beside_endpoint(endpoint: &Path) -> Option<PathBuf> {
    let name = endpoint.file_name()?.to_str()?;
    let spawn = name.replacen("endpoint-", "spawn-", 1);
    if spawn == name {
        return None;
    }
    Some(endpoint.with_file_name(spawn))
}

fn turn_id_of(params: &Value) -> Option<&str> {
    params
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
        .or_else(|| params.get("turnId").and_then(Value::as_str))
        .filter(|id| !id.is_empty())
}

fn item_turn_id(params: &Value) -> Option<&str> {
    let item = &params["item"];
    item.get("turnId")
        .and_then(Value::as_str)
        .or_else(|| item.get("turn_id").and_then(Value::as_str))
        .or_else(|| params.get("turnId").and_then(Value::as_str))
        .filter(|id| !id.is_empty())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn home(label: &str) -> PathBuf {
        let home =
            std::env::temp_dir().join(format!("lead-registry-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        home
    }

    /// The live identity of this test process, in the shape an endpoint record
    /// carries for the app-server it belongs to. The app-server itself is a
    /// child of this host, so a live process needs no second server here.
    fn live_process() -> EndpointProcess {
        let program = std::env::current_exe().unwrap();
        let user = harness_core::process_service::current_user().unwrap();
        let identity = harness_core::process_service::ServiceProcess::observe(
            std::process::id(),
            &program,
            0,
            &user,
        )
        .unwrap()
        .identity();
        EndpointProcess {
            pid: identity.pid,
            creation_time: identity.creation_time,
            program,
        }
    }

    fn record(thread: &str, process: &EndpointProcess) -> Value {
        json!({
            "schema": 1,
            "port": 51234,
            "token": "d".repeat(TOKEN_BYTES * 2),
            "threadId": thread,
            "process": {
                "pid": process.pid,
                "creationTime": process.creation_time,
                "program": process.program,
            },
        })
    }

    fn write_record(home: &Path, thread: &str, value: &Value) {
        let path = lead_endpoint_path(home, thread).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    #[test]
    fn a_registered_lead_endpoint_is_resolved_only_for_its_own_live_thread() {
        let home = home("live");
        let process = live_process();
        write_record(&home, "thread-live", &record("thread-live", &process));
        match registered_lead(&home, "thread-live") {
            RegisteredLead::Live(endpoint) => {
                assert_eq!(endpoint.thread_id.as_deref(), Some("thread-live"));
                assert_eq!(endpoint.port(), 51234);
            }
            RegisteredLead::Unavailable(cause) => panic!("a live record was refused: {cause}"),
        }
        // The same record is not an address of another conversation.
        match registered_lead(&home, "thread-other") {
            RegisteredLead::Live(_) => panic!("a record was resolved for another thread"),
            RegisteredLead::Unavailable(cause) => {
                assert!(cause.contains("no lead endpoint is registered"), "{cause}");
            }
        }
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn missing_malformed_mismatched_and_stale_lead_records_fail_closed() {
        let home = home("failures");
        match registered_lead(&home, "thread-missing") {
            RegisteredLead::Live(_) => panic!("a missing record was resolved"),
            RegisteredLead::Unavailable(cause) => {
                assert!(cause.contains("no lead endpoint is registered"), "{cause}");
            }
        }
        write_record(&home, "thread-malformed", &json!({"schema": 1}));
        match registered_lead(&home, "thread-malformed") {
            RegisteredLead::Live(_) => panic!("a malformed record was resolved"),
            RegisteredLead::Unavailable(cause) => {
                assert!(cause.contains("unreadable or malformed"), "{cause}");
                assert!(
                    !cause.contains(&"d".repeat(TOKEN_BYTES)),
                    "a refusal must not carry the record's bearer: {cause}"
                );
            }
        }
        let process = live_process();
        write_record(
            &home,
            "thread-mismatch",
            &record("thread-elsewhere", &process),
        );
        match registered_lead(&home, "thread-mismatch") {
            RegisteredLead::Live(_) => panic!("a record of another thread was resolved"),
            RegisteredLead::Unavailable(cause) => {
                assert!(cause.contains("another conversation"), "{cause}");
            }
        }
        let dead = EndpointProcess {
            pid: 1,
            creation_time: 1,
            program: process.program.clone(),
        };
        write_record(&home, "thread-stale", &record("thread-stale", &dead));
        match registered_lead(&home, "thread-stale") {
            RegisteredLead::Live(_) => panic!("a stale record was resolved"),
            RegisteredLead::Unavailable(cause) => {
                assert!(cause.contains("no longer live"), "{cause}");
            }
        }
        // A record with no process identity is state, not an address.
        write_record(
            &home,
            "thread-noprocess",
            &json!({
                "schema": 1,
                "port": 51234,
                "token": "d".repeat(TOKEN_BYTES * 2),
                "threadId": "thread-noprocess",
            }),
        );
        match registered_lead(&home, "thread-noprocess") {
            RegisteredLead::Live(_) => panic!("a record with no process was resolved"),
            RegisteredLead::Unavailable(cause) => {
                assert!(cause.contains("records no app-server process"), "{cause}");
            }
        }
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_lead_identity_that_cannot_key_a_record_is_never_an_address() {
        let home = home("keys");
        for thread in ["", "  ", ".", "..", "a/b", r"a\b", "thread:1"] {
            assert!(
                lead_endpoint_path(&home, thread).is_err(),
                "{thread:?} was accepted as a registry key"
            );
            assert!(matches!(
                registered_lead(&home, thread),
                RegisteredLead::Unavailable(_)
            ));
        }
        assert!(lead_endpoint_path(&home, &"t".repeat(129)).is_err());
        assert_eq!(
            lead_endpoint_path(&home, "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4").unwrap(),
            home.join("harness/lead-endpoints/01a0c719-f4d4-7880-a9d2-1a96ee0f23f4.json")
        );
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn publishing_and_retiring_a_lead_endpoint_leaves_no_address_behind() {
        let home = home("publish");
        let process = live_process();
        let path = lead_endpoint_path(&home, "thread-published").unwrap();
        write_record(
            &home,
            "thread-published",
            &record("thread-published", &process),
        );
        let endpoint = Endpoint::read(&path).unwrap();
        let published = publish_lead_endpoint(&home, &endpoint).unwrap();
        assert_eq!(published, path);
        // Publishing a conversation without a thread identity would put an
        // unaddressable record where every resolver looks for one.
        let anonymous = Endpoint::read(&path).unwrap();
        write_record(
            &home,
            "thread-anonymous",
            &record("thread-anonymous", &process),
        );
        let mut anonymous = anonymous;
        anonymous.thread_id = None;
        assert!(publish_lead_endpoint(&home, &anonymous).is_err());
        retire_lead_endpoint(&home, "thread-published").unwrap();
        assert!(!path.exists());
        // Retiring what is not there is the normal case, not a failure.
        retire_lead_endpoint(&home, "thread-published").unwrap();
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn a_lead_session_is_attached_to_its_own_checkout() {
        let checkout = PathBuf::from(r"D:\work\proj");
        assert!(inside_checkout(r"D:\work\proj", &checkout));
        assert!(inside_checkout(r"\\?\D:\work\proj", &checkout));
        assert!(inside_checkout(r"d:/work/proj/src", &checkout));
        assert!(!inside_checkout(r"D:\work\other", &checkout));
        assert!(!inside_checkout(r"D:\work\project", &checkout));
        // Host working files are not slot addresses: a registry lookup never
        // resolves one.
        let paths = ControlPaths::for_lead_host(Path::new(r"C:\home"), 4242);
        assert_eq!(
            paths.endpoint,
            PathBuf::from(r"C:\home\harness\lead-endpoints\host\4242.json")
        );
    }
}
