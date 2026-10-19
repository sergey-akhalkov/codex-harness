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

const JUDGMENT_REASON: &str = "lasting-benefit:lower-future-review-burden";
const JUDGMENT_ACCOUNTING: &str = "check+coordination+rework";

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
        self.run_feedback(args, &[("CODEX_HOME", home)])
    }

    /// Runs one feedback verb with CODEX_HOME removed and a synthetic user
    /// profile, so the launcher's own `CODEX_HOME`, else `USERPROFILE\.codex`
    /// resolution is exercised instead of an explicit home.
    fn feedback_with_user_profile(&self, profile: &Path, args: &[&str]) -> std::process::Output {
        let mut command = self.feedback_command(args);
        command.env_remove("CODEX_HOME");
        command.env("USERPROFILE", profile);
        command.output().unwrap()
    }

    fn run_feedback(&self, args: &[&str], env: &[(&str, &Path)]) -> std::process::Output {
        let mut command = self.feedback_command(args);
        for &(name, value) in env {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    fn feedback_command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(manager());
        command.arg("feedback").args(args);
        command.args([
            "--bd",
            self.bd.to_str().unwrap(),
            "--project",
            self.project.to_str().unwrap(),
        ]);
        command
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

    fn record_derived(&self, source: &str, reporter: &str, analysis: &str) -> std::process::Output {
        self.feedback(&[
            "record",
            "--source-feedback",
            source,
            "--observation",
            analysis,
            "--reporter",
            reporter,
            "--kind",
            "derived-evidence",
        ])
    }

    /// Creates an old-style derived card directly on the board so source
    /// validation can prove it refuses a derived-of-derived link.
    fn legacy_derived(&self, reporter: &str, episode: &str) -> String {
        let description = format!(
            "observation: legacy derived analysis\nscope: synthetic\nreporter: {reporter}\nepisode: {episode}\nkind: derived-evidence\n"
        );
        let created = bd_json(
            &self.bd,
            &self.project,
            &[
                "create",
                "Feedback: legacy derived analysis",
                "--type",
                "task",
                "--labels",
                "feedback",
                "--parent",
                &self.feature,
                "--description",
                &description,
                "--json",
            ],
        );
        created["id"].as_str().unwrap().to_owned()
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

    /// Comment texts in board order: a completed retry must add nothing.
    fn comment_texts(&self, id: &str) -> Vec<String> {
        let comments = bd_json(&self.bd, &self.project, &["comments", id, "--json"]);
        comments
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row["text"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn feedback_ids(&self) -> Vec<String> {
        let listed = bd_json(
            &self.bd,
            &self.project,
            &[
                "list", "--label", "feedback", "--status", "open", "--json", "--brief",
            ],
        );
        listed
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn derived_comments(&self, id: &str) -> Vec<String> {
        self.comment_texts(id)
            .into_iter()
            .filter(|text| text.starts_with("feedback-derived v1"))
            .collect()
    }

    /// A board task outside the bounded feedback shape, so derived intake can
    /// prove it refuses an unrelated source before any write.
    fn plain_task(&self, title: &str) -> String {
        let created = bd_json(
            &self.bd,
            &self.project,
            &[
                "create",
                title,
                "--type",
                "task",
                "--parent",
                &self.feature,
                "--json",
            ],
        );
        created["id"].as_str().unwrap().to_owned()
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

/// Every file under a directory, recursively, for proving the main
/// specification tree stayed empty or unchanged.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    files
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

/// The current `harness/installation.json` shape: the owners live under
/// `settings` (sourceRoot, codexHome, userHome, dependencyUserHome,
/// codexCommand, pathScope, pathAdded, versions) beside `metadataIdentity`,
/// `links` and `checksum`. Limits discovery reads only `settings.sourceRoot`.
fn installation_record(source_root: &Path) -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "schemaVersion": 2,
        "settings": {
            "sourceRoot": source_root,
            "codexHome": source_root.join("synthetic-codex-home"),
            "userHome": source_root.join("synthetic-user-home"),
            "dependencyUserHome": source_root.join("synthetic-user-home"),
            "codexCommand": source_root.join("synthetic-codex.exe"),
            "pathScope": "User",
            "pathAdded": true,
            "versions": {},
        },
        "metadataIdentity": {
            "volume_serial_number": 1,
            "file_id": 2,
            "creation_time": 3,
        },
        "links": [],
        "checksum": "0".repeat(64),
    }))
    .unwrap()
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

/// Two friction episodes from one synthetic reporter stay distinct feedback
/// identities through recording, triage and replay. This exercises board
/// bookkeeping only; real managed-session continuity is evidenced separately
/// through board feedback, not by the Rust test process.
#[test]
fn same_reporter_episodes_keep_identity_through_triage_and_replay() {
    let board = Board::new("same-reporter-episodes");
    let reporter = "executor-a";

    let first = board.record(
        "executor dispatch waited after tool output",
        reporter,
        "dispatch-wait-1",
        "executor",
    );
    let second = board.record(
        "the same dispatch wait recurred on a second assignment",
        reporter,
        "dispatch-wait-2",
        "executor",
    );
    assert_ne!(first, second);

    let listed = output_text(&board.feedback(&["list"]));
    for (id, episode, observation) in [
        (
            &first,
            "dispatch-wait-1",
            "executor dispatch waited after tool output",
        ),
        (
            &second,
            "dispatch-wait-2",
            "the same dispatch wait recurred on a second assignment",
        ),
    ] {
        assert!(
            listed.contains(&format!(
                "{id} reporter={reporter} episode={episode} kind=executor"
            )),
            "{listed}"
        );
        let item = board.item(id);
        let description = item["description"]
            .as_str()
            .or_else(|| item["content"].as_str())
            .expect("feedback description");
        let reporter_line = format!("reporter: {reporter}");
        let episode_line = format!("episode: {episode}");
        let observation_line = format!("observation: {observation}");
        assert_eq!(
            description
                .lines()
                .filter(|line| *line == reporter_line.as_str())
                .count(),
            1,
            "{description}"
        );
        assert_eq!(
            description
                .lines()
                .filter(|line| *line == episode_line.as_str())
                .count(),
            1,
            "{description}"
        );
        assert!(
            description
                .lines()
                .any(|line| line == observation_line.as_str()),
            "{description}"
        );
    }

    // Both episodes count once before the lead merges the repeated friction;
    // replaying the accepted decisions remains idempotent.
    let decisions = board.decisions(
        "same-executor.json",
        &[
            (&first, "process", None),
            (&second, "process", Some(&first)),
        ],
    );
    let out = board.feedback(&["triage", "--decisions", decisions.to_str().unwrap()]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    for id in [&first, &second] {
        assert!(
            text.contains(&format!(
                "applied feedback {id} route=incubator target={first} vote=counted"
            )),
            "{text}"
        );
    }
    let replay = board.feedback(&["triage", "--decisions", decisions.to_str().unwrap()]);
    let text = output_text(&replay);
    assert!(replay.status.success(), "{text}");
    assert_eq!(text.matches("vote=repeat").count(), 2, "{text}");

    let ledger = output_text(&board.feedback(&["ledger", "--item", &first]));
    assert!(
        ledger.contains("votes=4 counted=2 merges=1 routes=2 promotions=0"),
        "{ledger}"
    );
    assert!(
        ledger.contains(&format!("merge from={second} into={first}")),
        "{ledger}"
    );
    let votes: Vec<&str> = ledger
        .lines()
        .filter(|line| line.starts_with("vote "))
        .collect();
    assert_eq!(votes.len(), 4, "{ledger}");
    for episode in ["dispatch-wait-1", "dispatch-wait-2"] {
        assert_eq!(
            votes
                .iter()
                .filter(|line| {
                    line.contains(&format!("episode={episode}"))
                        && line.contains(&format!("reporter={reporter}"))
                })
                .count(),
            2,
            "{ledger}"
        );
        assert_eq!(
            votes
                .iter()
                .filter(|line| {
                    line.contains(&format!("episode={episode}")) && line.contains("counted=true")
                })
                .count(),
            1,
            "{ledger}"
        );
    }
    assert_eq!(
        votes
            .iter()
            .filter(|line| line.contains("reason=repeat"))
            .count(),
        2,
        "{ledger}"
    );

    board.drop();
}

#[test]
fn derived_intake_links_the_source_without_a_vote_or_another_card() {
    let board = Board::new("derived-link");
    let source = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let cards = board.feedback_ids();

    // The analysis arrives before triage and must not become its own card or
    // vote. Source identity comes from the board record, not the CLI.
    let out = board.record_derived(&source, "reflection-a", "the wait follows each tool result");
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!("derived feedback source={source} target={source}")),
        "{text}"
    );
    assert!(text.contains("source_reporter=exec-a"), "{text}");
    assert!(text.contains("source_kind=executor"), "{text}");
    assert!(text.contains("episode=e1"), "{text}");
    assert!(
        text.contains(&format!("parent={}", board.feature)),
        "{text}"
    );
    assert!(text.contains("record=written votes=0"), "{text}");
    assert_eq!(board.feedback_ids(), cards, "derived intake created a card");

    // Identical replay is harmless and reports the recorded outcome.
    let out = board.record_derived(&source, "reflection-a", "the wait follows each tool result");
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("record=already-recorded votes=0"), "{text}");
    assert_eq!(board.derived_comments(&source).len(), 1, "{text}");

    // Triage counts the source once. A second reflection restating the same
    // episode retains analysis without adding a vote.
    let batch = board.decisions("source.json", &[(&source, "process", None)]);
    let out = board.feedback(&["triage", "--decisions", batch.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));
    let out = board.record_derived(
        &source,
        "reflection-b",
        "another restatement of the same episode",
    );
    assert!(out.status.success(), "{}", output_text(&out));
    let ledger = output_text(&board.feedback(&["ledger", "--item", &source]));
    assert!(ledger.contains("votes=1 counted=1"), "{ledger}");
    assert!(ledger.contains("derived=2"), "{ledger}");
    assert!(ledger.contains("reporter=reflection-a"), "{ledger}");
    assert!(ledger.contains("reporter=reflection-b"), "{ledger}");

    // A genuinely independent episode still accumulates its own vote.
    let independent = board.record("dispatch waits after tools too", "exec-b", "e2", "executor");
    let batch = board.decisions(
        "independent.json",
        &[(&independent, "process", Some(&source))],
    );
    let out = board.feedback(&["triage", "--decisions", batch.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));
    let ledger = output_text(&board.feedback(&["ledger", "--item", &source]));
    assert!(ledger.contains("votes=2 counted=2"), "{ledger}");

    // Replaying the analysis after triage still changes neither count.
    let out = board.record_derived(&source, "reflection-a", "the wait follows each tool result");
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("record=already-recorded"), "{text}");
    let ledger = output_text(&board.feedback(&["ledger", "--item", &source]));
    assert!(ledger.contains("votes=2 counted=2"), "{ledger}");
    assert!(ledger.contains("derived=2"), "{ledger}");
    board.drop();
}

#[test]
fn derived_intake_refuses_invalid_sources_before_any_board_write() {
    let board = Board::new("derived-refusal");
    let source = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let unrelated = board.plain_task("Feedback-like task outside the bounded shape");
    let legacy = board.legacy_derived("reflection-old", "e9");
    let cards = board.feedback_ids();
    let source_state = board.board_state(&source);

    // Missing source.
    let out = board.record_derived("bdct-absent.9", "reflection-a", "analysis of nothing");
    assert!(!out.status.success(), "{}", output_text(&out));

    // Existing but unrelated board item.
    let out = board.record_derived(&unrelated, "reflection-a", "analysis of a plain task");
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("not a bounded feedback record"), "{text}");

    // A legacy derived card cannot be restated into a second derived record.
    let out = board.record_derived(&legacy, "reflection-b", "analysis of an analysis");
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("itself derived evidence"), "{text}");

    // Caller-supplied source identity is rejected instead of trusted.
    let out = board.feedback(&[
        "record",
        "--source-feedback",
        &source,
        "--observation",
        "analysis",
        "--reporter",
        "reflection-a",
        "--episode",
        "spoofed",
        "--kind",
        "derived-evidence",
    ]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("omit --episode, --scope and --parent"),
        "{text}"
    );

    // `--source-feedback` is not an alternate identity for ordinary intake.
    let out = board.feedback(&[
        "record",
        "--source-feedback",
        &source,
        "--observation",
        "analysis",
        "--scope",
        "synthetic",
        "--reporter",
        "reflection-a",
        "--episode",
        "e1",
        "--kind",
        "executor",
        "--parent",
        &board.feature,
    ]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("--source-feedback is only valid with --kind derived-evidence"),
        "{text}"
    );

    // Derived intake without its source refused before any board effect.
    let out = board.feedback(&[
        "record",
        "--observation",
        "analysis",
        "--reporter",
        "reflection-a",
        "--kind",
        "derived-evidence",
    ]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("--source-feedback is required"), "{text}");

    assert_eq!(
        board.feedback_ids(),
        cards,
        "a refused intake created a card"
    );
    assert_eq!(board.board_state(&source), source_state);
    assert!(board.derived_comments(&unrelated).is_empty());
    assert!(board.derived_comments(&legacy).is_empty());
    board.drop();
}

#[test]
fn derived_provenance_survives_merge_and_partial_retry() {
    let board = Board::new("derived-merge");
    let canonical = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let incoming = board.record("the wait repeats under load", "exec-b", "e2", "executor");
    let out = board.record_derived(
        &incoming,
        "reflection-a",
        "analysis of the load-dependent wait",
    );
    assert!(out.status.success(), "{}", output_text(&out));
    let batch = board.decisions("canonical.json", &[(&canonical, "process", None)]);
    let out = board.feedback(&["triage", "--decisions", batch.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));

    // Partial batch: the merge applies and copies the derived comment before
    // the missing action fails.
    let partial = board.decisions(
        "partial.json",
        &[
            (&incoming, "process", Some(&canonical)),
            ("bdct-absent.9", "process", None),
        ],
    );
    let out = board.feedback(&["triage", "--decisions", partial.to_str().unwrap()]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains(&format!(
            "applied feedback {incoming} route=incubator target={canonical} vote=counted"
        )),
        "{text}"
    );
    assert!(
        text.contains("failed: feedback bdct-absent.9 kind=process"),
        "{text}"
    );
    let ledger = output_text(&board.feedback(&["ledger", "--item", &canonical]));
    assert!(
        ledger.contains("votes=2 counted=2 merges=1 routes=2"),
        "{ledger}"
    );
    assert!(ledger.contains("derived=1"), "{ledger}");
    assert!(
        ledger.contains(&format!(
            "derived source={incoming} source_reporter=exec-b source_kind=executor reporter=reflection-a episode=e2"
        )),
        "{ledger}"
    );
    assert_eq!(board.derived_comments(&canonical).len(), 1, "{ledger}");

    // Retry converges: no second merge, derived copy or counted episode.
    let out = board.feedback(&["triage", "--decisions", partial.to_str().unwrap()]);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("vote=repeat"), "{text}");
    let ledger = output_text(&board.feedback(&["ledger", "--item", &canonical]));
    assert!(
        ledger.contains("votes=3 counted=2 merges=1 routes=2"),
        "{ledger}"
    );
    assert!(ledger.contains("derived=1"), "{ledger}");
    assert_eq!(board.derived_comments(&canonical).len(), 1, "{ledger}");

    // Analysis of the already-merged source attaches to the canonical item
    // while retaining the original source identity.
    let out = board.record_derived(
        &incoming,
        "reflection-b",
        "a later analysis of the same episode",
    );
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!("source={incoming} target={canonical}")),
        "{text}"
    );
    let ledger = output_text(&board.feedback(&["ledger", "--item", &canonical]));
    assert!(ledger.contains("votes=3 counted=2"), "{ledger}");
    assert!(ledger.contains("derived=2"), "{ledger}");
    board.drop();
}

#[test]
fn derived_copy_recovers_an_interrupted_merge_without_duplicates() {
    let board = Board::new("derived-interrupted");
    let canonical = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let incoming = board.record("the same wait under load", "exec-b", "e2", "executor");
    let out = board.record_derived(
        &incoming,
        "reflection-a",
        "analysis retained before the merge",
    );
    assert!(out.status.success(), "{}", output_text(&out));
    let batch = board.decisions("canonical.json", &[(&canonical, "process", None)]);
    let out = board.feedback(&["triage", "--decisions", batch.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));

    // Simulate a run interrupted after `bd duplicate` and its merge comment
    // but before derived evidence was copied.
    let _ = bd_json(
        &board.bd,
        &board.project,
        &["duplicate", &incoming, "--of", &canonical, "--json"],
    );
    let _ = bd_json(
        &board.bd,
        &board.project,
        &[
            "comment",
            &canonical,
            "--json",
            &format!("feedback-merge v1 from={incoming} into={canonical}"),
        ],
    );
    assert!(board.derived_comments(&canonical).is_empty());

    // Retrying the same action recognizes the recorded merge and applies only
    // the missing derived comment plus the one distinct vote.
    let merge = board.decisions("merge.json", &[(&incoming, "process", Some(&canonical))]);
    let out = board.feedback(&["triage", "--decisions", merge.to_str().unwrap()]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    let ledger = output_text(&board.feedback(&["ledger", "--item", &canonical]));
    assert!(
        ledger.contains("votes=2 counted=2 merges=1 routes=2"),
        "{ledger}"
    );
    assert!(ledger.contains("derived=1"), "{ledger}");
    assert_eq!(board.derived_comments(&canonical).len(), 1, "{ledger}");

    // A second retry adds nothing.
    let out = board.feedback(&["triage", "--decisions", merge.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));
    assert!(
        output_text(&out).contains("vote=repeat"),
        "{}",
        output_text(&out)
    );
    assert_eq!(board.derived_comments(&canonical).len(), 1);
    let ledger = output_text(&board.feedback(&["ledger", "--item", &canonical]));
    assert!(ledger.contains("votes=3 counted=2"), "{ledger}");
    assert!(ledger.contains("derived=1"), "{ledger}");
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

fn accepted_reflection_work(
    board: &Board,
) -> (
    harness_core::delivery_reflection::ReflectionController,
    String,
    String,
) {
    use harness_core::delivery_reflection::{
        AcceptanceBinding, ActivationRequest, JournalPolicy, Purpose, ReflectionController,
        acceptance_metadata,
    };
    let controller = ReflectionController::new(&board.bd, &board.project, &board.home);
    controller
        .activate(&ActivationRequest {
            lead_session: "lead-fixture".into(),
            run: None,
            boundary: None,
            journal: JournalPolicy::EnableIfMissing,
            limits: None,
        })
        .unwrap();
    let task = bd_json(
        &board.bd,
        &board.project,
        &[
            "create",
            "Accepted ordinary task",
            "--type",
            "task",
            "--parent",
            &board.feature,
            "--json",
        ],
    );
    let task = task["id"].as_str().unwrap().to_owned();
    let binding = AcceptanceBinding::new(
        &task,
        "run-fixture",
        "assignment-fixture",
        "result-fixture",
        "revision-fixture",
        vec!["executor-fixture".into()],
        "lead-fixture",
        "feedback-cli-test",
        Purpose::Ordinary,
    );
    for (key, value) in acceptance_metadata(&binding).unwrap() {
        let pair = format!("{key}={value}");
        bd_json(
            &board.bd,
            &board.project,
            &["update", &task, "--set-metadata", &pair, "--json"],
        );
    }
    bd_json(
        &board.bd,
        &board.project,
        &["close", &task, "--reason", "accepted", "--json"],
    );
    let poll = controller.poll().unwrap();
    assert_eq!(poll.accepted, 1, "{poll:?}");
    (controller, task, binding.acceptance_id().unwrap())
}

fn verified_promotion_command(
    board: &Board,
    item: &str,
    work: &str,
    lead: Option<&str>,
) -> Command {
    let mut command = board.feedback_command(&[
        "promote",
        "--item",
        item,
        "--verified-work",
        work,
        "--improvement-reason",
        "remove the verified redundant wait",
    ]);
    command.env("CODEX_HOME", &board.home);
    match lead {
        Some(lead) => {
            command.env("CODEX_THREAD_ID", lead);
        }
        None => {
            command.env_remove("CODEX_THREAD_ID");
        }
    }
    command
}

#[test]
fn verified_improvement_uses_one_episode_and_repairs_partial_promotion_without_votes() {
    let board = Board::new("verified-promotion");
    let item = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let decisions = board.decisions("admit.json", &[(&item, "process", None)]);
    let out = board.feedback(&["triage", "--decisions", decisions.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));
    let out = board.record_derived(&item, "reflection-a", "the same wait can be removed");
    assert!(out.status.success(), "{}", output_text(&out));
    let ordinary = board.feedback(&["promote", "--item", &item]);
    assert!(!ordinary.status.success());
    assert!(output_text(&ordinary).contains("below the configured threshold"));

    let (_controller, _task, work) = accepted_reflection_work(&board);
    let out = verified_promotion_command(&board, &item, &work, Some("lead-fixture"))
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("basis=verified-improvement counted=1 threshold=none"),
        "{text}"
    );
    let ledger = output_text(&board.feedback(&["ledger", "--item", &item]));
    assert!(ledger.contains("votes=1 counted=1"), "{ledger}");
    assert!(ledger.contains("promotions=1 derived=1"), "{ledger}");
    assert!(ledger.contains("basis=verified-improvement"), "{ledger}");
    let shown = bd_json(&board.bd, &board.project, &["show", &item, "--json"]);
    assert_eq!(shown[0]["metadata"]["harness.purpose"], "improvement");
    let comments = board.comment_texts(&item);

    // Reconstruct an interrupted post-record transition using the actual board.
    bd_json(
        &board.bd,
        &board.project,
        &["label", "add", &item, "incubator", "--json"],
    );
    bd_json(
        &board.bd,
        &board.project,
        &["label", "remove", &item, "backlog", "--json"],
    );
    bd_json(
        &board.bd,
        &board.project,
        &[
            "update",
            &item,
            "--set-metadata",
            "harness.purpose=ordinary",
            "--json",
        ],
    );
    let out = verified_promotion_command(&board, &item, &work, Some("lead-fixture"))
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("promotion already recorded"), "{text}");
    assert!(text.contains("basis=verified-improvement"), "{text}");
    assert_eq!(comments, board.comment_texts(&item));
    let shown = bd_json(&board.bd, &board.project, &["show", &item, "--json"]);
    assert_eq!(shown[0]["metadata"]["harness.purpose"], "improvement");
    let labels = board.labels(&item);
    assert!(labels.iter().any(|label| label == "backlog"));
    assert!(!labels.iter().any(|label| label == "incubator"));
    board.drop();
}

#[test]
fn verified_improvement_kit_route_keeps_private_provenance_out_of_public_writes() {
    let board = Board::new("verified-consumer");
    let kit = Board::new("verified-kit");
    let item = board.record(
        "private-fixture observation",
        "private-fixture-reporter",
        "private-fixture-episode",
        "executor",
    );
    let decisions = board.decisions("admit.json", &[(&item, "kit-concern", None)]);
    let out = board.feedback(&["triage", "--decisions", decisions.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));
    let (_controller, _task, work) = accepted_reflection_work(&board);
    let out = bd_run(
        &kit.bd,
        &kit.project,
        &["config", "set", "events-journal", "true"],
    );
    assert!(out.status.success(), "{}", output_text(&out));

    let promote = || {
        verified_promotion_command(&board, &item, &work, Some("lead-fixture"))
            .args([
                "--route",
                "kit-backlog",
                "--kit-project",
                kit.project.to_str().unwrap(),
                "--summary",
                "Remove redundant dispatch wait",
                "--scope",
                "executor dispatch",
            ])
            .env("BEADS_ACTOR", "private-fixture-actor")
            .output()
            .unwrap()
    };
    let out = promote();
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("basis=verified-improvement counted=1 threshold=none"),
        "{text}"
    );
    let items = bd_json(
        &kit.bd,
        &kit.project,
        &["list", "--label", "kit-feedback", "--json"],
    );
    let rows = items.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{items}");
    let kit_id = rows[0]["id"].as_str().unwrap();
    let shown = bd_json(&kit.bd, &kit.project, &["show", kit_id, "--json"]);
    assert_eq!(shown[0]["metadata"]["harness.purpose"], "improvement");
    let journal = bd_run(&kit.bd, &kit.project, &["events", "export"]);
    assert!(journal.status.success(), "{}", output_text(&journal));
    let public = format!("{shown}\n{}", String::from_utf8_lossy(&journal.stdout));
    assert!(public.contains("feedback-routing"), "{public}");
    for private in [
        "private-fixture",
        "lead-fixture",
        "executor-fixture",
        "remove the verified redundant wait",
        &work,
        &item,
        board.project.to_str().unwrap(),
    ] {
        assert!(
            !public.contains(private),
            "public board contains {private}: {public}"
        );
    }
    let source_comments = board.comment_texts(&item);
    assert!(source_comments.iter().any(|text| {
        text.contains("basis=verified-improvement")
            && text.contains("lead-fixture")
            && text.contains(&work)
    }));
    let out = promote();
    assert!(out.status.success(), "{}", output_text(&out));
    assert!(output_text(&out).contains("promotion already recorded"));
    assert_eq!(board.comment_texts(&item), source_comments);
    let after = bd_json(
        &kit.bd,
        &kit.project,
        &["list", "--label", "kit-feedback", "--json"],
    );
    assert_eq!(after, items);
    let replay_journal = bd_run(&kit.bd, &kit.project, &["events", "export"]);
    assert!(
        replay_journal.status.success(),
        "{}",
        output_text(&replay_journal)
    );
    assert_eq!(journal.stdout, replay_journal.stdout);
    kit.drop();
    board.drop();
}

#[test]
fn verified_improvement_refuses_inactive_wrong_lead_unrelated_and_stale_work_before_writes() {
    let board = Board::new("verified-refusals");
    let item = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let decisions = board.decisions("admit.json", &[(&item, "process", None)]);
    let out = board.feedback(&["triage", "--decisions", decisions.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));
    let before = board.comment_texts(&item);
    let out = verified_promotion_command(&board, &item, "missing-work", Some("lead-fixture"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(output_text(&out).contains("no delivery-reflection lifecycle"));
    assert_eq!(before, board.comment_texts(&item));

    let (controller, task, work) = accepted_reflection_work(&board);
    for (lead, requested_work, expected) in [
        (None, work.as_str(), "current real lead"),
        (Some("executor-fixture"), work.as_str(), "active lead"),
        (
            Some("lead-fixture"),
            "unrelated-work",
            "matches no retained acceptance",
        ),
    ] {
        let out = verified_promotion_command(&board, &item, requested_work, lead)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{}", output_text(&out));
        assert!(
            output_text(&out).contains(expected),
            "{}",
            output_text(&out)
        );
        assert_eq!(before, board.comment_texts(&item));
    }

    bd_json(&board.bd, &board.project, &["reopen", &task, "--json"]);
    let out = verified_promotion_command(&board, &item, &work, Some("lead-fixture"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        output_text(&out).contains("closed"),
        "{}",
        output_text(&out)
    );
    assert_eq!(before, board.comment_texts(&item));
    bd_json(
        &board.bd,
        &board.project,
        &["close", &task, "--reason", "accepted", "--json"],
    );

    let out = verified_promotion_command(&board, &item, &work, Some("lead-fixture"))
        .args([
            "--override-consequence",
            "critical",
            "--override-reason",
            "mixed basis",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(output_text(&out).contains("cannot be combined"));
    assert_eq!(before, board.comment_texts(&item));

    controller.stop().unwrap();
    let out = verified_promotion_command(&board, &item, &work, Some("lead-fixture"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(output_text(&out).contains("stopped"));
    assert_eq!(before, board.comment_texts(&item));
    assert!(board.labels(&item).iter().any(|label| label == "incubator"));
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

    // The second same-route invocation succeeds and confirms the recorded
    // outcome: no second promotion record and no comment changes.
    let comments_before = board.comment_texts(&item);
    let out = board.feedback(&["promote", "--item", &item]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "promotion already recorded: {item} route=backlog-task basis=votes counted=2 threshold=2 target=none"
        )),
        "{text}"
    );
    assert_eq!(board.comment_texts(&item), comments_before);
    let ledger = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&ledger);
    assert!(text.contains("promotions=1"), "{text}");
    board.drop();
}

/// Two identical successful promotions: the completed retry confirms the
/// recorded outcome with exactly one promotion record and no comment writes.
#[test]
fn completed_promotion_retry_confirms_without_writes() {
    let board = Board::new("retry");
    let item = board.record("dispatch waits after tools", "exec-a", "e1", "executor");
    let second = board.record("same wait, second reporter", "exec-b", "e2", "executor");
    let admitted = board.decisions(
        "admit.json",
        &[(&item, "process", None), (&second, "process", Some(&item))],
    );
    let out = board.feedback(&["triage", "--decisions", admitted.to_str().unwrap()]);
    assert!(out.status.success(), "{}", output_text(&out));

    let first_run = board.feedback(&["promote", "--item", &item]);
    let text = output_text(&first_run);
    assert!(first_run.status.success(), "{text}");
    assert!(text.contains("promoted "), "{text}");
    let comments_after_promotion = board.comment_texts(&item);

    let retry = board.feedback(&["promote", "--item", &item]);
    let text = output_text(&retry);
    assert!(retry.status.success(), "{text}");
    assert!(text.contains("promotion already recorded: "), "{text}");
    assert_eq!(board.comment_texts(&item), comments_after_promotion);

    let ledger = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&ledger);
    assert!(text.contains("promotions=1"), "{text}");
    assert!(text.contains("counted=2"), "{text}");
    let labels = board.labels(&item);
    assert!(labels.iter().any(|label| label == "backlog"), "{labels:?}");
    assert!(
        !labels.iter().any(|label| label == "incubator"),
        "{labels:?}"
    );
    board.drop();
}

/// Ordinary CLI users may leave CODEX_HOME unset and rely on
/// `USERPROFILE\.codex`, so the installed kit limits must still apply.
#[test]
fn installed_kit_limits_apply_without_codex_home() {
    let board = Board::new("user-profile");
    fs::remove_file(board.project.join("global/orchestration.toml")).unwrap();
    let kit = board.root.join("kit");
    fs::create_dir_all(kit.join("global")).unwrap();
    fs::write(
        kit.join("global/orchestration.toml"),
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 1\nvote_threshold = 5\nincubator_size_cap = 11\nfeedback_batch_limit = 7\n",
    )
    .unwrap();
    let profile = board.root.join("profile");
    fs::create_dir_all(profile.join(".codex/harness")).unwrap();
    fs::write(
        profile.join(".codex/harness/installation.json"),
        installation_record(&kit),
    )
    .unwrap();

    let listed = board.feedback_with_user_profile(&profile, &["list"]);
    let text = output_text(&listed);
    assert!(listed.status.success(), "{text}");
    assert!(text.contains("batch_limit=7"), "{text}");
    assert!(text.contains("limits=configured(installed kit "), "{text}");
    assert!(text.contains(&kit.display().to_string()), "{text}");

    let candidates = board.feedback_with_user_profile(&profile, &["candidates"]);
    let text = output_text(&candidates);
    assert!(candidates.status.success(), "{text}");
    assert!(text.contains("at threshold=5"), "{text}");

    // A profile without an installation record states the defaults.
    let bare = board.root.join("bare-profile");
    fs::create_dir_all(bare.join(".codex")).unwrap();
    let listed = board.feedback_with_user_profile(&bare, &["list"]);
    let text = output_text(&listed);
    assert!(listed.status.success(), "{text}");
    assert!(text.contains("batch_limit=8"), "{text}");
    assert!(text.contains("limits=defaults("), "{text}");
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
        installation_record(&kit),
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

    // The legacy top-level `sourceRoot` record is still read while an
    // installation migrates, so the supported fallback is not lost.
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

    // A retry confirms the recorded outcome, preserves the draft, keeps one
    // promotion record and adds no comment.
    let comments_before = board.comment_texts(&item);
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
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "promotion already recorded: {item} route=openspec-change basis=override counted=1 threshold=none target=openspec:lead-intent"
        )),
        "{text}"
    );
    assert_eq!(board.comment_texts(&item), comments_before);
    assert_eq!(fs::read(&draft).unwrap(), draft_before);
    let ledger = board.feedback(&["ledger", "--item", &item]);
    assert!(
        output_text(&ledger).contains("promotions=1"),
        "{}",
        output_text(&ledger)
    );

    // A different intended change is a different consequence: the completed
    // retry stays refused and names the recorded target.
    let other = board.project.join("openspec/changes/other-intent");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("proposal.md"), "# Other intent\n").unwrap();
    let out = board.feedback(&[
        "promote",
        "--item",
        &item,
        "--route",
        "openspec-change",
        "--openspec-change",
        "other-intent",
    ]);
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("already promoted"), "{text}");
    assert!(text.contains("openspec:lead-intent"), "{text}");
    assert!(text.contains("other-intent"), "{text}");
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

/// One native board comment, in the exact form the board-workflow skill
/// records pacing and benefit-gate lines.
fn comment(board: &Board, item: &str, text: &str) {
    let out = bd_run(
        &board.bd,
        &board.project,
        &["comment", item, "--json", text],
    );
    assert!(out.status.success(), "{}", bd_failed("comment", &out));
}

#[test]
fn ledger_reports_fresh_pacing_records_and_the_gate_default() {
    let board = Board::new("ledger-pacing");
    let item = board.record("pacing for the stage", "lead-1", "e5", "lead");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    comment(
        &board,
        &item,
        &format!(
            "pacing-observation v1 scope=gpt source=dashboard-snapshot used=93 resets_at={} window_minutes=10080 refusals=1 observed_at={now} max_age=3600",
            now + 1800
        ),
    );
    comment(
        &board,
        &item,
        "pacing-decision v1 id=gpt:concurrency scope=gpt knob=concurrency from=4 to=1 expires_at=none reason=pressure basis=dashboard",
    );
    comment(
        &board,
        &item,
        "pacing-decision v1 id=gpt:effort scope=gpt knob=effort from=xhigh to=high expires_at=none reason=pressure basis=dashboard",
    );
    comment(
        &board,
        &item,
        "pacing-revoke v1 id=gpt:effort reason=superseded",
    );
    comment(
        &board,
        &item,
        &format!(
            "benefit-gate v1 item={item} improvement=lane-reuse outcome=reject quality=regressed matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=140.0 regression_percent=40.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=regressed"
        ),
    );
    comment(
        &board,
        &item,
        &format!(
            "benefit-gate v1 item={item} improvement=lane-reuse outcome=adopt quality=unchanged matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=99.0 regression_percent=-1.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=adopted"
        ),
    );

    let out = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("pacing observation gpt: 93% used resets_at="),
        "{text}"
    );
    assert!(text.contains("refusals=1"), "{text}");
    assert!(
        text.contains("pacing decisions: 1 applicable of 2 recorded; 1 revoked"),
        "{text}"
    );
    assert!(
        text.contains("pacing decision pacing-decision v1 id=gpt:concurrency"),
        "{text}"
    );
    assert!(!text.contains("id=gpt:effort"), "{text}");
    assert!(
        text.contains(&format!(
            "benefit gate {item}: recorded outcome=adopt quality=unchanged across 2 attributable record(s)"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit gate {item}: supported=yes (recorded comparison consistent within its declared tolerance) default=adopted"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("benefit gate {item}: limitations: none")),
        "{text}"
    );
    assert!(
        text.contains("evidence: the recorded board comment only; the comparison has not been rerun or independently certified here"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit-gate record item={item} outcome=reject quality=regressed"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit-gate record item={item} outcome=adopt quality=unchanged"
        )),
        "{text}"
    );
    board.drop();
}

#[test]
fn ledger_keeps_unknown_and_stale_pacing_evidence_honest() {
    let board = Board::new("ledger-unknown");
    let item = board.record("unknown telemetry", "lead-1", "e6", "lead");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    comment(
        &board,
        &item,
        &format!(
            "pacing-observation v1 scope=gpt source=dashboard-snapshot used=unknown resets_at=unknown window_minutes=10080 refusals=0 observed_at={now} max_age=3600"
        ),
    );
    comment(
        &board,
        &item,
        &format!(
            "pacing-observation v1 scope=gpt source=dashboard-snapshot used=50 resets_at=unknown window_minutes=10080 refusals=0 observed_at={} max_age=3600",
            now - 7200
        ),
    );

    let out = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("pacing observation gpt: used unknown"),
        "{text}"
    );
    assert!(!text.contains("50% used"), "{text}");
    assert!(text.contains("stale_ignored=1"), "{text}");
    assert!(
        text.contains("pacing decisions: 0 applicable of 0 recorded; 0 revoked"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit gate {item}: no comparison recorded; unadopted"
        )),
        "{text}"
    );
    board.drop();
}

#[test]
fn ledger_reports_gate_records_and_the_latest_outcome() {
    let board = Board::new("ledger-gate");
    let item = board.record("promoted improvement", "lead-1", "e7", "lead");
    comment(
        &board,
        &item,
        &format!(
            "benefit-gate v1 item={item} improvement=shorten-help outcome=adopt quality=unchanged matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=99.0 regression_percent=-1.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=adopted"
        ),
    );
    comment(
        &board,
        &item,
        &format!(
            "benefit-gate v1 item={item} improvement=shorten-help outcome=inconclusive quality=unmeasurable matched=1 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=100.0 regression_percent=0.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=retest_unresolved"
        ),
    );

    let out = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "benefit gate {item}: recorded outcome=inconclusive quality=unmeasurable across 2 attributable record(s)"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit gate {item}: supported=no (the latest record is not an adoption decision) default=unadopted"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("benefit gate {item}: limitations: none")),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit-gate record item={item} outcome=adopt quality=unchanged"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit-gate record item={item} outcome=inconclusive quality=unmeasurable"
        )),
        "{text}"
    );
    assert!(
        !text.contains(&format!("benefit gate {item}: supported=yes")),
        "{text}"
    );
    assert!(
        text.contains("pacing observations: none recorded; telemetry unknown"),
        "{text}"
    );
    board.drop();
}

/// An adoption that predates the comparison fields, and a newer adoption that
/// contradicts its own measured quality, stay visible with their limitations:
/// the reader never fabricates missing values and never revives an earlier
/// adoption that a newer attributable record has superseded.
#[test]
fn ledger_keeps_incomplete_and_contradictory_adoptions_unproven() {
    let board = Board::new("ledger-unsupported");
    let item = board.record("legacy adoption", "lead-1", "e8", "lead");
    comment(
        &board,
        &item,
        &format!(
            "benefit-gate v1 item={item} improvement=legacy outcome=adopt quality=unchanged matched=2"
        ),
    );

    let out = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "benefit gate {item}: recorded outcome=adopt quality=unchanged across 1 attributable record(s)"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit gate {item}: supported=no (recorded comparison cannot support an adoption) default=unadopted"
        )),
        "{text}"
    );
    assert!(
        text.contains("tolerance_percent missing or not a finite non-negative number"),
        "{text}"
    );
    assert!(text.contains("accounting basis missing"), "{text}");
    assert!(!text.contains("default=adopted"), "{text}");

    // A newer complete record that adopts despite regressed quality must not
    // be presented as supported either.
    comment(
        &board,
        &item,
        &format!(
            "benefit-gate v1 item={item} improvement=legacy outcome=adopt quality=regressed matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=140.0 regression_percent=40.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=regressed"
        ),
    );
    let out = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "benefit gate {item}: recorded outcome=adopt quality=regressed across 2 attributable record(s)"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit gate {item}: supported=no (recorded comparison cannot support an adoption) default=unadopted"
        )),
        "{text}"
    );
    assert!(
        text.contains("limitations: quality regressed; regression beyond the declared tolerance"),
        "{text}"
    );
    assert!(!text.contains("default=adopted"), "{text}");

    // A newer malformed record stays exposed: it supersedes the earlier
    // adoption without silently restoring it, and the history remains.
    comment(
        &board,
        &item,
        &format!("benefit-gate v1 item={item} outcome=adopt"),
    );
    let out = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "benefit gate {item}: recorded outcome=adopt quality=absent across 3 attributable record(s)"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit gate {item}: supported=no (recorded comparison cannot support an adoption) default=unadopted"
        )),
        "{text}"
    );
    assert!(text.contains("limitations: no quality recorded;"), "{text}");
    assert!(!text.contains("default=adopted"), "{text}");
    assert!(
        text.contains(&format!(
            "benefit-gate record item={item} outcome=adopt quality=absent"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit-gate record item={item} outcome=adopt quality=regressed"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit-gate record item={item} outcome=adopt quality=unchanged"
        )),
        "{text}"
    );
    board.drop();
}

/// A later rejection supersedes a supported adoption: the item is no longer
/// reported as adopted, and the recorded history stays available.
#[test]
fn ledger_reports_a_withdrawal_after_a_supported_adoption() {
    let board = Board::new("ledger-withdraw");
    let item = board.record("withdrawn improvement", "lead-1", "e9", "lead");
    comment(
        &board,
        &item,
        &format!(
            "benefit-gate v1 item={item} improvement=lane-reuse outcome=adopt quality=improved matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=95.0 regression_percent=-5.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=adopted"
        ),
    );

    let out = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "benefit gate {item}: supported=yes (recorded comparison consistent within its declared tolerance) default=adopted"
        )),
        "{text}"
    );

    comment(
        &board,
        &item,
        &format!(
            "benefit-gate v1 item={item} improvement=lane-reuse outcome=reject quality=regressed matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=120.0 regression_percent=20.0 baseline=direct candidate=lane accounting=check+coordination+rework detail=rejected"
        ),
    );
    let out = board.feedback(&["ledger", "--item", &item]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "benefit gate {item}: recorded outcome=reject quality=regressed across 2 attributable record(s)"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit gate {item}: supported=no (the latest record is not an adoption decision) default=unadopted"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("benefit gate {item}: limitations: none")),
        "{text}"
    );
    assert!(!text.contains("default=adopted"), "{text}");
    assert!(
        text.contains(&format!(
            "benefit-gate record item={item} outcome=adopt quality=improved"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit-gate record item={item} outcome=reject quality=regressed"
        )),
        "{text}"
    );
    board.drop();
}

/// Admits one hypothesis through the real entry point and returns its card id.
fn admit(board: &Board, mechanism: &str, conditions: &str, basis: &str) -> String {
    admit_with(board, mechanism, conditions, basis, None)
}

/// The same, optionally naming a fresh evidential basis for reconsideration.
fn admit_with(
    board: &Board,
    mechanism: &str,
    conditions: &str,
    basis: &str,
    fresh_basis: Option<&str>,
) -> String {
    let mut args = vec![
        "hypothesis-admit",
        "--mechanism",
        mechanism,
        "--conditions",
        conditions,
        "--observation",
        "ev-observation",
        "--predicted",
        "less repeated work",
        "--counterexample",
        "the burden does not recover",
        "--acceptance",
        "the declared check passes",
        "--spec",
        "openspec/changes/demo",
        "--basis",
        basis,
    ];
    if let Some(fresh) = fresh_basis {
        args.push("--fresh-basis");
        args.push(fresh);
    }
    let out = board.feedback(&args);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    let id = text
        .split_whitespace()
        .nth(1)
        .expect("hypothesis id")
        .to_owned();
    assert!(id.starts_with("bdct-"), "{text}");
    id
}

/// Publishes one evidence-bound decision through the real entry point.
#[allow(clippy::too_many_arguments)]
fn decision(
    board: &Board,
    item: &str,
    experiment: &str,
    outcome: &str,
    quality: &str,
    candidate_seconds: &str,
    close: bool,
    defer: Option<&str>,
) -> std::process::Output {
    let args = decision_arguments(
        item,
        experiment,
        outcome,
        quality,
        candidate_seconds,
        close,
        defer,
    );
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    board.feedback(&args)
}

/// The same published decision with one named binding emptied, to prove the
/// real entry point refuses a decision that omits it.
fn decision_with_absent_binding(
    board: &Board,
    item: &str,
    experiment: &str,
    flag: &str,
) -> std::process::Output {
    let mut args = decision_arguments(item, experiment, "adopt", "unchanged", "96", false, None);
    let position = args
        .iter()
        .position(|value| value == flag)
        .expect("the binding flag is part of every decision");
    args[position + 1] = String::new();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    board.feedback(&args)
}

/// The argument vector behind [`decision`].
fn decision_arguments(
    item: &str,
    experiment: &str,
    outcome: &str,
    quality: &str,
    candidate_seconds: &str,
    close: bool,
    defer: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = [
        "hypothesis-decision",
        "--item",
        item,
        "--experiment",
        experiment,
        "--outcome",
        outcome,
        "--quality",
        quality,
        "--matched",
        "2",
        "--tolerance-percent",
        "10",
        "--baseline-seconds",
        "100",
        "--candidate-seconds",
        candidate_seconds,
        "--baseline-arm",
        "direct",
        "--candidate-arm",
        "lane",
        "--accounting",
        "check+rework",
        "--baseline-revision",
        "base001",
        "--candidate-revision",
        "cand002",
        "--acceptance",
        "evidence-9",
        "--coverage",
        "time+rounds",
        "--scope",
        "task:synthetic",
        "--reason",
        "lane-contention",
        "--detail",
        "paired synthetic task",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if close {
        args.push("--close".to_owned());
        args.push("yes".to_owned());
    }
    if let Some(until) = defer {
        args.push("--defer".to_owned());
        args.push(until.to_owned());
    }
    args
}

/// Publishes the same evidence-bound adoption under the explicit
/// engineering-judgment basis. The candidate is slower than the baseline:
/// the measured added cost must stay in the record rather than being hidden
/// behind a fabricated saving.
fn judgment_decision(
    board: &Board,
    item: &str,
    experiment: &str,
    basis_ref: Option<&str>,
) -> std::process::Output {
    judgment_decision_with_costs(
        board,
        item,
        experiment,
        basis_ref,
        ("base001", "cand002"),
        ("1000", "1100"),
    )
}

/// A judgment attempt after one adopted change: the local comparison must start
/// from the retained candidate revision and cost, so validation reaches the
/// behavior under test instead of failing as a stale baseline.
fn judgment_decision_after_adoption(
    board: &Board,
    item: &str,
    experiment: &str,
    basis_ref: Option<&str>,
) -> std::process::Output {
    judgment_decision_with_costs(
        board,
        item,
        experiment,
        basis_ref,
        ("cand002", "cand003"),
        ("1100", "1200"),
    )
}

fn judgment_decision_with_costs(
    board: &Board,
    item: &str,
    experiment: &str,
    basis_ref: Option<&str>,
    (baseline_revision, candidate_revision): (&str, &str),
    (baseline_tokens, candidate_tokens): (&str, &str),
) -> std::process::Output {
    let mut args = decision_arguments(item, experiment, "adopt", "unchanged", "115", false, None);
    replace_argument(&mut args, "--baseline-revision", baseline_revision);
    replace_argument(&mut args, "--candidate-revision", candidate_revision);
    let reason = args
        .iter()
        .position(|value| value == "--reason")
        .expect("--reason is part of every decision");
    args[reason + 1] = JUDGMENT_REASON.to_owned();
    let accounting = args
        .iter()
        .position(|value| value == "--accounting")
        .expect("--accounting is part of every decision");
    args[accounting + 1] = JUDGMENT_ACCOUNTING.to_owned();
    let detail = args
        .iter()
        .position(|value| value == "--detail")
        .expect("--detail is part of every decision");
    args[detail + 1] =
        "lead judged retained added cost against lower future review burden".to_owned();
    args.push("--basis".to_owned());
    args.push("engineering-judgment".to_owned());
    if let Some(reference) = basis_ref {
        args.push("--basis-ref".to_owned());
        args.push(reference.to_owned());
    }
    args.extend(judgment_token_arguments(board));
    replace_argument(&mut args, "--baseline-tokens", baseline_tokens);
    replace_argument(&mut args, "--candidate-tokens", candidate_tokens);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    board.feedback(&args)
}

/// Complete synthetic token-cost and observed payback input for one
/// engineering-judgment publication. The candidate spends more tokens and the
/// predeclared return is carried by concrete quality and incident measures in
/// the existing `PaybackRecord` file shape. Cumulative totals are deliberately
/// absent: the publication owner derives them from the retained board history.
fn judgment_token_arguments(board: &Board) -> Vec<String> {
    let payback = payback_file(
        board,
        "judgment-payback.json",
        "fewer-future-incidents",
        json!([
            {
                "measure": "quality_score",
                "unit": "points",
                "direction": "higher-is-better",
                "baseline": 80.0,
                "candidate": 90.0,
                "evidence": "evidence-9"
            },
            {
                "measure": "incidents",
                "unit": "count",
                "direction": "lower-is-better",
                "baseline": 3.0,
                "candidate": 1.0,
                "evidence": "evidence-9"
            }
        ]),
    );
    [
        "--baseline-tokens",
        "1000",
        "--candidate-tokens",
        "1100",
        "--payback-file",
        payback.to_str().expect("payback file path"),
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// Writes one bounded payback record file in the existing public shape.
fn payback_file(board: &Board, name: &str, mechanism: &str, measures: Value) -> PathBuf {
    let path = board.root.join(name);
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "mechanism": mechanism,
            "measures": measures,
            "horizonTasks": 5.0,
        }))
        .expect("payback record JSON"),
    )
    .expect("write payback record");
    path
}

fn replace_argument(args: &mut [String], flag: &str, value: &str) {
    let position = args
        .iter()
        .position(|argument| argument == flag)
        .unwrap_or_else(|| panic!("{flag} is part of the decision fixture"));
    args[position + 1] = value.to_owned();
}

fn push_argument(args: &mut Vec<String>, flag: &str, value: &str) {
    args.push(flag.to_owned());
    args.push(value.to_owned());
}

/// Records one scoped user removal decision through the real entry point.
fn removal_decision(
    board: &Board,
    item: &str,
    decision: &str,
    actions: Option<&str>,
    loss: Option<&str>,
    basis: Option<&str>,
    detail: &str,
) -> std::process::Output {
    let mut args = vec![
        "removal-decide",
        "--item",
        item,
        "--decision",
        decision,
        "--proposal",
        "openspec/changes/remove-x",
        "--target",
        "skill-x",
        "--detail",
        detail,
    ];
    if let Some(loss) = loss {
        args.push("--loss");
        args.push(loss);
    }
    if let Some(actions) = actions {
        args.push("--actions");
        args.push(actions);
    }
    if let Some(basis) = basis {
        args.push("--basis");
        args.push(basis);
    }
    board.feedback(&args)
}

/// Resolves one exact removal scope through the real entry point.
fn removal_check(board: &Board, item: &str, action: &str) -> std::process::Output {
    board.feedback(&[
        "removal-check",
        "--item",
        item,
        "--proposal",
        "openspec/changes/remove-x",
        "--target",
        "skill-x",
        "--action",
        action,
    ])
}

fn hypothesis_card_count(board: &Board) -> usize {
    let cards = bd_json(
        &board.bd,
        &board.project,
        &["list", "--label", "hypothesis", "--all", "--json"],
    );
    cards.as_array().map(Vec::len).unwrap_or_default()
}

fn assert_no_votes(board: &Board, item: &str) {
    let comments = board.comment_texts(item);
    assert!(
        !comments
            .iter()
            .any(|comment| comment.contains("feedback-vote")),
        "hypothesis records must not manufacture votes: {comments:?}"
    );
}

#[test]
fn hypothesis_cards_own_relationships_and_pending_workload_benefit() {
    let board = Board::new("hypothesis-ownership");
    let a = admit(&board, "m-cache", "c-dispatch", "basis-a");
    let b = admit(&board, "m-work", "c-dispatch", "basis-b");
    assert_ne!(a, b);

    let trial = board.feedback(&[
        "hypothesis-trial",
        "--item",
        &a,
        "--experiment",
        "exp-1",
        "--role",
        "candidate",
        "--counterpart",
        &b,
        "--evidence",
        "runs/exp-1",
    ]);
    let text = output_text(&trial);
    assert!(trial.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "trial hypothesis {a} experiment=exp-1 role=candidate counterpart={b} comment=written related=established"
        )),
        "{text}"
    );
    let after_trial = board.comment_texts(&a);
    let retry = board.feedback(&[
        "hypothesis-trial",
        "--item",
        &a,
        "--experiment",
        "exp-1",
        "--role",
        "candidate",
        "--counterpart",
        &b,
    ]);
    let text = output_text(&retry);
    assert!(retry.status.success(), "{text}");
    assert!(
        text.contains("comment=already-recorded related=already-present"),
        "{text}"
    );
    assert_eq!(
        board.comment_texts(&a),
        after_trial,
        "an identical trial adds no comment"
    );

    let implementation = board.feedback(&[
        "hypothesis-implement",
        "--item",
        &b,
        "--role",
        "workload",
        "--branch",
        "hypothesis/b",
        "--base",
        "0b960b4c87f21a38f5ade8b8d27e374bacb81b8a",
        "--revision",
        "abc1234",
        "--worktree",
        "wt-loc-1",
        "--runtime",
        "runtime-h-a",
        "--baseline-runtime",
        "runtime-h",
    ]);
    let text = output_text(&implementation);
    assert!(implementation.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "implementation hypothesis {b} role=workload branch=hypothesis/b revision=abc1234 comment=written"
        )),
        "{text}"
    );

    let decided = decision(
        &board,
        &a,
        "exp-1",
        "reject",
        "regressed",
        "140",
        true,
        None,
    );
    let text = output_text(&decided);
    assert!(decided.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "decision hypothesis {a} experiment=exp-1 outcome=reject publication=written"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("closed hypothesis {a} outcome=reject")),
        "{text}"
    );

    // A finished its investigation; B still awaits its own benefit evaluation.
    assert_eq!(board.item(&a)["status"], "closed");
    assert_eq!(board.item(&b)["status"], "open");
    let b_comments = board.comment_texts(&b);
    assert!(
        b_comments
            .iter()
            .any(|comment| comment.starts_with("hypothesis-implementation v1")),
        "{b_comments:?}"
    );
    assert!(
        !b_comments
            .iter()
            .any(|comment| comment.starts_with("benefit-gate")),
        "an implemented workload records no adoption of its own: {b_comments:?}"
    );
    assert_no_votes(&board, &a);
    assert_no_votes(&board, &b);

    // The evaluation relationship is the nonblocking `related` edge.
    let dependencies = board.item(&a)["dependencies"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        dependencies.iter().any(|dependency| {
            dependency["id"].as_str() == Some(b.as_str())
                && dependency["dependency_type"] == "related"
        }),
        "{dependencies:?}"
    );
    assert_eq!(
        hypothesis_card_count(&board),
        2,
        "one durable card per hypothesis"
    );
    board.drop();
}

#[test]
fn hypothesis_decision_publication_is_idempotent_and_evidence_bound() {
    let board = Board::new("hypothesis-decision");
    let h = admit(&board, "m-lane", "c-synthetic", "basis-1");
    let before = board.comment_texts(&h).len();

    let adopted = decision(&board, &h, "exp-1", "adopt", "unchanged", "96", false, None);
    let text = output_text(&adopted);
    assert!(adopted.status.success(), "{text}");
    assert!(text.contains("publication=written"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before + 1);

    // The ledger reads the recorded evidence binding back.
    let ledger = board.feedback(&["ledger", "--item", &h]);
    let text = output_text(&ledger);
    assert!(ledger.status.success(), "{text}");
    assert!(text.contains("default=adopted"), "{text}");
    assert!(text.contains("supported=yes"), "{text}");
    assert!(
        text.contains(&format!(
            "benefit gate {h}: binding: experiment=exp-1 revisions=base001..cand002 acceptance=evidence-9 coverage=time+rounds scope=task:synthetic reason=lane-contention"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit-gate record item={h} outcome=adopt quality=unchanged experiment=exp-1 revisions=base001..cand002 acceptance=evidence-9 coverage=time+rounds scope=task:synthetic reason=lane-contention"
        )),
        "{text}"
    );

    // A retried publication confirms the recorded decision with no new
    // comment and no second apparent experiment.
    let retry = decision(&board, &h, "exp-1", "adopt", "unchanged", "96", false, None);
    let text = output_text(&retry);
    assert!(retry.status.success(), "{text}");
    assert!(text.contains("publication=already-recorded"), "{text}");
    assert_eq!(
        board.comment_texts(&h).len(),
        before + 1,
        "a retry adds no duplicate decision comment"
    );

    // Missing or contradictory evidence cannot authorize adoption.
    let inert = decision(
        &board,
        &h,
        "exp-2",
        "adopt",
        "unchanged",
        "104",
        false,
        None,
    );
    let text = output_text(&inert);
    assert!(!inert.status.success(), "{text}");
    assert!(
        text.contains("is not supported by its own comparison"),
        "{text}"
    );

    // A faster candidate whose measured quality regressed is equally unable
    // to authorize an adoption.
    let contradicted = decision(&board, &h, "exp-2", "adopt", "regressed", "96", false, None);
    let text = output_text(&contradicted);
    assert!(!contradicted.status.success(), "{text}");
    assert!(text.contains("quality regressed"), "{text}");

    let same_revision = board.feedback(&[
        "hypothesis-decision",
        "--item",
        &h,
        "--experiment",
        "exp-3",
        "--outcome",
        "adopt",
        "--quality",
        "unchanged",
        "--matched",
        "2",
        "--tolerance-percent",
        "10",
        "--baseline-seconds",
        "100",
        "--candidate-seconds",
        "96",
        "--baseline-arm",
        "direct",
        "--candidate-arm",
        "lane",
        "--accounting",
        "check",
        "--baseline-revision",
        "base001",
        "--candidate-revision",
        "base001",
        "--acceptance",
        "evidence-9",
        "--coverage",
        "time",
        "--scope",
        "task:synthetic",
        "--reason",
        "lane-contention",
    ]);
    let text = output_text(&same_revision);
    assert!(!same_revision.status.success(), "{text}");
    assert!(text.contains("identical"), "{text}");

    // Every binding is required before an adoption can publish: an omitted
    // experiment, acceptance evidence, metric coverage, scope or reason is
    // refused and writes nothing.
    for (flag, phrase) in [
        ("--experiment", "experiment is required"),
        ("--acceptance", "acceptance is required"),
        ("--coverage", "coverage is required"),
        ("--scope", "scope is required"),
        ("--reason", "reason is required"),
    ] {
        let refused = decision_with_absent_binding(&board, &h, "exp-4", flag);
        let text = output_text(&refused);
        assert!(!refused.status.success(), "{flag}: {text}");
        assert!(text.contains(phrase), "{flag}: {text}");
    }

    assert_eq!(
        board.comment_texts(&h).len(),
        before + 1,
        "refused publications write nothing"
    );
    assert_no_votes(&board, &h);
    board.drop();
}

#[test]
fn engineering_judgment_publication_retains_cost_and_refuses_relabelling() {
    let board = Board::new("judgment-decision");
    let h = admit(&board, "m-judgment", "c-synthetic", "basis-1");
    let before = board.comment_texts(&h).len();

    let published = judgment_decision(
        &board,
        &h,
        "exp-judgment",
        Some("reflection:lead-judgment-1"),
    );
    let text = output_text(&published);
    assert!(published.status.success(), "{text}");
    assert!(text.contains("outcome=adopt publication=written"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before + 1);
    let decision_comment = board
        .comment_texts(&h)
        .into_iter()
        .find(|comment| comment.starts_with("benefit-gate v2"))
        .expect("the published judgment decision");
    assert!(
        decision_comment
            .contains("basis=engineering-judgment basis_ref=reflection:lead-judgment-1"),
        "{decision_comment}"
    );
    assert!(
        decision_comment.contains(
            "baseline_seconds=100.000 candidate_seconds=115.000 regression_percent=15.000"
        ),
        "the observed added cost stays recorded: {decision_comment}"
    );
    assert!(
        decision_comment.contains(&format!(
            "reason={JUDGMENT_REASON} outcome=adopt quality=unchanged"
        )),
        "the scoped lasting-benefit reason stays recorded: {decision_comment}"
    );
    assert!(
        decision_comment.contains(&format!("accounting={JUDGMENT_ACCOUNTING}")),
        "the full-cost accounting stays recorded: {decision_comment}"
    );
    assert!(
        decision_comment.contains("payback_measures=quality_score+incidents"),
        "the custom named measures stay recorded: {decision_comment}"
    );
    assert!(
        decision_comment.contains(r#""measure":"quality_score""#)
            && decision_comment.contains(r#""measure":"incidents""#),
        "the observed named values stay recorded rather than a declaration alone: {decision_comment}"
    );
    assert!(
        decision_comment
            .contains("cumulative_baseline_tokens=1000 cumulative_candidate_tokens=1100"),
        "the first publication derives its cumulative totals: {decision_comment}"
    );
    assert!(
        decision_comment
            .contains("detail=lead judged retained added cost against lower future review burden"),
        "the lead's bounded cost/benefit judgment stays recorded: {decision_comment}"
    );

    let ledger = board.feedback(&["ledger", "--item", &h]);
    let text = output_text(&ledger);
    assert!(ledger.status.success(), "{text}");
    assert!(
        text.contains(
            "supported=yes (recorded engineering-judgment adoption retains measured cost and its declared basis) default=adopted"
        ),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit gate {h}: binding: experiment=exp-judgment revisions=base001..cand002 acceptance=evidence-9 coverage=time+rounds scope=task:synthetic reason={JUDGMENT_REASON} basis=engineering-judgment basis_ref=reflection:lead-judgment-1"
        )),
        "{text}"
    );

    // A retried publication confirms the identical record without a second
    // comment or a second experiment count.
    let retry = judgment_decision(
        &board,
        &h,
        "exp-judgment",
        Some("reflection:lead-judgment-1"),
    );
    let text = output_text(&retry);
    assert!(retry.status.success(), "{text}");
    assert!(text.contains("publication=already-recorded"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before + 1);

    // The judgment basis needs its predeclared reference, and the reference
    // is only meaningful with the judgment basis.
    let missing_reference = judgment_decision_after_adoption(&board, &h, "exp-missing-ref", None);
    let text = output_text(&missing_reference);
    assert!(!missing_reference.status.success(), "{text}");
    assert!(text.contains("requires --basis-ref"), "{text}");

    let mut inert_args = decision_arguments(
        &h,
        "exp-inert-basis",
        "adopt",
        "unchanged",
        "96",
        false,
        None,
    );
    inert_args.push("--basis-ref".to_owned());
    inert_args.push("reflection:lead-judgment-1".to_owned());
    let args: Vec<&str> = inert_args.iter().map(String::as_str).collect();
    let inert = board.feedback(&args);
    let text = output_text(&inert);
    assert!(!inert.status.success(), "{text}");
    assert!(
        text.contains("--basis-ref requires --basis engineering-judgment"),
        "{text}"
    );

    // The legacy maintenance basis stays a policy-level strict meaning, not a
    // public override of an adoption.
    let mut legacy_args = decision_arguments(
        &h,
        "exp-legacy-basis",
        "adopt",
        "unchanged",
        "96",
        false,
        None,
    );
    legacy_args.push("--basis".to_owned());
    legacy_args.push("maintenance".to_owned());
    let args: Vec<&str> = legacy_args.iter().map(String::as_str).collect();
    let legacy = board.feedback(&args);
    let text = output_text(&legacy);
    assert!(!legacy.status.success(), "{text}");
    assert!(text.contains("not an opt-in adoption basis"), "{text}");

    let mut unknown_basis = decision_arguments(
        &h,
        "exp-unknown-basis",
        "adopt",
        "unchanged",
        "96",
        false,
        None,
    );
    unknown_basis.push("--basis".to_owned());
    unknown_basis.push("judgment".to_owned());
    let args: Vec<&str> = unknown_basis.iter().map(String::as_str).collect();
    let unknown = board.feedback(&args);
    let text = output_text(&unknown);
    assert!(!unknown.status.success(), "{text}");
    assert!(
        text.contains("--basis is efficiency or engineering-judgment"),
        "{text}"
    );

    let invalid_reference =
        judgment_decision_after_adoption(&board, &h, "exp-invalid-ref", Some("lead reason"));
    let text = output_text(&invalid_reference);
    assert!(!invalid_reference.status.success(), "{text}");
    assert!(text.contains("unsupported characters"), "{text}");

    // Quality regression and critical defects are never waived by judgment.
    let mut regressed_args = decision_arguments(
        &h,
        "exp-quality-regression",
        "adopt",
        "regressed",
        "115",
        false,
        None,
    );
    replace_argument(&mut regressed_args, "--baseline-revision", "cand002");
    replace_argument(&mut regressed_args, "--candidate-revision", "cand003");
    regressed_args.push("--basis".to_owned());
    regressed_args.push("engineering-judgment".to_owned());
    regressed_args.push("--basis-ref".to_owned());
    regressed_args.push("reflection:lead-judgment-1".to_owned());
    let mut regressed_tokens = judgment_token_arguments(&board);
    replace_argument(&mut regressed_tokens, "--baseline-tokens", "1100");
    replace_argument(&mut regressed_tokens, "--candidate-tokens", "1200");
    regressed_args.extend(regressed_tokens);
    let args: Vec<&str> = regressed_args.iter().map(String::as_str).collect();
    let regressed = board.feedback(&args);
    let text = output_text(&regressed);
    assert!(!regressed.status.success(), "{text}");
    assert!(text.contains("quality regressed"), "{text}");

    // Basis-less equal or slower evidence keeps the strict no-adoption
    // reading; judgment is an explicit declaration, never a fallback.
    let basis_less = decision(
        &board,
        &h,
        "exp-strict-slower",
        "adopt",
        "unchanged",
        "115",
        false,
        None,
    );
    let text = output_text(&basis_less);
    assert!(!basis_less.status.success(), "{text}");
    assert!(
        text.contains("candidate slower within tolerance")
            || text.contains("is not supported by its own comparison"),
        "{text}"
    );

    // An experiment already decided on the strict basis cannot be
    // reinterpreted as an engineering-judgment adoption.
    let strict = decision(
        &board,
        &h,
        "exp-reinterpret",
        "adopt",
        "unchanged",
        "96",
        false,
        None,
    );
    assert!(strict.status.success(), "{}", output_text(&strict));
    let reinterpret = judgment_decision_after_adoption(
        &board,
        &h,
        "exp-reinterpret",
        Some("reflection:lead-judgment-2"),
    );
    let text = output_text(&reinterpret);
    assert!(!reinterpret.status.success(), "{text}");
    assert!(
        text.contains("cannot reinterpret an earlier record"),
        "{text}"
    );

    assert_eq!(
        board.comment_texts(&h).len(),
        before + 2,
        "only the judgment decision and the strict decision were written"
    );
    assert_no_votes(&board, &h);
    board.drop();
}

#[test]
fn cumulative_judgment_costs_are_derived_without_manual_totals_and_refuse_drift() {
    let board = Board::new("judgment-cumulative");
    let h = admit(&board, "m-cumulative", "c-synthetic", "basis-cumulative");
    let before = board.comment_texts(&h).len();

    let supported = payback_file(
        &board,
        "payback-supported.json",
        "fewer-future-incidents",
        json!([
            {
                "measure": "quality_score",
                "unit": "points",
                "direction": "higher-is-better",
                "baseline": 80.0,
                "candidate": 90.0,
                "evidence": "evidence-9"
            },
            {
                "measure": "incidents",
                "unit": "count",
                "direction": "lower-is-better",
                "baseline": 3.0,
                "candidate": 1.0,
                "evidence": "evidence-9"
            }
        ]),
    );
    let unsupported = payback_file(
        &board,
        "payback-unsupported.json",
        "fewer-future-incidents",
        json!([{
            "measure": "quality_score",
            "unit": "points",
            "direction": "higher-is-better",
            "baseline": 90.0,
            "candidate": 90.0,
            "evidence": "evidence-9"
        }]),
    );
    let names_only = payback_file(
        &board,
        "payback-names-only.json",
        "fewer-future-incidents",
        json!([{
            "measure": "quality_score",
            "unit": "points",
            "direction": "higher-is-better"
        }]),
    );

    let publish = |experiment: &str,
                   baseline_revision: &str,
                   candidate_revision: &str,
                   baseline_tokens: &str,
                   candidate_tokens: &str,
                   cumulative: Option<(&str, &str, &str)>,
                   payback: &Path| {
        let mut args = decision_arguments(&h, experiment, "adopt", "unchanged", "115", false, None);
        replace_argument(&mut args, "--reason", JUDGMENT_REASON);
        replace_argument(&mut args, "--accounting", JUDGMENT_ACCOUNTING);
        replace_argument(&mut args, "--baseline-revision", baseline_revision);
        replace_argument(&mut args, "--candidate-revision", candidate_revision);
        push_argument(&mut args, "--basis", "engineering-judgment");
        push_argument(&mut args, "--basis-ref", "reflection:lead-cumulative");
        push_argument(&mut args, "--baseline-tokens", baseline_tokens);
        push_argument(&mut args, "--candidate-tokens", candidate_tokens);
        if let Some((baseline, candidate, identity)) = cumulative {
            push_argument(&mut args, "--cumulative-baseline-tokens", baseline);
            push_argument(&mut args, "--cumulative-candidate-tokens", candidate);
            push_argument(&mut args, "--cumulative-baseline", identity);
        }
        push_argument(
            &mut args,
            "--payback-file",
            payback.to_str().expect("payback file path"),
        );
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        board.feedback(&args)
    };

    // The first adopted change starts its cumulative total at the local
    // comparison, bound to the retained baseline, without caller arithmetic.
    let first = publish(
        "exp-first",
        "base001",
        "cand002",
        "1000",
        "1100",
        None,
        &supported,
    );
    assert!(first.status.success(), "{}", output_text(&first));
    assert_eq!(board.comment_texts(&h).len(), before + 1);
    let first_comment = board
        .comment_texts(&h)
        .into_iter()
        .find(|comment| comment.contains("experiment=exp-first"))
        .expect("the first published judgment decision");
    assert!(
        first_comment.contains(
            "baseline_tokens=1000 candidate_tokens=1100 cumulative_baseline_tokens=1000 cumulative_candidate_tokens=1100 cumulative_baseline=base001"
        ),
        "{first_comment}"
    );
    assert!(
        first_comment.contains("payback_measures=quality_score+incidents"),
        "custom named measures travel into the native record: {first_comment}"
    );
    assert!(
        first_comment.contains(r#""measure":"quality_score""#)
            && first_comment.contains(r#""measure":"incidents""#),
        "the observed values travel with the record: {first_comment}"
    );
    let records =
        harness_core::benefit_gate::parse_gate_comments(std::slice::from_ref(&first_comment));
    let assessment = harness_core::benefit_gate::assess(&records, &h)
        .expect("the published decision is attributable");
    assert_eq!(
        assessment.verdict,
        harness_core::benefit_gate::Verdict::Consistent,
        "{:?}",
        assessment.limitations
    );

    // Repeating the same publication confirms it and adds neither local nor
    // cumulative cost again.
    let repeat_first = publish(
        "exp-first",
        "base001",
        "cand002",
        "1000",
        "1100",
        None,
        &supported,
    );
    let text = output_text(&repeat_first);
    assert!(repeat_first.status.success(), "{text}");
    assert!(text.contains("publication=already-recorded"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before + 1);

    // A second related change starts from the retained candidate. The owner
    // adds only the local delta to the retained cumulative total.
    let second = publish(
        "exp-second",
        "cand002",
        "cand003",
        "1100",
        "1300",
        None,
        &supported,
    );
    assert!(second.status.success(), "{}", output_text(&second));
    assert_eq!(board.comment_texts(&h).len(), before + 2);
    let second_comment = board
        .comment_texts(&h)
        .into_iter()
        .find(|comment| comment.contains("experiment=exp-second"))
        .expect("the second published judgment decision");
    assert!(
        second_comment.contains(
            "baseline_tokens=1100 candidate_tokens=1300 cumulative_baseline_tokens=1000 cumulative_candidate_tokens=1300 cumulative_baseline=base001"
        ),
        "{second_comment}"
    );
    let repeat_second = publish(
        "exp-second",
        "cand002",
        "cand003",
        "1100",
        "1300",
        None,
        &supported,
    );
    let text = output_text(&repeat_second);
    assert!(repeat_second.status.success(), "{text}");
    assert!(text.contains("publication=already-recorded"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before + 2);

    // An explicit cumulative assertion that conflicts with the derived total
    // is refused before mutation.
    let conflicting = publish(
        "exp-third",
        "cand003",
        "cand004",
        "1300",
        "1400",
        Some(("1000", "1301", "base001")),
        &supported,
    );
    let text = output_text(&conflicting);
    assert!(!conflicting.status.success(), "{text}");
    assert!(
        text.contains("conflicts with the totals derived from the retained board history"),
        "{text}"
    );
    assert_eq!(board.comment_texts(&h).len(), before + 2);

    // Restarting the local comparison at the original baseline would count
    // retained cost twice; it is stale and refused before mutation.
    let reset = publish(
        "exp-fourth",
        "base001",
        "cand005",
        "1000",
        "1100",
        None,
        &supported,
    );
    let text = output_text(&reset);
    assert!(!reset.status.success(), "{text}");
    assert!(text.contains("retained baseline is stale"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before + 2);

    // A names-only payback declaration cannot authorize increased cost.
    let bare = publish(
        "exp-bare",
        "cand003",
        "cand006",
        "1300",
        "1400",
        None,
        &names_only,
    );
    let text = output_text(&bare);
    assert!(!bare.status.success(), "{text}");
    assert!(text.contains("lacks finite non-negative values"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before + 2);

    // A measured named measure that did not improve is evidence against the
    // payback, not a return.
    let flat = publish(
        "exp-flat",
        "cand003",
        "cand007",
        "1300",
        "1400",
        None,
        &unsupported,
    );
    let text = output_text(&flat);
    assert!(!flat.status.success(), "{text}");
    assert!(text.contains("did not improve"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before + 2);

    assert_no_votes(&board, &h);
    board.drop();
}

fn publish_cumulative_counterexample(
    board: &Board,
    item: &str,
    experiment: &str,
    revisions: (&str, &str),
    tokens: (&str, &str),
) -> std::process::Output {
    let supported = payback_file(
        board,
        "cumulative-counterexample-payback.json",
        "fewer-correction-rounds",
        json!([{
            "measure": "rounds",
            "unit": "count",
            "direction": "lower-is-better",
            "baseline": 3.0,
            "candidate": 1.0,
            "evidence": "evidence-9"
        }]),
    );
    let mut args = decision_arguments(item, experiment, "adopt", "unchanged", "115", false, None);
    replace_argument(&mut args, "--reason", JUDGMENT_REASON);
    replace_argument(&mut args, "--accounting", JUDGMENT_ACCOUNTING);
    replace_argument(&mut args, "--baseline-revision", revisions.0);
    replace_argument(&mut args, "--candidate-revision", revisions.1);
    push_argument(&mut args, "--basis", "engineering-judgment");
    push_argument(
        &mut args,
        "--basis-ref",
        "reflection:cumulative-counterexample",
    );
    push_argument(&mut args, "--baseline-tokens", tokens.0);
    push_argument(&mut args, "--candidate-tokens", tokens.1);
    push_argument(
        &mut args,
        "--payback-file",
        supported.to_str().expect("payback path"),
    );
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    board.feedback(&args)
}

#[test]
fn cumulative_judgment_counterexample_accepts_measured_token_reduction() {
    let board = Board::new("judgment-cumulative-saving");
    let item = admit(&board, "m-saving", "c-synthetic", "basis-saving");
    let first = publish_cumulative_counterexample(
        &board,
        &item,
        "exp-first",
        ("base001", "cand002"),
        ("1000", "1100"),
    );
    assert!(first.status.success(), "{}", output_text(&first));
    let saving = publish_cumulative_counterexample(
        &board,
        &item,
        "exp-saving",
        ("cand002", "cand003"),
        ("1100", "900"),
    );
    assert!(
        saving.status.success(),
        "measured savings must not be classified as a reset: {}",
        output_text(&saving)
    );
    let retained = board
        .comment_texts(&item)
        .into_iter()
        .find(|comment| comment.contains("experiment=exp-saving"))
        .expect("saving publication");
    assert!(retained.contains("cumulative_baseline_tokens=1000 cumulative_candidate_tokens=900 cumulative_baseline=base001"), "{retained}");
    assert_no_votes(&board, &item);
    board.drop();
}

#[test]
fn cumulative_judgment_counterexample_retains_baseline_across_hypotheses() {
    let board = Board::new("judgment-cumulative-hypotheses");
    let first_item = admit(&board, "m-first", "c-synthetic", "basis-first");
    let second_item = admit(&board, "m-second", "c-synthetic", "basis-second");
    let first = publish_cumulative_counterexample(
        &board,
        &first_item,
        "exp-first",
        ("base001", "cand002"),
        ("1000", "1100"),
    );
    assert!(first.status.success(), "{}", output_text(&first));
    let second = publish_cumulative_counterexample(
        &board,
        &second_item,
        "exp-second",
        ("cand002", "cand003"),
        ("1100", "1300"),
    );
    assert!(second.status.success(), "{}", output_text(&second));
    let retained = board
        .comment_texts(&second_item)
        .into_iter()
        .find(|comment| comment.contains("experiment=exp-second"))
        .expect("second publication");
    assert!(retained.contains("cumulative_baseline_tokens=1000 cumulative_candidate_tokens=1300 cumulative_baseline=base001"),
        "a new hypothesis in the same scope must not erase retained spending: {retained}");
    assert_no_votes(&board, &first_item);
    assert_no_votes(&board, &second_item);
    board.drop();
}

/// Reconciliation binds the claimed outcome and change to what the board and
/// card actually record before any OpenSpec effect: an unpublished decision,
/// an unadopted-outcome mismatch, an adopted outcome or another change name
/// are all refused, so a rejected experiment cannot be reconciled as
/// something the board does not record.
#[test]
fn hypothesis_reconcile_binds_the_recorded_decision_and_linked_change() {
    let board = Board::new("hypothesis-reconcile-bindings");
    let h = admit(&board, "m-reconcile", "c-reconcile", "basis-1");

    let absent = board.feedback(&[
        "hypothesis-reconcile",
        "--item",
        &h,
        "--outcome",
        "reject",
        "--change",
        "demo",
    ]);
    let text = output_text(&absent);
    assert!(!absent.status.success(), "{text}");
    assert!(text.contains("no recorded benefit-gate decision"), "{text}");

    let adopted = decision(&board, &h, "exp-1", "adopt", "unchanged", "96", false, None);
    assert!(adopted.status.success(), "{}", output_text(&adopted));
    let adopt = board.feedback(&[
        "hypothesis-reconcile",
        "--item",
        &h,
        "--outcome",
        "adopt",
        "--change",
        "demo",
    ]);
    let text = output_text(&adopt);
    assert!(!adopt.status.success(), "{text}");
    assert!(
        text.contains("adopted delta synchronizes through the adoption/integration owner"),
        "{text}"
    );

    let rejected = decision(
        &board,
        &h,
        "exp-2",
        "reject",
        "regressed",
        "140",
        true,
        None,
    );
    assert!(rejected.status.success(), "{}", output_text(&rejected));

    // The claimed outcome must be the decision the board actually records.
    let mismatch = board.feedback(&[
        "hypothesis-reconcile",
        "--item",
        &h,
        "--outcome",
        "inconclusive",
        "--change",
        "demo",
    ]);
    let text = output_text(&mismatch);
    assert!(!mismatch.status.success(), "{text}");
    assert!(
        text.contains("latest recorded decision is reject"),
        "{text}"
    );

    // Only the card's own linked change may be reconciled.
    let wrong_change = board.feedback(&[
        "hypothesis-reconcile",
        "--item",
        &h,
        "--outcome",
        "reject",
        "--change",
        "other-change",
    ]);
    let text = output_text(&wrong_change);
    assert!(!wrong_change.status.success(), "{text}");
    assert!(
        text.contains(
            "references spec 'openspec/changes/demo' instead of the change 'other-change'"
        ),
        "{text}"
    );

    // An inconclusive investigation is retained, not archived.
    let pending = admit(&board, "m-pending", "c-reconcile", "basis-2");
    let pending_decision = decision(
        &board,
        &pending,
        "exp-3",
        "inconclusive",
        "unmeasurable",
        "100",
        false,
        None,
    );
    assert!(
        pending_decision.status.success(),
        "{}",
        output_text(&pending_decision)
    );
    let inconclusive = board.feedback(&[
        "hypothesis-reconcile",
        "--item",
        &pending,
        "--outcome",
        "inconclusive",
        "--change",
        "demo",
        "--action",
        "archive",
    ]);
    let text = output_text(&inconclusive);
    assert!(!inconclusive.status.success(), "{text}");
    assert!(
        text.contains("refused for an inconclusive investigation"),
        "{text}"
    );
    assert_no_votes(&board, &h);
    board.drop();
}

/// A newer incomplete, malformed or contradictory v2 record supersedes an
/// earlier supported adoption on the real board read path: the item stays
/// unadopted, the limitations name the missing or contradicting evidence, and
/// the earlier decision remains visible in the recorded history.
#[test]
fn newer_incomplete_records_never_revive_an_earlier_adoption() {
    let board = Board::new("decision-supersession");
    let h = admit(&board, "m-supersede", "c-synthetic", "basis-2");
    let adopted = decision(
        &board,
        &h,
        "exp-10",
        "adopt",
        "unchanged",
        "96",
        false,
        None,
    );
    let text = output_text(&adopted);
    assert!(adopted.status.success(), "{text}");
    let ledger = board.feedback(&["ledger", "--item", &h]);
    let text = output_text(&ledger);
    assert!(text.contains("default=adopted"), "{text}");

    // The newer record omits its acceptance evidence, scope and reason: it is
    // retained as the latest decision and cannot authorize the adoption.
    comment(
        &board,
        &h,
        &format!(
            "benefit-gate v2 item={h} experiment=exp-11 revisions=base001..cand003 coverage=time outcome=adopt quality=unchanged matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=96.0 regression_percent=-4.0 baseline=direct candidate=lane accounting=check detail=incomplete binding"
        ),
    );
    let ledger = board.feedback(&["ledger", "--item", &h]);
    let text = output_text(&ledger);
    assert!(text.contains("across 2 attributable record(s)"), "{text}");
    assert!(text.contains("default=unadopted"), "{text}");
    assert!(!text.contains("default=adopted"), "{text}");
    assert!(
        text.contains(
            "limitations: acceptance evidence reference missing; decision scope missing; decision reason missing"
        ),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit-gate record item={h} outcome=adopt quality=unchanged experiment=exp-10"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "benefit-gate record item={h} outcome=adopt quality=unchanged experiment=exp-11"
        )),
        "{text}"
    );

    // A newer record contradicting its own measured quality supersedes the
    // earlier adoption without rewriting it.
    comment(
        &board,
        &h,
        &format!(
            "benefit-gate v2 item={h} experiment=exp-12 revisions=base001..cand004 acceptance=evidence-10 coverage=time scope=task:synthetic reason=lane outcome=adopt quality=regressed matched=2 tolerance_percent=10.0 baseline_seconds=100.0 candidate_seconds=140.0 regression_percent=40.0 baseline=direct candidate=lane accounting=check detail=regressed"
        ),
    );
    let ledger = board.feedback(&["ledger", "--item", &h]);
    let text = output_text(&ledger);
    assert!(text.contains("across 3 attributable record(s)"), "{text}");
    assert!(
        text.contains("limitations: quality regressed; regression beyond the declared tolerance"),
        "{text}"
    );
    assert!(!text.contains("default=adopted"), "{text}");

    // A newer malformed record stays unreadable: it supersedes the adoption
    // instead of being skipped in favor of the older supported record.
    comment(
        &board,
        &h,
        &format!("benefit-gate v2 item={h} outcome=adopt"),
    );
    let ledger = board.feedback(&["ledger", "--item", &h]);
    let text = output_text(&ledger);
    assert!(text.contains("across 4 attributable record(s)"), "{text}");
    assert!(
        text.contains("limitations: experiment reference missing"),
        "{text}"
    );
    assert!(!text.contains("default=adopted"), "{text}");
    board.drop();
}

#[test]
fn hypothesis_search_reuses_prior_conclusions_and_never_duplicates_cards() {
    let board = Board::new("hypothesis-search");
    let rejected = admit(&board, "m-rejected", "c-search", "basis-r");
    let decided = decision(
        &board,
        &rejected,
        "exp-r",
        "reject",
        "regressed",
        "140",
        true,
        None,
    );
    assert!(decided.status.success(), "{}", output_text(&decided));

    let deferred = admit(&board, "m-deferred", "c-search", "basis-d");
    let inconclusive = decision(
        &board,
        &deferred,
        "exp-d",
        "inconclusive",
        "unmeasurable",
        "100",
        false,
        Some("+24h"),
    );
    assert!(
        inconclusive.status.success(),
        "{}",
        output_text(&inconclusive)
    );
    assert_eq!(board.item(&deferred)["status"], "deferred");

    let open = admit(&board, "m-open", "c-search", "basis-o");

    // Search covers open, closed and deferred cards with their conclusions.
    let search = board.feedback(&["hypothesis-search"]);
    let text = output_text(&search);
    assert!(search.status.success(), "{text}");
    assert!(text.contains("hypotheses: 3 matching"), "{text}");
    assert!(
        text.contains(&format!("hypothesis {rejected} status=closed")),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "hypothesis {rejected} latest=reject experiment=exp-r scope=task:synthetic reason=lane-contention"
        )),
        "{text}"
    );
    assert!(
        text.contains(&format!("hypothesis {deferred} status=deferred")),
        "{text}"
    );
    assert!(
        text.contains(&format!("hypothesis {open} status=open")),
        "{text}"
    );
    let filtered = board.feedback(&["hypothesis-search", "--mechanism", "m-rejected"]);
    let text = output_text(&filtered);
    assert!(text.contains("hypotheses: 1 matching"), "{text}");
    assert!(!text.contains(&open), "{text}");

    // The same-condition rejection is reused as recorded: no new card, no
    // new comment.
    let before = board.comment_texts(&rejected);
    let reused = board.feedback(&[
        "hypothesis-admit",
        "--mechanism",
        "m-rejected",
        "--conditions",
        "c-search",
        "--observation",
        "ev-observation",
        "--predicted",
        "less repeated work",
        "--counterexample",
        "the burden does not recover",
        "--acceptance",
        "the declared check passes",
        "--spec",
        "openspec/changes/demo",
        "--basis",
        "basis-r",
    ]);
    let text = output_text(&reused);
    assert!(reused.status.success(), "{text}");
    assert!(
        text.contains(&format!(
            "hypothesis {rejected} reused conclusion=reject experiment=exp-r reason=lane-contention basis=basis-r"
        )),
        "{text}"
    );
    assert_eq!(board.comment_texts(&rejected), before);
    assert_eq!(hypothesis_card_count(&board), 3);

    // A fresh evidential basis reconsiders the same card and preserves the
    // earlier result.
    let fresh = admit_with(
        &board,
        "m-rejected",
        "c-search",
        "basis-r",
        Some("ev-fresh-2"),
    );
    assert_eq!(fresh, rejected, "reconsideration reuses the existing card");
    let comments = board.comment_texts(&rejected);
    assert!(
        comments.iter().any(|comment| comment
            == &format!(
                "hypothesis-reconsideration v1 item={rejected} basis=ev-fresh-2 prior=exp-r conclusion=reject"
            )),
        "{comments:?}"
    );
    assert!(
        comments
            .iter()
            .any(|comment| comment.starts_with("benefit-gate v2")),
        "the earlier rejection is preserved: {comments:?}"
    );
    assert_eq!(board.item(&rejected)["status"], "open");
    assert_eq!(hypothesis_card_count(&board), 3);

    // Once reconsidered (active again), a repeated admission returns the
    // existing card rather than creating another one.
    let existing = admit_with(
        &board,
        "m-rejected",
        "c-search",
        "basis-r",
        Some("ev-fresh-3"),
    );
    assert_eq!(existing, rejected);
    assert_eq!(hypothesis_card_count(&board), 3);
    assert_no_votes(&board, &rejected);
    board.drop();
}

#[test]
fn removal_authority_is_scoped_separate_from_benefit_and_latest_controlled() {
    let board = Board::new("removal");
    let candidate = admit(&board, "m-remove", "c-remove", "basis-1");
    let workload = admit(&board, "m-workload", "c-remove", "basis-2");

    // No decision exists yet; run-start authority and benefit verdicts are
    // never treated as consent.
    let missing = removal_check(&board, &candidate, "experiment");
    assert_eq!(missing.status.code(), Some(1), "{}", output_text(&missing));
    assert!(output_text(&missing).contains("result=missing"));

    let proposal = board.feedback(&[
        "removal-propose",
        "--item",
        &candidate,
        "--proposal",
        "openspec/changes/remove-x",
        "--target",
        "skill-x",
        "--evidence",
        "ev-removal",
        "--loss",
        "skill-x",
        "--detail",
        "preview prepared; nothing applied",
    ]);
    assert!(proposal.status.success(), "{}", output_text(&proposal));

    // A decision for an unrecorded proposal is refused: the user decides on
    // a presented proposal.
    let early = board.feedback(&[
        "removal-decide",
        "--item",
        &candidate,
        "--decision",
        "approve",
        "--proposal",
        "openspec/changes/remove-y",
        "--target",
        "skill-x",
        "--actions",
        "experiment",
    ]);
    let text = output_text(&early);
    assert!(!early.status.success(), "{text}");
    assert!(text.contains("no removal proposal"), "{text}");

    let approve = removal_decision(
        &board,
        &candidate,
        "approve",
        Some("experiment"),
        Some("skill-x"),
        Some("user-turn-7"),
        "approved the isolated experiment",
    );
    assert!(approve.status.success(), "{}", output_text(&approve));

    let authorized = removal_check(&board, &candidate, "experiment");
    assert_eq!(
        authorized.status.code(),
        Some(0),
        "{}",
        output_text(&authorized)
    );
    assert!(output_text(&authorized).contains("result=authorized"));
    let uncovered = removal_check(&board, &candidate, "integration");
    assert_eq!(uncovered.status.code(), Some(1));
    assert!(output_text(&uncovered).contains("result=not-covered"));

    // Workload B needs its own approval before any removal arm applies.
    let workload_check = removal_check(&board, &workload, "experiment");
    assert_eq!(workload_check.status.code(), Some(1));
    assert!(output_text(&workload_check).contains("result=missing"));

    // A favorable benefit result is not removal consent.
    let benefit = decision(
        &board,
        &candidate,
        "exp-1",
        "adopt",
        "unchanged",
        "96",
        false,
        None,
    );
    assert!(benefit.status.success(), "{}", output_text(&benefit));
    let after_benefit = removal_check(&board, &candidate, "integration");
    assert_eq!(after_benefit.status.code(), Some(1));
    assert!(output_text(&after_benefit).contains("result=not-covered"));

    // Refusal is recorded separately, and a later approval supersedes it.
    let refuse = removal_decision(
        &board,
        &candidate,
        "refuse",
        None,
        None,
        Some("user-turn-8"),
        "declined the removal",
    );
    assert!(refuse.status.success(), "{}", output_text(&refuse));
    let refused = removal_check(&board, &candidate, "experiment");
    assert_eq!(refused.status.code(), Some(1));
    assert!(output_text(&refused).contains("result=refused"));

    let reapprove = removal_decision(
        &board,
        &candidate,
        "approve",
        Some("experiment+integration"),
        Some("skill-x"),
        Some("user-turn-9"),
        "approved experiment and integration",
    );
    assert!(reapprove.status.success(), "{}", output_text(&reapprove));
    let integration = removal_check(&board, &candidate, "integration");
    assert_eq!(
        integration.status.code(),
        Some(0),
        "{}",
        output_text(&integration)
    );

    // Withdrawal blocks the next removal effect.
    let withdraw = removal_decision(
        &board,
        &candidate,
        "withdraw",
        None,
        None,
        None,
        "withdrawn while the loop was stopped",
    );
    assert!(withdraw.status.success(), "{}", output_text(&withdraw));
    let withdrawn = removal_check(&board, &candidate, "experiment");
    assert_eq!(withdrawn.status.code(), Some(1));
    assert!(output_text(&withdrawn).contains("result=withdrawn"));

    // A newer unreadable decision cannot recover the older approval.
    comment(
        &board,
        &candidate,
        &format!("removal-decision v1 item={candidate}"),
    );
    let malformed = removal_check(&board, &candidate, "experiment");
    assert_eq!(malformed.status.code(), Some(1));
    assert!(output_text(&malformed).contains("result=not-covered"));

    // A valid newer approval is honored again.
    let final_approve = removal_decision(
        &board,
        &candidate,
        "approve",
        Some("experiment"),
        Some("skill-x"),
        Some("user-turn-10"),
        "re-approved the isolated experiment",
    );
    assert!(
        final_approve.status.success(),
        "{}",
        output_text(&final_approve)
    );
    let authorized = removal_check(&board, &candidate, "experiment");
    assert_eq!(
        authorized.status.code(),
        Some(0),
        "{}",
        output_text(&authorized)
    );
    assert_no_votes(&board, &candidate);
    board.drop();
}

#[test]
fn removal_consent_binds_to_the_reviewed_proposal_content() {
    let board = Board::new("removal-binding");
    let h = admit(&board, "m-remove-bind", "c-remove", "basis-1");
    let propose = |loss: &str, evidence: &str, preview: &str, detail: &str| {
        board.feedback(&[
            "removal-propose",
            "--item",
            &h,
            "--proposal",
            "openspec/changes/remove-x",
            "--target",
            "skill-x",
            "--evidence",
            evidence,
            "--loss",
            loss,
            "--preview",
            preview,
            "--detail",
            detail,
        ])
    };
    let first = propose("skill-x", "ev-1", "preview-1", "consumer list: none known");
    assert!(first.status.success(), "{}", output_text(&first));

    let approve = removal_decision(
        &board,
        &h,
        "approve",
        Some("experiment"),
        Some("skill-x"),
        Some("user-turn-7"),
        "approved the isolated experiment",
    );
    assert!(approve.status.success(), "{}", output_text(&approve));
    assert_eq!(
        removal_check(&board, &h, "experiment").status.code(),
        Some(0)
    );

    // A retry of the *current* decision confirms it without a new comment.
    let before = board.comment_texts(&h).len();
    let retry = removal_decision(
        &board,
        &h,
        "approve",
        Some("experiment"),
        Some("skill-x"),
        Some("user-turn-7"),
        "approved the isolated experiment",
    );
    let text = output_text(&retry);
    assert!(retry.status.success(), "{text}");
    assert!(text.contains("record=already-recorded"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before);

    // A changed proposal version for the same proposal and target needs a
    // fresh decision: the old consent covered different content (here a
    // newly discovered consumer loss).
    let changed = propose(
        "skill-x+recovery",
        "ev-2",
        "preview-2",
        "consumer list: none known",
    );
    let text = output_text(&changed);
    assert!(changed.status.success(), "{text}");
    assert!(text.contains("record=written"), "{text}");
    let uncovered = removal_check(&board, &h, "experiment");
    let text = output_text(&uncovered);
    assert_eq!(uncovered.status.code(), Some(1), "{text}");
    assert!(text.contains("result=not-covered"), "{text}");
    assert!(text.contains("changed"), "{text}");

    // A mismatched approval loss cannot claim the reviewed scope.
    let before_mismatch = board.comment_texts(&h).len();
    let mismatched = removal_decision(
        &board,
        &h,
        "approve",
        Some("experiment"),
        Some("skill-x"),
        Some("user-turn-8"),
        "approved with a stale loss",
    );
    let text = output_text(&mismatched);
    assert!(!mismatched.status.success(), "{text}");
    assert!(
        text.contains("does not match the recorded proposal"),
        "{text}"
    );
    assert_eq!(board.comment_texts(&h).len(), before_mismatch);
    assert_eq!(
        removal_check(&board, &h, "experiment").status.code(),
        Some(1)
    );

    // Approving the changed content restores authority.
    let reapprove = removal_decision(
        &board,
        &h,
        "approve",
        Some("experiment"),
        Some("skill-x+recovery"),
        Some("user-turn-9"),
        "approved the changed proposal",
    );
    assert!(reapprove.status.success(), "{}", output_text(&reapprove));
    assert_eq!(
        removal_check(&board, &h, "experiment").status.code(),
        Some(0)
    );

    // Changing only the bounded prose detail is also a new reviewed version:
    // identical labels and references cannot retain the old consent.
    let detail_changed = propose(
        "skill-x+recovery",
        "ev-2",
        "preview-2",
        "consumer list: recovery path uses it",
    );
    let text = output_text(&detail_changed);
    assert!(detail_changed.status.success(), "{text}");
    assert!(text.contains("record=written"), "{text}");
    let detail_uncovered = removal_check(&board, &h, "experiment");
    let text = output_text(&detail_uncovered);
    assert_eq!(detail_uncovered.status.code(), Some(1), "{text}");
    assert!(text.contains("result=not-covered"), "{text}");
    assert!(text.contains("changed"), "{text}");

    // An incomplete newer attributable proposal record supersedes the earlier
    // version instead of silently reviving it.
    comment(
        &board,
        &h,
        &format!(
            "removal-proposal v1 item={h} proposal=openspec/changes/remove-x target=skill-x loss=skill-x+recovery"
        ),
    );
    let incomplete = removal_check(&board, &h, "experiment");
    let text = output_text(&incomplete);
    assert_eq!(incomplete.status.code(), Some(1), "{text}");
    assert!(text.contains("result=not-covered"), "{text}");
    assert!(text.contains("incomplete"), "{text}");

    // A fresh complete proposal of the same content plus a fresh explicit
    // decision restores authority.
    let restored = propose(
        "skill-x+recovery",
        "ev-2",
        "preview-2",
        "consumer list: recovery path uses it",
    );
    let text = output_text(&restored);
    assert!(restored.status.success(), "{text}");
    assert!(text.contains("record=written"), "{text}");
    let restored_decision = removal_decision(
        &board,
        &h,
        "approve",
        Some("experiment"),
        Some("skill-x+recovery"),
        Some("user-turn-11"),
        "approved the restored proposal",
    );
    assert!(
        restored_decision.status.success(),
        "{}",
        output_text(&restored_decision)
    );
    assert_eq!(
        removal_check(&board, &h, "experiment").status.code(),
        Some(0)
    );

    // Withdraw, then deliberately repeat the earlier approval: the repeated
    // decision is appended after the withdrawal (never silently skipped
    // against older history), truthfully reported, and becomes current.
    let withdraw = removal_decision(&board, &h, "withdraw", None, None, None, "withdrawn");
    assert!(withdraw.status.success(), "{}", output_text(&withdraw));
    assert_eq!(
        removal_check(&board, &h, "experiment").status.code(),
        Some(1)
    );
    let before_repeat = board.comment_texts(&h).len();
    let repeat = removal_decision(
        &board,
        &h,
        "approve",
        Some("experiment"),
        Some("skill-x+recovery"),
        Some("user-turn-11"),
        "approved the restored proposal",
    );
    let text = output_text(&repeat);
    assert!(repeat.status.success(), "{text}");
    assert!(text.contains("record=written"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before_repeat + 1);
    let authorized = removal_check(&board, &h, "experiment");
    assert_eq!(
        authorized.status.code(),
        Some(0),
        "{}",
        output_text(&authorized)
    );
    // The immediately repeated decision is confirmed, not appended again.
    let confirm = removal_decision(
        &board,
        &h,
        "approve",
        Some("experiment"),
        Some("skill-x+recovery"),
        Some("user-turn-11"),
        "approved the restored proposal",
    );
    let text = output_text(&confirm);
    assert!(confirm.status.success(), "{text}");
    assert!(text.contains("record=already-recorded"), "{text}");
    assert_eq!(board.comment_texts(&h).len(), before_repeat + 1);
    assert_no_votes(&board, &h);
    board.drop();
}

/// Requires the installed OpenSpec CLI and the owner PowerShell: exercises
/// the real reconciliation entry point against an isolated OpenSpec
/// configuration home. Model-free; every artifact lives in the fixture's
/// own temporary directory.
#[test]
#[ignore = "requires installed OpenSpec and owner PowerShell; isolated model-free CLI acceptance"]
fn installed_reconcile_retains_a_rejected_change_without_synchronizing_specs() {
    let board = Board::new("hypothesis-reconcile-installed");
    let configuration = board.root.join("openspec-configuration");
    fs::create_dir_all(&configuration).unwrap();
    let openspec = |args: &[&str]| -> std::process::Output {
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("pwsh");
            command.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-CommandWithArgs",
                "& openspec @args; exit $LASTEXITCODE",
            ]);
            command
        };
        #[cfg(not(windows))]
        let mut command = Command::new("openspec");
        command
            .args(args)
            .current_dir(&board.project)
            .env("APPDATA", &configuration)
            .env("XDG_CONFIG_HOME", &configuration)
            .env("XDG_DATA_HOME", &configuration)
            .env("LOCALAPPDATA", &configuration)
            .env("OPENSPEC_TELEMETRY", "0")
            .output()
            .unwrap()
    };
    // Prove the configuration is isolated before any change is created.
    let resolved = openspec(&["config", "path"]);
    assert!(resolved.status.success(), "{}", output_text(&resolved));
    assert!(
        Path::new(String::from_utf8_lossy(&resolved.stdout).trim()).starts_with(&configuration),
        "{}",
        output_text(&resolved)
    );
    fs::create_dir_all(board.project.join("openspec/changes")).unwrap();
    fs::write(
        board.project.join("openspec/config.yaml"),
        "schema: spec-driven\n",
    )
    .unwrap();
    let created = openspec(&["new", "change", "demo", "--schema", "spec-driven", "--json"]);
    assert!(created.status.success(), "{}", output_text(&created));
    let change = board.project.join("openspec/changes/demo");
    fs::write(
        change.join("proposal.md"),
        "## Why\nA rejected synthetic candidate.\n",
    )
    .unwrap();
    fs::create_dir_all(change.join("specs/synthetic-capability")).unwrap();
    fs::write(
        change.join("specs/synthetic-capability/spec.md"),
        "## ADDED Requirements\n\n### Requirement: Synthetic capability\nThe candidate SHALL behave synthetically.\n\n#### Scenario: Synthetic case\n- **WHEN** the candidate runs\n- **THEN** it behaves synthetically\n",
    )
    .unwrap();
    fs::write(
        change.join("design.md"),
        "## Context\nSynthetic rejection.\n",
    )
    .unwrap();
    fs::write(
        change.join("tasks.md"),
        "## Work\n- [x] Record the experiment outcome.\n- [ ] Restore the accepted runtime.\n",
    )
    .unwrap();
    let main_specs = board.project.join("openspec/specs");
    let main_specs_before = files_under(&main_specs);
    assert!(
        main_specs_before.is_empty(),
        "the delta must start unsynchronized: {main_specs_before:?}"
    );

    let h = admit(&board, "m-installed", "c-installed", "basis-1");
    let rejected = decision(
        &board,
        &h,
        "exp-1",
        "reject",
        "regressed",
        "140",
        true,
        None,
    );
    assert!(rejected.status.success(), "{}", output_text(&rejected));

    let isolated: [(&str, &Path); 5] = [
        ("CODEX_HOME", board.home.as_path()),
        ("APPDATA", configuration.as_path()),
        ("XDG_CONFIG_HOME", configuration.as_path()),
        ("XDG_DATA_HOME", configuration.as_path()),
        ("LOCALAPPDATA", configuration.as_path()),
    ];
    // An unfinished required task is not closable through the outcome: the
    // archive is reported unresolved and the change stays in place.
    let blocked = board.run_feedback(
        &[
            "hypothesis-reconcile",
            "--item",
            &h,
            "--outcome",
            "reject",
            "--change",
            "demo",
            "--action",
            "archive",
        ],
        &isolated,
    );
    let text = output_text(&blocked);
    assert_eq!(blocked.status.code(), Some(1), "{text}");
    assert!(
        text.contains("action=retained archive=unresolved"),
        "{text}"
    );
    assert!(text.contains("Restore the accepted runtime"), "{text}");
    assert!(change.join("tasks.md").is_file(), "{text}");
    assert_eq!(
        files_under(&main_specs),
        main_specs_before,
        "no unadopted delta may synchronize: {text}"
    );

    // Completing the change's own required tasks is the only thing that makes
    // the rejected change archivable; its delta still must not synchronize.
    fs::write(
        change.join("tasks.md"),
        "## Work\n- [x] Record the experiment outcome.\n- [x] Restore the accepted runtime.\n",
    )
    .unwrap();
    let archived = board.run_feedback(
        &[
            "hypothesis-reconcile",
            "--item",
            &h,
            "--outcome",
            "reject",
            "--change",
            "demo",
            "--action",
            "archive",
        ],
        &isolated,
    );
    let text = output_text(&archived);
    assert_eq!(archived.status.code(), Some(0), "{text}");
    assert!(text.contains("action=archived"), "{text}");
    assert!(text.contains("specs-synced=no"), "{text}");
    assert!(!change.exists(), "the change moved to the archive");
    let archive_root = board.project.join("openspec/changes/archive");
    let entries: Vec<PathBuf> = fs::read_dir(&archive_root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
    let archived_change = &entries[0];
    assert!(
        archived_change
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-demo"),
        "{archived_change:?}"
    );
    assert!(archived_change.join("tasks.md").is_file());
    assert!(
        archived_change
            .join("specs/synthetic-capability/spec.md")
            .is_file()
    );
    assert_eq!(
        files_under(&main_specs),
        main_specs_before,
        "the rejected delta reached the main specifications: {text}"
    );

    // The card keeps the rejection and the linked change stays visible with
    // the decision scope prior-result search must report.
    let search = board.feedback(&["hypothesis-search"]);
    let text = output_text(&search);
    assert!(search.status.success(), "{text}");
    assert!(
        text.contains(&format!("hypothesis {h} latest=reject")),
        "{text}"
    );
    assert!(text.contains("scope=task:synthetic"), "{text}");
    board.drop();
}
