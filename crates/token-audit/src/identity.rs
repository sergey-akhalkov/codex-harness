//! Hashed identities for the report and the explicit private-source record.
//!
//! The report never carries a raw local path: project and workspace identities
//! are hashes of the recorded directory name. Raw names leave the process only
//! when the caller names an explicit local destination with
//! `--private-sources`, which must stay outside tracked sources.
use crate::{SCHEMA_VERSION, model::Scan};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const PROJECT_PREFIX: &str = "project";
const ROOT_PREFIX: &str = "sessions-root";

/// Stable hashed identity of a project or workspace directory name.
pub fn project_identity(label: &str) -> String {
    digest(PROJECT_PREFIX, &label.to_lowercase())
}

/// Stable hashed identity of the scanned sessions root.
pub fn directory_identity(path: &Path) -> String {
    digest(ROOT_PREFIX, &path.to_string_lossy())
}

fn digest(prefix: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// Writes the explicit local private-source record: source digests and the raw
/// project directory names behind the report's hashed identities.
pub fn write_private_sources(path: &Path, scan: &Scan) -> io::Result<()> {
    if path.is_dir() {
        return Err(io::Error::other(
            "private source destination is a directory",
        ));
    }
    let destination = resolve(path)?;
    for input in &scan.inputs {
        if resolve(input)? == destination {
            return Err(io::Error::other(
                "private source destination must not overwrite a scanned rollout",
            ));
        }
    }
    let mut sources = Vec::new();
    for (index, input) in scan.inputs.iter().enumerate() {
        sources.push(json!({
            "input": index + 1,
            "sha256": file_digest(input)?,
            "bytes": fs::metadata(input)?.len(),
            "suffix": suffix(input),
        }));
    }
    let projects: Vec<_> = scan
        .projects
        .iter()
        .map(|(identity, label)| json!({"identity": identity, "label": label}))
        .collect();
    let record = json!({
        "schema_version": SCHEMA_VERSION,
        "generated_at": scan.report.generated_at,
        "projects": projects,
        "sources": sources,
    });
    let rendered = serde_json::to_string_pretty(&record).map_err(io::Error::other)?;
    fs::write(path, format!("{rendered}\n"))
}

fn suffix(path: &Path) -> String {
    path.extension().map_or_else(String::new, |value| {
        format!(".{}", value.to_string_lossy().to_lowercase())
    })
}

fn resolve(path: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == io::ErrorKind::NotFound => std::path::absolute(path),
        Err(error) => Err(error),
    }
}

fn file_digest(path: &Path) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut file = fs::File::open(path)?;
    io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}
