//! Shared runtime tool-catalogue probe: the code-mode `exec` script that
//! reports a native session's own nested tool names between fixed markers, the
//! one parser for that payload, and the classifier that recognises the
//! competing native `codex_worktrees` tool surface. The canned fixture servers
//! and the native cases that read a session's own catalogue share this owner.
#![allow(dead_code)] // Each test binary compiles the subset its own cases use.

use serde_json::Value;

pub const ALL_TOOLS_MARKER: &str = "CANNED_ALL_TOOLS_PROBE";
pub const ALL_TOOLS_END: &str = "CANNED_ALL_TOOLS_END";

/// The probe call id prefix the canned fixture issues for runtime readings.
pub const PROBE_CALL_PREFIX: &str = "canned-all-tools-probe-";

/// One code-mode `exec` script that reports the session's own runtime tool
/// names between the shared markers: the keys of the documented global `tools`
/// object plus the `ALL_TOOLS` metadata entries, which name the enabled nested
/// tools including deferred ones the static request schemas omit. Each caller
/// gives its call and response a distinct id, because the runtime catalogue
/// may change between calls.
pub fn all_tools_probe_script() -> String {
    format!(
        "const names = Object.keys(typeof tools === 'undefined' ? {{}} : tools);\n\
         const listed = typeof ALL_TOOLS === 'undefined' ? [] : ALL_TOOLS;\n\
         for (const entry of Object.values(listed)) names.push(typeof entry === 'string' ? entry : entry.name);\n\
         text('{ALL_TOOLS_MARKER}' + [...new Set(names)].join('|') + '{ALL_TOOLS_END}');"
    )
}

/// The runtime tool names one `exec` output item carries, if any. The session
/// exchanges them as one `|`-separated marker payload, a format that a JSON
/// string wrapper around the exec result cannot corrupt; an exec result can
/// arrive as raw text or as content parts, so both shapes are read.
pub fn probe_names(output: &Value) -> Option<Vec<String>> {
    let mut texts: Vec<&str> = Vec::new();
    match output {
        Value::String(text) => texts.push(text.as_str()),
        Value::Array(parts) => texts.extend(parts.iter().filter_map(|part| {
            part.as_str()
                .or_else(|| part.get("text").and_then(Value::as_str))
        })),
        _ => return None,
    }
    texts.into_iter().find_map(|text| {
        let start = text.find(ALL_TOOLS_MARKER)? + ALL_TOOLS_MARKER.len();
        let rest = &text[start..];
        let end = rest.find(ALL_TOOLS_END)?;
        let names = rest[..end]
            .split('|')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        (!names.is_empty()).then_some(names)
    })
}

/// The competing native-worktree entries of one runtime catalogue.
pub fn worktree_names(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| name.to_lowercase().contains("worktree"))
        .cloned()
        .collect()
}

/// The native `codex_worktrees` entries of one runtime catalogue: the exact
/// server prefix of the competing dynamic-tools surface this repository owns
/// and suppresses. Matching the known native names by that prefix keeps an
/// unrelated tool that merely carries a similar name from failing a
/// preserved-surface check.
pub fn native_worktree_names(names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|name| name.starts_with("mcp__codex_worktrees__"))
        .cloned()
        .collect()
}

/// The visible tool surface of one captured model request: tool names reachable
/// through top-level `tools` entries, `additional_tools` input namespaces and
/// entries nested under both, plus the tool-definition text that carries any
/// deferred declarations.
pub fn tool_surface(body: &str) -> (Vec<String>, String) {
    fn visit(items: &[Value], names: &mut Vec<String>) {
        for item in items {
            if let Some(name) = item.get("name").and_then(Value::as_str) {
                names.push(name.to_owned());
            }
            if let Some(name) = item
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(Value::as_str)
            {
                names.push(name.to_owned());
            }
            if let Some(nested) = item.get("tools").and_then(Value::as_array) {
                visit(nested, names);
            }
        }
    }
    let mut names = Vec::new();
    let mut definitions = String::new();
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(tools) = value.get("tools").and_then(Value::as_array) {
            definitions.push_str(
                &serde_json::to_string(tools).expect("native tool definitions serialize"),
            );
            visit(tools, &mut names);
        }
        if let Some(items) = value.get("input").and_then(Value::as_array) {
            for item in items
                .iter()
                .filter(|item| item["type"] == "additional_tools")
            {
                definitions.push_str(&item.to_string());
                if let Some(tools) = item.get("tools").and_then(Value::as_array) {
                    visit(tools, &mut names);
                }
            }
        }
    }
    (names, definitions)
}

/// The competing native-worktree fragments of one effective tool surface: a
/// tool whose name mentions a worktree, or the `codex_worktrees` marker when
/// the surface defers that dynamic-tools server behind the exec orchestrator.
pub fn worktree_fragments(surface: &(Vec<String>, String)) -> Vec<String> {
    let mut fragments = surface
        .0
        .iter()
        .filter(|name| name.to_lowercase().contains("worktree"))
        .cloned()
        .collect::<Vec<_>>();
    if surface.1.contains("codex_worktrees") {
        fragments.push("deferred:codex_worktrees".to_owned());
    }
    fragments
}
