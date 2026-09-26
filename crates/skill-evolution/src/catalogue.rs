//! Native-effective scoped catalogue. Overflow blocks growth, not required skills.
//!
//! The native consumer's `skills/list` result is authoritative for the effective
//! set. This module adds only what that response omits: package revision
//! identity, config disablement, differing sources for one name, duplicate links
//! and coverage limits. It never scans discovery roots itself.

use crate::{delivery, invalid, ownership, package, session};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
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
        ownership::protected_name(name) || !adding || self.owned_count < self.limit
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Coverage {
    #[default]
    Complete,
    Incomplete,
    Unavailable,
}

/// Effective skills plus explicit conflicts, duplicates and coverage limits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
            notes: vec![reason.into()],
            ..Self::default()
        }
    }
}

/// Bounded retrieval route for entries omitted by the delivery allowance: it
/// reruns this catalogue at the largest accepted allowance.
pub fn remainder_route() -> &'static str {
    "codex-harness skills catalogue --limit 1048576"
}

/// The `skills/list` result fields this view relies on.
#[derive(Deserialize)]
struct Listed {
    data: Vec<ListedRow>,
}

#[derive(Deserialize)]
struct ListedRow {
    cwd: PathBuf,
    skills: Vec<serde_json::Value>,
    errors: Vec<serde_json::Value>,
}

/// Derive the scoped view from one native `skills/list` result for `case`.
/// `metadata_limit` bounds the delivered entry metadata, not the discovery:
/// entries beyond it stay explicit through truncation; a malformed or
/// mismatched payload is an error, not an empty catalogue.
pub fn derive(
    native: &serde_json::Value,
    case: &Path,
    config: Option<&Path>,
    metadata_limit: usize,
) -> io::Result<View> {
    let case = case.canonicalize()?;
    let listed = Listed::deserialize(native).map_err(|error| {
        io::Error::other(format!("native skills/list result is unusable: {error}"))
    })?;
    let [row] = listed.data.as_slice() else {
        return Err(invalid(
            "skills response does not identify one requested project",
        ));
    };
    if !row.cwd.is_absolute()
        || row
            .cwd
            .canonicalize()
            .map_err(|_| invalid("skills response project is not readable"))?
            != case
    {
        return Err(invalid("skills response project mismatch"));
    }
    let mut notes: Vec<String> = row.errors.iter().map(discovery_note).collect();
    // One entry per canonical source; later links to one source stay explicit.
    let mut unique: Vec<Entry> = Vec::new();
    let mut duplicates = Vec::new();
    for skill in &row.skills {
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
    let mut used = 0;
    for entry in unique {
        let size = delivery::entry_size(&entry);
        if used + size > metadata_limit {
            break;
        }
        used += size;
        entries.push(entry);
    }
    let truncated = entries.len() < total;
    Ok(View {
        coverage: if notes.is_empty() && !truncated {
            Coverage::Complete
        } else {
            Coverage::Incomplete
        },
        omitted: total - entries.len(),
        entries,
        conflicts,
        duplicates,
        notes,
        truncated,
        measured_bytes,
    })
}

/// The native skill fields this view consumes.
#[derive(Deserialize)]
struct Native {
    name: String,
    description: String,
    path: PathBuf,
    scope: String,
    enabled: bool,
}

fn entry(
    skill: &serde_json::Value,
    config: Option<&Path>,
    notes: &mut Vec<String>,
) -> Option<Entry> {
    let native = serde_json::from_value::<Native>(skill.clone())
        .ok()
        .filter(|native| {
            !native.name.is_empty()
                && native.path.is_absolute()
                && NATIVE_SCOPES.contains(&native.scope.as_str())
        });
    let Some(native) = native else {
        notes.push(format!("unrecognized native skill entry: {skill}"));
        return None;
    };
    let declared_root = native.path.parent().unwrap_or(&native.path).to_path_buf();
    let (path, revision) = match package::load(&declared_root) {
        Ok(identity) => (identity.root, Some(identity.revision)),
        Err(_) => {
            notes.push(format!(
                "identity unavailable for native skill '{}' at {}",
                native.name,
                native.path.display()
            ));
            (
                declared_root
                    .canonicalize()
                    .unwrap_or_else(|_| declared_root.clone()),
                None,
            )
        }
    };
    // `canonicalize` returns a verbatim path on Windows; user configuration
    // names the skill the way discovery does, so check the declared path too.
    let disabled = config.is_some_and(|config| {
        session::config_disables(config, &declared_root) || session::config_disables(config, &path)
    });
    Some(Entry {
        name: native.name,
        scope: native.scope,
        applicability: native.description,
        path,
        revision,
        enabled: native.enabled && !disabled,
    })
}

/// One explicit note per native discovery error, with its source path when known.
fn discovery_note(error: &serde_json::Value) -> String {
    let message = error
        .get("message")
        .and_then(|message| message.as_str())
        .unwrap_or("unreported native discovery error");
    match error.get("path").and_then(|path| path.as_str()) {
        Some(path) => format!("native discovery error at {path}: {message}"),
        None => format!("native discovery error: {message}"),
    }
}

fn conflicts(entries: &[Entry]) -> Vec<Conflict> {
    let mut by_name: BTreeMap<&str, Vec<Source>> = BTreeMap::new();
    for entry in entries {
        by_name.entry(&entry.name).or_default().push(Source {
            scope: entry.scope.clone(),
            path: entry.path.clone(),
            revision: entry.revision.clone(),
        });
    }
    by_name
        .into_iter()
        .filter_map(|(name, sources)| {
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
            format!("---\nname: {name}\ndescription: fixture.\n---\n{body}\n"),
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
        let (local, other) = (
            case.join(".agents/skills/shared"),
            case.join(".agents/skills/other"),
        );
        skill(&local, "shared", "one source");
        skill(&other, "shared", "different source");
        let links = vec![
            native("shared", &local, "repo"),
            native("shared", &local, "user"),
            native("shared", &other, "repo"),
        ];
        let view = derive(&payload(&case, links), &case, None, usize::MAX).unwrap();
        assert_eq!(view.coverage, Coverage::Complete);
        assert_eq!(view.entries.len(), 2, "one entry per canonical source");
        assert_eq!(view.duplicates.len(), 1, "the extra link stays explicit");
        assert!(view.duplicates[0].contains("reach one source"));
        assert_eq!(view.conflicts.len(), 1);
        let c = &view.conflicts[0];
        assert_eq!(c.name, "shared");
        assert_eq!(c.sources.len(), 2);
        assert!(
            c.sources
                .iter()
                .all(|s| s.revision.as_deref().is_some_and(|r| r.len() > 16))
        );
        assert!(view.entries.iter().all(|entry| entry.revision.is_some()));
    }

    #[test]
    fn disablement_and_missing_identity_stay_explicit() {
        let root = case();
        let case = root.path().join("repo");
        let (enabled, disabled, absent) = (
            case.join(".agents/skills/enabled"),
            case.join(".agents/skills/disabled"),
            case.join(".agents/skills/absent"),
        );
        skill(&enabled, "enabled", "enabled");
        skill(&disabled, "disabled", "disabled");
        let config = root.path().join("config.toml");
        let disabled_md = disabled.join("SKILL.md").to_string_lossy().into_owned();
        fs::write(
            &config,
            format!(
                "[[skills.config]]\npath = {}\nenabled = false\n",
                serde_json::to_string(&disabled_md).unwrap()
            ),
        )
        .unwrap();
        let links = vec![
            native("enabled", &enabled, "repo"),
            native("disabled", &disabled, "repo"),
            native("absent", &absent, "repo"),
        ];
        let view = derive(&payload(&case, links), &case, Some(&config), usize::MAX).unwrap();
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
                .any(|n| n.contains("identity unavailable") && n.contains("absent"))
        );
    }

    #[test]
    fn native_errors_are_incomplete_and_truncation_keeps_a_route() {
        let root = case();
        let case = root.path().join("repo");
        let (first, second) = (
            case.join(".agents/skills/first"),
            case.join(".agents/skills/second"),
        );
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
        assert_eq!(
            remainder_route(),
            "codex-harness skills catalogue --limit 1048576"
        );
    }

    #[test]
    fn malformed_and_mismatched_native_payloads_are_errors_not_empty_catalogues() {
        let root = case();
        let case = root.path().join("repo");
        assert!(derive(&json!({}), &case, None, usize::MAX).is_err());
        assert!(derive(&json!({"data":[]}), &case, None, usize::MAX).is_err());
        let elsewhere = payload(&root.path().join("elsewhere"), Vec::new());
        assert!(derive(&elsewhere, &case, None, usize::MAX).is_err());
        let unavailable = View::unavailable("native launch registration is missing");
        assert_eq!(unavailable.coverage, Coverage::Unavailable);
        assert!(unavailable.entries.is_empty());
    }
}
