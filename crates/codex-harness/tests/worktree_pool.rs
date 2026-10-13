//! Native `worktree` lifecycle checks: one repository's shared pool of Git
//! worktrees for ordinary tasks, exercised through the compiled CLI against a
//! synthetic `file://` upstream. The checks never touch a real repository,
//! model or subscription; executor dispatches stop at the missing launcher
//! after the pool work, so no conversation or model request starts.
#![cfg(windows)]

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

/// A command that does not inherit the caller's session identity, so the
/// checks behave like the lead's own shell even when the suite runs inside an
/// executor conversation, where nested dispatch is refused.
fn lead_command() -> Command {
    let mut command = Command::new(manager());
    command.env_remove("HARNESS_EXECUTOR_SESSION");
    command.env_remove("CODEX_THREAD_ID");
    command.env_remove("CODEX_SESSION_ID");
    command.env_remove("HARNESS_EXECUTOR_RUN");
    command.env_remove("HARNESS_ORIGINATING_LEAD");
    command.env_remove("HARNESS_LEAD_THREAD");
    command.env_remove("HARNESS_LEAD_RECIPIENT");
    command
}

/// Owned temporary kit checkout (`proj`) on a local `file://` upstream plus a
/// Codex home without an installed launcher. Additional clones of the same
/// upstream act as independent target repositories that carry no kit
/// configuration of their own.
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new(name: &str, pool_size: u32, concurrency: u32) -> Self {
        let root =
            std::env::temp_dir().join(format!("worktree-pool-{name}-{}", std::process::id()));
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
        let source = root.join("proj");
        git(
            &root,
            &["clone", "-q", &file_url(&bare), source.to_str().unwrap()],
        );
        configure(&source);
        fs::create_dir_all(source.join("global")).unwrap();
        fs::write(
            source.join("global/orchestration.toml"),
            orchestration(pool_size, concurrency),
        )
        .unwrap();
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("ds.config.toml"), "model = 'deepseek-flash'\n").unwrap();
        Self { root, source, home }
    }

    /// An independent clone of the same upstream: a consuming repository with
    /// no kit configuration of its own.
    fn consumer(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        git(
            &self.root,
            &[
                "clone",
                "-q",
                &file_url(&self.root.join("remote.git")),
                path.to_str().unwrap(),
            ],
        );
        configure(&path);
        path
    }

    fn slot(&self, index: u32) -> PathBuf {
        self.root.join(format!("proj-wt{index}"))
    }

    fn worktree(&self, extra: &[&str]) -> Output {
        let mut command = lead_command();
        command.args(["worktree"]);
        command.args(extra);
        command.args([
            "--source",
            self.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
        ]);
        command.output().unwrap()
    }

    fn executor(&self, extra: &[&str]) -> Output {
        let mut command = lead_command();
        command.args(["executor"]);
        command.args(extra);
        command.args([
            "--source",
            self.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
        ]);
        command.env("CODEX_THREAD_ID", "lead-thread-synthetic");
        command.output().unwrap()
    }

    fn spawn(&self, extra: &[&str]) -> Output {
        let mut args = vec!["spawn", "--profile", "ds", "--exec", "assignment text"];
        args.extend_from_slice(extra);
        self.executor(&args)
    }

    /// The recorded slot mapping, loaded from the kit-local task state exactly
    /// as the lead's read path does.
    fn record(&self, repository: &Path, index: u32) -> Value {
        let state = self.home.join("harness/executor-pool");
        let canonical = repository.canonicalize().unwrap();
        for entry in fs::read_dir(&state).unwrap() {
            let candidate = entry.unwrap().path().join(format!("slot-{index}.json"));
            if !candidate.is_file() {
                continue;
            }
            let value: Value = serde_json::from_slice(&fs::read(&candidate).unwrap()).unwrap();
            let source = PathBuf::from(value["source"].as_str().unwrap());
            if source.canonicalize().unwrap() == canonical {
                return value;
            }
        }
        panic!("no slot-{index}.json for {}", repository.display());
    }

    fn handle(&self, repository: &Path, index: u32) -> String {
        self.record(repository, index)["handle"]
            .as_str()
            .unwrap_or_else(|| panic!("slot {index} has no generation handle"))
            .to_owned()
    }

    /// The kit-local pool state directory of one repository's canonical
    /// identity, discovered the same way the CLI resolves it.
    fn state_dir(&self, repository: &Path) -> PathBuf {
        let state = self.home.join("harness/executor-pool");
        let canonical = repository.canonicalize().unwrap();
        for entry in fs::read_dir(&state).unwrap() {
            let directory = entry.unwrap().path();
            let candidate = directory.join("slot-1.json");
            if !candidate.is_file() {
                continue;
            }
            let value: Value = serde_json::from_slice(&fs::read(&candidate).unwrap()).unwrap();
            if PathBuf::from(value["source"].as_str().unwrap())
                .canonicalize()
                .unwrap()
                == canonical
            {
                return directory;
            }
        }
        panic!("no pool state directory for {}", repository.display());
    }

    /// A live canned host for one slot, owned by this test process: the
    /// existing lease-liveness rule then reports that run as active.
    fn record_live_lease(&self, repository: &Path, index: u32, owner: &str) {
        let program = std::env::current_exe().unwrap();
        let user = harness_core::process_service::current_user().unwrap();
        let identity = harness_core::process_service::ServiceProcess::observe(
            std::process::id(),
            &program,
            0,
            &user,
        )
        .unwrap()
        .identity();
        fs::write(
            self.state_dir(repository)
                .join(format!("lease-{index}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema": 1,
                "owner": owner,
                "index": index,
                "path": self.slot(index).to_string_lossy(),
                "pid": identity.pid,
                "created": identity.creation_time,
                "program": program.to_string_lossy()
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn remove_lease(&self, repository: &Path, index: u32) {
        let _ = fs::remove_file(
            self.state_dir(repository)
                .join(format!("lease-{index}.json")),
        );
    }

    /// Registered checkout names, so slot reuse is checked against
    /// `git worktree list` instead of the harness's own bookkeeping.
    fn checkouts(&self, repository: &Path) -> Vec<String> {
        let mut names: Vec<String> = git_output(repository, &["worktree", "list", "--porcelain"])
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

    /// The lead's merge: a commit in the source checkout that becomes the base
    /// a released slot is reset to.
    fn merge_into_source(&self, file: &str, content: &str) -> String {
        fs::write(self.source.join(file), content).unwrap();
        git(&self.source, &["add", file]);
        git(&self.source, &["commit", "-qm", &format!("merge {file}")]);
        git_output(&self.source, &["rev-parse", "HEAD"])
    }

    fn status(&self, checkout: &Path) -> String {
        git_output(
            checkout,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        )
    }

    fn drop(self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn orchestration(pool_size: u32, concurrency: u32) -> String {
    format!(
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = {concurrency}\nworktree_pool_size = {pool_size}\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\n# Superseded by the pool size; accepted for compatibility.\nworktree_limit = 1\n"
    )
}

fn output_text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn succeeded(out: Output) -> String {
    assert!(out.status.success(), "{}", output_text(&out));
    output_text(&out)
}

fn refused(out: Output) -> String {
    assert!(!out.status.success(), "{}", output_text(&out));
    output_text(&out)
}

fn file_url(path: &Path) -> String {
    format!("file:///{}", path.to_str().unwrap().replace('\\', "/"))
}

fn configure(cwd: &Path) {
    git(cwd, &["config", "user.email", "worktree@example.test"]);
    git(cwd, &["config", "user.name", "Worktree"]);
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
fn ordinary_task_lifecycle_reuses_the_same_checkout_and_keeps_ignored_caches() {
    let fixture = Fixture::new("lifecycle", 4, 8);
    let head = git_output(&fixture.source, &["rev-parse", "HEAD"]);
    let acquire = succeeded(fixture.worktree(&["acquire", "--task", "t1"]));
    let slot = fixture.slot(1);
    assert!(
        acquire.contains(&format!(
            "worktree pool: repository={} identity={} source={} capacity=4",
            fixture.source.display(),
            fixture.source.display(),
            fixture.source.display()
        )),
        "{acquire}"
    );
    assert!(
        acquire.contains(&format!(
            "worktree acquire: slot=1 path={} presence=registered state=held base={head} task=t1",
            slot.display()
        )),
        "{acquire}"
    );
    let handle = fixture.handle(&fixture.source, 1);
    assert!(
        acquire.contains(&format!("handle={handle} owner=t1 ")),
        "{acquire}"
    );
    // List reports the same durable binding without touching it.
    let list = succeeded(fixture.worktree(&["list"]));
    assert!(
        list.contains(&format!(
            "worktree list: slot=1 path={} presence=registered state=held base={head} task=t1 handle={handle} owner=t1",
            slot.display()
        )),
        "{list}"
    );
    assert!(
        list.contains(&format!(
            "worktree list: slot=4 path={} presence=absent state=free base=- task=- handle=- owner=-",
            fixture.slot(4).display()
        )),
        "{list}"
    );
    assert!(!list.contains("foreign worktree"), "{list}");
    // Continue returns the held binding and never resets it.
    let continued = succeeded(fixture.worktree(&["continue", "--task", "t1"]));
    assert!(
        continued.contains(&format!(
            "worktree continue: slot=1 path={} presence=registered state=held base={head} task=t1 handle={handle} owner=t1",
            slot.display()
        )),
        "{continued}"
    );
    // Partial work and an ignored build cache live in the slot.
    fs::write(slot.join("work.txt"), "task work\n").unwrap();
    fs::create_dir_all(slot.join("cache")).unwrap();
    fs::write(slot.join("cache/artifact.bin"), "cached build output\n").unwrap();
    git(&slot, &["add", "work.txt"]);
    git(&slot, &["commit", "-qm", "task work"]);
    let partial = git_output(&slot, &["rev-parse", "HEAD"]);
    let continued = succeeded(fixture.worktree(&["continue", "--task", "t1", "--handle", &handle]));
    assert!(continued.contains("state=held"), "{continued}");
    assert!(slot.join("work.txt").is_file());
    assert_eq!(git_output(&slot, &["rev-parse", "HEAD"]), partial);
    // The lead integrates the task result: the source advances, and the
    // merged release resets the slot to the integrated commit.
    let merged = fixture.merge_into_source("work.txt", "task work\n");
    assert_ne!(merged, head);
    let release = succeeded(fixture.worktree(&[
        "release",
        "--task",
        "t1",
        "--disposition",
        "merged",
        "--reason",
        "integrated into the source checkout",
    ]));
    assert!(
        release.contains(&format!(
            "worktree release: task=t1 handle={handle} disposition=merged released; slot=1 path={} reset to {merged} and free for the next task",
            slot.display()
        )),
        "{release}"
    );
    assert_eq!(git_output(&slot, &["rev-parse", "HEAD"]), merged);
    assert!(
        fixture.status(&slot).is_empty(),
        "the released slot is clean"
    );
    assert!(
        slot.join("cache/artifact.bin").is_file(),
        "the ignored cache survives the reuse reset"
    );
    let list = succeeded(fixture.worktree(&["list"]));
    assert!(
        list.contains(&format!(
            "worktree list: slot=1 path={} presence=registered state=free",
            slot.display()
        )),
        "{list}"
    );
    assert!(list.contains("disposition=merged"), "{list}");
    // A later task reuses the same physical position; its discarded release
    // removes the changes and keeps the ignored cache.
    let acquire = succeeded(fixture.worktree(&["acquire", "--task", "t2"]));
    assert!(
        acquire.contains(&format!(
            "worktree acquire: slot=1 path={} presence=registered state=held base={merged} task=t2",
            slot.display()
        )),
        "{acquire}"
    );
    fs::write(slot.join("scratch.txt"), "discard me\n").unwrap();
    let release = succeeded(fixture.worktree(&[
        "release",
        "--task",
        "t2",
        "--disposition",
        "discarded",
        "--reason",
        "superseded experiment",
    ]));
    assert!(
        release.contains("disposition=discarded released"),
        "{release}"
    );
    assert!(!slot.join("scratch.txt").exists());
    assert!(slot.join("cache/artifact.bin").is_file());
    assert_eq!(git_output(&slot, &["rev-parse", "HEAD"]), merged);
    fixture.drop();
}

#[test]
fn linked_checkout_resolves_the_canonical_pool_identity() {
    let fixture = Fixture::new("linked", 4, 8);
    let head = git_output(&fixture.source, &["rev-parse", "HEAD"]);
    let linked = fixture.root.join("linked");
    git(
        &fixture.source,
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    let acquire = succeeded(fixture.worktree(&[
        "acquire",
        "--task",
        "t1",
        "--repository",
        linked.to_str().unwrap(),
    ]));
    let slot = fixture.slot(1);
    assert!(
        acquire.contains(&format!(
            "worktree pool: repository={} identity={} source={} capacity=4",
            linked.display(),
            fixture.source.display(),
            fixture.source.display()
        )),
        "{acquire}"
    );
    assert!(
        acquire.contains(&format!("worktree acquire: slot=1 path={}", slot.display())),
        "{acquire}"
    );
    assert!(acquire.contains(&format!("base={head}")), "{acquire}");
    // The primary checkout reports the same binding and never adopts the
    // linked checkout as a pool position.
    let list = succeeded(fixture.worktree(&["list"]));
    assert!(
        list.contains(&format!(
            "worktree list: slot=1 path={} presence=registered state=held base={head} task=t1",
            slot.display()
        )),
        "{list}"
    );
    assert!(
        list.contains(&format!(
            "foreign worktree (lead review): {}",
            linked.display()
        )),
        "{list}"
    );
    // Continue through the linked checkout returns the same binding, and the
    // release through it frees the position for the primary checkout.
    let handle = fixture.handle(&fixture.source, 1);
    let continued = succeeded(fixture.worktree(&[
        "continue",
        "--task",
        "t1",
        "--repository",
        linked.to_str().unwrap(),
    ]));
    assert!(
        continued.contains(&format!(
            "worktree continue: slot=1 path={}",
            slot.display()
        )),
        "{continued}"
    );
    assert!(
        continued.contains(&format!("handle={handle}")),
        "{continued}"
    );
    let release = succeeded(fixture.worktree(&[
        "release",
        "--task",
        "t1",
        "--disposition",
        "discarded",
        "--reason",
        "linked checkout release",
        "--repository",
        linked.to_str().unwrap(),
    ]));
    assert!(
        release.contains("disposition=discarded released"),
        "{release}"
    );
    assert_eq!(git_output(&slot, &["rev-parse", "HEAD"]), head);
    fixture.drop();
}

#[test]
fn explicit_pool_capacity_gates_acquisitions_apart_from_executor_concurrency() {
    let fixture = Fixture::new("capacity", 2, 8);
    let first = succeeded(fixture.worktree(&["acquire", "--task", "t1"]));
    assert!(first.contains("capacity=2"), "{first}");
    assert!(first.contains("worktree acquire: slot=1"), "{first}");
    let second = succeeded(fixture.worktree(&["acquire", "--task", "t2"]));
    assert!(second.contains("worktree acquire: slot=2"), "{second}");
    let refused_pool = refused(fixture.worktree(&["acquire", "--task", "t3"]));
    assert!(
        refused_pool.contains("no free slot in the pool of 2 (worktree_pool_size)"),
        "{refused_pool}"
    );
    assert!(
        refused_pool.contains("slot 1 is held by task 't1'"),
        "{refused_pool}"
    );
    assert!(
        refused_pool.contains("slot 2 is held by task 't2'"),
        "{refused_pool}"
    );
    assert!(
        !fixture.slot(3).exists(),
        "the directory capacity is worktree_pool_size, not the executor count"
    );
    // The same pool bound - not the executor concurrency of 8 - gates a
    // fresh dispatch, and the refusal names the configured size.
    let spawn = refused(fixture.spawn(&["--owner", "exec-cap"]));
    assert!(
        spawn.contains("no free slot in the pool of 2 (worktree_pool_size)"),
        "{spawn}"
    );
    // Only an explicit release returns the position.
    let release = succeeded(fixture.worktree(&[
        "release",
        "--task",
        "t1",
        "--disposition",
        "discarded",
        "--reason",
        "superseded acquisition",
    ]));
    assert!(
        release.contains("disposition=discarded released"),
        "{release}"
    );
    let reused = succeeded(fixture.worktree(&["acquire", "--task", "t3"]));
    assert!(reused.contains("worktree acquire: slot=1"), "{reused}");
    fixture.drop();
}

#[test]
fn ordinary_and_executor_tasks_share_one_pool_view() {
    let fixture = Fixture::new("shared", 4, 8);
    let acquisition = succeeded(fixture.worktree(&["acquire", "--task", "t1"]));
    assert!(
        acquisition.contains("worktree acquire: slot=1"),
        "{acquisition}"
    );
    let spawn = refused(fixture.spawn(&["--owner", "exec-adm"]));
    let executor_slot = fixture.slot(2);
    assert!(
        spawn.contains(&format!(
            "executor slot: index=2 path={} base=",
            executor_slot.display()
        )),
        "{spawn}"
    );
    assert!(spawn.contains("task=exec-adm"), "{spawn}");
    assert!(spawn.contains("owner=exec-adm"), "{spawn}");
    assert!(
        spawn.contains("installed Codex launcher is missing"),
        "{spawn}"
    );
    let record = fixture.record(&fixture.source, 2);
    assert_eq!(record["task"], "exec-adm");
    assert_eq!(record["owner"], "exec-adm");
    // One hard pool: both kinds of durable bindings appear in the same views.
    let list = succeeded(fixture.worktree(&["list"]));
    assert!(
        list.contains(&format!(
            "worktree list: slot=1 path={} presence=registered state=held",
            fixture.slot(1).display()
        )),
        "{list}"
    );
    assert!(list.contains("task=t1"), "{list}");
    assert!(
        list.contains(&format!(
            "worktree list: slot=2 path={} presence=registered state=held",
            executor_slot.display()
        )),
        "{list}"
    );
    assert!(list.contains("task=exec-adm"), "{list}");
    let pool = succeeded(fixture.executor(&["pool"]));
    assert!(
        pool.contains(&format!(
            "slot 1 {} presence=registered state=occupied",
            fixture.slot(1).display()
        )),
        "{pool}"
    );
    assert!(pool.contains("owner=t1 base="), "{pool}");
    assert!(pool.contains("task=t1 handle="), "{pool}");
    assert!(
        pool.contains(&format!(
            "slot 2 {} presence=registered state=occupied",
            executor_slot.display()
        )),
        "{pool}"
    );
    assert!(pool.contains("owner=exec-adm"), "{pool}");
    // A fresh acquisition never bypasses a held task: the next ordinary task
    // takes the next free position, and the executor release returns its own.
    let next = succeeded(fixture.worktree(&["acquire", "--task", "t2"]));
    assert!(next.contains("worktree acquire: slot=3"), "{next}");
    let release = succeeded(fixture.executor(&[
        "release",
        "--slot",
        "2",
        "--disposition",
        "discarded",
        "--reason",
        "fixture admission released",
    ]));
    assert!(
        release.contains("executor slot 2 released as discarded"),
        "{release}"
    );
    let reused = succeeded(fixture.worktree(&["acquire", "--task", "t3"]));
    assert!(reused.contains("worktree acquire: slot=2"), "{reused}");
    fixture.drop();
}

/// Executor admission is separate from pool capacity: an active run fills the
/// configured concurrency while pool positions stay free, and a held but
/// inactive run keeps capacity without consuming concurrency.
#[test]
fn executor_concurrency_gates_dispatch_apart_from_free_pool_slots() {
    let fixture = Fixture::new("admission", 4, 1);
    // An ordinary held task consumes capacity, never executor concurrency.
    let acquired = succeeded(fixture.worktree(&["acquire", "--task", "t1"]));
    assert!(acquired.contains("worktree acquire: slot=1"), "{acquired}");
    // One executor run on the next slot; the canned dispatch stops at the
    // missing launcher after the pool work, so no model request starts.
    let first = refused(fixture.spawn(&["--owner", "exec-a"]));
    assert!(first.contains("executor slot: index=2"), "{first}");
    assert!(first.contains("task=exec-a"), "{first}");
    // A live host for exec-a fills the concurrency of one; the two free
    // positions still do not grant executor-b another active run.
    fixture.record_live_lease(&fixture.source, 2, "exec-a");
    let refused_run = refused(fixture.spawn(&["--owner", "exec-b"]));
    assert!(
        refused_run.contains(
            "executor concurrency is full: 1 of 1 active runs (max_concurrent_executors)"
        ),
        "{refused_run}"
    );
    assert!(
        refused_run.contains("slot 2 is running 'exec-a'"),
        "{refused_run}"
    );
    assert!(
        !fixture.slot(3).exists(),
        "the refused dispatch allocated nothing"
    );
    // The host exiting frees concurrency, not capacity: exec-a's binding
    // stays held and the next dispatch takes the next free position.
    fixture.remove_lease(&fixture.source, 2);
    let second = refused(fixture.spawn(&["--owner", "exec-b"]));
    assert!(second.contains("executor slot: index=3"), "{second}");
    assert!(second.contains("task=exec-b"), "{second}");
    assert_eq!(fixture.record(&fixture.source, 2)["task"], "exec-a");
    assert_eq!(fixture.record(&fixture.source, 3)["task"], "exec-b");
    fixture.drop();
}

#[test]
fn offline_acquisitions_resolve_local_bases_without_fetching() {
    let fixture = Fixture::new("offline", 2, 8);
    let head = git_output(&fixture.source, &["rev-parse", "HEAD"]);
    // The configured upstream is gone: acquisition resolves local commits
    // only, and an explicit base never fetches from a remote.
    git(
        &fixture.source,
        &[
            "remote",
            "set-url",
            "origin",
            &file_url(&fixture.root.join("gone.git")),
        ],
    );
    let default_base = succeeded(fixture.worktree(&["acquire", "--task", "t-local"]));
    assert!(
        default_base.contains(&format!("base={head}")),
        "{default_base}"
    );
    fs::write(fixture.source.join("advance.txt"), "advance\n").unwrap();
    git(&fixture.source, &["add", "advance.txt"]);
    git(&fixture.source, &["commit", "-qm", "advance"]);
    let explicit =
        succeeded(fixture.worktree(&["acquire", "--task", "t-explicit", "--base", &head]));
    assert!(explicit.contains("worktree acquire: slot=2"), "{explicit}");
    assert!(explicit.contains(&format!("base={head}")), "{explicit}");
    let continued = succeeded(fixture.worktree(&["continue", "--task", "t-explicit"]));
    assert!(
        continued.contains("worktree continue: slot=2"),
        "{continued}"
    );
    let unknown = refused(fixture.worktree(&[
        "acquire",
        "--task",
        "t-bad",
        "--base",
        "0000000000000000000000000000000000000000",
    ]));
    assert!(
        unknown.contains("is not a commit available locally"),
        "{unknown}"
    );
    assert!(
        unknown.contains("an explicit base never fetches from a remote"),
        "{unknown}"
    );
    fixture.drop();
}
#[test]
fn missing_slot_refuses_until_pruned_and_held_bindings_survive() {
    let fixture = Fixture::new("missing", 3, 8);
    let acquire = succeeded(fixture.worktree(&["acquire", "--task", "t1"]));
    let slot = fixture.slot(1);
    assert!(
        acquire.contains(&format!("worktree acquire: slot=1 path={}", slot.display())),
        "{acquire}"
    );
    fs::remove_dir_all(&slot).unwrap();
    let list = succeeded(fixture.worktree(&["list"]));
    assert!(
        list.contains(&format!(
            "worktree list: slot=1 path={} presence=registered-missing state=held",
            slot.display()
        )),
        "{list}"
    );
    let refused_acquire = refused(fixture.worktree(&["acquire", "--task", "t2"]));
    assert!(
        refused_acquire.contains(&format!(
            "slot 1 {} is registered but missing",
            slot.display()
        )),
        "{refused_acquire}"
    );
    assert!(
        refused_acquire.contains("git worktree prune"),
        "{refused_acquire}"
    );
    // The recorded recovery, then the held task keeps its position and the
    // next task takes a different one.
    git(&fixture.source, &["worktree", "prune"]);
    let next = succeeded(fixture.worktree(&["acquire", "--task", "t2"]));
    assert!(next.contains("worktree acquire: slot=2"), "{next}");
    let list = succeeded(fixture.worktree(&["list"]));
    assert!(
        list.contains(&format!(
            "worktree list: slot=1 path={} presence=registered state=held",
            slot.display()
        )),
        "{list}"
    );
    assert!(list.contains("task=t1"), "{list}");
    let release = succeeded(fixture.worktree(&[
        "release",
        "--task",
        "t1",
        "--disposition",
        "discarded",
        "--reason",
        "deleted checkout recovered by prune",
    ]));
    assert!(release.contains("released; slot=1"), "{release}");
    let reused = succeeded(fixture.worktree(&["acquire", "--task", "t3"]));
    assert!(reused.contains("worktree acquire: slot=1"), "{reused}");
    fixture.drop();
}

#[test]
fn foreign_slot_is_preserved_and_recovers_after_moving_aside() {
    let fixture = Fixture::new("foreign", 3, 8);
    succeeded(fixture.worktree(&["acquire", "--task", "t1"]));
    let foreign = fixture.slot(2);
    fs::create_dir_all(&foreign).unwrap();
    fs::write(foreign.join("keep.txt"), "unrelated directory\n").unwrap();
    let list = succeeded(fixture.worktree(&["list"]));
    assert!(
        list.contains(&format!(
            "worktree list: slot=2 path={} presence=foreign",
            foreign.display()
        )),
        "{list}"
    );
    let refused_acquire = refused(fixture.worktree(&["acquire", "--task", "t2"]));
    assert!(
        refused_acquire.contains(&format!(
            "slot 2 {} exists but is not a registered worktree of {}",
            foreign.display(),
            fixture.source.display()
        )),
        "{refused_acquire}"
    );
    assert!(
        refused_acquire.contains("refusing to adopt or replace a foreign path"),
        "{refused_acquire}"
    );
    assert!(
        foreign.join("keep.txt").is_file(),
        "a foreign position is never replaced or deleted"
    );
    assert!(
        !fixture.slot(3).exists(),
        "the walk stops at the uncompromised position instead of skipping it"
    );
    let aside = fixture.root.join("moved-aside");
    fs::rename(&foreign, &aside).unwrap();
    let next = succeeded(fixture.worktree(&["acquire", "--task", "t2"]));
    assert!(
        next.contains(&format!(
            "worktree acquire: slot=2 path={} presence=registered",
            foreign.display()
        )),
        "{next}"
    );
    assert!(aside.join("keep.txt").is_file());
    fixture.drop();
}

#[test]
fn handoff_reuses_the_ordinary_checkout_without_a_second_worktree() {
    let fixture = Fixture::new("handoff", 4, 8);
    succeeded(fixture.worktree(&["acquire", "--task", "t1"]));
    let handle = fixture.handle(&fixture.source, 1);
    let head = git_output(&fixture.source, &["rev-parse", "HEAD"]);
    let slot = fixture.slot(1);
    fs::write(slot.join("partial.txt"), "partial assignment work\n").unwrap();
    git(&slot, &["add", "partial.txt"]);
    git(&slot, &["commit", "-qm", "partial work"]);
    let partial = git_output(&slot, &["rev-parse", "HEAD"]);
    let spawn =
        refused(fixture.spawn(&["--task", "t1", "--handle", &handle, "--owner", "exec-hand"]));
    assert!(
        spawn.contains("installed Codex launcher is missing"),
        "{spawn}"
    );
    assert!(
        spawn.contains(&format!(
            "executor slot: index=1 path={} base={head} owner=exec-hand",
            slot.display()
        )),
        "{spawn}"
    );
    assert!(
        spawn.contains(&format!("task=t1 handle={handle}")),
        "{spawn}"
    );
    assert!(
        spawn.contains(&format!("cwd={}", slot.display())),
        "{spawn}"
    );
    // No second checkout, no reset: the handed-off slot keeps the partial work.
    assert_eq!(fixture.checkouts(&fixture.source), ["proj", "proj-wt1"]);
    assert!(slot.join("partial.txt").is_file());
    assert_eq!(git_output(&slot, &["rev-parse", "HEAD"]), partial);
    let record = fixture.record(&fixture.source, 1);
    assert_eq!(record["task"], "t1");
    assert_eq!(record["owner"], "exec-hand");
    assert_eq!(record["handle"], handle.as_str());
    // The generic path cannot verify a session binding and fails closed.
    let refused_continue = refused(fixture.worktree(&["continue", "--task", "t1"]));
    assert!(
        refused_continue.contains(
            "task 't1' in slot 1 is bound to session exec-hand; continue it as that session or hand it to another session explicitly"
        ),
        "{refused_continue}"
    );
    // A former session that is not live may be explicitly replaced.
    let rebound = refused(fixture.spawn(&[
        "--task",
        "t1",
        "--handle",
        &handle,
        "--owner",
        "exec-hand-2",
    ]));
    assert!(
        rebound.contains(&format!(
            "executor slot: index=1 path={} base={head} owner=exec-hand-2",
            slot.display()
        )),
        "{rebound}"
    );
    assert!(
        rebound.contains(&format!("task=t1 handle={handle}")),
        "{rebound}"
    );
    assert!(slot.join("partial.txt").is_file());
    // The executor adapter returns the ordinary task's capacity explicitly.
    let release = succeeded(fixture.executor(&[
        "release",
        "--slot",
        "1",
        "--disposition",
        "discarded",
        "--reason",
        "handoff fixture discarded",
    ]));
    assert!(
        release.contains("executor slot 1 released as discarded"),
        "{release}"
    );
    let list = succeeded(fixture.worktree(&["list"]));
    assert!(
        list.contains(&format!(
            "worktree list: slot=1 path={} presence=registered state=free",
            slot.display()
        )),
        "{list}"
    );
    let reused = succeeded(fixture.worktree(&["acquire", "--task", "t2"]));
    assert!(reused.contains("worktree acquire: slot=1"), "{reused}");
    fixture.drop();
}

#[test]
fn target_repository_stands_alone_from_the_configuration_source() {
    let fixture = Fixture::new("target", 2, 8);
    let app = fixture.consumer("app");
    assert!(
        !app.join("global/orchestration.toml").exists(),
        "the target repository carries no kit configuration"
    );
    let head = git_output(&app, &["rev-parse", "HEAD"]);
    let acquire = succeeded(fixture.worktree(&[
        "acquire",
        "--task",
        "t-app",
        "--repository",
        app.to_str().unwrap(),
    ]));
    let app_slot = fixture.root.join("app-wt1");
    assert!(
        acquire.contains(&format!(
            "worktree pool: repository={} identity={} source={} capacity=2",
            app.display(),
            app.display(),
            fixture.source.display()
        )),
        "{acquire}"
    );
    assert!(
        acquire.contains(&format!(
            "worktree acquire: slot=1 path={} presence=registered state=held base={head} task=t-app",
            app_slot.display()
        )),
        "{acquire}"
    );
    // The same pool admits a fresh executor task, with its configured routing
    // while its checkout, source and repository stay distinct.
    let spawn =
        refused(fixture.spawn(&["--repository", app.to_str().unwrap(), "--owner", "exec-app"]));
    let exec_slot = fixture.root.join("app-wt2");
    assert!(
        spawn.contains("installed Codex launcher is missing"),
        "{spawn}"
    );
    assert!(
        spawn.contains(&format!(
            "executor slot: index=2 path={} base={head} owner=exec-app remote=origin/main source={} repository={} task=exec-app",
            exec_slot.display(),
            fixture.source.display(),
            app.display()
        )),
        "{spawn}"
    );
    assert!(
        spawn.contains(
            "executor presentation: mode=tui presentation=native-tui model=deepseek-flash"
        ),
        "{spawn}"
    );
    assert!(
        spawn.contains(&format!("cwd={}", exec_slot.display())),
        "{spawn}"
    );
    assert_eq!(fixture.record(&app, 1)["task"], "t-app");
    assert_eq!(fixture.record(&app, 2)["task"], "exec-app");
    let list = succeeded(fixture.worktree(&["list", "--repository", app.to_str().unwrap()]));
    assert!(list.contains("task=t-app"), "{list}");
    assert!(list.contains("task=exec-app"), "{list}");
    // Repeating the dispatch continues the held binding without a second
    // checkout or a reset, preserving partial work.
    fs::write(exec_slot.join("partial.txt"), "partial work\n").unwrap();
    let repeated =
        refused(fixture.spawn(&["--repository", app.to_str().unwrap(), "--owner", "exec-app"]));
    assert!(
        repeated.contains(&format!(
            "executor slot: index=2 path={} base={head} owner=exec-app",
            exec_slot.display()
        )),
        "{repeated}"
    );
    assert!(
        exec_slot.join("partial.txt").is_file(),
        "a continued binding keeps partial work"
    );
    assert_eq!(fixture.checkouts(&app), ["app", "app-wt1", "app-wt2"]);
    fixture.drop();
}

#[test]
fn stale_generations_and_superseded_releases_are_refused() {
    let fixture = Fixture::new("stale", 2, 8);
    succeeded(fixture.worktree(&["acquire", "--task", "t1"]));
    let first = fixture.handle(&fixture.source, 1);
    let stale_continue =
        refused(fixture.worktree(&["continue", "--task", "t1", "--handle", "stale-handle"]));
    assert!(
        stale_continue.contains(&format!("has generation {first} instead of stale-handle")),
        "{stale_continue}"
    );
    assert!(
        stale_continue.contains("the handle is stale after release or reassignment"),
        "{stale_continue}"
    );
    succeeded(fixture.worktree(&[
        "release",
        "--task",
        "t1",
        "--disposition",
        "discarded",
        "--reason",
        "first generation completed",
    ]));
    // The task can be acquired again as a new generation; the old handle and
    // the release it would authorize are refused.
    let second = succeeded(fixture.worktree(&["acquire", "--task", "t1"]));
    assert!(second.contains("worktree acquire: slot=1"), "{second}");
    let generation = fixture.handle(&fixture.source, 1);
    assert_ne!(generation, first);
    let superseded = refused(fixture.worktree(&[
        "release",
        "--task",
        "t1",
        "--disposition",
        "discarded",
        "--reason",
        "stale release attempt",
        "--handle",
        &first,
    ]));
    assert!(
        superseded.contains("the handle is stale after release or reassignment"),
        "{superseded}"
    );
    assert!(!superseded.contains("released;"), "{superseded}");
    let unknown = refused(fixture.worktree(&[
        "release",
        "--task",
        "never",
        "--disposition",
        "discarded",
        "--reason",
        "no such task",
    ]));
    assert!(
        unknown.contains("no recorded binding for task 'never'"),
        "{unknown}"
    );
    assert!(unknown.contains("nothing was released"), "{unknown}");
    // The current generation still releases normally.
    let release = succeeded(fixture.worktree(&[
        "release",
        "--task",
        "t1",
        "--disposition",
        "discarded",
        "--reason",
        "second generation completed",
    ]));
    assert!(release.contains("released; slot=1"), "{release}");
    fixture.drop();
}
