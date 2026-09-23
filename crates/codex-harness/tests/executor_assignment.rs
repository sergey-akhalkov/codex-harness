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
        self.assignment_with(name, &[], inputs, outputs)
    }

    /// The same schema-1 document with additional top-level fields, so a check
    /// can declare the optional contract fields or a deliberately malformed
    /// one without hand-writing JSON in every case.
    fn assignment_with(
        &self,
        name: &str,
        extra: &[(&str, serde_json::Value)],
        inputs: &[&str],
        outputs: &[&str],
    ) -> PathBuf {
        let path = self.root.join(name);
        let mut document = json!({
            "schema": 1,
            "objective": "Extend the synthetic module",
            "inputs": inputs,
            "outputs": outputs,
            "invariants": ["keep the change inside the checkout"],
            "acceptance": ["the synthetic check passes"],
        });
        let object = document.as_object_mut().unwrap();
        for (key, value) in extra {
            object.insert((*key).to_owned(), value.clone());
        }
        fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
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

/// An explicit `--base REV` must name the commit the inspected slot is really
/// at: this check never synchronizes, so a short revision is resolved in the
/// slot and anything the slot would have to move to is refused without writes.
#[test]
fn explicit_base_resolves_in_the_slot_and_rejects_other_revisions() {
    let fixture = Fixture::new("base");
    fs::write(fixture.slot.join("input.txt"), "declared input\n").unwrap();
    let assignment = fixture.assignment("assignment.json", &["input.txt"], &["out.txt"]);
    let head = fixture.head();
    let short = &head[..8];
    let before = git_output(
        &fixture.slot,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    );

    let out = fixture.check(&assignment, &["--base", short]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains(&format!("base={head}")), "{text}");
    assert!(text.contains(&format!("base: {head}")), "{text}");
    // The short revision appears only as the prefix of the emitted full one.
    assert_eq!(text.matches(&format!("base: {short}")).count(), 1, "{text}");
    assert_eq!(
        git_output(
            &fixture.slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        ),
        before
    );

    // A revision that exists in the repository but is not this slot's HEAD is
    // refused: the route does not synchronize the slot.
    fs::write(fixture.source.join("moved.txt"), "moved on\n").unwrap();
    git(&fixture.source, &["add", "moved.txt"]);
    git(
        &fixture.source,
        &["commit", "-qm", "move the source checkout on"],
    );
    let moved = git_output(&fixture.source, &["rev-parse", "HEAD"]);
    assert_ne!(moved, head);
    let out = fixture.check(&assignment, &["--base", &moved]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("never synchronizes"), "{text}");
    assert!(text.contains(&moved), "{text}");
    assert!(text.contains(&head), "{text}");

    // An unknown revision is refused the same way.
    let unknown = "0".repeat(40);
    let out = fixture.check(&assignment, &["--base", &unknown]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("is not a commit in"), "{text}");

    // Neither refusal wrote anything.
    assert_eq!(
        git_output(
            &fixture.slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        ),
        before
    );
    assert!(!fixture.slot.join("out.txt").exists());
    assert!(!fixture.root.join("home").exists());
    fixture.drop();
}

/// A missing slot cannot be inspected: the remedy must name the dispatch that
/// creates and synchronizes it, because pruning a stale registration alone
/// cannot bring the slot back.
#[test]
fn missing_slot_names_the_dispatch_that_creates_it() {
    let fixture = Fixture::new("missing-slot");
    let assignment = fixture.assignment("assignment.json", &[], &["out.txt"]);
    fs::remove_dir_all(&fixture.slot).unwrap();
    let out = fixture.check(&assignment, &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("is missing"), "{text}");
    assert!(text.contains("executor spawn --source"), "{text}");
    assert!(text.contains("create and synchronize"), "{text}");
    assert!(!fixture.slot.exists());
    assert!(!fixture.root.join("home").exists());
    fixture.drop();
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
    assert!(
        text.contains("verify the checkout is at the base above"),
        "{text}"
    );
    assert!(
        text.contains("Choose the installed skills this assignment needs"),
        "{text}"
    );
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

/// The result consumer and the escalation boundaries reach the exact brief
/// the installed render path prints: an existing schema-1 document keeps
/// working with the dispatching lead as the consumer, and a declared consumer
/// or trigger is carried beside the standing boundaries instead of replacing
/// them.
#[test]
fn consumer_and_escalation_reach_the_rendered_brief() {
    let fixture = Fixture::new("contract");
    fs::write(fixture.slot.join("input.txt"), "declared input\n").unwrap();

    // No declared contract fields: the schema-1 document still validates.
    let minimal = fixture.assignment("assignment.json", &["input.txt"], &["out.txt"]);
    let out = fixture.check(&minimal, &[]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("consumer: the lead that dispatched this assignment"),
        "{text}"
    );
    assert!(
        text.contains("escalate to the lead that dispatched this assignment"),
        "{text}"
    );
    assert!(
        text.contains("- a change to the agreed outcome or scope"),
        "{text}"
    );
    assert!(
        text.contains("- a concrete dependency you cannot obtain"),
        "{text}"
    );
    assert!(
        text.contains("work cycle (yours): read the declared inputs yourself"),
        "{text}"
    );
    assert!(
        text.contains("report a compact result: done and remaining work"),
        "{text}"
    );

    // Declared consumer and additional triggers travel with the standing ones.
    let declared = fixture.assignment_with(
        "declared.json",
        &[
            ("consumer", json!("the lead of epic sample-3mu")),
            (
                "escalate",
                json!(["any change to the synthetic fixture layout"]),
            ),
        ],
        &["input.txt"],
        &["out.txt"],
    );
    let out = fixture.check(&declared, &[]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("consumer: the lead of epic sample-3mu"),
        "{text}"
    );
    assert!(
        text.contains("escalate to the lead of epic sample-3mu"),
        "{text}"
    );
    assert!(
        text.contains("- any change to the synthetic fixture layout"),
        "{text}"
    );
    assert!(
        text.contains("- a material architecture or design change"),
        "{text}"
    );
    assert!(
        text.contains("the decision you need from the lead of epic sample-3mu"),
        "{text}"
    );
    fixture.drop();
}

/// Malformed contract fields fail before any rendering, with the field named,
/// and an unknown field beside them is still refused: the optional fields do
/// not loosen schema-1 validation.
#[test]
fn malformed_contract_fields_are_refused_without_writing() {
    let fixture = Fixture::new("contract-reject");
    fs::write(fixture.slot.join("input.txt"), "declared input\n").unwrap();
    let before = git_output(
        &fixture.slot,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    );

    for (name, field, value, expected) in [
        (
            "empty-consumer.json",
            "consumer",
            json!(""),
            "assignment consumer is empty",
        ),
        (
            "long-consumer.json",
            "consumer",
            json!("x".repeat(600)),
            "assignment consumer is",
        ),
        (
            "empty-trigger.json",
            "escalate",
            json!([""]),
            "assignment escalate[0] is empty",
        ),
        (
            "renamed-field.json",
            "escalations",
            json!(["any change to the layout"]),
            "unknown field",
        ),
    ] {
        let assignment =
            fixture.assignment_with(name, &[(field, value)], &["input.txt"], &["out.txt"]);
        let out = fixture.check(&assignment, &[]);
        let text = output_text(&out);
        assert!(!out.status.success(), "{name}: {text}");
        assert!(text.contains(expected), "{name}: {text}");
    }

    // Every refusal happened before anything was rendered, claimed or written.
    assert_eq!(
        git_output(
            &fixture.slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        ),
        before
    );
    assert!(!fixture.slot.join("out.txt").exists());
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
