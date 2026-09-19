use harness_core::inventory;
use std::fs;

fn write(path: &std::path::Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

#[test]
fn kit_json_inventory_lists_evolution_and_analysis_without_copying_bodies() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let home = root.path().join("codex");
    let user = root.path().join("user");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&user).unwrap();
    write(
        &source.join("global/kit.json"),
        br#"{"schema":1,"profile_name":"harness","profile":"global/harness.config.toml","instructions":"global/principles-of-work.md","skills":".agents/skills","agents":"global/agents","hooks":"global/hooks.json","token_hooks":"global/rtk-hooks.json"}"#,
    );
    write(
        &source.join("global/harness.config.toml"),
        b"model = 'gpt-6-astra'\n",
    );
    write(
        &source.join("global/principles-of-work.md"),
        b"# principles\n",
    );
    write(&source.join("global/hooks.json"), b"{\"hooks\":{}}");
    write(&source.join("global/rtk-hooks.json"), b"{\"hooks\":{}}");
    fs::create_dir_all(source.join("global/agents")).unwrap();
    write(
        &source.join(".agents/skills/skill-evolution/SKILL.md"),
        b"---\nname: skill-evolution\ndescription: Evolution workflow.\n---\n",
    );
    write(
        &source.join(".agents/skills/skills-usage-analysis/SKILL.md"),
        b"---\nname: skills-usage-analysis\ndescription: Usage report.\n---\n",
    );
    let inventory = inventory::read(&source, &home, &user).unwrap();
    let skills: Vec<_> = inventory
        .links
        .iter()
        .filter(|link| link.kind == "skill")
        .map(|link| link.name.as_str())
        .collect();
    assert!(skills.contains(&"skill-evolution"));
    assert!(skills.contains(&"skills-usage-analysis"));
    for link in inventory.links.iter().filter(|link| link.kind == "skill") {
        assert!(link.source.is_dir());
        assert!(!link.destination.starts_with(&source));
    }
}
