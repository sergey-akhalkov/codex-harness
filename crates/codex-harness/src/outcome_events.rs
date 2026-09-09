//! Incremental observation of visible `codex exec --json` events.
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const RECORD_LIMIT: usize = 32 * 1024 * 1024;
const ID_LIMIT: usize = 128;
const CHILD_LIMIT: usize = 4096;

pub(super) struct Events {
    source: File,
    observed: File,
    evidence: String,
    pending: Vec<u8>,
    skipping: bool,
    policy: Option<Regex>,
    policy_text: Option<String>,
    pub value: Value,
    pub errors: BTreeSet<String>,
}

impl Events {
    pub fn open(source: &Path, observed: &Path, pattern: Option<&str>) -> io::Result<Self> {
        let policy = pattern
            .map(|s| {
                regex::RegexBuilder::new(s)
                    .case_insensitive(true)
                    .size_limit(1024 * 1024)
                    .build()
            })
            .transpose()
            .map_err(|_| io::Error::other("invalid useful-command policy"))?;
        Ok(Self {
            source: File::open(source)?,
            observed: OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(observed)?,
            evidence: observed.to_string_lossy().into_owned(),
            pending: Vec::new(),
            skipping: false,
            policy,
            policy_text: pattern.map(str::to_owned),
            value: json!({"thread_id":null,"children":[],"first_useful_signal":null}),
            errors: BTreeSet::new(),
        })
    }

    /// Limit each poll's work so continuous output cannot starve cancellation.
    pub fn poll(&mut self, final_read: bool) -> io::Result<()> {
        let mut bytes = [0_u8; 64 * 1024];
        let mut read = 0_usize;
        loop {
            let count = self.source.read(&mut bytes)?;
            if count == 0 {
                break;
            }
            read += count;
            for byte in &bytes[..count] {
                if *byte == b'\n' {
                    if !self.skipping {
                        let pending = std::mem::take(&mut self.pending);
                        self.record(&pending)?;
                    }
                    self.pending.clear();
                    self.skipping = false;
                } else if !self.skipping {
                    if self.pending.len() == RECORD_LIMIT {
                        self.pending.clear();
                        self.skipping = true;
                        self.errors.insert("oversized_native_event".into());
                    } else {
                        self.pending.push(*byte);
                    }
                }
            }
            if !final_read && read >= 512 * 1024 {
                break;
            }
        }
        if final_read && (!self.pending.is_empty() || self.skipping) {
            self.errors.insert("truncated_native_event".into());
            self.pending.clear();
            self.skipping = false;
        }
        self.observed.flush()
    }

    fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        let Ok(event) = serde_json::from_slice::<Value>(bytes) else {
            self.errors.insert("malformed_native_event".into());
            return Ok(());
        };
        if !event.is_object() {
            self.errors.insert("malformed_native_event".into());
            return Ok(());
        }
        let at = now();
        serde_json::to_writer(&mut self.observed, &json!({"at":at,"event":event}))?;
        self.observed.write_all(b"\n")?;
        self.observe(&event, at);
        Ok(())
    }

    fn observe(&mut self, event: &Value, at: f64) {
        if event["type"] == "thread.started" {
            if let Some(id) = identifier(&event["thread_id"]) {
                if !self.value["thread_id"].is_null() && self.value["thread_id"] != id {
                    self.errors.insert("conflicting_thread_identity".into());
                } else {
                    self.value["thread_id"] = json!(id);
                }
            } else {
                self.errors.insert("invalid_thread_identity".into());
            }
        }
        if event["type"] == "turn.completed" {
            self.value["turn_completed"] = json!(true);
        }
        if event["type"] == "turn.failed" || event["type"] == "error" {
            self.errors.insert("native_error_event".into());
        }
        let item = &event["item"];
        if event["type"] == "item.completed"
            && item["type"] == "command_execution"
            && (item["exit_code"].is_i64() || item["exit_code"].is_u64())
        {
            let signal = json!({"at":at,"kind":"command_result",
                "item_id":identifier(&item["id"]),"evidence":self.evidence,
                "timestamp_scope":"observed on receipt; target polling interval 50ms"});
            if self.value["first_command_result"].is_null() {
                self.value["first_command_result"] = signal.clone();
            }
            if self.value["first_useful_signal"].is_null()
                && self.policy.as_ref().is_some_and(|policy| {
                    item["command"].as_str().is_some_and(|s| policy.is_match(s))
                })
            {
                self.value["first_useful_signal"] = signal;
                self.value["first_useful_signal"]["policy"] = json!(self.policy_text);
            }
        }
        if item["type"] == "collab_tool_call" {
            if let Some(ids) = item["receiver_thread_ids"].as_array() {
                for id in ids {
                    let Some(id) = identifier(id) else {
                        self.errors.insert("invalid_child_identity".into());
                        continue;
                    };
                    let children = self.value["children"].as_array_mut().expect("owned array");
                    if !children.iter().any(|v| v == id) {
                        if children.len() == CHILD_LIMIT {
                            self.errors.insert("child_identity_limit".into());
                            break;
                        }
                        children.push(json!(id));
                    }
                }
            } else {
                self.errors.insert("invalid_child_identity".into());
            }
        }
    }
}

fn identifier(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| {
        !s.is_empty()
            && s.len() <= ID_LIMIT
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    })
}

pub(super) fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, File, Events) {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("events.jsonl");
        let writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&source)
            .unwrap();
        let reader = Events::open(
            &source,
            &root.path().join("observed.jsonl"),
            Some("^verify$"),
        )
        .unwrap();
        (root, writer, reader)
    }

    #[test]
    fn partial_utf8_and_json_lines_wait_for_completion() {
        let (_root, mut writer, mut reader) = fixture();
        let line = "{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"command\":\"verify\",\"exit_code\":1,\"id\":\"check\",\"output\":\"проверка\"}}\n";
        let split = line.find('п').unwrap() + 1;
        writer.write_all(&line.as_bytes()[..split]).unwrap();
        writer.flush().unwrap();
        reader.poll(false).unwrap();
        assert!(reader.value["first_useful_signal"].is_null());
        assert!(reader.errors.is_empty());
        writer.write_all(&line.as_bytes()[split..]).unwrap();
        writer.flush().unwrap();
        reader.poll(true).unwrap();
        assert_eq!(reader.value["first_useful_signal"]["item_id"], "check");
        assert!(reader.errors.is_empty());
    }

    #[test]
    fn invalid_counter_types_and_noncompletion_do_not_invent_signals() {
        let (_root, _writer, mut reader) = fixture();
        for code in [
            Value::Null,
            json!(true),
            json!("0"),
            json!(0.5),
            json!([]),
            json!({}),
        ] {
            reader.observe(&json!({"type":"item.completed","item":{"type":"command_execution","command":"verify","exit_code":code}}),1.0);
        }
        reader.observe(&json!({"type":"item.started","item":{"type":"command_execution","command":"verify","exit_code":0}}),2.0);
        assert!(reader.value["first_useful_signal"].is_null());
        assert!(reader.value["first_command_result"].is_null());
        reader.observe(&json!({"type":"item.completed","item":{"type":"command_execution","command":"verify","exit_code":7}}),3.0);
        assert_eq!(reader.value["first_useful_signal"]["at"], 3.0);
    }

    #[test]
    fn oversized_and_malformed_records_preserve_the_next_visible_event() {
        let (_root, mut writer, mut reader) = fixture();
        writer.write_all(&vec![b'x'; RECORD_LIMIT + 1]).unwrap();
        writer
            .write_all(b"\ninvalid\n[]\n{\"type\":\"turn.completed\"}\n")
            .unwrap();
        writer.flush().unwrap();
        reader.poll(true).unwrap();
        assert!(reader.errors.contains("oversized_native_event"));
        assert!(reader.errors.contains("malformed_native_event"));
        assert_eq!(reader.value["turn_completed"], true);
    }
}
