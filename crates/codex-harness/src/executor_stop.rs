//! Urgent stop of one exact pooled executor run.
//!
//! The command addresses one run through the accepted checkout, slot, owner and
//! optional exact session; verifies the recorded run host by its full process
//! identity (pid, creation time and image path - never a bare pid, program name
//! or window title); requests native interruption when the run recorded a
//! control endpoint; boundedly terminates the recorded host and the recorded
//! processes of its tree; verifies the recorded terminal surface closed; and
//! writes an honest stop record. It never resets, cleans, releases or completes
//! the run, and it never sends a terminal command: the tab of the addressed run
//! closes because that run's own host process ended.

use super::control;
use super::observation::{
    self, HostIdentity, RunObservation, STATE_COMPLETED, STATE_STOPPED, STOP_ALREADY_COMPLETED,
    STOP_ALREADY_STOPPED, STOP_ERROR, STOP_PARTIAL, STOP_SCHEMA, STOP_STOPPED, StopRecord,
    StopSurvivor, StopTransition,
};
use super::{
    SessionLease, invalid, lease_path, option_text, parse_seconds, read_lease, receipt_binding,
    receipt_path, required,
};
use harness_core::orchestration_config;
use harness_core::process::{Deadline, ProcessIdentity};
use harness_core::process_service::{self, ServiceProcess};
use harness_core::task_control::ControlConnection;
use harness_core::task_succession;
use harness_core::task_view;
use harness_core::task_worktree;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

/// Exit code requested from the terminated host; it is the stop's own request,
/// never recorded as the run's observed exit code.
const STOP_EXIT_CODE: u32 = 130;
/// Default bound on the whole stop path when `--timeout` is not given.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Bounded wait for an accepted native interruption to end the run by itself.
const INTERRUPT_GRACE: Duration = Duration::from_secs(5);
/// Bounded wait for recorded processes to end after the host was terminated.
const TREE_GRACE: Duration = Duration::from_secs(5);
/// Bounded wait for the recorded terminal surface to close after the host ended.
const SURFACE_GRACE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(100);
/// Bound on recorded descendant processes one stop tracks by identity.
const MEMBER_LIMIT: usize = 512;

struct Request {
    source: PathBuf,
    codex_home: PathBuf,
    slot: u32,
    owner: String,
    session: Option<String>,
    timeout: Duration,
}

impl Request {
    fn parse(args: &[OsString]) -> io::Result<Self> {
        let mut source = None;
        let mut codex_home = None;
        let mut slot = None;
        let mut owner = None;
        let mut session = None;
        let mut timeout = None;
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let key = arg
                .to_str()
                .ok_or_else(|| invalid("invalid native executor options"))?;
            let value = iter
                .next()
                .ok_or_else(|| invalid("invalid native executor options"))?;
            match key {
                "--source" => source = Some(PathBuf::from(value)),
                "--codex-home" => codex_home = Some(PathBuf::from(value)),
                "--slot" => slot = Some(option_text(value)?),
                "--owner" => owner = Some(option_text(value)?),
                "--session" => session = Some(option_text(value)?),
                "--timeout" => timeout = Some(option_text(value)?),
                _ => return Err(invalid("invalid native executor options")),
            }
        }
        let owner = owner
            .filter(|owner| !owner.trim().is_empty())
            .ok_or_else(|| invalid("--owner ID is required"))?;
        Ok(Self {
            source: required(source, "--source")?,
            codex_home: required(codex_home, "--codex-home")?,
            slot: slot
                .ok_or_else(|| invalid("--slot is required"))?
                .parse()
                .map_err(|_| invalid("--slot must be a positive pool slot index"))?,
            owner,
            session,
            timeout: parse_seconds(timeout.as_deref(), DEFAULT_TIMEOUT.as_secs(), "--timeout")?,
        })
    }
}

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    let mut request = Request::parse(args)?;
    let started = Instant::now();
    let requested_ms = observation::now_ms();
    // Address one recorded run: the slot must be bound to the addressed owner,
    // and an explicit --session must be the session the receipt recorded.
    let config = orchestration_config::load(&request.source)?;
    let layout = task_worktree::pool(&request.source, config.max_concurrent_executors)?;
    layout.slot(request.slot)?;
    let record =
        task_worktree::load_slot_record(&request.codex_home, &request.source, request.slot)?
            .ok_or_else(|| {
                invalid(&format!(
                    "slot {} has no recorded session binding; stop addresses a dispatched run, so dispatch or resume that session first",
                    request.slot
                ))
            })?;
    match record.owner.as_deref() {
        Some(owner) if owner == request.owner => {}
        recorded => {
            return Err(invalid(&format!(
                "slot {} is bound to session {} instead of {}; stop that owner's run or release the slot explicitly",
                request.slot,
                recorded.unwrap_or("no session"),
                request.owner
            )));
        }
    }
    let receipt = receipt_path(&request.codex_home, &request.source, request.slot)?;
    let bytes = fs::read(&receipt).map_err(|error| {
        invalid(&format!(
            "executor stop needs the dispatch receipt {} to name the exact run: {error}; dispatch `codex-harness executor spawn` first",
            receipt.display()
        ))
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        invalid(&format!(
            "dispatch receipt {} is not JSON: {error}",
            receipt.display()
        ))
    })?;
    if let Some(binding) = receipt_binding(&value)?
        && (binding.index != request.slot || binding.owner != request.owner)
    {
        return Err(invalid(&format!(
            "the recorded run in slot {} belongs to owner {} instead of {}; stop that run or release the slot explicitly",
            binding.index, binding.owner, request.owner
        )));
    }
    let recorded_run = RunObservation::from_receipt(&value);
    let recorded_session = recorded_run
        .as_ref()
        .and_then(RunObservation::recorded_session)
        .map(str::to_owned);
    if let Some(session) = &request.session {
        let exact = task_succession::exact_session_id(session)?;
        match recorded_session.as_deref() {
            Some(recorded) if recorded == exact => {}
            Some(recorded) => {
                return Err(invalid(&format!(
                    "the recorded run in slot {} observed session {recorded} instead of {exact}; stop the exact recorded run or address its recorded session",
                    request.slot
                )));
            }
            None => {
                return Err(invalid(&format!(
                    "the recorded run in slot {} names no exact session, so --session {exact} cannot be verified against it; stop without --session to address the recorded run by its process identity",
                    request.slot
                )));
            }
        }
    }
    request.session = request.session.or(recorded_session);
    let mut stop = StopRecord {
        schema: STOP_SCHEMA,
        outcome: STOP_ERROR.into(),
        requested_ms,
        completed_ms: 0,
        duration_ms: 0,
        detail: String::new(),
        host: None,
        interrupt: None,
        ended: 0,
        survivors: Vec::new(),
        surface: None,
        undelivered: Vec::new(),
        repeats: 0,
        repeated_ms: None,
        exit_code: None,
    };
    let state = recorded_run
        .as_ref()
        .map(|run| run.state.clone())
        .unwrap_or_default();
    // A completed run keeps its result and a stopped run reports its recorded
    // state; neither is acted on again.
    if state == STATE_COMPLETED {
        stop.outcome = STOP_ALREADY_COMPLETED.into();
        stop.host = recorded_run.as_ref().and_then(|run| run.host.clone());
        stop.detail = format!(
            "the recorded run already completed (result {})",
            locator(recorded_run.as_ref().and_then(|run| run.result.as_deref()))
        );
        return finish(&request, &receipt, stop, started, StopTransition::Keep);
    }
    if state == STATE_STOPPED {
        // Idempotent repeat: the first stop's outcome, timestamps and measured
        // duration stand, and the repeat is counted instead of re-acting.
        match previous_stop_record(&value) {
            Some(mut previous) => {
                previous.repeats += 1;
                previous.repeated_ms = Some(observation::now_ms());
                previous.detail = format!(
                    "the run is already stopped; the recorded stop stands unchanged ({})",
                    previous.detail
                );
                stop = previous;
            }
            None => {
                stop.host = recorded_run.as_ref().and_then(|run| run.host.clone());
                stop.detail = "the recorded run is already stopped".into();
            }
        }
        stop.outcome = STOP_STOPPED.into();
        return finish(&request, &receipt, stop, started, StopTransition::Keep);
    }
    // One live host, verified by its recorded identity. The dispatch receipt's
    // observation is authoritative; the lease must agree with it when both
    // exist, because a disagreement means the address no longer names one run.
    let lease = read_lease(&lease_path(
        &request.codex_home,
        &request.source,
        request.slot,
    )?)
    .filter(|lease| lease.owner == request.owner && lease.index == request.slot);
    let receipt_host = recorded_run.as_ref().and_then(|run| run.host.clone());
    if let (Some(receipt_host), Some(lease)) = (&receipt_host, &lease)
        && (lease.pid != receipt_host.pid
            || lease.created != receipt_host.created
            || lease.program != receipt_host.program)
    {
        stop.host = Some(receipt_host.clone());
        stop.detail = format!(
            "the recorded host identity disagrees between the dispatch receipt (pid {}) and the lease (pid {}); nothing was terminated because the address no longer names one exact run; next action: inspect the slot and the receipt, then continue or release explicitly",
            receipt_host.pid, lease.pid
        );
        return finish(&request, &receipt, stop, started, StopTransition::Keep);
    }
    let host = receipt_host
        .clone()
        .or_else(|| lease.as_ref().map(lease_identity));
    let Some(host) = host else {
        stop.detail = format!(
            "the receipt records no exact host process identity for slot {} ({}), so stop refused to terminate anything; next action: inspect the recorded run, continue it with `executor resume`, or release the slot explicitly",
            request.slot,
            if recorded_run.is_none() {
                "a legacy receipt without observation coverage"
            } else {
                "no host was observed"
            }
        );
        return finish(&request, &receipt, stop, started, StopTransition::Keep);
    };
    stop.host = Some(host.clone());
    let user = process_service::current_user()?;
    let verified = match ServiceProcess::inspect(
        ProcessIdentity {
            pid: host.pid,
            creation_time: host.created,
        },
        &host.program,
        &user,
    ) {
        Ok(Some(process)) => process,
        Ok(None) => {
            // The identity is stale: a reused pid, a missing image or an ended
            // host. Nothing may be terminated on that evidence.
            stop.detail = format!(
                "no live process matches the recorded host identity (pid {}, {}) for slot {}; this stop terminated nothing and the run ended without a terminal record, so its exact exit code is unknown; the host's own job reaped its process tree when it died; next action: inspect the slot and the receipt, then continue the session with `executor resume` or release the slot explicitly",
                host.pid,
                host.program.display(),
                request.slot
            );
            return finish(
                &request,
                &receipt,
                stop,
                started,
                StopTransition::Unobserved,
            );
        }
        Err(error) => {
            stop.detail = format!(
                "the recorded host process (pid {}, {}) could not be verified for slot {}: {error}; nothing was terminated, because an old process id, program name or window title is not sufficient identity; next action: inspect pid {} yourself or re-dispatch the run",
                host.pid,
                host.program.display(),
                request.slot,
                host.pid
            );
            return finish(&request, &receipt, stop, started, StopTransition::Keep);
        }
    };
    // Recorded processes of the run: the host's live descendants captured by
    // full identity before it ends, plus the app-server child the
    // control-backed host records in its kit-local endpoint file.
    let endpoint = read_endpoint(&receipt, request.slot);
    let mut members = tree::descendants(host.pid, &user);
    if let Some(identity) = endpoint
        .as_ref()
        .and_then(|endpoint| endpoint.process.clone())
        && !members
            .iter()
            .any(|member| member.identity.pid == identity.pid)
    {
        members.push(Member {
            program: identity.program,
            identity: ProcessIdentity {
                pid: identity.pid,
                creation_time: identity.created,
            },
        });
    }
    members.truncate(MEMBER_LIMIT);
    // Native interruption when the run has a control endpoint; the termination
    // below stays the guarantee, and an unreachable endpoint never hides it.
    let mut interrupted = false;
    if let Some(endpoint) = &endpoint {
        let budget = remaining(&started, request.timeout).min(Duration::from_secs(5));
        let (accepted, text) = interrupt(endpoint, budget);
        interrupted = accepted;
        stop.interrupt = Some(text);
        if accepted && endpoint.thread_id.is_some() {
            let until = Instant::now() + remaining(&started, request.timeout).min(INTERRUPT_GRACE);
            while Instant::now() < until && verified.is_running()? {
                thread::sleep(POLL);
            }
        }
    }
    let mut ended = 0u64;
    let mut survivors = survivors_of_previous_stop(&value, &user);
    if verified.is_running()? {
        if let Err(error) = verified.terminate(STOP_EXIT_CODE) {
            survivors.push(StopSurvivor {
                kind: "process".into(),
                pid: Some(host.pid),
                created: Some(host.created),
                image: Some(host.program.clone()),
                surface: None,
                cause: format!("the recorded host could not be terminated: {error}"),
                next_action: format!(
                    "re-run this stop, or terminate pid {} ({}) yourself using the recorded identity",
                    host.pid,
                    host.program.display()
                ),
            });
        }
        if verified
            .wait_for_exit(Deadline::after(remaining(&started, request.timeout))?)
            .unwrap_or(false)
        {
            ended += 1;
        } else if survivors
            .iter()
            .all(|survivor| survivor.pid != Some(host.pid))
        {
            survivors.push(StopSurvivor {
                kind: "process".into(),
                pid: Some(host.pid),
                created: Some(host.created),
                image: Some(host.program.clone()),
                surface: None,
                cause: format!(
                    "the recorded host did not end within the {}s stop bound",
                    request.timeout.as_secs()
                ),
                next_action: format!(
                    "re-run this stop with a longer --timeout, or terminate pid {} ({}) yourself using the recorded identity",
                    host.pid,
                    host.program.display()
                ),
            });
        }
    } else {
        ended += 1;
    }
    // Verify the recorded processes of the tree by identity and boundedly
    // terminate anything that survived the host's own job, so the stop reports
    // actual termination instead of assuming a child ended with the host.
    let tree_until = Instant::now() + remaining(&started, request.timeout).min(TREE_GRACE);
    let mut pending: Vec<Member> = members.clone();
    let mut unverified: Vec<(Member, String)> = Vec::new();
    loop {
        let mut still_live = Vec::new();
        for member in pending.drain(..) {
            match ServiceProcess::inspect(member.identity, &member.program, &user) {
                Ok(Some(process)) => {
                    let _ = process.terminate(STOP_EXIT_CODE);
                    still_live.push(member);
                }
                Ok(None) => ended += 1,
                Err(error) => unverified.push((member, error.to_string())),
            }
        }
        if still_live.is_empty() && unverified.is_empty() {
            break;
        }
        if Instant::now() >= tree_until {
            for member in still_live {
                survivors.push(member_survivor(
                    &member,
                    "the recorded process survived the host's end and the stop's bounded termination",
                    "re-run this stop with a longer --timeout, or terminate that exact process yourself",
                ));
            }
            for (member, cause) in unverified {
                survivors.push(member_survivor(
                    &member,
                    &format!(
                        "the recorded process could not be verified after the host ended: {cause}"
                    ),
                    "inspect that exact process yourself; it was not terminated on unverified identity",
                ));
            }
            break;
        }
        for (member, _) in unverified.drain(..) {
            pending.push(member);
        }
        pending.extend(still_live);
        thread::sleep(POLL);
    }
    // The tab of the addressed run closes because its own host process ended;
    // this verifies that recorded surface is gone and never touches a window.
    let (surface, surface_survivor) = verify_surface(
        &value,
        &host,
        &user,
        Instant::now() + remaining(&started, request.timeout).min(SURFACE_GRACE),
    );
    stop.surface = surface;
    stop.ended = ended;
    if let Some(survivor) = surface_survivor {
        survivors.push(survivor);
    }
    stop.survivors = survivors;
    stop.outcome = if stop.survivors.is_empty() {
        STOP_STOPPED.into()
    } else {
        STOP_PARTIAL.into()
    };
    let interruption = if interrupted {
        "the run accepted a native interruption through its control endpoint and "
    } else {
        ""
    };
    stop.detail = format!(
        "{interruption}the recorded host (pid {}, {}) was terminated and verified gone by its recorded identity; {} recorded process(es) verified gone; {} recorded resource(s) remain",
        host.pid,
        host.program.display(),
        stop.ended,
        stop.survivors.len()
    );
    let transition = if stop.survivors.is_empty() {
        StopTransition::Stopped
    } else {
        StopTransition::Partial
    };
    finish(&request, &receipt, stop, started, transition)
}

/// Commits the stop record under the receipt lock and prints the honest report.
fn finish(
    request: &Request,
    receipt: &Path,
    mut stop: StopRecord,
    started: Instant,
    transition: StopTransition,
) -> io::Result<i32> {
    // A repeated stop preserves the first stop's timestamps and measured
    // duration; only the original stop measured the stop path.
    if stop.repeats == 0 {
        stop.completed_ms = observation::now_ms();
        stop.duration_ms = started.elapsed().as_millis() as u64;
    }
    let commit = observation::commit_stop(receipt, &mut stop, transition)?;
    // A repeated stop reports the current state; the recorded first stop keeps
    // its own outcome and measured duration.
    let reported = if stop.repeats > 0 {
        STOP_ALREADY_STOPPED
    } else {
        commit.outcome.as_str()
    };
    println!(
        "executor stop: slot {} owner {} session {}: {} in {}ms",
        request.slot,
        request.owner,
        request.session.as_deref().unwrap_or("unrecorded"),
        reported,
        stop.duration_ms
    );
    if stop.repeats > 0 {
        println!(
            "repeat: {} repeated stop request(s) recorded; the original stop outcome, timestamps and measured duration stand, and this request took {}ms",
            stop.repeats,
            started.elapsed().as_millis()
        );
    }
    println!("{}", stop.detail);
    match (&stop.host, &stop.interrupt) {
        (Some(host), Some(interrupt)) => println!(
            "host: pid {} {} - {interrupt}",
            host.pid,
            host.program.display()
        ),
        (Some(host), None) => println!(
            "host: pid {} {} (no control endpoint was recorded; termination was the bounded action)",
            host.pid,
            host.program.display()
        ),
        (None, _) => {}
    }
    println!(
        "tree: {} recorded process(es) verified gone, {} recorded resource(s) remain",
        stop.ended,
        stop.survivors.len()
    );
    if let Some(surface) = &stop.surface {
        println!("terminal: {surface}");
    }
    if !stop.undelivered.is_empty() {
        println!(
            "messages: {} queued message(s) marked undelivered: {}",
            stop.undelivered.len(),
            stop.undelivered.join(", ")
        );
    }
    println!(
        "receipt: {} state: {}{}",
        receipt.display(),
        if commit.state.is_empty() {
            "unrecorded (legacy receipt)".to_owned()
        } else {
            commit.state.clone()
        },
        match commit.exit_code {
            Some(code) => format!(" observed exit code {code}"),
            None => " exit code unknown (never observed)".into(),
        }
    );
    for survivor in &stop.survivors {
        match (survivor.kind.as_str(), survivor.pid) {
            ("process", Some(pid)) => println!(
                "survivor: process pid {pid} {}: {}; next action: {}",
                survivor
                    .image
                    .as_deref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "image unrecorded".into()),
                survivor.cause,
                survivor.next_action
            ),
            _ => println!(
                "survivor: {} {}: {}; next action: {}",
                survivor.kind,
                survivor.surface.as_deref().unwrap_or("unrecorded surface"),
                survivor.cause,
                survivor.next_action
            ),
        }
    }
    println!(
        "preserved: slot {} stays bound to {}; changed and untracked files, results and partial work are untouched; no reset, clean, release or completion claim",
        request.slot, request.owner
    );
    println!(
        "continue: codex-harness executor resume --source {} --codex-home {} --slot {} --owner {}{}",
        request.source.display(),
        request.codex_home.display(),
        request.slot,
        request.owner,
        match &request.session {
            Some(session) => format!(" --session {session}"),
            None => String::new(),
        }
    );
    Ok(match commit.outcome.as_str() {
        STOP_PARTIAL => 2,
        STOP_ERROR => 1,
        STOP_STOPPED | STOP_ALREADY_STOPPED | STOP_ALREADY_COMPLETED => 0,
        _ => 1,
    })
}

fn locator(path: Option<&Path>) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| "locator unrecorded".into())
}

fn lease_identity(lease: &SessionLease) -> HostIdentity {
    HostIdentity {
        pid: lease.pid,
        created: lease.created,
        program: lease.program.clone(),
    }
}

fn member_survivor(member: &Member, cause: &str, next_action: &str) -> StopSurvivor {
    StopSurvivor {
        kind: "process".into(),
        pid: Some(member.identity.pid),
        created: Some(member.identity.creation_time),
        image: Some(member.program.clone()),
        surface: None,
        cause: cause.into(),
        next_action: format!(
            "{next_action} (pid {}, {})",
            member.identity.pid,
            member.program.display()
        ),
    }
}

/// What an earlier stop recorded, for a repeated stop that must report and
/// preserve the actual recorded state instead of claiming a new one.
fn previous_stop_record(value: &Value) -> Option<StopRecord> {
    serde_json::from_value::<StopRecord>(value.get("stop")?.clone()).ok()
}

/// Re-verifies the survivors an earlier partial stop named, so a repeated stop
/// reports the actual current state instead of guessing, and boundedly
/// terminates any that is still live by its recorded identity.
fn survivors_of_previous_stop(value: &Value, user: &str) -> Vec<StopSurvivor> {
    let Some(stop) = previous_stop_record(value) else {
        return Vec::new();
    };
    if stop.outcome != STOP_PARTIAL {
        return Vec::new();
    }
    let mut survivors = Vec::new();
    for survivor in stop.survivors {
        let (Some(pid), Some(created), Some(image)) =
            (survivor.pid, survivor.created, survivor.image.as_deref())
        else {
            survivors.push(survivor);
            continue;
        };
        match ServiceProcess::inspect(
            ProcessIdentity {
                pid,
                creation_time: created,
            },
            image,
            user,
        ) {
            Ok(None) => {}
            Ok(Some(process)) => {
                let _ = process.terminate(STOP_EXIT_CODE);
                survivors.push(survivor);
            }
            Err(_) => survivors.push(survivor),
        }
    }
    survivors
}

fn remaining(started: &Instant, timeout: Duration) -> Duration {
    timeout.saturating_sub(started.elapsed())
}

/// One recorded process of a run's tree.
#[derive(Clone)]
struct Member {
    identity: ProcessIdentity,
    program: PathBuf,
}

/// Kit-local control endpoint of one run, recorded beside the receipt by the
/// control-backed exec host: `endpoint-<index>.json` with the listening port,
/// the capability token, the thread id and, when recorded, the app-server
/// child's exact process identity.
struct Endpoint {
    port: u16,
    token: String,
    thread_id: Option<String>,
    process: Option<HostIdentity>,
}

fn read_endpoint(receipt: &Path, index: u32) -> Option<Endpoint> {
    let path = receipt.with_file_name(format!("endpoint-{index}.json"));
    let value: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let port = value["port"].as_u64()? as u16;
    let token = value["token"].as_str()?.to_owned();
    let thread_id = value["threadId"].as_str().map(str::to_owned);
    let process = match (
        value["process"]["pid"].as_u64(),
        value["process"]["creationTime"].as_u64(),
        value["process"]["program"].as_str(),
    ) {
        (Some(pid), Some(created), Some(program)) => Some(HostIdentity {
            pid: pid as u32,
            created,
            program: PathBuf::from(program),
        }),
        _ => None,
    };
    Some(Endpoint {
        port,
        token,
        thread_id,
        process,
    })
}

/// Request native interruption through the recorded control endpoint. Returns
/// whether the interrupt was accepted and the text recorded verbatim; an
/// unreachable endpoint never blocks or hides the bounded termination.
fn interrupt(endpoint: &Endpoint, budget: Duration) -> (bool, String) {
    let connect = ControlConnection::connect(endpoint.port, &endpoint.token, budget);
    let mut connection = match connect {
        Ok(connection) => connection,
        Err(error) => {
            return (
                false,
                format!(
                    "the recorded control endpoint on port {} did not accept a connection: {error}; the run was terminated directly",
                    endpoint.port
                ),
            );
        }
    };
    let initialize = json!({
        "clientInfo": {"name": "harness-stop", "version": "1"},
        "capabilities": {"experimentalApi": true}
    });
    if let Err(error) = control_call(&mut connection, 1, "initialize", initialize, budget) {
        return (
            false,
            format!(
                "the recorded control endpoint rejected initialize: {error}; the run was terminated directly"
            ),
        );
    }
    if connection
        .send(&json!({"method": "initialized"}), budget)
        .is_err()
    {
        return (
            false,
            "the recorded control endpoint closed before the interrupt was requested; the run was terminated directly".into(),
        );
    }
    let Some(thread_id) = &endpoint.thread_id else {
        return (
            false,
            "the recorded control endpoint names no thread id, so no native interruption could be requested; the run was terminated directly".into(),
        );
    };
    // `turn/interrupt` addresses the active turn by id; the thread's own
    // record is the authoritative source for it, never a guess from recency.
    let read = match control_call(
        &mut connection,
        2,
        "thread/read",
        json!({"threadId": thread_id, "includeTurns": true}),
        budget,
    ) {
        Ok(value) => value,
        Err(error) => {
            return (
                false,
                format!(
                    "the recorded control endpoint rejected thread/read while locating the active turn: {error}; the run was terminated directly"
                ),
            );
        }
    };
    let thread = &read["thread"];
    if thread["id"].as_str() != Some(thread_id.as_str()) {
        return (
            false,
            "thread/read answered for another thread while locating the active turn; the run was terminated directly".into(),
        );
    }
    let Some(turn) = control::active_turn(thread) else {
        return (
            false,
            "no native turn was in progress, so there was nothing to interrupt; the run was terminated directly".into(),
        );
    };
    match control_call(
        &mut connection,
        3,
        "turn/interrupt",
        json!({"threadId": thread_id, "turnId": turn}),
        budget,
    ) {
        Ok(_) => (
            true,
            "native turn/interrupt accepted through the recorded control endpoint".into(),
        ),
        Err(error) => (
            false,
            format!(
                "the recorded control endpoint rejected turn/interrupt: {error}; the run was terminated directly"
            ),
        ),
    }
}

fn control_call(
    connection: &mut ControlConnection,
    id: u64,
    method: &str,
    params: Value,
    budget: Duration,
) -> io::Result<Value> {
    connection.send(
        &json!({"id": id, "method": method, "params": params}),
        budget,
    )?;
    let until = Instant::now() + budget;
    loop {
        if Instant::now() >= until {
            return Err(io::Error::other(format!(
                "{method} was not answered within {}s",
                budget.as_secs().max(1)
            )));
        }
        if let Some(value) = connection.receive(Duration::from_millis(200))? {
            if value.get("method").is_some() || value["id"] != json!(id) {
                continue;
            }
            if value.get("error").is_some() {
                return Err(io::Error::other(format!(
                    "{method} was rejected: {}",
                    value["error"]
                )));
            }
            return Ok(value["result"].clone());
        }
    }
}

/// Verifies the recorded terminal surface of the addressed run is gone. The tab
/// closes because its own host process ended; nothing here sends a terminal
/// command, so no window, sibling tab or other conversation can be affected.
fn verify_surface(
    value: &Value,
    host: &HostIdentity,
    user: &str,
    until: Instant,
) -> (Option<String>, Option<StopSurvivor>) {
    match value["host"].as_str() {
        Some("windows-terminal-tab") => match recorded_tab(value) {
            Some((title, window)) => {
                let label = format!(
                    "tab \"{title}\" in window {}",
                    window.as_deref().unwrap_or("(most recently used)")
                );
                loop {
                    if !terminal_window_titled(&title) {
                        return (Some(format!("the recorded {label} closed")), None);
                    }
                    if Instant::now() >= until {
                        return (
                            None,
                            Some(StopSurvivor {
                                kind: "terminal-tab".into(),
                                pid: None,
                                created: None,
                                image: None,
                                surface: Some(label.clone()),
                                cause: format!(
                                    "the recorded {label} was still open after the host was verified stopped"
                                ),
                                next_action: format!(
                                    "close {label} yourself; the run's processes are stopped and its files are preserved"
                                ),
                            }),
                        );
                    }
                    thread::sleep(POLL);
                }
            }
            None => (
                Some(
                    "the receipt records no tab title, so the recorded tab could not be verified"
                        .into(),
                ),
                None,
            ),
        },
        Some("owned-console") => {
            let Ok(snapshot) =
                serde_json::from_value::<task_view::Snapshot>(value["window"].clone())
            else {
                return (
                    Some("the receipt records no console window snapshot to verify".into()),
                    None,
                );
            };
            let label = format!("console window {:#x}", snapshot.window);
            loop {
                if matches!(snapshot.is_visible(&host.program, user), Ok(false)) {
                    return (Some(format!("the recorded {label} closed")), None);
                }
                if Instant::now() >= until {
                    return (
                        None,
                        Some(StopSurvivor {
                            kind: "console-window".into(),
                            pid: None,
                            created: None,
                            image: None,
                            surface: Some(label.clone()),
                            cause: format!(
                                "the recorded {label} was still visible after the host was verified stopped"
                            ),
                            next_action: format!(
                                "close {label} yourself; the run's processes are stopped and its files are preserved"
                            ),
                        }),
                    );
                }
                thread::sleep(POLL);
            }
        }
        Some(host_kind) => (
            Some(format!(
                "the recorded host {host_kind} owns no terminal tab or console window to verify"
            )),
            None,
        ),
        None => (
            Some("the receipt records no terminal surface (legacy record)".into()),
            None,
        ),
    }
}

/// The recorded identity of one run's tab: the dispatch arguments carry the
/// title and the target window of exactly that tab.
fn recorded_tab(value: &Value) -> Option<(String, Option<String>)> {
    let args = value["terminal"].as_array()?;
    let mut title = None;
    let mut window = None;
    let mut iter = args.iter().filter_map(Value::as_str);
    while let Some(arg) = iter.next() {
        match arg {
            "--title" => title = iter.next().map(str::to_owned),
            "-w" | "--window" => window = iter.next().map(str::to_owned),
            _ => {}
        }
    }
    title.map(|title| (title, window))
}

/// True while any visible terminal window title still carries the tab title.
pub(super) fn terminal_window_titled(title: &str) -> bool {
    task_view::terminal_windows()
        .into_iter()
        .any(|window| task_view::window_title(window).contains(title))
}

/// Live descendants of one recorded host, captured by full identity so they can
/// be verified and boundedly terminated after the host ends.
mod tree {
    use super::{Member, ServiceProcess};
    use std::{ffi::c_void, path::PathBuf};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const PROCESS_ENTRY_LIMIT: usize = 20_000;
    const MAX_PATH: usize = 260;

    /// The kernel32 process snapshot is declared here because the crate's
    /// selected windows-sys features do not include ToolHelp; no new dependency
    /// is introduced and only the recorded host's own descendants are inspected.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ProcessEntry32W {
        dw_size: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; MAX_PATH],
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
    }

    /// A snapshot handle that is always closed.
    struct Snapshot(*mut c_void);

    impl Drop for Snapshot {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    /// `(pid, parent pid)` pairs of the running system snapshot, bounded.
    fn process_table() -> Vec<(u32, u32)> {
        let snapshot = Snapshot(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) });
        if snapshot.0.is_null() || snapshot.0 as isize == -1 {
            return Vec::new();
        }
        let mut entry: ProcessEntry32W = unsafe { std::mem::zeroed() };
        entry.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
        let mut table = Vec::new();
        let mut present = unsafe { Process32FirstW(snapshot.0, &mut entry) };
        while present != 0 && table.len() < PROCESS_ENTRY_LIMIT {
            table.push((entry.th32_process_id, entry.th32_parent_process_id));
            present = unsafe { Process32NextW(snapshot.0, &mut entry) };
        }
        table
    }

    /// The full image path of one live process, or `None` when it cannot be read
    /// (it exited, or access is denied); a candidate without a verifiable image
    /// is never tracked or terminated.
    fn image_path(pid: u32) -> Option<PathBuf> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut buffer = [0u16; 32 * 1024];
        let mut length = buffer.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
        unsafe {
            CloseHandle(handle);
        }
        if ok == 0 || length == 0 {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(
            &buffer[..length as usize],
        )))
    }

    /// Live descendants of `root`, each verified by the existing service-process
    /// identity check (pid, creation time, image and account) before it is
    /// recorded; the root itself is excluded.
    pub(super) fn descendants(root: u32, user: &str) -> Vec<Member> {
        let table = process_table();
        if table.is_empty() {
            return Vec::new();
        }
        let mut frontier = vec![root];
        let mut members = Vec::new();
        while let Some(parent) = frontier.pop() {
            for (pid, ppid) in &table {
                if *ppid != parent || *pid == root || frontier.contains(pid) {
                    continue;
                }
                frontier.push(*pid);
                if members.len() >= super::MEMBER_LIMIT {
                    continue;
                }
                let Some(program) = image_path(*pid) else {
                    continue;
                };
                if let Ok(process) = ServiceProcess::observe(*pid, &program, 0, user) {
                    members.push(Member {
                        identity: process.identity(),
                        program,
                    });
                }
            }
        }
        members
    }
}
