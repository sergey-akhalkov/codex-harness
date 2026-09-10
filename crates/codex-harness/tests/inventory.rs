//! Native declarative inventory through the actual manager CLI; no mutations
//! outside these owned fixtures and no model calls.
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    home: PathBuf,
    user: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let home = root.path().join("codex-home");
        let user = root.path().join("user-home");
        for path in ["global/agents/nested", "skills/one"] {
            fs::create_dir_all(source.join(path)).unwrap();
        }
        for path in [
            "global/profile.toml",
            "global/instructions.md",
            "global/hooks.json",
            "global/token-hooks.json",
        ] {
            fs::write(source.join(path), "inert test data").unwrap();
        }
        fs::write(
            source.join("skills/one/SKILL.md"),
            "---\nname: one\nmetadata:\n  name: nested-name\n---\n# Body\nname: ignored-body\n",
        )
        .unwrap();
        fs::write(
            source.join("global/agents/nested/middle.toml"),
            "name = 'middle'\nmodel = 'xai/grok-4.6'\n",
        )
        .unwrap();
        fs::write(source.join("global/kit.json"),serde_json::to_vec(&json!({"schema":1,"profile_name":"harness","profile":"global/profile.toml","instructions":"global/instructions.md","skills":"skills","agents":"global/agents","hooks":"global/hooks.json","token_hooks":"global/token-hooks.json"})).unwrap()).unwrap();
        Self {
            root,
            source,
            home,
            user,
        }
    }
    fn run(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["inventory", "--source"])
            .arg(&self.source)
            .arg("--codex-home")
            .arg(&self.home)
            .arg("--user-home")
            .arg(&self.user)
            .current_dir(self.root.path())
            .output()
            .unwrap()
    }
    fn success(&self) -> Value {
        let out = self.run();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}

#[test]
fn fresh_inventory_reads_current_descriptors_without_installing_anything() {
    let f = Fixture::new();
    let report = f.success();
    assert_eq!(report["links"].as_array().unwrap().len(), 3);
    assert!(
        report["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|link| link["kind"] != "profile")
    );
    assert_eq!(report["agents"][0]["name"], "middle");
    assert!(
        report["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|link| link["connection"] == "missing")
    );
    assert!(!f.home.exists());
    assert!(!f.user.exists());
    fs::write(
        f.source.join("skills/one/SKILL.md"),
        "---\nname: changed\n---\n",
    )
    .unwrap();
    let changed = f.success();
    assert!(
        changed["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| link["name"] == "changed")
    );
    assert!(!f.home.exists());
    assert!(!f.user.exists());
}

#[test]
fn duplicate_invalid_or_unbounded_descriptors_are_private_failures() {
    for data in [
        "---\nname: '\n---\n".to_owned(),
        "---\nmetadata:\n  name: wrong\n---\n".into(),
        "---\nname: one\nname: two\n---\n".into(),
        "private-marker".repeat(22000),
    ] {
        let f = Fixture::new();
        fs::write(f.source.join("skills/one/SKILL.md"), data).unwrap();
        let out = f.run();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(!stderr.contains("panicked") && !stderr.contains("private-marker"));
        assert!(!f.home.exists());
    }
    let f = Fixture::new();
    fs::write(
        f.source.join("global/agents/duplicate.toml"),
        "name = 'middle'\n",
    )
    .unwrap();
    assert_eq!(f.run().status.code(), Some(2));
}

#[test]
fn traversal_or_unknown_manifest_fields_do_not_read_or_write_foreign_targets() {
    for (field, value) in [
        ("profile", json!("../private.txt")),
        ("new_executable", json!("private-marker")),
        ("schema", json!(9)),
    ] {
        let f = Fixture::new();
        fs::write(f.root.path().join("private.txt"), "private-marker").unwrap();
        let path = f.source.join("global/kit.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest[field] = value;
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let out = f.run();
        assert_eq!(out.status.code(), Some(2));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("private-marker"));
        assert_eq!(
            fs::read_to_string(f.root.path().join("private.txt")).unwrap(),
            "private-marker"
        );
        assert!(!f.home.exists());
    }
}

#[cfg(windows)]
#[test]
fn direct_links_are_distinct_from_foreign_files_and_dangling_links() {
    use std::os::windows::fs::symlink_file;
    let f = Fixture::new();
    fs::create_dir(&f.home).unwrap();
    symlink_file(
        f.source.join("global/instructions.md"),
        f.home.join("AGENTS.md"),
    )
    .unwrap();
    fs::write(f.home.join("harness.config.toml"), "foreign configuration").unwrap();
    let report = f.success();
    let links = report["links"].as_array().unwrap();
    assert_eq!(
        links.iter().find(|v| v["kind"] == "instructions").unwrap()["connection"],
        "linked"
    );
    assert!(links.iter().all(|link| link["kind"] != "profile"));
    assert_eq!(
        fs::read_to_string(f.home.join("harness.config.toml")).unwrap(),
        "foreign configuration"
    );
    fs::remove_file(f.home.join("AGENTS.md")).unwrap();
    fs::write(f.home.join("AGENTS.md"), "foreign instructions").unwrap();
    let foreign = f.success();
    assert_eq!(
        foreign["links"]
            .as_array()
            .unwrap()
            .iter()
            .find(|link| link["kind"] == "instructions")
            .unwrap()["connection"],
        "conflict"
    );
    assert_eq!(
        fs::read_to_string(f.home.join("AGENTS.md")).unwrap(),
        "foreign instructions"
    );
    fs::remove_file(f.home.join("AGENTS.md")).unwrap();
    symlink_file(f.root.path().join("absent.md"), f.home.join("AGENTS.md")).unwrap();
    let report = f.success();
    assert_eq!(
        report["links"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["kind"] == "instructions")
            .unwrap()["connection"],
        "conflict"
    );
}

#[cfg(windows)]
#[test]
fn source_and_destination_reparse_parents_are_rejected() {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    let f = Fixture::new();
    fs::create_dir(&f.user).unwrap();
    let foreign = f.root.path().join("foreign");
    fs::create_dir(&foreign).unwrap();
    fs::write(foreign.join("keep"), "foreign contents").unwrap();
    symlink_dir(&foreign, f.user.join(".agents")).unwrap();
    assert_eq!(f.run().status.code(), Some(2));
    fs::remove_dir(f.user.join(".agents")).unwrap();
    fs::remove_file(f.source.join("global/instructions.md")).unwrap();
    symlink_file(
        foreign.join("keep"),
        f.source.join("global/instructions.md"),
    )
    .unwrap();
    assert_eq!(f.run().status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(foreign.join("keep")).unwrap(),
        "foreign contents"
    );
}

#[test]
fn foreign_global_descriptors_collide_while_reuse_and_unrelated_names_pass() {
    let f = Fixture::new();
    fs::create_dir_all(f.user.join(".agents/skills/other")).unwrap();
    fs::write(
        f.user.join(".agents/skills/other/SKILL.md"),
        "---\nname: one\n---\nprivate-marker\n",
    )
    .unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(!stderr.contains("private-marker"));
    assert_eq!(
        fs::read_to_string(f.user.join(".agents/skills/other/SKILL.md")).unwrap(),
        "---\nname: one\n---\nprivate-marker\n"
    );

    let f = Fixture::new();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    fs::write(
        f.home.join("agents/foreign.toml"),
        "name = 'middle'\nsecret = 'private-marker'\n",
    )
    .unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("private-marker"));
    assert_eq!(
        fs::read_to_string(f.home.join("agents/foreign.toml")).unwrap(),
        "name = 'middle'\nsecret = 'private-marker'\n"
    );

    let f = Fixture::new();
    fs::create_dir_all(f.home.join("agents/vendor/nested")).unwrap();
    fs::write(
        f.home.join("agents/vendor/nested/copy.toml"),
        "name = 'middle'\n",
    )
    .unwrap();
    assert_eq!(f.run().status.code(), Some(2));

    let f = Fixture::new();
    fs::create_dir_all(f.user.join(".agents/skills/other")).unwrap();
    fs::create_dir_all(f.home.join("agents/vendor/nested")).unwrap();
    fs::write(
        f.user.join(".agents/skills/other/SKILL.md"),
        "---\nname: extra\n---\n",
    )
    .unwrap();
    fs::write(
        f.home.join("agents/vendor/nested/extra.toml"),
        "name = 'extra'\n",
    )
    .unwrap();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    fs::write(f.home.join("agents/other.toml"), "name = 'other'\n").unwrap();
    let report = f.success();
    assert_eq!(report["agents"][0]["name"], "middle");
    assert_eq!(
        fs::read_to_string(f.user.join(".agents/skills/other/SKILL.md")).unwrap(),
        "---\nname: extra\n---\n"
    );
}

#[cfg(windows)]
#[test]
fn matching_kit_skill_link_is_not_a_foreign_collision() {
    use std::os::windows::fs::symlink_dir;
    let f = Fixture::new();
    fs::create_dir_all(f.user.join(".agents/skills")).unwrap();
    symlink_dir(
        f.source.join("skills/one"),
        f.user.join(".agents/skills/one"),
    )
    .unwrap();
    let report = f.success();
    assert_eq!(
        report["links"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["kind"] == "skill")
            .unwrap()["connection"],
        "linked"
    );
}

#[cfg(windows)]
#[test]
fn matching_kit_agent_directory_link_is_not_a_foreign_collision() {
    use std::os::windows::fs::symlink_dir;
    let f = Fixture::new();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    symlink_dir(
        f.source.join("global/agents"),
        f.home.join("agents/codex-harness"),
    )
    .unwrap();
    let report = f.success();
    assert_eq!(
        report["links"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["kind"] == "agents")
            .unwrap()["connection"],
        "linked"
    );
    assert_eq!(report["agents"][0]["name"], "middle");
}

#[cfg(windows)]
#[test]
fn wrong_target_managed_agent_directory_is_a_conflict_not_a_scan_target() {
    use std::os::windows::fs::symlink_dir;
    let f = Fixture::new();
    let foreign = f.root.path().join("foreign-agents");
    fs::create_dir_all(foreign.join("nested")).unwrap();
    fs::write(
        foreign.join("nested/copy.toml"),
        "name = 'middle'\nsecret = 'private-marker'\n",
    )
    .unwrap();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    symlink_dir(&foreign, f.home.join("agents/codex-harness")).unwrap();
    let report = f.success();
    assert_eq!(
        report["links"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["kind"] == "agents")
            .unwrap()["connection"],
        "conflict"
    );
    assert_eq!(
        fs::read_to_string(foreign.join("nested/copy.toml")).unwrap(),
        "name = 'middle'\nsecret = 'private-marker'\n"
    );
}

#[test]
fn malformed_oversized_or_cyclic_global_descriptors_are_not_clean() {
    let f = Fixture::new();
    fs::create_dir_all(f.user.join(".agents/skills/other")).unwrap();
    fs::write(
        f.user.join(".agents/skills/other/SKILL.md"),
        "---\nname: one\nname: two\n---\nprivate-marker\n",
    )
    .unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("private-marker"));
    assert_eq!(
        fs::read_to_string(f.user.join(".agents/skills/other/SKILL.md")).unwrap(),
        "---\nname: one\nname: two\n---\nprivate-marker\n"
    );

    let f = Fixture::new();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    fs::write(
        f.home.join("agents/huge.toml"),
        format!(
            "name = 'middle'\nnote = '{}'\n",
            "private-marker".repeat(22000)
        ),
    )
    .unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("private-marker"));
    assert!(
        fs::read_to_string(f.home.join("agents/huge.toml"))
            .unwrap()
            .contains("private-marker")
    );

    let f = Fixture::new();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    fs::write(f.home.join("agents/broken.toml"), "name =\n").unwrap();
    assert_eq!(f.run().status.code(), Some(2));
}

#[cfg(windows)]
#[test]
fn cyclic_foreign_agent_trees_do_not_report_clean() {
    use std::os::windows::fs::symlink_dir;
    let f = Fixture::new();
    let loop_root = f.home.join("agents/loop");
    fs::create_dir_all(&loop_root).unwrap();
    symlink_dir(&loop_root, loop_root.join("back")).unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(2));
    assert!(loop_root.join("back").exists());
}

#[cfg(windows)]
#[test]
fn linked_external_unique_skill_and_agent_pass() {
    use std::os::windows::fs::symlink_dir;
    let f = Fixture::new();
    let skills = f.root.path().join("external-skills/extra");
    fs::create_dir_all(&skills).unwrap();
    fs::write(
        skills.join("SKILL.md"),
        "---\nname: extra\n---\nprivate-marker\n",
    )
    .unwrap();
    fs::create_dir_all(f.user.join(".agents/skills")).unwrap();
    symlink_dir(&skills, f.user.join(".agents/skills/extra")).unwrap();

    let agents = f.root.path().join("external-agents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(agents.join("extra.toml"), "name = 'extra'\n").unwrap();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    symlink_dir(&agents, f.home.join("agents/vendor")).unwrap();

    let report = f.success();
    assert_eq!(report["agents"][0]["name"], "middle");
    assert_eq!(
        fs::read_to_string(skills.join("SKILL.md")).unwrap(),
        "---\nname: extra\n---\nprivate-marker\n"
    );
    assert_eq!(
        fs::read_to_string(agents.join("extra.toml")).unwrap(),
        "name = 'extra'\n"
    );
}

#[cfg(windows)]
#[test]
fn linked_same_name_collision_is_rejected_and_bytes_are_preserved() {
    use std::os::windows::fs::symlink_dir;
    let f = Fixture::new();
    let skills = f.root.path().join("external-skills/copy");
    fs::create_dir_all(&skills).unwrap();
    fs::write(
        skills.join("SKILL.md"),
        "---\nname: one\n---\nprivate-marker\n",
    )
    .unwrap();
    fs::create_dir_all(f.user.join(".agents/skills")).unwrap();
    symlink_dir(&skills, f.user.join(".agents/skills/copy")).unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("private-marker"));
    assert_eq!(
        fs::read_to_string(skills.join("SKILL.md")).unwrap(),
        "---\nname: one\n---\nprivate-marker\n"
    );

    let f = Fixture::new();
    let agents = f.root.path().join("external-agents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(
        agents.join("copy.toml"),
        "name = 'middle'\nsecret = 'private-marker'\n",
    )
    .unwrap();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    symlink_dir(&agents, f.home.join("agents/vendor")).unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("private-marker"));
    assert_eq!(
        fs::read_to_string(agents.join("copy.toml")).unwrap(),
        "name = 'middle'\nsecret = 'private-marker'\n"
    );
}

#[cfg(windows)]
#[test]
fn linked_descriptor_file_is_discovered() {
    use std::os::windows::fs::symlink_file;
    let f = Fixture::new();
    let source = f.root.path().join("external.toml");
    fs::write(&source, "name = 'extra'\n").unwrap();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    symlink_file(&source, f.home.join("agents/extra.toml")).unwrap();
    let report = f.success();
    assert_eq!(report["agents"][0]["name"], "middle");
    assert_eq!(fs::read_to_string(&source).unwrap(), "name = 'extra'\n");
}

#[cfg(windows)]
#[test]
fn dangling_or_malformed_foreign_links_are_private_failures() {
    use std::os::windows::fs::{symlink_dir, symlink_file};
    let f = Fixture::new();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    symlink_dir(
        f.root.path().join("absent-agents"),
        f.home.join("agents/missing"),
    )
    .unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));

    let f = Fixture::new();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    symlink_file(
        f.root.path().join("absent.toml"),
        f.home.join("agents/broken.toml"),
    )
    .unwrap();
    let out = f.run();
    assert_eq!(out.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
}

#[test]
fn agent_descriptors_use_complete_native_toml_with_private_failures() {
    let f = Fixture::new();
    fs::create_dir_all(f.home.join("agents")).unwrap();
    let descriptor = f.home.join("agents/extra.toml");
    for contents in [
        "'name' = 'extra'\n",
        "name = \"extra\" # comment\n",
        "name = \"ex\\u0074ra\"\n",
        "name = '''extra'''\n",
    ] {
        fs::write(&descriptor, contents).unwrap();
        f.success();
        assert_eq!(fs::read_to_string(&descriptor).unwrap(), contents);
    }
    for contents in [
        "name = 'extra'\nsecret = 'private-marker",
        "name = 'extra'\nname = 'middle'\n",
        "'name' = 'middle'\n",
    ] {
        fs::write(&descriptor, contents).unwrap();
        let output = f.run();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private-marker"));
        assert_eq!(fs::read_to_string(&descriptor).unwrap(), contents);
    }
}
