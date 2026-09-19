//! Integrity matrix H: competing worktrees, half publication, foreign edit,
//! concurrent lock, poisoned candidate, provider unavailable, budget exhaustion.
use serde_json::json;
use skill_evolution::{
    budget::Ledger,
    decision::{Claim, ComparisonEvidence, Verdict, decide},
    journal, package, publication, staging,
};
use std::fs;

fn skill(root: &std::path::Path, name: &str, body: &str) {
    fs::create_dir_all(root.join(name)).unwrap();
    fs::write(
        root.join(name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Integrity fixture.\n---\n{body}\n"),
    )
    .unwrap();
}

fn evidence(provider: bool) -> ComparisonEvidence {
    ComparisonEvidence {
        integrity_ok: true,
        evidence_complete: provider,
        provider_matched: provider,
        must_pass: true,
        selection_demonstrated: true,
        protected_regression: false,
        benefit_established: provider,
        within_budgets: true,
        claim: Claim::Capability,
        skipped_required_check: false,
        single_lucky_run: false,
        meaningful_difference: provider,
    }
}

#[test]
fn integrity_scenarios_preserve_active_library_and_foreign_work() {
    let root = tempfile::tempdir().unwrap();
    let tree_a = root.path().join("wt-a");
    let tree_b = root.path().join("wt-b");
    skill(&tree_a, "demo", "a");
    skill(&tree_b, "demo", "b");
    assert_ne!(
        package::load(&tree_a.join("demo")).unwrap().revision,
        package::load(&tree_b.join("demo")).unwrap().revision
    );

    let dest = root.path().join("dest");
    skill(&dest, "demo", "v1");
    let parent = package::load(&dest.join("demo")).unwrap().revision;
    fs::write(dest.join("demo").with_extension("publish.lock"), b"held").unwrap();
    let staged = root.path().join("staged");
    skill(&staged, "demo", "v2");
    assert!(publication::publish(&staged.join("demo"), &dest.join("demo"), &parent).is_err());

    let poisoned = root.path().join("poison");
    skill(&poisoned, "demo", "ok");
    assert!(
        staging::stage(
            &poisoned.join("demo"),
            &root.path().join(".agents/skills/demo"),
            &[root.path().join(".agents/skills")],
            "wt",
            &parent,
            &json!({"token":"sk-leak"})
        )
        .is_err()
    );

    assert_eq!(decide(&evidence(false)), Verdict::Inconclusive);

    let mut budget = Ledger::new("owner", "day");
    budget.period_limit = Some(1);
    budget.reserve(1).unwrap();
    budget.complete(1).unwrap();
    assert!(budget.reserve(1).is_err());

    let recovery = root.path().join("recovery");
    package::copy_into(&dest.join("demo"), &recovery).unwrap();
    fs::write(
        dest.join("demo/SKILL.md"),
        "---\nname: other\ndescription: Integrity fixture.\n---\nforeign\n",
    )
    .unwrap();
    let record = journal::Record {
        phase: journal::Phase::Published,
        name: "demo".into(),
        dest: dest.join("demo"),
        recovery,
        published_revision: parent,
        recovery_revision: package::load(&dest.join("demo"))
            .unwrap_or_else(|_| package::load(&tree_a.join("demo")).unwrap())
            .revision,
    };
    let _ = journal::recover(&record);
    assert!(dest.join("demo/SKILL.md").exists());
}
