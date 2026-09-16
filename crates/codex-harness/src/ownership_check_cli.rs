//! CLI adapter for the migration's executable ownership accounting.
use harness_core::executable_ownership;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::PathBuf;

pub fn run(args: &[OsString]) -> io::Result<i32> {
    let mut options = BTreeMap::new();
    let mut json = false;
    let mut iter = args.iter();
    while let Some(key) = iter.next() {
        if key == "--json" {
            json = true;
            continue;
        }
        if !["--source", "--classification"]
            .iter()
            .any(|value| key == value)
        {
            return Err(io::Error::other("unknown ownership-check option"));
        }
        let value = iter
            .next()
            .ok_or_else(|| io::Error::other("missing ownership-check option value"))?;
        if options.insert(key.clone(), PathBuf::from(value)).is_some() {
            return Err(io::Error::other("duplicate ownership-check option"));
        }
    }
    let source = options
        .get(&OsString::from("--source"))
        .cloned()
        .ok_or_else(|| io::Error::other("--source is required"))?;
    let document_path = options
        .get(&OsString::from("--classification"))
        .cloned()
        .unwrap_or_else(|| source.join("docs/evidence/executable-ownership.json"));
    let bytes = fs::read(&document_path).map_err(|error| {
        io::Error::other(format!(
            "cannot read executable ownership classification {}: {error}",
            document_path.display()
        ))
    })?;
    let document: executable_ownership::OwnershipDocument = serde_json::from_slice(&bytes)
        .map_err(|error| {
            io::Error::other(format!(
                "invalid executable ownership classification {}: {error}",
                document_path.display()
            ))
        })?;
    let report = executable_ownership::check(&source, &document)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        let counts = report
            .classified_counts
            .iter()
            .map(|(kind, count)| format!("{kind}: {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "executable ownership: {} executable files ({}), {} scanned Rust files, {} findings",
            report.executable_files,
            if counts.is_empty() {
                "none classified".to_string()
            } else {
                counts
            },
            report.embedded_scan_files,
            report.findings.len()
        );
        for external in &report.external_roots {
            println!("external root {}: {}", external.path, external.reason);
        }
        for finding in &report.findings {
            match &finding.path {
                Some(path) => println!("{} {path}: {}", finding.kind, finding.detail),
                None => println!("{}: {}", finding.kind, finding.detail),
            }
        }
    }
    Ok(if report.is_clean() { 0 } else { 1 })
}
