//! Installed OpenSpec contract checks, with all configuration writes isolated.
use harness_core::improvement_spec::{ExperimentContract, OpenSpec, Specification};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

fn contract() -> ExperimentContract {
    ExperimentContract {
        acceptance_artifact: "design.md".into(),
        mechanism: "Avoid repeated parsing of identical source input".into(),
        counterexample: "The source changes on every invocation".into(),
        applicability: "Repeated reads at the same source revision".into(),
        independent_acceptance: "An unchanged external checker rejects stale output".into(),
        meaningful_effect: "At least 10 percent lower accepted-task elapsed time".into(),
        operating_conditions: "Both arms use the same warm-cache policy".into(),
        comparison_policy: "Matched pairs with required correctness and no resource regression"
            .into(),
        stopping_rule: "Run the predeclared pairs; do not stop on a favorable result".into(),
    }
}

#[test]
fn absent_acceptance_or_escaping_artifact_is_not_a_planning_contract() {
    let mut value = contract();
    value.independent_acceptance.clear();
    assert!(
        value
            .validate()
            .unwrap_err()
            .to_string()
            .contains("independent_acceptance")
    );
    let mut value = contract();
    value.acceptance_artifact = "../outside.md".into();
    assert!(value.validate().is_err());
    assert!(contract().validate().is_ok());
}

fn native(arguments: &[&str], cwd: &Path, environment: &BTreeMap<String, String>) -> Vec<u8> {
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("pwsh");
        command.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-CommandWithArgs",
            "& openspec @args; exit $LASTEXITCODE",
        ]);
        command
    };
    #[cfg(not(windows))]
    let mut command = Command::new("openspec");
    let out = command
        .args(arguments)
        .current_dir(cwd)
        .envs(environment)
        .env("OPENSPEC_TELEMETRY", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn fill_change(root: &Path) {
    fs::create_dir_all(root.join("specs/repeated-reads")).unwrap();
    fs::write(root.join("proposal.md"), "## Why\nIdentical reads waste work.\n\n## What Changes\nReuse identical input parsing.\n\n## Capabilities\n\n### New Capabilities\n- `repeated-reads`: Reuse parsing with correct invalidation.\n\n### Modified Capabilities\nNone.\n\n## Impact\nReader and its checks.\n").unwrap();
    fs::write(root.join("design.md"), "## Context\nRepeated source reads.\n\n## Decisions\nKey reuse on content identity.\n\n## Experiment acceptance\nAn unchanged external checker rejects stale output. The pair freezes source, runtime and cache policy; changed source is the counterexample. Require at least 10 percent lower elapsed time with correctness and no resource regression; run the predeclared matched pairs, never stop on a favorable result.\n").unwrap();
    fs::write(root.join("tasks.md"), "## Implementation\n- [ ] Implement correct read reuse.\n- [ ] Verify invalidation and complete the declared comparison.\n").unwrap();
    fs::write(root.join("specs/repeated-reads/spec.md"), "## ADDED Requirements\n\n### Requirement: Reuse preserves output\nThe reader SHALL preserve correct output when reusing identical inputs.\n\n#### Scenario: Source changes\n- **WHEN** the input content changes\n- **THEN** the next read returns the changed content\n").unwrap();
}

fn installed_case(store: bool) {
    let temporary = tempfile::Builder::new()
        .prefix("improvement-spec-")
        .tempdir()
        .unwrap();
    let source = temporary.path().join("source with spaces");
    let configuration = temporary.path().join("configuration");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&configuration).unwrap();
    let environment = BTreeMap::from([
        ("APPDATA".to_owned(), configuration.display().to_string()),
        (
            "XDG_CONFIG_HOME".to_owned(),
            configuration.display().to_string(),
        ),
    ]);
    // Prove isolation before any CLI operation that changes a store registry.
    let resolved = native(&["config", "path"], &source, &environment);
    let resolved = String::from_utf8(resolved).unwrap();
    assert!(
        Path::new(resolved.trim()).starts_with(&configuration),
        "configuration was not isolated: {resolved}"
    );
    let planning_root = if store {
        temporary.path().join("planning")
    } else {
        source.clone()
    };
    if store {
        native(
            &[
                "store",
                "setup",
                "owned-improvement",
                "--path",
                planning_root.to_str().unwrap(),
                "--no-init-git",
                "--json",
            ],
            &source,
            &environment,
        );
    } else {
        fs::create_dir_all(source.join("openspec/changes")).unwrap();
        fs::write(source.join("openspec/config.yaml"), "schema: spec-driven\n").unwrap();
    }
    let target = Specification {
        project: source,
        change: "improve-repeated-reads".into(),
        store: store.then(|| "owned-improvement".into()),
        planning_root: planning_root.clone(),
    };
    let api = OpenSpec { environment };
    api.scaffold(&target).unwrap();
    api.instructions(&target, "proposal").unwrap();
    assert!(
        api.qualify(&target, &contract()).is_err(),
        "empty scaffold must never permit implementation"
    );
    let root = planning_root.join("openspec/changes/improve-repeated-reads");
    fill_change(&root);
    let qualified = api.qualify(&target, &contract()).unwrap();
    assert_eq!(qualified.implementation_state, "ready");
    assert_eq!(qualified.artifacts.len(), 4);
    api.revalidate(&qualified).unwrap();
    for file in [
        "proposal.md",
        "design.md",
        "tasks.md",
        "specs/repeated-reads/spec.md",
    ] {
        let path = root.join(file);
        let bytes = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert!(
            api.qualify(&target, &contract()).is_err(),
            "missing {file} allowed implementation"
        );
        fs::write(path, bytes).unwrap();
    }
    let mut absent = contract();
    absent.independent_acceptance.clear();
    assert!(api.qualify(&target, &absent).is_err());
    fs::write(
        root.join("design.md"),
        "## Revised acceptance\nThe workload now has different requirements.\n",
    )
    .unwrap();
    assert!(
        api.revalidate(&qualified)
            .unwrap_err()
            .to_string()
            .contains("changed")
    );
    let mut wrong = target;
    wrong.planning_root = temporary.path().to_path_buf();
    assert!(api.qualify(&wrong, &contract()).is_err());
}

#[test]
#[ignore = "requires installed OpenSpec and owner PowerShell; model-free isolated CLI acceptance"]
fn installed_repository_gate_rejects_missing_and_changed_planning_inputs() {
    installed_case(false);
}

#[test]
#[ignore = "requires installed OpenSpec; creates a store only in an isolated configuration home"]
fn installed_registered_store_is_resolved_and_checked_without_retargeting_source() {
    installed_case(true);
}
