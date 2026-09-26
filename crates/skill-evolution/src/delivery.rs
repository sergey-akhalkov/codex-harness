//! Bounded catalogue delivery. Ordinary hooks stay off; a list is not awareness.
//!
//! Rendering never claims the complete active set when the native evidence was
//! unavailable, incomplete or truncated, and never claims current-turn delivery.

use crate::{catalogue, invalid};
use std::{fmt::Write as _, io};

const FIELD_LIMIT: usize = 2048;
/// Literals of one entry line without the field contents:
/// `name=;scope=;enabled=;applicability=;path=;revision=\n` plus the boolean.
const LINE_OVERHEAD: usize = 53;

fn revision(entry: &catalogue::Entry) -> &str {
    entry.revision.as_deref().unwrap_or("unavailable")
}

/// Delivered bytes of one entry, measured before any truncation.
pub fn entry_size(entry: &catalogue::Entry) -> usize {
    LINE_OVERHEAD
        + entry.name.len()
        + entry.scope.len()
        + entry.applicability.len()
        + entry.path.to_string_lossy().len()
        + revision(entry).len()
        + if entry.enabled { 4 } else { 5 }
}

pub fn entry_line(entry: &catalogue::Entry) -> io::Result<String> {
    let path = entry.path.to_string_lossy();
    for field in [
        entry.name.as_str(),
        entry.scope.as_str(),
        entry.applicability.as_str(),
        path.as_ref(),
        revision(entry),
    ] {
        if field.len() > FIELD_LIMIT
            || field.contains('\0')
            || field.contains('<')
            || field.contains('>')
            || field.contains('\n')
        {
            return Err(invalid("malformed or injected catalogue field"));
        }
    }
    if entry.name.is_empty() || path.is_empty() || revision(entry).is_empty() {
        return Err(invalid("incomplete catalogue identity"));
    }
    Ok(format!(
        "name={};scope={};enabled={};applicability={};path={};revision={}\n",
        entry.name,
        entry.scope,
        entry.enabled,
        entry.applicability,
        path,
        revision(entry)
    ))
}

/// Render the derived view, stating every gap explicitly.
pub fn render(view: &catalogue::View, limit: usize) -> io::Result<String> {
    let mut out = String::new();
    if view.coverage == catalogue::Coverage::Unavailable {
        out.push_str("native_discovery=unavailable\n");
        for note in &view.notes {
            let _ = writeln!(out, "reason={note}");
        }
        out.push_str("effective_set=unknown\n");
        out.push_str(
            "remedy=restore the native read (register harness/native-launch.json or pass --upstream PATH), then rerun codex-harness skills catalogue\n",
        );
        out.push_str(
            "awareness=none: this read cannot establish model awareness or current-turn delivery\n",
        );
        return Ok(out);
    }
    let total = view.entries.len() + view.omitted;
    let _ = writeln!(
        out,
        "catalogue: {total} effective; delivered={}; measured_metadata_bytes={}; coverage={}",
        view.entries.len(),
        view.measured_bytes,
        match view.coverage {
            catalogue::Coverage::Complete => "complete",
            catalogue::Coverage::Incomplete => "incomplete",
            catalogue::Coverage::Unavailable => unreachable!("handled above"),
        }
    );
    for entry in &view.entries {
        out.push_str(&entry_line(entry)?);
    }
    for conflict in &view.conflicts {
        let sources = conflict
            .sources
            .iter()
            .map(|source| {
                format!(
                    "{}:{}@{}",
                    source.scope,
                    source.path.display(),
                    source.revision.as_deref().unwrap_or("unavailable")
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let _ = writeln!(
            out,
            "conflict: name={} sources=[{sources}] (native effective selection keeps both)",
            conflict.name
        );
    }
    for duplicate in &view.duplicates {
        let _ = writeln!(out, "duplicate_link: {duplicate}");
    }
    for note in &view.notes {
        let _ = writeln!(out, "incomplete: {note}");
    }
    if view.truncated {
        let _ = writeln!(
            out,
            "omitted={} beyond the {limit}-byte catalogue allowance; route={}",
            view.omitted,
            catalogue::remainder_route()
        );
    }
    out.push_str(
        "awareness=discovery-only: revision identity here is not current-turn delivery; read codex-harness skills identity --path DIRECTORY for a live revision\n",
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::Coverage;
    use std::path::PathBuf;

    fn entry(name: &str, revision: Option<&str>, enabled: bool) -> catalogue::Entry {
        catalogue::Entry {
            name: name.into(),
            scope: "repo".into(),
            applicability: "when useful".into(),
            path: PathBuf::from(format!("skills/{name}")),
            revision: revision.map(|revision| revision.into()),
            enabled,
        }
    }

    fn view(entries: Vec<catalogue::Entry>) -> catalogue::View {
        catalogue::View {
            coverage: Coverage::Complete,
            entries,
            conflicts: Vec::new(),
            duplicates: Vec::new(),
            notes: Vec::new(),
            truncated: false,
            omitted: 0,
            measured_bytes: 0,
        }
    }

    #[test]
    fn delivered_size_matches_the_rendered_line() {
        let enabled = entry("demo", Some("abc123"), true);
        assert_eq!(entry_line(&enabled).unwrap().len(), entry_size(&enabled));
        let disabled = entry("demo", None, false);
        assert_eq!(entry_line(&disabled).unwrap().len(), entry_size(&disabled));
        assert!(
            entry_line(&disabled)
                .unwrap()
                .contains("revision=unavailable")
        );
    }

    #[test]
    fn malformed_and_injected_fields_are_refused() {
        assert!(entry_line(&entry("demo", Some("abc"), true)).is_ok());
        assert!(entry_line(&entry("demo\ninject", Some("abc"), true)).is_err());
        assert!(entry_line(&entry("<script>", Some("abc"), true)).is_err());
        assert!(entry_line(&entry("demo", Some(""), true)).is_err());
    }

    #[test]
    fn incomplete_and_truncated_views_never_claim_the_complete_set() {
        let mut bounded = view(vec![entry("demo", Some("abc"), true)]);
        bounded.coverage = Coverage::Incomplete;
        bounded.notes = vec!["native discovery error at bad/SKILL.md: missing frontmatter".into()];
        bounded.truncated = true;
        bounded.omitted = 4;
        let text = render(&bounded, 512).unwrap();
        assert!(text.contains("coverage=incomplete"));
        assert!(text.contains("omitted=4 beyond the 512-byte catalogue allowance"));
        assert!(
            text.contains("route=codex-harness skills catalogue --limit 1048576"),
            "{text}"
        );
        assert!(text.contains("incomplete: native discovery error"));
        assert!(text.contains("awareness=discovery-only"));

        let mut conflict = view(vec![entry("demo", Some("abc"), true)]);
        conflict.conflicts = vec![catalogue::Conflict {
            name: "demo".into(),
            sources: vec![
                catalogue::Source {
                    scope: "repo".into(),
                    path: PathBuf::from("skills/demo"),
                    revision: Some("abc".into()),
                },
                catalogue::Source {
                    scope: "user".into(),
                    path: PathBuf::from("user/skills/demo"),
                    revision: Some("def".into()),
                },
            ],
        }];
        let text = render(&conflict, 4096).unwrap();
        assert!(text.contains("conflict: name=demo sources=[repo:"));
        assert!(text.contains("native effective selection keeps both"));
    }

    #[test]
    fn unavailable_native_is_explicit_and_offers_only_the_route() {
        let unavailable = catalogue::View::unavailable("native launch registration is missing");
        let text = render(&unavailable, 512).unwrap();
        assert!(text.contains("native_discovery=unavailable"));
        assert!(text.contains("reason=native launch registration is missing"));
        assert!(text.contains("effective_set=unknown"));
        assert!(text.contains("remedy=restore the native read"));
        assert!(!text.contains("name="), "no complete-looking scan: {text}");
    }
}
