use std::path::PathBuf;

#[test]
fn kit_source_contains_evolution_and_analysis_skills() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let kit: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("global/kit.json")).unwrap()).unwrap();
    assert_eq!(kit["skills"], ".agents/skills");
    for name in ["skill-evolution", "skills-usage-analysis"] {
        assert!(
            root.join(".agents/skills")
                .join(name)
                .join("SKILL.md")
                .is_file()
        );
    }
}
