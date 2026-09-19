//! Isolated clone keeps project skills and Git memory. Not a global install.
use std::{fs, process::Command};

fn git(dir: &std::path::Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn fresh_clone_keeps_project_skill_and_memory_without_global_install() {
    let root = tempfile::tempdir().unwrap();
    let origin = root.path().join("origin");
    fs::create_dir_all(origin.join(".agents/skills/demo")).unwrap();
    fs::create_dir_all(origin.join("docs/memory")).unwrap();
    fs::write(
        origin.join(".agents/skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Clone fixture.\n---\n",
    )
    .unwrap();
    fs::write(origin.join("docs/memory/README.md"), "# memory\n").unwrap();
    git(&origin, &["init", "-q"]);
    git(&origin, &["config", "user.email", "fixture@example.test"]);
    git(&origin, &["config", "user.name", "Fixture"]);
    git(&origin, &["add", "."]);
    git(&origin, &["commit", "-q", "-m", "origin"]);
    let clone = root.path().join("clone");
    assert!(
        Command::new("git")
            .args([
                "clone",
                "-q",
                origin.to_str().unwrap(),
                clone.to_str().unwrap()
            ])
            .status()
            .unwrap()
            .success()
    );
    let skill = skill_evolution::package::load(&clone.join(".agents/skills/demo")).unwrap();
    assert_eq!(skill.name, "demo");
    assert!(clone.join("docs/memory/README.md").is_file());
    assert!(!clone.join(".agents/skills/demo").is_symlink());
}
