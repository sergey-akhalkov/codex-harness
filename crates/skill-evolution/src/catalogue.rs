//! Native-effective scoped catalogue. Overflow blocks growth, not required skills.
//!
//! The native consumer's `skills/list` result is authoritative for the effective
//! set. This module adds only what that response omits: canonical package
//! identity (revision), config disablement, differing sources for one name,
//! duplicate links to one source and coverage limits. It never scans discovery
//! roots itself, so it cannot become a second maintained catalogue.

use crate::{delivery, invalid, ownership, package, session};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Admission {
    pub owned_count: u32,
    pub limit: u32,
}

impl Admission {
    pub fn overflow(&self) -> bool {
        self.owned_count > self.limit
    }

    pub fn allow_growth(&self, name: &str, adding: bool) -> bool {
        if ownership::protected_name(name) {
            return true;
        }
        if !adding {
            return true;
        }
        self.owned_count < self.limit
    }
}

const NATIVE_SCOPES: [&str; 4] = ["user", "repo", "system", "admin"];

/// One skill from the native effective set with the kit identity native omits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub scope: String,
    pub applicability: String,
    pub path: PathBuf,
    /// Kit revision of the live package; `None` when identity was unreadable.
    pub revision: Option<String>,
    /// Native `enabled` combined with explicit `[[skills.config]]` disablement.
    pub enabled: bool,
}

/// One distinct source of a conflicting name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub scope: String,
    pub path: PathBuf,
    pub revision: Option<String>,
}

/// The same name is effective from two different canonical sources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub name: String,
    pub sources: Vec<Source>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    Complete,
    Incomplete,
    Unavailable,
}

/// Effective skills plus explicit conflicts, duplicates and coverage limits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub coverage: Coverage,
    /// Entries within the delivery allowance; `omitted` counts the rest.
    pub entries: Vec<Entry>,
    pub conflicts: Vec<Conflict>,
    /// One note per link collapsed onto a source already present.
    pub duplicates: Vec<String>,
    /// Reasons the effective set or its identity is not complete.
    pub notes: Vec<String>,
    pub truncated: bool,
    pub omitted: usize,
    /// Complete delivered metadata size before any truncation.
    pub measured_bytes: usize,
}

impl View {
    /// Native discovery produced no usable evidence; the effective set is unknown.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            coverage: Coverage::Unavailable,
            entries: Vec::new(),
            conflicts: Vec::new(),
            duplicates: Vec::new(),
            notes: vec![reason.into()],
            truncated: false,
            omitted: 0,
            measured_bytes: 0,
        }
    }
}

pub fn remainder_route() -> &'static str {
    "codex-harness skills usage"
}

/// Derive the scoped view from one native `skills/list` result for `case`.
///
/// `metadata_limit` bounds the delivered entry metadata, not the discovery:
/// entries beyond it stay explicit through `truncated`, `omitted` and the
/// remainder route. A malformed or mismatched payload is an error, not an
/// empty catalogue.
pub fn derive(
    native: &serde_json::Value,
    case: &Path,
    config: Option<&Path>,
    metadata_limit: usize,
) -> io::Result<View> {
    let case = case.canonicalize()?;
    let data = native
        .get("data")
        .and_then(|data| data.as_array())
        .ok_or_else(|| invalid("native skills/list payload has no data array"))?;
    if data.len() != 1 {
        return Err(invalid(
            "skills response does not identify one requested project",
        ));
    }
    let row = &data[0];
    let cwd = row
        .get("cwd")
        .and_then(|cwd| cwd.as_str())
        .map(Path::new)
        .ok_or_else(|| invalid("skills response lacks project identity"))?;
    if !cwd.is_absolute()
        || cwd
            .canonicalize()
            .map_err(|_| invalid("skills response project is not readable"))?
            != case
    {
        return Err(invalid("skills response project mismatch"));
    }
    let mut notes = Vec::new();
    let errors = row
        .get("errors")
        .and_then(|errors| errors.as_array())
        .ok_or_else(|| invalid("skills response lacks an errors array"))?;
    for error in errors {
        let path = error.get("path").and_then(|path| path.as_str());
        let message = error
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or("unreported native discovery error");
        notes.push(match path {
            Some(path) => format!("native discovery error at {path}: {message}"),
            None => format!("native discovery error: {message}"),
        });
    }
    let skills = row
        .get("skills")
        .and_then(|skills| skills.as_array())
        .ok_or_else(|| invalid("skills response lacks a skills array"))?;
    let mut unique: Vec<Entry> = Vec::new();
    let mut duplicates = Vec::new();
    for skill in skills {
        let Some(entry) = entry(skill, config, &mut notes) else {
            continue;
        };
        match unique.iter().find(|existing| existing.path == entry.path) {
            Some(existing) => duplicates.push(format!(
                "{} '{}' and {} '{}' reach one source at {}",
                existing.scope,
                existing.name,
                entry.scope,
                entry.name,
                entry.path.display()
            )),
            None => unique.push(entry),
        }
    }
    let conflicts = conflicts(&unique);
    let measured_bytes: usize = unique.iter().map(delivery::entry_size).sum();
    let total = unique.len();
    let mut entries = Vec::new();
    let mut used = 0usize;
    for entry in unique {
        let size = delivery::entry_size(&entry);
        if used + size > metadata_limit {
            break;
        }
        used += size;
        entries.push(entry);
    }
    let truncated = entries.len() < total;
    let omitted = total - entries.len();
    let coverage = if notes.is_empty() && !truncated {
        Coverage::Complete
    } else {
        Coverage::Incomplete
    };
    Ok(View {
        coverage,
        entries,
        conflicts,
        duplicates,
        notes,
        truncated,
        omitted,
        measured_bytes,
    })
}

fn entry(
    skill: &serde_json::Value,
    config: Option<&Path>,
    notes: &mut Vec<String>,
) -> Option<Entry> {
    let name = skill
        .get("name")
        .and_then(|name| name.as_str())
        .filter(|name| !name.is_empty());
    let applicability = skill
        .get("description")
        .and_then(|description| description.as_str());
    let declared = skill
        .get("path")
        .and_then(|path| path.as_str())
        .filter(|path| Path::new(path).is_absolute());
    let scope = skill
        .get("scope")
        .and_then(|scope| scope.as_str())
        .filter(|scope| NATIVE_SCOPES.contains(scope));
    let enabled = skill.get("enabled").and_then(|enabled| enabled.as_bool());
    let (Some(name), Some(applicability), Some(declared), Some(scope), Some(enabled)) =
        (name, applicability, declared, scope, enabled)
    else {
        notes.push(format!("unrecognized native skill entry: {skill}"));
        return None;
    };
    let declared = PathBuf::from(declared);
    let declared_root = declared
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| declared.clone());
    let (path, revision) = match package::load(&declared_root) {
        Ok(identity) => (identity.root, Some(identity.revision)),
        Err(_) => {
            notes.push(format!(
                "identity unavailable for native skill '{name}' at {}",
                declared.display()
            ));
            (
                declared_root
                    .canonicalize()
                    .unwrap_or_else(|_| declared_root.clone()),
                None,
            )
        }
    };
    // `canonicalize` returns a verbatim path on Windows; match the declared path
    // too, because user configuration names the skill the way discovery does.
    let disabled = config.is_some_and(|config| {
        session::config_disables(config, &declared_root) || session::config_disables(config, &path)
    });
    let enabled = enabled && !disabled;
    Some(Entry {
        name: name.to_owned(),
        scope: scope.to_owned(),
        applicability: applicability.to_owned(),
        path,
        revision,
        enabled,
    })
}

fn conflicts(entries: &[Entry]) -> Vec<Conflict> {
    let mut names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| {
            let sources: Vec<Source> = entries
                .iter()
                .filter(|entry| entry.name == name)
                .map(|entry| Source {
                    scope: entry.scope.clone(),
                    path: entry.path.clone(),
                    revision: entry.revision.clone(),
                })
                .collect();
            (sources.len() > 1).then(|| Conflict {
                name: name.to_owned(),
                sources,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn skill(root: &Path, name: &str, body: &str) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Catalogue fixture.\n---\n{body}\n"),
        )
        .unwrap();
    }

    fn case() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("repo")).unwrap();
        root
    }

    fn payload(case: &Path, skills: Vec<serde_json::Value>) -> serde_json::Value {
        json!({"data":[{"cwd":case,"skills":skills,"errors":[]}]})
    }

    fn native(name: &str, dir: &Path, scope: &str) -> serde_json::Value {
        json!({"name":name,"description":format!("{name} applies."),"path":dir.join("SKILL.md"),
            "scope":scope,"enabled":true,"pluginId":null})
    }

    #[test]
    fn overflow_blocks_managed_growth_and_keeps_protected_skills() {
        let admission = Admission {
            owned_count: 3,
            limit: 2,
        };
        assert!(admission.overflow());
        assert!(!admission.allow_growth("demo", true));
        assert!(admission.allow_growth("demo", false));
        assert!(admission.allow_growth("project-verification", true));
    }

    #[test]
    fn duplicate_links_to_one_source_keep_one_revision_and_conflicts_stay_distinct() {
        let root = case();
        let case = root.path().join("repo");
        let local = case.join(".agents/skills/shared");
        let conflict = case.join(".agents/skills/other");
        skill(&local, "shared", "one source");
        skill(&conflict, "shared", "different source");
        let view = derive(
            &payload(
                &case,
                vec![
                    native("shared", &local, "repo"),
                    native("shared", &local, "user"),
                    native("shared", &conflict, "repo"),
                ],
            ),
            &case,
            None,
            usize::MAX,
        )
        .unwrap();
        assert_eq!(view.coverage, Coverage::Complete);
        assert_eq!(view.entries.len(), 2, "one entry per canonical source");
        assert_eq!(view.duplicates.len(), 1, "the extra link stays explicit");
        assert!(view.duplicates[0].contains("reach one source"));
        assert_eq!(view.conflicts.len(), 1);
        let conflict = &view.conflicts[0];
        assert_eq!(conflict.name, "shared");
        assert_eq!(conflict.sources.len(), 2);
        assert!(
            conflict
                .sources
                .iter()
                .all(|source| source.revision.as_deref().is_some_and(|r| r.len() > 16))
        );
        assert!(view.entries.iter().all(|entry| entry.revision.is_some()));
    }

    #[test]
    fn disablement_and_missing_identity_stay_explicit() {
        let root = case();
        let case = root.path().join("repo");
        let enabled = case.join(".agents/skills/enabled");
        let disabled = case.join(".agents/skills/disabled");
        let absent = case.join(".agents/skills/absent");
        skill(&enabled, "enabled", "enabled");
        skill(&disabled, "disabled", "disabled");
        let config = root.path().join("config.toml");
        fs::write(
            &config,
            format!(
                "[[skills.config]]\npath = {}\nenabled = false\n",
                serde_json::to_string(&disabled.join("SKILL.md").to_string_lossy().as_ref())
                    .unwrap()
            ),
        )
        .unwrap();
        let view = derive(
            &payload(
                &case,
                vec![
                    native("enabled", &enabled, "repo"),
                    native("disabled", &disabled, "repo"),
                    native("absent", &absent, "repo"),
                ],
            ),
            &case,
            Some(&config),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(view.coverage, Coverage::Incomplete);
        let row = |name: &str| {
            view.entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap()
        };
        assert!(row("enabled").enabled);
        assert!(!row("disabled").enabled, "config disablement is retained");
        assert!(row("absent").revision.is_none());
        assert!(
            view.notes
                .iter()
                .any(|note| note.contains("identity unavailable") && note.contains("absent"))
        );
    }

    #[test]
    fn native_errors_are_incomplete_and_truncation_keeps_a_route() {
        let root = case();
        let case = root.path().join("repo");
        let first = case.join(".agents/skills/first");
        let second = case.join(".agents/skills/second");
        skill(&first, "first", "first");
        skill(&second, "second", "second");
        let mut broken = payload(&case, vec![native("first", &first, "repo")]);
        broken["data"][0]["errors"] =
            json!([{"path":case.join("bad/SKILL.md"),"message":"missing frontmatter"}]);
        let view = derive(&broken, &case, None, usize::MAX).unwrap();
        assert_eq!(view.coverage, Coverage::Incomplete);
        assert!(view.notes[0].contains("missing frontmatter"));

        let full = payload(
            &case,
            vec![
                native("first", &first, "repo"),
                native("second", &second, "repo"),
            ],
        );
        let sized = derive(&full, &case, None, usize::MAX).unwrap();
        assert!(sized.measured_bytes > 0);
        let bounded = derive(&full, &case, None, sized.measured_bytes / 2).unwrap();
        assert!(bounded.truncated);
        assert_eq!(bounded.coverage, Coverage::Incomplete);
        assert_eq!(bounded.omitted, 1);
        assert_eq!(bounded.measured_bytes, sized.measured_bytes);
        assert_eq!(remainder_route(), "codex-harness skills usage");
    }

    #[test]
    fn malformed_and_mismatched_native_payloads_are_errors_not_empty_catalogues() {
        let root = case();
        let case = root.path().join("repo");
        assert!(derive(&json!({}), &case, None, usize::MAX).is_err());
        let empty = json!({"data":[]});
        assert!(derive(&empty, &case, None, usize::MAX).is_err());
        let elsewhere = payload(&root.path().join("elsewhere"), Vec::new());
        assert!(derive(&elsewhere, &case, None, usize::MAX).is_err());
        let unavailable = View::unavailable("native launch registration is missing");
        assert_eq!(unavailable.coverage, Coverage::Unavailable);
        assert!(unavailable.entries.is_empty());
    }
}
