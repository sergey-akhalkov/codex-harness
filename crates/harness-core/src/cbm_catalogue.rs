//! Explicit local CBM catalogue input. This parses a saved `cbm-catalogue`
//! JSON report; it does not launch CBM, cache definitions, or certify origin
//! from a self-declared digest.
#![cfg(windows)]

use crate::{
    cbm_index::AUDITED_BUILD, dependency_mcp_probe::strict_json, registration_native::ReadGuard,
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    io::{self, Read},
    path::Path,
};

const BYTE_LIMIT: u64 = 4 * 1024 * 1024;
const PROTOCOL: &str = "2024-11-05";
const SERVER: &str = "codebase-memory-mcp";
const STATE: &str = "catalogue-read";
const KNOWN_TOOLS: [&str; 15] = [
    "index_repository",
    "search_graph",
    "query_graph",
    "trace_path",
    "get_code_snippet",
    "get_graph_schema",
    "get_architecture",
    "search_code",
    "list_projects",
    "delete_project",
    "index_status",
    "check_index_coverage",
    "detect_changes",
    "manage_adr",
    "ingest_traces",
];

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

fn field<'a>(object: &'a serde_json::Map<String, Value>, name: &str) -> io::Result<&'a Value> {
    object
        .get(name)
        .ok_or_else(|| invalid("CBM catalogue field missing"))
}

fn text<'a>(object: &'a serde_json::Map<String, Value>, name: &str) -> io::Result<&'a str> {
    field(object, name)?
        .as_str()
        .ok_or_else(|| invalid("CBM catalogue field incompatible"))
}

fn validate_schema(schema: &Value) -> io::Result<()> {
    if schema["type"] != "object" || !schema["properties"].is_object() {
        return Err(invalid("MCP tool input schema incompatible"));
    }
    if let Some(required) = schema.get("required") {
        let required = required
            .as_array()
            .ok_or_else(|| invalid("MCP required schema incompatible"))?;
        let mut seen = BTreeSet::new();
        for key in required {
            let key = key
                .as_str()
                .ok_or_else(|| invalid("MCP required schema incompatible"))?;
            if schema["properties"].get(key).is_none() || !seen.insert(key) {
                return Err(invalid("MCP required schema incompatible"));
            }
        }
    }
    Ok(())
}

fn validate_definition(tool: &Value, names: &mut BTreeSet<String>) -> io::Result<()> {
    if !tool.is_object() {
        return Err(invalid("MCP invalid tool definition"));
    }
    let name = tool["name"]
        .as_str()
        .filter(|name| !name.is_empty() && name.len() <= 128)
        .ok_or_else(|| invalid("MCP invalid tool name"))?;
    validate_schema(&tool["inputSchema"])?;
    if tool
        .get("description")
        .is_some_and(|value| !value.is_string())
        || tool.get("title").is_some_and(|value| !value.is_string())
        || tool
            .get("annotations")
            .is_some_and(|value| !value.is_object())
    {
        return Err(invalid("MCP invalid tool definition"));
    }
    if let Some(schema) = tool.get("outputSchema") {
        validate_schema(schema)?;
    }
    if !names.insert(name.to_owned()) {
        return Err(invalid("MCP duplicate or excessive tools"));
    }
    Ok(())
}

/// Read complete CBM tool definitions from an explicitly selected local
/// `cbm-catalogue` JSON report. Structural and pin-label checks only; the
/// claimed digest is not origin evidence.
pub fn read(path: &Path) -> std::io::Result<Vec<Value>> {
    let mut guard = ReadGuard::open(path)?;
    let length = guard.file.metadata()?.len();
    if length > BYTE_LIMIT {
        return Err(invalid("CBM catalogue exceeds its byte limit"));
    }
    let mut bytes = Vec::new();
    (&mut guard.file)
        .take(BYTE_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != length {
        return Err(invalid("CBM catalogue truncated"));
    }
    let report = strict_json(&bytes)?;
    let object = report
        .as_object()
        .ok_or_else(|| invalid("CBM catalogue is not an object"))?;
    if text(object, "state")? != STATE
        || text(object, "server")? != SERVER
        || text(object, "protocol_version")? != PROTOCOL
        || text(object, "artifact_sha256")? != AUDITED_BUILD
    {
        return Err(invalid("CBM catalogue pin incompatible"));
    }
    let tools = field(object, "tools")?
        .as_array()
        .ok_or_else(|| invalid("MCP tools catalogue missing"))?;
    let tool_count = field(object, "tool_count")?
        .as_u64()
        .ok_or_else(|| invalid("CBM catalogue tool_count incompatible"))?;
    if tool_count != tools.len() as u64 || tools.len() != KNOWN_TOOLS.len() {
        return Err(invalid("CBM catalogue tool_count incompatible"));
    }
    let mut names = BTreeSet::new();
    for tool in tools {
        validate_definition(tool, &mut names)?;
    }
    if names != KNOWN_TOOLS.iter().copied().map(str::to_owned).collect() {
        return Err(invalid("CBM catalogue names incompatible"));
    }
    Ok(tools.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn report(tools: Vec<Value>) -> Value {
        json!({
            "state": STATE,
            "server": SERVER,
            "protocol_version": PROTOCOL,
            "artifact_sha256": AUDITED_BUILD,
            "tool_count": tools.len(),
            "tools": tools,
        })
    }

    fn definition(name: &str) -> Value {
        json!({
            "name": name,
            "title": format!("{name} 日本"),
            "description": format!("{name} α 😀"),
            "annotations": {"readOnlyHint": true},
            "outputSchema": {"type": "object", "properties": {}},
            "inputSchema": {"type": "object", "properties": {}, "required": []},
        })
    }

    fn known_tools() -> Vec<Value> {
        KNOWN_TOOLS.into_iter().map(definition).collect()
    }

    fn write_report(root: &Path, value: &Value) -> std::path::PathBuf {
        let path = root.join("catalogue.json");
        fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }

    #[test]
    fn utf8_definitions_are_preserved_from_an_ordinary_file() {
        let root = tempfile::tempdir().unwrap();
        let path = write_report(root.path(), &report(known_tools()));
        let before = fs::read(&path).unwrap();
        let tools = read(&path).unwrap();
        assert_eq!(tools.len(), 15);
        assert_eq!(tools[0]["name"], "index_repository");
        assert_eq!(tools[0]["title"], "index_repository 日本");
        assert_eq!(tools[0]["description"], "index_repository α 😀");
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
        assert_eq!(tools[0]["outputSchema"]["type"], "object");
        assert_eq!(tools[0]["outputSchema"]["properties"], json!({}));
        assert_eq!(tools[14]["name"], "ingest_traces");
        assert_eq!(fs::read(&path).unwrap(), before);
        let names: BTreeSet<_> = fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(
            names,
            BTreeSet::from([std::ffi::OsString::from("catalogue.json")])
        );
    }

    #[test]
    fn malformed_truncated_duplicate_and_oversize_inputs_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("catalogue.json");
        fs::write(&path, b"{").unwrap();
        assert!(read(&path).is_err());
        fs::write(&path, b"[{\"name\":\"index_repository\"}]").unwrap();
        assert!(read(&path).is_err());
        let complete = serde_json::to_vec(&report(known_tools())).unwrap();
        fs::write(&path, &complete[..complete.len() / 2]).unwrap();
        assert!(read(&path).is_err());
        fs::write(
            &path,
            br#"{"state":"catalogue-read","server":"codebase-memory-mcp","protocol_version":"2024-11-05","artifact_sha256":"b4b403b1d7c4def3785f148b93f345ce8427858f4f5489ce28580c4387a336a6","tool_count":15,"tools":[],"tools":[]}"#,
        )
        .unwrap();
        assert!(read(&path).is_err());
        let path = write_report(root.path(), &report(known_tools()));
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(BYTE_LIMIT + 1).unwrap();
        assert!(read(&path).is_err());
        assert_eq!(file.metadata().unwrap().len(), BYTE_LIMIT + 1);
    }

    #[test]
    fn name_and_schema_mismatches_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let mut tools = known_tools();
        tools[0]["name"] = json!("unknown_tool");
        assert!(read(&write_report(root.path(), &report(tools))).is_err());

        let mut tools = known_tools();
        tools[1]["name"] = json!("index_repository");
        assert!(read(&write_report(root.path(), &report(tools))).is_err());

        let mut tools = known_tools();
        tools.pop();
        let mut value = report(tools);
        value["tool_count"] = json!(15);
        assert!(read(&write_report(root.path(), &value)).is_err());

        let mut tools = known_tools();
        tools[1]["inputSchema"] = json!({"type": "string", "properties": {}});
        assert!(read(&write_report(root.path(), &report(tools))).is_err());

        let mut tools = known_tools();
        tools[2]["inputSchema"]["required"] = json!(["missing"]);
        assert!(read(&write_report(root.path(), &report(tools))).is_err());

        let mut tools = known_tools();
        tools[3]["outputSchema"] = json!({"type": "object"});
        assert!(read(&write_report(root.path(), &report(tools))).is_err());

        let mut value = report(known_tools());
        value["artifact_sha256"] = json!("0".repeat(64));
        assert!(read(&write_report(root.path(), &value)).is_err());
        value["artifact_sha256"] = json!(AUDITED_BUILD);
        value["state"] = json!("protocol-ready");
        assert!(read(&write_report(root.path(), &value)).is_err());
    }

    #[test]
    fn aliases_are_refused_without_writing_the_target() {
        let root = tempfile::tempdir().unwrap();
        let target = write_report(root.path(), &report(known_tools()));
        let original = fs::read(&target).unwrap();
        let alias = root.path().join("alias.json");
        std::os::windows::fs::symlink_file(&target, &alias).unwrap();
        assert!(read(&alias).is_err());
        assert_eq!(fs::read(&target).unwrap(), original);
        assert!(
            fs::symlink_metadata(&alias)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}
