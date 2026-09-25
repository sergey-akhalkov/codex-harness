//! One bounded literal UTF-8 content source shared by `executor message`, `lead
//! message` and `executor spawn --exec -`, the harness-owned spill that delivers
//! an oversized payload, and the recorded-identity address defaults of the
//! lead-side executor commands.
//!
//! Content arrives literally from exactly one source: a short `--text` value, a
//! UTF-8 `--file`, or the full content of standard input (`--text -`, or no
//! content flag with a piped stream). Nothing here evaluates a shell, and no
//! source is truncated. A payload whose composed delivery exceeds the inline
//! bound is written once, complete, under the harness message state, and the
//! conversation receives a compact pointer naming the message identity, the
//! exact byte size and the absolute path the recipient must read.
//!
//! Every source is bounded at [`CEILING`]; above it the command refuses with the
//! actual number and sends nothing. An interactive terminal with no content
//! argument is refused as no content instead of blocking on a stream nobody
//! writes.
//!
//! Addressing is resolved only from the harness's own records: the explicit
//! flag, `CODEX_HOME` or the launcher's own resolution for the kit home, the
//! installation record for the accepted checkout, and the recorded live pool
//! lease, slot binding and receipt for the run identity. The working directory,
//! process names, window titles and "most recent session" are never consulted,
//! and a resolved value passes through exactly the verification an explicitly
//! typed one does.

use std::{
    fs, io,
    io::{IsTerminal, Read},
    path::{Path, PathBuf},
};

use super::observation::{
    self, RunObservation, STATE_COMPLETED, STATE_DEFECT, STATE_FAILED, STATE_INTERRUPTED,
    STATE_PARTIAL_STOP, STATE_STOPPED,
};
use super::{invalid, lease_live, read_lease, receipt_path};
use harness_core::task_worktree;

/// The inline delivery bound. A composed payload at or below it is delivered as
/// the literal text, byte-identically to the delivery this command always made;
/// a larger one is delivered through the spill.
pub(crate) const INLINE_BOUND: u64 = 256 * 1024;
/// The ceiling of one message, from any source. Above it the command refuses
/// with the actual number and nothing is sent: no truncation, no split.
pub(crate) const CEILING: u64 = 8 * 1024 * 1024;
/// Size bound of the shared spill directory under the kit home. Reaching it is
/// an honest refusal naming what occupies the directory, never silent eviction
/// of a message a live run may still need.
pub(crate) const MESSAGES_DIR_BOUND: u64 = 128 * 1024 * 1024;
/// Bound on address ambiguity listings and on the skipped-run notes of one
/// refusal, so a hand-edited or crowded state directory cannot grow output.
const MAX_ADDRESS_ENTRIES: usize = 6;

/// The literal content of one message from its declared source.
///
/// `--text TEXT` and `--file FILE` keep their literal meaning, `--text -`
/// selects the piped standard input explicitly, and having no content flag at
/// all means the same source. Both flags together stay a refusal: one message is
/// one input.
pub(crate) fn literal_content(text: Option<String>, file: Option<PathBuf>) -> io::Result<String> {
    match (text, file) {
        (Some(_), Some(_)) => Err(invalid(
            "give either --text TEXT or --file FILE, not both: one message is one input; nothing was sent",
        )),
        (Some(text), None) if text == "-" => piped_content(),
        (Some(text), None) => Ok(text),
        (None, Some(file)) => read_content_file(&file),
        (None, None) => piped_content(),
    }
}

/// The literal UTF-8 content of `--file` for one message: whatever the file
/// holds, with its real line breaks, bounded by the message ceiling. A file that
/// is not UTF-8 text is refused instead of being lossily rewritten; a leading
/// byte-order mark is framing, not content.
pub(crate) fn read_content_file(path: &Path) -> io::Result<String> {
    let bytes = observation::read_bounded(path, CEILING + 1).map_err(|error| {
        invalid(&format!(
            "message file {} is unreadable: {error}; nothing was sent",
            path.display()
        ))
    })?;
    decode(
        bytes,
        &format!("message file {}", path.display()),
        "nothing was sent",
    )
}

/// The full content of the standard input stream. `Ok(None)` means an
/// interactive terminal: the caller refuses as no content instead of blocking
/// on a stream nobody writes.
pub(crate) fn read_stdin_if_piped() -> io::Result<Option<String>> {
    if io::stdin().is_terminal() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(CEILING + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            invalid(&format!(
                "standard input could not be read: {error}; nothing was sent"
            ))
        })?;
    Ok(Some(decode(bytes, "standard input", "nothing was sent")?))
}

/// The content of the piped stream, or the refusal that names every supported
/// source. An empty stream is no content: it is refused by the caller's own
/// empty-message check, never sent as an empty message.
fn piped_content() -> io::Result<String> {
    match read_stdin_if_piped()? {
        Some(text) => Ok(text),
        None => Err(invalid(
            "no message content was given and standard input is an interactive terminal; pipe the text on standard input, or pass --text TEXT / --file FILE; nothing was sent",
        )),
    }
}

/// The complete text one command requires from a piped standard input stream,
/// refusing an interactive terminal and an empty stream before anything else
/// happens: `effect` names the supported sources and what the refusal left
/// undone, so no slot is allocated and no model request follows one.
pub(crate) fn piped_required(what: &str, effect: &str) -> io::Result<String> {
    match read_stdin_if_piped()? {
        Some(text) if !text.trim().is_empty() => Ok(text),
        Some(_) => Err(invalid(&format!("{what} is empty; {effect}"))),
        None => Err(invalid(&format!(
            "{what} is absent and standard input is an interactive terminal; {effect}"
        ))),
    }
}

/// One bounded literal read: the ceiling, then UTF-8 and the byte-order mark.
/// The refusal names the actual size so the caller never has to know the limit
/// in advance.
fn decode(bytes: Vec<u8>, what: &str, suffix: &str) -> io::Result<String> {
    if bytes.len() as u64 > CEILING {
        return Err(invalid(&format!(
            "{what} is {} bytes, above the {CEILING}-byte ceiling of one message; nothing was truncated and {suffix}",
            bytes.len()
        )));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| invalid(&format!("{what} is not UTF-8 text; {suffix}")))?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}

/// One spilled payload as the delivery records it: the message identity, the
/// absolute file carrying the complete literal payload and its exact size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Spill {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
    pub(crate) bytes: u64,
}

/// One composed delivery: the text the conversation receives and, when the
/// payload did not fit inline, the spill that text points at. `text` is
/// byte-identical to the literal payload for an inline-sized message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Delivery {
    pub(crate) text: String,
    pub(crate) spill: Option<Spill>,
}

impl Delivery {
    /// The recorded delivery evidence of a spill, merged into one message
    /// attempt so the receipt and the watch surfaces never present a spilled
    /// payload as inline delivery.
    pub(crate) fn record(&self, entry: &mut serde_json::Value) {
        let Some(spill) = &self.spill else {
            return;
        };
        entry["delivery"] = serde_json::Value::String("spill".into());
        entry["payloadPath"] = serde_json::Value::String(spill.path.display().to_string());
        entry["payloadBytes"] = serde_json::Value::from(spill.bytes);
    }

    /// One recorded message attempt carrying this delivery's evidence: an
    /// inline-sized message keeps exactly the fields it always had.
    pub(crate) fn recorded(&self, mut entry: serde_json::Value) -> serde_json::Value {
        self.record(&mut entry);
        entry
    }
}

/// Composes one delivery from the literal payload. `frame` renders the text the
/// conversation receives for a given body: the payload itself in the addressed
/// executor direction, or the recorded header plus body in the lead direction.
///
/// Above the inline bound the complete payload is written once to the harness
/// message state and the conversation receives the pointer envelope instead;
/// the payload itself is never truncated, split or evaluated.
pub(crate) fn compose(
    home: &Path,
    id: &str,
    payload: &str,
    frame: impl Fn(&str) -> String,
) -> io::Result<Delivery> {
    let inline = frame(payload);
    if payload.len() as u64 <= INLINE_BOUND && inline.len() as u64 <= INLINE_BOUND {
        return Ok(Delivery {
            text: inline,
            spill: None,
        });
    }
    let path = write_spill(home, id, payload)?;
    let pending = pointer_text(id, payload.len() as u64, &path);
    Ok(Delivery {
        text: frame(&pending),
        spill: Some(Spill {
            id: id.to_owned(),
            path,
            bytes: payload.len() as u64,
        }),
    })
}

/// The harness message state directory: one home-wide owner of spilled message
/// payloads, outside every worktree, repository and temporary directory.
pub(crate) fn messages_dir(home: &Path) -> PathBuf {
    home.join("harness/messages")
}

/// The compact pointer envelope one spilled message delivers: the identity, the
/// exact size and the absolute path of the complete literal payload, with the
/// instruction the recipient needs and no transport or size knowledge.
fn pointer_text(id: &str, bytes: u64, path: &Path) -> String {
    format!(
        "[message {id} is {bytes} bytes - larger than one conversation input, so the complete literal payload was written to a harness file and must be read now]\npath: {}\nbytes: {bytes}\nread: open that file with your ordinary file tools and treat its whole content as this message; nothing was truncated, summarized or split",
        path.display()
    )
}

/// Persists one complete payload under the message directory, keyed by the
/// message identity that already names this input, and returns its absolute
/// path. The write is atomic (temp then rename), so no reader ever sees a
/// partial payload, and the directory is checked against its bound before
/// anything is written.
fn write_spill(home: &Path, id: &str, payload: &str) -> io::Result<PathBuf> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(invalid(&format!(
            "message identity {id:?} cannot name a message file; nothing was sent"
        )));
    }
    let dir = messages_dir(home);
    fs::create_dir_all(&dir).map_err(|error| {
        invalid(&format!(
            "the harness message directory {} could not be created: {error}; nothing was sent",
            dir.display()
        ))
    })?;
    let path = dir.join(format!("{id}.txt"));
    let existing = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    let occupied = directory_bytes(&dir)?.saturating_sub(existing);
    let wanted = occupied + payload.len() as u64;
    if wanted > MESSAGES_DIR_BOUND {
        return Err(invalid(&format!(
            "the harness message directory {} holds {occupied} bytes and this {}-byte payload would exceed its {MESSAGES_DIR_BOUND}-byte bound; nothing was sent. Next action: release or clean the run records that own those messages (`codex-harness executor release` removes the messages of the run it releases)",
            dir.display(),
            payload.len()
        )));
    }
    let temp = dir.join(format!(".{id}.{}.tmp", std::process::id()));
    fs::write(&temp, payload.as_bytes()).map_err(|error| {
        invalid(&format!(
            "the message payload could not be written to {}: {error}; nothing was sent",
            temp.display()
        ))
    })?;
    fs::rename(&temp, &path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        invalid(&format!(
            "the message payload could not be published as {}: {error}; nothing was sent",
            path.display()
        ))
    })?;
    Ok(path)
}

/// The bytes currently held by the message directory's own files.
fn directory_bytes(dir: &Path) -> io::Result<u64> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut total = 0u64;
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

/// Removes the spilled message files one run's own records name, and nothing
/// else: the payload paths recorded on that run's messages are the only files
/// this touches, so a neighboring run's messages survive its release. Called
/// with the run-record cleanup, it is what keeps the message directory bounded
/// in ordinary use.
pub(crate) fn cleanup_recorded_messages(home: &Path, receipt: &Path) -> io::Result<usize> {
    let bytes = match fs::read(receipt) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return Ok(0),
    };
    let dir = messages_dir(home);
    let mut removed = 0;
    for record in ["messages", "leadMessages", "replyRequests"] {
        let Some(entries) = value.get(record).and_then(serde_json::Value::as_array) else {
            continue;
        };
        for entry in entries {
            let Some(path) = entry.get("payloadPath").and_then(|path| path.as_str()) else {
                continue;
            };
            let path = PathBuf::from(path);
            if path.parent().is_none_or(|parent| parent != dir.as_path()) {
                continue;
            }
            match fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(io::Error::other(format!(
                        "the recorded message payload {} could not be removed with its run records: {error}",
                        path.display()
                    )));
                }
            }
        }
    }
    Ok(removed)
}

/// The kit home one lead-side executor command works in: the explicit flag,
/// else `CODEX_HOME`, else the launcher's own documented resolution
/// (`USERPROFILE\.codex`). Only the environment and the recorded installation
/// are consulted - never the working directory.
pub(crate) fn codex_home(explicit: Option<PathBuf>) -> io::Result<PathBuf> {
    if let Some(home) = explicit {
        return Ok(home);
    }
    harness_core::native_launcher::codex_home().map_err(|error| {
        invalid(&format!(
            "--codex-home is not given and the kit home cannot be resolved: {error}; pass --codex-home DIRECTORY"
        ))
    })
}

/// The accepted checkout: the explicit flag, else the source checkout the
/// installation record under that home names. A relocation is not guessed and
/// the working directory is not a candidate.
pub(crate) fn source(explicit: Option<PathBuf>, home: &Path) -> io::Result<PathBuf> {
    match explicit {
        Some(source) => Ok(source),
        None => recorded_source(home).ok_or_else(|| {
            invalid(&format!(
                "--source is not given and {} records no installed source checkout; pass --source CHECKOUT",
                home.join("harness/installation.json").display()
            ))
        }),
    }
}

/// The source checkout of the installation recorded under one home. The current
/// metadata keeps its owners inside `settings`; the legacy import schema kept
/// `sourceRoot` at the top level. Only those two documented locations are read.
fn recorded_source(home: &Path) -> Option<PathBuf> {
    let path = home.join("harness/installation.json");
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return None;
    }
    let record: serde_json::Value = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
    let root = record
        .get("settings")
        .and_then(|settings| settings.get("sourceRoot"))
        .or_else(|| record.get("sourceRoot"))
        .and_then(|value| value.as_str())?;
    let root = PathBuf::from(root);
    root.is_dir().then_some(root)
}

/// One run resolved from recorded state: the slot binding, the owner it is bound
/// to and, when the receipt records one, its exact session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordedRun {
    pub(crate) slot: u32,
    pub(crate) owner: String,
    pub(crate) session: Option<String>,
}

/// Resolves the pieces of one address that were not supplied explicitly, from
/// the recorded state only:
///
/// - nothing supplied: the one live pooled run, refusing to guess between
///   several and reporting an honest state error when none is live;
/// - `--slot N` only: the owner that slot is bound to, live or not, so the
///   command's own verification reports the observed state;
/// - `--owner ID` only: the one slot whose binding names that owner;
/// - both supplied: nothing is resolved here and the values are verified
///   exactly as before.
pub(crate) fn resolve_run(
    home: &Path,
    source: &Path,
    slot: Option<u32>,
    owner: Option<&str>,
    action: &str,
) -> io::Result<RecordedRun> {
    match (slot, owner) {
        (Some(slot), Some(owner)) => Ok(RecordedRun {
            slot,
            owner: owner.to_owned(),
            session: recorded_session(home, source, slot),
        }),
        (Some(slot), None) => bound_slot(home, source, slot, action),
        (None, Some(owner)) => owner_slot(home, source, owner, action),
        (None, None) => live_run(home, source, action),
    }
}

/// The run one slot is bound to, whether or not its host is still running: the
/// binding is what the caller's verification then checks against the live run,
/// so a stale binding keeps reporting the existing observed-state error.
fn bound_slot(home: &Path, source: &Path, slot: u32, action: &str) -> io::Result<RecordedRun> {
    let record = task_worktree::load_slot_record(home, source, slot)?;
    let owner = record.and_then(|record| record.owner);
    let Some(owner) = owner else {
        return Err(invalid(&format!(
            "slot {slot} has no recorded session binding and {action} addresses a dispatched run, so dispatch or resume that session first; nothing was {}",
            action_effect(action)
        )));
    };
    Ok(RecordedRun {
        slot,
        owner,
        session: recorded_session(home, source, slot),
    })
}

/// The one slot whose recorded binding names the supplied owner.
fn owner_slot(home: &Path, source: &Path, owner: &str, action: &str) -> io::Result<RecordedRun> {
    let found = recorded_slots(home, source)?
        .into_iter()
        .filter(|record| record.owner.as_deref() == Some(owner))
        .map(|record| RecordedRun {
            slot: record.index,
            owner: owner.to_owned(),
            session: recorded_session(home, source, record.index),
        })
        .collect::<Vec<_>>();
    match found.as_slice() {
        [one] => Ok(RecordedRun {
            slot: one.slot,
            owner: one.owner.clone(),
            session: one.session.clone(),
        }),
        [] => Err(with_effect(
            invalid(&format!(
                "no recorded slot is bound to owner {owner} in {}; pass --slot N to address the run explicitly",
                source_note(home, source),
            )),
            action,
        )),
        several => Err(ambiguous(&format!("owner {owner}"), several, action)),
    }
}

/// The one live run recorded for this home and source, as the pool's own leases
/// and slot bindings describe it.
fn live_run(home: &Path, source: &Path, action: &str) -> io::Result<RecordedRun> {
    let mut live = Vec::new();
    let mut skipped = Vec::new();
    for lease in live_leases(home, source)? {
        let slot = lease.index;
        if task_worktree::load_slot_record(home, source, slot)?
            .and_then(|record| record.owner)
            .as_deref()
            != Some(lease.owner.as_str())
        {
            skipped.push(format!("slot {slot}: not bound to {}", lease.owner));
            continue;
        }
        match recorded_state(home, source, slot) {
            Some(state) if lifecycle_ended(&state) => {
                skipped.push(format!("slot {slot}: recorded run ended as {state}"));
                continue;
            }
            None => {
                skipped.push(format!("slot {slot}: no dispatch receipt"));
                continue;
            }
            Some(_) => {}
        }
        live.push(RecordedRun {
            slot,
            owner: lease.owner.clone(),
            session: recorded_session(home, source, slot),
        });
    }
    match live.as_slice() {
        [one] => Ok(one.clone()),
        [] => {
            let note = skipped_note(&skipped);
            let detail = if note.is_empty() {
                String::new()
            } else {
                format!("; recorded but not addressable: {note}")
            };
            Err(with_effect(
                invalid(&format!(
                    "no live executor run is recorded in {} for {}{detail}; dispatch `codex-harness executor spawn` first, or pass --slot N --owner ID to address a recorded run",
                    home.display(),
                    source.display()
                )),
                action,
            ))
        }
        several => Err(ambiguous("", several, action)),
    }
}

/// The refusal of an ambiguous address: the bounded list of the recorded runs
/// and the argument that disambiguates them. Choosing one is never inferred.
fn ambiguous(what: &str, found: &[RecordedRun], action: &str) -> io::Error {
    let mut names = Vec::new();
    for run in found.iter().take(MAX_ADDRESS_ENTRIES) {
        names.push(format!(
            "slot {} owner {} session {}",
            run.slot,
            run.owner,
            run.session.as_deref().unwrap_or("unrecorded")
        ));
    }
    let more = found.len().saturating_sub(names.len());
    let more = if more == 0 {
        String::new()
    } else {
        format!("; and {more} more")
    };
    let what = if what.is_empty() {
        String::new()
    } else {
        format!(" ({what})")
    };
    invalid(&format!(
        "{action} addresses exactly one run and {} match{what}: {}{more}; pass --slot N to address exactly one of them; nothing was {}",
        found.len(),
        names.join("; "),
        action_effect(action),
    ))
}

fn with_effect(error: io::Error, action: &str) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("{error}; nothing was {}", action_effect(action)),
    )
}

fn action_effect(action: &str) -> &'static str {
    match action {
        "watch" => "observed",
        _ => "sent",
    }
}

/// Bounded summary of the recorded runs that could not be addressed, so an
/// honest error names what the records actually hold.
fn skipped_note(skipped: &[String]) -> String {
    let mut note = skipped
        .iter()
        .take(MAX_ADDRESS_ENTRIES)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if skipped.len() > MAX_ADDRESS_ENTRIES {
        note.push_str(&format!(
            "; and {} more",
            skipped.len() - MAX_ADDRESS_ENTRIES
        ));
    }
    note
}

fn source_note(home: &Path, source: &Path) -> String {
    format!(
        "{} records the pool of {}",
        home.display(),
        source.display()
    )
}

/// The live leases recorded for this home and source, by their own recorded
/// process identity.
fn live_leases(home: &Path, source: &Path) -> io::Result<Vec<super::SessionLease>> {
    let dir = task_worktree::pool_state_dir(home, source)?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut leases = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("lease-") && name.ends_with(".json")) {
            continue;
        }
        if let Some(lease) = read_lease(&entry.path())
            && lease_live(&lease)
        {
            leases.push(lease);
        }
    }
    leases.sort_by_key(|lease| lease.index);
    Ok(leases)
}

/// The recorded slot bindings of this home and source, by index.
fn recorded_slots(home: &Path, source: &Path) -> io::Result<Vec<task_worktree::SlotRecord>> {
    let dir = task_worktree::pool_state_dir(home, source)?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut slots = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(index) = name
            .strip_prefix("slot-")
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|index| index.parse::<u32>().ok())
        else {
            continue;
        };
        if let Some(record) = task_worktree::load_slot_record(home, source, index)? {
            slots.push(record);
        }
    }
    slots.sort_by_key(|record| record.index);
    Ok(slots)
}

/// The recorded lifecycle state of one slot's run, from its dispatch receipt.
fn recorded_state(home: &Path, source: &Path, slot: u32) -> Option<String> {
    receipt_value(home, source, slot)
        .and_then(|value| RunObservation::from_receipt(&value).map(|run| run.state.clone()))
}

/// The exact session one slot's dispatch receipt recorded, when it has one.
fn recorded_session(home: &Path, source: &Path, slot: u32) -> Option<String> {
    receipt_value(home, source, slot)
        .and_then(|value| RunObservation::from_receipt(&value))
        .and_then(|run| run.recorded_session().map(str::to_owned))
}

fn receipt_value(home: &Path, source: &Path, slot: u32) -> Option<serde_json::Value> {
    let path = receipt_path(home, source, slot).ok()?;
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// True for a recorded lifecycle that already ended: such a run keeps its result
/// and is continued explicitly, never messaged or stopped through a default.
fn lifecycle_ended(state: &str) -> bool {
    matches!(
        state,
        STATE_COMPLETED
            | STATE_STOPPED
            | STATE_PARTIAL_STOP
            | STATE_FAILED
            | STATE_DEFECT
            | STATE_INTERRUPTED
    )
}
