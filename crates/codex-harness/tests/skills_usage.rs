use skill_evolution::{ledger, usage};
use std::{fs, process::Command};

fn write_skill(root: &std::path::Path, name: &str) {
    fs::create_dir_all(root.join(name)).unwrap();
    fs::write(
        root.join(name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Usage CLI fixture.\n---\n"),
    )
    .unwrap();
}

#[test]
fn usage_lists_project_before_global_and_does_not_mutate() {
    let root = tempfile::Builder::new()
        .prefix("skills-usage-")
        .tempdir()
        .unwrap();
    let project = root.path().join("repo");
    let user = root.path().join("user");
    write_skill(&project.join(".agents/skills"), "alpha");
    write_skill(&user.join(".agents/skills"), "beta");
    let before = fs::read(project.join(".agents/skills/alpha/SKILL.md")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "usage", "--user-home"])
        .arg(&user)
        .arg("--codex-home")
        .arg(root.path().join("codex"))
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let project_at = text.find("project").unwrap();
    let global_at = text.find("global").unwrap();
    assert!(project_at < global_at);
    assert!(text.contains("alpha"));
    assert!(text.contains("beta"));
    assert!(text.contains("unknown"));
    assert!(text.contains("candidates"));
    assert!(text.contains("no library changes applied"));
    assert_eq!(
        fs::read(project.join(".agents/skills/alpha/SKILL.md")).unwrap(),
        before
    );
}

#[test]
fn usage_outside_a_project_has_no_project_section() {
    let root = tempfile::Builder::new()
        .prefix("skills-usage-none-")
        .tempdir()
        .unwrap();
    let user = root.path().join("user");
    write_skill(&user.join(".agents/skills"), "beta");
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "usage", "--user-home"])
        .arg(&user)
        .arg("--codex-home")
        .arg(root.path().join("codex"))
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("no project skill section applies"));
    assert!(text.contains("beta"));
}

#[test]
fn matrix_i_unknown_not_observed_candidates_and_no_apply() {
    let root = tempfile::Builder::new()
        .prefix("skills-usage-matrix-")
        .tempdir()
        .unwrap();
    let project = root.path().join("repo");
    let user = root.path().join("user");
    write_skill(&project.join(".agents/skills"), "alpha");
    write_skill(&project.join(".agents/skills"), "project-verification");
    write_skill(&user.join(".agents/skills"), "beta");
    let ledger_path = root
        .path()
        .join("codex/harness/skill-invocation-ledger.jsonl");
    fs::create_dir_all(ledger_path.parent().unwrap()).unwrap();
    let report = usage::report(&project, &user, &ledger_path, Some((1.0, 2.0)), None).unwrap();
    assert!(matches!(
        report
            .project
            .iter()
            .find(|r| r.name == "alpha")
            .unwrap()
            .last_invocation,
        ledger::LastInvocation::NotObserved { .. }
    ));
    let candidates = usage::candidates(&report);
    assert!(candidates.iter().any(|c| c.name == "alpha"));
    assert!(candidates.iter().all(|c| c.name != "project-verification"));
    assert!(!usage::analysis_request("implement the parser"));
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "usage", "--user-home"])
        .arg(&user)
        .arg("--codex-home")
        .arg(root.path().join("codex"))
        .current_dir(&project)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("no library changes applied"));
    assert!(project.join(".agents/skills/alpha/SKILL.md").is_file());
}

#[test]
fn identity_reads_the_live_descriptor_without_mutating_it() {
    let root = tempfile::Builder::new()
        .prefix("skills-identity-")
        .tempdir()
        .unwrap();
    let skill = root.path().join("demo");
    write_skill(root.path(), "demo");
    let before = fs::read(skill.join("SKILL.md")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&skill)
        .arg("--operation")
        .arg("update")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["name"], "demo");
    assert_eq!(value["operation"], "update");
    assert!(value["revision"].as_str().unwrap().len() > 16);
    assert_eq!(fs::read(skill.join("SKILL.md")).unwrap(), before);
}

#[test]
fn identity_observes_the_live_descriptor_and_never_refunds_tokens() {
    let root = tempfile::Builder::new()
        .prefix("skills-identity-observe-")
        .tempdir()
        .unwrap();
    write_skill(root.path(), "demo");
    let skill = root.path().join("demo");
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&skill)
        .arg("--operation")
        .arg("update")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["observed"], true);
    assert_eq!(value["stale"], false);
    assert_eq!(value["tokens_refunded"], false);
    assert_eq!(value["delivery_complete"], true);
    let retired = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&skill)
        .arg("--operation")
        .arg("retire")
        .output()
        .unwrap();
    let retired: serde_json::Value = serde_json::from_slice(&retired.stdout).unwrap();
    assert_eq!(retired["stale"], true);
    assert_eq!(retired["delivery_complete"], false);
    assert_eq!(retired["tokens_refunded"], false);
}

#[test]
fn publish_is_not_delivery_until_identity_observes_the_live_revision() {
    let root = tempfile::Builder::new()
        .prefix("skills-publish-observe-")
        .tempdir()
        .unwrap();
    write_skill(root.path(), "demo");
    let staged = root.path().join("demo");
    fs::write(
        staged.join("SKILL.md"),
        "---\nname: demo\ndescription: Usage CLI fixture.\n---\nv2\n",
    )
    .unwrap();
    let dest = root.path().join("live");
    let journal = root.path().join("awareness.json");
    let request = root.path().join("publish.json");
    fs::write(
        &request,
        serde_json::to_vec(&serde_json::json!({
            "staged": staged,
            "dest": dest,
            "expected_parent": "",
            "journal": journal
        }))
        .unwrap(),
    )
    .unwrap();
    let published = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "publish", "--request"])
        .arg(&request)
        .output()
        .unwrap();
    assert!(
        published.status.success(),
        "{}",
        String::from_utf8_lossy(&published.stderr)
    );
    let published: serde_json::Value = serde_json::from_slice(&published.stdout).unwrap();
    assert_eq!(published["observed"], false);
    assert_eq!(published["delivery_complete"], false);
    assert_eq!(published["tokens_refunded"], false);
    let observed = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&dest)
        .arg("--operation")
        .arg("update")
        .arg("--journal")
        .arg(&journal)
        .output()
        .unwrap();
    assert!(
        observed.status.success(),
        "{}",
        String::from_utf8_lossy(&observed.stderr)
    );
    let observed: serde_json::Value = serde_json::from_slice(&observed.stdout).unwrap();
    assert_eq!(observed["revision"], published["revision"]);
    assert_eq!(observed["observed"], true);
    assert_eq!(observed["delivery_complete"], true);
    assert_eq!(observed["tokens_refunded"], false);
    fs::write(
        dest.join("SKILL.md"),
        "---\nname: demo\ndescription: Usage CLI fixture.\n---\nv3-rollback\n",
    )
    .unwrap();
    let rolled = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&dest)
        .arg("--operation")
        .arg("rollback")
        .arg("--journal")
        .arg(&journal)
        .output()
        .unwrap();
    let rolled: serde_json::Value = serde_json::from_slice(&rolled.stdout).unwrap();
    assert_ne!(rolled["revision"], published["revision"]);
    assert_eq!(rolled["operation"], "rollback");
    assert_eq!(rolled["delivery_complete"], true);
    assert_eq!(rolled["tokens_refunded"], false);
}

#[test]
fn identity_after_removed_package_is_incomplete_and_does_not_refund_tokens() {
    let root = tempfile::Builder::new()
        .prefix("skills-identity-removed-")
        .tempdir()
        .unwrap();
    write_skill(root.path(), "demo");
    let skill = root.path().join("demo");
    let journal = root.path().join("awareness.json");
    let first = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&skill)
        .arg("--operation")
        .arg("update")
        .arg("--journal")
        .arg(&journal)
        .output()
        .unwrap();
    assert!(first.status.success());
    let first: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["delivery_complete"], true);
    fs::remove_dir_all(&skill).unwrap();
    let missing = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&skill)
        .arg("--operation")
        .arg("retire")
        .arg("--journal")
        .arg(&journal)
        .output()
        .unwrap();
    assert_ne!(missing.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&missing.stdout);
    assert!(!stdout.contains("\"tokens_refunded\": true"), "{stdout}");
    let journaled: serde_json::Value =
        serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
    assert_eq!(journaled["tokens_refunded"], false);
    assert_eq!(journaled["observed"], true);
}

#[test]
fn identity_honors_skills_config_disablement() {
    let root = tempfile::Builder::new()
        .prefix("skills-identity-disabled-")
        .tempdir()
        .unwrap();
    write_skill(root.path(), "demo");
    let skill = root.path().join("demo");
    let home = root.path().join("codex");
    fs::create_dir_all(&home).unwrap();
    let skill_md = skill.join("SKILL.md");
    fs::write(
        home.join("config.toml"),
        format!(
            "[[skills.config]]\npath = {}\nenabled = false\n",
            serde_json::to_string(&skill_md.to_string_lossy().as_ref()).unwrap()
        ),
    )
    .unwrap();
    let enabled = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&skill)
        .output()
        .unwrap();
    assert!(enabled.status.success());
    let enabled: serde_json::Value = serde_json::from_slice(&enabled.stdout).unwrap();
    assert_eq!(enabled["delivery_complete"], true);
    assert_eq!(enabled["enabled"], true);
    let disabled = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "identity", "--path"])
        .arg(&skill)
        .arg("--codex-home")
        .arg(&home)
        .output()
        .unwrap();
    assert!(
        disabled.status.success(),
        "{}",
        String::from_utf8_lossy(&disabled.stderr)
    );
    let disabled: serde_json::Value = serde_json::from_slice(&disabled.stdout).unwrap();
    assert_eq!(disabled["enabled"], false);
    assert_eq!(disabled["delivery_complete"], false);
    assert_eq!(disabled["tokens_refunded"], false);
    assert_eq!(disabled["stale"], true);
}

#[test]
fn usage_lists_disabled_from_codex_config() {
    let root = tempfile::Builder::new()
        .prefix("skills-usage-disabled-")
        .tempdir()
        .unwrap();
    let project = root.path().join("repo");
    let user = root.path().join("user");
    let home = root.path().join("codex");
    write_skill(&user.join(".agents/skills"), "beta");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&project).unwrap();
    let skill = user.join(".agents/skills/beta");
    fs::write(
        home.join("config.toml"),
        format!(
            "[[skills.config]]\npath = {}\nenabled = false\n",
            serde_json::to_string(&skill.join("SKILL.md").to_string_lossy().as_ref()).unwrap()
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["skills", "usage", "--user-home"])
        .arg(&user)
        .arg("--codex-home")
        .arg(&home)
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("beta"));
    assert!(text.contains("disabled"));
    assert!(
        text.lines()
            .any(|line| line.contains("beta") && line.contains("disabled"))
    );
}
