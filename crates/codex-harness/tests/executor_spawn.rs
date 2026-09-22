//! Configured executor dispatch into the harness-owned worktree pool. The
//! pool slots are ordinary Git worktrees of a synthetic `file://` upstream, so
//! these checks never touch a real repository, model or subscription; live
//! subscription work stays opt-in.
#![cfg(windows)]

use serde_json::{Value, json};
use std::{
    fs,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

/// Owned temporary checkout of a local `file://` upstream plus a Codex home
/// without an installed launcher, so a dispatch performs its whole pool
/// allocation and stops before any launcher process starts.
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new(name: &str, pool_size: u32) -> Self {
        let root =
            std::env::temp_dir().join(format!("executor-pool-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let bare = root.join("remote.git");
        git(
            &root,
            &[
                "init",
                "--bare",
                "-q",
                "--initial-branch=main",
                bare.to_str().unwrap(),
            ],
        );
        let seed = root.join("seed");
        git(
            &root,
            &[
                "init",
                "-q",
                "--initial-branch=main",
                seed.to_str().unwrap(),
            ],
        );
        configure(&seed);
        fs::write(seed.join(".gitignore"), "cache/\n").unwrap();
        fs::write(seed.join("README.md"), "seed\n").unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-qm", "seed"]);
        git(&seed, &["remote", "add", "origin", &file_url(&bare)]);
        git(&seed, &["push", "-q", "origin", "main"]);
        // The source checkout is the repository dispatch runs against; the
        // author clone advances upstream between dispatches.
        let source = root.join("proj");
        git(
            &root,
            &["clone", "-q", &file_url(&bare), source.to_str().unwrap()],
        );
        configure(&source);
        fs::create_dir_all(source.join("global")).unwrap();
        fs::write(
            source.join("global/orchestration.toml"),
            orchestration(pool_size),
        )
        .unwrap();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("ds.config.toml"), "model = 'deepseek-flash'\n").unwrap();
        let author = root.join("author");
        git(
            &root,
            &["clone", "-q", &file_url(&bare), author.to_str().unwrap()],
        );
        configure(&author);
        Self { root, source, home }
    }

    fn slot(&self, index: u32) -> PathBuf {
        self.root.join(format!("proj-wt{index}"))
    }

    fn spawn(&self, extra: &[&str]) -> std::process::Output {
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "spawn",
            "--source",
            self.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
            "--profile",
            "ds",
            "--exec",
            "assignment text",
        ]);
        command.args(extra);
        command.output().unwrap()
    }

    fn resume(&self, extra: &[&str]) -> std::process::Output {
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "resume",
            "--source",
            self.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
            "--slot",
            "1",
            "--owner",
            "exec-ds-52",
            "--session",
            "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4",
            "--exec",
            "continue the interrupted assignment",
        ]);
        command.args(extra);
        command.output().unwrap()
    }

    fn release(&self, extra: &[&str]) -> std::process::Output {
        let mut command = Command::new(manager());
        command.args([
            "executor",
            "release",
            "--source",
            self.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
        ]);
        command.args(extra);
        command.output().unwrap()
    }

    fn pool(&self) -> String {
        let out = Command::new(manager())
            .args([
                "executor",
                "pool",
                "--source",
                self.source.to_str().unwrap(),
                "--codex-home",
                self.home.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
        );
        output_text(&out)
    }

    /// The recorded slot mapping, loaded from the kit-local task state exactly
    /// as the lead's read path does.
    fn record(&self, index: u32) -> Value {
        serde_json::from_slice(&fs::read(self.record_path(index)).unwrap()).unwrap()
    }

    fn record_path(&self, index: u32) -> PathBuf {
        let state = self.home.join("harness/executor-pool");
        let source = fs::read_dir(&state)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.is_dir())
            .expect("one pool state directory per source checkout");
        source.join(format!("slot-{index}.json"))
    }

    /// Registered checkout names of the source, so slot reuse is checked
    /// against `git worktree list` instead of the harness's own bookkeeping.
    fn checkouts(&self) -> Vec<String> {
        let mut names: Vec<String> = git_output(&self.source, &["worktree", "list", "--porcelain"])
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(|path| {
                Path::new(path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    fn advance_upstream(&self, file: &str) -> String {
        let author = self.root.join("author");
        fs::write(author.join(file), "upstream moved on\n").unwrap();
        git(&author, &["add", file]);
        git(&author, &["commit", "-qm", &format!("update {file}")]);
        git(&author, &["push", "-q", "origin", "main"]);
        git_output(&author, &["rev-parse", "HEAD"])
    }

    /// The lead's merge: a commit in the source checkout that becomes the base
    /// a released slot is reset to.
    fn merge_into_source(&self, file: &str) -> String {
        fs::write(self.source.join(file), "merged assignment\n").unwrap();
        git(&self.source, &["add", file]);
        git(&self.source, &["commit", "-qm", &format!("merge {file}")]);
        git_output(&self.source, &["rev-parse", "HEAD"])
    }

    fn drop(self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn orchestration(pool_size: u32) -> String {
    format!(
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = {pool_size}\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\n# Superseded by the pool size; accepted for compatibility.\nworktree_limit = 1\n"
    )
}

fn output_text(out: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn file_url(path: &Path) -> String {
    format!("file:///{}", path.to_str().unwrap().replace('\\', "/"))
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
fn configured_executor_rejects_unlisted_profile() {
    let root = std::env::temp_dir().join(format!("executor-spawn-{}", std::process::id()));
    let source = root.join("source");
    let home = root.join("home");
    let workspace = root.join("workspace");
    fs::create_dir_all(source.join("global")).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    fs::write(
        source.join("global/orchestration.toml"),
        include_str!("../../../global/orchestration.toml"),
    )
    .unwrap();
    let out = Command::new(manager())
        .args([
            "executor",
            "spawn",
            "--source",
            source.to_str().unwrap(),
            "--codex-home",
            home.to_str().unwrap(),
            "--workspace",
            workspace.to_str().unwrap(),
            "--profile",
            "zai",
            "--exec",
            "unused",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("is not an executor"), "{err}");
    assert!(!err.to_lowercase().contains("substitut"), "{err}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn pooled_spawn_derives_and_synchronizes_a_slot_before_launch() {
    let fixture = Fixture::new("derive", 1);
    let upstream = fixture.advance_upstream("upstream.txt");
    assert_ne!(
        git_output(&fixture.source, &["rev-parse", "HEAD"]),
        upstream,
        "the fixture must start behind its upstream"
    );
    let out = fixture.spawn(&[]);
    let text = output_text(&out);
    assert!(!out.status.success());
    // The dispatch stops at the launcher, after the pool work: the derived
    // slot, its synchronized base and the session binding are already visible.
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let slot = fixture.slot(1);
    assert!(
        text.contains(&format!("executor slot: index=1 path={}", slot.display()))
            || text.contains("executor slot: index=1 path="),
        "{text}"
    );
    assert!(text.contains(&format!("base={upstream}")), "{text}");
    assert!(text.contains("owner=exec-ds-"), "{text}");
    assert!(slot.is_dir(), "{text}");
    assert_eq!(
        git_output(&slot, &["rev-parse", "HEAD"]),
        upstream,
        "the slot reaches the fetched upstream base before the first model request"
    );
    assert!(
        git_output(
            &slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        )
        .is_empty(),
        "a synchronized slot starts clean"
    );
    // Native isolation flags and the retired warning path are gone: the
    // worktrees feature is not enabled in this Codex home.
    assert!(!text.contains("--worktree"), "{text}");
    assert!(!text.contains("managed worktrees unavailable"), "{text}");
    assert!(!text.contains("worktree warning"), "{text}");
    assert_eq!(fixture.checkouts(), ["proj", "proj-wt1"]);
    let record = fixture.record(1);
    assert_eq!(record["state"], "occupied");
    assert_eq!(record["base"], upstream.as_str());
    assert_eq!(record["index"], 1);
    assert!(
        record["owner"].as_str().unwrap().starts_with("exec-ds-"),
        "{record}"
    );
    assert!(
        record["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("proj-wt1"),
        "{record}"
    );
    fixture.drop();
}

#[test]
fn a_live_session_host_keeps_its_slot_from_other_dispatches() {
    let fixture = Fixture::new("liveness", 1);
    assert!(!fixture.spawn(&["--owner", "exec-live"]).status.success());
    let record = fixture.record(1);
    let slot = fixture.slot(1);
    // A stand-in for the tab host: `executor run --file` is the process that
    // stays alive for the session, so it records the slot's liveness. The
    // command it runs only keeps that host alive for a few seconds.
    let receipt = fixture.root.join("host-receipt.json");
    fs::write(
        &receipt,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "launcher": r"C:\Windows\System32\cmd.exe",
            "args": ["/c", "ping -n 4 127.0.0.1 >nul"],
            "slot": {
                "index": 1,
                "path": slot,
                "source": fixture.source,
                "owner": "exec-live",
                "base": record["base"],
                "remote": "origin",
                "branch": "main",
            },
        }))
        .unwrap(),
    )
    .unwrap();
    let mut host = Command::new(manager())
        .args(["executor", "run", "--file", receipt.to_str().unwrap()])
        .env("CODEX_HOME", &fixture.home)
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let listed = loop {
        let listed = fixture.pool();
        if listed.contains("lease=live") {
            break listed;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the session host never recorded a live lease: {listed}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(listed.contains("state=occupied"), "{listed}");
    // A live session keeps its slot: another identity cannot take it and the
    // same identity is never dispatched into a second session.
    let other = fixture.spawn(&["--owner", "exec-other"]);
    let text = output_text(&other);
    assert!(text.contains("no free slot in the pool of 1"), "{text}");
    assert!(
        text.contains("occupied by live session exec-live"),
        "{text}"
    );
    let same = fixture.spawn(&["--owner", "exec-live"]);
    assert!(
        output_text(&same).contains("is already live in slot 1"),
        "{}",
        output_text(&same)
    );
    assert!(host.wait().unwrap().success());
    // The host exited: the same slot is reclaimable instead of multiplying.
    let next = fixture.spawn(&["--owner", "exec-other"]);
    assert!(!next.status.success());
    assert_eq!(fixture.record(1)["owner"], "exec-other");
    assert_eq!(fixture.checkouts(), ["proj", "proj-wt1"]);
    fixture.drop();
}

#[test]
fn resume_rebinds_the_interrupted_slot_without_resetting_partial_work() {
    let fixture = Fixture::new("resume", 1);
    let spawned = fixture.spawn(&["--owner", "exec-ds-52"]);
    let text = output_text(&spawned);
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let slot = fixture.slot(1);
    let head = git_output(&slot, &["rev-parse", "HEAD"]);
    // The interrupted session leaves partial work in its slot; after its host
    // is gone, reconciliation clears the recorded owner.
    let partial = slot.join("partial-work.txt");
    fs::write(&partial, "partial work\n").unwrap();
    let pool = fixture.pool();
    assert!(pool.contains("owner=exec-ds-52"), "{pool}");
    // Resume adopts the reconciled slot for the same owner and the exact
    // session without fetch, reset or clean.
    let resumed = fixture.resume(&[]);
    let text = output_text(&resumed);
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    assert!(text.contains("owner=exec-ds-52"), "{text}");
    assert!(partial.is_file(), "resume must preserve partial work");
    assert_eq!(
        git_output(&slot, &["rev-parse", "HEAD"]),
        head,
        "resume must not resynchronize the slot"
    );
    let record = fixture.record(1);
    assert_eq!(record["state"], "occupied");
    assert_eq!(record["owner"], "exec-ds-52");
    assert_eq!(record["base"], head.as_str());
    // The same owner resumes repeatedly after further interruptions.
    let again = fixture.resume(&[]);
    assert!(
        output_text(&again).contains("owner=exec-ds-52"),
        "{}",
        output_text(&again)
    );
    assert!(partial.is_file());
    // Another owner is refused with both identities named, and the tree keeps
    // its partial work.
    let other = fixture.resume(&["--owner", "exec-ds-9"]);
    let text = output_text(&other);
    assert!(!other.status.success(), "{text}");
    assert!(
        text.contains("bound to session exec-ds-52 instead of exec-ds-9"),
        "{text}"
    );
    assert!(partial.is_file());
    assert_eq!(fixture.record(1)["owner"], "exec-ds-52");
    fixture.drop();
}

#[test]
fn pooled_spawn_accepts_the_source_checkout_and_refuses_ad_hoc_worktree_paths() {
    let fixture = Fixture::new("workspace", 2);
    let lane = fixture.root.join("task-legacy-lane");
    fs::create_dir_all(&lane).unwrap();
    let refused = fixture.spawn(&["--workspace", lane.to_str().unwrap()]);
    let text = output_text(&refused);
    assert!(!refused.status.success());
    assert!(
        text.contains("executor isolation comes from the harness pool")
            && text.contains("--workspace must be the source checkout"),
        "{text}"
    );
    assert!(
        !fixture.slot(1).exists(),
        "a refused workspace must not allocate a slot: {text}"
    );
    let source_workspace = fixture.spawn(&["--workspace", fixture.source.to_str().unwrap()]);
    let text = output_text(&source_workspace);
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    assert!(fixture.slot(1).is_dir(), "{text}");
    assert_eq!(fixture.checkouts(), ["proj", "proj-wt1"]);
    fixture.drop();
}

#[test]
fn pooled_dispatch_records_the_mapping_and_reuses_the_same_slot_after_interruption() {
    let fixture = Fixture::new("reuse", 1);
    let base = git_output(&fixture.source, &["rev-parse", "HEAD"]);
    let first = fixture.spawn(&["--owner", "exec-1"]);
    assert!(
        !first.status.success(),
        "the launcher is intentionally absent"
    );
    let record = fixture.record(1);
    assert_eq!(record["state"], "occupied");
    assert_eq!(record["owner"], "exec-1");
    assert_eq!(record["base"], base.as_str());
    // The mapping is exposed through the pool read path after the interruption.
    let listed = fixture.pool();
    assert!(listed.contains("slot 1"), "{listed}");
    assert!(listed.contains("state=occupied"), "{listed}");
    assert!(listed.contains("owner=exec-1"), "{listed}");
    assert!(listed.contains(&format!("base={base}")), "{listed}");
    // The same session reclaims its slot instead of allocating a new tree.
    let second = fixture.spawn(&["--owner", "exec-1"]);
    assert!(!second.status.success());
    assert_eq!(fixture.checkouts(), ["proj", "proj-wt1"]);
    assert_eq!(fixture.record(1)["owner"], "exec-1");
    // A later session reuses the same slot path as well.
    let third = fixture.spawn(&["--owner", "exec-2"]);
    assert!(!third.status.success());
    assert_eq!(fixture.checkouts(), ["proj", "proj-wt1"]);
    let record = fixture.record(1);
    assert_eq!(record["state"], "occupied");
    assert_eq!(record["owner"], "exec-2");
    fixture.drop();
}

#[test]
fn exhausted_pool_aborts_with_a_concrete_cause_and_no_new_tree() {
    let fixture = Fixture::new("exhausted", 2);
    assert!(!fixture.spawn(&["--owner", "exec-1"]).status.success());
    fs::write(fixture.slot(1).join("work.txt"), "unreviewed work\n").unwrap();
    assert!(!fixture.spawn(&["--owner", "exec-2"]).status.success());
    assert!(fixture.slot(2).is_dir());
    fs::write(fixture.slot(2).join("work.txt"), "unreviewed work\n").unwrap();
    let third = fixture.spawn(&["--owner", "exec-3"]);
    let text = output_text(&third);
    assert!(!third.status.success());
    assert!(text.contains("no free slot in the pool of 2"), "{text}");
    assert!(
        text.contains("slot 1 holds local or untracked changes"),
        "{text}"
    );
    assert!(
        text.contains("slot 2 holds local or untracked changes"),
        "{text}"
    );
    assert!(
        text.contains("merge or explicitly discard them before reuse"),
        "{text}"
    );
    assert!(
        !text.contains("worktree warning"),
        "the limit warning is replaced by the pool-invariant refusal: {text}"
    );
    assert_eq!(fixture.checkouts(), ["proj", "proj-wt1", "proj-wt2"]);
    assert!(
        !fixture.slot(3).exists(),
        "an exhausted pool never registers another tree"
    );
    // Preserved slots report their reason instead of being silently reused.
    let listed = fixture.pool();
    assert!(listed.contains("state=awaiting-review"), "{listed}");
    assert!(listed.contains("tree=dirty"), "{listed}");
    fixture.drop();
}

#[test]
fn released_slot_records_its_disposition_and_re_enters_the_pool() {
    let fixture = Fixture::new("release", 1);
    assert!(!fixture.spawn(&["--owner", "exec-1"]).status.success());
    let slot = fixture.slot(1);
    fs::create_dir_all(slot.join("cache")).unwrap();
    fs::write(slot.join("cache/artifact"), "warm\n").unwrap();
    let merged = fixture.merge_into_source("merged.txt");
    let out = fixture.release(&[
        "--slot",
        "1",
        "--disposition",
        "merged",
        "--reason",
        "accepted assignment",
    ]);
    let text = output_text(&out);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("released as merged"), "{text}");
    assert!(text.contains(&merged), "{text}");
    assert_eq!(git_output(&slot, &["rev-parse", "HEAD"]), merged);
    assert!(
        slot.join("cache/artifact").is_file(),
        "ignored build caches stay warm across slot release"
    );
    assert!(
        git_output(
            &slot,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        )
        .is_empty(),
        "a released slot is clean at the merged base"
    );
    let record = fixture.record(1);
    assert_eq!(record["state"], "released");
    assert_eq!(record["disposition"], "merged");
    assert_eq!(record["reason"], "accepted assignment");
    // The released slot serves the next dispatch instead of multiplying trees.
    assert!(!fixture.spawn(&["--owner", "exec-2"]).status.success());
    assert_eq!(fixture.checkouts(), ["proj", "proj-wt1"]);
    let record = fixture.record(1);
    assert_eq!(record["state"], "occupied");
    assert_eq!(record["owner"], "exec-2");
    fixture.drop();
}

#[test]
fn preserved_slot_reports_its_limitation_and_stays_out_of_the_pool() {
    let fixture = Fixture::new("preserve", 1);
    assert!(!fixture.spawn(&["--owner", "exec-1"]).status.success());
    let slot = fixture.slot(1);
    let locked = slot.join("locked.txt");
    fs::write(&locked, "held by the lead\n").unwrap();
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&locked)
        .unwrap();
    let out = fixture.release(&[
        "--slot",
        "1",
        "--disposition",
        "discarded",
        "--reason",
        "rejected assignment",
    ]);
    drop(handle);
    let text = output_text(&out);
    assert_eq!(out.status.code(), Some(2), "{text}");
    assert!(text.contains("preserved"), "{text}");
    assert!(
        text.contains("preserving the lane for retirement"),
        "the release reports why the slot stays out of the pool: {text}"
    );
    let record = fixture.record(1);
    assert_eq!(record["state"], "awaitingReview");
    assert_eq!(record["disposition"], "discarded");
    // The lead's recorded disposition survives; the preserved reason is the
    // limitation that kept the slot out of the pool.
    assert!(
        record["reason"]
            .as_str()
            .unwrap()
            .contains("preserving the lane for retirement"),
        "{record}"
    );
    // The preserved slot is reported and never selected or reset.
    let listed = fixture.pool();
    assert!(listed.contains("state=awaiting-review"), "{listed}");
    assert!(listed.contains("disposition=discarded"), "{listed}");
    assert!(locked.is_file(), "unreviewed work is preserved");
    let next = fixture.spawn(&["--owner", "exec-2"]);
    let text = output_text(&next);
    assert!(text.contains("no free slot in the pool of 1"), "{text}");
    assert!(locked.is_file(), "the preserved slot is not reset: {text}");
    fixture.drop();
}

#[test]
fn pool_reports_foreign_and_beyond_pool_trees_for_lead_review() {
    let fixture = Fixture::new("inventory", 2);
    let legacy = fixture.root.join("task-legacy-lane");
    git(
        &fixture.source,
        &[
            "worktree",
            "add",
            "--detach",
            legacy.to_str().unwrap(),
            "HEAD",
        ],
    );
    let beyond = fixture.root.join("proj-wt3");
    git(
        &fixture.source,
        &[
            "worktree",
            "add",
            "--detach",
            beyond.to_str().unwrap(),
            "HEAD",
        ],
    );
    let listed = fixture.pool();
    assert!(
        listed.contains(&format!(
            "foreign worktree (lead review): {}",
            legacy.display()
        )),
        "{listed}"
    );
    assert!(
        listed.contains(&format!(
            "worktree beyond the configured pool (lead review): {}",
            beyond.display()
        )),
        "{listed}"
    );
    // Dispatch reports them without adopting or deleting them.
    let spawn = fixture.spawn(&["--owner", "exec-1"]);
    let text = output_text(&spawn);
    assert!(text.contains("pool inventory (lead review)"), "{text}");
    assert!(legacy.is_dir() && beyond.is_dir());
    assert!(
        fixture.checkouts().contains(&"proj-wt1".to_owned()),
        "{:?}",
        fixture.checkouts()
    );
    assert!(
        fixture.record(1)["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("proj-wt1")
    );
    fixture.drop();
}

#[test]
#[ignore = "requires HARNESS_LIVE_CODEX_HOME with xai profile and subscription; runs one real assignment in a pool slot of this checkout"]
fn configured_xai_executor_serves_a_subscribed_tool_from_its_pool_slot() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let home = PathBuf::from(std::env::var_os("HARNESS_LIVE_CODEX_HOME").expect("live Codex home"));
    assert!(home.is_absolute());
    let source = source.canonicalize().unwrap();
    let mut child = Command::new(manager());
    child
        .args([
            "executor",
            "spawn",
            "--source",
            source.to_str().unwrap(),
            "--codex-home",
            home.to_str().unwrap(),
            "--owner",
            "exec-xai-live-probe",
            "--exec",
            "Create proof.txt containing exactly orch-xai. Use a local shell tool. Do not spawn agents.",
        ]);
    let out = child.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "executor spawn failed: {stderr}{stdout}"
    );
    // The recorded slot mapping is the read path for the bound checkout, and
    // its receipt carries the dispatched session.
    let slot = pooled_slot_path(&home);
    assert!(stdout.contains("executor slot: index="), "{stdout}");
    let receipt: Value =
        serde_json::from_slice(&fs::read(pooled_receipt_path(&home, 1)).unwrap()).unwrap();
    assert!(
        !slot.join("executor-spawn.json").exists(),
        "the dispatch receipt is kit-local state, not executor work"
    );
    assert_eq!(receipt["profile"], "xai");
    assert_eq!(receipt["args"][0], "--profile");
    assert_eq!(receipt["args"][1], "xai");
    assert_eq!(receipt["model"], "grok-4.6");
    assert_eq!(receipt["modelProvider"], "xai");
    assert_eq!(receipt["reasoningEffort"], "xhigh");
    assert_eq!(receipt["visible"], true);
    assert_eq!(receipt["slot"]["owner"], "exec-xai-live-probe");
    assert_eq!(receipt["slot"]["index"], 1);
    assert_eq!(receipt["slot"]["path"], slot.to_str().unwrap());
    assert!(
        !receipt["args"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg == "exec" || arg == "--json"),
        "visible spawn must not be headless exec: {receipt}"
    );
    assert_eq!(
        fs::read_to_string(slot.join("proof.txt")).unwrap().trim(),
        "orch-xai"
    );
    let _ = Duration::from_secs(1);
}

/// Slot path of the first recorded pool slot, read from the kit-local state
/// that `executor pool` also reports.
fn pooled_state_dir(home: &Path) -> PathBuf {
    let state = home.join("harness/executor-pool");
    fs::read_dir(&state)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_dir())
        .expect("pool state directory")
}

fn pooled_slot_path(home: &Path) -> PathBuf {
    let record: Value =
        serde_json::from_slice(&fs::read(pooled_state_dir(home).join("slot-1.json")).unwrap())
            .unwrap();
    PathBuf::from(record["path"].as_str().unwrap())
}

fn pooled_receipt_path(home: &Path, index: u32) -> PathBuf {
    pooled_state_dir(home).join(format!("spawn-{index}.json"))
}
