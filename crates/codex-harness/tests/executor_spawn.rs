//! Configured executor dispatch into the harness-owned worktree pool. The
//! pool slots are ordinary Git worktrees of a synthetic `file://` upstream, so
//! these checks never touch a real repository, model or subscription; live
//! subscription work stays opt-in.
#![cfg(windows)]

use harness_core::process::{SHARED_CPU_PERCENT, SharedCpuBudget};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::windows::ffi::OsStrExt,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, STILL_ACTIVE},
    System::{
        JobObjects::{
            IsProcessInJob, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
            JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
            JobObjectCpuRateControlInformation, OpenJobObjectW, QueryInformationJobObject,
        },
        Threading::{
            GetCurrentProcess, GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

/// Documented JOB_OBJECT_QUERY right (winnt.h); windows-sys does not export the
/// job access rights. A query-only handle can neither assign, configure nor
/// terminate the object it opens.
const JOB_OBJECT_QUERY: u32 = 0x0004;

fn manager() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"))
}

/// A dispatch invocation that does not inherit the caller's session identity:
/// an executor running this suite (the normal case) sets
/// `HARNESS_EXECUTOR_SESSION` and its own `CODEX_THREAD_ID`. The kit refuses
/// nested dispatch, so the fixture must dispatch as the lead's own shell would.
/// Successful dispatches set a synthetic thread id explicitly.
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
        let mut command = self.spawn_command();
        command.args(["--exec", "assignment text"]);
        command.args(extra);
        command.output().unwrap()
    }

    /// The spawn options an assignment check supplies itself, without the
    /// default free-text assignment every existing check relies on.
    fn spawn_command(&self) -> Command {
        let mut command = lead_command();
        command.args([
            "executor",
            "spawn",
            "--source",
            self.source.to_str().unwrap(),
            "--codex-home",
            self.home.to_str().unwrap(),
            "--profile",
            "ds",
        ]);
        command.env("CODEX_THREAD_ID", "lead-thread-synthetic");
        command
    }

    fn resume(&self, extra: &[&str]) -> std::process::Output {
        let mut command = self.resume_command();
        command.args(["--exec", "continue the interrupted assignment"]);
        command.args(extra);
        command.output().unwrap()
    }

    fn resume_command(&self) -> Command {
        let mut command = lead_command();
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
        ]);
        command.env("CODEX_THREAD_ID", "lead-thread-synthetic");
        command
    }

    fn release(&self, extra: &[&str]) -> std::process::Output {
        let mut command = lead_command();
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
        let out = lead_command()
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
    let out = lead_command()
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
fn pooled_spawn_defaults_to_the_native_tui_and_qualifies_both_spellings() {
    let fixture = Fixture::new("presentation", 1);
    fs::write(
        fixture.home.join("ds.config.toml"),
        "model = 'deepseek-flash'\nmodel_provider = 'deepseek'\nmodel_reasoning_effort = 'max'\n",
    )
    .unwrap();
    for (extra, mode, presentation) in [
        (&[][..], "tui", "native-tui"),
        (&["--mode", "tui"][..], "tui", "native-tui"),
        (&["--mode", "exec"][..], "exec", "native-inline"),
    ] {
        let out = fixture.spawn(extra);
        let text = output_text(&out);
        assert!(!out.status.success(), "{mode}: {text}");
        assert!(
            text.contains("installed Codex launcher is missing"),
            "{mode}: dispatch must stop before a model request: {text}"
        );
        assert!(
            text.contains(&format!(
                "executor presentation: mode={mode} presentation={presentation} model=deepseek-flash provider=deepseek effort=max cwd="
            )),
            "{mode}: {text}"
        );
        assert!(
            text.contains(&fixture.slot(1).display().to_string()),
            "{mode}: cwd is not the bound slot: {text}"
        );
        assert!(!text.contains("coverage=unavailable"), "{mode}: {text}");
        assert!(!text.contains("interactive launcher"), "{mode}: {text}");
        assert!(
            !text.contains("turn/start") && !text.contains("assignment was submitted"),
            "{mode}: {text}"
        );
    }
    let unknown = fixture.spawn(&["--mode", "headless"]);
    let text = output_text(&unknown);
    assert!(text.contains("unknown executor mode"), "{text}");
    assert!(!text.contains("executor presentation:"), "{text}");
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
            // This liveness-only stand-in does not run native Codex or a
            // shell tool. Supply the prepared environment a real dispatch
            // records; native shell preflight has its own integration check.
            "shell": {
                "path": std::env::var_os("PATH").unwrap(),
                "executable": "owned-fixture-pwsh.exe",
                "version": "owned fixture; no shell tool execution",
                "sandbox_mode": "danger-full-access",
            },
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
    let mut host = lead_command()
        .args(["executor", "run", "--file", receipt.to_str().unwrap()])
        .env("CODEX_HOME", &fixture.home)
        .env("CODEX_HARNESS_CPU_ACCOUNT", cpu_account(&fixture.root))
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
    // The tab host that keeps this slot's liveness is the session owner, so it
    // holds the account allowance while it runs; attaching the shared ceiling
    // does not change the pool bookkeeping it records.
    let budget = SharedCpuBudget::acquire(&cpu_account(&fixture.root), SHARED_CPU_PERCENT).unwrap();
    let job = budget.name().to_owned();
    assert_eq!(job_cpu_rate(&job), shared_cpu_rate(), "{listed}");
    let host_handle = observe_pid(host.id(), Duration::from_secs(20));
    assert!(
        process_in_job(host_handle, &job),
        "the dispatched session host must hold the account allowance: {listed}"
    );
    unsafe { CloseHandle(host_handle) };
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

fn assignment_document(objective: &str, inputs: &[&str], outputs: &[&str]) -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "schema": 1,
        "objective": objective,
        "inputs": inputs,
        "outputs": outputs,
        "invariants": ["keep the change inside the checkout"],
        "acceptance": ["the synthetic check passes"],
    }))
    .unwrap()
}

/// A structured assignment is validated against the allocated slot before the
/// launcher starts; the rejected dispatch returns its unused claim to the pool
/// instead of leaving an occupied position behind.
#[test]
fn structured_assignment_rejection_releases_the_unused_slot_claim() {
    let fixture = Fixture::new("assignment-reject", 1);
    let assignment = fixture.root.join("assignment.json");
    fs::write(
        &assignment,
        assignment_document(
            "Use an input that the slot does not have",
            &["missing/input.txt"],
            &["out/result.txt"],
        ),
    )
    .unwrap();
    let out = fixture
        .spawn_command()
        .args(["--assignment", assignment.to_str().unwrap()])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    // The declared input, not the launcher, is the reported cause.
    assert!(text.contains("missing/input.txt"), "{text}");
    assert!(text.contains("is missing from the checkout"), "{text}");
    assert!(!text.contains("launcher"), "{text}");
    assert!(text.contains("was returned to the pool"), "{text}");
    let record = fixture.record(1);
    assert_eq!(record["state"], "released");
    assert_eq!(record["disposition"], "discarded", "{record}");
    assert!(record["owner"].is_null(), "{record}");

    // The released position is reclaimed by the next dispatch: a valid
    // assignment (an existing input and a new output) reaches the launcher.
    let valid = fixture.root.join("valid.json");
    fs::write(
        &valid,
        assignment_document(
            "Extend the synthetic checkout",
            &["README.md"],
            &["out/result.txt"],
        ),
    )
    .unwrap();
    let out = fixture
        .spawn_command()
        .args(["--assignment", valid.to_str().unwrap()])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    assert!(text.contains("executor slot: index=1 path="), "{text}");
    fixture.drop();
}

/// A rejected structured resume keeps its claim and the interrupted session's
/// partial work: the brief is never rendered against a reset slot, and the
/// same owner resumes the same slot once the assignment is corrected.
#[test]
fn resume_with_a_rejected_assignment_keeps_the_slot_and_partial_work() {
    let fixture = Fixture::new("resume-assignment", 1);
    let spawned = fixture.spawn(&["--owner", "exec-ds-52"]);
    assert!(
        output_text(&spawned).contains("installed Codex launcher is missing"),
        "{}",
        output_text(&spawned)
    );
    let slot = fixture.slot(1);
    let partial = slot.join("partial-work.txt");
    fs::write(&partial, "partial work\n").unwrap();

    let rejected = fixture.root.join("rejected.json");
    fs::write(
        &rejected,
        assignment_document("Continue with a missing input", &["gone.txt"], &[]),
    )
    .unwrap();
    let out = fixture
        .resume_command()
        .args(["--assignment", rejected.to_str().unwrap()])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("gone.txt"), "{text}");
    assert!(text.contains("is missing from the checkout"), "{text}");
    assert!(text.contains("stays bound to exec-ds-52"), "{text}");
    assert!(text.contains("its partial work"), "{text}");
    assert_eq!(fs::read_to_string(&partial).unwrap(), "partial work\n");
    let record = fixture.record(1);
    assert_eq!(record["state"], "occupied");
    assert_eq!(record["owner"], "exec-ds-52");

    // The corrected assignment resumes the recorded slot with the brief that
    // names that slot; the dispatch stops at the missing launcher, not at
    // validation, and the partial work is still in place.
    let corrected = fixture.root.join("corrected.json");
    fs::write(
        &corrected,
        assignment_document(
            "Finish the interrupted outcome",
            &["partial-work.txt"],
            &["out/final.txt"],
        ),
    )
    .unwrap();
    let out = fixture
        .resume_command()
        .args(["--assignment", corrected.to_str().unwrap()])
        .output()
        .unwrap();
    let text = output_text(&out);
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    assert_eq!(fs::read_to_string(&partial).unwrap(), "partial work\n");
    fixture.drop();
}

#[test]
fn restart_preserves_dirty_worktree_reuses_assignment_and_refuses_a_stale_session() {
    let fixture = Fixture::new("restart-cache", 1);
    let spawned = fixture.spawn(&["--owner", "exec-ds-52"]);
    assert!(output_text(&spawned).contains("installed Codex launcher is missing"));
    let slot = fixture.slot(1);
    fs::write(slot.join("partial.txt"), "preserved work\n").unwrap();
    let before = git_output(&slot, &["rev-parse", "HEAD"]);
    let session = "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4";
    let receipt = pooled_receipt_path(&fixture.home, 1);
    fs::write(&receipt, serde_json::to_vec_pretty(&json!({
        "slot":{"index":1,"source":fixture.source,"path":slot,"owner":"exec-ds-52","base":before.trim(),"remote":"origin","branch":"main"},
        "control":{"assignment":"Finish the original CPU budget assignment; preserve completed edits."},
        "observation":{"schema":1,"coverage":"native","state":"stopped","session":session,"updatedMs":1}
    })).unwrap()).unwrap();
    let command = |previous: &str| {
        let mut command = lead_command();
        command.args([
            "executor",
            "restart",
            "--source",
            fixture.source.to_str().unwrap(),
            "--codex-home",
            fixture.home.to_str().unwrap(),
            "--slot",
            "1",
            "--owner",
            "exec-ds-52",
            "--session",
            previous,
        ]);
        command.env("CODEX_THREAD_ID", "lead-thread-synthetic");
        command
    };
    let refused = command("01a0c719-f4d4-7880-a9d2-1a96ee0f2401")
        .output()
        .unwrap();
    assert!(
        output_text(&refused).contains("previous session no longer occupies"),
        "{}",
        output_text(&refused)
    );
    let out = command(session).output().unwrap();
    let text = output_text(&out);
    assert!(
        text.contains("fresh conversation, preserved worktree"),
        "{text}"
    );
    assert!(
        text.contains("presentation=native-tui"),
        "restart must keep the managed presentation: {text}"
    );
    assert!(
        text.contains("previous-session="),
        "restart must name the predecessor: {text}"
    );
    assert!(
        text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    assert!(
        !text.contains("requires --exec"),
        "the recorded original assignment is reused: {text}"
    );
    assert_eq!(
        fs::read_to_string(slot.join("partial.txt")).unwrap(),
        "preserved work\n"
    );
    assert_eq!(git_output(&slot, &["rev-parse", "HEAD"]), before);
    fixture.drop();
}

/// Free-text dispatch is unchanged, and exactly one assignment source is
/// accepted by both dispatch paths.
#[test]
fn spawn_and_resume_take_exactly_one_assignment_source() {
    let fixture = Fixture::new("prompt-source", 1);
    let assignment = fixture.root.join("assignment.json");
    fs::write(
        &assignment,
        assignment_document("Do the work", &[], &["out.txt"]),
    )
    .unwrap();
    let both = fixture.spawn(&["--assignment", assignment.to_str().unwrap()]);
    let text = output_text(&both);
    assert!(!both.status.success(), "{text}");
    assert!(text.contains("mutually exclusive"), "{text}");

    let neither = fixture.spawn_command().output().unwrap();
    let text = output_text(&neither);
    assert!(!neither.status.success(), "{text}");
    assert!(
        text.contains("--exec PROMPT or --assignment FILE is required"),
        "{text}"
    );

    let neither = fixture.resume_command().output().unwrap();
    let text = output_text(&neither);
    assert!(!neither.status.success(), "{text}");
    assert!(
        text.contains("--exec PROMPT or --assignment FILE is required"),
        "{text}"
    );
    let both = fixture
        .resume_command()
        .args(["--exec", "free text", "--assignment"])
        .arg(&assignment)
        .output()
        .unwrap();
    let text = output_text(&both);
    assert!(!both.status.success(), "{text}");
    assert!(text.contains("mutually exclusive"), "{text}");
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
    assert!(
        text.contains("presentation=native-tui"),
        "resume must select the managed native presentation: {text}"
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
    let fixture = Fixture::new("exhausted", 4);
    for index in 1..=4 {
        let owner = format!("exec-{index}");
        assert!(!fixture.spawn(&["--owner", &owner]).status.success());
        assert!(fixture.slot(index).is_dir());
        fs::write(fixture.slot(index).join("work.txt"), "unreviewed work\n").unwrap();
    }
    let fifth = fixture.spawn(&["--owner", "exec-5"]);
    let text = output_text(&fifth);
    assert!(!fifth.status.success());
    assert!(text.contains("no free slot in the pool of 4"), "{text}");
    for index in 1..=4 {
        assert!(
            text.contains(&format!("slot {index} holds local or untracked changes")),
            "{text}"
        );
    }
    assert!(
        text.contains("merge or explicitly discard them before reuse"),
        "{text}"
    );
    assert!(
        !text.contains("worktree warning"),
        "the limit warning is replaced by the pool-invariant refusal: {text}"
    );
    assert_eq!(
        fixture.checkouts(),
        ["proj", "proj-wt1", "proj-wt2", "proj-wt3", "proj-wt4"]
    );
    assert!(
        !fixture.slot(5).exists(),
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
    let mut child = lead_command();
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
        ])
        .env("CODEX_THREAD_ID", "lead-thread-synthetic");
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
    assert_eq!(receipt["mode"], "tui");
    assert_eq!(receipt["control"]["presentation"], "native-tui");
    assert_eq!(receipt["args"], json!([]));
    assert_eq!(receipt["model"], "grok-4.6");
    assert_eq!(receipt["modelProvider"], "xai");
    assert_eq!(receipt["reasoningEffort"], "xhigh");
    assert_eq!(receipt["visible"], true);
    assert_eq!(receipt["slot"]["owner"], "exec-xai-live-probe");
    assert_eq!(receipt["slot"]["index"], 1);
    assert_eq!(receipt["slot"]["path"], slot.to_str().unwrap());
    assert_eq!(receipt["observation"]["coverage"], "native", "{receipt}");
    assert!(
        !receipt["observation"]["result"].is_null(),
        "the observation records the result locator: {receipt}"
    );
    assert!(stdout.contains("observation: coverage=native"), "{stdout}");
    assert_eq!(
        fs::read_to_string(slot.join("proof.txt")).unwrap().trim(),
        "orch-xai"
    );
    let _ = Duration::from_secs(1);
}

#[test]
fn executor_dispatch_is_refused_inside_an_executor_session() {
    for command in ["spawn", "resume", "run", "succeed"] {
        let output = lead_command()
            .args(["executor", command])
            .env("HARNESS_EXECUTOR_SESSION", "1")
            .output()
            .unwrap();
        assert!(!output.status.success(), "{command} must refuse");
        let text = output_text(&output);
        assert!(
            text.contains("cannot dispatch executors"),
            "{command}: {text}"
        );
        assert!(
            text.contains("return the need to the lead"),
            "{command}: {text}"
        );
    }
}

#[test]
fn executor_host_marks_the_session_environment() {
    let root = std::env::temp_dir().join(format!("executor-run-marker-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let receipt = root.join("receipt.json");
    fs::write(
        &receipt,
        serde_json::to_vec(&json!({
            "launcher": r"C:\Windows\System32\cmd.exe",
            "args": ["/c", "set HARNESS_EXECUTOR_SESSION"],
            "slot": Value::Null
        }))
        .unwrap(),
    )
    .unwrap();
    let output = lead_command()
        .args(["executor", "run", "--file", receipt.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", output_text(&output));
    assert!(
        output_text(&output).contains("HARNESS_EXECUTOR_SESSION=1"),
        "{}",
        output_text(&output)
    );
    let _ = fs::remove_dir_all(&root);
}

/// Two leads, one checkout, explicit profiles. Spawn must not collapse them
/// by checkout or leak the dispatching thread id. The executor shell path
/// then preserves those explicit arguments and does not invent an endpoint.
/// No launcher or native thread is started.
#[test]
fn two_leads_sharing_a_checkout_keep_explicit_profiles_on_the_shell_path() {
    let fixture = Fixture::new("two-leads", 2);
    fs::write(
        fixture.home.join("lead-b.config.toml"),
        "model = 'lead-b-model'\nmodel_provider = 'lead-b-provider'\nmodel_reasoning_effort = 'low'\n",
    )
    .unwrap();
    fs::write(
        fixture.source.join("global/orchestration.toml"),
        "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"lead-a\", \"lead-b\"]\nmax_concurrent_executors = 2\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\nworktree_limit = 1\n",
    )
    .unwrap();
    fs::write(
        fixture.home.join("lead-a.config.toml"),
        "model = 'lead-a-model'\nmodel_provider = 'lead-a-provider'\nmodel_reasoning_effort = 'high'\n",
    )
    .unwrap();
    let source = fixture.source.display().to_string();
    let mut recorded_sources = Vec::new();
    for (profile, model, thread) in [
        ("lead-a", "lead-a-model", "lead-thread-a"),
        ("lead-b", "lead-b-model", "lead-thread-b"),
    ] {
        let mut command = lead_command();
        command.env_remove("WT_SESSION");
        command.env("CODEX_THREAD_ID", thread);
        command.env("CODEX_SESSION_ID", thread);
        command.args([
            "executor",
            "spawn",
            "--source",
            fixture.source.to_str().unwrap(),
            "--codex-home",
            fixture.home.to_str().unwrap(),
            "--profile",
            profile,
            "--exec",
            "assignment text",
        ]);
        let out = command.output().unwrap();
        let text = output_text(&out);
        assert!(!out.status.success(), "{profile}: {text}");
        assert!(
            text.contains("installed Codex launcher is missing"),
            "{profile}: dispatch must stop before a model request: {text}"
        );
        assert!(
            text.contains(&format!("owner=exec-{profile}-")),
            "{profile}: caller was not attributed by its explicit profile: {text}"
        );
        assert!(
            text.contains(&format!("model={model}")),
            "{profile}: profile settings were not kept: {text}"
        );
        assert!(
            text.contains("source="),
            "{profile}: dispatch record has no checkout: {text}"
        );
        let recorded_source = text
            .split("source=")
            .nth(1)
            .and_then(|rest| rest.lines().next())
            .unwrap_or("")
            .trim();
        assert!(
            recorded_source.contains("proj"),
            "{profile}: shared checkout was not recorded: {recorded_source}"
        );
        recorded_sources.push(recorded_source.to_owned());
        assert!(
            !text.contains(thread),
            "{profile}: dispatch leaked the lead thread id: {text}"
        );
        assert!(
            !text.contains("endpoint-"),
            "{profile}: spawn invented an endpoint before a launcher: {text}"
        );
        let receipt = recorded_originating_lead(&fixture.home);
        assert_eq!(
            receipt["threadId"], thread,
            "{profile}: dispatch did not record its originating lead: {receipt}"
        );
        assert!(
            receipt["dispatcher"]["pid"].as_u64().unwrap_or(0) > 0,
            "{receipt}"
        );
        assert!(
            receipt["dispatcher"]["creationTime"].as_u64().unwrap_or(0) > 0,
            "{receipt}"
        );
        let generation = receipt["runGeneration"].as_str().unwrap();
        assert!(!generation.is_empty(), "{receipt}");
        assert_ne!(generation, thread, "run context reused the lead thread id");
        assert!(receipt.get("endpoint").is_none(), "{receipt}");
    }
    assert_eq!(
        recorded_sources[0], recorded_sources[1],
        "two leads did not share one checkout"
    );

    let shell = owner_powershell();
    let script = fixture.root.join("identity.ps1");
    fs::write(
        &script,
        "$report, $profile, $checkout, $settings = $args\nif ($args.Count -ne 4) { throw \"expected four shell arguments\" }\n$thread = [string]$env:CODEX_THREAD_ID\n$marker = [string]$env:HARNESS_EXECUTOR_SESSION\n@(\"marker=$marker\", \"thread=$thread\", \"profile=$profile\", \"checkout=$checkout\", \"settings=$settings\") | Set-Content -LiteralPath $report -Encoding ascii\n",
    )
    .unwrap();
    for (profile, settings) in [
        ("lead-a", "model=\"lead-a-model\""),
        ("lead-b", "model=\"lead-b-model\""),
    ] {
        let report = fixture.root.join(format!("shell-{profile}.txt"));
        let receipt = fixture.root.join(format!("shell-{profile}.json"));
        fs::write(
            &receipt,
            serde_json::to_vec(&json!({
                "launcher": shell.to_string_lossy(),
                "args": [
                    "-NoProfile",
                    "-File",
                    script.to_string_lossy(),
                    report.to_string_lossy(),
                    profile,
                    source,
                    settings
                ],
                "slot": Value::Null
            }))
            .unwrap(),
        )
        .unwrap();
        let output = lead_command()
            .args(["executor", "run", "--file", receipt.to_str().unwrap()])
            .env_remove("CODEX_THREAD_ID")
            .env_remove("CODEX_SESSION_ID")
            .env_remove("WT_SESSION")
            .current_dir(&fixture.source)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{profile}: {}",
            output_text(&output)
        );
        let recorded = fs::read_to_string(&report).unwrap();
        assert!(
            recorded.contains("marker=1"),
            "{profile}: shell was not the executor host: {recorded}"
        );
        assert!(
            recorded.contains("thread=\n") || recorded.contains("thread=\r\n"),
            "{profile}: shell reused a lead thread id: {recorded}"
        );
        assert!(
            recorded.contains(&format!("profile={profile}\n"))
                || recorded.contains(&format!("profile={profile}\r\n")),
            "{profile}: explicit profile was not preserved: {recorded}"
        );
        assert!(
            recorded.contains(&format!("checkout={source}")),
            "{profile}: shared checkout was not preserved: {recorded}"
        );
        assert!(
            recorded.contains(&format!("settings={settings}")),
            "{profile}: settings argument was rewritten: {recorded}"
        );
        assert!(
            !fixture
                .root
                .join(format!("endpoint-{profile}.json"))
                .exists(),
            "{profile}: shell path invented an endpoint"
        );
    }
    fixture.drop();
}

#[test]
fn a_live_thread_id_is_recorded_and_the_child_does_not_keep_it() {
    let fixture = Fixture::new("lead-record", 1);
    let out = fixture.spawn(&["--owner", "exec-recorded"]);
    let text = output_text(&out);
    assert!(
        text.contains("installed Codex launcher is missing"),
        "a live thread id must still dispatch: {text}"
    );
    assert!(
        !text.contains("lead-thread-synthetic"),
        "dispatch leaked the lead thread id: {text}"
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(pooled_receipt_path(&fixture.home, 1)).unwrap()).unwrap();
    assert_eq!(
        receipt["originatingLead"]["threadId"],
        "lead-thread-synthetic"
    );
    assert!(
        receipt["originatingLead"].get("endpoint").is_none(),
        "{receipt}"
    );
    fixture.drop();

    let child = lead_command()
        .args([
            "executor",
            "run",
            r"C:\Windows\System32\cmd.exe",
            "/c",
            "echo thread=%CODEX_THREAD_ID%",
        ])
        .env("CODEX_THREAD_ID", "lead-thread-synthetic")
        .output()
        .unwrap();
    let text = output_text(&child);
    assert!(child.status.success(), "{text}");
    assert!(
        !text.contains("lead-thread-synthetic"),
        "child retained the lead thread id: {text}"
    );
}

/// The bearer of the inherited record. The copy beside the receipt carries it
/// so the executor's own `lead message` can connect; it never reaches output.
const LEAD_TOKEN: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

/// The live identity of this test process, in the shape a lead endpoint record
/// carries for the app-server it belongs to. Inheritance only has to confirm
/// that identity is live; the record is never connected to here.
fn live_process() -> Value {
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
    json!({
        "pid": identity.pid,
        "creationTime": identity.creation_time,
        "program": program,
    })
}

/// One managed lead session's registry record, exactly as `lead start`
/// publishes it: the address of the exact native thread, never a message.
fn lead_record(thread: &str, port: u16, process: &Value) -> Value {
    json!({
        "schema": 1,
        "port": port,
        "token": LEAD_TOKEN,
        "threadId": thread,
        "process": process,
    })
}

fn write_lead_registry(home: &Path, thread: &str, record: &Value) {
    let path = home.join(format!("harness/lead-endpoints/{thread}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, serde_json::to_vec_pretty(record).unwrap()).unwrap();
}

/// The dispatch inherits the live registered endpoint of the lead that runs in
/// this shell: the copy beside the receipt is that exact record, and it is what
/// lets a fresh spawned executor reach its own lead.
#[test]
fn a_live_registered_lead_endpoint_is_inherited_beside_the_receipt() {
    let fixture = Fixture::new("lead-inherit", 1);
    let process = live_process();
    write_lead_registry(
        &fixture.home,
        "lead-thread-synthetic",
        &lead_record("lead-thread-synthetic", 51234, &process),
    );
    let out = fixture.spawn(&["--owner", "exec-lead-inherit"]);
    let text = output_text(&out);
    assert!(
        text.contains("installed Codex launcher is missing"),
        "the dispatch must still run its own checks: {text}"
    );
    assert!(text.contains("lead channel: inherited"), "{text}");
    assert!(
        !text.contains("lead-thread-synthetic"),
        "dispatch leaked the lead thread id: {text}"
    );
    let state =
        harness_core::task_worktree::pool_state_dir(&fixture.home, &fixture.source).unwrap();
    let inherited: Value =
        serde_json::from_slice(&fs::read(state.join("lead-endpoint-1.json")).unwrap()).unwrap();
    assert_eq!(inherited["threadId"], "lead-thread-synthetic");
    assert_eq!(inherited["port"], 51234);
    assert_eq!(inherited["token"], LEAD_TOKEN);
    assert_eq!(inherited["process"]["pid"], process["pid"]);
    assert_eq!(
        inherited["process"]["creationTime"],
        process["creationTime"]
    );
    fixture.drop();
}

/// Endpoint inheritance fails closed: a stale record is not inherited, a
/// leftover copy of another conversation is retired, and the spawn reports the
/// unavailable channel with its remedy while the dispatch itself continues
/// exactly as before.
#[test]
fn a_stale_lead_endpoint_is_refused_and_retired() {
    let fixture = Fixture::new("lead-stale", 1);
    let process = live_process();
    let dead = json!({
        "pid": 1,
        "creationTime": 1,
        "program": process["program"].clone(),
    });
    write_lead_registry(
        &fixture.home,
        "lead-thread-synthetic",
        &lead_record("lead-thread-synthetic", 51234, &dead),
    );
    let state =
        harness_core::task_worktree::pool_state_dir(&fixture.home, &fixture.source).unwrap();
    fs::create_dir_all(&state).unwrap();
    let leftover = state.join("lead-endpoint-1.json");
    fs::write(
        &leftover,
        serde_json::to_vec_pretty(&json!({"threadId": "lead-thread-other"})).unwrap(),
    )
    .unwrap();
    let out = fixture.spawn(&["--owner", "exec-lead-stale"]);
    let text = output_text(&out);
    assert!(
        text.contains("installed Codex launcher is missing"),
        "the dispatch must still run its own checks: {text}"
    );
    assert!(text.contains("lead channel: unavailable"), "{text}");
    assert!(
        text.contains("no longer live"),
        "the refusal must name the stale process: {text}"
    );
    assert!(
        text.contains("codex-harness lead start"),
        "the refusal must name the supported remedy: {text}"
    );
    assert!(
        !text.contains("lead-thread-synthetic"),
        "dispatch leaked the lead thread id: {text}"
    );
    assert!(
        !leftover.exists(),
        "a stale lead endpoint of this slot must not survive the dispatch"
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(pooled_receipt_path(&fixture.home, 1)).unwrap()).unwrap();
    assert_eq!(
        receipt["originatingLead"]["threadId"], "lead-thread-synthetic",
        "the dispatching lead is still recorded as attribution: {receipt}"
    );
    fixture.drop();
}

/// A record that names another conversation, or one that is not readable as an
/// endpoint at all, is not this thread's address: both inherit nothing.
#[test]
fn a_mismatched_or_malformed_lead_endpoint_is_refused() {
    let fixture = Fixture::new("lead-mismatch", 1);
    let process = live_process();
    write_lead_registry(
        &fixture.home,
        "lead-thread-synthetic",
        &lead_record("lead-thread-elsewhere", 51234, &process),
    );
    let out = fixture.spawn(&["--owner", "exec-lead-mismatch"]);
    let text = output_text(&out);
    assert!(text.contains("lead channel: unavailable"), "{text}");
    assert!(
        text.contains("belongs to another conversation"),
        "the refusal must name the mismatch: {text}"
    );
    assert!(
        !text.contains("lead-thread-elsewhere") && !text.contains("lead-thread-synthetic"),
        "dispatch leaked a lead thread id: {text}"
    );
    let state =
        harness_core::task_worktree::pool_state_dir(&fixture.home, &fixture.source).unwrap();
    assert!(!state.join("lead-endpoint-1.json").exists());
    fixture.drop();

    let fixture = Fixture::new("lead-malformed", 1);
    write_lead_registry(
        &fixture.home,
        "lead-thread-synthetic",
        &json!({"schema": 1, "token": LEAD_TOKEN}),
    );
    let out = fixture.spawn(&["--owner", "exec-lead-malformed"]);
    let text = output_text(&out);
    assert!(text.contains("lead channel: unavailable"), "{text}");
    assert!(
        text.contains("unreadable or malformed"),
        "the refusal must name the unusable record: {text}"
    );
    assert!(
        !text.contains(LEAD_TOKEN),
        "a refusal must never print the record's bearer: {text}"
    );
    fixture.drop();
}

/// `lead start` reports its own options, refuses an option it does not own
/// before anything starts, and an executor never hosts a lead session.
#[test]
fn lead_start_reports_its_options_and_refuses_unusable_ones() {
    let help = lead_command().args(["lead", "--help"]).output().unwrap();
    let text = output_text(&help);
    assert!(help.status.success(), "{text}");
    assert!(text.contains("codex-harness lead start"), "{text}");
    assert!(text.contains("codex-harness lead message"), "{text}");
    assert!(text.contains("kit-local endpoint"), "{text}");

    let usage = lead_command()
        .args(["lead", "start", "--help"])
        .output()
        .unwrap();
    let text = output_text(&usage);
    assert!(usage.status.success(), "{text}");
    assert!(text.contains("--session THREAD_ID"), "{text}");
    assert!(text.contains("--exec PROMPT"), "{text}");
    assert!(text.contains("after the frontend attaches"), "{text}");

    let bad = lead_command()
        .args(["lead", "start", "--profile", "ds"])
        .output()
        .unwrap();
    let text = output_text(&bad);
    assert!(!bad.status.success(), "{text}");
    assert!(
        text.contains("invalid lead start option --profile"),
        "{text}"
    );
    for option in [
        "--source CHECKOUT",
        "--codex-home DIRECTORY",
        "--session THREAD_ID",
    ] {
        assert!(text.contains(option), "{text}");
    }

    let root = std::env::temp_dir().join(format!("lead-start-options-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let blank = lead_command()
        .args(["lead", "start", "--codex-home"])
        .arg(&root)
        .args(["--session", " "])
        .output()
        .unwrap();
    let text = output_text(&blank);
    assert!(!blank.status.success(), "{text}");
    assert!(
        text.contains("--session needs the exact thread identity"),
        "{text}"
    );

    let nested = lead_command()
        .env("HARNESS_EXECUTOR_SESSION", "1")
        .args(["lead", "start", "--help"])
        .output()
        .unwrap();
    let text = output_text(&nested);
    assert!(!nested.status.success(), "{text}");
    assert!(
        text.contains("executor sessions cannot host a lead session"),
        "{text}"
    );
    assert!(text.contains("lead message"), "{text}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn missing_or_blank_lead_id_refuses_before_a_model_request() {
    let fixture = Fixture::new("lead-missing", 1);
    let mut missing = fixture.spawn_command();
    missing.env_remove("CODEX_THREAD_ID");
    missing.args(["--exec", "assignment text"]);
    let missing = missing.output().unwrap();
    let text = output_text(&missing);
    assert!(!missing.status.success(), "{text}");
    assert!(text.contains("CODEX_THREAD_ID is missing"), "{text}");
    assert!(text.contains("refusing before a model request"), "{text}");
    assert!(
        !text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    let mut blank = fixture.spawn_command();
    blank.env("CODEX_THREAD_ID", " \t");
    blank.args(["--exec", "assignment text"]);
    let blank = blank.output().unwrap();
    let text = output_text(&blank);
    assert!(!blank.status.success(), "{text}");
    assert!(text.contains("CODEX_THREAD_ID is blank"), "{text}");
    assert!(text.contains("refusing before a model request"), "{text}");
    assert!(
        !text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    fixture.drop();
}

#[test]
fn a_caller_supplied_lead_id_is_not_authority() {
    let fixture = Fixture::new("lead-override", 1);
    let mut command = fixture.spawn_command();
    command.args([
        "--lead-thread",
        "forged-lead-thread",
        "--exec",
        "assignment text",
    ]);
    let out = command.output().unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("caller-supplied lead id or recipient is not authority"),
        "{text}"
    );
    assert!(text.contains("refusing before a model request"), "{text}");
    assert!(!text.contains("forged-lead-thread"), "{text}");
    assert!(
        !text.contains("installed Codex launcher is missing"),
        "{text}"
    );

    let mut overridden = fixture.spawn_command();
    overridden.env("HARNESS_ORIGINATING_LEAD", "lead-thread-synthetic");
    overridden.args(["--exec", "assignment text"]);
    let out = overridden.output().unwrap();
    let text = output_text(&out);
    assert!(!out.status.success(), "{text}");
    assert!(
        text.contains("caller-supplied lead id or recipient is not authority"),
        "{text}"
    );
    assert!(
        !text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    fixture.drop();
}

#[test]
fn a_copied_or_sibling_marker_refuses_before_a_model_request() {
    let fixture = Fixture::new("lead-marker", 3);
    let mut copied = fixture.spawn_command();
    copied
        .env("HARNESS_EXECUTOR_RUN", "copied-marker-synthetic")
        .args(["--owner", "exec-copied", "--exec", "assignment text"]);
    let copied = copied.output().unwrap();
    let text = output_text(&copied);
    assert!(!copied.status.success(), "{text}");
    assert!(
        text.contains("copied run marker is not authority"),
        "{text}"
    );
    assert!(text.contains("refusing before a model request"), "{text}");
    assert!(
        !text.contains("installed Codex launcher is missing"),
        "{text}"
    );

    let mut sleeper = Command::new(r"C:\Windows\System32\ping.exe")
        .args(["-n", "30", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let program = PathBuf::from(r"C:\Windows\System32\ping.exe");
    let user = harness_core::process_service::current_user().unwrap();
    let identity =
        harness_core::process_service::ServiceProcess::observe(sleeper.id(), &program, 0, &user)
            .unwrap()
            .identity();
    let state = pooled_state_dir(&fixture.home);
    fs::write(
        state.join("spawn-9.json"),
        serde_json::to_vec_pretty(&json!({
            "originatingLead": {
                "schema": 1,
                "threadId": "lead-thread-sibling",
                "runGeneration": "sibling-marker-synthetic",
                "dispatcher": {
                    "pid": identity.pid,
                    "creationTime": identity.creation_time,
                    "program": program
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let sibling = fixture
        .spawn_command()
        .env("HARNESS_EXECUTOR_RUN", "sibling-marker-synthetic")
        .args(["--owner", "exec-sibling", "--exec", "assignment text"])
        .output()
        .unwrap();
    let text = output_text(&sibling);
    let _ = sleeper.kill();
    let _ = sleeper.wait();
    assert!(!sibling.status.success(), "{text}");
    assert!(
        text.contains("sibling run marker is not the originating lead"),
        "{text}"
    );
    assert!(text.contains("refusing before a model request"), "{text}");
    assert!(
        !text.contains("installed Codex launcher is missing"),
        "{text}"
    );

    fs::write(
        state.join("spawn-8.json"),
        serde_json::to_vec_pretty(&json!({
            "originatingLead": {
                "schema": 1,
                "threadId": "lead-thread-stale",
                "runGeneration": "stale-marker-synthetic",
                "dispatcher": {
                    "pid": 1,
                    "creationTime": 1,
                    "program": program
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let stale = fixture
        .spawn_command()
        .env("HARNESS_EXECUTOR_RUN", "stale-marker-synthetic")
        .args(["--owner", "exec-stale", "--exec", "assignment text"])
        .output()
        .unwrap();
    let text = output_text(&stale);
    assert!(!stale.status.success(), "{text}");
    assert!(
        text.contains("stale run reference is not authority"),
        "{text}"
    );
    assert!(
        !text.contains("installed Codex launcher is missing"),
        "{text}"
    );
    fixture.drop();
}

#[test]
fn a_descendant_cwd_change_keeps_the_recorded_lead() {
    let fixture = Fixture::new("lead-cwd", 2);
    let other = fixture.root.join("other-cwd");
    fs::create_dir_all(&other).unwrap();
    let thread = "lead-thread-synthetic";
    for (cwd, owner) in [
        (&fixture.source, "exec-cwd-parent"),
        (&other, "exec-cwd-child"),
    ] {
        let mut command = fixture.spawn_command();
        command.current_dir(cwd);
        command.env("CODEX_THREAD_ID", thread);
        command.args(["--owner", owner, "--exec", "assignment text"]);
        let out = command.output().unwrap();
        let text = output_text(&out);
        assert!(
            text.contains("installed Codex launcher is missing"),
            "{owner} must still dispatch from {}: {text}",
            cwd.display()
        );
        assert!(
            !text.contains(thread),
            "{owner} leaked the lead thread id: {text}"
        );
        let receipt = recorded_originating_lead(&fixture.home);
        assert_eq!(
            receipt["threadId"], thread,
            "{owner} cwd change did not keep the recorded lead: {receipt}"
        );
        assert_ne!(receipt["threadId"], other.display().to_string());
    }
    fixture.drop();
}

fn recorded_originating_lead(home: &Path) -> Value {
    let mut found = None;
    for entry in fs::read_dir(pooled_state_dir(home)).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy();
        if !name.starts_with("spawn-") || !name.ends_with(".json") {
            continue;
        }
        let receipt: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let lead = receipt.get("originatingLead").cloned();
        if lead.is_some() {
            found = lead;
        }
    }
    found.expect("dispatch receipt records an originating lead")
}

fn owner_powershell() -> PathBuf {
    let listed = Command::new("where.exe")
        .arg("pwsh.exe")
        .output()
        .expect("where.exe");
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(str::trim)
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .expect("PowerShell 7 pwsh.exe must be on PATH for the executor shell path")
}

/// Owned native child double: the launch fixture is a Rust binary, so these
/// checks never put a shell program into a dispatch receipt.
fn launch_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-launch-fixture"))
}

/// The child's own outcome must reach the runner unchanged in both directions:
/// a successful launcher stays success, and a failed launcher is not reported
/// as a completed repair.
#[test]
fn executor_run_reports_the_child_outcome() {
    let success = lead_command()
        .args(["executor", "run"])
        .arg(launch_fixture())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(success.status.code(), Some(0), "{}", output_text(&success));
    let failure = lead_command()
        .args(["executor", "run"])
        .arg(launch_fixture())
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(
        failure.status.code(),
        Some(19),
        "the child exit code must reach the caller: {}",
        output_text(&failure)
    );
}

/// The installed dispatch path runs the tab host as `executor run --file`, so
/// a failed session must not be recorded as a successful one there either.
#[test]
fn executor_run_receipt_propagates_a_failed_child() {
    let root = std::env::temp_dir().join(format!("executor-run-failed-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let receipt = root.join("receipt.json");
    fs::write(
        &receipt,
        serde_json::to_vec_pretty(&json!({
            "schema": 1,
            "launcher": launch_fixture(),
            "args": [],
            "slot": Value::Null
        }))
        .unwrap(),
    )
    .unwrap();
    let output = lead_command()
        .args(["executor", "run", "--file", receipt.to_str().unwrap()])
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(19),
        "the tab host must forward the failed session: {}",
        output_text(&output)
    );
    let _ = fs::remove_dir_all(&root);
}

/// A launcher that cannot start is a failure with a named cause, never a
/// silent success.
#[test]
fn executor_run_startup_error_stays_a_failure() {
    let relative = lead_command()
        .args(["executor", "run", "harness-absent-launcher.exe"])
        .output()
        .unwrap();
    assert!(!relative.status.success());
    assert!(
        output_text(&relative).contains("launcher must be absolute"),
        "{}",
        output_text(&relative)
    );
    let missing = std::env::temp_dir().join("harness-absent-launcher.exe");
    let absent = lead_command()
        .args(["executor", "run"])
        .arg(&missing)
        .output()
        .unwrap();
    assert!(
        !absent.status.success(),
        "a launcher that cannot start must not report success: {}",
        output_text(&absent)
    );
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

/// The account CPU budget of one test-local account directory. Every budget
/// case uses its own, so the machine's real allowance and any unrelated
/// consumer of the same Windows account stay untouched.
fn cpu_account(root: &Path) -> PathBuf {
    root.join("cpu-budget")
}

/// The delivered aggregate ceiling in the kernel's 0.01% units.
fn shared_cpu_rate() -> u32 {
    (SHARED_CPU_PERCENT * 100.0) as u32
}

/// Query-only handle on a named Job object: membership and the effective rate
/// are read from the kernel instead of from diagnostics printed by the process
/// under test.
fn open_job(name: &str) -> HANDLE {
    let object: Vec<u16> = std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, object.as_ptr()) };
    assert!(!handle.is_null(), "job object {name} is not observable");
    handle
}

/// Kernel CPU rate of one named Job (0 means no rate control is enabled).
fn job_cpu_rate(name: &str) -> u32 {
    let handle = open_job(name);
    let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = unsafe { std::mem::zeroed() };
    let queried = unsafe {
        QueryInformationJobObject(
            handle,
            JobObjectCpuRateControlInformation,
            (&mut cpu as *mut JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            std::ptr::null_mut(),
        )
    };
    unsafe { CloseHandle(handle) };
    assert_ne!(queried, 0, "job object {name} could not be queried");
    if cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_ENABLE == 0 {
        return 0;
    }
    assert_ne!(
        cpu.ControlFlags & JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
        0,
        "job {name} is not a hard cap"
    );
    unsafe { cpu.Anonymous.CpuRate }
}

/// Kernel membership of one live process in one named Job.
fn process_in_job(process: HANDLE, job: &str) -> bool {
    let job_handle = open_job(job);
    let mut member = 0;
    let queried = unsafe { IsProcessInJob(process, job_handle, &mut member) };
    unsafe { CloseHandle(job_handle) };
    assert_ne!(queried, 0, "IsProcessInJob failed");
    member != 0
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let started = std::time::Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < timeout,
            "{} was not created within {timeout:?}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// One running fixture process, opened by the pid it recorded itself, so the
/// kernel answers for the actual payload and not for a wrapper.
fn observe_payload(marker: &Path, timeout: Duration) -> HANDLE {
    wait_for_path(marker, timeout);
    let pid: u32 = fs::read_to_string(marker)
        .unwrap()
        .trim()
        .parse()
        .expect("payload pid");
    observe_pid(pid, timeout)
}

fn observe_pid(pid: u32, timeout: Duration) -> HANDLE {
    let started = std::time::Instant::now();
    loop {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if !handle.is_null() {
            let mut code = 0;
            if unsafe { GetExitCodeProcess(handle, &mut code) } != 0 && code == STILL_ACTIVE as u32
            {
                return handle;
            }
            unsafe { CloseHandle(handle) };
        }
        assert!(
            started.elapsed() < timeout,
            "pid {pid} was not observable while it was running"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn current_process() -> HANDLE {
    unsafe { GetCurrentProcess() }
}

/// An unrelated terminal tab or plain process of the same account: it never
/// passes through a dispatch route, so no admission may enrol it.
fn unrelated_process(marker: &Path) -> Child {
    let mut command = Command::new(launch_fixture());
    command
        .env_remove("CODEX_HARNESS_CPU_ACCOUNT")
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "20000")
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().unwrap()
}

/// The record of one tab host invocation: its receipt file and the recorded
/// payload identity it must produce for the kernel checks below.
struct TabHost {
    receipt: PathBuf,
    payload: PathBuf,
}

impl TabHost {
    fn resume_receipt(root: &Path, name: &str, arguments: &[String]) -> Self {
        let receipt = root.join(format!("{name}-receipt.json"));
        fs::write(
            &receipt,
            serde_json::to_vec_pretty(&json!({
                "schema": 1,
                "launcher": launch_fixture(),
                "args": arguments,
                "slot": Value::Null,
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            receipt,
            payload: root.join(format!("{name}-payload.pid")),
        }
    }

    /// One dispatched session as the dispatch route starts it: the tab host is
    /// the process that lives for the session, and its cwd is the session
    /// workspace the payload must keep.
    fn start(&self, workspace: &Path, account: &Path) -> Child {
        let mut command = lead_command();
        command
            .args(["executor", "run", "--file", self.receipt.to_str().unwrap()])
            .current_dir(workspace)
            .env("CODEX_HARNESS_CPU_ACCOUNT", account)
            .env("HARNESS_LAUNCH_FIXTURE_STARTED", &self.payload)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.spawn().unwrap()
    }
}

/// End the hosted payload's input and collect the host's own outcome.
fn finish_host(host: Child) -> std::process::Output {
    host.wait_with_output().unwrap()
}

/// The recorded arguments of one resume dispatch, exactly as the resume route
/// writes them into the receipt.
fn resume_arguments(workspace: &Path, result: &Path) -> Vec<String> {
    [
        "exec",
        "--json",
        "--skip-git-repo-check",
        "-C",
        workspace.to_str().unwrap(),
        "--output-last-message",
        result.to_str().unwrap(),
        "resume",
        "01a0c719-f4d4-7880-a9d2-1a96ee0f23f4",
        "continue the interrupted assignment",
    ]
    .iter()
    .map(|argument| (*argument).to_owned())
    .collect()
}

/// One dispatched session joins the account CPU allowance before its payload
/// exists, the payload inherits that one ceiling, and the terminal tab or plain
/// process that started the session stays outside it. Arguments, cwd, streams
/// and the exit code of the resumed session reach the payload unchanged.
#[test]
fn executor_session_host_joins_the_shared_account_budget_before_its_payload() {
    let root = std::env::temp_dir().join(format!("executor-budget-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let workspace = root.join("proj-wt1");
    fs::create_dir_all(&workspace).unwrap();
    let account = cpu_account(&root);
    let arguments = resume_arguments(&workspace, &root.join("result.txt"));
    let host = TabHost::resume_receipt(&root, "session", &arguments);

    let mut unrelated = unrelated_process(&root.join("unrelated.pid"));
    let unrelated_pid = observe_payload(&root.join("unrelated.pid"), Duration::from_secs(30));

    let mut session = host.start(&workspace, &account);
    // The payload is alive and still reading its input, so both members of this
    // session can be asked directly.
    let payload = observe_payload(&host.payload, Duration::from_secs(30));
    let budget = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
    let job = budget.name().to_owned();
    assert_eq!(
        job_cpu_rate(&job),
        shared_cpu_rate(),
        "the account group must carry the delivered 75% hard cap"
    );
    assert!(
        process_in_job(payload, &job),
        "the dispatched payload must be inside the account allowance before it does work"
    );
    let host_handle = observe_pid(session.id(), Duration::from_secs(30));
    assert!(
        process_in_job(host_handle, &job),
        "the tab host of this session must hold the account allowance"
    );
    assert!(
        !process_in_job(unrelated_pid, &job),
        "an unrelated process of the same account must stay outside the allowance"
    );
    assert!(
        !process_in_job(current_process(), &job),
        "starting a session must not enrol the terminal or the dispatching caller"
    );

    // The hosted payload receives the recorded interaction, then ends: the host
    // forwards the payload's own outcome instead of reporting a fabricated one.
    session
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"assignment input\n")
        .unwrap();
    drop(session.stdin.take());
    let finished = finish_host(session);
    assert_eq!(
        finished.status.code(),
        Some(0),
        "the session outcome must be the payload's own: {}",
        output_text(&finished)
    );
    let recorded: Value = serde_json::from_str(String::from_utf8_lossy(&finished.stdout).trim())
        .expect("the payload reports its own invocation");
    assert_eq!(
        recorded["args"],
        serde_json::to_value(&arguments).unwrap(),
        "the resumed session arguments must reach the payload unchanged: {recorded}"
    );
    assert_eq!(
        PathBuf::from(recorded["cwd"].as_str().unwrap()),
        workspace,
        "the payload must run in the session workspace: {recorded}"
    );
    assert_eq!(
        recorded["stdin"], "assignment input\n",
        "interactive input must reach the payload: {recorded}"
    );
    assert!(
        String::from_utf8_lossy(&finished.stderr).contains("upstream stderr"),
        "the payload's own stderr must stay separate and visible"
    );
    unsafe { CloseHandle(payload) };
    unsafe { CloseHandle(host_handle) };
    unsafe { CloseHandle(unrelated_pid) };
    let _ = unrelated.kill();
    let _ = unrelated.wait();
    let _ = fs::remove_dir_all(&root);
}

/// Admission failure follows the visible fail-open contract: the requested
/// session starts once with its own arguments and outcome, the warning names
/// the failed ceiling, its cause, the affected scope and the recovery step, and
/// a peer session keeps its allowance unchanged.
#[test]
fn executor_run_reports_degraded_coverage_when_admission_fails() {
    let root = std::env::temp_dir().join(format!("executor-degraded-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let workspace = root.join("proj-wt1");
    fs::create_dir_all(&workspace).unwrap();
    let account = cpu_account(&root);
    let budget = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
    let job = budget.name().to_owned();

    // A peer session that is already capped keeps running while the failing
    // session starts.
    let peer_arguments = resume_arguments(&workspace, &root.join("peer-result.txt"));
    let peer = TabHost::resume_receipt(&root, "peer", &peer_arguments);
    let mut peer_session = peer.start(&workspace, &account);
    let peer_payload = observe_payload(&peer.payload, Duration::from_secs(30));
    assert!(
        process_in_job(peer_payload, &job),
        "the peer session must hold the account allowance"
    );

    let arguments = resume_arguments(&workspace, &root.join("result.txt"));
    let degraded = TabHost::resume_receipt(&root, "degraded", &arguments);
    // A relative account directory is unusable, so the allowance cannot be
    // established at all: the session is requested anyway and reports it.
    let mut session = degraded.start(&workspace, Path::new("relative-cpu-budget"));
    drop(session.stdin.take());
    let finished = finish_host(session);
    let reported = output_text(&finished);
    assert_eq!(
        finished.status.code(),
        Some(0),
        "the requested session must still start and report its own outcome: {reported}"
    );
    assert!(
        reported.contains("runs outside the shared 75% account CPU allowance"),
        "the warning must name the failed ceiling: {reported}"
    );
    assert!(
        reported.contains("must be absolute and normalized"),
        "the warning must name the concrete cause: {reported}"
    );
    assert!(
        reported.contains("other sessions keep their allowance"),
        "the warning must state that peers are unaffected: {reported}"
    );
    assert!(
        reported.contains("coverage stays degraded") && reported.contains("restarted into it"),
        "the warning must state degraded coverage and the recovery step: {reported}"
    );
    assert!(
        !reported.to_lowercase().contains("joined the shared"),
        "a failed admission must never be reported as a capped start: {reported}"
    );
    let recorded: Value = serde_json::from_str(String::from_utf8_lossy(&finished.stdout).trim())
        .expect("the requested payload must still run exactly as asked");
    assert_eq!(
        recorded["args"],
        serde_json::to_value(&arguments).unwrap(),
        "a degraded session keeps the requested arguments: {recorded}"
    );
    assert_eq!(
        PathBuf::from(recorded["cwd"].as_str().unwrap()),
        workspace,
        "a degraded session keeps the requested cwd: {recorded}"
    );

    // The peer's allowance is neither lifted nor duplicated by the failure.
    assert_eq!(job_cpu_rate(&job), shared_cpu_rate());
    assert!(
        process_in_job(peer_payload, &job),
        "the peer session must keep its allowance after a peer's admission failure"
    );
    drop(peer_session.stdin.take());
    let peer_finished = finish_host(peer_session);
    assert_eq!(
        peer_finished.status.code(),
        Some(0),
        "{}",
        output_text(&peer_finished)
    );
    unsafe { CloseHandle(peer_payload) };
    let _ = fs::remove_dir_all(&root);
}
