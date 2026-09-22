//! Strict versioned structured executor assignments.
//!
//! An assignment file replaces free-text dispatch with declared facts: the
//! objective, the existing inputs it may rely on, the output files it owns,
//! its invariants and its acceptance conditions. Validation is deterministic
//! and model-free: the document, its limits and every declared path are
//! checked against the allocated checkout before a launcher process opens a
//! model conversation, and the rendered brief carries the actual checkout,
//! the committed base and the exact relative paths.

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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub schema: u32,
    pub objective: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub invariants: Vec<String>,
    pub acceptance: Vec<String>,
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
    let mut text = String::new();
    text.push_str("Structured executor assignment (schema 1)\n");
    text.push_str(&format!("objective: {}\n", assignment.objective.trim()));
    text.push_str(&format!("checkout: {}\n", context.checkout.display()));
    text.push_str(&format!("base: {}\n", context.base));
    text.push_str(&format!("source: {}\n", context.source.display()));
    text.push_str(&format!("owner: {}\n", context.owner));
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
    text.push_str(
        "Keep every change inside the checkout above. When the outcome is complete, report what changed, the exact files touched and how each acceptance item was verified.\n",
    );
    if text.len() > MAX_BRIEF_BYTES {
        return Err(invalid(&format!(
            "the rendered assignment brief is {} bytes; the limit is {MAX_BRIEF_BYTES}",
            text.len()
        )));
    }
    Ok(text)
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
        let _ = fs::remove_dir_all(root);
    }
}
