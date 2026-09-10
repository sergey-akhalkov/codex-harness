//! Bounded event and documentation evidence for the native outcome oracle.
//!
//! A successful skill signal is only a successful completed tool-input path
//! reference to `<name>/SKILL.md`. That is not proof that the skill body was
//! read or that its instructions were applied.
//!
//! Visible native `item.completed` `command_execution` / `mcp_tool_call` items
//! are retained. Opaque reasoning and compaction payloads are never decoded.
//! Observation errors stay as static labels plus record indexes and never
//! include file contents or private event text.
//!
//! Native command success is `status == "completed"` together with a typed
//! integer zero `exit_code`. Boolean, null, and non-integer codes are not a
//! natural zero. `failed`, `declined`, and `in_progress` stay unsuccessful
//! even with zero. Native MCP success is `status == "completed"` with no
//! `error`. Extra `result.isError`, if present, is a recognized error and is
//! not a native exec result field.
//!
//! Relevant records missing native required fields, or with wrong-typed
//! `command` / `aggregated_output`, are incomplete observations. They may be
//! retained for diagnosis; success helpers do not stringify those fields.

use harness_core::build_identity::{hash_bytes, ordinary};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

const DOCUMENT_FILE_LIMIT: u64 = 1024 * 1024;
const DOCUMENT_TOTAL_LIMIT: u64 = 8 * 1024 * 1024;
const DOCUMENT_COUNT_LIMIT: usize = 1024;
const DIRECTORY_ENTRY_LIMIT: usize = 8192;
const EVENT_RECORD_LIMIT: usize = 8 * 1024 * 1024;
const EVENT_TOTAL_LIMIT: u64 = 64 * 1024 * 1024;
const EVENT_ROW_LIMIT: usize = 100_000;
const DOCUMENT_HOMES: [&str; 3] = ["docs", "doc", ".notes"];

pub(crate) struct Evidence {
    pub items: Vec<Value>,
    pub documentation: String,
    pub updated_documentation: String,
    pub errors: Vec<String>,
}

/// Collect completed tool items and documentation from an owned evidence root.
/// Missing `events.jsonl` is an incomplete observation, not an empty success.
/// Documentation strings come from one bounded read per selected file.
pub(crate) fn collect(
    evidence_root: &Path,
    workspace: &Path,
    previous_documents: &BTreeMap<String, String>,
) -> io::Result<Evidence> {
    let mut errors = Vec::new();
    let items = collect_items(evidence_root, &mut errors)?;
    let (documentation, updated_documentation) =
        collect_documentation(workspace, previous_documents, &mut errors)?;
    Ok(Evidence {
        items,
        documentation,
        updated_documentation,
        errors,
    })
}

pub(crate) fn tool_input(item: &Value) -> String {
    let command = item.get("command").and_then(Value::as_str).unwrap_or("");
    let arguments = match item.get("arguments") {
        None | Some(Value::Null) => "{}".to_string(),
        Some(value) => value.to_string(),
    };
    command.to_owned() + &arguments
}

/// True when a successful completed item's tool-input contains a path reference
/// to `<name>/SKILL.md`. This is not evidence of reading or applying the skill.
pub(crate) fn referenced_skill(items: &[Value], name: &str) -> bool {
    let Ok(pattern) = Regex::new(&format!(
        r"(?i)[/\\]+{}[/\\]+SKILL\.md",
        regex::escape(name)
    )) else {
        return false;
    };
    items
        .iter()
        .any(|item| successful_reference(item) && pattern.is_match(&tool_input(item)))
}

pub(crate) fn successful_command(items: &[Value], command: &Regex, output: Option<&Regex>) -> bool {
    items.iter().any(|item| {
        item_type(item) == Some("command_execution")
            && command_succeeded(item)
            && item
                .get("command")
                .and_then(Value::as_str)
                .is_some_and(|text| command.is_match(text))
            && output.is_none_or(|pattern| {
                item.get("aggregated_output")
                    .and_then(Value::as_str)
                    .is_some_and(|text| pattern.is_match(text))
            })
    })
}

fn collect_items(evidence_root: &Path, errors: &mut Vec<String>) -> io::Result<Vec<Value>> {
    let path = evidence_root.join("events.jsonl");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            errors.push("missing_event_log".into());
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
        Ok(_) => ordinary(&path)?,
    }
    let mut reader = File::open(&path)?;
    let mut items = Vec::new();
    let mut pending = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    let mut index = 0_usize;
    let mut skipping = false;
    let mut row_limit = false;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        for byte in &buffer[..count] {
            total = total
                .checked_add(1)
                .filter(|value| *value <= EVENT_TOTAL_LIMIT)
                .ok_or_else(|| io::Error::other("event log exceeds the native total bound"))?;
            if *byte == b'\n' {
                finish_record(
                    &mut pending,
                    &mut skipping,
                    &mut row_limit,
                    index,
                    &mut items,
                    errors,
                );
                index += 1;
                continue;
            }
            if skipping || row_limit {
                continue;
            }
            if pending.len() == EVENT_RECORD_LIMIT {
                pending.clear();
                skipping = true;
                continue;
            }
            pending.push(*byte);
        }
    }
    if skipping || !pending.is_empty() {
        errors.push(format!("truncated_event_record:{index}"));
    }
    Ok(items)
}

fn finish_record(
    pending: &mut Vec<u8>,
    skipping: &mut bool,
    row_limit: &mut bool,
    index: usize,
    items: &mut Vec<Value>,
    errors: &mut Vec<String>,
) {
    if index >= EVENT_ROW_LIMIT {
        if !*row_limit {
            errors.push(format!("event_row_limit:{index}"));
            *row_limit = true;
        }
        pending.clear();
        *skipping = false;
        return;
    }
    if *skipping {
        errors.push(format!("oversized_event_record:{index}"));
        pending.clear();
        *skipping = false;
        return;
    }
    if pending.last() == Some(&b'\r') {
        pending.pop();
    }
    if !pending.is_empty() {
        observe_record(pending, index, items, errors);
    }
    pending.clear();
}

fn observe_record(bytes: &[u8], index: usize, items: &mut Vec<Value>, errors: &mut Vec<String>) {
    let Ok(row) = serde_json::from_slice::<Value>(bytes) else {
        errors.push(format!("malformed_event_record:{index}"));
        return;
    };
    let Some(object) = row.as_object() else {
        errors.push(format!("malformed_event_record:{index}"));
        return;
    };
    if object.get("type").and_then(Value::as_str) != Some("item.completed") {
        return;
    }
    let Some(item) = object.get("item") else {
        errors.push(format!("incomplete_event_record:{index}"));
        return;
    };
    if !item.is_object() {
        errors.push(format!("incomplete_event_record:{index}"));
        return;
    }
    match item.get("type").and_then(Value::as_str) {
        Some("command_execution") => {
            if !command_shape(item) {
                errors.push(format!("incomplete_event_record:{index}"));
            }
            items.push(item.clone());
        }
        Some("mcp_tool_call") => {
            if !mcp_shape(item) {
                errors.push(format!("incomplete_event_record:{index}"));
            }
            items.push(item.clone());
        }
        _ => {}
    }
}

fn collect_documentation(
    workspace: &Path,
    previous: &BTreeMap<String, String>,
    errors: &mut Vec<String>,
) -> io::Result<(String, String)> {
    ordinary(workspace)?;
    let mut ordered = Vec::new();
    push_document_path(workspace, Path::new("README.md"), &mut ordered, errors)?;
    push_document_path(workspace, Path::new("outcome.json"), &mut ordered, errors)?;
    let mut markdown = Vec::new();
    for home in DOCUMENT_HOMES {
        collect_markdown_tree(workspace, Path::new(home), &mut markdown, errors)?;
    }
    markdown.sort();
    markdown.dedup();
    for relative in markdown {
        if relative != "README.md" {
            ordered.push(relative);
        }
    }
    let mut current = Vec::new();
    let mut updated = Vec::new();
    let mut total = 0_u64;
    for relative in ordered {
        if current.len() >= DOCUMENT_COUNT_LIMIT {
            errors.push("document_count_limit".into());
            break;
        }
        let path = workspace.join(&relative);
        match read_workspace_document(workspace, &path) {
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                errors.push(format!("incomplete_document:{relative}"));
                continue;
            }
            Err(error) => return Err(error),
            Ok(None) => continue,
            Ok(Some(bytes)) => {
                let size = bytes.len() as u64;
                if total.saturating_add(size) > DOCUMENT_TOTAL_LIMIT {
                    errors.push(format!("document_total_limit:{relative}"));
                    break;
                }
                total += size;
                let digest = hash_bytes(&bytes);
                if std::str::from_utf8(&bytes).is_err() {
                    errors.push(format!("invalid_document_encoding:{relative}"));
                }
                let text = String::from_utf8_lossy(&bytes).into_owned();
                current.push(text.clone());
                if relative.ends_with(".md") && previous.get(&relative) != Some(&digest) {
                    updated.push(text);
                }
            }
        }
    }
    Ok((current.join("\n"), updated.join("\n")))
}

fn push_document_path(
    workspace: &Path,
    relative: &Path,
    paths: &mut Vec<String>,
    errors: &mut Vec<String>,
) -> io::Result<()> {
    let path = workspace.join(relative);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(meta) => {
            if let Err(error) = ordinary(&path) {
                if error.kind() == io::ErrorKind::Other {
                    errors.push(format!("incomplete_document:{}", relative_key(relative)));
                    return Ok(());
                }
                return Err(error);
            }
            if meta.is_file() {
                paths.push(relative_key(relative));
            }
            Ok(())
        }
    }
}

fn collect_markdown_tree(
    workspace: &Path,
    relative: &Path,
    paths: &mut Vec<String>,
    errors: &mut Vec<String>,
) -> io::Result<()> {
    let root = workspace.join(relative);
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
        Ok(_) => {
            if let Err(error) = ordinary(&root) {
                if error.kind() == io::ErrorKind::Other {
                    errors.push(format!("incomplete_document:{}", relative_key(relative)));
                    return Ok(());
                }
                return Err(error);
            }
        }
    }
    if !root.is_dir() {
        return Ok(());
    }
    let mut pending = vec![relative.to_path_buf()];
    let mut visited_entries = 0;
    while let Some(current) = pending.pop() {
        if current.components().count() > 32 {
            errors.push("document_directory_depth_limit".into());
            continue;
        }
        if paths.len() >= DOCUMENT_COUNT_LIMIT {
            if !errors.iter().any(|error| error == "document_count_limit") {
                errors.push("document_count_limit".into());
            }
            break;
        }
        let directory = workspace.join(&current);
        if let Err(error) = ordinary(&directory) {
            if error.kind() == io::ErrorKind::Other {
                errors.push(format!("incomplete_document:{}", relative_key(&current)));
                continue;
            }
            return Err(error);
        }
        let mut entries: Vec<PathBuf> = fs::read_dir(&directory)?
            .take(DIRECTORY_ENTRY_LIMIT - visited_entries + 1)
            .map(|entry| entry.map(|item| item.path()))
            .collect::<io::Result<_>>()?;
        visited_entries += entries.len();
        if visited_entries > DIRECTORY_ENTRY_LIMIT {
            errors.push("document_directory_entry_limit".into());
            break;
        }
        entries.sort();
        for path in entries {
            let Some(name) = path.file_name() else {
                continue;
            };
            let child = current.join(name);
            let meta = match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
                Ok(meta) => meta,
            };
            if let Err(error) = ordinary(&path) {
                if error.kind() == io::ErrorKind::Other {
                    errors.push(format!("incomplete_document:{}", relative_key(&child)));
                    continue;
                }
                return Err(error);
            }
            if meta.is_dir() {
                pending.push(child);
            } else if meta.is_file() && path.extension().is_some_and(|ext| ext == "md") {
                if paths.len() >= DOCUMENT_COUNT_LIMIT {
                    if !errors.iter().any(|error| error == "document_count_limit") {
                        errors.push("document_count_limit".into());
                    }
                    break;
                }
                paths.push(relative_key(&child));
            }
        }
    }
    Ok(())
}

fn read_workspace_document(workspace: &Path, path: &Path) -> io::Result<Option<Vec<u8>>> {
    ordinary(path)?;
    if !path.is_file() {
        return Ok(None);
    }
    scoped_workspace_path(workspace, path)?;
    let mut bytes = Vec::new();
    File::open(path)?
        .take(DOCUMENT_FILE_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > DOCUMENT_FILE_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "document exceeds the native file bound",
        ));
    }
    scoped_workspace_path(workspace, path)?;
    ordinary(path)?;
    Ok(Some(bytes))
}

fn scoped_workspace_path(workspace: &Path, path: &Path) -> io::Result<()> {
    let workspace = fs::canonicalize(workspace)?;
    let resolved = fs::canonicalize(path)?;
    if !resolved.starts_with(&workspace) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "document is outside the workspace",
        ));
    }
    Ok(())
}

fn relative_key(path: &Path) -> String {
    path.components()
        .filter_map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn item_type(item: &Value) -> Option<&str> {
    item.get("type").and_then(Value::as_str)
}

fn integer_exit_code(item: &Value) -> Option<i64> {
    match item.get("exit_code") {
        Some(Value::Number(number)) if number.is_i64() || number.is_u64() => number
            .as_i64()
            .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok())),
        _ => None,
    }
}

fn command_shape(item: &Value) -> bool {
    item.get("id").and_then(Value::as_str).is_some()
        && matches!(
            item.get("status").and_then(Value::as_str),
            Some("in_progress" | "completed" | "failed" | "declined")
        )
        && item.get("command").and_then(Value::as_str).is_some()
        && item
            .get("aggregated_output")
            .and_then(Value::as_str)
            .is_some()
        && match item.get("exit_code") {
            Some(Value::Null) => true,
            Some(Value::Number(number)) => number
                .as_i64()
                .is_some_and(|code| i32::try_from(code).is_ok()),
            _ => false,
        }
}

fn mcp_shape(item: &Value) -> bool {
    item.get("id").and_then(Value::as_str).is_some()
        && item.get("server").and_then(Value::as_str).is_some()
        && item.get("tool").and_then(Value::as_str).is_some()
        && matches!(item.get("status").and_then(Value::as_str), Some("in_progress" | "completed" | "failed"))
        // Native arguments are serde_json::Value with a default, not object-only.
        && match item.get("result") {
            None | Some(Value::Null) => true,
            Some(Value::Object(result)) => result.get("content").is_some_and(Value::is_array),
            _ => false,
        }
        && match item.get("error") {
            None | Some(Value::Null) => true,
            Some(Value::Object(error)) => error.get("message").is_some_and(Value::is_string),
            _ => false,
        }
}

fn successful_reference(item: &Value) -> bool {
    match item_type(item) {
        Some("command_execution") => command_succeeded(item),
        Some("mcp_tool_call") => mcp_succeeded(item),
        _ => false,
    }
}

fn command_succeeded(item: &Value) -> bool {
    command_shape(item)
        && item.get("status").and_then(Value::as_str) == Some("completed")
        && integer_exit_code(item) == Some(0)
}

fn mcp_succeeded(item: &Value) -> bool {
    if !mcp_shape(item) {
        return false;
    }
    if item.get("status").and_then(Value::as_str) != Some("completed") {
        return false;
    }
    if item.get("error").is_some_and(|value| !value.is_null()) {
        return false;
    }
    matches!(
        item.pointer("/result/isError"),
        Some(Value::Bool(false)) | Some(Value::Null) | None
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{fs, io::Write};

    fn regex(pattern: &str) -> Regex {
        Regex::new(pattern).unwrap()
    }

    fn write_events(root: &Path, rows: &[&str]) {
        fs::write(root.join("events.jsonl"), rows.join("\n") + "\n").unwrap();
    }

    fn command_item(command: &str, status: &str, exit_code: Value, output: &str) -> Value {
        json!({
            "type": "command_execution",
            "id": "cmd",
            "status": status,
            "command": command,
            "exit_code": exit_code,
            "aggregated_output": output,
        })
    }

    fn mcp_item(status: &str, path: &str, error: Option<Value>) -> Value {
        let mut item = json!({
            "type": "mcp_tool_call",
            "id": "mcp",
            "server": "serena",
            "tool": "read_file",
            "status": status,
            "arguments": {"path": path},
            "result": {"content": []},
        });
        if let Some(error) = error {
            item["error"] = error;
        }
        item
    }

    fn collect_ok(
        evidence: &Path,
        workspace: &Path,
        previous: &BTreeMap<String, String>,
    ) -> Evidence {
        collect(evidence, workspace, previous).unwrap()
    }

    #[test]
    fn successful_and_failed_and_nonnumeric_command_exits() {
        let zero = command_item("cargo test", "completed", json!(0), "ok");
        let failed = command_item("cargo test", "completed", json!(1), "ok");
        let null_code = command_item("cargo test", "completed", Value::Null, "ok");
        let bool_code = command_item("cargo test", "completed", json!(true), "ok");
        let failed_zero = command_item("cargo test", "failed", json!(0), "ok");
        let declined_zero = command_item("cargo test", "declined", json!(0), "ok");
        let in_progress_zero = command_item("cargo test", "in_progress", json!(0), "ok");
        let running_zero = command_item("cargo test", "running", json!(0), "ok");
        let pattern = regex("(?i)cargo test");
        let output = regex("(?i)ok");
        assert!(successful_command(
            std::slice::from_ref(&zero),
            &pattern,
            Some(&output)
        ));
        assert!(!successful_command(&[failed], &pattern, Some(&output)));
        assert!(!successful_command(&[null_code], &pattern, Some(&output)));
        assert!(!successful_command(&[bool_code], &pattern, Some(&output)));
        assert!(!successful_command(&[failed_zero], &pattern, Some(&output)));
        assert!(!successful_command(
            &[declined_zero],
            &pattern,
            Some(&output)
        ));
        assert!(!successful_command(
            &[in_progress_zero],
            &pattern,
            Some(&output)
        ));
        assert!(!successful_command(
            &[running_zero],
            &pattern,
            Some(&output)
        ));
        assert!(!successful_command(
            &[command_item("cargo test", "completed", json!(0), "other")],
            &pattern,
            Some(&output)
        ));
        assert!(!referenced_skill(
            &[command_item(
                r"Get-Content C:/skills/project-verification/SKILL.md",
                "completed",
                json!(true),
                "body"
            )],
            "project-verification"
        ));
        assert!(!referenced_skill(
            &[command_item(
                r"Get-Content C:/skills/project-verification/SKILL.md",
                "failed",
                json!(0),
                "body"
            )],
            "project-verification"
        ));
    }

    #[test]
    fn mcp_completed_with_error_is_not_a_successful_reference() {
        let path = "C:/skills/project-verification/SKILL.md";
        let native = mcp_item("completed", path, None);
        let failed_status = mcp_item("failed", path, None);
        let in_progress = mcp_item("in_progress", path, None);
        let error_field = mcp_item("completed", path, Some(json!("denied")));
        let mut extra_error = mcp_item("completed", path, None);
        extra_error["result"]["isError"] = json!(true);
        assert!(referenced_skill(
            std::slice::from_ref(&native),
            "project-verification"
        ));
        assert_eq!(native["result"].get("isError"), None);
        assert!(!referenced_skill(&[failed_status], "project-verification"));
        assert!(!referenced_skill(&[in_progress], "project-verification"));
        assert!(!referenced_skill(&[error_field], "project-verification"));
        assert!(!referenced_skill(&[extra_error], "project-verification"));
        assert_eq!(
            tool_input(&mcp_item("completed", path, None)),
            r#"{"path":"C:/skills/project-verification/SKILL.md"}"#
        );
    }

    #[test]
    fn native_value_arguments_are_allowed_but_invalid_status_result_and_exit_shapes_are_incomplete()
    {
        let path = "C:/skills/project-verification/SKILL.md";
        let mut native = mcp_item("completed", path, None);
        native["arguments"] = json!([path]);
        assert!(mcp_shape(&native));
        assert!(referenced_skill(&[native.clone()], "project-verification"));
        native["result"] = json!({});
        assert!(!mcp_shape(&native));
        assert!(!referenced_skill(&[native], "project-verification"));
        let bad_status = command_item("cargo test", "running", json!(0), "ok");
        assert!(!command_shape(&bad_status));
        let overflow = command_item("cargo test", "completed", json!(2147483648_u64), "ok");
        assert!(!command_shape(&overflow));
    }

    #[test]
    fn invalid_document_encoding_and_excessive_directory_depth_are_incomplete() {
        let workspace = tempfile::tempdir().unwrap();
        let evidence = tempfile::tempdir().unwrap();
        write_events(evidence.path(), &[]);
        fs::write(workspace.path().join("README.md"), [0xff, b'x']).unwrap();
        let mut nested = workspace.path().join("docs");
        for _ in 0..34 {
            nested.push("x");
        }
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("record.md"), "not reached").unwrap();
        let found = collect_ok(evidence.path(), workspace.path(), &BTreeMap::new());
        assert!(
            found
                .errors
                .contains(&"invalid_document_encoding:README.md".into())
        );
        assert!(
            found
                .errors
                .contains(&"document_directory_depth_limit".into())
        );
        assert!(!found.documentation.contains("not reached"));
    }

    #[test]
    fn only_completed_command_and_mcp_items_are_collected() {
        let root = tempfile::tempdir().unwrap();
        let evidence = root.path().join("evidence");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&evidence).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        write_events(
            &evidence,
            &[
                r#"{"type":"item.started","item":{"type":"command_execution","id":"cmd","status":"in_progress","command":"cargo test","aggregated_output":"","exit_code":0}}"#,
                r#"{"type":"item.completed","item":{"type":"agent_message","id":"msg","text":"used project-verification/SKILL.md"}}"#,
                r#"{"type":"reasoning","text":"{\"type\":\"item.completed\",\"item\":{\"type\":\"command_execution\",\"id\":\"hidden\",\"status\":\"completed\",\"command\":\"hidden\",\"aggregated_output\":\"\",\"exit_code\":0}}"}"#,
                r#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd","status":"completed","command":"cargo test","exit_code":0,"aggregated_output":"ok"}}"#,
                r#"{"type":"item.completed","item":{"type":"mcp_tool_call","id":"mcp","server":"serena","tool":"read_file","status":"completed","arguments":{"path":"C:/skills/project-verification/SKILL.md"},"result":{"content":[]}}}"#,
            ],
        );
        let evidence_result = collect_ok(&evidence, &workspace, &BTreeMap::new());
        assert_eq!(evidence_result.items.len(), 2);
        assert_eq!(evidence_result.items[0]["type"], "command_execution");
        assert_eq!(evidence_result.items[0]["status"], "completed");
        assert_eq!(evidence_result.items[1]["type"], "mcp_tool_call");
        assert!(
            !evidence_result
                .errors
                .iter()
                .any(|error| error.contains("hidden"))
        );
        assert!(referenced_skill(
            &evidence_result.items,
            "project-verification"
        ));
        assert!(successful_command(
            &evidence_result.items,
            &regex("(?i)cargo test"),
            Some(&regex("(?i)^ok$"))
        ));
    }

    #[test]
    fn malformed_truncated_events_salvage_later_valid_records() {
        let root = tempfile::tempdir().unwrap();
        let evidence = root.path().join("evidence");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&evidence).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        let mut file = File::create(evidence.join("events.jsonl")).unwrap();
        file.write_all(&vec![b'x'; EVENT_RECORD_LIMIT + 1]).unwrap();
        file.write_all(b"\nnot-json\n[]\n").unwrap();
        file.write_all(br#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd","status":"completed","command":"cargo test","exit_code":0,"aggregated_output":"ok"}}"#).unwrap();
        file.write_all(b"\n").unwrap();
        file.write_all(br#"{"type":"item.completed"}"#).unwrap();
        file.write_all(b"\nnot-closed").unwrap();
        drop(file);
        let evidence_result = collect_ok(&evidence, &workspace, &BTreeMap::new());
        assert_eq!(evidence_result.items.len(), 1);
        assert!(
            evidence_result
                .errors
                .contains(&"oversized_event_record:0".into())
        );
        assert!(
            evidence_result
                .errors
                .contains(&"malformed_event_record:1".into())
        );
        assert!(
            evidence_result
                .errors
                .contains(&"malformed_event_record:2".into())
        );
        assert!(
            evidence_result
                .errors
                .contains(&"incomplete_event_record:4".into())
        );
        assert!(
            evidence_result
                .errors
                .contains(&"truncated_event_record:5".into())
        );
        assert!(successful_command(
            &evidence_result.items,
            &regex("cargo test"),
            None
        ));
        assert!(
            !evidence_result
                .errors
                .iter()
                .any(|error| error.contains("cargo test"))
        );
        assert!(
            !evidence_result
                .errors
                .iter()
                .any(|error| error.contains("not-json"))
        );
    }

    #[test]
    fn incomplete_native_shapes_are_flagged_and_not_successful() {
        let root = tempfile::tempdir().unwrap();
        let evidence = root.path().join("evidence");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&evidence).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        write_events(
            &evidence,
            &[
                r#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd","status":"completed","command":{"text":"cargo test"},"exit_code":0,"aggregated_output":"ok"}}"#,
                r#"{"type":"item.completed","item":{"type":"mcp_tool_call","id":"mcp","status":"completed","arguments":{"path":"C:/skills/project-verification/SKILL.md"},"result":{"content":[]}}}"#,
                r#"{"type":"item.completed","item":{"type":"command_execution","id":"cmd","status":"completed","command":"cargo test","exit_code":0,"aggregated_output":"ok"}}"#,
            ],
        );
        let evidence_result = collect_ok(&evidence, &workspace, &BTreeMap::new());
        assert_eq!(evidence_result.items.len(), 3);
        assert!(
            evidence_result
                .errors
                .contains(&"incomplete_event_record:0".into())
        );
        assert!(
            evidence_result
                .errors
                .contains(&"incomplete_event_record:1".into())
        );
        let pattern = regex("cargo test");
        assert!(!successful_command(
            &[evidence_result.items[0].clone()],
            &pattern,
            None
        ));
        assert!(!referenced_skill(
            std::slice::from_ref(&evidence_result.items[1]),
            "project-verification"
        ));
        assert!(successful_command(
            &[evidence_result.items[2].clone()],
            &pattern,
            None
        ));
        let object_command = json!({
            "type": "command_execution",
            "id": "cmd",
            "status": "completed",
            "command": {"text": "cargo test"},
            "exit_code": 0,
            "aggregated_output": "ok",
        });
        assert_eq!(tool_input(&object_command), "{}");
        assert!(!successful_command(&[object_command], &pattern, None));
    }

    #[test]
    fn missing_event_log_is_incomplete() {
        let root = tempfile::tempdir().unwrap();
        let evidence = root.path().join("evidence");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&evidence).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        let evidence_result = collect_ok(&evidence, &workspace, &BTreeMap::new());
        assert!(evidence_result.items.is_empty());
        assert!(evidence_result.errors.contains(&"missing_event_log".into()));
    }

    #[test]
    fn updated_docs_exclude_unchanged_markdown_and_outcome_json() {
        let root = tempfile::tempdir().unwrap();
        let evidence = root.path().join("evidence");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&evidence).unwrap();
        fs::create_dir_all(workspace.join("docs/sub")).unwrap();
        fs::create_dir_all(workspace.join("doc")).unwrap();
        fs::create_dir_all(workspace.join(".notes")).unwrap();
        write_events(&evidence, &[]);
        fs::write(workspace.join("README.md"), "# keep\n").unwrap();
        fs::write(workspace.join("outcome.json"), "{\"status\":\"passed\"}\n").unwrap();
        fs::write(workspace.join("docs/current.md"), "current docs\n").unwrap();
        fs::write(workspace.join("docs/sub/nested.md"), "nested\n").unwrap();
        fs::write(workspace.join("doc/guide.md"), "guide\n").unwrap();
        fs::write(workspace.join(".notes/note.md"), "note\n").unwrap();
        fs::write(workspace.join("docs/skip.txt"), "ignored\n").unwrap();
        let mut previous = BTreeMap::new();
        previous.insert(
            "README.md".into(),
            hash_bytes(&fs::read(workspace.join("README.md")).unwrap()),
        );
        previous.insert(
            "docs/current.md".into(),
            hash_bytes(&fs::read(workspace.join("docs/current.md")).unwrap()),
        );
        fs::write(workspace.join("docs/current.md"), "updated current\n").unwrap();
        let evidence_result = collect_ok(&evidence, &workspace, &previous);
        assert!(evidence_result.documentation.contains("# keep"));
        assert!(
            evidence_result
                .documentation
                .contains("{\"status\":\"passed\"}")
        );
        assert!(evidence_result.documentation.contains("updated current"));
        assert!(evidence_result.documentation.contains("nested"));
        assert!(evidence_result.documentation.contains("guide"));
        assert!(evidence_result.documentation.contains("note"));
        assert!(!evidence_result.documentation.contains("ignored"));
        assert!(!evidence_result.updated_documentation.contains("# keep"));
        assert!(
            !evidence_result
                .updated_documentation
                .contains("{\"status\":\"passed\"}")
        );
        assert!(
            evidence_result
                .updated_documentation
                .contains("updated current")
        );
        assert!(evidence_result.updated_documentation.contains("nested"));
        let readme = evidence_result.documentation.find("# keep").unwrap();
        let outcome = evidence_result
            .documentation
            .find("{\"status\":\"passed\"}")
            .unwrap();
        let current = evidence_result
            .documentation
            .find("updated current")
            .unwrap();
        let nested = evidence_result.documentation.find("nested").unwrap();
        assert!(readme < outcome && outcome < current && current < nested);
    }

    #[test]
    fn redirected_missing_and_oversized_inputs_fail_or_stay_incomplete() {
        let root = tempfile::tempdir().unwrap();
        let evidence = root.path().join("evidence");
        let workspace = root.path().join("workspace");
        let foreign = root.path().join("foreign.md");
        fs::create_dir_all(&evidence).unwrap();
        fs::create_dir_all(workspace.join("docs")).unwrap();
        write_events(&evidence, &[]);
        fs::write(&foreign, "secret\n").unwrap();
        std::os::windows::fs::symlink_file(&foreign, workspace.join("README.md")).unwrap();
        fs::write(
            workspace.join("docs/big.md"),
            vec![b'a'; DOCUMENT_FILE_LIMIT as usize + 1],
        )
        .unwrap();
        let evidence_result = collect_ok(&evidence, &workspace, &BTreeMap::new());
        assert!(
            evidence_result
                .errors
                .iter()
                .any(|error| error == "incomplete_document:README.md")
        );
        assert!(
            evidence_result
                .errors
                .iter()
                .any(|error| error == "incomplete_document:docs/big.md")
        );
        assert!(!evidence_result.documentation.contains("secret"));
        assert!(
            !evidence_result
                .errors
                .iter()
                .any(|error| error.contains("secret"))
        );
        assert!(!evidence_result.documentation.contains(&"a".repeat(8)));

        let missing_root = tempfile::tempdir().unwrap();
        let missing_evidence = missing_root.path().join("missing-evidence");
        fs::create_dir_all(&missing_evidence).unwrap();
        let missing = collect(
            &missing_evidence,
            &missing_root.path().join("no-workspace"),
            &BTreeMap::new(),
        );
        assert!(missing.is_err());
    }

    #[test]
    fn escaped_skill_path_is_a_reference_without_claiming_application() {
        let item = command_item(
            r#"Get-Content "C:\\Users\\sample\\.agents\\skills\\project-verification\\SKILL.md""#,
            "completed",
            json!(0),
            "body",
        );
        assert!(referenced_skill(
            std::slice::from_ref(&item),
            "project-verification"
        ));
        assert!(!referenced_skill(&[item], "reproduce-regression"));
    }
}
