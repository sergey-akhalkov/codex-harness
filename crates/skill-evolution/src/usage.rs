//! Model-free project-then-global usage report.

use crate::{
    ledger::{self, LastInvocation, Record},
    package,
};
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub name: String,
    pub scope: String,
    pub enabled: bool,
    pub last_invocation: LastInvocation,
    pub kind: Option<ledger::Kind>,
    pub r#where: String,
    pub role: Option<ledger::Role>,
    pub path: PathBuf,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub project: Vec<Row>,
    pub global: Vec<Row>,
    pub project_section: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    pub name: String,
    pub scope: String,
    pub action: &'static str,
    pub reason: &'static str,
}

fn protected(name: &str) -> bool {
    name.starts_with("openspec-")
        || matches!(
            name,
            "skill-evolution" | "skills-usage-analysis" | "project-verification" | "project-memory"
        )
}

pub fn candidates(report: &Report) -> Vec<Candidate> {
    report
        .project
        .iter()
        .chain(report.global.iter())
        .filter_map(|row| {
            if protected(&row.name) {
                return None;
            }
            match row.last_invocation {
                LastInvocation::NotObserved { .. } => Some(Candidate {
                    name: row.name.clone(),
                    scope: row.scope.clone(),
                    action: "disable",
                    reason: "not_observed under complete coverage",
                }),
                LastInvocation::Unknown | LastInvocation::Observed(_) => None,
            }
        })
        .collect()
}

pub fn report(
    cwd: &Path,
    user_home: &Path,
    ledger_path: &Path,
    coverage: Option<(f64, f64)>,
    config: Option<&Path>,
) -> io::Result<Report> {
    let records = ledger::load(ledger_path)?;
    let project_root = cwd.join(".agents/skills");
    let global_root = user_home.join(".agents/skills");
    let project = if project_root.is_dir() {
        list(&project_root, "project", &records, coverage, config)?
    } else {
        Vec::new()
    };
    let mut global = if global_root.is_dir() {
        list(&global_root, "global", &records, coverage, config)?
    } else {
        Vec::new()
    };
    let project_canonical: Vec<_> = project
        .iter()
        .filter_map(|row| row.path.canonicalize().ok())
        .collect();
    global.retain(|row| {
        row.path
            .canonicalize()
            .ok()
            .is_none_or(|path| !project_canonical.contains(&path))
    });
    Ok(Report {
        project_section: project_root.is_dir(),
        project,
        global,
    })
}

fn list(
    root: &Path,
    scope: &str,
    records: &[Record],
    coverage: Option<(f64, f64)>,
    config: Option<&Path>,
) -> io::Result<Vec<Row>> {
    let mut rows = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(root)?.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let identity = match package::load(&path) {
            Ok(identity) => identity,
            Err(_) => continue,
        };
        let last = ledger::last_for(records, &identity.name, coverage);
        let (kind, role, where_) = match &last {
            LastInvocation::Observed(record) => (
                Some(record.kind),
                Some(record.role),
                record.worktree.clone(),
            ),
            _ => (None, None, String::new()),
        };
        rows.push(Row {
            name: identity.name,
            scope: scope.into(),
            enabled: config.is_none_or(|config| !crate::session::config_disables(config, &path)),
            last_invocation: last,
            kind,
            r#where: where_,
            role,
            path: identity.root,
            revision: identity.revision,
        });
    }
    Ok(rows)
}

pub fn analysis_request(prompt: &str) -> bool {
    let prompt = prompt.trim().to_ascii_lowercase();
    if prompt.is_empty() {
        return false;
    }
    let usage = [
        "unused skill",
        "skill usage",
        "library growth",
        "what to disable",
        "last invocation",
    ];
    let skip = ["implement", "fix the", "openspec", "debug"];
    usage.iter().any(|key| prompt.contains(key)) && !skip.iter().any(|key| prompt.contains(key))
}

pub fn render(report: &Report) -> String {
    let mut out = String::new();
    if report.project_section {
        out.push_str("project\n");
        push_rows(&mut out, &report.project);
        out.push_str("global\n");
        push_rows(&mut out, &report.global);
    } else {
        out.push_str("no project skill section applies\n");
        out.push_str("global\n");
        push_rows(&mut out, &report.global);
    }
    out.push_str("candidates\n");
    let listed = candidates(report);
    if listed.is_empty() {
        out.push_str("  (none: unknown last invocation is not a deletion reason)\n");
    } else {
        for row in listed {
            out.push_str(&format!(
                "  {}  {}  {}  {}\n",
                row.scope, row.name, row.action, row.reason
            ));
        }
    }
    out.push_str("no library changes applied\n");
    out
}

fn push_rows(out: &mut String, rows: &[Row]) {
    if rows.is_empty() {
        out.push_str("  (none)\n");
        return;
    }
    for row in rows {
        let last = match &row.last_invocation {
            LastInvocation::Unknown => "unknown".to_owned(),
            LastInvocation::NotObserved { .. } => "not_observed".to_owned(),
            LastInvocation::Observed(record) => format!("{}", record.timestamp as u64),
        };
        let kind = row
            .kind
            .map(|kind| format!("{kind:?}").to_ascii_lowercase())
            .unwrap_or_else(|| "-".into());
        out.push_str(&format!(
            "  {}  {}  {}  {}  {}\n",
            row.name,
            if row.enabled { "enabled" } else { "disabled" },
            last,
            kind,
            row.scope
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_skill(root: &Path, name: &str) {
        fs::create_dir_all(root.join(name)).unwrap();
        fs::write(
            root.join(name).join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Usage fixture.\n---\n"),
        )
        .unwrap();
    }

    #[test]
    fn project_skills_are_listed_first_and_canonical_duplicates_are_omitted() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("repo");
        let user = root.path().join("user");
        write_skill(&project.join(".agents/skills"), "alpha");
        write_skill(&user.join(".agents/skills"), "beta");
        fs::create_dir_all(user.join(".agents/skills")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(
            project.join(".agents/skills/alpha"),
            user.join(".agents/skills/alpha"),
        )
        .unwrap();
        let report = report(
            &project,
            &user,
            &root.path().join("missing-ledger.jsonl"),
            None,
            None,
        )
        .unwrap();
        assert!(report.project_section);
        assert_eq!(report.project[0].name, "alpha");
        assert!(report.global.iter().all(|row| row.name != "alpha"));
        assert_eq!(report.global[0].name, "beta");
        assert!(matches!(
            report.project[0].last_invocation,
            LastInvocation::Unknown
        ));
        let text = render(&report);
        assert!(text.find("project").unwrap() < text.find("global").unwrap());
        assert!(candidates(&report).is_empty());
    }

    #[test]
    fn analysis_skill_does_not_match_ordinary_or_empty_prompts() {
        assert!(analysis_request("which unused skills can I disable?"));
        assert!(!analysis_request("implement the parser"));
        assert!(!analysis_request("fix the build"));
        assert!(!analysis_request(""));
    }
}
