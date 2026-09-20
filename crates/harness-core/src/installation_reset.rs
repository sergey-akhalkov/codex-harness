//! Deliberate repair for installations whose recorded link ownership no
//! longer matches reality. Reset removes recorded link objects only; it never
//! touches their targets or regular files, and it writes a receipt before the
//! first removal. The ordinary lifecycle verbs keep refusing silent repair.
use serde::Serialize;
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_METADATA_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct ResetReport {
    pub status: &'static str,
    pub removed_links: Vec<PathBuf>,
    pub already_missing: Vec<PathBuf>,
    pub preserved_foreign: Vec<PathBuf>,
    pub receipt: PathBuf,
}

/// Paths recorded as owned in either metadata schema, read leniently: reset
/// must work exactly when strict validation cannot.
fn owned_link_paths(bytes: &[u8]) -> io::Result<Vec<PathBuf>> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| io::Error::other("installation metadata is not readable JSON"))?;
    let mut paths = Vec::new();
    for link in value["links"]
        .as_array()
        .ok_or_else(|| io::Error::other("installation metadata records no links"))?
    {
        if !link["owned"].as_bool().unwrap_or(false) {
            continue;
        }
        if let Some(raw) = link["object"]["path"]
            .as_str()
            .or_else(|| link["destination"].as_str())
            && local_absolute(Path::new(raw))
        {
            paths.push(PathBuf::from(raw));
        }
    }
    Ok(paths)
}

fn local_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.components().next().is_some_and(|component| {
            matches!(
                component,
                Component::Prefix(prefix)
                    if matches!(prefix.kind(), std::path::Prefix::Disk(_))
            )
        })
}

fn inside_roots(path: &Path, roots: &[&Path]) -> bool {
    let lower = path.to_string_lossy().to_lowercase();
    roots.iter().any(|root| {
        let root = root.to_string_lossy().trim_end_matches('\\').to_lowercase();
        lower.starts_with(&format!("{root}\\"))
    })
}

fn remove_link(path: &Path) -> io::Result<()> {
    // File and directory reparse points need different removal calls; both
    // remove the link object only and never its target.
    fs::remove_file(path).or_else(|_| fs::remove_dir(path))
}

/// Remove the recorded owned links of one installation and its metadata.
/// Regular files and directories at recorded paths are preserved and
/// reported, as are paths outside the supplied homes.
pub fn run(codex_home: &Path, user_home: &Path) -> io::Result<ResetReport> {
    let metadata = codex_home.join("harness/installation.json");
    let meta = fs::metadata(&metadata).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::other("installation metadata is missing; nothing to reset")
        } else {
            error
        }
    })?;
    if !meta.is_file() || meta.len() > MAX_METADATA_BYTES {
        return Err(io::Error::other(
            "installation metadata is missing or oversized; nothing to reset",
        ));
    }
    let recorded = owned_link_paths(&fs::read(&metadata)?)?;
    let roots = [codex_home, user_home];
    let mut report = ResetReport {
        status: "reset",
        removed_links: Vec::new(),
        already_missing: Vec::new(),
        preserved_foreign: Vec::new(),
        receipt: codex_home.join("harness").join(format!(
            "installation.reset-{}.json",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|value| value.as_millis())
                .unwrap_or_default()
        )),
    };
    // Auditability before the first removal: the receipt lists every planned
    // path even if a later step fails.
    write_receipt(&report, &recorded)?;
    for path in recorded {
        if !inside_roots(&path, &roots) {
            report.preserved_foreign.push(path);
            continue;
        }
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                report.already_missing.push(path);
            }
            Err(error) => return Err(error),
            Ok(entry) if entry.file_type().is_symlink() => {
                remove_link(&path)?;
                report.removed_links.push(path);
            }
            Ok(_) => report.preserved_foreign.push(path),
        }
    }
    fs::remove_file(&metadata)?;
    write_receipt(&report, &[])?;
    Ok(report)
}

fn write_receipt(report: &ResetReport, planned: &[PathBuf]) -> io::Result<()> {
    let mut document = serde_json::to_value(report)?;
    if !planned.is_empty() {
        document["planned"] = serde_json::to_value(planned)?;
    }
    if let Some(parent) = report.receipt.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&report.receipt, serde_json::to_vec_pretty(&document)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(home: &Path, links: serde_json::Value) -> PathBuf {
        let path = home.join("harness/installation.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 2,
                "settings": {"sourceRoot": "D:\\nonexistent"},
                "metadataIdentity": {},
                "links": links,
                "checksum": "stale"
            }))
            .unwrap(),
        )
        .unwrap();
        path
    }

    #[test]
    fn reset_removes_only_recorded_links_inside_the_homes() {
        let root = tempfile::tempdir().unwrap();
        let codex = root.path().join("codex-home");
        let user = root.path().join("user-home");
        fs::create_dir_all(codex.join("harness/bin")).unwrap();
        fs::create_dir_all(user.join(".agents/skills")).unwrap();
        let target = root.path().join("target.txt");
        fs::write(&target, "target survives\n").unwrap();
        let link = codex.join("harness/bin/codex-harness.exe");
        std::os::windows::fs::symlink_file(&target, &link).unwrap();
        let regular = user.join(".agents/skills/foreign.txt");
        fs::write(&regular, "user file\n").unwrap();
        let missing = user.join(".agents/skills/team-lead");
        let outside = root.path().join("outside.link");
        std::os::windows::fs::symlink_file(&target, &outside).unwrap();
        let metadata_path = metadata(
            &codex,
            serde_json::json!([
                {"owned": true, "object": {"path": link.to_string_lossy()}},
                {"owned": true, "object": {"path": regular.to_string_lossy()}},
                {"owned": true, "object": {"path": missing.to_string_lossy()}},
                {"owned": true, "object": {"path": outside.to_string_lossy()}},
                {"owned": false, "object": {"path": target.to_string_lossy()}}
            ]),
        );
        let report = run(&codex, &user).unwrap();
        assert_eq!(report.removed_links, std::slice::from_ref(&link));
        assert_eq!(report.already_missing, std::slice::from_ref(&missing));
        assert!(report.preserved_foreign.contains(&regular));
        assert!(report.preserved_foreign.contains(&outside));
        assert_eq!(fs::read_to_string(&target).unwrap(), "target survives\n");
        assert!(regular.is_file());
        assert!(outside.is_symlink());
        assert!(report.receipt.is_file());
        assert!(!metadata_path.exists());
    }

    #[test]
    fn reset_rejects_missing_metadata() {
        let root = tempfile::tempdir().unwrap();
        let error = run(&root.path().join("home"), root.path()).unwrap_err();
        assert!(error.to_string().contains("nothing to reset"));
    }
}
