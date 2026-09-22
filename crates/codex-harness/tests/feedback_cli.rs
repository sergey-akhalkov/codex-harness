//! Installed `codex-harness feedback` over a synthetic, owned bd board.
//!
//! Every check runs the real entry point against an isolated project created
//! by this test: no consumer board, no kit checkout state and no model call.
//! bd v1.3.0 must be discoverable (HARNESS_BD_EXE, CODEX_HOME/harness/bin or
//! PATH); each board operation is a real bd process, so this file is the
//! slowest in the crate by design.

use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

fn bd_name() -> &'static str {
    if cfg!(windows) { "bd.exe" } else { "bd" }
}

fn bd_executable() -> PathBuf {
    if let Some(value) = std::env::var_os("HARNESS_BD_EXE") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return path;
        }
    }
    if let Some(home) = std::env::var_os("CODEX_HOME") {
        let path = PathBuf::from(home).join("harness/bin").join(bd_name());
        if path.is_file() {
            return path;
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let lower = dir.to_string_lossy().to_ascii_lowercase();
            if lower.ends_with(r"\windowsapps") || lower.contains(r"\windowsapps\") {
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

/// One isolated project board with configured limits: threshold 2 (the kit
/// default is 3) and a triage batch of 2 (the kit default is 8), so every
/// check that names a configured value would fail with the defaults.
struct Board {
    root: PathBuf,
    project: PathBuf,
    feature: String,
    bd: PathBuf,
    /// Controlled CODEX_HOME: no installation record unless a check writes
    /// one, so limits resolution never depends on the machine's own install.
    home: PathBuf,
}

impl Board {
    fn new(name: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let root = std::env::temp_dir().join(format!(
            "feedback-cli-{name}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        seed_git(&project);
        let bd = bd_executable();
        let init = bd_run(
            &bd,
            &project,
            &[
                "init",
                "--skip-agents",
                "--non-interactive",
                "--quiet",
                "--prefix",
                "bdct",
            ],
        );
        assert!(init.status.success(), "{}", bd_failed("init", &init));
        let epic = bd_json(
            &bd,
            &project,
            &[
                "create",
                "Stage: synthetic feedback CLI",
                "--type",
                "epic",
                "--json",
            ],
        );
        let feature = bd_json(
            &bd,
            &project,
            &[
                "create",
                "Spec: feedback CLI",
                "--type",
                "feature",
                "--parent",
                epic["id"].as_str().unwrap(),
                "--json",
            ],
        );
        fs::create_dir_all(project.join("global")).unwrap();
        fs::write(
            project.join("global/orchestration.toml"),
            "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 2\nincubator_size_cap = 32\nfeedback_batch_limit = 2\n",
        )
        .unwrap();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        Self {
            root,
            project,
            feature: feature["id"].as_str().unwrap().to_owned(),
            bd,
            home,
        }
    }

    /// Runs one feedback verb against this board with the explicit bd path.
    fn feedback(&self, args: &[&str]) -> std::process::Output {
        self.feedback_with_home(&self.home, args)
    }

    /// The same, with an explicit CODEX_HOME (an installed kit record lives
    /// there or does not).
    fn feedback_with_home(&self, home: &Path, args: &[&str]) -> std::process::Output {
        let mut command = Command::new(manager());
        command.arg("feedback").args(args);
        command.args([
            "--bd",
            self.bd.to_str().unwrap(),
            "--project",
            self.project.to_str().unwrap(),
        ]);
        command.env("CODEX_HOME", home);
        command.output().unwrap()
    }

    fn record(&self, observation: &str, reporter: &str, episode: &str, kind: &str) -> String {
        let out = self.feedback(&[
            "record",
            "--observation",
            observation,
            "--scope",
            "synthetic",
            "--reporter",
            reporter,
            "--episode",
            episode,
            "--kind",
            kind,
            "--parent",
            &self.feature,
        ]);
        let text = output_text(&out);
        assert!(out.status.success(), "{text}");
        let id = text
            .split_whitespace()
            .nth(2)
            .expect("recorded feedback id")
            .to_owned();
        assert!(id.starts_with("bdct-"), "{text}");
        id
    }

    fn decisions(&self, name: &str, entries: &[(&str, &str, Option<&str>)]) -> PathBuf {
        let path = self.root.join(name);
        let decisions: Vec<Value> = entries
            .iter()
            .map(|(feedback, kind, merge_into)| {
                json!({"feedback": feedback, "kind": kind, "merge_into": merge_into})
            })
            .collect();
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({"schema": 1, "decisions": decisions})).unwrap(),
        )
        .unwrap();
        path
    }

    fn item(&self, id: &str) -> Value {
        let shown = bd_json(&self.bd, &self.project, &["show", id, "--json"]);
        match shown {
            Value::Array(rows) => rows.into_iter().next().expect("one shown issue"),
            value => value,
        }
    }

    fn labels(&self, id: &str) -> Vec<String> {
        self.item(id)["labels"]
            .as_array()
            .map(|labels| {
                labels
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The observable board state a read-only verb must not change.
    fn board_state(&self, id: &str) -> String {
        let comments = bd_json(&self.bd, &self.project, &["comments", id, "--json"]);
        let texts: Vec<String> = comments
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row["text"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        format!(
            "status={} labels={:?} comments={texts:?}",
            self.item(id)["status"],
            self.labels(id)
        )
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

fn seed_git(project: &Path) {
    git(project, &["init", "-q"]);
    git(
        project,
        &["config", "user.email", "feedback-cli@example.test"],
    );
    git(project, &["config", "user.name", "Feedback CLI"]);
    fs::write(project.join("README.md"), "synthetic board fixture\n").unwrap();
    git(project, &["add", "README.md"]);
    git(project, &["commit", "-qm", "seed"]);
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

fn bd_run(bd: &Path, project: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bd)
        .args(args)
        .current_dir(project)
        .env("BD_NON_INTERACTIVE", "1")
        .env("BEADS_ACTOR", "feedback-cli-test")
        .output()
        .unwrap()
}

fn bd_json(bd: &Path, project: &Path, args: &[&str]) -> Value {
    let out = bd_run(bd, project, args);
    assert!(
        out.status.success(),
        "{}",
        bd_failed(args.first().copied().unwrap_or("bd"), &out)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn bd_failed(op: &str, out: &std::process::Output) -> String {
    format!(
        "bd {op} failed: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn read_only_verbs_leave_the_board_unchanged() {
    let board = Board::new("readonly");
    let item = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let before = board.board_state(&item);

    let listed = board.feedback(&["list"]);
    let text = output_text(&listed);
    assert!(listed.status.success(), "{text}");
    assert!(text.contains(&format!("{item} reporter=exec-a")), "{text}");
    assert!(text.contains("feedback: 1 open batch_limit=2"), "{text}");
    assert!(text.contains("limits=configured("), "{text}");

    let ledger = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&ledger);
    assert!(ledger.status.success(), "{text}");
    assert!(
        text.contains("votes=0 counted=0 merges=0 routes=0 promotions=0"),
        "{text}"
    );

    let candidates = board.feedback(&["candidates"]);
    let text = output_text(&candidates);
    assert!(candidates.status.success(), "{text}");
    assert!(
        text.contains("promotion candidates: 0 at threshold=2"),
        "{text}"
    );

    assert_eq!(board.board_state(&item), before, "reads must not mutate");
    board.drop();
}

#[test]
fn bounded_triage_counts_once_and_excludes_diagnostics() {
    let board = Board::new("triage");
    let first = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let second = board.record("same wait, second reporter", "exec-b", "e2", "executor");
    let diagnostic = board.record("automated probe finding", "ci", "e3", "diagnostic");
    let batch = board.decisions(
        "batch.json",
        &[
            (&first, "process", None),
            (&second, "process", Some(&first)),
            (&diagnostic, "tool", None),
        ],
    );

    let out = board.feedback(&["triage", "--decisions", batch.to_str().unwrap()]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "applied feedback {first} route=incubator target={first} vote=counted"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "applied feedback {second} route=incubator target={first} vote=counted"
        )),
        "{text}"
    );
    assert!(
        text.contains("deferred 1 decisions beyond feedback_batch_limit=2"),
        "{text}"
    );
    assert!(text.contains("triage complete: applied=2"), "{text}");

    // Repeating the same decisions adds no counted vote and no second merge.
    let out = board.feedback(&["triage", "--decisions", batch.to_str().unwrap()]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "applied feedback {first} route=incubator target={first} vote=repeat"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "applied feedback {second} route=incubator target={first} vote=repeat"
        )),
        "{text}"
    );

    let remaining = board.decisions("remaining.json", &[(&diagnostic, "tool", None)]);
    let out = board.feedback(&["triage", "--decisions", remaining.to_str().unwrap()]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "applied feedback {diagnostic} route=incubator target={diagnostic} vote=automated-diagnostic"
        )),
        "{text}"
    );

    // Two distinct counted votes, one merge, and the diagnostic never counts.
    let ledger = board.feedback(&["ledger", "--item", &first]);
    let text = output_text(&ledger);
    assert!(
        text.contains("votes=4 counted=2 merges=1 routes=2 promotions=0"),
        "{text}"
    );
    let ledger = board.feedback(&["ledger", "--item", &diagnostic]);
    let text = output_text(&ledger);
    assert!(text.contains("votes=1 counted=0"), "{text}");
    assert!(
        text.contains("counted=false reason=automated-diagnostic"),
        "{text}"
    );

    // The configured threshold 2 (not the kit default 3) makes the item
    // eligible, and the diagnostic stays out of the candidate set.
    let candidates = board.feedback(&["candidates"]);
    let text = output_text(&candidates);
    assert!(candidates.status.success(), "{text}");
    assert!(
        text.contains("promotion candidates: 1 at threshold=2"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "candidate {first} counted=2 threshold=2 kinds=process"
        )),
        "{text}"
    );
    assert!(!text.contains(&format!("candidate {diagnostic}")), "{text}");
    board.drop();
}

#[test]
fn partial_triage_failure_reports_the_applied_prefix_and_recovers() {
    let board = Board::new("partial");
    let first = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let decisions = board.decisions(
        "partial.json",
        &[
            (&first, "orchestration", None),
            ("bdct-absent.9", "process", None),
        ],
    );
    let out = board.feedback(&["triage", "--decisions", decisions.to_str().unwrap()]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains(&format!(
            "applied feedback {first} route=incubator target={first} vote=counted"
        )),
        "{text}"
    );
    assert!(
        text.contains("failed: feedback bdct-absent.9 kind=process"),
        "{text}"
    );
    assert!(
        text.contains("applied 1 decisions before the failure"),
        "{text}"
    );
    assert!(
        text.contains("rerun the same decisions to continue"),
        "{text}"
    );

    // Recovery reruns the same decisions: the applied action repeats without
    // another counted vote, and the failing action stays visible.
    let out = board.feedback(&["triage", "--decisions", decisions.to_str().unwrap()]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("vote=repeat"), "{text}");
    assert!(
        text.contains("failed: feedback bdct-absent.9 kind=process"),
        "{text}"
    );
    let ledger = board.feedback(&["ledger", "--item", &first]);
    let text = output_text(&ledger);
    assert!(text.contains("votes=2 counted=1"), "{text}");
    board.drop();
}

#[test]
fn promotion_is_explicit_recorded_and_never_repeated() {
    let board = Board::new("promote");
    let item = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let second = board.record("same wait, second reporter", "exec-b", "e2", "executor");
    let admitted = board.decisions(
        "admit.json",
        &[(&item, "process", None), (&second, "process", Some(&item))],
    );
    let out = board.feedback(&["triage", "--decisions", admitted.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));

    let out = board.feedback(&["promote", "--item", &item]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "promoted {item} route=backlog-task basis=votes counted=2 threshold=2 target=none"
        )),
        "{text}"
    );
    let labels = board.labels(&item);
    assert!(labels.iter().any(|label| label == "backlog"), "{labels:?}");
    assert!(
        !labels.iter().any(|label| label == "incubator"),
        "{labels:?}"
    );

    // A completed promotion is refused with the retained outcome and is not
    // written a second time.
    let out = board.feedback(&["promote", "--item", &item]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("already promoted"), "{text}");
    assert!(text.contains("route=backlog-task"), "{text}");
    let ledger = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&ledger);
    assert!(text.contains("promotions=1"), "{text}");
    board.drop();
}

/// The owning orchestration configuration is the installed kit, not the
/// consumer project: an ordinary command must pick up the installed custom
/// limits without being told where the kit lives, while an explicit --source
/// still overrides them.
#[test]
fn installed_kit_limits_apply_without_a_source_flag() {
    let board = Board::new("limits");
    // The consumer project carries no kit configuration of its own.
    fs::remove_file(board.project.join("global/orchestration.toml")).unwrap();
    let kit = board.root.join("kit");
    fs::create_dir_all(kit.join("global")).unwrap();
    fs::write(
        kit.join("global/orchestration.toml"),
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 5\nincubator_size_cap = 11\nfeedback_batch_limit = 7\n",
    )
    .unwrap();
    fs::create_dir_all(board.home.join("harness")).unwrap();
    fs::write(
        board.home.join("harness/installation.json"),
        serde_json::to_vec(&json!({"schemaVersion": 1, "sourceRoot": kit})).unwrap(),
    )
    .unwrap();

    let listed = board.feedback(&["list"]);
    let text = output_text(&listed);
    assert!(listed.status.success(), "{text}");
    assert!(text.contains("batch_limit=7"), "{text}");
    assert!(text.contains("limits=configured(installed kit "), "{text}");
    assert!(text.contains(&kit.display().to_string()), "{text}");

    let candidates = board.feedback(&["candidates"]);
    let text = output_text(&candidates);
    assert!(candidates.status.success(), "{text}");
    assert!(text.contains("at threshold=5"), "{text}");

    // An explicit --source overrides the installed kit.
    let other = board.root.join("other-kit");
    fs::create_dir_all(other.join("global")).unwrap();
    fs::write(
        other.join("global/orchestration.toml"),
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 2\nincubator_size_cap = 32\nfeedback_batch_limit = 3\n",
    )
    .unwrap();
    let listed = board.feedback(&["list", "--source", other.to_str().unwrap()]);
    let text = output_text(&listed);
    assert!(listed.status.success(), "{text}");
    assert!(text.contains("batch_limit=3"), "{text}");
    assert!(
        text.contains(&format!(
            "limits=configured({})",
            other.join("global/orchestration.toml").display()
        )),
        "{text}"
    );

    // No installed kit and no project configuration: the kit defaults are
    // stated instead of being silently assumed.
    let bare = board.root.join("bare-home");
    fs::create_dir_all(&bare).unwrap();
    let listed = board.feedback_with_home(&bare, &["list"]);
    let text = output_text(&listed);
    assert!(listed.status.success(), "{text}");
    assert!(text.contains("batch_limit=8"), "{text}");
    assert!(text.contains("limits=defaults("), "{text}");
    board.drop();
}

/// A requirement promotion enters the OpenSpec workflow's own change: the
/// command validates the intended change, records its reference and never
/// writes into openspec/, so an existing draft survives every retry.
#[test]
fn openspec_change_promotion_validates_the_change_and_preserves_its_draft() {
    let board = Board::new("openspec");
    let item = board.record("accepted behavior must change", "lead-1", "e1", "lead");
    let admitted = board.decisions("admit.json", &[(&item, "requirement", None)]);
    let out = board.feedback(&["triage", "--decisions", admitted.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));

    // Without an intended change the promotion records nothing and names the
    // OpenSpec route that creates it.
    let out = board.feedback(&["promote", "--item", &item, "--route", "openspec-change"]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("does not exist at"), "{text}");
    assert!(text.contains("openspec new change feedback-"), "{text}");
    assert!(text.contains("nothing was recorded"), "{text}");
    let ledger = board.feedback(&["ledger", "--item", &item]);
    assert!(
        output_text(&ledger).contains("promotions=0"),
        "{}",
        output_text(&ledger)
    );
    assert!(board.labels(&item).iter().any(|label| label == "incubator"));

    // The OpenSpec workflow created the intended change; the promotion records
    // its reference and leaves the draft exactly as it was.
    let change = board.project.join("openspec/changes/lead-intent");
    fs::create_dir_all(&change).unwrap();
    let draft = change.join("proposal.md");
    fs::write(&draft, "# Lead intent\n\n## Why\n\nsynthetic draft\n").unwrap();
    let draft_before = fs::read(&draft).unwrap();
    let out = board.feedback(&[
        "promote",
        "--item",
        &item,
        "--route",
        "openspec-change",
        "--openspec-change",
        "lead-intent",
        "--override-consequence",
        "accepted behavior would silently change",
        "--override-reason",
        "synthetic material evidence",
    ]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "promoted {item} route=openspec-change basis=override counted=1 threshold=none target=openspec:lead-intent"
        )),
        "{text}"
    );
    assert_eq!(fs::read(&draft).unwrap(), draft_before);
    let labels = board.labels(&item);
    assert!(labels.iter().any(|label| label == "openspec"), "{labels:?}");
    assert!(
        !labels.iter().any(|label| label == "incubator"),
        "{labels:?}"
    );

    // A retry preserves the draft, keeps one promotion record and reports the
    // retained outcome.
    let out = board.feedback(&[
        "promote",
        "--item",
        &item,
        "--route",
        "openspec-change",
        "--openspec-change",
        "lead-intent",
    ]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("already promoted"), "{text}");
    assert!(text.contains("openspec:lead-intent"), "{text}");
    assert_eq!(fs::read(&draft).unwrap(), draft_before);
    let ledger = board.feedback(&["ledger", "--item", &item]);
    assert!(
        output_text(&ledger).contains("promotions=1"),
        "{}",
        output_text(&ledger)
    );

    // The reference is refused with a route that is not the OpenSpec one.
    let out = board.feedback(&[
        "promote",
        "--item",
        &item,
        "--route",
        "backlog-task",
        "--openspec-change",
        "lead-intent",
    ]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("applies only to a promotion whose route is openspec-change"),
        "{text}"
    );
    board.drop();
}

#[test]
fn consequence_override_promotes_without_votes_and_kit_route_needs_its_wording() {
    let board = Board::new("override");
    let item = board.record("material data-loss finding", "lead-1", "e1", "lead");
    let admitted = board.decisions("admit.json", &[(&item, "material", None)]);
    let out = board.feedback(&["triage", "--decisions", admitted.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));

    // The kit route requires the sanitized kit-level wording and its board;
    // without them the command refuses before any board write.
    let out = board.feedback(&["promote", "--item", &item, "--route", "kit-backlog"]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("requires --kit-project"), "{text}");

    let out = board.feedback(&[
        "promote",
        "--item",
        &item,
        "--override-consequence",
        "silent data loss",
        "--override-reason",
        "reproduced twice in synthetic checks",
    ]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "promoted {item} route=backlog-task basis=override counted=1 threshold=none target=none"
        )),
        "{text}"
    );
    board.drop();
}
