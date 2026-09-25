//! Strict versioned structured executor assignments.
//!
//! An assignment file replaces free-text dispatch with declared facts: the
//! objective, the existing inputs it may rely on, the output files it owns,
//! its invariants, its acceptance conditions, the consumer of the returned
//! result and the escalation boundaries that return decisions to that
//! consumer. Validation is deterministic and model-free: the document, its
//! limits and every declared path are checked against the allocated checkout
//! before a launcher process opens a model conversation, and the rendered
//! brief carries the actual checkout, the committed base and the exact
//! relative paths together with the executor's own work cycle and the compact
//! evidence result the consumer expects back.
//!
//! This module owns the installed exchange guidance for both dispatch paths:
//! the structured brief renders it with the declared consumer, and a free-text
//! assignment receives the identical rule, so the lead channel, its waiting
//! behavior and the board-first rule are stated once instead of per caller.

use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

/// Version of the assignment document this build accepts.
pub const ASSIGNMENT_SCHEMA: u32 = 1;
/// The assignment file is bounded: its brief becomes a launcher argument.
pub const MAX_ASSIGNMENT_BYTES: u64 = 256 * 1024;
pub const MAX_OBJECTIVE_BYTES: usize = 1024;
pub const MAX_LIST_ITEMS: usize = 64;
pub const MAX_ITEM_BYTES: usize = 512;
pub const MAX_RELATIVE_PATH_BYTES: usize = 240;
/// Bound on the rendered brief handed to the launcher process.
pub const MAX_BRIEF_BYTES: usize = 16 * 1024;

/// Who consumes the returned result when the assignment does not declare a
/// consumer: the lead that dispatched it is always the direct consumer.
pub const DEFAULT_CONSUMER: &str = "the lead that dispatched this assignment";

/// Escalation boundaries every assignment carries. Declared `escalate` items
/// are added to these; they never replace them, because a change to the agreed
/// outcome, a material architecture change, missing authority and an
/// unobtainable dependency always return to the consumer.
pub const STANDING_ESCALATIONS: [&str; 4] = [
    "a change to the agreed outcome or scope",
    "a material architecture or design change",
    "missing authority or access",
    "a concrete dependency you cannot obtain",
];

/// The one escalation channel every assignment names. The installed command
/// addresses the originating lead itself, so no recipient, slot, session,
/// checkout or endpoint is discovered, supplied or taught here.
pub const LEAD_CHANNEL_COMMAND: &str = "codex-harness lead message";

/// What one question costs and how it is answered: the run stays live and
/// independent work continues, so no polling loop, keep-alive ritual or resume
/// is needed to wait. One owner for this wording, so the structured brief and
/// the free-text assignment teach the same rule.
const WAITING_RULE: &str = "An unanswered request keeps the run, session and worktree available while it waits for the reply - no polling, keep-alive loop or resume - and independent authorized work may continue. An executor watch result of 3 means answer that request; it is not completion, failure or a resume trigger.\n";

/// The standing boundaries every assignment carries, without the declared
/// additions.
fn standing_escalations() -> Vec<String> {
    STANDING_ESCALATIONS.map(str::to_owned).to_vec()
}

/// The escalation rule that precedes the boundary list: the single installed
/// command, its default reply request and its no-reply notice form, when asking
/// is justified at all, and what stays with the executor and the bd board.
fn lead_channel_heading(consumer: &str) -> String {
    format!(
        "escalate to {consumer} through {LEAD_CHANNEL_COMMAND} --text '...' (or --file FILE for literal UTF-8), the installed command that addresses the originating lead itself and needs no recipient, slot, session or endpoint supplied; it asks for a reply unless --notify marks a notice that needs none. Ask only after investigating the available facts, and only for a boundary below, a material ambiguity, or an authority/access boundary you cannot cross - everything else, including ordinary implementation errors and routine progress, is yours, and durable blockers, decisions and results stay on the bd issue"
    )
}

/// Renders the escalation rule, its boundary list and the waiting rule that
/// every newly dispatched or continued executor receives, whether it was
/// dispatched as free text or as a structured assignment. Escalation stays
/// exceptional: routine progress, repeated status and ordinary implementation
/// errors belong to the executor.
fn push_lead_channel(text: &mut String, consumer: &str, escalations: &[String]) {
    push_list(text, &lead_channel_heading(consumer), escalations);
    text.push_str(WAITING_RULE);
}

/// The free-text assignment one session receives: the caller's own text stays
/// first and literal, and the installed guidance follows it, so a free-text
/// caller does not have to restate the rule by hand. Resume and restart render
/// through this same function, and a restart keeps the text it recorded, so no
/// path needs a second copy of the guidance.
pub fn free_text_brief(text: &str) -> String {
    let mut rendered = text.trim_end().to_owned();
    rendered.push_str("\n\n");
    push_lead_channel(&mut rendered, DEFAULT_CONSUMER, &standing_escalations());
    rendered
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub schema: u32,
    pub objective: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub invariants: Vec<String>,
    pub acceptance: Vec<String>,
    /// Consumer of the returned result; omitted means the dispatching lead.
    #[serde(default)]
    pub consumer: Option<String>,
    /// Additional triggers that return the decision to the consumer; the
    /// standing boundaries apply in every case.
    #[serde(default)]
    pub escalate: Vec<String>,
}

/// The actual dispatch context the brief names: the bound checkout, its
/// committed base, the session owner and the source checkout it came from.
pub struct AssignmentContext<'a> {
    pub checkout: &'a Path,
    pub base: &'a str,
    pub owner: &'a str,
    pub source: &'a Path,
}

impl Assignment {
    /// Reads one assignment file and checks schema, limits and path syntax.
    /// Nothing here touches a checkout or a model.
    pub fn load(path: &Path) -> io::Result<Self> {
        let metadata = fs::metadata(path).map_err(|error| {
            invalid(&format!(
                "assignment file {} is unreadable: {error}",
                path.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(invalid(&format!(
                "assignment path {} is not a regular file",
                path.display()
            )));
        }
        if metadata.len() > MAX_ASSIGNMENT_BYTES {
            return Err(invalid(&format!(
                "assignment file {} is {} bytes; the limit is {MAX_ASSIGNMENT_BYTES}",
                path.display(),
                metadata.len()
            )));
        }
        let bytes = fs::read(path)?;
        let assignment: Self = serde_json::from_slice(&bytes).map_err(|error| {
            invalid(&format!(
                "assignment file {} is not a schema {ASSIGNMENT_SCHEMA} JSON document: {error}",
                path.display()
            ))
        })?;
        assignment.validate()?;
        Ok(assignment)
    }

    fn validate(&self) -> io::Result<()> {
        if self.schema != ASSIGNMENT_SCHEMA {
            return Err(invalid(&format!(
                "assignment schema {} is unsupported; this build accepts schema {ASSIGNMENT_SCHEMA}",
                self.schema
            )));
        }
        require_text("objective", &self.objective, MAX_OBJECTIVE_BYTES)?;
        require_text_list("invariants", &self.invariants)?;
        require_text_list("acceptance", &self.acceptance)?;
        require_text_list("escalate", &self.escalate)?;
        if let Some(consumer) = &self.consumer {
            require_text("consumer", consumer, MAX_ITEM_BYTES)?;
        }
        if self.acceptance.is_empty() {
            return Err(invalid(
                "assignment acceptance is empty; name at least one checkable acceptance item",
            ));
        }
        require_paths("inputs", &self.inputs)?;
        require_paths("outputs", &self.outputs)?;
        Ok(())
    }

    /// Validates the declared paths against the allocated checkout. Existing
    /// inputs must be regular files inside the checkout; output paths may not
    /// exist yet, but every existing ancestor must stay inside the checkout,
    /// so traversal and symlink or reparse escapes are rejected before the
    /// dispatched session starts.
    pub fn validate_paths(&self, checkout: &Path) -> io::Result<()> {
        let root = checkout.canonicalize().map_err(|error| {
            invalid(&format!(
                "the allocated checkout {} cannot be resolved: {error}",
                checkout.display()
            ))
        })?;
        for (index, declared) in self.inputs.iter().enumerate() {
            let parts = relative_parts(&format!("inputs[{index}]"), declared)?;
            resolve_input(&root, declared, &parts)?;
        }
        for (index, declared) in self.outputs.iter().enumerate() {
            let parts = relative_parts(&format!("outputs[{index}]"), declared)?;
            resolve_output(&root, declared, &parts)?;
        }
        Ok(())
    }
}

/// Validates the declared paths and renders the brief the session receives.
/// The same function serves dispatch and the model-free `executor assignment`
/// path, so a lead can exercise the exact rendering without a model request.
pub fn brief(assignment: &Assignment, context: &AssignmentContext<'_>) -> io::Result<String> {
    assignment.validate_paths(context.checkout)?;
    let consumer = consumer_text(assignment);
    let mut escalations = standing_escalations();
    escalations.extend(
        assignment
            .escalate
            .iter()
            .map(|item| item.trim().to_owned()),
    );
    let mut text = String::new();
    text.push_str("Structured executor assignment (schema 1)\n");
    text.push_str(&format!("objective: {}\n", assignment.objective.trim()));
    text.push_str(&format!("checkout: {}\n", context.checkout.display()));
    text.push_str(&format!("base: {}\n", context.base));
    text.push_str(&format!("source: {}\n", context.source.display()));
    text.push_str(&format!("owner: {}\n", context.owner));
    text.push_str(&format!("consumer: {consumer}\n"));
    push_list(
        &mut text,
        "inputs (declared; verified regular files inside the checkout)",
        &assignment.inputs,
    );
    push_list(
        &mut text,
        "outputs (owned by this assignment; new files are allowed)",
        &assignment.outputs,
    );
    push_list(&mut text, "invariants", &assignment.invariants);
    push_list(&mut text, "acceptance", &assignment.acceptance);
    push_lead_channel(&mut text, consumer, &escalations);
    text.push_str(
        "work cycle (yours): read the declared inputs yourself - source bodies are supplied only when reading is unavailable - investigate the current source and callers before editing, implement the outcome you own, run the applicable checks through the real entry point, then correct your own local errors and repeat.\n",
    );
    text.push_str(
        "Before editing, verify the checkout is at the base above (git rev-parse HEAD) and report a mismatch instead of editing. Choose the installed skills this assignment needs and announce their first use. Keep every change inside the checkout and leave partial work resumable for a continuation.\n",
    );
    text.push_str(
        "Run heavy builds and checks through codex-harness heavy -- PROGRAM ARGS. It queues against the shared account budget without a lead grant; keep the default account and budget unless the assignment explicitly authorizes another. Ordinary source reads and edits need no heavy-command slot.\n",
    );
    text.push_str(
        &format!(
            "When the outcome is complete, report a compact result: done and remaining work, the checkout and base you worked from, the exact files touched, how each acceptance item was verified with the checks actually run, limitations, the decision you need from {consumer}, and where the full detail lives.\n"
        ),
    );
    if text.len() > MAX_BRIEF_BYTES {
        return Err(invalid(&format!(
            "the rendered assignment brief is {} bytes; the limit is {MAX_BRIEF_BYTES}",
            text.len()
        )));
    }
    Ok(text)
}

/// The declared consumer, or the dispatching lead when the field is omitted
/// or blank. `validate` rejects a declared blank value, and the fallback keeps
/// a directly constructed assignment rendering a named consumer.
fn consumer_text(assignment: &Assignment) -> &str {
    assignment
        .consumer
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_CONSUMER)
}

fn push_list(text: &mut String, title: &str, values: &[String]) {
    if values.is_empty() {
        text.push_str(&format!("{title}: none declared\n"));
        return;
    }
    text.push_str(&format!("{title}:\n"));
    for value in values {
        text.push_str(&format!("- {}\n", value.trim()));
    }
}

fn resolve_input(root: &Path, declared: &str, parts: &[String]) -> io::Result<()> {
    let joined = joined_path(root, parts);
    match fs::symlink_metadata(&joined) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(invalid(&format!(
            "declared input {declared} is missing from the checkout: {} does not exist; correct the assignment or the checkout before dispatching",
            joined.display()
        ))),
        Err(error) => Err(invalid(&format!(
            "declared input {declared} cannot be inspected at {}: {error}",
            joined.display()
        ))),
        Ok(_) => {
            let resolved = fs::canonicalize(&joined).map_err(|error| {
                invalid(&format!(
                    "declared input {declared} cannot be resolved: {error}"
                ))
            })?;
            if !resolved.starts_with(root) {
                return Err(invalid(&format!(
                    "declared input {declared} resolves outside the checkout to {}; the assignment is rejected without reading the target",
                    resolved.display()
                )));
            }
            let metadata = fs::metadata(&resolved).map_err(|error| {
                invalid(&format!("declared input {declared} is unreadable: {error}"))
            })?;
            if !metadata.is_file() {
                return Err(invalid(&format!(
                    "declared input {declared} is not a regular file"
                )));
            }
            Ok(())
        }
    }
}

/// Walks the existing prefix of an output path. Every entry that exists is
/// resolved with links followed and must land inside the checkout; the first
/// missing component ends the walk, because the rest of the path is new.
fn resolve_output(root: &Path, declared: &str, parts: &[String]) -> io::Result<()> {
    let mut current = root.to_path_buf();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(invalid(&format!(
                    "declared output {declared} cannot be inspected at {}: {error}",
                    current.display()
                )));
            }
            Ok(_) => {
                let resolved = fs::canonicalize(&current).map_err(|error| {
                    invalid(&format!(
                        "declared output {declared} has an unresolvable entry at {}: {error}",
                        current.display()
                    ))
                })?;
                if !resolved.starts_with(root) {
                    return Err(invalid(&format!(
                        "declared output {declared} resolves outside the checkout through {}; the assignment is rejected",
                        current.display()
                    )));
                }
                let last = index + 1 == parts.len();
                if !last && !resolved.is_dir() {
                    return Err(invalid(&format!(
                        "declared output {declared} has an existing non-directory ancestor {}",
                        current.display()
                    )));
                }
                if last && resolved.is_dir() {
                    return Err(invalid(&format!(
                        "declared output {declared} is an existing directory; name the file to write"
                    )));
                }
                current = resolved;
            }
        }
    }
    Ok(())
}

fn joined_path(root: &Path, parts: &[String]) -> PathBuf {
    let mut path = root.to_path_buf();
    for part in parts {
        path.push(part);
    }
    path
}

fn require_text(field: &str, value: &str, max: usize) -> io::Result<()> {
    if value.trim().is_empty() {
        return Err(invalid(&format!("assignment {field} is empty")));
    }
    if value.len() > max {
        return Err(invalid(&format!(
            "assignment {field} is {} bytes; the limit is {max}",
            value.len()
        )));
    }
    Ok(())
}

fn require_text_list(field: &str, values: &[String]) -> io::Result<()> {
    if values.len() > MAX_LIST_ITEMS {
        return Err(invalid(&format!(
            "assignment {field} has {} entries; the limit is {MAX_LIST_ITEMS}",
            values.len()
        )));
    }
    for (index, value) in values.iter().enumerate() {
        require_text(&format!("{field}[{index}]"), value, MAX_ITEM_BYTES)?;
    }
    Ok(())
}

fn require_paths(field: &str, values: &[String]) -> io::Result<()> {
    if values.len() > MAX_LIST_ITEMS {
        return Err(invalid(&format!(
            "assignment {field} has {} paths; the limit is {MAX_LIST_ITEMS}",
            values.len()
        )));
    }
    let mut seen = BTreeSet::new();
    for (index, value) in values.iter().enumerate() {
        let name = format!("{field}[{index}]");
        let parts = relative_parts(&name, value)?;
        if !seen.insert(parts.join("/")) {
            return Err(invalid(&format!(
                "assignment {name} repeats an already declared path: {value}"
            )));
        }
    }
    Ok(())
}

/// Splits a declared path into plain relative components. Both separators are
/// accepted so a declaration reads the same in either convention; rooted,
/// traversing, drive-relative and alternate-stream names are rejected before
/// any filesystem lookup.
fn relative_parts(field: &str, value: &str) -> io::Result<Vec<String>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid(&format!("assignment {field} is empty")));
    }
    if value.len() > MAX_RELATIVE_PATH_BYTES {
        return Err(invalid(&format!(
            "assignment {field} is {} bytes; the limit is {MAX_RELATIVE_PATH_BYTES}",
            value.len()
        )));
    }
    if value.contains('\0') {
        return Err(invalid(&format!(
            "assignment {field} contains a NUL character"
        )));
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return Err(invalid(&format!(
            "assignment {field} must be relative to the checkout, not rooted: {value}"
        )));
    }
    let mut parts = Vec::new();
    for part in trimmed.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(invalid(&format!(
                "assignment {field} must not traverse with '..': {value}"
            )));
        }
        if part.contains(':') {
            return Err(invalid(&format!(
                "assignment {field} must be a plain relative path without ':': {value}"
            )));
        }
        parts.push(part.to_owned());
    }
    if parts.is_empty() {
        return Err(invalid(&format!(
            "assignment {field} names no file: {value}"
        )));
    }
    Ok(parts)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn document(objective: &str, inputs: &[&str], outputs: &[&str]) -> String {
        serde_json::to_string_pretty(&serde_json::json!({
            "schema": 1,
            "objective": objective,
            "inputs": inputs,
            "outputs": outputs,
            "invariants": ["keep the change inside the checkout"],
            "acceptance": ["the named check passes"],
        }))
        .unwrap()
    }

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("executor-assignment-{name}-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn checked_in(root: &Path, name: &str, document: &str) -> Assignment {
        let path = root.join(name);
        fs::write(&path, document).unwrap();
        Assignment::load(&path).unwrap()
    }

    /// The escalation wording this change replaces. The rule must replace it
    /// rather than grow beside it, so the renderer keeps no copy.
    const SUPERSEDED_ESCALATION_HEADING: &str = "escalate to {consumer} (everything else, including ordinary implementation errors, is yours to resolve)";

    /// The exact rule and waiting behavior an assignment must render. The
    /// acceptance for this change is this rendered text, so the expectation is
    /// literal: a wording change has to update it deliberately.
    fn expected_lead_channel(consumer: &str) -> String {
        format!(
            "escalate to {consumer} through codex-harness lead message --text '...' (or --file FILE for literal UTF-8), the installed command that addresses the originating lead itself and needs no recipient, slot, session or endpoint supplied; it asks for a reply unless --notify marks a notice that needs none. Ask only after investigating the available facts, and only for a boundary below, a material ambiguity, or an authority/access boundary you cannot cross - everything else, including ordinary implementation errors and routine progress, is yours, and durable blockers, decisions and results stay on the bd issue:\n"
        )
    }

    const EXPECTED_WAITING_RULE: &str = "An unanswered request keeps the run, session and worktree available while it waits for the reply - no polling, keep-alive loop or resume - and independent authorized work may continue. An executor watch result of 3 means answer that request; it is not completion, failure or a resume trigger.\n";

    #[test]
    fn schema_version_fields_and_limits_are_strict() {
        let root = temp_root("strict");
        let path = root.join("assignment.json");
        fs::write(&path, document("Do the work", &["input.txt"], &["out.txt"])).unwrap();
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let assignment = Assignment::load(&path).unwrap();
        assert_eq!(assignment.schema, 1);
        assignment.validate_paths(&root).unwrap();

        let mut unknown: serde_json::Value =
            serde_json::from_str(&document("Do the work", &[], &[])).unwrap();
        unknown["extra"] = serde_json::json!(true);
        fs::write(&path, serde_json::to_string(&unknown).unwrap()).unwrap();
        assert!(
            Assignment::load(&path)
                .unwrap_err()
                .to_string()
                .contains("unknown field")
        );

        let mut unsupported: serde_json::Value =
            serde_json::from_str(&document("Do the work", &[], &[])).unwrap();
        unsupported["schema"] = serde_json::json!(2);
        fs::write(&path, serde_json::to_string(&unsupported).unwrap()).unwrap();
        assert!(
            Assignment::load(&path)
                .unwrap_err()
                .to_string()
                .contains("schema 2 is unsupported")
        );

        let mut missing: serde_json::Value =
            serde_json::from_str(&document("Do the work", &[], &[])).unwrap();
        missing.as_object_mut().unwrap().remove("acceptance");
        fs::write(&path, serde_json::to_string(&missing).unwrap()).unwrap();
        assert!(
            Assignment::load(&path)
                .unwrap_err()
                .to_string()
                .contains("missing field")
        );

        let text = document(&"x".repeat(MAX_OBJECTIVE_BYTES + 1), &[], &[]);
        fs::write(&path, text).unwrap();
        assert!(
            Assignment::load(&path)
                .unwrap_err()
                .to_string()
                .contains("the limit is")
        );

        for declared in ["..\\outside.txt", "C:\\outside.txt", "/outside.txt", ""] {
            let text = document("Do the work", &[declared], &[]);
            fs::write(&path, text).unwrap();
            assert!(
                Assignment::load(&path).is_err(),
                "{declared} must be rejected"
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn inputs_must_exist_and_outputs_may_be_new() {
        let root = temp_root("paths");
        fs::create_dir_all(root.join("crates")).unwrap();
        fs::write(root.join("crates/input.rs"), "fn main() {}\n").unwrap();
        let assignment = checked_in(
            &root,
            "assignment.json",
            &document(
                "Extend the module",
                &["crates/input.rs"],
                &["crates/new/module.rs"],
            ),
        );
        assignment.validate_paths(&root).unwrap();

        let missing = checked_in(
            &root,
            "missing.json",
            &document("Extend the module", &["crates/absent.rs"], &[]),
        );
        let error = missing.validate_paths(&root).unwrap_err().to_string();
        assert!(error.contains("crates/absent.rs"), "{error}");
        assert!(error.contains("is missing"), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_brief_names_the_checkout_base_and_exact_paths() {
        let root = temp_root("brief");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let assignment = checked_in(
            &root,
            "assignment.json",
            &document("Ship the outcome", &["input.txt"], &["out/result.txt"]),
        );
        let source = root.join("source");
        let text = brief(
            &assignment,
            &AssignmentContext {
                checkout: &root,
                base: "0123456789abcdef0123456789abcdef01234567",
                owner: "exec-ds-7",
                source: &source,
            },
        )
        .unwrap();
        assert!(text.contains("Structured executor assignment (schema 1)"));
        assert!(text.contains(&format!("checkout: {}", root.display())));
        assert!(text.contains("base: 0123456789abcdef0123456789abcdef01234567"));
        assert!(text.contains("owner: exec-ds-7"));
        assert!(text.contains("- input.txt"));
        assert!(text.contains("- out/result.txt"));
        assert!(text.contains("- the named check passes"));
        assert!(text.contains(&format!("source: {}", source.display())));
        assert_eq!(text.matches("source:").count(), 1, "{text}");
        assert!(
            text.contains("verify the checkout is at the base above"),
            "{text}"
        );
        assert!(
            text.contains("Choose the installed skills this assignment needs"),
            "{text}"
        );
        // The result consumer, the escalation boundaries and the executor's
        // own cycle are part of the rendered contract, not of the caller's
        // prose.
        assert!(
            text.contains(&format!("consumer: {DEFAULT_CONSUMER}")),
            "{text}"
        );
        assert!(
            text.contains(&expected_lead_channel(DEFAULT_CONSUMER)),
            "{text}"
        );
        for boundary in STANDING_ESCALATIONS {
            assert!(
                text.contains(&format!("- {boundary}\n")),
                "{boundary}\n{text}"
            );
        }
        assert!(
            text.contains("work cycle (yours): read the declared inputs yourself"),
            "{text}"
        );
        assert!(
            text.contains("report a compact result: done and remaining work"),
            "{text}"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consumer_and_escalation_are_optional_and_validated() {
        let root = temp_root("contract-fields");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let source = root.join("source");
        let path = root.join("assignment.json");
        let context = AssignmentContext {
            checkout: &root,
            base: "0123456789abcdef0123456789abcdef01234567",
            owner: "exec-ds-7",
            source: &source,
        };

        // Omission keeps a schema-1 document valid: the dispatching lead
        // consumes the result and the standing boundaries still apply.
        let minimal = checked_in(
            &root,
            "assignment.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        assert_eq!(minimal.consumer, None);
        assert!(minimal.escalate.is_empty());
        let text = brief(&minimal, &context).unwrap();
        assert!(
            text.contains(&expected_lead_channel(DEFAULT_CONSUMER)),
            "{text}"
        );
        assert!(
            text.contains("the decision you need from the lead that dispatched this assignment"),
            "{text}"
        );

        // A declared consumer and extra triggers are carried into the brief,
        // and the standing boundaries are not replaced by them.
        let mut declared: serde_json::Value =
            serde_json::from_str(&document("Ship the outcome", &["input.txt"], &[])).unwrap();
        declared["consumer"] = serde_json::json!("the lead of epic sample-3mu");
        declared["escalate"] = serde_json::json!(["any change to the synthetic crate layout"]);
        fs::write(&path, serde_json::to_string(&declared).unwrap()).unwrap();
        let assignment = Assignment::load(&path).unwrap();
        let text = brief(&assignment, &context).unwrap();
        assert!(
            text.contains("consumer: the lead of epic sample-3mu"),
            "{text}"
        );
        assert!(
            text.contains(&expected_lead_channel("the lead of epic sample-3mu")),
            "{text}"
        );
        assert!(
            text.contains("- any change to the synthetic crate layout"),
            "{text}"
        );
        assert!(
            text.contains("- a material architecture or design change"),
            "{text}"
        );
        assert!(
            text.contains("the decision you need from the lead of epic sample-3mu"),
            "{text}"
        );

        // Malformed values fail before any rendering, with the field named.
        let too_long_consumer = "x".repeat(MAX_ITEM_BYTES + 1);
        for (field, value, expected) in [
            (
                "consumer",
                serde_json::json!(""),
                "assignment consumer is empty",
            ),
            (
                "consumer",
                serde_json::json!(too_long_consumer),
                "the limit is",
            ),
            (
                "escalate",
                serde_json::json!([""]),
                "assignment escalate[0] is empty",
            ),
            (
                "escalate",
                serde_json::json!(vec!["escalate this"; MAX_LIST_ITEMS + 1]),
                "the limit is",
            ),
        ] {
            let mut broken: serde_json::Value =
                serde_json::from_str(&document("Ship the outcome", &["input.txt"], &[])).unwrap();
            broken[field] = value;
            fs::write(&path, serde_json::to_string(&broken).unwrap()).unwrap();
            let error = Assignment::load(&path).unwrap_err().to_string();
            assert!(error.contains(expected), "{field}: {error}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_document_within_field_limits_can_still_exceed_the_brief_budget() {
        let root = temp_root("brief-budget");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let mut assignment = checked_in(
            &root,
            "assignment.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        // Every list and item stays inside its own limit; the rendered brief
        // still cannot fit the launcher argument budget.
        let item = format!("keep the change inside the checkout: {}", "x".repeat(400));
        assignment.invariants = vec![item.clone(); MAX_LIST_ITEMS];
        assignment.acceptance = vec![item; MAX_LIST_ITEMS];
        let error = brief(
            &assignment,
            &AssignmentContext {
                checkout: &root,
                base: "0123456789abcdef0123456789abcdef01234567",
                owner: "exec-ds-7",
                source: &root,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("the rendered assignment brief is"),
            "{error}"
        );
        assert!(
            error.contains(&format!("the limit is {MAX_BRIEF_BYTES}")),
            "{error}"
        );
        let _ = fs::remove_dir_all(root);
    }

    /// The guidance an executor receives must stay a small, bounded addition to
    /// one fixed payload: this test binds the canonical payload, the rendered
    /// size and the replacement of the superseded escalation wording, so the
    /// instruction-size report can be reproduced from the source.
    #[test]
    fn one_fixed_payload_keeps_the_rendered_guidance_concise() {
        let root = std::env::temp_dir().join("executor-assignment-measure-fixed");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for input in [
            "openspec/changes/sample/design.md",
            "crates/sample/src/first.rs",
            "crates/sample/src/second.rs",
            "crates/sample/tests/first.rs",
        ] {
            let path = root.join(input);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "// declared input\n").unwrap();
        }
        let document = serde_json::to_string_pretty(&serde_json::json!({
            "schema": 1,
            "objective": "Implement the structured-renderer half of sample change 9.9: add concise installed guidance to generated structured assignment briefs and leave the free-text renderer to the other worker.",
            "inputs": [
                "openspec/changes/sample/design.md",
                "crates/sample/src/first.rs",
                "crates/sample/src/second.rs",
                "crates/sample/tests/first.rs"
            ],
            "outputs": ["crates/sample/src/first.rs"],
            "invariants": [
                "Rendered structured briefs must concisely explain the sample command, its default reply request, its notice switch, automatic waiting with no keep-alive or polling, independent work, board ownership, and the answer-required watch result.",
                "Guidance is exceptional only: material ambiguity, an authority or access boundary, or an unresolvable dependency after investigation; ordinary implementation errors and routine progress stay off-channel.",
                "Do not teach address discovery, receipt editing, process identity checks, resume rituals, recursive delegation or re-enabling native agent tools.",
                "Keep the addition within MAX_BRIEF_BYTES and replace superseded wording rather than appending a second workflow where possible.",
                "Do not edit the free-text renderer: another worker owns it and the free-text half remains for lead integration."
            ],
            "acceptance": [
                "Existing and new assignment tests prove the exact rendered guidance, limits, Unicode and default consumer behavior.",
                "cargo fmt --all -- --check passes.",
                "codex-harness heavy -- cargo test --locked -p codex-harness executor_assignment --jobs 1 -- --test-threads=1 passes."
            ]
        }))
        .unwrap();
        let assignment = checked_in(&root, "assignment.json", &document);
        let text = brief(
            &assignment,
            &AssignmentContext {
                checkout: &root,
                base: "d01df098da47c0d87e5f6268210ab1f2401e152d",
                owner: "exec-measure",
                source: &root,
            },
        )
        .unwrap();
        let guidance = format!(
            "{}{}",
            expected_lead_channel(DEFAULT_CONSUMER),
            EXPECTED_WAITING_RULE
        );
        println!("CANONICAL_BRIEF_BYTES {}", text.len());
        println!("CANONICAL_GUIDANCE_BYTES {}", guidance.len());
        assert!(
            text.len() <= MAX_BRIEF_BYTES,
            "the canonical brief is {} bytes",
            text.len()
        );
        assert!(
            guidance.len() <= 1024,
            "the added guidance is {} bytes; keep it concise",
            guidance.len()
        );
        assert!(
            !text.contains(SUPERSEDED_ESCALATION_HEADING),
            "the superseded escalation wording is replaced, not kept beside the rule: {text}"
        );
        let _ = fs::remove_dir_all(root);
    }

    /// The guidance a real dispatch renders: the one lead command, its default
    /// reply request and its notice form, when asking is justified, what stays
    /// with the executor, and the waiting rule that removes every keep-alive
    /// ritual - with no address, process or resume mechanics taught beside it.
    #[test]
    fn the_rendered_guidance_names_the_lead_channel_and_nothing_else() {
        let root = temp_root("guidance");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let assignment = checked_in(
            &root,
            "assignment.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        let text = brief(
            &assignment,
            &AssignmentContext {
                checkout: &root,
                base: "0123456789abcdef0123456789abcdef01234567",
                owner: "exec-ds-7",
                source: &root,
            },
        )
        .unwrap();
        let start = text.find("escalate to").expect("the rule is rendered");
        let end = text
            .find("work cycle (yours)")
            .expect("the executor's cycle follows the rule");
        let rendered = &text[start..end];
        assert_eq!(rendered.matches("escalate to").count(), 1, "{rendered}");
        assert!(
            rendered.starts_with(&expected_lead_channel(DEFAULT_CONSUMER)),
            "{rendered}"
        );
        assert!(rendered.ends_with(EXPECTED_WAITING_RULE), "{rendered}");
        for required in [
            "codex-harness lead message",
            "--text '...'",
            "--file FILE for literal UTF-8",
            "asks for a reply unless --notify marks a notice that needs none",
            "after investigating the available facts",
            "a material ambiguity, or an authority/access boundary",
            "ordinary implementation errors and routine progress, is yours",
            "durable blockers, decisions and results stay on the bd issue",
            "no polling, keep-alive loop or resume",
            "independent authorized work may continue",
            "An executor watch result of 3 means answer that request",
        ] {
            assert!(rendered.contains(required), "{required}\n{rendered}");
        }
        // The rule is the whole workflow: nothing teaches recipient or session
        // discovery, receipt editing, process identity checks, a resume ritual,
        // recursive delegation or re-enabling the native agent tools.
        for forbidden in [
            "--session",
            "CODEX_",
            "PID",
            "endpoint-",
            "turn/steer",
            "executor spawn",
            "sub-agent",
            "agent tool",
            "receipt",
            "reply-to",
        ] {
            assert!(!rendered.contains(forbidden), "{forbidden}\n{rendered}");
        }
        let _ = fs::remove_dir_all(root);
    }

    /// Free text is a supported dispatch path, so it receives the identical
    /// rule: the caller's text stays first and literal and the same block
    /// follows it. One owner for the wording means the structured brief and a
    /// free-text assignment cannot drift into two workflows.
    #[test]
    fn free_text_and_structured_assignments_teach_one_guidance_rule() {
        let literal = "Réparer le contrat - keep 'quoted' text and --notify literal, суммарный контроль\nsecond line";
        let free = free_text_brief(literal);
        assert!(free.starts_with(literal), "{free}");
        let mut block = expected_lead_channel(DEFAULT_CONSUMER);
        for boundary in STANDING_ESCALATIONS {
            block.push_str(&format!("- {boundary}\n"));
        }
        block.push_str(EXPECTED_WAITING_RULE);
        assert!(free.contains(&block), "{free}");
        assert_eq!(free.matches("escalate to").count(), 1, "{free}");

        // The same rendered block reaches the structured path, so an executor
        // cannot receive two versions of the rule.
        let root = temp_root("one-rule");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let assignment = checked_in(
            &root,
            "assignment.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        let structured = brief(
            &assignment,
            &AssignmentContext {
                checkout: &root,
                base: "0123456789abcdef0123456789abcdef01234567",
                owner: "exec-ds-7",
                source: &root,
            },
        )
        .unwrap();
        assert!(structured.contains(&block), "{structured}");
        // A caller's trailing blank line is dropped instead of being rendered
        // into the rule; nothing else about the text is rewritten.
        let padded = free_text_brief("do the work\n\n");
        assert!(padded.starts_with("do the work\n"), "{padded}");
        let _ = fs::remove_dir_all(root);
    }

    /// Every limit is a byte limit. These two documents hold the same number of
    /// characters per item; the multibyte one is refused for its bytes while
    /// its ASCII twin renders, so the budget cannot be mistaken for characters.
    #[test]
    fn multibyte_assignment_content_is_measured_in_bytes() {
        let root = temp_root("multibyte");
        fs::write(root.join("input.txt"), "input\n").unwrap();
        let context = AssignmentContext {
            checkout: &root,
            base: "0123456789abcdef0123456789abcdef01234567",
            owner: "exec-ds-7",
            source: &root,
        };
        let items = 60;
        let mut wide = checked_in(
            &root,
            "wide.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        wide.invariants = vec!["é".repeat(200); items];
        assert_eq!(wide.invariants[0].chars().count(), 200);
        assert!(wide.invariants[0].len() <= MAX_ITEM_BYTES);
        let error = brief(&wide, &context).unwrap_err().to_string();
        assert!(
            error.contains("the rendered assignment brief is"),
            "{error}"
        );
        assert!(
            error.contains(&format!("the limit is {MAX_BRIEF_BYTES}")),
            "{error}"
        );

        let mut ascii = checked_in(
            &root,
            "ascii.json",
            &document("Ship the outcome", &["input.txt"], &[]),
        );
        ascii.invariants = vec!["x".repeat(200); items];
        assert_eq!(ascii.invariants[0].chars().count(), 200);
        let text = brief(&ascii, &context).unwrap();
        assert!(text.len() <= MAX_BRIEF_BYTES, "{}", text.len());
        for boundary in STANDING_ESCALATIONS {
            assert!(text.contains(boundary), "{boundary}\n{text}");
        }
        let _ = fs::remove_dir_all(root);
    }
}
