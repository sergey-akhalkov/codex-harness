//! Repeated cache-loss protection over the exact executor's recorded usage.
//! No provider calls, transcript rendering or opaque reasoning inspection.

use super::{Line, SpoolTail, now_ms, update_receipt_field};
use serde_json::{Value, json};
use std::os::windows::ffi::OsStrExt;
use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Storage::FileSystem::{
        FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_NOTIFY_CHANGE_SIZE, FindCloseChangeNotification, FindFirstChangeNotificationW,
        FindNextChangeNotification,
    },
    System::Threading::WaitForSingleObject,
};

const LARGE_INPUT: u64 = 100_000;
const REQUIRED_MISSES: u32 = 3;

/// Kernel change notification, checked by the host's existing event loop.
/// A sparse metadata reconciliation covers Windows' delayed write notices.
struct Changes(HANDLE);

impl Changes {
    fn new(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root)?;
        let path: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: the path is NUL-terminated and the returned handle is owned.
        let handle = unsafe {
            FindFirstChangeNotificationW(
                path.as_ptr(),
                1,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_SIZE
                    | FILE_NOTIFY_CHANGE_LAST_WRITE,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    fn take(&self) -> io::Result<bool> {
        // SAFETY: this object owns a live notification handle until Drop.
        match unsafe { WaitForSingleObject(self.0, 0) } {
            WAIT_TIMEOUT => Ok(false),
            WAIT_OBJECT_0 => {
                // Rearm before reading so a concurrent append is not lost.
                if unsafe { FindNextChangeNotification(self.0) } == 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(true)
                }
            }
            _ => Err(io::Error::last_os_error()),
        }
    }
}

impl Drop for Changes {
    fn drop(&mut self) {
        // SAFETY: closes this object's uniquely owned notification handle.
        unsafe {
            FindCloseChangeNotification(self.0);
        }
    }
}

#[derive(Debug)]
pub(crate) struct CacheLoss(pub String, pub Value);

impl std::fmt::Display for CacheLoss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CacheLoss {}

#[derive(Default)]
struct Detector {
    warm: bool,
    consecutive: u32,
    cumulative: u64,
    responses: u64,
    input: u64,
    cached: u64,
    invalid_records: u64,
}

impl Detector {
    fn observe(&mut self, record: &Value, historical: bool) -> Option<CacheLoss> {
        let usage = &record["usage"];
        let values = (
            usage["input_tokens"].as_u64(),
            usage["cached_input_tokens"].as_u64(),
            record["thread_token_usage"]["input_tokens"].as_u64(),
        );
        let (Some(input), Some(cached), Some(cumulative)) = values else {
            self.consecutive = 0;
            self.invalid_records += 1;
            return None;
        };
        if cached > input || cumulative < input {
            self.consecutive = 0;
            self.invalid_records += 1;
            return None;
        }
        if cumulative <= self.cumulative {
            return None; // Duplicate or out-of-order notification, never a new miss.
        }
        self.cumulative = cumulative;
        self.responses += 1;
        self.input = input;
        self.cached = cached;
        let miss = input - cached;
        if input >= LARGE_INPUT && u128::from(cached) * 10 >= u128::from(input) * 9 {
            self.warm = true;
        }
        if historical || !self.warm || miss < LARGE_INPUT || miss < input.div_ceil(2) {
            self.consecutive = 0;
            return None;
        }
        self.consecutive += 1;
        (self.consecutive >= REQUIRED_MISSES).then(|| {
            CacheLoss(format!(
                "DeepSeek cache loss: {} consecutive responses after cache warmup each missed at least {LARGE_INPUT} input tokens and 50% of input; last input={input} cached={cached} miss={miss}. Automatic continuation is stopped; files and session are retained",
                self.consecutive
            ), json!({"warm":self.warm,"responses":self.responses,
                "consecutiveMisses":self.consecutive,"lastInputTokens":input,
                "lastCachedTokens":cached,"lastMissTokens":miss}))
        })
    }
}

pub(crate) struct Monitor {
    home: PathBuf,
    receipt: PathBuf,
    started_ms: u64,
    started: Instant,
    changes: Changes,
    dirty: bool,
    next_reconcile: Instant,
    observed_bytes: u64,
    session: Option<String>,
    tail: Option<SpoolTail>,
    handoff: VecDeque<String>,
    detector: Detector,
    unavailable: bool,
}

impl Monitor {
    /// The receipt's resolved provider selects this policy; other providers
    /// retain their current behavior. The host supplies its actual CODEX_HOME.
    pub(crate) fn new(receipt: &Path, home: &Path) -> io::Result<Option<Self>> {
        let value: Value = serde_json::from_slice(&fs::read(receipt)?)?;
        if value["modelProvider"] != "deepseek"
            && !value["model"]
                .as_str()
                .is_some_and(|model| model.starts_with("deepseek-"))
        {
            return Ok(None);
        }
        let now = Instant::now();
        let monitor = Self {
            home: home.to_owned(),
            receipt: receipt.to_owned(),
            started_ms: now_ms(),
            started: now,
            changes: Changes::new(&home.join("sessions"))?,
            dirty: true,
            next_reconcile: now + Duration::from_secs(1),
            observed_bytes: 0,
            session: None,
            tail: None,
            handoff: VecDeque::new(),
            detector: Detector::default(),
            unavailable: false,
        };
        monitor.record("waiting-for-usage", None)?;
        println!(
            "cache guard: enabled for DeepSeek; waiting for exact-session per-response usage (not a hard spending cap)"
        );
        Ok(Some(monitor))
    }

    fn record(&self, status: &str, reason: Option<&str>) -> io::Result<()> {
        update_receipt_field(
            &self.receipt,
            "cacheGuard",
            json!({"status": status, "session": self.session, "warm": self.detector.warm,
                "responses": self.detector.responses, "consecutiveMisses": self.detector.consecutive,
                "lastInputTokens": self.detector.input, "lastCachedTokens": self.detector.cached,
                "lastMissTokens": self.detector.input - self.detector.cached,
                "invalidRecords": self.detector.invalid_records,
                "reason": reason}),
        )
    }

    pub(crate) fn poll(
        &mut self,
        session: Option<&str>,
        native_activity: bool,
    ) -> io::Result<Option<CacheLoss>> {
        let now = Instant::now();
        if let Some(session) = session {
            match &self.session {
                Some(bound) if bound != session => {
                    return Err(io::Error::other(
                        "cache guard native session identity changed",
                    ));
                }
                None => {
                    self.session = Some(session.to_owned());
                    self.dirty = true;
                }
                _ => {}
            }
        }
        self.dirty |= self.changes.take()? || native_activity;
        if now >= self.next_reconcile {
            self.next_reconcile = now + Duration::from_secs(1);
            if let Some(tail) = &self.tail {
                // Query the open file, not directory-cached metadata. Cached
                // writes can be readable long before a notification arrives.
                let length = tail.file.metadata()?.len();
                if length < self.observed_bytes {
                    return Err(io::Error::other(
                        "cache guard rollout was truncated; usage coverage lost",
                    ));
                }
                self.dirty |= length > self.observed_bytes;
            } else if self.session.is_some() {
                self.dirty = true;
            }
        }
        if self.tail.is_none() && self.dirty {
            if let Some(session) = &self.session
                && let Some(tail) = find_usage(&self.home, session)?
            {
                self.tail = Some(tail);
            }
            if self.tail.is_none() {
                self.dirty = false;
            }
        }
        let previous = self.detector.responses;
        let previous_invalid = self.detector.invalid_records;
        if self.dirty
            && let Some(tail) = &mut self.tail
        {
            // At most 1 MiB per poll, using the existing bounded line reader.
            // This also catches up with a resumed history without loading it.
            for _ in 0..64 {
                let read = tail.read_available()?;
                if read == 0 {
                    self.dirty = false;
                    break;
                }
                self.observed_bytes += read as u64;
                while let Some(line) = tail.take_line(false) {
                    let Line::Text(line) = line else { continue };
                    if line.contains("\"response_item\"")
                        && !line.contains("\"type\":\"reasoning\"")
                        && let Ok(event) = serde_json::from_str::<Value>(&line)
                        && let Some(text) = visible_progress(&event)
                    {
                        if self.handoff.len() == 8 {
                            self.handoff.pop_front();
                        }
                        self.handoff.push_back(text);
                    }
                    if !line.contains("\"token_usage_record\"") {
                        continue;
                    }
                    let Ok(event) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if event["type"] != "token_usage_record"
                        || event["payload"]["thread_id"].as_str() != self.session.as_deref()
                    {
                        continue;
                    }
                    let Some(timestamp) = event["timestamp"]
                        .as_str()
                        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                    else {
                        continue;
                    };
                    let historical = timestamp.timestamp_millis() < self.started_ms as i64;
                    if let Some(mut loss) = self.detector.observe(&event["payload"], historical) {
                        loss.1["session"] = json!(self.session);
                        loss.1["handoff"] = json!(self.handoff);
                        // Stop first: receipt locks must not delay containment.
                        return Ok(Some(loss));
                    }
                }
            }
        }
        if self.detector.invalid_records != previous_invalid {
            self.unavailable = true;
            let reason = "invalid per-response cache counters; consecutive loss count reset and protection is unverified until valid usage resumes";
            self.record("unavailable", Some(reason))?;
            println!("cache guard: {reason}");
        } else if self.detector.responses != previous {
            self.record(
                if self.detector.warm {
                    "armed"
                } else {
                    "warming"
                },
                None,
            )?;
            if previous == 0 || self.unavailable {
                println!("cache guard: exact-session per-response usage is available");
                self.unavailable = false;
            }
        } else if self.detector.responses == 0
            && !self.unavailable
            && now.duration_since(self.started) >= Duration::from_secs(30)
        {
            self.unavailable = true;
            let reason = "no valid exact-session per-response usage observed; cache-loss protection is unverified";
            self.record("unavailable", Some(reason))?;
            println!("cache guard: {reason}");
        }
        Ok(None)
    }
}

/// Bounded visible evidence only. Opaque reasoning is neither read nor
/// decoded, and a tool output is never turned into an instruction or success.
fn visible_progress(event: &Value) -> Option<String> {
    if event["type"] != "response_item" {
        return None;
    }
    let payload = &event["payload"];
    let text = match payload["type"].as_str()? {
        "message" if payload["role"] == "assistant" => {
            let text = payload["content"]
                .as_array()?
                .iter()
                .filter_map(|item| item["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            format!("Assistant report (unverified): {text}")
        }
        "function_call" => format!(
            "Tool request {}: {}",
            payload["name"].as_str()?,
            payload["arguments"].as_str()?
        ),
        "custom_tool_call" => format!(
            "Tool request {}: {}",
            payload["name"].as_str()?,
            payload["input"].as_str()?
        ),
        "function_call_output" | "custom_tool_call_output" => format!(
            "Tool output (may be partial): {}",
            payload["output"].as_str()?
        ),
        _ => return None,
    };
    if text.chars().count() <= 1000 {
        Some(text)
    } else {
        let tail: String = text
            .chars()
            .rev()
            .take(480)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        Some(format!(
            "{}\n[…omitted…]\n{tail}",
            super::excerpt(&text, 480)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_monitor_ignores_foreign_usage_and_retains_only_visible_progress() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let receipt = root.path().join("receipt.json");
        fs::write(&receipt, r#"{"modelProvider":"deepseek"}"#).unwrap();
        let mut monitor = Monitor::new(&receipt, root.path()).unwrap().unwrap();
        let path = root.path().join("sessions/rollout-fixture-session-a.jsonl");
        let mut writer = fs::File::create(path).unwrap();
        writeln!(
            writer,
            "{}",
            json!({"type":"session_meta","payload":{"id":"session-a"}})
        )
        .unwrap();
        for (thread, input, cached, total) in [
            ("session-a", 200_000, 199_000, 200_000),
            ("session-b", 200_000, 0, 400_000),
            ("session-b", 200_000, 0, 600_000),
            ("session-b", 200_000, 0, 800_000),
        ] {
            let mut payload = usage(input, cached, total);
            payload["thread_id"] = json!(thread);
            writeln!(writer, "{}", json!({"type":"token_usage_record","timestamp":chrono::DateTime::from_timestamp_millis(now_ms() as i64).unwrap().to_rfc3339(),"payload":payload})).unwrap();
        }
        assert!(monitor.poll(Some("session-a"), false).unwrap().is_none());
        assert_eq!(monitor.detector.responses, 1);
        assert_eq!(monitor.detector.consecutive, 0);
        let consumed = monitor.observed_bytes;
        while monitor.changes.take().unwrap() {}
        monitor.next_reconcile = Instant::now();
        assert!(monitor.poll(Some("session-a"), false).unwrap().is_none());
        assert_eq!(monitor.observed_bytes, consumed);
        assert!(!monitor.dirty);
        for payload in [
            json!({"type":"reasoning","encrypted_content":"opaque-secret"}),
            json!({"type":"message","role":"assistant","content":[{"text":"Implementation done; tests still running."}]}),
            json!({"type":"custom_tool_call_output","output":"build interrupted; outcome unknown"}),
        ] {
            writeln!(
                writer,
                "{}",
                json!({"type":"response_item","payload":payload})
            )
            .unwrap();
        }
        for total in [400_000, 600_000, 800_000] {
            let mut payload = usage(200_000, 0, total);
            payload["thread_id"] = json!("session-a");
            writeln!(writer, "{}", json!({"type":"token_usage_record","timestamp":chrono::DateTime::from_timestamp_millis(now_ms() as i64).unwrap().to_rfc3339(),"payload":payload})).unwrap();
        }
        // Discard kernel notifications deliberately: the sparse open-handle
        // size check must still catch readable writes before writer closure.
        while monitor.changes.take().unwrap() {}
        monitor.next_reconcile = Instant::now();
        let until = Instant::now() + Duration::from_secs(3);
        let loss = loop {
            if let Some(loss) = monitor.poll(Some("session-a"), false).unwrap() {
                break loss;
            }
            assert!(
                Instant::now() < until,
                "no cache loss detected while writer is open"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let handoff = loss.1["handoff"].as_array().unwrap();
        assert_eq!(handoff.len(), 2);
        assert!(handoff[0].as_str().unwrap().contains("tests still running"));
        assert!(handoff[1].as_str().unwrap().contains("outcome unknown"));
        assert!(!loss.1.to_string().contains("opaque-secret"));
    }

    #[test]
    fn cache_notification_discovers_a_new_file_before_the_writer_closes() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let changes = Changes::new(root.path()).unwrap();
        assert!(!changes.take().unwrap());
        let mut writer = fs::File::create(root.path().join("live.jsonl")).unwrap();
        writeln!(writer, "first record").unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while !changes.take().unwrap() {
            assert!(
                Instant::now() < until,
                "no notification while writer remains open"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        while changes.take().unwrap() {}
        drop(writer);
    }

    fn usage(input: u64, cached: u64, total: u64) -> Value {
        json!({"usage": {"input_tokens": input, "cached_input_tokens": cached},
            "thread_token_usage": {"input_tokens": total}})
    }

    #[test]
    fn cold_input_and_other_providers_do_not_trip() {
        let mut detector = Detector::default();
        for n in 1..=8 {
            assert!(
                detector
                    .observe(&usage(400_000, 0, n * 400_000), false)
                    .is_none()
            );
        }
        assert!(!detector.warm);
        let root = tempfile::tempdir().unwrap();
        let receipt = root.path().join("receipt.json");
        fs::write(&receipt, r#"{"modelProvider":"other"}"#).unwrap();
        assert!(Monitor::new(&receipt, root.path()).unwrap().is_none());
    }

    #[test]
    fn three_distinct_losses_after_warmup_trip_and_duplicates_do_not_count() {
        let mut detector = Detector::default();
        assert!(
            detector
                .observe(&usage(400_000, 399_000, 400_000), false)
                .is_none()
        );
        let miss = usage(400_000, 6_000, 800_000);
        assert!(detector.observe(&miss, false).is_none());
        for _ in 0..5 {
            assert!(detector.observe(&miss, false).is_none());
        }
        assert_eq!(detector.consecutive, 1);
        assert!(
            detector
                .observe(&usage(400_000, 6_000, 1_200_000), false)
                .is_none()
        );
        let loss = detector
            .observe(&usage(400_000, 6_000, 1_600_000), false)
            .unwrap();
        assert!(loss.0.contains("miss=394000"));
        assert_eq!(detector.responses, 4);
    }

    #[test]
    fn recovery_small_misses_and_invalid_counters_break_the_streak() {
        let mut detector = Detector::default();
        let mut total = 0;
        for (input, cached) in [
            (100_000, 90_000),
            (400_000, 0),
            (400_000, 0),
            (400_000, 390_000),
            (400_000, 0),
            (400_000, 0),
            (90_000, 0),
            (400_000, 0),
            (400_000, 0),
            (100_000, 100_001),
            (400_000, 0),
            (400_000, 0),
        ] {
            total += input;
            assert!(
                detector
                    .observe(&usage(input, cached, total), false)
                    .is_none()
            );
        }
        assert!(
            detector
                .observe(&json!({"usage":{"input_tokens":-1}}), false)
                .is_none()
        );
        assert_eq!(detector.consecutive, 0);
        assert_eq!(detector.invalid_records, 2);
    }

    #[test]
    fn resumed_history_arms_without_replaying_its_old_losses() {
        let mut detector = Detector::default();
        detector.observe(&usage(200_000, 190_000, 200_000), true);
        for n in 2..=10 {
            assert!(
                detector
                    .observe(&usage(200_000, 0, n * 200_000), true)
                    .is_none()
            );
        }
        assert!(detector.warm);
        assert_eq!(detector.consecutive, 0);
        assert!(
            detector
                .observe(&usage(200_000, 0, 2_200_000), false)
                .is_none()
        );
        assert!(
            detector
                .observe(&usage(200_000, 0, 2_400_000), false)
                .is_none()
        );
        assert!(
            detector
                .observe(&usage(200_000, 0, 2_600_000), false)
                .is_some()
        );
    }

    #[test]
    fn rollout_filename_never_establishes_session_identity() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions/2026/01/01");
        fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("rollout-fixture-session-a.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-b\"}}\n",
        )
        .unwrap();
        assert!(find_usage(root.path(), "session-a").unwrap().is_none());
        fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"session-a\"}}\n",
        )
        .unwrap();
        assert!(find_usage(root.path(), "session-a").unwrap().is_some());
        assert!(find_usage(root.path(), "../session-a").is_err());
    }
}

/// The event stream establishes the session identity. A filename only
/// narrows candidates; the rollout's own metadata must match before use.
fn find_usage(home: &Path, session: &str) -> io::Result<Option<SpoolTail>> {
    if session.is_empty()
        || !session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(io::Error::other(
            "cache guard received an invalid native session id",
        ));
    }
    let mut pending = vec![(home.join("sessions"), 0)];
    let mut visited = 0;
    while let Some((directory, depth)) = pending.pop() {
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            visited += 1;
            if visited > 20_000 {
                return Err(io::Error::other(
                    "cache guard session discovery exceeded 20000 entries",
                ));
            }
            let kind = entry.file_type()?;
            if kind.is_dir() && depth < 3 {
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(&format!("-{session}.jsonl"))
            {
                let mut tail = SpoolTail::new(fs::File::open(entry.path())?);
                for _ in 0..33 {
                    if tail.read_available()? == 0 {
                        break;
                    }
                    if let Some(line) = tail.take_line(false) {
                        if let Line::Text(line) = line
                            && let Ok(meta) = serde_json::from_str::<Value>(&line)
                            && meta["type"] == "session_meta"
                            && meta["payload"]["id"] == session
                        {
                            // Reopen at zero so bytes buffered past the header
                            // are processed by the normal incremental reader.
                            return Ok(Some(SpoolTail::new(fs::File::open(entry.path())?)));
                        }
                        break;
                    }
                }
            }
        }
    }
    Ok(None)
}
