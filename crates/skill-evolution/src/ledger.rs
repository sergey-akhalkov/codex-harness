//! Installation-owned invocation ledger. No skill bodies, transcripts or secrets.

use crate::invalid;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Explicit,
    Implicit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Parent,
    Child,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    pub path: String,
    pub revision: String,
    pub scope: String,
    pub kind: Kind,
    pub worktree: String,
    pub session: String,
    pub role: Role,
    pub timestamp: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LastInvocation {
    Unknown,
    NotObserved { window_start: f64, window_end: f64 },
    Observed(Record),
}

pub fn append(path: &Path, record: &Record) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut line = serde_json::to_vec(record)?;
    line.push(b'\n');
    file.write_all(&line)?;
    Ok(())
}

pub fn load(path: &Path) -> io::Result<Vec<Record>> {
    match fs::File::open(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
        Ok(file) => {
            if file.metadata()?.len() > LIMIT {
                return Err(invalid("invocation ledger exceeds its bound"));
            }
            let mut records = Vec::new();
            for line in BufReader::new(file).lines() {
                let line = line?;
                if line.is_empty() {
                    continue;
                }
                records
                    .push(serde_json::from_str(&line).map_err(|_| invalid("invalid ledger row"))?);
            }
            Ok(records)
        }
    }
}

pub fn last_for(records: &[Record], name: &str, coverage: Option<(f64, f64)>) -> LastInvocation {
    let latest = records
        .iter()
        .filter(|row| row.name == name)
        .max_by(|a, b| {
            a.timestamp
                .partial_cmp(&b.timestamp)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    match latest {
        Some(row) => LastInvocation::Observed(row.clone()),
        None => match coverage {
            Some((window_start, window_end)) => LastInvocation::NotObserved {
                window_start,
                window_end,
            },
            None => LastInvocation::Unknown,
        },
    }
}

/// Classify a native session event without storing bodies or transcripts.
pub fn classify_event(event: &Value) -> Option<Kind> {
    let item = event.get("item").or_else(|| event.pointer("/payload/item"));
    if let Some(item) = item {
        match item.get("type").and_then(Value::as_str) {
            Some("skill") | Some("skill_invocation") => return Some(Kind::Explicit),
            _ => {}
        }
    }
    if event.get("type").and_then(Value::as_str) == Some("skill") {
        return Some(Kind::Explicit);
    }
    if referenced_skill_md(event) {
        return Some(Kind::Implicit);
    }
    None
}

fn referenced_skill_md(event: &Value) -> bool {
    let ty = event
        .pointer("/item/type")
        .or_else(|| event.pointer("/payload/item/type"))
        .or_else(|| event.pointer("/payload/type"))
        .and_then(Value::as_str);
    if !matches!(ty, Some("command_execution") | Some("mcp_tool_call")) {
        return false;
    }
    let haystacks = [
        event.pointer("/item/command"),
        event.pointer("/payload/item/command"),
        event.pointer("/item/arguments/path"),
        event.pointer("/payload/item/arguments/path"),
    ];
    haystacks.iter().any(|value| {
        value
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("SKILL.md"))
    })
}

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn default_path(codex_home: &Path) -> PathBuf {
    codex_home.join("harness/skill-invocation-ledger.jsonl")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalogue_presence_is_not_a_call_and_unknown_is_not_never() {
        let catalogue = json!({"type":"skills.list","skills":[{"name":"project-verification"}]});
        assert!(classify_event(&catalogue).is_none());
        assert_eq!(
            last_for(&[], "project-verification", None),
            LastInvocation::Unknown
        );
        let observed = last_for(&[], "project-verification", Some((1.0, 2.0)));
        assert!(matches!(observed, LastInvocation::NotObserved { .. }));
    }

    #[test]
    fn explicit_item_and_skill_md_read_are_distinct() {
        let explicit = json!({"item":{"type":"skill","name":"project-verification"}});
        let implicit =
            json!({"item":{"type":"command_execution","command":"Get-Content SKILL.md"}});
        assert_eq!(classify_event(&explicit), Some(Kind::Explicit));
        assert_eq!(classify_event(&implicit), Some(Kind::Implicit));
        let root = tempfile::tempdir().unwrap();
        let path = default_path(root.path());
        append(
            &path,
            &Record {
                name: "project-verification".into(),
                path: "SKILL.md".into(),
                revision: "abc".into(),
                scope: "project".into(),
                kind: Kind::Implicit,
                worktree: "repo".into(),
                session: "s1".into(),
                role: Role::Parent,
                timestamp: 10.0,
            },
        )
        .unwrap();
        let rows = load(&path).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(
            !serde_json::to_string(&rows[0])
                .unwrap()
                .contains("transcript")
        );
    }
}
