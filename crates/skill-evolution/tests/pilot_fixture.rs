use skill_evolution::package;
use std::path::PathBuf;

fn workspace_skill() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.agents/skills/project-verification")
}

fn shortened() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/project-verification-short")
}

#[test]
fn shortening_keeps_name_and_description_and_drops_reference_files() {
    let baseline = package::load(&workspace_skill()).unwrap();
    let candidate = package::load(&shortened()).unwrap();
    assert_eq!(baseline.name, "project-verification");
    assert_eq!(candidate.name, baseline.name);
    assert_eq!(candidate.description, baseline.description);
    assert!(baseline.files.contains_key("references/command-records.md"));
    assert!(baseline.files.contains_key("references/feedback.md"));
    assert_eq!(candidate.files.len(), 1);
    assert!(candidate.files.contains_key("SKILL.md"));
    assert_ne!(candidate.revision, baseline.revision);
}
