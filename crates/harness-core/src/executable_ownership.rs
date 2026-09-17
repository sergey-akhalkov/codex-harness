//! Executable ownership accounting for the Rust migration boundary.
//!
//! The migration keeps an enforceable inventory instead of trusting file
//! counts: every foreign-language executable file in the working tree must be
//! classified, remaining first-party legacy paths surface as open work, inert
//! analysis data needs a genuine first-party Rust consumer, third-party paths
//! need provenance and cannot hide in first-party roots, and maintained Rust
//! source must not embed or generate foreign-language programs for execution.
//! The forbidden embedded-program markers live in the classification document
//! so this checker never contains those program fragments itself.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Roots that always belong to first-party maintained source. Foreign
/// executables there can never be reclassified as third-party material.
const FIRST_PARTY_ROOTS: &[&str] = &["tools/", "tests/", ".agents/", "global/", "crates/"];

/// The only location where inert language-analysis samples may live.
const INERT_DATA_PREFIX: &str = "tests/fixtures/";

const FIRST_PARTY_LEGACY: &str = "first-party-legacy";
const INERT_DATA: &str = "inert-data";
const THIRD_PARTY: &str = "third-party";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipDocument {
    pub schema_version: u32,
    pub executable_extensions: Vec<String>,
    pub generated_cache_dirs: Vec<String>,
    pub external_roots: Vec<ExternalRoot>,
    pub rust_source_roots: Vec<String>,
    pub embedded_program_markers: Vec<EmbeddedMarker>,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalRoot {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedMarker {
    pub name: String,
    pub needle: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub classification: String,
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub source: String,
    pub executable_files: usize,
    pub classified_counts: BTreeMap<String, usize>,
    pub external_roots: Vec<ExternalRoot>,
    pub embedded_scan_files: usize,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root).ok().map(normalize)
}

fn is_first_party(path: &str) -> bool {
    FIRST_PARTY_ROOTS
        .iter()
        .any(|root| path == root.trim_end_matches('/') || path.starts_with(root))
}

fn executable_extensions(document: &OwnershipDocument) -> BTreeSet<String> {
    document
        .executable_extensions
        .iter()
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

fn has_executable_extension(path: &str, extensions: &BTreeSet<String>) -> bool {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| extensions.contains(&format!(".{value}").to_ascii_lowercase()))
        .unwrap_or(false)
}

struct WalkLimits {
    cache_dirs: BTreeSet<String>,
    external_roots: BTreeSet<String>,
}

impl WalkLimits {
    fn skip_dir(&self, root: &Path, dir: &Path) -> bool {
        dir.file_name().is_some_and(|name| {
            self.cache_dirs
                .contains(&name.to_string_lossy().to_string())
        }) || self.external(root, dir).is_some()
    }

    fn external<'a>(&'a self, root: &Path, path: &'a Path) -> Option<&'a String> {
        let key = relative(root, path)?;
        self.external_roots
            .iter()
            .find(|prefix| key == **prefix || key.starts_with(&format!("{prefix}/")))
    }
}

fn collect_files(
    root: &Path,
    dir: &Path,
    limits: &WalkLimits,
    extensions: &BTreeSet<String>,
    executable: &mut Vec<String>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if limits.skip_dir(root, &path) {
                continue;
            }
            collect_files(root, &path, limits, extensions, executable)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Some(key) = relative(root, &path) else {
            continue;
        };
        if limits.external(root, &path).is_some() {
            continue;
        }
        if has_executable_extension(&key, extensions) {
            executable.push(key);
        }
    }
    Ok(())
}

fn collect_rust_sources(
    root: &Path,
    dir: &Path,
    limits: &WalkLimits,
    out: &mut Vec<PathBuf>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if limits.skip_dir(root, &path) {
                continue;
            }
            collect_rust_sources(root, &path, limits, out)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Some(key) = relative(root, &path) else {
            continue;
        };
        // `tests/fixtures` trees hold Rust program data consumed through
        // include_str!, not maintained first-party implementation source.
        if key.contains("/tests/fixtures/") || key.starts_with("tests/fixtures/") {
            continue;
        }
        if Path::new(&key)
            .extension()
            .is_some_and(|value| value.eq_ignore_ascii_case("rs"))
        {
            out.push(path);
        }
    }
    Ok(())
}

fn finding(kind: &'static str, path: Option<String>, detail: impl Into<String>) -> Finding {
    Finding {
        kind,
        path,
        detail: detail.into(),
    }
}

/// Validate the ownership document against the current working tree.
pub fn check(source: &Path, document: &OwnershipDocument) -> io::Result<Report> {
    if document.schema_version != 1 {
        return Err(io::Error::other(
            "unsupported executable ownership schema version",
        ));
    }
    let limits = WalkLimits {
        cache_dirs: document
            .generated_cache_dirs
            .iter()
            .map(|value| normalize_path_string(value))
            .collect(),
        external_roots: document
            .external_roots
            .iter()
            .map(|root| normalize_path_string(&root.path))
            .collect(),
    };
    let extensions = executable_extensions(document);
    let mut executable_files = Vec::new();
    collect_files(source, source, &limits, &extensions, &mut executable_files)?;
    executable_files.sort();

    let mut findings: Vec<Finding> = Vec::new();
    let mut classified: BTreeMap<String, (&Entry, usize)> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for (index, entry) in document.entries.iter().enumerate() {
        if !matches!(
            entry.classification.as_str(),
            FIRST_PARTY_LEGACY | INERT_DATA | THIRD_PARTY
        ) {
            findings.push(finding(
                "invalid-entry",
                None,
                format!("unknown classification {}", entry.classification),
            ));
            continue;
        }
        if entry.paths.is_empty() {
            findings.push(finding(
                "invalid-entry",
                None,
                format!("{} entry has no paths", entry.classification),
            ));
        }
        for path in &entry.paths {
            let key = normalize_path_string(path);
            if classified.contains_key(&key) {
                findings.push(finding(
                    "invalid-entry",
                    Some(key),
                    "path is classified more than once",
                ));
                continue;
            }
            let absolute = source.join(Path::new(&key));
            if !absolute.is_file() {
                findings.push(finding(
                    "stale-entry",
                    Some(key),
                    "classified path is missing from the working tree",
                ));
                continue;
            }
            if !has_executable_extension(&key, &extensions) {
                findings.push(finding(
                    "invalid-entry",
                    Some(key),
                    "classified path does not use a declared executable extension",
                ));
                continue;
            }
            *counts.entry(entry.classification.clone()).or_default() += 1;
            classified.insert(key, (entry, index));
        }
    }

    for path in &executable_files {
        let Some((entry, _)) = classified.get(path) else {
            findings.push(finding(
                "unclassified-executable",
                Some(path.clone()),
                "foreign-language executable file has no ownership classification",
            ));
            continue;
        };
        match entry.classification.as_str() {
            FIRST_PARTY_LEGACY => {
                if entry.owner_task.as_deref().unwrap_or("").trim().is_empty() {
                    findings.push(finding(
                        "invalid-entry",
                        Some(path.clone()),
                        "first-party legacy path has no owning migration task",
                    ));
                } else {
                    findings.push(finding(
                        "legacy-executable",
                        Some(path.clone()),
                        format!(
                            "first-party foreign-language path remains; owner {}",
                            entry.owner_task.as_deref().unwrap_or_default()
                        ),
                    ));
                }
            }
            INERT_DATA => {
                if !path.starts_with(INERT_DATA_PREFIX) {
                    findings.push(finding(
                        "relabeling",
                        Some(path.clone()),
                        "inert analysis data must live under tests/fixtures/",
                    ));
                }
                let Some(consumer) = entry.consumer.as_deref() else {
                    findings.push(finding(
                        "invalid-entry",
                        Some(path.clone()),
                        "inert data entry has no consumer",
                    ));
                    continue;
                };
                let consumer_path = source.join(Path::new(&normalize_path_string(consumer)));
                let content = fs::read(&consumer_path).ok();
                if content.is_none() || !normalize_path_string(consumer).ends_with(".rs") {
                    findings.push(finding(
                        "relabeling",
                        Some(path.clone()),
                        format!("inert data consumer {consumer} is not an existing Rust source"),
                    ));
                    continue;
                }
                let bytes = content.unwrap_or_default();
                let content = String::from_utf8_lossy(&bytes);
                if !content.contains(path.as_str()) {
                    findings.push(finding(
                        "relabeling",
                        Some(path.clone()),
                        format!("inert data is not referenced by its consumer {consumer}"),
                    ));
                }
            }
            THIRD_PARTY => {
                if entry.provenance.as_deref().unwrap_or("").trim().is_empty() {
                    findings.push(finding(
                        "invalid-entry",
                        Some(path.clone()),
                        "third-party path has no provenance",
                    ));
                }
                if is_first_party(path) {
                    findings.push(finding(
                        "relabeling",
                        Some(path.clone()),
                        "first-party root cannot be reclassified as third-party",
                    ));
                }
            }
            _ => {}
        }
    }

    let mut rust_sources = Vec::new();
    for root in &document.rust_source_roots {
        let path = source.join(Path::new(&normalize_path_string(root)));
        if path.is_dir() {
            collect_rust_sources(source, &path, &limits, &mut rust_sources)?;
        }
    }
    let mut scanned_rust_files = 0usize;
    for file in &rust_sources {
        let content = fs::read(file)?;
        scanned_rust_files += 1;
        for marker in &document.embedded_program_markers {
            if content
                .windows(marker.needle.len())
                .any(|window| window == marker.needle.as_bytes())
            {
                findings.push(finding(
                    "embedded-foreign-program",
                    relative(source, file),
                    format!(
                        "maintained Rust source contains foreign program marker {}",
                        marker.name
                    ),
                ));
            }
        }
    }

    findings.sort_by(|left, right| {
        (
            left.kind,
            left.path.as_deref().unwrap_or_default(),
            &left.detail,
        )
            .cmp(&(
                right.kind,
                right.path.as_deref().unwrap_or_default(),
                &right.detail,
            ))
    });
    Ok(Report {
        source: source.to_string_lossy().to_string(),
        executable_files: executable_files.len(),
        classified_counts: counts,
        external_roots: document.external_roots.clone(),
        embedded_scan_files: scanned_rust_files,
        findings,
    })
}

fn normalize_path_string(value: &str) -> String {
    value.replace('\\', "/")
}
