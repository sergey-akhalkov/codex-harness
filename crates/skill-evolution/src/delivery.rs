//! Bounded catalogue render. A list is not awareness: rendering never claims
//! the complete active set when the native evidence was unavailable, incomplete
//! or truncated, and never claims current-turn delivery.

use crate::{catalogue, invalid};
use std::{fmt::Write as _, io};

const FIELD_LIMIT: usize = 2048;

fn revision(entry: &catalogue::Entry) -> &str {
    entry.revision.as_deref().unwrap_or("unavailable")
}

fn source(source: &catalogue::Source) -> String {
    format!(
        "{}:{}@{}",
        source.scope,
        source.path.display(),
        source.revision.as_deref().unwrap_or("unavailable")
    )
}

/// The exact delivered line of one entry, before validation.
fn line(entry: &catalogue::Entry) -> String {
    format!(
        "name={};scope={};enabled={};applicability={};path={};revision={}\n",
        entry.name,
        entry.scope,
        entry.enabled,
        entry.applicability,
        entry.path.to_string_lossy(),
        revision(entry)
    )
}

/// Delivered bytes of one entry, measured before any truncation.
pub fn entry_size(entry: &catalogue::Entry) -> usize {
    line(entry).len()
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
        if field.len() > FIELD_LIMIT || field.contains(['\0', '<', '>', '\n']) {
            return Err(invalid("malformed or injected catalogue field"));
        }
    }
    if entry.name.is_empty() || path.is_empty() || revision(entry).is_empty() {
        return Err(invalid("incomplete catalogue identity"));
    }
    Ok(line(entry))
}

/// Render the derived view, stating every gap explicitly.
pub fn render(view: &catalogue::View, limit: usize) -> io::Result<String> {
    if view.coverage == catalogue::Coverage::Unavailable {
        let mut out = String::from("native_discovery=unavailable\n");
        out.extend(view.notes.iter().map(|note| format!("reason={note}\n")));
        out.push_str("effective_set=unknown\nremedy=restore the native read (register harness/native-launch.json or pass --upstream PATH), then rerun codex-harness skills catalogue\nawareness=none: this read cannot establish model awareness or current-turn delivery\n");
        return Ok(out);
    }
    let mut out = format!(
        "catalogue: {} effective; delivered={}; measured_metadata_bytes={}; coverage={}\n",
        view.entries.len() + view.omitted,
        view.entries.len(),
        view.measured_bytes,
        if view.coverage == catalogue::Coverage::Complete {
            "complete"
        } else {
            "incomplete"
        }
    );
    for entry in &view.entries {
        out.push_str(&entry_line(entry)?);
    }
    for conflict in &view.conflicts {
        let sources: Vec<String> = conflict.sources.iter().map(source).collect();
        let _ = writeln!(
            out,
            "conflict: name={} sources=[{}] (native effective selection keeps both)",
            conflict.name,
            sources.join(" ")
        );
    }
    out.extend(
        view.duplicates
            .iter()
            .map(|duplicate| format!("duplicate_link: {duplicate}\n")),
    );
    out.extend(
        view.notes
            .iter()
            .map(|note| format!("incomplete: {note}\n")),
    );
    if view.truncated {
        let _ = writeln!(
            out,
            "omitted={} beyond the {limit}-byte catalogue allowance; route={}",
            view.omitted,
            catalogue::remainder_route()
        );
    }
    out.push_str("awareness=discovery-only: revision identity here is not current-turn delivery; read codex-harness skills identity --path DIRECTORY for a live revision\n");
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
            entries,
            ..Default::default()
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
        let source = |scope: &str, path: &str, revision: &str| catalogue::Source {
            scope: scope.into(),
            path: PathBuf::from(path),
            revision: Some(revision.into()),
        };
        conflict.conflicts = vec![catalogue::Conflict {
            name: "demo".into(),
            sources: vec![
                source("repo", "skills/demo", "abc"),
                source("user", "user/skills/demo", "def"),
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
