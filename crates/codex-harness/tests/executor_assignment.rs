//! Model-free structured-assignment validation and rendering through the
//! installed entry point: `executor assignment` names one pool slot, checks
//! the assignment against that checkout and prints the exact brief a session
//! would receive. Nothing here claims a slot, writes a file or starts a
//! launcher, so no model, subscription or terminal is involved.
#![cfg(windows)]

use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[path = "fixtures/synthetic_role_build.rs"]
mod synthetic_role_build;

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
        // Build agents can provide a short 8.3 temp prefix (RUNNER~1) while
        // the rendered checkout path is canonicalized; anchor every path on
        // the long form so the rendered brief matches either environment.
        let canonical = root.canonicalize().unwrap();
        // Strip the verbatim prefix: the manager renders ordinary long paths.
        let root = canonical
            .to_string_lossy()
            .strip_prefix(r"\\?\")
            .map(PathBuf::from)
            .unwrap_or(canonical);
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

/// The same owned checkout plus a controlled CODEX_HOME for the real
/// `executor spawn` entry point. The launcher is deliberately absent: a
/// dispatch must reach the durable pool binding and then stop at the missing
/// launcher, so no model, subscription or terminal is involved.
struct SpawnFixture {
    base: Fixture,
    home: PathBuf,
}

impl SpawnFixture {
    fn new(name: &str) -> Self {
        let base = Fixture::new(name);
        let home = base.root.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("ds.config.toml"), "model = 'deepseek-flash'\n").unwrap();
        Self { base, home }
    }

    /// Installs the real bd executable into the controlled home and initializes
    /// one synthetic board in the consuming source checkout.
    fn with_board(name: &str) -> Self {
        let fixture = Self::new(name);
        let bin = fixture.home.join("harness/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::copy(bd_executable(), bin.join("bd.exe")).unwrap();
        let out = fixture.bd_run(&[
            "init",
            "--skip-agents",
            "--non-interactive",
            "--quiet",
            "--prefix",
            "assign",
        ]);
        assert!(out.status.success(), "{}", bd_failed("init", &out));
        fixture
    }

    fn bd(&self) -> PathBuf {
        self.home.join("harness/bin/bd.exe")
    }

    fn bd_run(&self, args: &[&str]) -> std::process::Output {
        Command::new(self.bd())
            .args(args)
            .current_dir(&self.base.source)
            .env("BD_NON_INTERACTIVE", "1")
            .env("BEADS_ACTOR", "executor-assignment-test")
            .output()
            .unwrap()
    }

    fn bd_json(&self, args: &[&str]) -> Value {
        let out = self.bd_run(args);
        assert!(
            out.status.success(),
            "{}",
            bd_failed(args.first().copied().unwrap_or("bd"), &out)
        );
        serde_json::from_slice(&out.stdout).unwrap_or_else(|error| {
            panic!(
                "bd {} did not return JSON: {error}\n{}",
                args.join(" "),
                String::from_utf8_lossy(&out.stdout)
            )
        })
    }

    fn create_issue(&self, title: &str, issue_type: &str, parent: Option<&str>) -> String {
        let mut args = vec!["create", title, "--type", issue_type];
        if let Some(parent) = parent {
            args.push("--parent");
            args.push(parent);
        }
        args.push("--json");
        self.bd_json(&args)["id"].as_str().unwrap().to_owned()
    }

    fn set_metadata(&self, issue: &str, key: &str, value: &str) {
        let pair = format!("{key}={value}");
        self.bd_json(&["update", issue, "--set-metadata", &pair, "--json"]);
    }

    fn show(&self, issue: &str) -> Value {
        let shown = self.bd_json(&["show", issue, "--json"]);
        shown
            .as_array()
            .and_then(|rows| rows.first())
            .cloned()
            .unwrap_or(shown)
    }

    fn assignment(&self, name: &str, board_task: &str, purpose: Option<&str>) -> PathBuf {
        let mut contract = json!({
            "schema": 1,
            "objective": "run the synthetic board-bound assignment",
            "board_task": board_task,
            "inputs": ["README.md"],
            "outputs": ["out/bound.txt"],
            "invariants": ["keep the change inside the checkout"],
            "acceptance": ["the synthetic check passes"],
        });
        if let Some(purpose) = purpose {
            contract["purpose"] = json!(purpose);
        }
        let path = self.base.root.join(name);
        fs::write(&path, serde_json::to_vec_pretty(&contract).unwrap()).unwrap();
        path
    }

    fn legacy_assignment(&self, name: &str) -> PathBuf {
        let path = self.base.root.join(name);
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "objective": "legacy structured assignment without a board binding",
                "inputs": ["README.md"],
                "outputs": ["out/legacy.txt"],
                "invariants": ["keep the change inside the checkout"],
                "acceptance": ["the synthetic check passes"],
            }))
            .unwrap(),
        )
        .unwrap();
        path
    }

    /// The real dispatch entry point, pointed at the controlled home and the
    /// exact local base. The missing launcher is the intended stop after the
    /// pool/slot owner wrote its durable binding.
    fn spawn(&self, prompt: &[&str], extra: &[&str]) -> std::process::Output {
        self.spawn_command(prompt, extra).output().unwrap()
    }

    fn spawn_command(&self, prompt: &[&str], extra: &[&str]) -> Command {
        let base = self.base.head();
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "spawn",
            "--source",
            self.base.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
            "--profile",
            "ds",
            "--base",
            &base,
        ]);
        command.args(prompt);
        command.args(extra);
        command.env("CODEX_HOME", &self.home);
        command.env("CODEX_THREAD_ID", "lead-thread-synthetic");
        for key in [
            "HARNESS_EXECUTOR_SESSION",
            "CODEX_SESSION_ID",
            "HARNESS_EXECUTOR_RUN",
            "HARNESS_ORIGINATING_LEAD",
            "HARNESS_LEAD_THREAD",
            "HARNESS_LEAD_RECIPIENT",
        ] {
            command.env_remove(key);
        }
        command
    }

    /// One explicit continuation of the recorded slot session through the real
    /// `executor resume` owner. Free text is recorded as itself, not as the
    /// slot's bound structured assignment.
    fn resume_command(&self, owner: &str, session: &str, prompt: &str) -> Command {
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "resume",
            "--source",
            self.base.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
            "--slot",
            "1",
            "--owner",
            owner,
            "--session",
            session,
            "--exec",
            prompt,
        ]);
        command.env("CODEX_HOME", &self.home);
        command.env("CODEX_THREAD_ID", "lead-thread-synthetic");
        for key in [
            "HARNESS_EXECUTOR_SESSION",
            "CODEX_SESSION_ID",
            "HARNESS_EXECUTOR_RUN",
            "HARNESS_ORIGINATING_LEAD",
            "HARNESS_LEAD_THREAD",
            "HARNESS_LEAD_RECIPIENT",
        ] {
            command.env_remove(key);
        }
        command
    }

    fn release(&self, extra: &[&str]) -> std::process::Output {
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "release",
            "--source",
            self.base.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
        ]);
        command.args(extra);
        for key in [
            "HARNESS_EXECUTOR_SESSION",
            "CODEX_SESSION_ID",
            "HARNESS_EXECUTOR_RUN",
            "HARNESS_ORIGINATING_LEAD",
            "HARNESS_LEAD_THREAD",
            "HARNESS_LEAD_RECIPIENT",
        ] {
            command.env_remove(key);
        }
        command.output().unwrap()
    }

    fn state_dir(&self) -> PathBuf {
        self.home.join("harness/executor-pool")
    }

    fn record(&self, index: u32) -> Value {
        serde_json::from_slice(&fs::read(self.record_path(index)).unwrap()).unwrap()
    }

    fn record_path(&self, index: u32) -> PathBuf {
        let source = fs::read_dir(self.state_dir())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.is_dir())
            .expect("one pool state directory per source checkout");
        source.join(format!("slot-{index}.json"))
    }

    fn drop_fixture(self) {
        self.base.drop();
    }
}

fn bd_name() -> &'static str {
    "bd.exe"
}

fn bd_executable() -> PathBuf {
    if let Some(value) = std::env::var_os("HARNESS_BD_EXE") {
        let candidate = PathBuf::from(value);
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        let candidate = PathBuf::from(home).join("harness/bin").join(bd_name());
        if candidate.is_file() {
            return candidate;
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let lower = dir.to_string_lossy().to_ascii_lowercase();
            if lower.ends_with(r"\windowsapps") || lower.contains(r"\windowsapps\\") {
                continue;
            }
            let candidate = dir.join(bd_name());
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    panic!("bd v1.3.0 is required on PATH, CODEX_HOME/harness/bin, or HARNESS_BD_EXE");
}

fn bd_failed(op: &str, out: &std::process::Output) -> String {
    format!(
        "bd {op} failed: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
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

/// Install the native CLI test double where the launcher owner resolves it.
fn install_launcher(home: &Path, exe: &Path) {
    let launcher = home.join("harness/bin/codex.exe");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    if fs::hard_link(exe, &launcher).is_err() {
        fs::copy(exe, &launcher).unwrap_or_else(|error| {
            panic!(
                "the fixture home could not install the native CLI {}: {error}",
                exe.display()
            )
        });
    }
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
    assert!(text.contains("executor spawn --repository"), "{text}");
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
        text.contains("feedback: return the result and any decision you escalate to the lead that dispatched this assignment through codex-harness lead message (short --text fallback)."),
        "{text}"
    );
    // Standing responsibility guidance (work cycle, base verification, the
    // heavy-command lane, the compact result shape) reaches a managed session
    // through its role context and is not restated in the brief.
    for standing in [
        "work cycle (yours)",
        "verify the checkout is at the base above",
        "Choose the installed skills this assignment needs",
        "report a compact result: done and remaining work",
    ] {
        assert!(!text.contains(standing), "{standing}\n{text}");
    }
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

/// The result consumer, the concrete feedback address and the declared
/// additional boundaries reach the exact brief the installed render path
/// prints: an existing schema-1 document keeps working with the dispatching
/// lead as the consumer, and a declared consumer or trigger is carried beside
/// the feedback address while the standing boundaries stay owned by the
/// executor role fragment.
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
        !text.contains("purpose:"),
        "an omitted purpose stays unspecified: {text}"
    );
    assert!(
        text.contains(
            "escalate to the lead that dispatched this assignment (additional boundaries)"
        ),
        "{text}"
    );
    assert!(
        text.contains("feedback: return the result and any decision you escalate to the lead that dispatched this assignment through codex-harness lead message (short --text fallback)."),
        "{text}"
    );
    // Standing boundaries, waiting behavior and the compact result shape stay
    // owned by the executor role fragment: none of them is copied into the
    // brief.
    for standing in [
        "- a change to the agreed outcome or scope",
        "- a concrete dependency you cannot obtain",
        "work cycle (yours)",
        "no polling, keep-alive loop or resume",
        "report a compact result: done and remaining work",
    ] {
        assert!(!text.contains(standing), "{standing}\n{text}");
    }

    // Declared consumer and additional triggers travel with the standing ones.
    let declared = fixture.assignment_with(
        "declared.json",
        &[
            ("consumer", json!("the lead of epic sample-3mu")),
            ("purpose", json!("improvement")),
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
    assert!(text.contains("purpose: improvement"), "{text}");
    assert!(
        text.contains("escalate to the lead of epic sample-3mu (additional boundaries)"),
        "{text}"
    );
    assert!(
        text.contains("- any change to the synthetic fixture layout"),
        "{text}"
    );
    assert!(
        text.contains("feedback: return the result and any decision you escalate to the lead of epic sample-3mu through codex-harness lead message (short --text fallback)."),
        "{text}"
    );
    assert!(
        !text.contains("- a material architecture or design change"),
        "standing boundaries stay with the role fragment: {text}"
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
            "unknown-purpose.json",
            "purpose",
            json!("auto"),
            "purpose 'auto' is not ordinary, reflection or improvement",
        ),
        (
            "empty-purpose.json",
            "purpose",
            json!(""),
            "assignment purpose is empty",
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

/// A normal structured assignment that names a consumer-board task reaches the
/// real dispatch owner and leaves a durable slot binding: canonical board
/// scope, issue, assignment content hash and the declared purpose. The
/// missing launcher is the intended stop after that write.
#[test]
fn board_bound_assignment_records_verified_task_purpose_and_identity() {
    let fixture = SpawnFixture::with_board("bound-identity");
    let task = fixture.create_issue("Bound ordinary task", "task", None);
    let assignment = fixture.assignment("bound.json", &task, Some("ordinary"));
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-bound"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "the dispatch must reach the launcher only after the slot binding: {text}"
    );
    let record = fixture.record(1);
    assert_eq!(record["state"], "occupied", "{record}");
    let binding = &record["assignment"];
    assert_eq!(binding["issue"], task, "{record}");
    assert_eq!(binding["purpose"], "ordinary", "{record}");
    assert_eq!(binding["purposeSource"], "declared", "{record}");
    let sha = binding["assignmentSha256"].as_str().expect("content hash");
    assert_eq!(sha.len(), 64, "{record}");
    assert!(
        sha.chars().all(|value| value.is_ascii_hexdigit()),
        "{record}"
    );
    let context = fixture.bd_json(&["context", "--json"]);
    assert_eq!(binding["source"]["project_id"], context["project_id"]);
    assert_eq!(binding["source"]["database"], context["database"]);
    assert_eq!(binding["source"]["beads_dir"], context["beads_dir"]);
    assert!(!binding["source"]["commit"].as_str().unwrap().is_empty());
    assert!(
        Path::new(binding["source"]["repo_root"].as_str().unwrap()).ends_with("proj"),
        "{record}"
    );
    let shown = fixture.show(&task);
    assert_eq!(shown["metadata"]["harness.purpose"], "ordinary", "{shown}");
    fixture.drop_fixture();
}

/// A known nonordinary purpose on the board lineage is retained on the child
/// binding and written to the child's existing `harness.purpose` metadata
/// without disturbing unrelated metadata.
#[test]
fn board_bound_assignment_inherits_known_nonordinary_purpose() {
    let fixture = SpawnFixture::with_board("bound-inherit");
    let epic = fixture.create_issue("Improvement epic", "epic", None);
    fixture.set_metadata(&epic, "harness.purpose", "improvement");
    let child = fixture.create_issue("Improvement child", "task", Some(&epic));
    fixture.set_metadata(&child, "synthetic.keep", "kept");
    let assignment = fixture.assignment("inherit.json", &child, None);
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-inherit"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let record = fixture.record(1);
    let binding = &record["assignment"];
    assert_eq!(binding["issue"], child, "{record}");
    assert_eq!(binding["purpose"], "improvement", "{record}");
    assert_eq!(
        binding["purposeSource"],
        format!("ancestor:{epic}"),
        "{record}"
    );
    let shown = fixture.show(&child);
    assert_eq!(
        shown["metadata"]["harness.purpose"], "improvement",
        "{shown}"
    );
    assert_eq!(shown["metadata"]["synthetic.keep"], "kept", "{shown}");
    fixture.drop_fixture();
}

/// An ordinary declaration under a nonordinary ancestor contradicts the
/// retained lineage and is refused before a slot claim or a launcher.
#[test]
fn conflicting_purpose_declaration_refuses_before_slot_or_launcher() {
    let fixture = SpawnFixture::with_board("bound-conflict");
    let epic = fixture.create_issue("Improvement epic", "epic", None);
    fixture.set_metadata(&epic, "harness.purpose", "improvement");
    let child = fixture.create_issue("Improvement child", "task", Some(&epic));
    let assignment = fixture.assignment("conflict.json", &child, Some("ordinary"));
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-conflict"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("does not silently leave its purpose lineage"),
        "{text}"
    );
    assert!(
        !text.contains("installed Codex launcher"),
        "the refusal must precede every launch stage: {text}"
    );
    assert!(
        !fixture.state_dir().exists(),
        "a refused declaration must not claim a slot"
    );
    let shown = fixture.show(&child);
    assert!(
        shown
            .get("metadata")
            .and_then(|metadata| metadata.get("harness.purpose"))
            .is_none(),
        "a refused declaration must not write board purpose metadata: {shown}"
    );
    fixture.drop_fixture();
}

/// A bound dispatch cannot manufacture a nonordinary purpose by declaration;
/// improvement and reflection purposes must come from the task or its lineage.
#[test]
fn unbacked_nonordinary_declaration_refuses_before_slot_or_launcher() {
    let fixture = SpawnFixture::with_board("bound-unbacked");
    let task = fixture.create_issue("Unbacked improvement", "task", None);
    let assignment = fixture.assignment("unbacked.json", &task, Some("improvement"));
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-unbacked"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("nonordinary purposes are inherited"),
        "{text}"
    );
    assert!(!text.contains("installed Codex launcher"), "{text}");
    assert!(
        !fixture.state_dir().exists(),
        "an unbacked declaration must not claim a slot"
    );
    let shown = fixture.show(&task);
    assert!(
        shown
            .get("metadata")
            .and_then(|metadata| metadata.get("harness.purpose"))
            .is_none(),
        "an unbacked declaration must not write board purpose metadata: {shown}"
    );
    fixture.drop_fixture();
}

/// A board task id that the consuming board cannot resolve refuses before a
/// slot claim or launcher, rather than becoming an unbound ordinary dispatch.
#[test]
fn unknown_board_task_refuses_before_slot_or_launcher() {
    let fixture = SpawnFixture::with_board("bound-missing");
    let missing = "assign-missing";
    let assignment = fixture.assignment("missing.json", missing, Some("ordinary"));
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-missing"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("show failed") && text.contains(missing),
        "{text}"
    );
    assert!(!text.contains("installed Codex launcher"), "{text}");
    assert!(
        !fixture.state_dir().exists(),
        "an unknown board task must not claim a slot"
    );
    fixture.drop_fixture();
}

/// A task with no declared purpose and no nonordinary ancestor is retained as
/// explicitly unknown, not inferred as ordinary, while the ordinary dispatch
/// itself still works.
#[test]
fn unknown_purpose_is_retained_as_unknown_not_ordinary() {
    let fixture = SpawnFixture::with_board("bound-unknown");
    let task = fixture.create_issue("Unknown purpose task", "task", None);
    let assignment = fixture.assignment("unknown.json", &task, None);
    let out = fixture.spawn(
        &["--assignment", assignment.to_str().unwrap()],
        &["--owner", "exec-ds-unknown"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let record = fixture.record(1);
    let binding = &record["assignment"];
    assert_eq!(binding["issue"], task, "{record}");
    assert!(binding["purpose"].is_null(), "{record}");
    assert_eq!(binding["purposeSource"], "unknown", "{record}");
    let shown = fixture.show(&task);
    assert!(
        shown
            .get("metadata")
            .and_then(|metadata| metadata.get("harness.purpose"))
            .is_none(),
        "{shown}"
    );
    fixture.drop_fixture();
}

/// A repeated owner continues its exact slot binding; a different board task
/// on that binding is refused and the original binding stays intact.
#[test]
fn conflicting_recorded_board_binding_refuses_a_retry() {
    let fixture = SpawnFixture::with_board("bound-retry");
    let first = fixture.create_issue("First bound task", "task", None);
    let second = fixture.create_issue("Second bound task", "task", None);
    let first_assignment = fixture.assignment("first.json", &first, None);
    let second_assignment = fixture.assignment("second.json", &second, None);
    let out = fixture.spawn(
        &["--assignment", first_assignment.to_str().unwrap()],
        &["--owner", "exec-ds-retry"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let before = fixture.record(1);
    assert_eq!(before["assignment"]["issue"], first, "{before}");

    let out = fixture.spawn(
        &["--assignment", second_assignment.to_str().unwrap()],
        &["--owner", "exec-ds-retry"],
    );
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("is already bound to board task"),
        "the recorded binding conflict must be named: {text}"
    );
    assert!(!text.contains("installed Codex launcher"), "{text}");
    let after = fixture.record(1);
    assert_eq!(
        after["assignment"], before["assignment"],
        "the refused retry must not replace the binding"
    );
    assert_eq!(after["handle"], before["handle"], "{after}");
    fixture.drop_fixture();
}

/// Free text never resolves a board: the dispatch reaches the launcher and
/// records no assignment binding.
#[test]
fn free_text_dispatch_keeps_an_unbound_slot_record() {
    let fixture = SpawnFixture::new("free-text");
    let out = fixture.spawn(&["--exec", "free text assignment"], &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let record = fixture.record(1);
    assert!(record["assignment"].is_null(), "{record}");
    assert!(
        !fixture.base.source.join(".beads").exists(),
        "free text must not initialize or contact a board"
    );
    fixture.drop_fixture();
}

/// A schema-1 assignment without `board_task` keeps the legacy unbound path.
#[test]
fn legacy_structured_dispatch_keeps_an_unbound_slot_record() {
    let fixture = SpawnFixture::new("legacy-structure");
    let assignment = fixture.legacy_assignment("legacy.json");
    let out = fixture.spawn(&["--assignment", assignment.to_str().unwrap()], &[]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let record = fixture.record(1);
    assert!(record["assignment"].is_null(), "{record}");
    fixture.drop_fixture();
}

/// One completed ordinary executor's exact native facts are retained by run
/// generation, so releasing and reusing the slot index cannot replace them.
/// A later free-text continuation of the same bound slot is retained as
/// explicitly ineligible while its ordinary output and receipt stay usable.
#[test]
fn completed_ordinary_facts_survive_slot_reuse_and_free_text_is_ineligible() {
    let fixture = SpawnFixture::with_board("completion-retained");
    // The continuation resolves the recorded upstream; no dispatch fetches or
    // rewrites it. A local path keeps this check offline.
    git(
        &fixture.base.source,
        &[
            "remote",
            "add",
            "origin",
            fixture.base.source.to_str().unwrap(),
        ],
    );
    install_launcher(
        &fixture.home,
        Path::new(env!("CARGO_BIN_EXE_harness-executor-fixture")),
    );
    fs::write(
        fixture.home.join("ds.config.toml"),
        "model = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    // The controlled child reports its started-thread identity from the
    // active config, so the explicit route must be present there too before
    // dispatch appends the workspace trust entry.
    fs::write(
        fixture.home.join("config.toml"),
        "model = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    let kit = synthetic_role_build::kit_source(&fixture.base.root);
    let build = synthetic_role_build::recorded_build(&fixture.base.root, &kit);
    synthetic_role_build::register(&fixture.home, &fixture.base.root, &build);

    let final_message = "fixture turn completed; the committed solution is in the bound slot";
    let first_task = fixture.create_issue("Completed ordinary task", "task", None);
    let first_assignment =
        fixture.assignment("completion-first.json", &first_task, Some("ordinary"));
    let first_session = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
    let mut command = fixture.spawn_command(
        &["--assignment", first_assignment.to_str().unwrap()],
        &["--owner", "exec-ds-completion-a"],
    );
    command.env("HARNESS_EXECUTOR_FIXTURE_MODE", "hang");
    command.env("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE", "control-app-server");
    command.env("HARNESS_IMPROVEMENT_FIXTURE_SESSION", first_session);
    command.env_remove("WT_SESSION");
    let first_run = command.output().unwrap();
    let text = output_text(&first_run);
    assert_eq!(first_run.status.code(), Some(0), "{text}");

    let state = harness_core::task_worktree::pool_state_dir(&fixture.home, &fixture.base.source)
        .expect("pool state directory");
    let receipt_path = state.join("spawn-1.json");
    let slot_message = state.join("message-1.txt");
    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    let first_generation = receipt["originatingLead"]["runGeneration"]
        .as_str()
        .expect("run generation")
        .to_owned();
    assert_eq!(receipt["control"]["role"]["role"], "executor", "{receipt}");
    assert_eq!(
        receipt["control"]["submittedAssignment"]["kind"], "structured",
        "{receipt}"
    );
    assert_eq!(receipt["observation"]["state"], "completed", "{receipt}");
    assert_eq!(receipt["observation"]["exitCode"], 0, "{receipt}");
    assert_eq!(
        receipt["observation"]["session"], first_session,
        "{receipt}"
    );
    assert_eq!(
        receipt["observation"]["turn"], "fixture-turn-1",
        "the completed turn is observed, not inferred: {receipt}"
    );
    assert_eq!(
        receipt["slot"]["assignment"]["issue"], first_task,
        "{receipt}"
    );
    assert!(
        receipt["slot"]["handle"]
            .as_str()
            .is_some_and(|handle| !handle.is_empty()),
        "{receipt}"
    );
    assert_eq!(fs::read_to_string(&slot_message).unwrap(), final_message);
    let first_revision = git_output(&fixture.base.slot, &["rev-parse", "HEAD"]);
    assert!(
        git_output(
            &fixture.base.slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        )
        .is_empty(),
        "the completed fixture run leaves the bound checkout clean"
    );

    let load = |generation: &str| {
        harness_core::task_worktree::load_retained_completion(
            &fixture.home,
            &fixture.base.source,
            generation,
        )
        .unwrap()
    };
    let first = load(&first_generation).expect("the first completion is retained");
    println!("retained-first={}", serde_json::to_string(&first).unwrap());
    assert!(first.eligibility.eligible, "{first:?}");
    assert!(first.eligibility.reasons.is_empty(), "{first:?}");
    assert_eq!(first.run_generation, first_generation);
    assert_eq!(first.slot, 1);
    assert_eq!(first.owner, "exec-ds-completion-a");
    assert_eq!(first.task.as_deref(), Some("exec-ds-completion-a"));
    assert_eq!(first.role.as_deref(), Some("executor"));
    assert_eq!(first.state, "completed");
    assert_eq!(first.exit_code, Some(0));
    assert_eq!(first.session.as_deref(), Some(first_session));
    assert_eq!(first.turn.as_deref(), Some("fixture-turn-1"));
    let binding = first.assignment.as_ref().expect("board assignment binding");
    assert_eq!(binding.issue, first_task);
    assert_eq!(binding.purpose.as_deref(), Some("ordinary"));
    assert_eq!(
        first.submitted_assignment,
        Some(
            harness_core::task_worktree::AssignmentSubmission::structured(
                binding.assignment_sha256.clone()
            )
        )
    );
    let result = first.result.as_ref().expect("retained result artifact");
    assert!(result.locator.is_file(), "{result:?}");
    assert_eq!(
        result.sha256,
        harness_core::build_identity::hash_file(&result.locator).unwrap()
    );
    assert_eq!(result.bytes, fs::metadata(&result.locator).unwrap().len());
    assert_eq!(
        first.source.revision.as_deref(),
        Some(first_revision.as_str())
    );
    assert_eq!(first.source.dirty, Some(false));

    let released = fixture.release(&[
        "--slot",
        "1",
        "--disposition",
        "discarded",
        "--reason",
        "synthetic completion accepted",
    ]);
    let text = output_text(&released);
    assert_eq!(released.status.code(), Some(0), "{text}");
    let retained_result = first.result.as_ref().expect("retained result");
    assert!(
        retained_result.locator.is_file(),
        "the per-run retained copy survives release, independent of the slot index"
    );
    assert_eq!(
        harness_core::build_identity::hash_file(&retained_result.locator).unwrap(),
        retained_result.sha256,
        "release must not alter the retained acceptance input"
    );
    assert_eq!(
        load(&first_generation).expect("retained through release"),
        first,
        "release must not change the completed run's retained facts or result"
    );

    let second_task = fixture.create_issue("Second completed task", "task", None);
    let second_assignment =
        fixture.assignment("completion-second.json", &second_task, Some("ordinary"));
    let second_session = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f6";
    let mut command = fixture.spawn_command(
        &["--assignment", second_assignment.to_str().unwrap()],
        &["--owner", "exec-ds-completion-b"],
    );
    command.env("HARNESS_EXECUTOR_FIXTURE_MODE", "hang");
    command.env("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE", "control-app-server");
    command.env("HARNESS_IMPROVEMENT_FIXTURE_SESSION", second_session);
    command.env_remove("WT_SESSION");
    let second_run = command.output().unwrap();
    let text = output_text(&second_run);
    assert_eq!(second_run.status.code(), Some(0), "{text}");

    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(
        receipt["slot"]["assignment"]["issue"], second_task,
        "{receipt}"
    );
    assert_eq!(
        receipt["observation"]["session"], second_session,
        "{receipt}"
    );
    let second_generation = receipt["originatingLead"]["runGeneration"]
        .as_str()
        .expect("second run generation")
        .to_owned();
    assert_ne!(first_generation, second_generation);
    assert_eq!(
        load(&first_generation).expect("first retained after slot reuse"),
        first,
        "reusing the slot must not replace the previous run's acceptance input"
    );
    let second = load(&second_generation).expect("the second completion is retained");
    println!(
        "retained-second={}",
        serde_json::to_string(&second).unwrap()
    );
    assert!(second.eligibility.eligible, "{second:?}");
    assert_eq!(second.task.as_deref(), Some("exec-ds-completion-b"));
    assert_eq!(
        fs::read_to_string(&slot_message).unwrap(),
        final_message,
        "the slot-indexed result now belongs to the reused slot's run"
    );

    let first_list = harness_core::task_worktree::retained_completions_for_task(
        &fixture.home,
        &fixture.base.source,
        &first_task,
    )
    .unwrap();
    assert_eq!(first_list.len(), 1, "{first_list:?}");
    assert_eq!(first_list[0].run_generation, first_generation);
    let second_list = harness_core::task_worktree::retained_completions_for_task(
        &fixture.home,
        &fixture.base.source,
        &second_task,
    )
    .unwrap();
    assert_eq!(second_list.len(), 1, "{second_list:?}");
    assert_eq!(second_list[0].run_generation, second_generation);

    // A free-text continuation keeps the bound slot's board binding but must
    // never read as the bound structured assignment. Its own output remains
    // ordinary and inspectable while acceptance eligibility is refused.
    let mut command = fixture.resume_command(
        "exec-ds-completion-b",
        second_session,
        "continue with free text",
    );
    command.env("HARNESS_EXECUTOR_FIXTURE_MODE", "hang");
    command.env("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE", "control-app-server");
    command.env("HARNESS_IMPROVEMENT_FIXTURE_SESSION", second_session);
    command.env_remove("WT_SESSION");
    let resumed = command.output().unwrap();
    let text = output_text(&resumed);
    assert_eq!(resumed.status.code(), Some(0), "{text}");
    // `resume --exec` returns at dispatch acceptance. The terminal
    // ineligibility is asserted from the retained facts below, which are the
    // authority for acceptance rather than this process's transient output.

    let receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    assert_eq!(
        receipt["control"]["submittedAssignment"]["kind"], "freeText",
        "{receipt}"
    );
    assert_eq!(receipt["observation"]["state"], "completed", "{receipt}");
    assert_eq!(receipt["observation"]["exitCode"], 0, "{receipt}");
    assert_eq!(
        receipt["observation"]["session"], second_session,
        "{receipt}"
    );
    let third_generation = receipt["originatingLead"]["runGeneration"]
        .as_str()
        .expect("continuation run generation")
        .to_owned();
    assert_ne!(third_generation, second_generation);
    let third = load(&third_generation).expect("the continuation completion is retained");
    println!(
        "retained-free-text={}",
        serde_json::to_string(&third).unwrap()
    );
    assert!(
        !third.eligibility.eligible,
        "free text must not claim the bound structured assignment: {third:?}"
    );
    assert!(
        third
            .eligibility
            .reasons
            .iter()
            .any(|reason| reason.contains("freeText")),
        "{:?}",
        third.eligibility.reasons
    );
    assert_eq!(third.task.as_deref(), Some("exec-ds-completion-b"));
    assert_eq!(
        third
            .assignment
            .as_ref()
            .map(|binding| binding.issue.as_str()),
        Some(second_task.as_str()),
        "the free-text continuation keeps the bound board issue for review"
    );
    assert_eq!(
        fs::read_to_string(&slot_message).unwrap(),
        final_message,
        "an ineligible completion keeps its ordinary output available"
    );
    assert_eq!(
        load(&first_generation).expect("first retained through continuation"),
        first
    );
    assert_eq!(
        load(&second_generation).expect("second retained through continuation"),
        second
    );

    fixture.drop_fixture();
}
