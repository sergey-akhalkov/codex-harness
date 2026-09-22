//! Model-free structured-assignment validation and rendering through the
//! installed entry point: `executor assignment` names one pool slot, checks
//! the assignment against that checkout and prints the exact brief a session
//! would receive. Nothing here claims a slot, writes a file or starts a
//! launcher, so no model, subscription or terminal is involved.
#![cfg(windows)]

use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

/// Owned source checkout with one registered pool slot: the same layout a
/// dispatch allocates, without any upstream or launcher.
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    slot: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("executor-assignment-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source = root.join("proj");
        git(
            &root,
            &[
                "init",
                "-q",
                "--initial-branch=main",
                source.to_str().unwrap(),
            ],
        );
        configure(&source);
        fs::write(source.join("README.md"), "seed\n").unwrap();
        fs::create_dir_all(source.join("global")).unwrap();
        fs::write(
            source.join("global/orchestration.toml"),
            "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\n",
        )
        .unwrap();
        git(&source, &["add", "."]);
        git(&source, &["commit", "-qm", "seed"]);
        let slot = root.join("proj-wt1");
        git(
            &source,
            &[
                "worktree",
                "add",
                "--detach",
                slot.to_str().unwrap(),
                "HEAD",
            ],
        );
        Self { root, source, slot }
    }

    fn head(&self) -> String {
        git_output(&self.source, &["rev-parse", "HEAD"])
    }

    fn assignment(&self, name: &str, inputs: &[&str], outputs: &[&str]) -> PathBuf {
        let path = self.root.join(name);
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "objective": "Extend the synthetic module",
                "inputs": inputs,
                "outputs": outputs,
                "invariants": ["keep the change inside the checkout"],
                "acceptance": ["the synthetic check passes"],
            }))
            .unwrap(),
        )
        .unwrap();
        path
    }

    fn check(&self, assignment: &Path, extra: &[&str]) -> std::process::Output {
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "assignment",
            "--source",
            self.source.to_str().unwrap(),
            "--slot",
            "1",
            "--assignment",
            assignment.to_str().unwrap(),
        ]);
        command.args(extra);
        command.output().unwrap()
    }

    fn drop(self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn output_text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn configure(cwd: &Path) {
    git(cwd, &["config", "user.email", "executor@example.test"]);
    git(cwd, &["config", "user.name", "Executor"]);
}

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_output(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn dry_run_renders_the_bound_checkout_base_and_exact_paths() {
    let fixture = Fixture::new("render");
    fs::write(fixture.slot.join("input.txt"), "declared input\n").unwrap();
    fs::write(fixture.slot.join("existing_child.rs"), "// child\n").unwrap();
    let assignment = fixture.assignment(
        "assignment.json",
        &["input.txt", "existing_child.rs"],
        &["crates/new/module.rs"],
    );
    let before = git_output(
        &fixture.slot,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    );
    let out = fixture.check(&assignment, &["--owner", "exec-ds-9"]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("executor assignment valid: slot=1"), "{text}");
    assert!(
        text.contains(&format!("checkout={}", fixture.slot.display())),
        "{text}"
    );
    assert!(text.contains(&format!("base={}", fixture.head())), "{text}");
    assert!(text.contains("inputs=2 outputs=1"), "{text}");
    assert!(
        text.contains("(nothing claimed, written or launched)"),
        "{text}"
    );
    assert!(text.contains("Structured executor assignment (schema 1)"));
    assert!(
        text.contains(&format!("checkout: {}", fixture.slot.display())),
        "{text}"
    );
    assert!(text.contains("owner: exec-ds-9"), "{text}");
    assert!(text.contains("- input.txt"), "{text}");
    assert!(text.contains("- existing_child.rs"), "{text}");
    assert!(text.contains("- crates/new/module.rs"), "{text}");
    assert!(text.contains("- the synthetic check passes"), "{text}");
    // The check is read-only: the slot keeps exactly the state it had and no
    // state root is written.
    assert_eq!(
        git_output(
            &fixture.slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        ),
        before,
        "the dry run must not change the slot"
    );
    assert!(
        !fixture.slot.join("crates/new/module.rs").exists(),
        "the dry run must not create the declared output"
    );
    assert!(!fixture.root.join("home").exists());
    fixture.drop();
}

#[test]
fn missing_inputs_traversal_and_absolute_paths_are_rejected() {
    let fixture = Fixture::new("reject");
    let missing = fixture.assignment("missing.json", &["crates/absent.rs"], &[]);
    let out = fixture.check(&missing, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("crates/absent.rs"), "{text}");
    assert!(text.contains("is missing from the checkout"), "{text}");

    let traversal = fixture.assignment("traversal.json", &["../outside.txt"], &[]);
    let out = fixture.check(&traversal, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("must not traverse with '..'"), "{text}");

    let absolute = fixture.assignment("absolute.json", &[r"C:\outside.txt"], &[]);
    let out = fixture.check(&absolute, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("must be a plain relative path"), "{text}");
    fixture.drop();
}

/// A directory junction is the unprivileged Windows reparse point a declared
/// path could use to leave the checkout; both an input reaching through it and
/// an output created through it must be rejected before anything is read.
#[test]
fn escaping_reparse_points_are_rejected_for_inputs_and_outputs() {
    let fixture = Fixture::new("escape");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "outside the checkout\n").unwrap();
    let junction = fixture.slot.join("escape");
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "junction creation failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    let input = fixture.assignment("link-input.json", &["escape/secret.txt"], &[]);
    let out = fixture.check(&input, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("resolves outside the checkout"), "{text}");

    let output = fixture.assignment("link-output.json", &[], &["escape/new.txt"]);
    let out = fixture.check(&output, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("resolves outside the checkout"), "{text}");
    assert!(
        !outside.join("new.txt").exists(),
        "the escape must not create anything outside the checkout"
    );
    fixture.drop();
}
