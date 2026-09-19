//! Candidate packages live outside skill discovery. No Git mutation.

use crate::{invalid, ordinary_metadata, package};
use serde_json::{Map, Value};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const SECRET_VALUES: &[&str] = &["sk-", "BEGIN PRIVATE KEY"];
const SECRET_KEYS: &[&str] = &["api_key", "authorization", "token", "transcript", "secret"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    pub identity: package::Identity,
    pub evidence: PathBuf,
    pub worktree: String,
    pub parent_revision: String,
}

pub fn stage(
    source: &Path,
    destination: &Path,
    discovery_roots: &[PathBuf],
    worktree: &str,
    parent_revision: &str,
    evidence: &Value,
) -> io::Result<Staged> {
    let dest = destination
        .canonicalize()
        .unwrap_or_else(|_| destination.to_path_buf());
    for root in discovery_roots {
        let root = root.canonicalize().unwrap_or_else(|_| root.clone());
        if dest.starts_with(&root) {
            return Err(invalid("candidate staging must stay outside discovery"));
        }
    }
    if dest.components().any(|c| c.as_os_str() == ".agents")
        && dest.components().any(|c| c.as_os_str() == "skills")
    {
        return Err(invalid("candidate staging must stay outside discovery"));
    }
    let identity = package::copy_into(source, destination)?;
    let redacted = redact(evidence);
    let evidence_path = destination.join("evidence.json");
    fs::write(&evidence_path, serde_json::to_vec_pretty(&redacted)?)?;
    ordinary_metadata(&evidence_path)?;
    Ok(Staged {
        identity,
        evidence: evidence_path,
        worktree: worktree.into(),
        parent_revision: parent_revision.into(),
    })
}

fn redact(value: &Value) -> Value {
    match value {
        Value::String(text) if SECRET_VALUES.iter().any(|needle| text.contains(needle)) => {
            Value::String("[redacted]".into())
        }
        Value::Array(values) => Value::Array(values.iter().map(redact).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, value) in map {
                let hidden = SECRET_KEYS
                    .iter()
                    .any(|needle| key.to_ascii_lowercase().contains(*needle));
                out.insert(
                    key.clone(),
                    if hidden {
                        Value::String("[redacted]".into())
                    } else {
                        redact(value)
                    },
                );
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn skill(root: &Path) {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nname: demo\ndescription: Staging fixture.\n---\nBody\n",
        )
        .unwrap();
    }

    #[test]
    fn staging_refuses_discovery_and_redacts_secrets() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("src");
        skill(&source);
        let discovery = root.path().join(".agents/skills");
        fs::create_dir_all(&discovery).unwrap();
        assert!(
            stage(
                &source,
                &discovery.join("demo"),
                std::slice::from_ref(&discovery),
                "wt",
                "parent",
                &json!({"ok": true})
            )
            .is_err()
        );
        let dest = root.path().join("docs/memory/skill-evolution/demo");
        let staged = stage(
            &source,
            &dest,
            &[discovery],
            "wt",
            "parent",
            &json!({"token":"sk-secret","note":"safe","transcript":"user said hi"}),
        )
        .unwrap();
        let evidence: Value = serde_json::from_slice(&fs::read(&staged.evidence).unwrap()).unwrap();
        assert_eq!(evidence["token"], "[redacted]");
        assert_eq!(evidence["note"], "safe");
        assert_eq!(evidence["transcript"], "[redacted]");
        assert!(
            !staged
                .identity
                .root
                .starts_with(root.path().join(".agents/skills"))
        );
        assert_eq!(staged.parent_revision, "parent");
    }
}
