use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        let result = Self(tempfile::tempdir().unwrap());
        fs::create_dir(result.repo()).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(result.repo())
                .status()
                .unwrap()
                .success()
        );
        result
    }
    fn repo(&self) -> std::path::PathBuf {
        self.0.path().join("repo")
    }
    fn write(&self, name: &str, body: &str) {
        let path = self.repo().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    fn check(&self, terms: Option<&Path>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_harness-source-check"));
        command
            .arg("--root")
            .arg(self.repo())
            .current_dir(self.0.path());
        if let Some(terms) = terms {
            command.arg("--private-terms").arg(terms);
        }
        command.output().unwrap()
    }
    fn account(&self, invoked: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_harness-source-check"));
        command
            .arg("--root")
            .arg(self.repo())
            .arg("--accounting")
            .current_dir(self.0.path());
        for spec in invoked {
            command.arg("--invoked").arg(spec);
        }
        command.output().unwrap()
    }
}

#[test]
fn checks_new_files_links_and_anchors_without_following_sample_code() {
    let f = Fixture::new();
    f.write(
        "README.md",
        "[Guide](docs/guide.md#useful-api)\n```text\n[Example](not-a-real-file.md)\n```\n",
    );
    f.write("docs/guide.md", "# Useful API\n\n[Home](../README.md)\n");
    let good = f.check(None);
    assert!(
        good.status.success(),
        "{}",
        String::from_utf8_lossy(&good.stdout)
    );
    f.write("docs/guide.md", "# Renamed\n[Missing](absent.md)\n");
    let bad = f.check(None);
    let output = String::from_utf8_lossy(&bad.stdout);
    assert_eq!(bad.status.code(), Some(1));
    assert!(output.contains("missing-local-anchor"));
    assert!(output.contains("missing-local-link"));
}

#[test]
fn detects_private_content_and_paths_without_echoing_audit_inputs() {
    let f = Fixture::new();
    let terms = f.0.path().join("local-terms.txt");
    fs::write(&terms, "synthetic-confidential\n").unwrap();
    f.write(
        "synthetic-confidential/notes.md",
        "synthetic-confidential payload\n",
    );
    let bad = f.check(Some(&terms));
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&bad.stdout),
        String::from_utf8_lossy(&bad.stderr)
    );
    assert_eq!(bad.status.code(), Some(1));
    assert!(output.contains("private-term") && output.contains("private-path"));
    assert!(!output.contains("synthetic-confidential"));
    f.write("terms.txt", "synthetic-confidential\n");
    let refused = f.check(Some(&f.repo().join("terms.txt")));
    assert_eq!(refused.status.code(), Some(2));
    let nested = Command::new(env!("CARGO_BIN_EXE_harness-source-check"))
        .arg("--root")
        .arg(f.repo().join("synthetic-confidential"))
        .arg("--private-terms")
        .arg(f.repo().join("terms.txt"))
        .output()
        .unwrap();
    assert_eq!(nested.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&nested.stderr).contains("synthetic-confidential"));
}

#[test]
fn detects_shared_state_and_tracked_caches_but_allows_deleted_caches() {
    let f = Fixture::new();
    f.write(".gitignore", "__pycache__/\n");
    f.write("__pycache__/sample.pyc", "cached fixture");
    assert!(f.check(None).status.success());
    assert!(
        Command::new("git")
            .args(["add", "-f", "__pycache__/sample.pyc"])
            .current_dir(f.repo())
            .status()
            .unwrap()
            .success()
    );
    let bad = f.check(None);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stdout).contains("tracked-runtime-artifact"));
    fs::remove_file(f.repo().join("__pycache__/sample.pyc")).unwrap();
    assert!(f.check(None).status.success());
    f.write(
        "global/harness.config.toml",
        "[projects.'C:/workspace/sample']\ntrust_level = 'trusted'\n",
    );
    f.write("notes.md", "Path C:/Users/synthetic-owner/work\n");
    let bad = f.check(None);
    assert_eq!(bad.status.code(), Some(1));
    let output = String::from_utf8_lossy(&bad.stdout);
    assert!(output.contains("shared-project-trust"));
    assert!(output.contains("machine-home-path"));
}

#[test]
fn principles_limit_checks_bytes_at_the_exact_boundary() {
    let f = Fixture::new();
    let limit = 24_576;
    // Multi-byte text makes a character-count implementation insufficient.
    f.write("global/principles-of-work.md", &"é".repeat(limit / 2));
    assert!(f.check(None).status.success());
    let oversized = format!("{}x", "é".repeat(limit / 2));
    f.write("global/principles-of-work.md", &oversized);
    let bad = f.check(None);
    assert_eq!(bad.status.code(), Some(1));
    let output = String::from_utf8_lossy(&bad.stdout);
    assert!(output.contains("principles-size-limit actual=24577 allowed=24576"));
    assert_eq!(
        fs::read_to_string(f.repo().join("global/principles-of-work.md")).unwrap(),
        oversized
    );
}

fn kit_fixture() -> Fixture {
    let f = Fixture::new();
    f.write(
        "global/kit.json",
        r#"{
        "schema":1,"profile_name":"harness","profile":"global/harness.config.toml",
        "instructions":"global/principles-of-work.md","skills":"fixture-skills",
        "agents":"global/agents","hooks":"global/hooks.json","token_hooks":"global/rtk-hooks.json"
    }"#,
    );
    f.write("global/principles-of-work.md", "# Working principles\n");
    f.write(
        "global/harness.config.toml",
        "developer_instructions = 'Shared context'\n",
    );
    f.write(
        "global/roles.toml",
        r#"schema = 1
shared_skills = ["fixture-skill"]
[roles.lead]
instructions = "global/roles/lead.md"
skills = ["lead-only"]
[roles.executor]
instructions = "global/roles/executor.md"
skills = []
[roles.delegated-lead]
instructions = "global/roles/delegated-lead.md"
skills = []
"#,
    );
    f.write("global/roles/lead.md", "# Lead\n");
    f.write("global/roles/delegated-lead.md", "# Delegated lead\n");
    f.write("global/roles/executor.md", "# Executor\n");
    f.write(
        "fixture-skills/fixture-skill/SKILL.md",
        "---\nname: fixture-skill\ndescription: Owned fixture skill\n---\nFixture.\n",
    );
    f.write(
        "fixture-skills/lead-only/SKILL.md",
        "---\nname: lead-only\ndescription: Lead fixture skill\n---\nLead.\n",
    );
    for owner in harness_core::report_owners::TOKEN_AUDIT_OWNERS {
        f.write(owner, "# Owner\n");
    }
    f
}

#[test]
fn kit_requires_complete_role_inputs_and_classified_skills() {
    let f = kit_fixture();
    assert!(f.check(None).status.success());
    fs::remove_file(f.repo().join("global/roles/executor.md")).unwrap();
    let missing = f.check(None);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stdout).contains("role-context-invalid"));
    f.write("global/roles/executor.md", "# Executor\n");
    assert!(f.check(None).status.success());
    f.write(
        "fixture-skills/unclassified/SKILL.md",
        "---\nname: unclassified\ndescription: Unclassified fixture skill\n---\nFixture.\n",
    );
    let unclassified = f.check(None);
    assert_eq!(unclassified.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unclassified.stdout).contains("role-context-invalid"));
}

#[test]
fn kit_requires_principles_and_existing_report_owners() {
    let f = kit_fixture();
    assert!(f.check(None).status.success());
    let owner = harness_core::report_owners::TOKEN_AUDIT_OWNERS[0];
    fs::remove_file(f.repo().join(owner)).unwrap();
    let missing = f.check(None);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stdout).contains("missing-report-owner"));
    f.write(owner, "# Restored\n");
    fs::remove_file(f.repo().join("global/principles-of-work.md")).unwrap();
    let missing = f.check(None);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stdout).contains("missing-principles"));
}

#[test]
fn kit_refuses_an_owner_link_outside_the_checkout() {
    let f = kit_fixture();
    let owner = f
        .repo()
        .join(harness_core::report_owners::TOKEN_AUDIT_OWNERS[0]);
    let outside = f.0.path().join("outside.md");
    fs::write(&outside, "# Outside\n").unwrap();
    fs::remove_file(&owner).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&outside, &owner).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &owner).unwrap();
    let bad = f.check(None);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stdout).contains("invalid-report-owner"));
    assert_eq!(fs::read_to_string(outside).unwrap(), "# Outside\n");
}

fn accounting_field(output: &str, role: &str, surface: &str, key: &str) -> usize {
    let prefix = format!("guidance-accounting role={role} surface={surface} ");
    let line = output
        .lines()
        .find(|line| line.starts_with(prefix.as_str()))
        .unwrap_or_else(|| panic!("no {surface} accounting line for {role}"));
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(format!("{key}=").as_str()))
        .unwrap_or_else(|| panic!("no {key} field"))
        .parse()
        .unwrap()
}

#[test]
fn accounting_reports_the_fixed_load_per_role_once() {
    let f = kit_fixture();
    let measured = f.account(&[]);
    assert_eq!(
        measured.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&measured.stderr)
    );
    let output = String::from_utf8_lossy(&measured.stdout);
    assert!(
        output.contains(concat!(
            "guidance-accounting role=lead surface=fixed visible_skills=2 metadata_bytes=63 metadata_words=8 ",
            "common_bytes=21 common_words=3 developer_bytes=14 developer_words=2 role_bytes=7 role_words=2 ",
            "fixed_bytes=105 fixed_words=15"
        )),
        "{output}"
    );
    assert!(
        output.contains(concat!(
            "guidance-accounting role=executor surface=fixed visible_skills=1 metadata_bytes=34 metadata_words=4 ",
            "common_bytes=21 common_words=3 developer_bytes=14 developer_words=2 role_bytes=11 role_words=2 ",
            "fixed_bytes=80 fixed_words=11"
        )),
        "{output}"
    );
}

#[test]
fn accounting_attributes_growth_to_the_owning_role_and_surface() {
    let f = kit_fixture();
    // A longer executor fragment changes only the executor role surface.
    f.write("global/roles/executor.md", "# Executor\none more rule\n");
    let grown = f.account(&[]);
    let output = String::from_utf8_lossy(&grown.stdout).into_owned();
    assert_eq!(
        accounting_field(&output, "executor", "fixed", "role_bytes"),
        25
    );
    assert_eq!(
        accounting_field(&output, "executor", "fixed", "fixed_bytes"),
        94
    );
    assert_eq!(accounting_field(&output, "lead", "fixed", "role_bytes"), 7);
    assert_eq!(
        accounting_field(&output, "lead", "fixed", "fixed_bytes"),
        105
    );
    // Shared catalogue metadata moves both roles; a lead-only skill description
    // moves only the lead total.
    f.write(
        "fixture-skills/fixture-skill/SKILL.md",
        "---\nname: fixture-skill\ndescription: Owned fixture skill plus\n---\nFixture.\n",
    );
    let shared = f.account(&[]);
    let output = String::from_utf8_lossy(&shared.stdout).into_owned();
    assert_eq!(
        accounting_field(&output, "executor", "fixed", "metadata_bytes"),
        39
    );
    assert_eq!(
        accounting_field(&output, "lead", "fixed", "metadata_bytes"),
        68
    );
    f.write(
        "fixture-skills/lead-only/SKILL.md",
        "---\nname: lead-only\ndescription: Lead fixture skill more\n---\nLead.\n",
    );
    let lead_only = f.account(&[]);
    let output = String::from_utf8_lossy(&lead_only.stdout).into_owned();
    assert_eq!(
        accounting_field(&output, "executor", "fixed", "metadata_bytes"),
        39
    );
    assert_eq!(
        accounting_field(&output, "lead", "fixed", "metadata_bytes"),
        73
    );
    // Bytes, not characters: a multi-byte fragment counts UTF-8 bytes.
    f.write("global/roles/executor.md", "# Executor\n\u{e9}\n");
    let unicode = f.account(&[]);
    let output = String::from_utf8_lossy(&unicode.stdout).into_owned();
    assert_eq!(
        accounting_field(&output, "executor", "fixed", "role_bytes"),
        14
    );
    assert_eq!(
        accounting_field(&output, "executor", "fixed", "role_words"),
        3
    );
}

#[test]
fn accounting_counts_invoked_bodies_and_deduplicated_local_references() {
    let f = kit_fixture();
    f.write(
        "fixture-skills/fixture-skill/SKILL.md",
        "---\nname: fixture-skill\ndescription: Owned fixture skill\n---\nFixture body.\n[Notes](notes.md)\n[Again](notes.md)\n[Site](https://example.com/x)\n",
    );
    f.write("fixture-skills/fixture-skill/notes.md", "notes\n");
    let measured = f.account(&["executor=fixture-skill"]);
    assert_eq!(
        measured.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&measured.stderr)
    );
    let output = String::from_utf8_lossy(&measured.stdout);
    assert!(
        output.contains(concat!(
            "guidance-accounting role=executor surface=invoked invoked_skills=1 invoked_bytes=80 invoked_words=5 ",
            "local_references=1 local_reference_bytes=6 local_reference_words=1 external_references=1 ",
            "unresolved_references=0 composed_bytes=166 composed_words=17"
        )),
        "{output}"
    );
    assert!(!output.contains("role=lead surface=invoked"), "{output}");
    // A skill that is not visible to the chosen role is refused.
    let hidden = f.account(&["executor=lead-only"]);
    assert_eq!(hidden.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&hidden.stderr).contains("not visible to that role"),
        "{}",
        String::from_utf8_lossy(&hidden.stderr)
    );
}

#[test]
fn accounting_counts_repeated_invoked_names_once() {
    let f = kit_fixture();
    f.write(
        "fixture-skills/fixture-skill/SKILL.md",
        "---\nname: fixture-skill\ndescription: Owned fixture skill\n---\nFixture body.\n[Notes](notes.md)\n[Again](notes.md)\n[Site](https://example.com/x)\n",
    );
    f.write("fixture-skills/fixture-skill/notes.md", "notes\n");
    let once = f.account(&["executor=fixture-skill"]);
    let repeated = f.account(&["executor=fixture-skill,fixture-skill"]);
    for measured in [&once, &repeated] {
        assert_eq!(
            measured.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&measured.stderr)
        );
    }
    let line = |output: &Output| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find(|line| line.starts_with("guidance-accounting role=executor surface=invoked "))
            .expect("no executor invoked accounting line")
            .to_owned()
    };
    // Repeating a name must not change the measured line.
    assert_eq!(line(&once), line(&repeated));
    assert_eq!(
        accounting_field(&line(&repeated), "executor", "invoked", "invoked_skills"),
        1
    );
}

#[test]
fn accounting_counts_links_across_invoked_bodies_once() {
    let f = kit_fixture();
    f.write(
        "global/roles.toml",
        "schema = 1\nshared_skills = [\"fixture-skill\", \"other-skill\"]\n[roles.lead]\ninstructions = \"global/roles/lead.md\"\nskills = [\"lead-only\"]\n[roles.delegated-lead]\ninstructions = \"global/roles/delegated-lead.md\"\nskills = []\n[roles.executor]\ninstructions = \"global/roles/executor.md\"\nskills = []\n",
    );
    f.write(
        "fixture-skills/fixture-skill/SKILL.md",
        "---\nname: fixture-skill\ndescription: Owned fixture skill\n---\nFixture body.\n[Notes](notes.md)\n[Other](../other-skill/SKILL.md)\n[Shared](../shared.md)\n",
    );
    f.write("fixture-skills/fixture-skill/notes.md", "notes\n");
    f.write(
        "fixture-skills/other-skill/SKILL.md",
        "---\nname: other-skill\ndescription: Other fixture skill\n---\nOther body.\n[Shared](../shared.md)\n",
    );
    f.write("fixture-skills/shared.md", "shared\n");
    let measured = f.account(&["executor=fixture-skill,other-skill"]);
    assert_eq!(
        measured.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&measured.stderr)
    );
    let output = String::from_utf8_lossy(&measured.stdout).into_owned();
    // Both bodies count once, the link into the other invoked body and the
    // shared target do not add text again, and notes.md resolves from the
    // invoking skill's own directory.
    let field = |key: &str| accounting_field(&output, "executor", "invoked", key);
    assert_eq!(field("invoked_skills"), 2);
    assert_eq!(field("invoked_bytes"), 123);
    assert_eq!(field("invoked_words"), 8);
    assert_eq!(field("local_references"), 2);
    assert_eq!(field("local_reference_bytes"), 13);
    assert_eq!(field("local_reference_words"), 2);
    assert_eq!(field("external_references"), 0);
    assert_eq!(field("unresolved_references"), 0);
    assert_eq!(field("composed_bytes"), 248);
    assert_eq!(field("composed_words"), 25);
    assert!(
        output.contains("reference_scope=all_links_upper_bound"),
        "{output}"
    );
}

#[test]
fn accounting_normalizes_checkout_line_endings() {
    let f = kit_fixture();
    let files = [
        ("global/principles-of-work.md", "# Working principles\n"),
        (
            "global/harness.config.toml",
            "developer_instructions = 'Shared context'\n",
        ),
        ("global/roles/executor.md", "# Executor\none more rule\n"),
        (
            "fixture-skills/fixture-skill/SKILL.md",
            "---\nname: fixture-skill\ndescription: Owned fixture skill\n---\nFixture body.\n[Notes](notes.md)\n",
        ),
        ("fixture-skills/fixture-skill/notes.md", "notes\n"),
    ];
    for (name, body) in files {
        f.write(name, body);
    }
    let lf = f.account(&["executor=fixture-skill"]);
    assert_eq!(
        lf.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&lf.stderr)
    );
    let lf_output = String::from_utf8_lossy(&lf.stdout).into_owned();
    for (name, body) in files {
        f.write(name, &body.replace('\n', "\r\n"));
    }
    let crlf = f.account(&["executor=fixture-skill"]);
    assert_eq!(
        crlf.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&crlf.stderr)
    );
    let crlf_output = String::from_utf8_lossy(&crlf.stdout).into_owned();
    for (surface, key) in [
        ("fixed", "fixed_bytes"),
        ("fixed", "fixed_words"),
        ("invoked", "invoked_bytes"),
        ("invoked", "invoked_words"),
        ("invoked", "local_reference_bytes"),
        ("invoked", "local_reference_words"),
    ] {
        assert_eq!(
            accounting_field(&crlf_output, "executor", surface, key),
            accounting_field(&lf_output, "executor", surface, key),
            "{surface} {key} changed with checkout line endings"
        );
    }
}

#[test]
fn accounting_requires_a_kit_root_and_valid_role_inputs() {
    let plain = Fixture::new();
    plain.write("README.md", "# Plain\n");
    let refused = plain.account(&[]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("role accounting unavailable"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    let f = kit_fixture();
    let unpaired = Command::new(env!("CARGO_BIN_EXE_harness-source-check"))
        .arg("--root")
        .arg(f.repo())
        .args(["--invoked", "executor=fixture-skill"])
        .current_dir(f.0.path())
        .output()
        .unwrap();
    assert_eq!(unpaired.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&unpaired.stderr).contains("--invoked requires --accounting"),
        "{}",
        String::from_utf8_lossy(&unpaired.stderr)
    );
    let wrong_role = f.account(&["planner=fixture-skill"]);
    assert_eq!(wrong_role.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&wrong_role.stderr).contains("lead or executor"),
        "{}",
        String::from_utf8_lossy(&wrong_role.stderr)
    );

    fs::remove_file(f.repo().join("global/roles/executor.md")).unwrap();
    let broken = f.account(&[]);
    assert_eq!(broken.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&broken.stdout).contains("role-context-invalid"));
    assert!(
        String::from_utf8_lossy(&broken.stderr).contains("role accounting unavailable"),
        "{}",
        String::from_utf8_lossy(&broken.stderr)
    );
}

#[test]
fn accounting_keeps_findings_and_still_reports_a_valid_kit() {
    let f = kit_fixture();
    f.write("README.md", "[Missing](absent.md)\n");
    let bad = f.account(&[]);
    assert_eq!(bad.status.code(), Some(1));
    let output = String::from_utf8_lossy(&bad.stdout);
    assert!(output.contains("missing-local-link"), "{output}");
    assert!(
        output.contains("guidance-accounting role=executor surface=fixed"),
        "{output}"
    );
}
