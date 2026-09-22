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
    let limit = 24 * 1024;
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
    f.write("global/kit.json", "{}");
    f.write("global/principles-of-work.md", "# Working principles\n");
    for owner in harness_core::report_owners::TOKEN_AUDIT_OWNERS {
        f.write(owner, "# Owner\n");
    }
    f
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
