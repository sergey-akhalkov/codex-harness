//! Installed OpenSpec contract checks, with all configuration writes isolated.
use harness_core::improvement_spec::{
    ExperimentContract, MeasurementScope, MeasurementWorkload, OpenSpec, REMOVAL_PROPOSAL_CLAUSES,
    REMOVAL_PROPOSAL_HEADING, Specification,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn contract() -> ExperimentContract {
    ExperimentContract {
        acceptance_artifact: "design.md".into(),
        acceptance_heading: "## Experiment acceptance".into(),
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

fn measurement_scope() -> MeasurementScope {
    MeasurementScope {
        observed_problem: "Identical repeated reads waste accepted-task time".into(),
        investigation_scope: "The reader's repeated reads at one frozen source revision".into(),
        measurement_question: "How much accepted-task time do identical repeated reads cost?"
            .into(),
        workload: MeasurementWorkload {
            operation: "cargo build -p example-reader".into(),
            contract: "proposal.md#measurement".into(),
        },
        evidence_references: vec!["retained outcome record: repeated reads".into()],
        limits: "One local machine and one frozen source revision".into(),
        declaration_artifact: "proposal.md".into(),
        declaration_heading: "## Measurement".into(),
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

#[test]
fn incomplete_measurement_scope_is_not_a_measurement_plan() {
    assert!(measurement_scope().validate().is_ok());
    let mut value = measurement_scope();
    value.measurement_question.clear();
    assert!(
        value
            .validate()
            .unwrap_err()
            .to_string()
            .contains("measurement_question")
    );
    let mut value = measurement_scope();
    value.workload.operation.clear();
    assert!(
        value
            .validate()
            .unwrap_err()
            .to_string()
            .contains("workload.operation")
    );
    let mut value = measurement_scope();
    value.evidence_references.clear();
    assert!(
        value
            .validate()
            .unwrap_err()
            .to_string()
            .contains("evidence_references")
    );
    let mut value = measurement_scope();
    value.limits = "  ".into();
    assert!(value.validate().is_err());
    let mut value = measurement_scope();
    value.declaration_heading = "Measurement".into();
    assert!(
        value
            .validate()
            .unwrap_err()
            .to_string()
            .contains("declaration_heading")
    );
    let mut value = measurement_scope();
    value.declaration_artifact = "../outside.md".into();
    assert!(value.validate().is_err());
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
    fs::write(root.join("proposal.md"), "## Why\nIdentical reads waste work.\n\n## Measurement\nObserved problem: identical repeated reads waste accepted-task time. Investigation scope: reads at one frozen source revision. Measurement question: how much accepted-task time do they cost? Evidence: retained outcome record. Limits: one local machine. Workload: an existing build operation.\n\n## What Changes\nReuse identical input parsing.\n\n## Capabilities\n\n### New Capabilities\n- `repeated-reads`: Reuse parsing with correct invalidation.\n\n### Modified Capabilities\nNone.\n\n## Impact\nReader and its checks.\n").unwrap();
    fs::write(root.join("design.md"), "## Context\nRepeated source reads.\n\n## Decisions\nKey reuse on content identity.\n\n## Experiment acceptance\nAn unchanged external checker rejects stale output. The pair freezes source, runtime and cache policy; changed source is the counterexample. Require at least 10 percent lower elapsed time with correctness and no resource regression; run the predeclared matched pairs, never stop on a favorable result.\n").unwrap();
    fs::write(root.join("tasks.md"), "## Implementation\n- [ ] Implement correct read reuse.\n- [ ] Verify invalidation and complete the declared comparison.\n").unwrap();
    fs::write(root.join("specs/repeated-reads/spec.md"), "## ADDED Requirements\n\n### Requirement: Reuse preserves output\nThe reader SHALL preserve correct output when reusing identical inputs.\n\n#### Scenario: Source changes\n- **WHEN** the input content changes\n- **THEN** the next read returns the changed content\n").unwrap();
}

/// The reviewable removal proposal a hypothesis's own change states: target and
/// source references, the unapplied preview, evidence and its gaps, measured
/// versus predicted benefit, lost scenarios, consumer/configuration/
/// installation impact, alternatives, retained checks and restoration.
fn removal_section() -> String {
    [
        REMOVAL_PROPOSAL_HEADING,
        "",
        "- Target: capability-x",
        "- Source: openspec/changes/improve-repeated-reads",
        "- Evidence: file:observations/unused-capability.txt",
        "- Gaps: no invocation telemetry covers the recovery path; indirect consumers are unverified",
        "- Measured: not yet measured; the retained observation records no consumption in either arm",
        "- Predicted: lower catalogue and instruction exposure on every accepted task",
        "- Loss: rare-manual-recovery",
        "- Lost scenarios: a manual recovery in a degraded environment loses its documented route",
        "- Impact: the installed skill catalogue, the owned configuration and the current installation",
        "- Alternatives: keep the capability, narrow its exposure or consolidate it into an existing route",
        "- Retained checks: the independent oracle and the installation ownership check stay binding",
        "- Restoration: restore the capability from the pinned revision and re-run the installation lifecycle",
        "- Preview: preview:retained/unapplied-capability-x.diff",
        "",
    ]
    .join("\n")
}

/// One installed OpenSpec fixture: an isolated configuration home, the
/// selected repository or registered store, and the target change scaffolded
/// through the installed CLI. The temporary directory must stay alive for the
/// fixture to resolve.
struct Installed {
    temporary: tempfile::TempDir,
    target: Specification,
    api: OpenSpec,
}

fn installed(store: bool) -> Installed {
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
        (
            "XDG_DATA_HOME".to_owned(),
            configuration.display().to_string(),
        ),
        (
            "LOCALAPPDATA".to_owned(),
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
        assert!(
            configuration
                .join("openspec/stores/registry.yaml")
                .is_file(),
            "store registry was not written to the isolated data home"
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
    let mut wrong_creation = target.clone();
    wrong_creation.change = "must-not-create".into();
    wrong_creation.planning_root = temporary.path().to_owned();
    assert!(api.scaffold(&wrong_creation).is_err());
    assert!(
        !planning_root
            .join("openspec/changes/must-not-create")
            .exists()
    );
    api.scaffold(&target).unwrap();
    Installed {
        temporary,
        target,
        api,
    }
}

fn installed_case(store: bool) {
    let Installed {
        temporary,
        target,
        api,
    } = installed(store);
    let planning_root = target.planning_root.clone();
    api.instructions(&target, "proposal").unwrap();
    assert!(
        api.qualify(&target, &contract()).is_err(),
        "empty scaffold must never permit implementation"
    );
    let root = planning_root.join("openspec/changes/improve-repeated-reads");
    // An empty scaffold states no measurement scope: directed measurement is
    // blocked until the hypothesis's own change declares one.
    assert!(
        api.begin_measurement(&target, &measurement_scope())
            .is_err(),
        "an empty scaffold permitted a directed measurement"
    );
    // The initial change states the observed problem, investigation scope,
    // measurement question, evidence, limits and targeted tasks; it does not
    // invent a solution yet.
    fs::write(
        root.join("proposal.md"),
        "## Why\nIdentical repeated reads waste accepted-task time.\n\n## Measurement\nObserved problem: identical repeated reads waste time. Investigation scope: reads at one frozen source revision. Measurement question: how much accepted-task time do they cost? Evidence: retained outcome record. Limits: one local machine. Workload: an existing build operation.\n",
    )
    .unwrap();
    fs::write(
        root.join("tasks.md"),
        "## Measurement\n- [ ] Run the targeted baseline measurement.\n",
    )
    .unwrap();
    let measured = api
        .begin_measurement(&target, &measurement_scope())
        .unwrap();
    assert_eq!(measured.change_root, root.canonicalize().unwrap());
    assert_eq!(measured.artifacts.len(), 2);
    assert_eq!(
        measured.scope.workload.operation,
        "cargo build -p example-reader"
    );
    // A missing measurement question blocks the directed measurement even
    // though the change exists.
    let mut incomplete = measurement_scope();
    incomplete.measurement_question.clear();
    assert!(api.begin_measurement(&target, &incomplete).is_err());
    // A one-line candidate instruction cannot skip requirements, design or
    // experiment acceptance: implementation stays undispatched.
    assert!(
        api.qualify(&target, &contract()).is_err(),
        "a one-line candidate skipped the candidate-implementation prerequisites"
    );
    // Removing the tasks artifact blocks the directed measurement.
    let tasks = fs::read(root.join("tasks.md")).unwrap();
    fs::remove_file(root.join("tasks.md")).unwrap();
    assert!(
        api.begin_measurement(&target, &measurement_scope())
            .is_err()
    );
    fs::write(root.join("tasks.md"), tasks).unwrap();
    // A change without the declared measurement scope blocks it too.
    let proposal = fs::read_to_string(root.join("proposal.md")).unwrap();
    fs::write(
        root.join("proposal.md"),
        "## Why\nNo targeted measurement is specified.\n",
    )
    .unwrap();
    assert!(
        api.begin_measurement(&target, &measurement_scope())
            .unwrap_err()
            .to_string()
            .contains("measurement scope")
    );
    fs::write(root.join("proposal.md"), proposal).unwrap();
    // The workload is the existing build operation linked from this change:
    // no second hypothesis or change exists for it.
    let changes = fs::read_dir(planning_root.join("openspec/changes"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir() && entry.file_name() != "archive")
        .count();
    assert_eq!(
        changes, 1,
        "the measurement workload was forced into a separate change"
    );
    // The same change must own the directed measurement and the later
    // candidate-implementation prerequisite.
    fill_change(&root);
    let qualified = api.qualify(&target, &contract()).unwrap();
    assert_eq!(qualified.change_root, measured.change_root);
    api.revalidate_measurement(&measured).unwrap();
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
    let design = fs::read_to_string(root.join("design.md")).unwrap();
    fs::write(
        root.join("design.md"),
        "## Design\nNo experiment acceptance is specified.\n",
    )
    .unwrap();
    assert!(
        api.qualify(&target, &contract())
            .unwrap_err()
            .to_string()
            .contains("acceptance section")
    );
    fs::write(
        root.join("design.md"),
        format!("{design}\nThe workload now has different requirements.\n"),
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
    assert!(api.begin_measurement(&wrong, &measurement_scope()).is_err());
}

/// Relative paths and bytes of every file under a directory, for proving an
/// operation wrote nothing or moved content intact. A missing directory is an
/// empty tree.
fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, directory: &Path, files: &mut Vec<(String, Vec<u8>)>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, files);
            } else if path.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push((relative, fs::read(&path).unwrap()));
            }
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort();
    files
}

/// Completion reconciliation against the installed CLI: retention reads the
/// change's actual task state without writing, an unfinished required task
/// cannot be closed through an experiment outcome, and a completed rejected
/// change is archived without its unadopted delta reaching the main
/// specifications.
fn completion_case(store: bool) {
    let Installed {
        temporary: _temporary,
        target,
        api,
    } = installed(store);
    let root = target
        .planning_root
        .join("openspec/changes/improve-repeated-reads");
    fill_change(&root);
    let qualified = api.qualify(&target, &contract()).unwrap();
    assert_eq!(qualified.implementation_state, "ready");
    let main_specs = target.planning_root.join("openspec/specs");
    let main_specs_before = tree(&main_specs);

    // Retention reads the change's actual task state and writes nothing; the
    // change reference and every artifact stay accessible.
    let change_before = tree(&root);
    let completion = api.completion(&target).unwrap();
    assert_eq!(completion.change_root, root.canonicalize().unwrap());
    assert_eq!(completion.state, "ready");
    assert_eq!(
        (completion.total, completion.complete, completion.remaining),
        (2, 0, 2)
    );
    assert!(!completion.is_complete());
    assert_eq!(completion.artifacts.len(), 4);
    assert_eq!(
        tree(&root),
        change_before,
        "reading completion must not write"
    );
    assert_eq!(
        tree(&main_specs),
        main_specs_before,
        "reading completion must not touch the main specifications"
    );

    // An unfinished required task cannot be closed through an experiment
    // outcome: the supported archive refuses and the change is retained.
    let refused = api.archive_unadopted(&target, &completion).unwrap_err();
    assert!(
        refused.to_string().contains("unfinished required task"),
        "{refused}"
    );
    assert_eq!(tree(&root), change_before, "a refusal must not write");
    assert_eq!(tree(&main_specs), main_specs_before);

    // The change's own artifact is the only thing that makes it completable;
    // the recorded decision never substitutes for it.
    fs::write(
        root.join("tasks.md"),
        "## Implementation\n- [x] Implement correct read reuse.\n- [x] Verify invalidation and complete the declared comparison.\n",
    )
    .unwrap();
    let completed = api.completion(&target).unwrap();
    assert!(completed.is_complete());
    assert_eq!(completed.state, "all_done");
    assert_eq!(completed.remaining, 0);
    assert!(completed.unfinished_tasks.is_empty());

    // A stale reconciliation cannot authorize archiving changed artifacts.
    let stale = api.archive_unadopted(&target, &completion).unwrap_err();
    assert!(stale.to_string().contains("changed since"), "{stale}");
    assert!(root.exists(), "a stale refusal must not write");

    // Supported archival of the unadopted change: the artifacts move with the
    // change and the main specifications keep their exact content.
    let originals: Vec<(PathBuf, Vec<u8>)> = completed
        .artifacts
        .keys()
        .map(|path| {
            (
                path.strip_prefix(&completed.change_root)
                    .unwrap()
                    .to_path_buf(),
                fs::read(path).unwrap(),
            )
        })
        .collect();
    let archived = api.archive_unadopted(&target, &completed).unwrap();
    assert!(archived.archived_as.ends_with("-improve-repeated-reads"));
    assert!(archived.archive_root.is_dir());
    for (relative, bytes) in &originals {
        assert_eq!(
            fs::read(archived.archive_root.join(relative)).unwrap(),
            *bytes,
            "{}",
            relative.display()
        );
    }
    assert!(!root.exists(), "the archived change moved out of changes/");
    assert_eq!(
        tree(&main_specs),
        main_specs_before,
        "the unadopted delta must not synchronize into the main specifications"
    );
    assert_eq!(archived.main_specs_files, main_specs_before.len());
    // Reconciliation repeats fail against the moved change instead of
    // re-archiving it.
    assert!(api.completion(&target).is_err());
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

#[test]
#[ignore = "requires installed OpenSpec and owner PowerShell; model-free isolated CLI acceptance"]
fn installed_completion_reconciliation_retains_and_archives_without_spec_sync() {
    completion_case(false);
}

#[test]
#[ignore = "requires installed OpenSpec; creates a store only in an isolated configuration home"]
fn installed_completion_reconciliation_resolves_a_registered_store() {
    completion_case(true);
}

/// The reviewable removal proposal is resolved from the hypothesis's own
/// change: every clause is carried with its value, the section digest binds
/// the reviewed content, and a drifted artifact is refused instead of read.
fn removal_proposal_case() {
    let Installed {
        temporary: _temporary,
        target,
        api,
    } = installed(false);
    let root = target
        .planning_root
        .join("openspec/changes/improve-repeated-reads");
    fill_change(&root);
    let qualified = api.qualify(&target, &contract()).unwrap();
    // A qualified change that states no section states no reviewable
    // proposal: the refusal names the exact heading.
    let absent = api.removal_proposal(&qualified).unwrap_err();
    assert!(
        absent.to_string().contains(REMOVAL_PROPOSAL_HEADING),
        "{absent}"
    );

    let design = fs::read_to_string(root.join("design.md")).unwrap();
    fs::write(
        root.join("design.md"),
        format!("{design}\n{}", removal_section()),
    )
    .unwrap();
    let qualified = api.qualify(&target, &contract()).unwrap();
    let receipt = api.removal_proposal(&qualified).unwrap();
    assert_eq!(receipt.change_root, root.canonicalize().unwrap());
    assert_eq!(receipt.heading, REMOVAL_PROPOSAL_HEADING);
    assert!(
        receipt.artifact.ends_with("design.md"),
        "{:?}",
        receipt.artifact
    );
    assert_eq!(receipt.target, "capability-x");
    assert_eq!(receipt.source, "openspec/changes/improve-repeated-reads");
    assert_eq!(receipt.evidence, "file:observations/unused-capability.txt");
    assert!(receipt.gaps.contains("unverified"), "{}", receipt.gaps);
    assert!(
        receipt.measured.contains("not yet measured"),
        "{}",
        receipt.measured
    );
    assert!(
        receipt.predicted.contains("lower catalogue"),
        "{}",
        receipt.predicted
    );
    assert_eq!(receipt.loss, "rare-manual-recovery");
    assert!(
        receipt.lost_scenarios.contains("manual recovery"),
        "{}",
        receipt.lost_scenarios
    );
    assert!(
        receipt.impact.contains("installation"),
        "{}",
        receipt.impact
    );
    assert!(
        receipt.alternatives.contains("keep the capability"),
        "{}",
        receipt.alternatives
    );
    assert!(
        receipt.retained_checks.contains("oracle"),
        "{}",
        receipt.retained_checks
    );
    assert!(
        receipt.restoration.contains("pinned revision"),
        "{}",
        receipt.restoration
    );
    assert_eq!(
        receipt.preview,
        "preview:retained/unapplied-capability-x.diff"
    );
    for label in REMOVAL_PROPOSAL_CLAUSES {
        assert!(
            receipt.clause(label).is_some_and(|value| !value.is_empty()),
            "{label}"
        );
    }

    // The digest binds the exact reviewed section: changing one prose clause
    // changes it, so an approval recorded for the old content cannot silently
    // cover the changed proposal. A drifted artifact is refused first.
    let edited = fs::read_to_string(root.join("design.md")).unwrap().replace(
        "indirect consumers are unverified",
        "indirect consumers were later verified absent",
    );
    fs::write(root.join("design.md"), edited).unwrap();
    let drifted = api.removal_proposal(&qualified).unwrap_err();
    assert!(
        drifted.to_string().contains("changed after qualification"),
        "{drifted}"
    );
    let requalified = api.qualify(&target, &contract()).unwrap();
    let updated = api.removal_proposal(&requalified).unwrap();
    assert_ne!(updated.section_digest, receipt.section_digest);
    assert_eq!(updated.target, receipt.target);
    assert!(updated.gaps.contains("verified absent"), "{}", updated.gaps);
}

#[test]
#[ignore = "requires installed OpenSpec and owner PowerShell; model-free isolated CLI acceptance"]
fn installed_removal_proposal_requires_every_clause_and_binds_its_section() {
    removal_proposal_case();
}

/// An incomplete or ambiguous removal proposal is refused by clause before
/// anything can be presented for a decision.
#[test]
#[ignore = "requires installed OpenSpec and owner PowerShell; model-free isolated CLI acceptance"]
fn installed_incomplete_or_ambiguous_removal_proposals_are_refused_by_clause() {
    let Installed {
        temporary: _temporary,
        target,
        api,
    } = installed(false);
    let root = target
        .planning_root
        .join("openspec/changes/improve-repeated-reads");
    fill_change(&root);
    let base = fs::read_to_string(root.join("design.md")).unwrap();
    let state = |section: &str| {
        fs::write(root.join("design.md"), format!("{base}\n{section}")).unwrap();
        api.qualify(&target, &contract()).unwrap()
    };
    let refusal = |section: &str| {
        let qualified = state(section);
        api.removal_proposal(&qualified).unwrap_err().to_string()
    };

    let missing = removal_section()
        .lines()
        .filter(|line| !line.starts_with("- Restoration:"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        refusal(&missing).contains("missing the clause Restoration:"),
        "{}",
        refusal(&missing)
    );

    let duplicated = format!("{missing}\n- Target: capability-y\n");
    assert!(
        refusal(&duplicated).contains("Target: is stated more than once"),
        "{}",
        refusal(&duplicated)
    );

    let empty = removal_section().replace(
        "- Gaps: no invocation telemetry covers the recovery path; indirect consumers are unverified",
        "- Gaps:",
    );
    assert!(
        refusal(&empty).contains("Gaps: is empty"),
        "{}",
        refusal(&empty)
    );

    // The section is stated exactly once: a second resolved artifact stating
    // it is ambiguous and refused.
    state(&removal_section());
    let proposal = fs::read_to_string(root.join("proposal.md")).unwrap();
    fs::write(
        root.join("proposal.md"),
        format!("{proposal}\n{}", removal_section()),
    )
    .unwrap();
    let qualified = api.qualify(&target, &contract()).unwrap();
    let ambiguous = api.removal_proposal(&qualified).unwrap_err().to_string();
    assert!(
        ambiguous.contains("more than one resolved artifact"),
        "{ambiguous}"
    );
}
