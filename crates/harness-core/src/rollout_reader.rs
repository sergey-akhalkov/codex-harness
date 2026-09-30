//! Tolerant reader for Codex rollout session JSONL files.
//!
//! Owns the recognized rollout event vocabulary (`session_meta`,
//! `turn_context`, `event_msg`, `token_usage_record`, `response_item`,
//! `compacted`), both recorded usage formats (older `event_msg` token-count
//! snapshots and newer `token_usage_record` per-response entries with turn and
//! thread captures), response-identity deduplication, turn association,
//! instruction byte capture and per-file coverage counters for malformed,
//! unknown or skipped records.
//!
//! Only allowlisted visible telemetry is parsed; reasoning and encrypted state
//! are ignored. Instruction texts are measured but never retained: consumers
//! receive byte counts, identities and counters, not instruction or transcript
//! content.
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
};

/// Recorded token counters of one response, turn or thread snapshot.
pub const TOKEN_FIELDS: [&str; 5] = [
    "input_tokens",
    "cached_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
    "total_tokens",
];
pub type Usage = BTreeMap<String, Option<u64>>;

const MAX_RECORD_BYTES: u64 = 32 * 1024 * 1024;
const EVENT_TYPES: [&str; 6] = [
    "session_meta",
    "turn_context",
    "event_msg",
    "token_usage_record",
    "response_item",
    "compacted",
];

/// Version of the checkpoint envelope written by [`ParserCheckpoint`].
pub const CHECKPOINT_FORMAT_VERSION: u32 = 1;

/// Semantics version of the parser and of the state a checkpoint carries.
///
/// Any change to recognised events, aggregation or the checkpointed state
/// shape must bump this value: stored checkpoints that do not match are
/// discarded and the file is fully parsed again.
pub const PARSER_VERSION: u32 = 1;

/// Per-file counters for malformed, unknown or skipped records.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// Non-blank lines read from the file.
    pub lines: u64,
    /// Lines parsed as JSON values (`recognized_events` plus `unrecognized_events`).
    pub events: u64,
    /// Parsed events with a recognized top-level event type.
    pub recognized_events: u64,
    /// Parsed JSON lines that are not recognized rollout events.
    pub unrecognized_events: u64,
    /// Lines that are not valid JSON.
    pub corrupt_lines: u64,
    /// Lines skipped because they exceed the per-record size bound.
    pub oversized_lines: u64,
}

/// Recorded instruction sizes; texts are measured and never retained.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionBytes {
    /// Bytes of recorded `session_meta` base instructions.
    pub base_bytes: u64,
    /// Bytes of recorded per-turn developer instruction messages.
    pub developer_bytes: u64,
}

/// One deduplicated model response with its recorded turn association.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnUsage {
    /// Recorded turn identity; `None` when the record carries none.
    pub turn_id: Option<String>,
    /// Stable response identity used for deduplication.
    pub response_id: Option<String>,
    /// Usage recorded for this response, the counted delta.
    pub usage: Usage,
    /// Cumulative turn usage when the record carries it.
    pub turn_usage: Option<Usage>,
    /// Cumulative thread usage when the record carries it.
    pub thread_usage: Option<Usage>,
    /// Recorded event timestamp of the usage record; `None` when it carried
    /// none. This is when the record was written, not when the provider
    /// generated tokens.
    #[serde(with = "optional_timestamp")]
    pub timestamp: Option<DateTime<Utc>>,
    /// Model in effect from the recorded turn context, when identifiable.
    pub model: Option<String>,
    /// Reasoning effort in effect from the recorded turn context, when
    /// identifiable.
    pub effort: Option<String>,
}

/// One recorded usage amount with the recorded event timestamp beside it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageSnapshot {
    #[serde(with = "optional_timestamp")]
    pub timestamp: Option<DateTime<Utc>>,
    pub usage: Usage,
}

/// Strong change-generation identity of one rollout file, read from its open
/// handle: volume and file identity, NTFS change time and size.
///
/// Equality of all three proves that no data was written between the two
/// observations. Size and modification time alone are not proof: an in-place
/// edit can preserve both, while NTFS updates the change time for every data
/// or metadata modification and `SetFileTime` resets it to the current time
/// instead of restoring an arbitrary value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIdentity {
    pub volume_serial: u64,
    pub file_id: [u8; 16],
    pub change_time: i64,
    pub size: u64,
}

impl FileIdentity {
    /// Identity of an open handle. Unavailable when the platform or the
    /// filesystem does not expose it, which forces full parsing.
    #[cfg(windows)]
    pub fn of_file(file: &File) -> std::io::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_BASIC_INFO, FILE_ID_INFO, FileBasicInfo, FileIdInfo, GetFileInformationByHandleEx,
        };
        let handle = file.as_raw_handle();
        let mut basic = FILE_BASIC_INFO::default();
        let ok = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileBasicInfo,
                std::ptr::from_mut(&mut basic).cast(),
                u32::try_from(size_of::<FILE_BASIC_INFO>()).unwrap_or_default(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut id = FILE_ID_INFO::default();
        let ok = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                std::ptr::from_mut(&mut id).cast(),
                u32::try_from(size_of::<FILE_ID_INFO>()).unwrap_or_default(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            volume_serial: id.VolumeSerialNumber,
            file_id: id.FileId.Identifier,
            change_time: basic.ChangeTime,
            size: file.metadata()?.len(),
        })
    }

    /// Identity is unavailable on platforms without the query; callers fall
    /// back to full parsing.
    #[cfg(not(windows))]
    pub fn of_file(_file: &File) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "file change identity is unavailable on this platform",
        ))
    }

    /// Identity of a path, for diagnostics and tests.
    pub fn of_path(path: &Path) -> std::io::Result<Self> {
        Self::of_file(&File::open(path)?)
    }

    /// `Ok(())` when `current` still identifies the same unchanged bytes.
    ///
    /// The returned reason is the most informative difference, and every
    /// difference forces full parsing.
    pub fn verify_unchanged(&self, current: &Self) -> Result<(), &'static str> {
        if self.volume_serial != current.volume_serial || self.file_id != current.file_id {
            return Err("file_replaced");
        }
        if current.size < self.size {
            return Err("file_truncated");
        }
        if current.size > self.size {
            return Err("file_grew");
        }
        if self.change_time != current.change_time {
            return Err("file_modified");
        }
        Ok(())
    }
}

/// Why a stored checkpoint could not be interpreted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckpointError {
    /// The stored bytes are not a checkpoint of this shape.
    Corrupt,
    /// The stored envelope or parser version differs from the current one.
    VersionMismatch,
}

/// Parser state at the end of the last complete line of one rollout file,
/// together with the change identity of the bytes it covers.
///
/// A checkpoint is a disposable local cache. It never replaces reading the
/// file: reuse additionally requires the stored identity to still describe
/// the bytes before [`ParserCheckpoint::boundary`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParserCheckpoint {
    format: u32,
    parser: u32,
    boundary: u64,
    identity: FileIdentity,
    state: Reader,
}

impl ParserCheckpoint {
    /// Offset after the last complete line covered by this checkpoint.
    pub fn boundary(&self) -> u64 {
        self.boundary
    }

    /// Change identity of the bytes covered by this checkpoint.
    pub fn identity(&self) -> FileIdentity {
        self.identity
    }

    /// Serializes the checkpoint for local storage.
    pub fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(std::io::Error::other)
    }

    /// Parses stored bytes, rejecting foreign formats and parser versions.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CheckpointError> {
        let checkpoint: Self =
            serde_json::from_slice(bytes).map_err(|_| CheckpointError::Corrupt)?;
        if checkpoint.format != CHECKPOINT_FORMAT_VERSION || checkpoint.parser != PARSER_VERSION {
            return Err(CheckpointError::VersionMismatch);
        }
        Ok(checkpoint)
    }
}

/// One parsed rollout session file.
#[derive(Debug)]
pub struct SessionSummary {
    /// Accounted thread row; consumed by delegation accounting as-is.
    pub row: Value,
    /// Duplicate-detection fingerprint of the accounted identity and usage.
    pub fingerprint: Value,
    /// Coverage warning codes observed while reading.
    pub warnings: BTreeSet<String>,
    /// Malformed, unknown and skipped record counters.
    pub coverage: Coverage,
    /// Sizes of recorded instructions.
    pub instructions: InstructionBytes,
    /// Recorded output bytes per tool name, matched through call identity.
    pub tool_output_bytes: BTreeMap<String, u64>,
    /// Deduplicated responses in recorded order with turn association.
    pub turns: Vec<TurnUsage>,
    /// Recorded cumulative counter snapshots in event order, with their
    /// recorded timestamps. Increments between them are the interval evidence
    /// of sessions that recorded no per-response usage.
    pub cumulative: Vec<UsageSnapshot>,
    /// Recorded usage of response records that carried no stable identity, so
    /// duplicates cannot be detected and interval allocation stays unknown.
    pub unidentified: Vec<UsageSnapshot>,
    /// Response identities whose repeated records disagreed; their usage
    /// stays unknown.
    pub conflicts: BTreeSet<String>,
    /// Open source handle retained for caller-side identity checks.
    pub source: Option<File>,
}

/// One reader pass over one rollout file.
#[derive(Debug)]
pub struct IncrementalRead {
    pub summary: SessionSummary,
    /// Source bytes read by this pass.
    pub bytes_read: u64,
    /// JSON events parsed by this pass; a reused prefix is not re-parsed.
    pub events_parsed: u64,
    /// The supplied checkpoint's proven prefix was reused.
    pub reused: bool,
    /// Why a supplied checkpoint was not reused.
    pub invalidation: Option<&'static str>,
    /// Fresh checkpoint for the parsed prefix; absent when the file changed
    /// while it was read or when nothing new needed storing.
    pub checkpoint: Option<ParserCheckpoint>,
}

/// RFC3339 round-trip for recorded instants.
///
/// The workspace chrono build does not enable serde support, and the
/// checkpoint format stores instants as RFC3339 text with nanosecond
/// precision so a resumed read observes exactly the parsed instant.
mod optional_timestamp {
    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &Option<DateTime<Utc>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(instant) => {
                serializer.serialize_some(&instant.to_rfc3339_opts(SecondsFormat::Nanos, true))
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<DateTime<Utc>>, D::Error> {
        let raw = Option::<String>::deserialize(deserializer)?;
        raw.map(|text| {
            DateTime::parse_from_rfc3339(&text)
                .map(|instant| instant.with_timezone(&Utc))
                .map_err(serde::de::Error::custom)
        })
        .transpose()
    }
}

/// Byte size of one recorded tool output: the string length when the record
/// carries text, otherwise the serialized size of the structured value.
fn output_bytes(value: &Value) -> u64 {
    if let Some(text) = value.as_str() {
        return text.len() as u64;
    }
    serde_json::to_string(value)
        .map(|text| text.len() as u64)
        .unwrap_or(0)
}

/// Recorded token counters of a value; missing or unusable fields stay unknown.
pub fn usage(raw: &Value) -> Usage {
    TOKEN_FIELDS
        .iter()
        .map(|key| ((*key).to_owned(), raw.get(key).and_then(Value::as_u64)))
        .collect()
}

pub fn unknown_usage() -> Usage {
    usage(&Value::Null)
}

/// Name-based provider attribution for recognized model names. Unknown or
/// local model names stay unattributed (`None`): this function never guesses
/// a provider or billing route.
pub fn model_provider(model: &str) -> Option<&'static str> {
    match model {
        "gpt-6-astra" | "openai/gpt-6-astra" => Some("OpenAI"),
        "xai/grok-4.6" | "grok-4.6" => Some("xai"),
        _ => None,
    }
}

pub fn list(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

pub fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    let text = value.as_str()?;
    if text.len() > 128 {
        return None;
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(parsed.with_timezone(&Utc));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y%m%dT%H%M%S%.f",
    ] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(text, format) {
            return Some(parsed.and_utc());
        }
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)
        .map(|v| v.and_utc())
}

fn usage_snapshot(value: &Value) -> Option<Usage> {
    value.is_object().then(|| usage(value))
}

fn identifier(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    (text.len() <= 128
        && text
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./:-".contains(&b)))
    .then(|| text.to_owned())
}

/// Extracts recorded text from a string or a list of text parts.
fn text_parts(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_owned();
    }
    let mut result = String::new();
    if value.is_object() {
        // Current session records carry nested instruction objects such as
        // `{"text": "..."}` instead of a bare string or a content list.
        item_text(value, &mut result);
    } else {
        for item in list(value) {
            item_text(item, &mut result);
        }
    }
    result
}

fn item_text(item: &Value, result: &mut String) {
    for key in ["text", "input_text"] {
        if let Some(text) = item[key].as_str() {
            result.push_str(text);
        }
    }
}

fn message_text(payload: &Value) -> String {
    text_parts(&payload["content"])
}

fn prefix(text: &str, count: usize) -> String {
    text.trim_start()
        .chars()
        .take(count)
        .collect::<String>()
        .to_lowercase()
}

fn continuations(triggers: &[Value], turns: &[Value], users: &[Value]) -> usize {
    let triggers: BTreeSet<_> = triggers.iter().filter_map(timestamp).collect();
    let turns: BTreeSet<_> = turns.iter().filter_map(timestamp).collect();
    let users: BTreeSet<_> = users.iter().filter_map(timestamp).collect();
    let mut claimed = BTreeSet::new();
    for trigger in triggers {
        let Some(turn) = turns
            .range((
                std::ops::Bound::Excluded(trigger),
                std::ops::Bound::Unbounded,
            ))
            .next()
        else {
            continue;
        };
        let user = users
            .range((
                std::ops::Bound::Excluded(trigger),
                std::ops::Bound::Unbounded,
            ))
            .next();
        if user.is_none_or(|user| user >= turn) {
            claimed.insert(*turn);
        }
    }
    claimed.len()
}

fn project_label(value: &str) -> Option<String> {
    if value.trim().is_empty() {
        return None;
    }
    Some(
        Path::new(value)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("workspace")
            .to_owned(),
    )
}

fn only(values: &BTreeSet<String>) -> Option<String> {
    (values.len() == 1).then(|| values.first().unwrap().clone())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Reader {
    thread_ids: Vec<String>,
    parents: BTreeSet<String>,
    models: BTreeSet<String>,
    efforts: BTreeSet<String>,
    session_ids: BTreeSet<String>,
    forked: BTreeSet<String>,
    versions: BTreeSet<String>,
    projects: BTreeSet<String>,
    warnings: BTreeSet<String>,
    counts: BTreeMap<String, u64>,
    responses: BTreeMap<String, Usage>,
    conflicts: BTreeSet<String>,
    children: BTreeSet<String>,
    durations: Vec<u64>,
    stamps: Vec<Value>,
    triggers: Vec<Value>,
    turns: Vec<Value>,
    users: Vec<Value>,
    tool_calls: BTreeMap<String, String>,
    tool_output_bytes: BTreeMap<String, u64>,
    usage: Usage,
    previous: Option<Usage>,
    saw_meta: bool,
    coverage: Coverage,
    instructions: InstructionBytes,
    turn_usages: Vec<TurnUsage>,
    /// Recorded model/effort per turn identity from `turn_context` events.
    contexts: BTreeMap<String, (Option<String>, Option<String>)>,
    /// Most recent recorded model/effort, in event order.
    context: Option<(Option<String>, Option<String>)>,
    /// Recorded cumulative snapshots in event order.
    cumulative: Vec<UsageSnapshot>,
    /// Recorded amounts of response records without a stable identity.
    unidentified: Vec<UsageSnapshot>,
}

impl Reader {
    fn bump(&mut self, key: &'static str) {
        *self.counts.entry(key.to_owned()).or_default() += 1;
    }
    fn warn(&mut self, code: &str) {
        self.warnings.insert(code.to_owned());
    }
    fn stamp(target: &mut Vec<Value>, event: &Value) {
        if event["timestamp"].as_str().is_some_and(|s| s.len() <= 128) {
            target.push(event["timestamp"].clone());
        }
    }
    fn check_usage(&mut self, value: &Usage) {
        if value.values().any(Option::is_none) {
            self.warn("invalid_or_missing_token_fields");
        }
        if value["cached_input_tokens"]
            .zip(value["input_tokens"])
            .is_some_and(|(c, i)| c > i)
        {
            self.warn("cached_input_exceeds_input");
        }
    }

    fn event(&mut self, event: Value) {
        if !event.is_object() {
            self.warn("invalid_event");
            self.coverage.unrecognized_events += 1;
            return;
        }
        let Some(kind) = event["type"].as_str() else {
            self.warn("invalid_event_type");
            self.coverage.unrecognized_events += 1;
            return;
        };
        if !EVENT_TYPES.contains(&kind) {
            self.coverage.unrecognized_events += 1;
            return;
        }
        self.coverage.recognized_events += 1;
        let p = &event["payload"];
        if !p.is_object() {
            self.warn("invalid_payload");
            return;
        }
        if kind == "session_meta" {
            self.saw_meta = true;
            self.instructions.base_bytes = self
                .instructions
                .base_bytes
                .saturating_add(text_parts(&p["base_instructions"]).len() as u64);
            for field in ["id", "session_id"] {
                if !p[field].is_null() {
                    if let Some(id) = identifier(&p[field]) {
                        if field == "id" {
                            if !self.thread_ids.contains(&id) {
                                self.thread_ids.push(id);
                            }
                        } else {
                            self.session_ids.insert(id);
                        }
                    } else {
                        self.warn("invalid_thread_id");
                    }
                }
            }
            if let Some(v) = identifier(&p["cli_version"]) {
                self.versions.insert(v);
            }
            if let Some(v) = identifier(&p["forked_from_id"]) {
                self.forked.insert(v);
                self.warn("forked_or_compacted_history");
            }
            for candidate in [
                &p["parent_thread_id"],
                &p["source"]["subagent"]["thread_spawn"]["parent_thread_id"],
                &p["thread_source"]["subagent"]["thread_spawn"]["parent_thread_id"],
            ] {
                if !candidate.is_null() {
                    if let Some(parent) = identifier(candidate) {
                        self.parents.insert(parent);
                    } else {
                        self.warn("invalid_parent_id");
                    }
                }
            }
        } else if kind == "turn_context" {
            let model = identifier(&p["model"]);
            if let Some(model) = &model {
                self.models.insert(model.clone());
            } else {
                self.warn("missing_model_context");
            }
            let effort = p.get("effort").unwrap_or(&p["reasoning_effort"]);
            let effort = effort
                .as_str()
                .filter(|v| {
                    [
                        "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
                    ]
                    .contains(v)
                })
                .map(str::to_owned);
            if let Some(e) = &effort {
                self.efforts.insert(e.clone());
            } else {
                self.warn("missing_reasoning_context");
            }
            if let Some(turn) = identifier(&p["turn_id"]) {
                self.contexts.insert(turn, (model.clone(), effort.clone()));
            }
            self.context = Some((model, effort));
            for value in std::iter::once(&p["cwd"]).chain(list(&p["workspace_roots"])) {
                if let Some(label) = value.as_str().and_then(project_label) {
                    self.projects.insert(label);
                }
            }
        } else if p["type"] == "token_count" {
            let Some(raw) = p["info"].get("total_token_usage") else {
                return;
            };
            let next = usage(raw);
            self.check_usage(&next);
            if self.previous.as_ref().is_some_and(|previous| {
                TOKEN_FIELDS
                    .iter()
                    .any(|key| next[*key].zip(previous[*key]).is_some_and(|(a, b)| a < b))
            }) {
                self.warn("cumulative_usage_decreased");
            }
            self.previous = Some(next.clone());
            self.usage = next.clone();
            self.cumulative.push(UsageSnapshot {
                timestamp: timestamp(&event["timestamp"]),
                usage: next,
            });
        } else if kind == "event_msg" && p["type"] == "task_started" {
            self.bump("task_started");
            Self::stamp(&mut self.turns, &event);
        } else if kind == "event_msg" && p["type"] == "task_complete" {
            self.bump("task_complete");
            if let Some(ms) = p["duration_ms"].as_u64() {
                self.durations.push(ms);
            }
        } else if kind == "event_msg" && p["type"] == "turn_aborted" {
            self.bump("turn_aborted");
        } else if kind == "token_usage_record" {
            let next = usage(&p["usage"]);
            let recorded_at = timestamp(&event["timestamp"]);
            if let Some(id) = identifier(&p["response_id"]) {
                if let Some(previous) = self.responses.get(&id) {
                    if previous != &next {
                        self.responses.insert(id.clone(), unknown_usage());
                        self.conflicts.insert(id);
                        self.warn("conflicting_response_id");
                    }
                } else {
                    let turn_id = identifier(&p["turn_id"]);
                    let context = turn_id
                        .as_ref()
                        .and_then(|turn| self.contexts.get(turn).cloned())
                        .or_else(|| self.context.clone());
                    let (model, effort) = context.unwrap_or((None, None));
                    self.responses.insert(id.clone(), next.clone());
                    Self::stamp(&mut self.turns, &event);
                    self.turn_usages.push(TurnUsage {
                        turn_id,
                        response_id: Some(id),
                        usage: next.clone(),
                        turn_usage: usage_snapshot(&p["turn_token_usage"]),
                        thread_usage: usage_snapshot(&p["thread_token_usage"]),
                        timestamp: recorded_at,
                        model,
                        effort,
                    });
                }
            } else {
                self.warn("missing_response_id");
                self.unidentified.push(UsageSnapshot {
                    timestamp: recorded_at,
                    usage: next.clone(),
                });
            }
            self.check_usage(&next);
            if next["reasoning_output_tokens"]
                .zip(next["output_tokens"])
                .is_some_and(|(r, o)| r > o)
            {
                self.warn("reasoning_not_included_in_output");
            }
        } else if kind == "response_item" {
            if p["type"] == "function_call" {
                if let (Some(call), Some(name)) = (p["call_id"].as_str(), p["name"].as_str()) {
                    self.tool_calls.insert(call.to_owned(), name.to_owned());
                }
            } else if p["type"] == "function_call_output" {
                if let Some(call) = p["call_id"].as_str() {
                    let name = self
                        .tool_calls
                        .get(call)
                        .cloned()
                        .unwrap_or_else(|| "unmatched_call".to_owned());
                    let bytes = output_bytes(&p["output"]);
                    *self.tool_output_bytes.entry(name).or_default() += bytes;
                }
            } else if p["type"] == "message" {
                let text = message_text(p);
                if p["role"] == "developer" {
                    self.instructions.developer_bytes = self
                        .instructions
                        .developer_bytes
                        .saturating_add(text.len() as u64);
                }
                if p["role"] == "user" {
                    let head = prefix(&text, 96);
                    let continuation = prefix(&text, 160);
                    if [
                        "<hook_prompt",
                        "<subagent_notification",
                        "<codex_internal_context",
                    ]
                    .iter()
                    .any(|m| head.starts_with(m))
                    {
                        self.bump("hook_messages");
                        *self.counts.entry("hook_chars".to_owned()).or_default() +=
                            text.chars().count() as u64;
                    } else if head.starts_with("<turn_aborted")
                        || head.starts_with("the user interrupted")
                        || [
                            "resume after tool-host restart",
                            "resume user goal",
                            "your turn ended",
                        ]
                        .iter()
                        .any(|phrase| continuation.contains(phrase))
                    {
                        self.bump("continuation_notices");
                        Self::stamp(&mut self.triggers, &event);
                    } else {
                        self.bump("user_messages");
                        Self::stamp(&mut self.users, &event);
                    }
                } else if p["phase"] == "commentary" {
                    self.bump("commentary_messages");
                } else if p["phase"] == "final_answer" {
                    self.bump("final_messages");
                }
            } else if p["type"] == "function_call" && p["name"] == "spawn_agent" {
                self.bump("spawn_calls");
            }
        } else if kind == "compacted" {
            self.bump("compacted_windows");
        }
        Self::stamp(&mut self.stamps, &event);
        if kind == "event_msg" && p["type"] == "item_completed" {
            for child in list(&p["item"]["receiver_thread_ids"]) {
                if let Some(id) = identifier(child) {
                    self.children.insert(id);
                }
            }
        }
    }

    fn finish(mut self) -> SessionSummary {
        let mut id = self.thread_ids.first().cloned();
        if self.thread_ids.len() > 1 {
            let inherited = |id: &String| {
                self.session_ids.contains(id)
                    || self.parents.contains(id)
                    || self.forked.contains(id)
            };
            if (!self.parents.is_empty() || !self.forked.is_empty())
                && self.thread_ids[1..].iter().all(inherited)
            {
                self.warn("forked_or_compacted_history");
            } else {
                self.warn("conflicting_thread_ids");
                id = None;
            }
        }
        if !self.saw_meta || self.thread_ids.is_empty() {
            self.warn("missing_thread_id");
        }
        if id.is_none() {
            self.usage = unknown_usage();
            self.responses.clear();
            self.children.clear();
            self.turn_usages.clear();
            self.cumulative.clear();
            self.unidentified.clear();
            self.conflicts.clear();
        }
        if self.parents.len() > 1 {
            self.warn("conflicting_parent_ids");
        }
        let model = only(&self.models);
        let mut provider = model.as_deref().and_then(model_provider);
        if self.models.len() > 1 {
            self.warn("mixed_model_attribution");
        } else if provider.is_none() {
            self.warn("unsupported_or_missing_model");
        }
        if self.efforts.len() > 1 {
            self.warn("mixed_reasoning");
        }
        if self.efforts.is_empty() {
            self.warn("missing_reasoning");
        }
        if self.usage.values().any(Option::is_none) {
            self.warn("missing_usage");
        }
        if self.warnings.contains("missing_model_context") {
            provider = None;
        }
        let project = if self.projects.len() > 1 {
            self.warn("mixed_project");
            Some("mixed".to_owned())
        } else {
            only(&self.projects)
        };
        if self.counts.get("compacted_windows").is_some_and(|n| *n > 0) {
            self.warn("forked_or_compacted_history");
        }
        if self.counts.get("turn_aborted").is_some_and(|n| *n > 0) {
            self.warn("interrupted_turn");
        }
        let parsed: Vec<_> = self
            .stamps
            .iter()
            .filter_map(|v| timestamp(v).map(|t| (t, v)))
            .collect();
        let first = parsed.iter().min_by_key(|(t, _)| *t);
        let last = parsed.iter().max_by_key(|(t, _)| *t);
        let elapsed =
            (parsed.len() >= 2).then(|| (last.unwrap().0 - first.unwrap().0).num_seconds().max(0));
        let mut row = json!({"id":id,"parent_id":only(&self.parents),"model":model,"reasoning":only(&self.efforts),
            "provider":provider,"missing_usage":self.usage.values().any(Option::is_none),"partial":!self.warnings.is_empty(),
            "project":project,"cli_version":only(&self.versions),"elapsed_seconds":elapsed,
            "response_count":self.responses.len(),"response_ids":self.responses.keys().collect::<Vec<_>>(),
            "response_usages":self.responses,"response_conflict_ids":self.conflicts,"spawned_child_ids":self.children,
            "actual_continuations":continuations(&self.triggers,&self.turns,&self.users),
            "turn_duration_ms_sum":if self.durations.is_empty() { None } else { self.durations.iter().try_fold(0u64,|a,b|a.checked_add(*b)) },
            "first_timestamp":first.map(|(_,v)| *v),"last_timestamp":last.map(|(_,v)| *v)});
        for (key, value) in self.usage {
            row[&key] = json!(value);
        }
        for key in [
            "spawn_calls",
            "hook_messages",
            "hook_chars",
            "continuation_notices",
            "task_started",
            "task_complete",
            "turn_aborted",
            "commentary_messages",
            "final_messages",
            "user_messages",
            "compacted_windows",
        ] {
            row[key] = json!(self.counts.get(key).copied().unwrap_or(0));
        }
        let selected: BTreeMap<_, _> = ["id", "parent_id", "model", "reasoning", "provider"]
            .into_iter()
            .chain(TOKEN_FIELDS)
            .map(|key| (key, row[key].clone()))
            .collect();
        let fingerprint = json!([
            selected,
            self.models,
            self.efforts,
            self.parents,
            self.warnings,
            self.responses
        ]);
        SessionSummary {
            row,
            fingerprint,
            warnings: self.warnings,
            coverage: self.coverage,
            instructions: self.instructions,
            tool_output_bytes: self.tool_output_bytes,
            turns: self.turn_usages,
            cumulative: self.cumulative,
            unidentified: self.unidentified,
            conflicts: self.conflicts,
            source: None,
        }
    }
}

/// Reads one rollout session file tolerantly; unreadable input yields warnings.
pub fn read(path: &Path) -> SessionSummary {
    read_incremental(path, None).summary
}

/// Reads one rollout file, reusing a supplied checkpoint only when its stored
/// identity still proves the bytes before its boundary unchanged.
///
/// Every uncertainty falls back to parsing from the start of the file: an
/// unavailable or changed identity, a boundary past the current size and an
/// unreadable checkpoint never suppress parsing. The trailing line is
/// consumed for the returned summary but stays outside the returned
/// checkpoint, so a later pass re-reads it until it is complete.
pub fn read_incremental(path: &Path, checkpoint: Option<ParserCheckpoint>) -> IncrementalRead {
    let fresh = || Reader {
        usage: unknown_usage(),
        ..Reader::default()
    };
    let mut reader = fresh();
    let mut held = None;
    let mut bytes_read = 0u64;
    let mut events_parsed = 0u64;
    let mut reused = false;
    let mut invalidation = None;
    let mut identity_before = None;
    let mut identity_after = None;
    let mut start = 0u64;
    let mut boundary = 0u64;
    let mut boundary_state = None;
    if let Ok(mut file) = File::open(path) {
        identity_before = FileIdentity::of_file(&file).ok();
        if let Some(stored) = checkpoint {
            match identity_before {
                Some(current) => match stored.identity.verify_unchanged(&current) {
                    Ok(()) if stored.boundary <= current.size => {
                        start = stored.boundary;
                        boundary = stored.boundary;
                        reader = stored.state;
                        reused = true;
                    }
                    Ok(()) => invalidation = Some("file_boundary_unreachable"),
                    Err(reason) => invalidation = Some(reason),
                },
                None => invalidation = Some("identity_unavailable"),
            }
        }
        let mut readable = true;
        if start > 0 && file.seek(SeekFrom::Start(start)).is_err() {
            // Defensive: a regular file seek does not fail in practice, and a
            // failure still cannot suppress parsing.
            reused = false;
            invalidation = Some("resume_failed");
            start = 0;
            boundary = 0;
            reader = fresh();
            if file.seek(SeekFrom::Start(0)).is_err() {
                reader.warn("unreadable_input");
                readable = false;
            }
        }
        if readable {
            let mut stream = BufReader::new(file);
            let mut line = Vec::new();
            let mut consumed = start;
            loop {
                line.clear();
                // A malformed giant line cannot force unbounded allocation.
                let count = (&mut stream)
                    .take(MAX_RECORD_BYTES + 1)
                    .read_until(b'\n', &mut line);
                match count {
                    Ok(0) => break,
                    Ok(read) => {
                        consumed += read as u64;
                        bytes_read += read as u64;
                    }
                    Err(_) => {
                        reader.warn("unreadable_input");
                        break;
                    }
                }
                let terminated = line.last() == Some(&b'\n');
                if !terminated {
                    // Anything after the last complete line is consumed for
                    // the returned summary but stays outside the
                    // checkpointed prefix, so the next pass re-reads it.
                    boundary_state = Some(reader.clone());
                }
                if line.len() as u64 > MAX_RECORD_BYTES {
                    reader.warn("oversized_jsonl_record");
                    reader.coverage.oversized_lines += 1;
                    if terminated {
                        boundary = consumed;
                    } else {
                        match stream.skip_until(b'\n') {
                            Ok(0) => {}
                            Ok(skipped) => {
                                consumed += skipped as u64;
                                bytes_read += skipped as u64;
                                boundary = consumed;
                            }
                            Err(_) => {
                                reader.warn("unreadable_input");
                                break;
                            }
                        }
                    }
                    continue;
                }
                if line.iter().all(u8::is_ascii_whitespace) {
                    if terminated {
                        boundary = consumed;
                    }
                    continue;
                }
                reader.coverage.lines += 1;
                match serde_json::from_slice(&line) {
                    Ok(event) => {
                        reader.coverage.events += 1;
                        events_parsed += 1;
                        reader.event(event);
                    }
                    Err(_) => {
                        reader.warn("corrupt_jsonl");
                        reader.coverage.corrupt_lines += 1;
                    }
                }
                if terminated {
                    boundary = consumed;
                }
            }
            identity_after = FileIdentity::of_file(stream.get_ref()).ok();
            held = Some(stream.into_inner());
        } else {
            identity_after = None;
            held = Some(file);
        }
    } else {
        reader.warn("unreadable_input");
    }
    let stable = identity_before.is_some() && identity_before == identity_after;
    let nothing_new = reused && bytes_read == 0;
    let stored_state = if stable && !nothing_new {
        Some(boundary_state.unwrap_or_else(|| reader.clone()))
    } else {
        None
    };
    let mut summary = reader.finish();
    summary.source = held;
    let checkpoint = stored_state
        .zip(identity_after)
        .map(|(state, identity)| ParserCheckpoint {
            format: CHECKPOINT_FORMAT_VERSION,
            parser: PARSER_VERSION,
            boundary,
            identity,
            state,
        });
    IncrementalRead {
        summary,
        bytes_read,
        events_parsed,
        reused,
        invalidation,
        checkpoint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, path::PathBuf};

    fn write(dir: &Path, name: &str, events: &[Value]) -> PathBuf {
        let path = dir.join(name);
        let text: String = events.iter().map(|event| format!("{event}\n")).collect();
        std::fs::write(&path, text).unwrap();
        path
    }
    fn meta(id: &str) -> Value {
        json!({"type":"session_meta","payload":{"id":id,"session_id":id}})
    }
    fn context() -> Value {
        json!({"type":"turn_context","payload":{"model":"fixture-model","effort":"high"}})
    }
    fn counts(amount: u64) -> Value {
        json!({"input_tokens":amount,"cached_input_tokens":amount/2,"output_tokens":amount/2,"reasoning_output_tokens":amount/5,"total_tokens":amount+amount/2})
    }
    fn record(turn: &str, response: &str, delta: u64, cumulative: u64) -> Value {
        json!({"type":"token_usage_record","payload":{"turn_id":turn,"response_id":response,
            "usage":counts(delta),"turn_token_usage":counts(cumulative),"thread_token_usage":counts(cumulative)}})
    }

    #[test]
    fn older_token_count_format_keeps_the_last_cumulative_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let path = write(
            root.path(),
            "older.jsonl",
            &[
                meta("thread_one"),
                context(),
                json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":counts(10)}}}),
                json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":counts(20)}}}),
            ],
        );
        let session = read(&path);
        assert_eq!(session.row["id"], "thread_one");
        assert_eq!(session.row["input_tokens"], 20);
        assert_eq!(session.row["total_tokens"], 30);
        assert_eq!(session.row["missing_usage"], false);
        assert_eq!(session.coverage.recognized_events, 4);
        assert_eq!(session.coverage.unrecognized_events, 0);
        assert_eq!(session.coverage.corrupt_lines, 0);
        assert!(session.turns.is_empty());
        assert!(session.source.is_some());
    }

    #[test]
    fn newer_usage_records_deduplicate_responses_and_associate_turns() {
        let root = tempfile::tempdir().unwrap();
        let path = write(
            root.path(),
            "newer.jsonl",
            &[
                meta("thread_two"),
                context(),
                record("turn_one", "resp_one", 10, 10),
                record("turn_one", "resp_one", 10, 10),
                record("turn_two", "resp_two", 20, 30),
            ],
        );
        let session = read(&path);
        assert_eq!(session.row["response_count"], 2);
        assert_eq!(
            session.row["response_usages"]["resp_one"]["total_tokens"],
            15
        );
        assert_eq!(
            session.row["response_usages"]["resp_two"]["total_tokens"],
            30
        );
        assert!(
            session.row["response_conflict_ids"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(session.turns.len(), 2);
        assert_eq!(session.turns[0].turn_id.as_deref(), Some("turn_one"));
        assert_eq!(session.turns[0].response_id.as_deref(), Some("resp_one"));
        assert_eq!(
            session.turns[1].turn_usage.as_ref().unwrap()["total_tokens"],
            Some(45)
        );
        assert_eq!(
            session.turns[1].thread_usage.as_ref().unwrap()["input_tokens"],
            Some(30)
        );
        // The newer format carries no thread cumulative snapshot for the row.
        assert_eq!(session.row["missing_usage"], true);
    }

    #[test]
    fn conflicting_response_identity_invalidates_and_missing_turn_identity_stays_explicit() {
        let root = tempfile::tempdir().unwrap();
        let mut no_turn = record("turn_unused", "resp_no_turn", 5, 5);
        no_turn["payload"]
            .as_object_mut()
            .unwrap()
            .remove("turn_id");
        no_turn["payload"]
            .as_object_mut()
            .unwrap()
            .remove("turn_token_usage");
        let mut unnamed = record("turn_unused", "resp_unused", 5, 5);
        unnamed["payload"]
            .as_object_mut()
            .unwrap()
            .remove("response_id");
        let path = write(
            root.path(),
            "conflicts.jsonl",
            &[
                meta("thread_three"),
                context(),
                record("turn_one", "resp_dup", 10, 10),
                record("turn_one", "resp_dup", 20, 20),
                no_turn,
                unnamed,
            ],
        );
        let session = read(&path);
        assert!(
            session.row["response_usages"]["resp_dup"]["total_tokens"].is_null(),
            "conflicting response usage must stay unknown"
        );
        assert!(
            session.row["response_conflict_ids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|id| id == "resp_dup")
        );
        assert!(session.warnings.contains("conflicting_response_id"));
        assert!(session.warnings.contains("missing_response_id"));
        assert_eq!(session.turns.len(), 2);
        assert_eq!(session.turns[1].turn_id, None);
        assert_eq!(session.turns[1].turn_usage, None);
        assert_eq!(session.turns[1].usage["total_tokens"], Some(7));
    }

    #[test]
    fn instruction_bytes_are_measured_without_retaining_text() {
        let root = tempfile::tempdir().unwrap();
        let mut meta = meta("thread_four");
        meta["payload"]["base_instructions"] = "base prompt".into();
        let path = write(
            root.path(),
            "instructions.jsonl",
            &[
                meta,
                context(),
                json!({"type":"response_item","payload":{"type":"message","role":"developer",
                    "content":[{"type":"input_text","text":"developer block"}]}}),
                json!({"type":"response_item","payload":{"type":"message","role":"user",
                    "content":[{"type":"input_text","text":"user request"}]}}),
            ],
        );
        let session = read(&path);
        assert_eq!(session.instructions.base_bytes, 11);
        assert_eq!(session.instructions.developer_bytes, 15);
        assert!(!format!("{:?}", session.row).contains("developer block"));
    }

    #[test]
    fn nested_base_instruction_objects_are_measured_like_strings() {
        let root = tempfile::tempdir().unwrap();
        let mut meta = meta("thread_nested");
        meta["payload"]["base_instructions"] = json!({"text": "nested base prompt"});
        let path = write(root.path(), "nested.jsonl", &[meta, context()]);
        let session = read(&path);
        assert_eq!(session.instructions.base_bytes, 18);
    }

    #[test]
    fn tool_output_bytes_are_matched_through_call_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = write(
            root.path(),
            "tools.jsonl",
            &[
                meta("thread_tools"),
                context(),
                json!({"type":"response_item","payload":{"type":"function_call",
                    "call_id":"call-1","name":"exec_command"}}),
                json!({"type":"response_item","payload":{"type":"function_call_output",
                    "call_id":"call-1","output":"0123456789"}}),
                json!({"type":"response_item","payload":{"type":"function_call_output",
                    "call_id":"call-2","output":{"rows":[1,2,3]}}}),
            ],
        );
        let session = read(&path);
        assert_eq!(session.tool_output_bytes["exec_command"], 10);
        assert!(session.tool_output_bytes.contains_key("unmatched_call"));
        assert!(session.tool_output_bytes["unmatched_call"] > 0);
    }

    #[test]
    fn unknown_and_malformed_lines_count_as_coverage_not_new_warnings() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("coverage.jsonl");
        let text = [
            meta("thread_five").to_string(),
            context().to_string(),
            json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":counts(10)}}}).to_string(),
            json!({"type":"world_state","payload":{"full":true}}).to_string(),
            "not json".to_owned(),
            json!([]).to_string(),
        ]
        .join("\n");
        std::fs::write(&path, text).unwrap();
        let session = read(&path);
        assert_eq!(session.coverage.lines, 6);
        assert_eq!(session.coverage.events, 5);
        assert_eq!(session.coverage.recognized_events, 3);
        assert_eq!(session.coverage.unrecognized_events, 2);
        assert_eq!(session.coverage.corrupt_lines, 1);
        assert_eq!(session.coverage.oversized_lines, 0);
        assert!(session.warnings.contains("corrupt_jsonl"));
        assert!(session.warnings.contains("invalid_event"));
        assert!(!session.warnings.contains("unrecognized_event"));
        assert_eq!(session.row["total_tokens"], 15);
    }

    #[test]
    fn unreadable_input_yields_a_warning_without_a_source_handle() {
        let root = tempfile::tempdir().unwrap();
        let session = read(&root.path().join("missing.jsonl"));
        assert!(session.source.is_none());
        assert!(session.warnings.contains("unreadable_input"));
        assert_eq!(session.coverage, Coverage::default());
        assert_eq!(session.row["id"], Value::Null);
    }

    fn stamped(mut event: Value, text: &str) -> Value {
        event["timestamp"] = text.into();
        event
    }

    fn frame(volume: u64, file: u8, change: i64, size: u64) -> FileIdentity {
        FileIdentity {
            volume_serial: volume,
            file_id: [file; 16],
            change_time: change,
            size,
        }
    }

    #[test]
    fn identity_verification_reports_the_first_material_difference() {
        let base = frame(1, 7, 10, 100);
        assert!(base.verify_unchanged(&base).is_ok());
        assert_eq!(
            base.verify_unchanged(&frame(2, 7, 10, 100)),
            Err("file_replaced")
        );
        assert_eq!(
            base.verify_unchanged(&frame(1, 7, 10, 40)),
            Err("file_truncated")
        );
        assert_eq!(
            base.verify_unchanged(&frame(1, 7, 10, 140)),
            Err("file_grew")
        );
        assert_eq!(
            base.verify_unchanged(&frame(1, 7, 11, 100)),
            Err("file_modified")
        );
    }

    #[cfg(windows)]
    #[test]
    fn in_place_mutation_with_preserved_size_and_time_changes_the_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = write(root.path(), "identity.jsonl", &[meta("thread_identity")]);
        let first = FileIdentity::of_path(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        let position = bytes.len() / 2;
        bytes[position] = if bytes[position] == b'x' { b'y' } else { b'x' };
        let mut handle = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        handle.write_all(&bytes).unwrap();
        handle.sync_all().unwrap();
        handle.set_modified(modified).unwrap();
        drop(handle);
        let preserved = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert!(
            preserved
                .duration_since(modified)
                .is_ok_and(|delta| delta.as_millis() < 1),
            "the modification time was not preserved: {preserved:?} vs {modified:?}"
        );
        let second = FileIdentity::of_path(&path).unwrap();
        assert_eq!(second.size, first.size);
        assert_eq!(second.file_id, first.file_id);
        assert_ne!(
            second.change_time, first.change_time,
            "a write must advance the NTFS change time even when the write time is restored"
        );
        assert_eq!(first.verify_unchanged(&second), Err("file_modified"));
    }

    #[cfg(windows)]
    #[test]
    fn replacement_and_truncation_change_the_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = write(root.path(), "replaced.jsonl", &[meta("thread_one")]);
        let first = FileIdentity::of_path(&path).unwrap();
        assert!(
            first
                .verify_unchanged(&FileIdentity::of_path(&path).unwrap())
                .is_ok()
        );
        std::fs::remove_file(&path).unwrap();
        let text = format!("{}\n", meta("thread_one"));
        std::fs::write(&path, text).unwrap();
        let replaced = FileIdentity::of_path(&path).unwrap();
        assert!(first.verify_unchanged(&replaced).is_err());
        std::fs::write(&path, "{}").unwrap();
        let truncated = FileIdentity::of_path(&path).unwrap();
        assert_eq!(replaced.verify_unchanged(&truncated), Err("file_truncated"));
    }

    #[test]
    fn usage_records_keep_recorded_timestamps_and_turn_context_attribution() {
        let root = tempfile::tempdir().unwrap();
        let mut first_context = context();
        first_context["payload"]["turn_id"] = "turn_one".into();
        let mut second_context = context();
        second_context["payload"]["model"] = "other-model".into();
        second_context["payload"]["turn_id"] = "turn_two".into();
        let path = write(
            root.path(),
            "attribution.jsonl",
            &[
                meta("thread_attribution"),
                first_context,
                stamped(
                    record("turn_one", "resp_one", 10, 10),
                    "2026-09-20T10:00:00Z",
                ),
                second_context,
                stamped(
                    record("turn_two", "resp_two", 20, 30),
                    "2026-09-20T11:00:00Z",
                ),
            ],
        );
        let session = read(&path);
        assert_eq!(session.turns.len(), 2);
        assert_eq!(
            session.turns[0].timestamp,
            timestamp(&json!("2026-09-20T10:00:00Z"))
        );
        assert_eq!(session.turns[0].model.as_deref(), Some("fixture-model"));
        assert_eq!(session.turns[0].effort.as_deref(), Some("high"));
        assert_eq!(session.turns[1].model.as_deref(), Some("other-model"));
        assert_eq!(
            session.turns[1].timestamp,
            timestamp(&json!("2026-09-20T11:00:00Z"))
        );
    }

    #[test]
    fn cumulative_snapshots_and_unidentified_records_keep_their_recorded_data() {
        let root = tempfile::tempdir().unwrap();
        let snapshot = |amount: u64, stamp: &str| {
            stamped(
                json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":counts(amount)}}}),
                stamp,
            )
        };
        let mut unnamed = record("turn_one", "unused", 5, 5);
        unnamed["payload"]
            .as_object_mut()
            .unwrap()
            .remove("response_id");
        let path = write(
            root.path(),
            "evidence.jsonl",
            &[
                meta("thread_evidence"),
                context(),
                snapshot(10, "2026-09-20T10:00:00Z"),
                snapshot(20, "2026-09-20T11:00:00Z"),
                stamped(unnamed, "2026-09-20T11:30:00Z"),
            ],
        );
        let session = read(&path);
        assert_eq!(session.cumulative.len(), 2);
        assert_eq!(
            session.cumulative[0].timestamp,
            timestamp(&json!("2026-09-20T10:00:00Z"))
        );
        assert_eq!(session.cumulative[0].usage["input_tokens"], Some(10));
        assert_eq!(session.cumulative[1].usage["input_tokens"], Some(20));
        assert_eq!(session.unidentified.len(), 1);
        assert_eq!(session.unidentified[0].usage["input_tokens"], Some(5));
        assert_eq!(
            session.unidentified[0].timestamp,
            timestamp(&json!("2026-09-20T11:30:00Z"))
        );
    }

    #[test]
    fn an_unchanged_checkpointed_read_reuses_the_prefix_without_reading() {
        let root = tempfile::tempdir().unwrap();
        let path = write(
            root.path(),
            "resume.jsonl",
            &[
                meta("thread_resume"),
                context(),
                stamped(
                    record("turn_one", "resp_one", 10, 10),
                    "2026-09-20T10:00:00Z",
                ),
            ],
        );
        let first = read_incremental(&path, None);
        assert!(!first.reused);
        assert_eq!(first.invalidation, None);
        assert_eq!(first.bytes_read, std::fs::metadata(&path).unwrap().len());
        assert_eq!(first.events_parsed, 3);
        let checkpoint = first.checkpoint.clone().unwrap();
        assert_eq!(
            checkpoint.boundary(),
            std::fs::metadata(&path).unwrap().len()
        );
        let second = read_incremental(&path, Some(checkpoint));
        assert!(second.reused);
        assert_eq!(second.invalidation, None);
        assert_eq!(second.bytes_read, 0);
        assert_eq!(second.events_parsed, 0);
        assert!(
            second.checkpoint.is_none(),
            "an unchanged input needs no rewritten checkpoint"
        );
        assert_eq!(second.summary.row, first.summary.row);
        assert_eq!(second.summary.fingerprint, first.summary.fingerprint);
        assert_eq!(second.summary.coverage, first.summary.coverage);
        assert_eq!(second.summary.turns, first.summary.turns);
        assert_eq!(second.summary.warnings, first.summary.warnings);
    }

    #[test]
    fn a_partial_trailing_line_is_re_read_until_complete() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("partial.jsonl");
        let lines = [meta("thread_partial").to_string(), context().to_string()];
        let third = stamped(
            record("turn_one", "resp_one", 10, 10),
            "2026-09-20T10:00:00Z",
        )
        .to_string();
        let cut = third.len() - 5;
        std::fs::write(
            &path,
            [lines[0].as_str(), lines[1].as_str(), &third[..cut]].join("\n"),
        )
        .unwrap();
        let first = read_incremental(&path, None);
        assert!(first.summary.warnings.contains("corrupt_jsonl"));
        let checkpoint = first.checkpoint.clone().unwrap();
        assert_eq!(
            checkpoint.boundary(),
            (lines[0].len() + lines[1].len() + 2) as u64
        );
        // The unchanged partial tail is re-read from the last complete line
        // and is never covered by the checkpoint.
        let unchanged = read_incremental(&path, Some(checkpoint.clone()));
        assert!(unchanged.reused);
        // Only the unterminated fragment after the last complete line is
        // re-read; everything before the boundary is reused.
        assert_eq!(unchanged.bytes_read, cut as u64);
        assert_eq!(unchanged.summary.row, first.summary.row);
        assert_eq!(unchanged.summary.coverage, first.summary.coverage);
        // Completing the event grows the file: growth alone cannot prove the
        // prefix unchanged, so the file is parsed in full and the completed
        // event is consumed exactly once.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(&third.as_bytes()[cut..]).unwrap();
        drop(file);
        let full = read(&path);
        let resumed = read_incremental(&path, Some(checkpoint));
        assert!(!resumed.reused);
        assert_eq!(resumed.invalidation, Some("file_grew"));
        assert_eq!(resumed.bytes_read, std::fs::metadata(&path).unwrap().len());
        assert_eq!(resumed.summary.row, full.row);
        assert_eq!(resumed.summary.coverage, full.coverage);
        assert_eq!(resumed.summary.turns, full.turns);
        assert_eq!(resumed.summary.turns.len(), 1);
        assert!(!resumed.summary.warnings.contains("corrupt_jsonl"));
    }

    #[test]
    fn a_changed_file_forces_a_full_parse_with_a_reason() {
        let root = tempfile::tempdir().unwrap();
        let path = write(
            root.path(),
            "grown.jsonl",
            &[meta("thread_grown"), context()],
        );
        let first = read_incremental(&path, None);
        let checkpoint = first.checkpoint.clone().unwrap();
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(format!("{}\n", record("turn_one", "resp_one", 10, 10)).as_bytes())
            .unwrap();
        drop(file);
        let resumed = read_incremental(&path, Some(checkpoint));
        assert!(!resumed.reused);
        assert_eq!(resumed.invalidation, Some("file_grew"));
        assert_eq!(
            resumed.bytes_read,
            std::fs::metadata(&path).unwrap().len(),
            "an unproven prefix cannot reduce reads"
        );
        let full = read(&path);
        assert_eq!(resumed.summary.row, full.row);
        assert_eq!(resumed.summary.coverage, full.coverage);
        assert_eq!(resumed.summary.turns, full.turns);
    }

    #[test]
    fn checkpoints_reject_corrupt_bytes_and_foreign_versions() {
        let root = tempfile::tempdir().unwrap();
        let path = write(
            root.path(),
            "checkpoint.jsonl",
            &[meta("thread_checkpoint")],
        );
        let checkpoint = read_incremental(&path, None).checkpoint.unwrap();
        let bytes = checkpoint.to_bytes().unwrap();
        let restored = ParserCheckpoint::from_bytes(&bytes).unwrap();
        assert_eq!(restored.boundary(), checkpoint.boundary());
        assert!(matches!(
            ParserCheckpoint::from_bytes(b"not a checkpoint"),
            Err(CheckpointError::Corrupt)
        ));
        let foreign =
            String::from_utf8(bytes)
                .unwrap()
                .replacen("\"parser\":1", "\"parser\":999", 1);
        assert!(matches!(
            ParserCheckpoint::from_bytes(foreign.as_bytes()),
            Err(CheckpointError::VersionMismatch)
        ));
    }

    #[test]
    fn resuming_from_a_checkpoint_matches_a_full_read_of_the_same_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let path = write(
            root.path(),
            "equivalent.jsonl",
            &[
                meta("thread_equivalent"),
                context(),
                stamped(
                    record("turn_one", "resp_one", 10, 10),
                    "2026-09-20T10:00:00Z",
                ),
                snapshot_event(20, "2026-09-20T11:00:00Z"),
            ],
        );
        let checkpoint = read_incremental(&path, None).checkpoint.unwrap();
        let full = read(&path);
        let resumed = read_incremental(&path, Some(checkpoint));
        assert_eq!(resumed.summary.row, full.row);
        assert_eq!(resumed.summary.fingerprint, full.fingerprint);
        assert_eq!(resumed.summary.coverage, full.coverage);
        assert_eq!(resumed.summary.instructions, full.instructions);
        assert_eq!(resumed.summary.tool_output_bytes, full.tool_output_bytes);
        assert_eq!(resumed.summary.turns, full.turns);
        assert_eq!(resumed.summary.cumulative, full.cumulative);
        assert_eq!(resumed.summary.conflicts, full.conflicts);
    }

    fn snapshot_event(amount: u64, stamp: &str) -> Value {
        stamped(
            json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":counts(amount)}}}),
            stamp,
        )
    }
}
