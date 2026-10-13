//! Native `worktree` lifecycle checks: one repository's shared pool of Git
//! worktrees for ordinary tasks, exercised through the compiled CLI against a
//! synthetic `file://` upstream. The checks never touch a real repository,
//! model or subscription; executor dispatches stop at the missing launcher
//! after the pool work, so no conversation or model request starts.
#![cfg(windows)]

use serde_json::Value;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

/// The compiled manager used by the checks, or an explicitly supplied one so
/// the same oracle can run against a preserved pre-correction executable.
fn manager() -> PathBuf {
    std::env::var_os("HARNESS_POOL_TEST_MANAGER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_codex-harness")))
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

    /// The same fresh dispatch as [`Fixture::spawn`], started without waiting
    /// so a check can hold the pool admission lock across its admission phase.
    fn spawn_child(&self, extra: &[&str]) -> CapturedChild {
        let mut args = vec!["spawn", "--profile", "ds", "--exec", "assignment text"];
        args.extend_from_slice(extra);
        self.executor_child(&args)
    }

    /// One `executor ...` process started without waiting; its stdout and
    /// stderr are captured to check-owned files, so a panicking check still
    /// kills and reaps exactly the child it started.
    fn executor_child(&self, extra: &[&str]) -> CapturedChild {
        self.executor_child_with_env(extra, &[])
    }

    /// [`Fixture::executor_child`] with extra environment variables for the
    /// dispatch, used to route a controlled gate into a checkout hook.
    fn executor_child_with_env(&self, extra: &[&str], env: &[(&str, &str)]) -> CapturedChild {
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
        for (key, value) in env {
            command.env(key, value);
        }
        capture_command(command, &self.root)
    }

    /// A live canned host lease for a position that may still have no slot
    /// record. The admission gate counts it as an active run while this check
    /// process stays alive; the returned path is the file to delete when the
    /// run is meant to end.
    fn seed_live_lease(&self, repository: &Path, index: u32, owner: &str) -> PathBuf {
        let state = harness_core::task_worktree::pool_state_dir(&self.home, repository).unwrap();
        fs::create_dir_all(&state).unwrap();
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
        let path = state.join(format!("lease-{index}.json"));
        fs::write(
            &path,
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
        path
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

/// Configure one command whose stdout and stderr are captured to files the
/// check owns. A full pipe buffer can never deadlock the check: the files are
/// private to this process and are read after the child exits or is killed.
fn capture_command(mut command: Command, root: &Path) -> CapturedChild {
    static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(0);
    let sequence = NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed);
    let stdout = root.join(format!("child-{}-{sequence}-out.log", std::process::id()));
    let stderr = root.join(format!("child-{}-{sequence}-err.log", std::process::id()));
    let _ = fs::remove_file(&stdout);
    let _ = fs::remove_file(&stderr);
    let out = fs::File::create(&stdout).unwrap();
    let err = fs::File::create(&stderr).unwrap();
    let child = command
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .unwrap();
    CapturedChild {
        child,
        stdout: Some(stdout),
        stderr: Some(stderr),
    }
}

/// One check-owned child with file-captured output. Dropping it kills and
/// reaps the child, so a panicking check never leaves an owned process or a
/// blocked hook behind; unrelated processes are never signalled.
struct CapturedChild {
    child: Child,
    stdout: Option<PathBuf>,
    stderr: Option<PathBuf>,
}

impl CapturedChild {
    /// Whether the child already exited without reaping it.
    fn try_wait(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().unwrap()
    }

    /// The captured output of the child. The caller must have observed an
    /// exit or killed it: output still arriving is not part of this read.
    fn collect_output(&mut self) -> Output {
        let status = match self.child.try_wait().unwrap() {
            Some(status) => status,
            None => self.child.wait().unwrap(),
        };
        let stdout = self
            .stdout
            .as_ref()
            .map(|path| fs::read(path).unwrap_or_default())
            .unwrap_or_default();
        let stderr = self
            .stderr
            .as_ref()
            .map(|path| fs::read(path).unwrap_or_default())
            .unwrap_or_default();
        Output {
            status,
            stdout,
            stderr,
        }
    }

    /// Kill and reap this check-owned child.
    fn kill_and_reap(&mut self) {
        if self.child.try_wait().unwrap().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

impl Drop for CapturedChild {
    fn drop(&mut self) {
        self.kill_and_reap();
        if let Some(path) = self.stdout.take() {
            let _ = fs::remove_file(path);
        }
        if let Some(path) = self.stderr.take() {
            let _ = fs::remove_file(path);
        }
    }
}

/// Wait for one running dispatch, failing with its output instead of hanging
/// when it outruns the bound.
fn wait_bounded(child: &mut CapturedChild, bound: Duration, context: &str) -> Output {
    let deadline = Instant::now() + bound;
    loop {
        if child.try_wait().is_some() {
            return child.collect_output();
        }
        if Instant::now() >= deadline {
            child.kill_and_reap();
            let out = child.collect_output();
            panic!(
                "{context}: dispatch did not exit within {bound:?}: {}",
                output_text(&out)
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// While a check holds the pool admission lock, no dispatch may finish its
/// admission phase. A finish before the lock is released means the admission
/// decision and the startup reservation it authorizes were published as two
/// separate steps.
fn assert_blocked_while_held(children: &mut [CapturedChild], settle: Duration, context: &str) {
    let deadline = Instant::now() + settle;
    while Instant::now() < deadline {
        for (position, child) in children.iter_mut().enumerate() {
            if child.try_wait().is_some() {
                let out = child.collect_output();
                panic!(
                    "{context}: dispatch {position} finished while the pool admission lock \
                     was held, so admission and the startup reservation are not one atomic \
                     step: {}",
                    output_text(&out)
                );
            }
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Poll one child-captured file until it exists or the bound elapses. The
/// child is killed on failure so an unanswered wait cannot leak it.
fn wait_for_file(child: &mut CapturedChild, path: &Path, bound: Duration, context: &str) -> String {
    let deadline = Instant::now() + bound;
    loop {
        match fs::read_to_string(path) {
            Ok(text) if !text.trim().is_empty() => return text,
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => panic!("{context}: {} is unreadable: {error}", path.display()),
        }
        if Instant::now() >= deadline {
            child.kill_and_reap();
            let out = child.collect_output();
            panic!(
                "{context}: {} did not appear within {bound:?}: {}",
                path.display(),
                output_text(&out)
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
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

/// Admission and its startup reservation are one atomic step: the pool-wide
/// admission lock parks a dispatch after its courtesy check but before the
/// decisive count, so simultaneous dispatches cannot both count the allowance
/// as free and then claim different positions. The same gate serves fresh
/// spawn, handoff and continuation, while an allowance that is actually free
/// still admits one independent startup.
///
/// The check holds the admission lock exactly as a competing dispatch would.
/// A dispatch that finishes while the lock is held proves the decision and
/// the reservation were published separately: the uncorrected candidate let
/// both simultaneous dispatches pass admission, claim the two free positions
/// and stop only at the missing launcher.
#[test]
fn simultaneous_dispatch_admission_is_atomic_with_the_startup_reservation() {
    let fixture = Fixture::new("admission-atomic", 2, 1);
    let state =
        harness_core::task_worktree::pool_state_dir(&fixture.home, &fixture.source).unwrap();
    let slot_record = |index: u32| state.join(format!("slot-{index}.json"));
    let lease = |index: u32| state.join(format!("lease-{index}.json"));

    // Two simultaneous fresh dispatches are started while this check holds
    // the admission lock. Neither may claim a position, publish a host lease
    // or finish its pool work before the lock is released.
    let guard =
        harness_core::task_worktree::begin_pool_admission(&fixture.home, &fixture.source).unwrap();
    let mut children = vec![
        fixture.spawn_child(&["--owner", "exec-a"]),
        fixture.spawn_child(&["--owner", "exec-b"]),
    ];
    let settle = Duration::from_secs(3);
    assert_blocked_while_held(&mut children, settle, "fresh spawn pair");
    assert!(
        !slot_record(1).exists() && !slot_record(2).exists(),
        "a dispatch published a slot claim while the admission lock was held"
    );
    assert!(
        !lease(1).exists() && !lease(2).exists(),
        "a dispatch published a host lease while the admission lock was held"
    );

    // A live run fills the concurrency of one while the gate is still held:
    // both parked dispatches passed their courtesy check on an empty
    // allowance and must refuse once released, because the decisive count
    // happens under the guard, after the allowance changed.
    let seeded = fixture.seed_live_lease(&fixture.source, 1, "exec-seed");
    drop(guard);
    for (position, child) in children.iter_mut().enumerate() {
        let text = refused(wait_bounded(
            child,
            Duration::from_secs(30),
            &format!("fresh spawn {position}"),
        ));
        assert!(
            text.contains(
                "executor concurrency is full: 1 of 1 active runs (max_concurrent_executors)"
            ),
            "{text}"
        );
        assert!(text.contains("slot 1 is running 'exec-seed'"), "{text}");
        assert!(
            !text.contains("installed Codex launcher is missing"),
            "an over-limit dispatch reached the launcher: {text}"
        );
    }
    assert!(
        !slot_record(1).exists() && !slot_record(2).exists(),
        "a refused dispatch claimed a pool position"
    );

    // The holder gone, the same allowance admits one independent startup:
    // the gate serializes the decision, it does not consume capacity.
    fs::remove_file(&seeded).unwrap();
    let admitted = refused(fixture.spawn(&["--owner", "exec-a"]));
    assert!(admitted.contains("executor slot: index=1"), "{admitted}");
    assert!(
        admitted.contains("installed Codex launcher is missing"),
        "{admitted}"
    );
    assert_eq!(fixture.record(&fixture.source, 1)["owner"], "exec-a");

    // Continuation parks at the same gate over its recorded binding: the
    // exact slot, owner and partial work are untouched while the lock is
    // held, and the resume is admitted once the lock is released.
    let partial = fixture.slot(1).join("partial.txt");
    fs::write(&partial, "partial assignment work\n").unwrap();
    let guard =
        harness_core::task_worktree::begin_pool_admission(&fixture.home, &fixture.source).unwrap();
    let mut resumed = fixture.executor_child(&[
        "resume",
        "--slot",
        "1",
        "--owner",
        "exec-a",
        "--session",
        "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4",
        "--exec",
        "continue the interrupted assignment",
    ]);
    assert_blocked_while_held(std::slice::from_mut(&mut resumed), settle, "continuation");
    assert_eq!(fixture.record(&fixture.source, 1)["owner"], "exec-a");
    drop(guard);
    let text = refused(wait_bounded(
        &mut resumed,
        Duration::from_secs(30),
        "continuation",
    ));
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    assert!(text.contains("owner=exec-a"), "{text}");
    assert!(partial.is_file(), "continuation must preserve partial work");

    // Handoff parks at the same gate too: the ordinary task's position keeps
    // its checkout and partial work across the handed-off dispatch.
    let acquired = succeeded(fixture.worktree(&["acquire", "--task", "t1"]));
    assert!(acquired.contains("worktree acquire: slot=2"), "{acquired}");
    let handle = fixture.handle(&fixture.source, 2);
    let partial_handoff = fixture.slot(2).join("partial-handoff.txt");
    fs::write(&partial_handoff, "partial handoff work\n").unwrap();
    let guard =
        harness_core::task_worktree::begin_pool_admission(&fixture.home, &fixture.source).unwrap();
    let mut handoff =
        fixture.spawn_child(&["--task", "t1", "--handle", &handle, "--owner", "exec-hand"]);
    assert_blocked_while_held(std::slice::from_mut(&mut handoff), settle, "handoff");
    assert_eq!(fixture.record(&fixture.source, 2)["owner"], "t1");
    drop(guard);
    let text = refused(wait_bounded(
        &mut handoff,
        Duration::from_secs(30),
        "handoff",
    ));
    assert!(
        text.contains(&format!(
            "executor slot: index=2 path={} base=",
            fixture.slot(2).display()
        )),
        "{text}"
    );
    assert!(text.contains("owner=exec-hand"), "{text}");
    assert!(text.contains("task=t1"), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    assert!(
        partial_handoff.is_file(),
        "handoff must preserve partial work"
    );
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

/// Serialization control: with pool capacity and executor allowance both two,
/// a second eligible dispatch must make its own progress while the first one
/// is inside a slow checkout preparation. The shared admission step ends at
/// the startup reservation, so a build that keeps the admission lock across
/// preparation blocks this second startup until the first one finishes; this
/// check bounds that wait and reports the blocked startup instead of hanging.
#[test]
fn independent_startup_progresses_while_another_preparation_is_slow() {
    let fixture = Fixture::new("slow-prep", 2, 2);
    // The checkout hook gates only dispatches that receive the gate variables:
    // the first dispatch's `git worktree add` stops inside its preparation
    // while this check keeps that process and its reservation alive. The
    // in-hook bound releases an orphaned hook even if the check itself dies.
    let hooks = fixture.source.join(".git/hooks");
    fs::create_dir_all(&hooks).unwrap();
    let started = fixture.root.join("preparation-started");
    let gate = fixture.root.join("preparation-gate");
    let hook = [
        "#!/bin/sh",
        "if [ -n \"$WORKTREE_POOL_TEST_GATE\" ]; then",
        "  echo started > \"$WORKTREE_POOL_TEST_STARTED\"",
        "  n=0",
        "  while [ ! -f \"$WORKTREE_POOL_TEST_GATE\" ] && [ $n -lt 450 ]; do",
        "    sleep 0.2",
        "    n=$((n+1))",
        "  done",
        "fi",
        "exit 0",
    ]
    .join("\n");
    fs::write(hooks.join("post-checkout"), format!("{hook}\n")).unwrap();
    // Slot 2 is materialized up front: the second dispatch is not gated, and
    // two simultaneous `git worktree add` runs would race on the shared
    // worktree bookkeeping.
    git(
        &fixture.source,
        &[
            "worktree",
            "add",
            "--detach",
            fixture.slot(2).to_str().unwrap(),
            "HEAD",
        ],
    );

    let mut first = fixture.executor_child_with_env(
        &[
            "spawn",
            "--profile",
            "ds",
            "--exec",
            "assignment text",
            "--owner",
            "exec-a",
        ],
        &[
            ("WORKTREE_POOL_TEST_GATE", gate.to_str().unwrap()),
            ("WORKTREE_POOL_TEST_STARTED", started.to_str().unwrap()),
        ],
    );
    // The reservation must be recorded before the slow preparation starts:
    // that is what excludes rivals while the admission guard is released.
    let state =
        harness_core::task_worktree::pool_state_dir(&fixture.home, &fixture.source).unwrap();
    let record = state.join("slot-1.json");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let reserved = fs::read(&record)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|value| value["task"] == "exec-a" && value["startup"].is_object());
        if reserved {
            break;
        }
        if Instant::now() >= deadline {
            first.kill_and_reap();
            panic!("slot 1 did not record the exec-a startup reservation before preparation");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    wait_for_file(
        &mut first,
        &started,
        Duration::from_secs(30),
        "slot 1 preparation",
    );
    assert!(
        first.try_wait().is_none(),
        "the first dispatch left preparation before the gate was released"
    );

    // The second independent startup must claim its own position and finish
    // its dispatch attempt while the first preparation is still held.
    let mut second = fixture.executor_child(&[
        "spawn",
        "--profile",
        "ds",
        "--exec",
        "assignment text",
        "--owner",
        "exec-b",
    ]);
    let deadline = Instant::now() + Duration::from_secs(60);
    while second.try_wait().is_none() {
        if Instant::now() >= deadline {
            panic!(
                "the second eligible startup did not finish while the first preparation \
                 was held; the admission step must end at the startup reservation"
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let second_text = refused(second.collect_output());
    assert!(
        second_text.contains("executor slot: index=2"),
        "the second eligible startup must claim its own position while the first \
         preparation is held: {second_text}"
    );
    assert!(
        second_text.contains("installed Codex launcher is missing"),
        "{second_text}"
    );
    assert!(
        first.try_wait().is_none(),
        "the first dispatch stopped being prepared before the gate was released"
    );

    fs::write(&gate, "release\n").unwrap();
    let released = refused(wait_bounded(
        &mut first,
        Duration::from_secs(60),
        "slot 1 spawn",
    ));
    assert!(
        released.contains("executor slot: index=1"),
        "the gated dispatch must complete on its reserved position: {released}"
    );
    assert!(
        released.contains("installed Codex launcher is missing"),
        "{released}"
    );
    fixture.drop();
}
