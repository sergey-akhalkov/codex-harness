//! Fixture consumer repos outside this checkout. Not months of production use.
use serde_json::json;
use skill_evolution::{
    routing::{self, Knowledge, Observation},
    staging,
};
use std::{fs, process::Command};

fn git(repo: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success());
}

fn repo(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let repo = root.join(name);
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "fixture@example.test"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    fs::write(
        repo.join("README.md"),
        format!("# {name} fixture consumer\n"),
    )
    .unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    repo
}

#[test]
fn two_independent_fixture_repos_stage_outside_discovery() {
    let root = tempfile::tempdir().unwrap();
    let one = repo(root.path(), "consumer-one");
    let two = repo(root.path(), "consumer-two");
    assert_ne!(one, two);
    let observation = Observation {
        reusable_procedure: true,
        checkable_result: true,
        project_fact_only: false,
        tentative: false,
        existing_tool: false,
        read_only: false,
        routine_success: false,
    };
    assert_eq!(routing::classify(&observation), Knowledge::Procedure);
    let src = root.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("SKILL.md"),
        "---\nname: demo\ndescription: Fixture procedure.\n---\n",
    )
    .unwrap();
    for repo in [&one, &two] {
        let staged = staging::stage(
            &src,
            &repo.join("docs/memory/skill-evolution/demo"),
            &[repo.join(".agents/skills")],
            repo.file_name().unwrap().to_string_lossy().as_ref(),
            "none",
            &json!({"fixture": true}),
        )
        .unwrap();
        assert!(
            !staged
                .identity
                .root
                .starts_with(repo.join(".agents/skills"))
        );
        assert!(
            !Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(repo)
                .output()
                .unwrap()
                .stdout
                .is_empty()
        );
    }
}
