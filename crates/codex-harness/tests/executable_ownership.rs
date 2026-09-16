//! Executable ownership accounting keeps the migration boundary enforceable.
use harness_core::executable_ownership::{
    self, EmbeddedMarker, Entry, ExternalRoot, OwnershipDocument,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn checkout() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn document(entries: Vec<Entry>) -> OwnershipDocument {
    OwnershipDocument {
        schema_version: 1,
        executable_extensions: vec![".py".into(), ".ps1".into()],
        generated_cache_dirs: vec![".git".into(), "target".into(), "__pycache__".into()],
        external_roots: vec![ExternalRoot {
            path: ".venv".into(),
            reason: "external dependency environment".into(),
        }],
        rust_source_roots: vec!["crates".into(), "tools/rtk-adapter".into()],
        embedded_program_markers: vec![
            EmbeddedMarker {
                name: "python-shebang".into(),
                // Assemble program markers so this maintained test source
                // never contains a forbidden foreign program fragment itself.
                needle: format!("#!/usr/bin/{tail}", tail = "env"),
            },
            EmbeddedMarker {
                name: "powershell-requires".into(),
                needle: format!("#requ{i} - Version", i = "ires"),
            },
        ],
        entries,
    }
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn legacy(paths: &[&str]) -> Entry {
    Entry {
        classification: "first-party-legacy".into(),
        paths: paths.iter().map(|value| value.to_string()).collect(),
        owner_task: Some("migrate-harness-to-rust#8.2".into()),
        note: Some("fixture classification".into()),
        consumer: None,
        provenance: None,
    }
}

fn inert(paths: &[&str], consumer: &str) -> Entry {
    Entry {
        classification: "inert-data".into(),
        paths: paths.iter().map(|value| value.to_string()).collect(),
        owner_task: None,
        note: Some("fixture analysis sample".into()),
        consumer: Some(consumer.into()),
        provenance: None,
    }
}

fn third_party(paths: &[&str], provenance: &str) -> Entry {
    Entry {
        classification: "third-party".into(),
        paths: paths.iter().map(|value| value.to_string()).collect(),
        owner_task: None,
        note: Some("fixture dependency".into()),
        consumer: None,
        provenance: Some(provenance.into()),
    }
}

fn kinds(report: &executable_ownership::Report) -> Vec<String> {
    report
        .findings
        .iter()
        .map(|finding| finding.kind.to_string())
        .collect()
}

fn clean_fixture(root: &Path) -> OwnershipDocument {
    write(&root.join("crates/demo/src/main.rs"), "fn main() {}\n");
    write(
        &root.join("tools/rtk-adapter/src/main.rs"),
        "fn main() {}\n",
    );
    write(
        &root.join("crates/demo/src/consumer.rs"),
        "pub const SAMPLE: &str = \"tests/fixtures/lsp/sample.py\";\n",
    );
    write(
        &root.join("tests/fixtures/lsp/sample.py"),
        "# language-analysis sample\n",
    );
    write(&root.join(".venv/lib/tool.py"), "# external runtime\n");
    document(vec![inert(
        &["tests/fixtures/lsp/sample.py"],
        "crates/demo/src/consumer.rs",
    )])
}

#[test]
fn third_party_dependencies_and_inert_data_remain_distinguishable() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("tree");
    fs::create_dir_all(&source).unwrap();
    let mut doc = clean_fixture(&source);
    write(
        &source.join("vendor/upstream/tool.py"),
        "# vendored upstream\n",
    );
    doc.entries.push(third_party(
        &["vendor/upstream/tool.py"],
        "upstream-tool 1.2.3 (vendored release archive)",
    ));
    let report = executable_ownership::check(&source, &doc).unwrap();
    assert!(report.is_clean(), "{:?}", report.findings);
    assert_eq!(report.executable_files, 2);
    assert_eq!(report.classified_counts.get("inert-data"), Some(&1));
    assert_eq!(report.classified_counts.get("third-party"), Some(&1));
}

#[test]
fn unclassified_foreign_executable_fails() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("tree");
    fs::create_dir_all(&source).unwrap();
    let doc = clean_fixture(&source);
    write(&source.join("tools/new_helper.py"), "# unexpected helper\n");
    let report = executable_ownership::check(&source, &doc).unwrap();
    assert!(kinds(&report).contains(&"unclassified-executable".to_string()));
}

#[test]
fn embedded_or_generated_foreign_program_fails() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("tree");
    fs::create_dir_all(&source).unwrap();
    let doc = clean_fixture(&source);
    let python_marker = doc.embedded_program_markers[0].needle.clone();
    write(
        &source.join("crates/demo/src/emitter.rs"),
        &format!("pub const PROGRAM: &str = \"{} python\";\n", python_marker),
    );
    let report = executable_ownership::check(&source, &doc).unwrap();
    let embedded: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| finding.kind == "embedded-foreign-program")
        .collect();
    assert_eq!(embedded.len(), 1);
    assert_eq!(
        embedded[0].path.as_deref(),
        Some("crates/demo/src/emitter.rs")
    );

    let root = TempDir::new().unwrap();
    let source = root.path().join("tree");
    fs::create_dir_all(&source).unwrap();
    let doc = clean_fixture(&source);
    let powershell_marker = doc.embedded_program_markers[1].needle.clone();
    write(
        &source.join("tools/rtk-adapter/src/generated.rs"),
        &format!("const SCRIPT: &str = \"{} 7.4\";\n", powershell_marker),
    );
    let report = executable_ownership::check(&source, &doc).unwrap();
    assert!(kinds(&report).contains(&"embedded-foreign-program".to_string()));
}

#[test]
fn owned_package_relabeling_fails() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("tree");
    fs::create_dir_all(&source).unwrap();
    let mut doc = clean_fixture(&source);
    write(&source.join("tools/legacy.py"), "# owned helper\n");
    doc.entries.push(third_party(
        &["tools/legacy.py"],
        "claimed external package",
    ));
    let report = executable_ownership::check(&source, &doc).unwrap();
    assert!(kinds(&report).contains(&"relabeling".to_string()));

    let root = TempDir::new().unwrap();
    let source = root.path().join("tree");
    fs::create_dir_all(&source).unwrap();
    let mut doc = clean_fixture(&source);
    write(
        &source.join("tests/fixtures/lsp/owned.py"),
        "# owned helper\n",
    );
    doc.entries.push(inert(
        &["tests/fixtures/lsp/owned.py"],
        "crates/demo/src/missing.rs",
    ));
    let report = executable_ownership::check(&source, &doc).unwrap();
    assert!(kinds(&report).contains(&"relabeling".to_string()));

    let root = TempDir::new().unwrap();
    let source = root.path().join("tree");
    fs::create_dir_all(&source).unwrap();
    let mut doc = clean_fixture(&source);
    write(
        &source.join("tests/fixtures/lsp/unreferenced.py"),
        "# owned helper\n",
    );
    doc.entries.push(inert(
        &["tests/fixtures/lsp/unreferenced.py"],
        "crates/demo/src/consumer.rs",
    ));
    let report = executable_ownership::check(&source, &doc).unwrap();
    assert!(kinds(&report).contains(&"relabeling".to_string()));

    let root = TempDir::new().unwrap();
    let source = root.path().join("tree");
    fs::create_dir_all(&source).unwrap();
    let mut doc = clean_fixture(&source);
    write(&source.join("tools/disguised.py"), "# owned helper\n");
    doc.entries.push(inert(
        &["tools/disguised.py"],
        "crates/demo/src/consumer.rs",
    ));
    let report = executable_ownership::check(&source, &doc).unwrap();
    assert!(kinds(&report).contains(&"relabeling".to_string()));
}

#[test]
fn inventory_entries_stay_valid_and_actionable() {
    let root = TempDir::new().unwrap();
    let source = root.path().join("tree");
    fs::create_dir_all(&source).unwrap();
    let mut doc = clean_fixture(&source);
    doc.entries.push(legacy(&["tools/missing.py"]));
    write(&source.join("tools/live.py"), "# legacy\n");
    doc.entries.push(legacy(&["tools/live.py"]));
    let mut unknown = legacy(&["tools/present.py"]);
    write(&source.join("tools/present.py"), "# legacy\n");
    unknown.classification = "friendly".into();
    doc.entries.push(unknown);
    let mut unowned = legacy(&["tools/unowned.py"]);
    write(&source.join("tools/unowned.py"), "# legacy\n");
    unowned.owner_task = None;
    doc.entries.push(unowned);
    let report = executable_ownership::check(&source, &doc).unwrap();
    let found = kinds(&report);
    assert!(found.contains(&"stale-entry".to_string()));
    assert!(found.contains(&"invalid-entry".to_string()));
    assert!(found.contains(&"legacy-executable".to_string()));
}

#[test]
fn current_checkout_inventory_is_complete() {
    let source = checkout();
    let bytes = fs::read(source.join("docs/evidence/executable-ownership.json")).unwrap();
    let doc: OwnershipDocument = serde_json::from_slice(&bytes).unwrap();
    let report = executable_ownership::check(&source, &doc).unwrap();
    let unexpected: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| finding.kind != "legacy-executable")
        .collect();
    assert!(
        unexpected.is_empty(),
        "unexpected ownership findings: {unexpected:?}"
    );
    let legacy = report
        .findings
        .iter()
        .filter(|finding| finding.kind == "legacy-executable")
        .count();
    assert!(legacy > 0);
    assert_eq!(
        report.classified_counts.get("inert-data"),
        Some(&2),
        "declared analysis samples must stay classified"
    );
    assert_eq!(report.executable_files, legacy + 2);
}

#[test]
fn cli_reports_findings_and_clean_trees() {
    let clean = TempDir::new().unwrap();
    let source = clean.path().join("tree");
    fs::create_dir_all(&source).unwrap();
    let doc = clean_fixture(&source);
    let document_path = source.join("docs/evidence/executable-ownership.json");
    fs::create_dir_all(document_path.parent().unwrap()).unwrap();
    fs::write(&document_path, serde_json::to_vec_pretty(&doc).unwrap()).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .arg("ownership-check")
        .arg("--source")
        .arg(&source)
        .status()
        .unwrap();
    assert!(status.success());

    write(&source.join("tools/unclassified.py"), "# unexpected\n");
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .arg("ownership-check")
        .arg("--source")
        .arg(&source)
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let kinds: Vec<&str> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"unclassified-executable"));
}
