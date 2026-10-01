//! Optional heavy-command queue evidence.
//!
//! This is not a second journal, scheduler or model driver. A caller that asks
//! for it gets one JSON document per admission in a directory it already owns.
//! Correlation labels join that document to the existing attempt, tool-call and
//! command evidence. Missing, malformed and partial documents are unknown, never
//! a measured zero. This module does not subtract time or tokens and does not
//! decide whether a task was blocked or doing useful work.
//!
//! # Integration contract (`schema_version` 1)
//!
//! Trust [`interpret`], not a lone `measured_zero` field. [`QueueDelay`] is the
//! classification a later accounting owner may consume:
//!
//! - `MeasuredZero` is a fully observed immediate grant. It is not an absent file.
//! - `UnrelatedWait` is a completed wait whose observed holders were all proven
//!   to belong to other attempts, and whose identity did not change across the
//!   poll samples. It is not yet eligible to subtract: the accounting owner must
//!   still clip it to the attempt, intersect verified task-blocked intervals and
//!   remove useful overlap. `alignable: false` means the monotonic interval must
//!   not be placed on a wall-clock timeline.
//! - `SelfContention` is a wait with at least one holder proven to share this
//!   attempt. Do not treat it as external.
//! - `Inherited` is a nested lease. It has no queue interval and must not be
//!   unioned as a second episode. Link it through `parent_admission_id`.
//! - `FailedAdmission` is cancellation, timeout or failure before a usable
//!   grant. Do not turn it into a successful adjusted duration.
//! - `Unknown` covers a missing file, a malformed or partial document, an
//!   unmapped clock, a changed holder identity, or a holder that is not tagged.
//!   A missing attempt tag is unknown, never unrelated.
//!
//! Queue time ends at grant or at the terminal observation. `post_grant` is the
//! separate delay until the payload is resumed. `delay_ns: null` after start is
//! unknown, not zero. `poll_resolution_ns` is the admission poll; the recorded
//! interval can start late and end late by up to one poll. Do not shrink it here.
//!
//! `filetime` is 100-nanosecond ticks since 1601-01-01 UTC. Unix milliseconds are
//! `(filetime * 100 - 11_644_473_600_000_000_000) / 1_000_000`. Use that conversion
//! only when the clock mapping is paired. A jumped or unmapped clock must not be
//! placed on a rollout timeline.
//!
//! An unbound record (`bound_to_attempt: false`) does not prove anything about
//! an attempt, even when the record itself measured zero. Union episodes by
//! `admission_id`. A shared `command_id` is a hint, not an episode key.
//!
//! Public documents contain no account paths, command lines or foreign holder
//! text. Private holder records in the account directory remain the bounded
//! private evidence.
#![cfg(windows)]

use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};
use windows_sys::Win32::{
    Foundation::FILETIME, System::SystemInformation::GetSystemTimePreciseAsFileTime,
};

/// Document schema. A different value is unusable, not a measured zero.
pub const SCHEMA: &str = "codex-harness.heavy-queue-evidence.v1";
/// Contract version the accounting owner can pin.
pub const SCHEMA_VERSION: u32 = 1;
/// Owning producer. Not a separate monitoring service.
pub const PRODUCER: &str = "heavy-command";
/// Resource name recorded on every document.
pub const RESOURCE: &str = "heavy-command";
/// Caller-supplied evidence directory. Absent means ordinary operation.
pub const EVIDENCE_ENV: &str = "CODEX_HARNESS_HEAVY_QUEUE_EVIDENCE";
/// Optional attempt correlation. A missing value does not mean "unrelated".
pub const ATTEMPT_ENV: &str = "CODEX_HARNESS_HEAVY_ATTEMPT";
/// Optional tool-call correlation.
pub const TOOL_CALL_ENV: &str = "CODEX_HARNESS_HEAVY_TOOL_CALL";
/// Optional command correlation. Not unique per admission.
pub const COMMAND_ENV: &str = "CODEX_HARNESS_HEAVY_COMMAND";
/// Parent admission id set by an admitted parent. Honored only after the kernel
/// has already accepted the inherited lease.
pub const PARENT_ADMISSION_ENV: &str = "CODEX_HARNESS_HEAVY_PARENT_ADMISSION";
/// Stable stderr prefix. The remainder is a fixed reason, never a path.
pub const EVIDENCE_FAILURE_PREFIX: &str = "heavy: queue evidence was not recorded:";
const MAX_TOKEN: usize = 80;
const MAX_EVIDENCE_BYTES: u64 = 64 * 1024;
const JUMP_SLACK_NS: u64 = 1_000_000_000;
const JUMP_RATIO: u64 = 10;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryPerformanceCounter(lp_performance_count: *mut i64) -> i32;
    fn QueryPerformanceFrequency(lp_frequency: *mut i64) -> i32;
}

/// Labels that join this admission to existing attempt evidence.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correlation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
}

/// What the public entry point was asked to record. An empty observation is
/// ordinary operation and writes nothing.
#[derive(Clone, Debug, Default)]
pub struct Observation {
    pub directory: Option<std::path::PathBuf>,
    pub correlation: Correlation,
}

impl Observation {
    pub fn collecting(&self) -> bool {
        self.directory.is_some()
    }
}

/// One paired reading of the shared monotonic counter and the wall clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockSample {
    pub qpc: u64,
    pub filetime: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockMapping {
    Paired,
    Unmapped,
    Jumped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeKind {
    Queue,
    Inherited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalKind {
    ImmediateGrant,
    WaitedGrant,
    Inherited,
    Cancelled,
    Timeout,
    Failure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HolderClass {
    SameAttempt,
    OtherAttempt,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HolderReason {
    Tagged,
    Untagged,
    WaiterUntagged,
    Unreadable,
    LegacyLock,
}

/// Public holder classification. No pid, command line or path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolderFinding {
    pub classification: HolderClass,
    pub reason: HolderReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u32>,
}

/// A live holder tag already read by the admission owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveHolder {
    pub attempt_id: Option<String>,
    pub slot: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueBody {
    pub waited: bool,
    pub measured_ns: Option<u64>,
    pub measured_zero: bool,
    pub holders: Vec<HolderFinding>,
    pub holder_identity_stable: bool,
    pub holder_samples: u32,
    pub poll_resolution_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostGrantBody {
    pub started: Option<bool>,
    pub delay_ns: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockBody {
    pub source: String,
    pub frequency: u64,
    pub start: Option<ClockSample>,
    pub end: Option<ClockSample>,
    pub mapping: ClockMapping,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageBody {
    pub queue_boundary: String,
    pub gaps: Vec<String>,
}

/// One admission document. Fields are public so a consumer can audit them;
/// classification goes through [`interpret`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueEvidence {
    pub schema: String,
    pub schema_version: u32,
    pub producer: String,
    pub admission_id: String,
    pub parent_admission_id: Option<String>,
    pub resource: String,
    pub correlation: Correlation,
    pub episode: EpisodeKind,
    pub terminal: TerminalKind,
    pub failure: Option<String>,
    pub admitted_at: Option<ClockSample>,
    pub clock: ClockBody,
    pub queue: Option<QueueBody>,
    pub post_grant: PostGrantBody,
    pub coverage: CoverageBody,
}

/// What a reader could load. Absence is explicit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceView {
    Absent,
    Malformed { reason: &'static str },
    Record(Box<QueueEvidence>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitClass {
    External,
    SelfContention,
    UnknownIdentity,
    Unstable,
}

/// Classification for the accounting owner. This is not a deduction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueDelay {
    MeasuredZero,
    UnrelatedWait {
        monotonic_ns: u64,
        poll_resolution_ns: u64,
        alignable: bool,
    },
    SelfContention {
        monotonic_ns: u64,
    },
    Inherited,
    FailedAdmission {
        terminal: TerminalKind,
        monotonic_ns: Option<u64>,
    },
    Unknown {
        reason: &'static str,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostGrant {
    Unknown,
    NotStarted,
    Started { delay_ns: Option<u64> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interpreted {
    pub admission_id: Option<String>,
    pub parent_admission_id: Option<String>,
    pub bound_to_attempt: bool,
    pub delay: QueueDelay,
    pub post_grant: PostGrant,
    pub correlation: Correlation,
}

/// Inputs the admission owner has actually observed. The module decides which
/// numbers are measured.
#[derive(Clone, Debug)]
pub struct EpisodeDraft {
    pub admission_id: String,
    pub parent_admission_id: Option<String>,
    pub correlation: Correlation,
    pub episode: EpisodeKind,
    pub terminal: TerminalKind,
    pub failure: Option<String>,
    pub frequency: u64,
    pub admitted_at: Option<ClockSample>,
    pub queue_start: Option<ClockSample>,
    pub queue_end: Option<ClockSample>,
    pub waited: bool,
    pub holders: Vec<HolderFinding>,
    pub holder_samples: u32,
    pub holder_changed: bool,
    pub poll_resolution_ns: u64,
    pub payload_started: Option<bool>,
}

/// Rejects path-like and unbounded labels before they can enter a document.
pub fn validate_token(value: &str) -> Result<String, &'static str> {
    if value.is_empty() || value.len() > MAX_TOKEN {
        return Err("correlation label must be 1..=80 ascii bytes");
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err("correlation label must be an ascii token without paths");
    }
    Ok(value.to_owned())
}

/// Prints a fixed evidence failure. The command's own result stays unchanged.
pub fn expose(reason: &'static str) {
    eprintln!("{EVIDENCE_FAILURE_PREFIX} {reason}");
}

/// Maps a write error to a fixed reason. The OS message is not printed: it can
/// contain a path.
pub fn expose_write(error: &io::Error) {
    let reason = match error.kind() {
        io::ErrorKind::NotFound => "evidence directory is missing",
        io::ErrorKind::PermissionDenied => "evidence directory is not writable",
        io::ErrorKind::InvalidInput | io::ErrorKind::NotADirectory => {
            "evidence directory is not a directory"
        }
        _ => "evidence document could not be written",
    };
    expose(reason);
}

pub fn failure_token(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::Interrupted => "cancelled",
        io::ErrorKind::TimedOut => "timed_out",
        io::ErrorKind::InvalidData => "invalid_lock",
        io::ErrorKind::InvalidInput => "invalid_input",
        io::ErrorKind::NotFound => "not_found",
        _ => "io",
    }
}

pub fn terminal_of(error: &io::Error) -> TerminalKind {
    match error.kind() {
        io::ErrorKind::Interrupted => TerminalKind::Cancelled,
        io::ErrorKind::TimedOut => TerminalKind::Timeout,
        _ => TerminalKind::Failure,
    }
}

/// Shared monotonic counter plus a wall-clock sample taken in the same observation.
pub fn sample_clock() -> io::Result<(u64, ClockSample)> {
    let mut frequency = 0i64;
    let mut counter = 0i64;
    let mut filetime = FILETIME::default();
    let ok = unsafe {
        QueryPerformanceFrequency(&mut frequency) != 0
            && frequency > 0
            && QueryPerformanceCounter(&mut counter) != 0
            && counter >= 0
    };
    if !ok {
        return Err(io::Error::other(
            "the shared performance counter could not be read",
        ));
    }
    unsafe { GetSystemTimePreciseAsFileTime(&mut filetime) };
    let filetime = (u64::from(filetime.dwHighDateTime) << 32) | u64::from(filetime.dwLowDateTime);
    Ok((
        frequency as u64,
        ClockSample {
            qpc: counter as u64,
            filetime,
        },
    ))
}

pub fn classify_holders(
    waiter: Option<&str>,
    live: &[LiveHolder],
    saw_unreadable: bool,
) -> Vec<HolderFinding> {
    if live.is_empty() {
        return vec![HolderFinding {
            classification: HolderClass::Unknown,
            reason: if saw_unreadable {
                HolderReason::Unreadable
            } else {
                HolderReason::LegacyLock
            },
            slot: None,
        }];
    }
    live.iter()
        .map(|holder| classify_one(waiter, holder))
        .collect()
}

fn classify_one(waiter: Option<&str>, holder: &LiveHolder) -> HolderFinding {
    let (classification, reason) = match (waiter, holder.attempt_id.as_deref()) {
        (Some(waiter), Some(held)) if waiter == held => {
            (HolderClass::SameAttempt, HolderReason::Tagged)
        }
        (Some(_), Some(_)) => (HolderClass::OtherAttempt, HolderReason::Tagged),
        (None, Some(_)) => (HolderClass::Unknown, HolderReason::WaiterUntagged),
        (_, None) => (HolderClass::Unknown, HolderReason::Untagged),
    };
    HolderFinding {
        classification,
        reason,
        slot: holder.slot,
    }
}

pub fn write_episode(directory: &Path, draft: &EpisodeDraft) -> io::Result<std::path::PathBuf> {
    let admission_id = validate_token(&draft.admission_id)
        .map_err(|_| io::Error::other("admission identity is not a token"))?;
    if let Some(parent) = &draft.parent_admission_id {
        validate_token(parent)
            .map_err(|_| io::Error::other("parent admission identity is not a token"))?;
    }
    let metadata = fs::metadata(directory).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => error,
        _ => io::Error::new(
            io::ErrorKind::InvalidInput,
            "evidence directory is not usable",
        ),
    })?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "evidence directory is not a directory",
        ));
    }
    let document = document_from(draft);
    let path = directory.join(format!("{admission_id}.json"));
    let temporary = directory.join(format!("{admission_id}.json.tmp"));
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|_| io::Error::other("evidence document could not be serialized"))?;
    fs::write(&temporary, bytes)?;
    if path.exists() {
        let _ = fs::remove_file(&temporary);
        return Err(io::Error::other(
            "evidence document already exists for this admission",
        ));
    }
    replace_file(&temporary, &path)?;
    Ok(path)
}

pub fn note_command_started(directory: &Path, admission_id: &str) -> io::Result<()> {
    update_post_grant(directory, admission_id, true)
}

pub fn note_command_not_started(directory: &Path, admission_id: &str) -> io::Result<()> {
    update_post_grant(directory, admission_id, false)
}

fn update_post_grant(directory: &Path, admission_id: &str, started: bool) -> io::Result<()> {
    let admission_id = validate_token(admission_id)
        .map_err(|_| io::Error::other("admission identity is not a token"))?;
    let path = directory.join(format!("{admission_id}.json"));
    let mut document = match read_evidence(&path) {
        EvidenceView::Record(document) => document,
        EvidenceView::Absent => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "evidence document is missing",
            ));
        }
        EvidenceView::Malformed { .. } => {
            return Err(io::Error::other("evidence document could not be read back"));
        }
    };
    document.post_grant.started = Some(started);
    document.post_grant.delay_ns = None;
    if started
        && let (Some(admitted), Ok((frequency, now))) = (document.admitted_at, sample_clock())
        && frequency == document.clock.frequency
        && now.qpc >= admitted.qpc
    {
        document.post_grant.delay_ns = monotonic_ns(frequency, admitted.qpc, now.qpc);
    }
    document.coverage.gaps = gap_names(&document);
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|_| io::Error::other("evidence document could not be serialized"))?;
    let temporary = directory.join(format!("{admission_id}.json.tmp"));
    fs::write(&temporary, bytes)?;
    replace_file(&temporary, &path)
}

/// Windows `rename` does not replace an existing file. Move the previous
/// document aside and restore it if the new name cannot be published.
fn replace_file(temporary: &Path, path: &Path) -> io::Result<()> {
    if !path.exists() {
        return fs::rename(temporary, path).inspect_err(|_| {
            let _ = fs::remove_file(temporary);
        });
    }
    let backup = path.with_extension("json.bak");
    fs::rename(path, &backup)?;
    if let Err(error) = fs::rename(temporary, path) {
        let _ = fs::rename(&backup, path);
        let _ = fs::remove_file(temporary);
        return Err(error);
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

pub fn read_evidence(path: &Path) -> EvidenceView {
    if !path.exists() {
        return EvidenceView::Absent;
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => {
            return EvidenceView::Malformed {
                reason: "not_a_regular_file",
            };
        }
        Err(_) => {
            return EvidenceView::Malformed {
                reason: "unreadable",
            };
        }
    };
    if metadata.len() > MAX_EVIDENCE_BYTES {
        return EvidenceView::Malformed {
            reason: "oversized",
        };
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return EvidenceView::Malformed {
                reason: "unreadable",
            };
        }
    };
    let document: QueueEvidence = match serde_json::from_slice(&bytes) {
        Ok(document) => document,
        Err(_) => {
            return EvidenceView::Malformed {
                reason: "malformed_json",
            };
        }
    };
    if document.schema != SCHEMA
        || document.schema_version != SCHEMA_VERSION
        || document.producer != PRODUCER
        || document.resource != RESOURCE
        || validate_token(&document.admission_id).is_err()
    {
        return EvidenceView::Malformed {
            reason: "unsupported_schema",
        };
    }
    if document
        .parent_admission_id
        .as_deref()
        .is_some_and(|parent| validate_token(parent).is_err())
    {
        return EvidenceView::Malformed {
            reason: "unsupported_schema",
        };
    }
    EvidenceView::Record(Box::new(document))
}

pub fn read_directory(directory: &Path) -> io::Result<Vec<EvidenceView>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.ends_with(".json") || name.ends_with(".tmp") {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path()).ok();
        if metadata.as_ref().is_some_and(|metadata| metadata.is_file()) {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths.into_iter().map(|path| read_evidence(&path)).collect())
}

pub fn interpret(view: &EvidenceView) -> Interpreted {
    let EvidenceView::Record(document) = view else {
        return Interpreted {
            admission_id: None,
            parent_admission_id: None,
            bound_to_attempt: false,
            delay: QueueDelay::Unknown {
                reason: match view {
                    EvidenceView::Absent => "missing_trace",
                    EvidenceView::Malformed { reason } => reason,
                    EvidenceView::Record(_) => "missing_trace",
                },
            },
            post_grant: PostGrant::Unknown,
            correlation: Correlation::default(),
        };
    };
    Interpreted {
        admission_id: Some(document.admission_id.clone()),
        parent_admission_id: document.parent_admission_id.clone(),
        bound_to_attempt: document.correlation.attempt_id.is_some(),
        delay: delay_of(document),
        post_grant: post_grant_of(document),
        correlation: document.correlation.clone(),
    }
}

fn delay_of(document: &QueueEvidence) -> QueueDelay {
    if document.episode == EpisodeKind::Inherited {
        if document.queue.is_some() || document.terminal != TerminalKind::Inherited {
            return QueueDelay::Unknown {
                reason: "inconsistent_inheritance",
            };
        }
        return QueueDelay::Inherited;
    }
    let Some(queue) = &document.queue else {
        return QueueDelay::Unknown {
            reason: "queue_boundary_incomplete",
        };
    };
    if queue.measured_zero != recomputed_zero(document, queue) {
        return QueueDelay::Unknown {
            reason: "inconsistent_queue",
        };
    }
    if queue.measured_ns != recomputed_ns(document, queue) {
        return QueueDelay::Unknown {
            reason: "inconsistent_queue",
        };
    }
    match document.terminal {
        TerminalKind::ImmediateGrant if !queue.waited && queue.measured_zero => {
            if document.clock.mapping != ClockMapping::Paired {
                return QueueDelay::Unknown {
                    reason: "clock_unmapped",
                };
            }
            QueueDelay::MeasuredZero
        }
        TerminalKind::ImmediateGrant if !queue.waited => QueueDelay::Unknown {
            reason: "clock_unmapped",
        },
        TerminalKind::WaitedGrant if queue.waited => wait_class(document, queue),
        TerminalKind::Cancelled | TerminalKind::Timeout | TerminalKind::Failure => {
            QueueDelay::FailedAdmission {
                terminal: document.terminal,
                monotonic_ns: queue.measured_ns,
            }
        }
        _ => QueueDelay::Unknown {
            reason: "inconsistent_queue",
        },
    }
}

fn wait_class(document: &QueueEvidence, queue: &QueueBody) -> QueueDelay {
    let Some(ns) = queue.measured_ns else {
        return QueueDelay::Unknown {
            reason: "queue_boundary_incomplete",
        };
    };
    if !queue.holder_identity_stable {
        return QueueDelay::Unknown {
            reason: "holder_identity_changed",
        };
    }
    if queue
        .holders
        .iter()
        .any(|holder| holder.classification == HolderClass::SameAttempt)
    {
        return QueueDelay::SelfContention { monotonic_ns: ns };
    }
    if queue.holders.is_empty()
        || queue
            .holders
            .iter()
            .any(|holder| holder.classification == HolderClass::Unknown)
    {
        return QueueDelay::Unknown {
            reason: "holder_identity_unknown",
        };
    }
    QueueDelay::UnrelatedWait {
        monotonic_ns: ns,
        poll_resolution_ns: queue.poll_resolution_ns,
        alignable: document.clock.mapping == ClockMapping::Paired,
    }
}

fn post_grant_of(document: &QueueEvidence) -> PostGrant {
    match document.post_grant.started {
        None => PostGrant::Unknown,
        Some(false) => PostGrant::NotStarted,
        Some(true) => PostGrant::Started {
            delay_ns: document.post_grant.delay_ns,
        },
    }
}

fn document_from(draft: &EpisodeDraft) -> QueueEvidence {
    let inherited = draft.episode == EpisodeKind::Inherited;
    let (start, end) = if inherited {
        (draft.admitted_at, draft.admitted_at)
    } else if draft.waited {
        (draft.queue_start, draft.queue_end.or(draft.admitted_at))
    } else if draft.terminal == TerminalKind::ImmediateGrant {
        (draft.admitted_at, draft.admitted_at)
    } else {
        (draft.queue_start, draft.queue_end.or(draft.admitted_at))
    };
    let mapping = match (start, end, draft.frequency) {
        (Some(start), Some(end), frequency) if frequency > 0 => mapping_of(frequency, start, end),
        _ => ClockMapping::Unmapped,
    };
    let queue = if inherited {
        None
    } else {
        let measured_ns = measured_interval(draft, start, end, mapping);
        let measured_zero = draft.terminal == TerminalKind::ImmediateGrant
            && !draft.waited
            && measured_ns == Some(0)
            && mapping == ClockMapping::Paired;
        Some(QueueBody {
            waited: draft.waited,
            measured_ns,
            measured_zero,
            holders: draft.holders.clone(),
            holder_identity_stable: !draft.holder_changed,
            holder_samples: draft.holder_samples,
            poll_resolution_ns: draft.poll_resolution_ns,
        })
    };
    let boundary = if inherited {
        "not_applicable"
    } else if queue
        .as_ref()
        .is_some_and(|queue| queue.measured_ns.is_some())
    {
        "complete"
    } else {
        "incomplete"
    };
    let mut document = QueueEvidence {
        schema: SCHEMA.to_owned(),
        schema_version: SCHEMA_VERSION,
        producer: PRODUCER.to_owned(),
        admission_id: draft.admission_id.clone(),
        parent_admission_id: draft.parent_admission_id.clone(),
        resource: RESOURCE.to_owned(),
        correlation: draft.correlation.clone(),
        episode: draft.episode,
        terminal: draft.terminal,
        failure: draft.failure.clone(),
        admitted_at: draft.admitted_at,
        clock: ClockBody {
            source: "qpc+filetime".to_owned(),
            frequency: draft.frequency,
            start,
            end,
            mapping,
        },
        queue,
        post_grant: PostGrantBody {
            started: draft.payload_started,
            delay_ns: None,
        },
        coverage: CoverageBody {
            queue_boundary: boundary.to_owned(),
            gaps: Vec::new(),
        },
    };
    document.coverage.gaps = gap_names(&document);
    document
}

fn measured_interval(
    draft: &EpisodeDraft,
    start: Option<ClockSample>,
    end: Option<ClockSample>,
    mapping: ClockMapping,
) -> Option<u64> {
    if draft.terminal == TerminalKind::ImmediateGrant && !draft.waited {
        return (mapping == ClockMapping::Paired).then_some(0);
    }
    if !draft.waited {
        return None;
    }
    let (Some(start), Some(end)) = (start, end) else {
        return None;
    };
    if mapping == ClockMapping::Unmapped || draft.frequency == 0 || end.qpc < start.qpc {
        return None;
    }
    monotonic_ns(draft.frequency, start.qpc, end.qpc)
}

fn clocks_are_equal(document: &QueueEvidence) -> bool {
    matches!(
        (document.clock.start, document.clock.end),
        (Some(start), Some(end)) if start.qpc == end.qpc
    )
}

fn recomputed_zero(document: &QueueEvidence, queue: &QueueBody) -> bool {
    document.terminal == TerminalKind::ImmediateGrant
        && !queue.waited
        && document.clock.mapping == ClockMapping::Paired
        && clocks_are_equal(document)
}

fn recomputed_ns(document: &QueueEvidence, queue: &QueueBody) -> Option<u64> {
    if !queue.waited {
        return recomputed_zero(document, queue).then_some(0);
    }
    let (Some(start), Some(end)) = (document.clock.start, document.clock.end) else {
        return None;
    };
    if document.clock.mapping == ClockMapping::Unmapped
        || document.clock.frequency == 0
        || end.qpc < start.qpc
    {
        return None;
    }
    monotonic_ns(document.clock.frequency, start.qpc, end.qpc)
}

fn mapping_of(frequency: u64, start: ClockSample, end: ClockSample) -> ClockMapping {
    if end.qpc < start.qpc || end.filetime < start.filetime {
        return ClockMapping::Unmapped;
    }
    let Some(qpc_ns) = monotonic_ns(frequency, start.qpc, end.qpc) else {
        return ClockMapping::Unmapped;
    };
    let filetime_ns = end
        .filetime
        .saturating_sub(start.filetime)
        .saturating_mul(100);
    let slack = JUMP_SLACK_NS.max(qpc_ns / JUMP_RATIO);
    if qpc_ns.abs_diff(filetime_ns) > slack {
        ClockMapping::Jumped
    } else {
        ClockMapping::Paired
    }
}

fn monotonic_ns(frequency: u64, start: u64, end: u64) -> Option<u64> {
    if frequency == 0 || end < start {
        return None;
    }
    Some(end.saturating_sub(start).saturating_mul(1_000_000_000) / frequency)
}

fn gap_names(document: &QueueEvidence) -> Vec<String> {
    let mut gaps = Vec::new();
    if document.correlation.attempt_id.is_none() {
        gaps.push("missing_attempt".to_owned());
    }
    if document.correlation.tool_call_id.is_none() {
        gaps.push("missing_tool_call".to_owned());
    }
    if document.correlation.command_id.is_none() {
        gaps.push("missing_command".to_owned());
    }
    if document.episode == EpisodeKind::Inherited && document.parent_admission_id.is_none() {
        gaps.push("parent_unlinked".to_owned());
    }
    match document.clock.mapping {
        ClockMapping::Unmapped => gaps.push("clock_unmapped".to_owned()),
        ClockMapping::Jumped => gaps.push("clock_jumped".to_owned()),
        ClockMapping::Paired => {}
    }
    if document.episode == EpisodeKind::Queue
        && !document
            .queue
            .as_ref()
            .is_some_and(|queue| queue.measured_ns.is_some())
    {
        gaps.push("queue_boundary_incomplete".to_owned());
    }
    if document
        .queue
        .as_ref()
        .is_some_and(|queue| !queue.holder_identity_stable)
    {
        gaps.push("holder_identity_changed".to_owned());
    }
    if document.queue.as_ref().is_some_and(|queue| {
        queue.waited
            && queue
                .holders
                .iter()
                .any(|holder| holder.classification == HolderClass::Unknown)
    }) {
        gaps.push("holder_identity_unknown".to_owned());
    }
    match document.post_grant.started {
        None => gaps.push("post_grant_unknown".to_owned()),
        Some(true) if document.post_grant.delay_ns.is_none() => {
            gaps.push("post_grant_unmapped".to_owned());
        }
        _ => {}
    }
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(qpc: u64, filetime: u64) -> ClockSample {
        ClockSample { qpc, filetime }
    }

    fn draft(terminal: TerminalKind, waited: bool) -> EpisodeDraft {
        let at = sample(1_000, 10_000_000_000);
        EpisodeDraft {
            admission_id: "admission-1".to_owned(),
            parent_admission_id: None,
            correlation: Correlation {
                attempt_id: Some("attempt-a".to_owned()),
                tool_call_id: Some("tool-1".to_owned()),
                command_id: Some("cmd-1".to_owned()),
            },
            episode: EpisodeKind::Queue,
            terminal,
            failure: None,
            frequency: 1_000_000_000,
            admitted_at: Some(at),
            queue_start: waited.then_some(sample(1_000, 10_000_000_000)),
            queue_end: waited.then_some(sample(1_500_000_000, 11_500_000_000)),
            waited,
            holders: Vec::new(),
            holder_samples: u32::from(waited),
            holder_changed: false,
            poll_resolution_ns: 20_000_000,
            payload_started: Some(false),
        }
    }

    #[test]
    fn missing_and_malformed_traces_are_unknown_not_zero() {
        let missing = interpret(&read_evidence(Path::new("does-not-exist-heavy-queue.json")));
        assert!(matches!(
            missing.delay,
            QueueDelay::Unknown {
                reason: "missing_trace"
            }
        ));
        assert!(!matches!(missing.delay, QueueDelay::MeasuredZero));

        let root = tempfile::tempdir().unwrap();
        let partial = root.path().join("partial.json");
        fs::write(&partial, b"{").unwrap();
        let malformed = interpret(&read_evidence(&partial));
        assert!(matches!(
            malformed.delay,
            QueueDelay::Unknown {
                reason: "malformed_json"
            }
        ));
        fs::write(&partial, b"{\"schema\":\"other\"}").unwrap();
        assert!(matches!(
            interpret(&read_evidence(&partial)).delay,
            QueueDelay::Unknown {
                reason: "malformed_json" | "unsupported_schema"
            }
        ));
    }

    #[test]
    fn immediate_grant_is_measured_zero_and_a_wait_is_not() {
        let root = tempfile::tempdir().unwrap();
        let path = write_episode(root.path(), &draft(TerminalKind::ImmediateGrant, false)).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains('\\') && !text.contains("command="), "{text}");
        let interpreted = interpret(&read_evidence(&path));
        assert_eq!(interpreted.delay, QueueDelay::MeasuredZero);
        assert!(interpreted.bound_to_attempt);

        let mut waited = draft(TerminalKind::WaitedGrant, true);
        waited.admission_id = "admission-2".to_owned();
        waited.queue_start = Some(sample(1_000_000_000, 10_000_000_000));
        waited.queue_end = Some(sample(1_500_000_000, 10_005_000_000));
        waited.admitted_at = waited.queue_end;
        waited.holders = vec![HolderFinding {
            classification: HolderClass::OtherAttempt,
            reason: HolderReason::Tagged,
            slot: Some(0),
        }];
        let path = write_episode(root.path(), &waited).unwrap();
        match interpret(&read_evidence(&path)).delay {
            QueueDelay::UnrelatedWait {
                monotonic_ns,
                alignable,
                ..
            } => {
                assert_eq!(monotonic_ns, 500_000_000);
                assert!(alignable);
            }
            other => panic!("expected an unrelated wait, got {other:?}"),
        }
    }

    #[test]
    fn holder_identity_distinguishes_same_other_and_unknown() {
        let same = classify_holders(
            Some("attempt-a"),
            &[LiveHolder {
                attempt_id: Some("attempt-a".to_owned()),
                slot: None,
            }],
            false,
        );
        assert_eq!(same[0].classification, HolderClass::SameAttempt);
        let other = classify_holders(
            Some("attempt-a"),
            &[LiveHolder {
                attempt_id: Some("attempt-b".to_owned()),
                slot: None,
            }],
            false,
        );
        assert_eq!(other[0].classification, HolderClass::OtherAttempt);
        let untagged = classify_holders(
            Some("attempt-a"),
            &[LiveHolder {
                attempt_id: None,
                slot: None,
            }],
            false,
        );
        assert_eq!(untagged[0].classification, HolderClass::Unknown);
        assert_eq!(untagged[0].reason, HolderReason::Untagged);
        let waiter_untagged = classify_holders(
            None,
            &[LiveHolder {
                attempt_id: Some("attempt-b".to_owned()),
                slot: None,
            }],
            false,
        );
        assert_eq!(waiter_untagged[0].classification, HolderClass::Unknown);
        assert_ne!(waiter_untagged[0].classification, HolderClass::OtherAttempt);
        let legacy = classify_holders(Some("attempt-a"), &[], false);
        assert_eq!(legacy[0].reason, HolderReason::LegacyLock);
    }

    #[test]
    fn inherited_record_has_no_queue_episode_and_a_jump_is_not_alignable() {
        let root = tempfile::tempdir().unwrap();
        let mut inherited = draft(TerminalKind::Inherited, false);
        inherited.episode = EpisodeKind::Inherited;
        inherited.parent_admission_id = Some("parent-1".to_owned());
        inherited.queue_start = None;
        inherited.queue_end = None;
        let path = write_episode(root.path(), &inherited).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"queue\": null"), "{text}");
        assert_eq!(
            interpret(&read_evidence(&path)).delay,
            QueueDelay::Inherited
        );

        let mut jumped = draft(TerminalKind::WaitedGrant, true);
        jumped.admission_id = "admission-jump".to_owned();
        jumped.queue_end = Some(sample(2_000_000_000, 10_000_000_100));
        jumped.holders = vec![HolderFinding {
            classification: HolderClass::OtherAttempt,
            reason: HolderReason::Tagged,
            slot: None,
        }];
        let path = write_episode(root.path(), &jumped).unwrap();
        match interpret(&read_evidence(&path)).delay {
            QueueDelay::UnrelatedWait { alignable, .. } => assert!(!alignable),
            other => panic!("expected an unalignable wait, got {other:?}"),
        }
    }

    #[test]
    fn changed_or_unknown_holders_and_failed_admission_are_not_external() {
        let root = tempfile::tempdir().unwrap();
        let mut changed = draft(TerminalKind::WaitedGrant, true);
        changed.holder_changed = true;
        changed.holders = vec![HolderFinding {
            classification: HolderClass::OtherAttempt,
            reason: HolderReason::Tagged,
            slot: None,
        }];
        let path = write_episode(root.path(), &changed).unwrap();
        assert!(matches!(
            interpret(&read_evidence(&path)).delay,
            QueueDelay::Unknown {
                reason: "holder_identity_changed"
            }
        ));

        let mut same = draft(TerminalKind::WaitedGrant, true);
        same.admission_id = "admission-same".to_owned();
        same.holders = vec![HolderFinding {
            classification: HolderClass::SameAttempt,
            reason: HolderReason::Tagged,
            slot: None,
        }];
        let path = write_episode(root.path(), &same).unwrap();
        assert!(matches!(
            interpret(&read_evidence(&path)).delay,
            QueueDelay::SelfContention { .. }
        ));

        let mut timed_out = draft(TerminalKind::Timeout, true);
        timed_out.admission_id = "admission-timeout".to_owned();
        timed_out.holders = vec![HolderFinding {
            classification: HolderClass::OtherAttempt,
            reason: HolderReason::Tagged,
            slot: None,
        }];
        let path = write_episode(root.path(), &timed_out).unwrap();
        assert!(matches!(
            interpret(&read_evidence(&path)).delay,
            QueueDelay::FailedAdmission {
                terminal: TerminalKind::Timeout,
                ..
            }
        ));
    }

    #[test]
    fn correlation_tokens_reject_paths() {
        assert!(validate_token("attempt-a").is_ok());
        assert!(validate_token(r"C:\Users\private\account").is_err());
        assert!(validate_token("../attempt").is_err());
        assert!(validate_token("").is_err());
    }

    #[test]
    fn a_real_clock_sample_pairs_the_shared_counter_with_filetime() {
        let (frequency, sample) = sample_clock().unwrap();
        assert!(frequency > 0);
        assert!(sample.qpc > 0);
        assert!(sample.filetime > 0);
    }
}
