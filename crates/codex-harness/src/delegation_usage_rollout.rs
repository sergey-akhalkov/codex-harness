//! Allowlisted visible telemetry only; reasoning/encrypted state is ignored.
use super::*;
use std::io::{BufRead, BufReader};

#[derive(Default)]
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
    counts: BTreeMap<&'static str, u64>,
    responses: BTreeMap<String, Usage>,
    conflicts: BTreeSet<String>,
    children: BTreeSet<String>,
    durations: Vec<u64>,
    stamps: Vec<Value>,
    triggers: Vec<Value>,
    turns: Vec<Value>,
    users: Vec<Value>,
    usage: Usage,
    previous: Option<Usage>,
    saw_meta: bool,
}

impl Reader {
    fn bump(&mut self, key: &'static str) {
        *self.counts.entry(key).or_default() += 1;
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
            return;
        }
        let Some(kind) = event["type"].as_str() else {
            self.warn("invalid_event_type");
            return;
        };
        if ![
            "session_meta",
            "turn_context",
            "event_msg",
            "token_usage_record",
            "response_item",
            "compacted",
        ]
        .contains(&kind)
        {
            return;
        }
        let p = &event["payload"];
        if !p.is_object() {
            self.warn("invalid_payload");
            return;
        }
        if kind == "session_meta" {
            self.saw_meta = true;
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
            if let Some(model) = identifier(&p["model"]) {
                self.models.insert(model);
            } else {
                self.warn("missing_model_context");
            }
            let effort = p.get("effort").unwrap_or(&p["reasoning_effort"]);
            if let Some(e) = effort.as_str().filter(|v| {
                ["none", "minimal", "low", "medium", "high", "xhigh", "max"].contains(v)
            }) {
                self.efforts.insert(e.to_owned());
            } else {
                self.warn("missing_reasoning_context");
            }
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
            self.usage = next;
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
            if let Some(id) = identifier(&p["response_id"]) {
                if let Some(previous) = self.responses.get(&id) {
                    if previous != &next {
                        self.responses.insert(id.clone(), unknown_usage());
                        self.conflicts.insert(id);
                        self.warn("conflicting_response_id");
                    }
                } else {
                    self.responses.insert(id, next.clone());
                    Self::stamp(&mut self.turns, &event);
                }
            } else {
                self.warn("missing_response_id");
            }
            self.check_usage(&next);
            if next["reasoning_output_tokens"]
                .zip(next["output_tokens"])
                .is_some_and(|(r, o)| r > o)
            {
                self.warn("reasoning_not_included_in_output");
            }
        } else if kind == "response_item" {
            if p["type"] == "message" {
                let text = message_text(p);
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
                        *self.counts.entry("hook_chars").or_default() +=
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

    fn finish(mut self) -> (Value, Value, BTreeSet<String>) {
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
        }
        if self.parents.len() > 1 {
            self.warn("conflicting_parent_ids");
        }
        let model = only(&self.models);
        let mut provider = match model.as_deref() {
            Some("gpt-6-astra" | "openai/gpt-6-astra") => Some("OpenAI"),
            Some("xai/grok-4.6" | "grok-4.6") => Some("xai"),
            _ => None,
        };
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
        (row, fingerprint, self.warnings)
    }
}

pub(super) fn read(path: &Path) -> (Value, Value, BTreeSet<String>, Option<File>) {
    let mut reader = Reader {
        usage: unknown_usage(),
        ..Reader::default()
    };
    let mut held = None;
    match File::open(path) {
        Ok(file) => {
            let mut stream = BufReader::new(file);
            let mut line = Vec::new();
            loop {
                line.clear();
                // A malformed giant line cannot force unbounded allocation.
                let count = (&mut stream)
                    .take(32 * 1024 * 1024 + 1)
                    .read_until(b'\n', &mut line);
                match count {
                    Ok(0) => break,
                    Ok(_) if line.len() > 32 * 1024 * 1024 => {
                        reader.warn("oversized_jsonl_record");
                        if line.last() != Some(&b'\n') && stream.skip_until(b'\n').is_err() {
                            reader.warn("unreadable_input");
                            break;
                        }
                        continue;
                    }
                    Ok(_) => {}
                    Err(_) => {
                        reader.warn("unreadable_input");
                        break;
                    }
                }
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                match serde_json::from_slice(&line) {
                    Ok(event) => reader.event(event),
                    Err(_) => reader.warn("corrupt_jsonl"),
                }
            }
            held = Some(stream.into_inner());
        }
        Err(_) => reader.warn("unreadable_input"),
    }
    let (row, fingerprint, warnings) = reader.finish();
    (row, fingerprint, warnings, held)
}
