//! Real heavy-command CLI cases in owned account roots, without models or globals.
//!
//! Every case uses a synthetic heavy-command account and a synthetic shared CPU
//! budget account, so the machine queue, its budget and the account CPU ceiling
//! are exercised through the same owners while the real machine state and any
//! unrelated consumer stay untouched. Membership is never taken from the
//! command's own diagnostics: this file queries the kernel directly for the
//! named Job objects and for the payload processes that must belong to them.
#![cfg(windows)]
use harness_core::{
    heavy_command::{Budget, LEASE_ENV},
    process::SHARED_CPU_PERCENT,
};
use serde_json::Value;
use std::{
    ffi::OsStr,
    fs,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, STILL_ACTIVE},
    System::{
        JobObjects::{
            AssignProcessToJobObject, IsProcessInJob, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
            JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation, OpenJobObjectW,
            QueryInformationJobObject,
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
/// The account CPU budget override: heavy callers must never join the machine's
/// real budget from a test.
const CPU_ACCOUNT_ENV: &str = "CODEX_HARNESS_CPU_ACCOUNT";
/// Per-launch escape hatch. Tests remove it unless they are proving that it wins.
const CPU_PERCENT_ENV: &str = "CODEX_HARNESS_CPU_PERCENT";

fn manager() -> &'static str {
    env!("CARGO_BIN_EXE_codex-harness")
}

fn fixture_target() -> &'static str {
    env!("CARGO_BIN_EXE_harness-launch-fixture")
}

/// Test-local shared CPU budget account for one heavy account directory.
fn cpu_account(account: &Path) -> PathBuf {
    account.with_file_name("cpu-budget")
}

/// The installed shared ceiling in the kernel's 0.01% units.
fn shared_cpu_rate() -> u32 {
    (SHARED_CPU_PERCENT * 100.0) as u32
}

/// A heavy call of the owned fixture target through the real CLI.
fn heavy(account: &Path, current_dir: &Path) -> Command {
    let mut command = Command::new(manager());
    command
        .arg("heavy")
        .arg("--account")
        .arg(account)
        .arg("--")
        .arg(fixture_target())
        .current_dir(current_dir)
        .env(CPU_ACCOUNT_ENV, cpu_account(account))
        .env_remove(CPU_PERCENT_ENV)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// Query-only handle on a named Job object.
fn open_job(name: &str) -> HANDLE {
    let object: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, object.as_ptr()) };
    assert!(!handle.is_null(), "job object {name} is not observable");
    handle
}

fn query_job<T: Copy>(handle: HANDLE, class: i32) -> T {
    let mut value = std::mem::MaybeUninit::<T>::zeroed();
    let queried = unsafe {
        QueryInformationJobObject(
            handle,
            class,
            value.as_mut_ptr().cast(),
            std::mem::size_of::<T>() as u32,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(queried, 0, "job object query failed");
    unsafe { value.assume_init() }
}

/// Kernel CPU rate of one named Job (0 means no rate control is enabled).
fn job_cpu_rate(name: &str) -> u32 {
    let handle = open_job(name);
    let cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION =
        query_job(handle, JobObjectCpuRateControlInformation);
    unsafe { CloseHandle(handle) };
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

/// Kernel containment flags and memory limit of one named Job.
fn job_limits(name: &str) -> (usize, bool) {
    let handle = open_job(name);
    let limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
        query_job(handle, JobObjectExtendedLimitInformation);
    unsafe { CloseHandle(handle) };
    (
        limits.JobMemoryLimit,
        limits.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE != 0,
    )
}

/// Kernel membership of one live process in one named Job, asked from this test
/// process: the payload's own evidence, never the wrapper's claim.
fn process_in_job(process: HANDLE, job: &str) -> bool {
    let job_handle = open_job(job);
    let mut member = 0;
    let queried = unsafe { IsProcessInJob(process, job_handle, &mut member) };
    unsafe { CloseHandle(job_handle) };
    assert_ne!(queried, 0, "IsProcessInJob failed");
    member != 0
}

/// This test process's own identity, so a forged marker can name a holder that
/// is unquestionably alive and still be refused on its Job name alone.
fn current_identity() -> (u32, u64) {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut creation, mut exit, mut kernel, mut user) = unsafe {
        (
            std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>(),
            std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>(),
            std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>(),
            std::mem::zeroed::<windows_sys::Win32::Foundation::FILETIME>(),
        )
    };
    let queried = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    assert_ne!(queried, 0, "GetProcessTimes failed");
    let ticks = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    (std::process::id(), ticks)
}

/// Wait until the payload process recorded by `HARNESS_LAUNCH_FIXTURE_STARTED`
/// (the fixture writes its own pid there) is observable and still running, and
/// return a query-only handle on it.
fn wait_for_payload(marker: &Path, timeout: Duration) -> HANDLE {
    wait_for_path(marker, timeout);
    let pid: u32 = fs::read_to_string(marker)
        .unwrap()
        .trim()
        .parse()
        .expect("payload pid");
    let started = Instant::now();
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
            "payload pid={pid} was not observable while it was running"
        );
        sleep(Duration::from_millis(20));
    }
}

/// The text recorded in an owned diagnostic file, read while its writer is
/// still running.
fn log_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn wait_for_log(path: &Path, needle: &str, timeout: Duration) -> String {
    let started = Instant::now();
    loop {
        let text = log_text(path);
        if text.contains(needle) {
            return text;
        }
        assert!(
            started.elapsed() < timeout,
            "{needle:?} did not appear in the diagnostics within {timeout:?}: {text}"
        );
        sleep(Duration::from_millis(20));
    }
}

/// Value of a `prefix=value` or `prefix="value"` diagnostic field.
fn field(text: &str, prefix: &str) -> String {
    let start = text
        .find(prefix)
        .unwrap_or_else(|| panic!("{prefix:?} is missing from {text}"))
        + prefix.len();
    text[start..]
        .trim_start_matches('"')
        .split(['"', ' ', '\n'])
        .next()
        .unwrap()
        .to_owned()
}

/// One heavy call whose diagnostics stream into an owned file, so the Job names
/// and the payload can be observed while the command is still running.
fn heavy_logged(account: &Path, current_dir: &Path, log: &Path) -> Command {
    let mut command = heavy(account, current_dir);
    command
        .stderr(Stdio::from(fs::File::create(log).unwrap()))
        .stdout(Stdio::null());
    command
}

fn finish(mut child: Child, timeout: Duration) -> Output {
    let started = Instant::now();
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "call exceeded {timeout:?}: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        sleep(Duration::from_millis(20));
    }
}

fn run(command: &mut Command, timeout: Duration) -> Output {
    finish(command.spawn().unwrap(), timeout)
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn budget_cli(account: &Path, options: &[&str]) -> Output {
    let mut command = Command::new(manager());
    command
        .arg("heavy")
        .arg("budget")
        .arg("--account")
        .arg(account);
    command.args(options);
    command.output().unwrap()
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < timeout,
            "{} was not created within {timeout:?}",
            path.display()
        );
        sleep(Duration::from_millis(20));
    }
}

fn read_number(path: &Path) -> u128 {
    fs::read_to_string(path)
        .unwrap()
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("{} is not a millisecond stamp", path.display()))
}

fn lock_free(path: &Path) -> bool {
    match fs::OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => matches!(file.try_lock(), Ok(())),
        Err(_) => false,
    }
}

fn wait_for_lock_free(path: &Path, timeout: Duration) -> bool {
    let started = Instant::now();
    loop {
        if lock_free(path) {
            return true;
        }
        if started.elapsed() > timeout {
            return false;
        }
        sleep(Duration::from_millis(50));
    }
}

#[test]
fn two_callers_share_one_account_queue_across_checkouts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    for name in ["checkout-one", "checkout-two"] {
        fs::create_dir_all(root.join(name)).unwrap();
    }
    let first_log = root.join("first.log");
    let first_started = root.join("first.started");
    let first_ended = root.join("first.ended");
    let first_pid = root.join("first.pid");
    let second_started = root.join("second.started");
    let first_ended_evidence = first_ended.clone();

    let mut first = heavy_logged(&account, &root.join("checkout-one"), &first_log);
    first
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "1500")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &first_started)
        .env("HARNESS_HEAVY_FIXTURE_ENDED", &first_ended)
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &first_pid)
        .env("CODEX_HOME", root.join("home-one"));
    let first = first.spawn().unwrap();
    wait_for_path(&first_started, Duration::from_secs(30));
    // Kernel evidence while the first payload runs: it belongs to the one
    // account budget at the installed ceiling, and the batch Job adds no
    // second rate of its own.
    let payload = wait_for_payload(&first_pid, Duration::from_secs(30));
    let first_text = wait_for_log(
        &first_log,
        "shared account CPU budget job=",
        Duration::from_secs(30),
    );
    let shared_job = field(&first_text, "shared account CPU budget job=");
    assert_eq!(job_cpu_rate(&shared_job), shared_cpu_rate());
    assert!(
        process_in_job(payload, &shared_job),
        "the first payload must be a member of the one shared account budget"
    );
    let heavy_job = field(&first_text, "job scope=aggregate name=");
    assert_eq!(
        job_cpu_rate(&heavy_job),
        0,
        "the default policy must not add a per-batch CPU rate"
    );
    unsafe { CloseHandle(payload) };

    // A second checkout with its own Codex home shares the same queue and the
    // same machine budget instead of multiplying the allowance.
    let mut second = heavy(&account, &root.join("checkout-two"));
    second
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &second_started)
        .env("CODEX_HOME", root.join("home-two"));
    let second = second.spawn().unwrap();

    let first = finish(first, Duration::from_secs(120));
    let second = finish(second, Duration::from_secs(120));
    let first_stderr = log_text(&first_log);
    let second_stderr = stderr(&second);
    assert_eq!(first.status.code(), Some(0), "{first_stderr}");
    assert_eq!(second.status.code(), Some(0), "{second_stderr}");
    assert!(
        !first_stderr.contains("waiting for the account heavy-command slot"),
        "the first caller must be admitted immediately: {first_stderr}"
    );
    assert!(
        second_stderr.contains("waiting for the account heavy-command slot"),
        "the second caller must report actual queue waiting: {second_stderr}"
    );
    assert!(
        second_stderr.contains("holder pid=") && second_stderr.contains("command="),
        "the queued diagnostic must name the running holder: {second_stderr}"
    );
    assert!(
        second_stderr.contains(&format!("shared account CPU budget job=\"{shared_job}\"")),
        "both callers must join one shared account group instead of one per checkout: {second_stderr}"
    );
    assert!(
        second_stderr.contains("kernel-verified member of the shared account CPU budget")
            || second_stderr.contains("joined the shared account CPU budget"),
        "the second caller must verify its shared membership with the kernel: {second_stderr}"
    );
    let budget = Budget::default();
    for (name, text) in [("first", &first_stderr), ("second", &second_stderr)] {
        assert!(
            text.contains(&format!("account={}", account.display())),
            "the {name} caller must use the shared account queue: {text}"
        );
        assert!(
            text.contains(&format!("memory_limit_bytes={}", budget.memory_bytes)),
            "the {name} caller must use the one machine budget: {text}"
        );
        assert!(
            text.contains("kill_on_close=true"),
            "the {name} caller must own a kill-on-close Job: {text}"
        );
        assert!(
            text.contains("cpu_rate=0"),
            "the {name} caller must not add a per-batch CPU cap by default: {text}"
        );
    }
    let ended = read_number(&first_ended_evidence);
    let started = read_number(&second_started);
    assert!(
        ended < started,
        "the second command started at {started} before the first ended at {ended}"
    );
}

#[test]
fn stop_reasons_and_startup_failures_are_distinct_and_release_the_slot() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();

    let mut normal = heavy(&account, &work);
    normal
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let normal = run(&mut normal, Duration::from_secs(120));
    assert_eq!(normal.status.code(), Some(0), "{}", stderr(&normal));
    assert!(stderr(&normal).contains("heavy: exited code=0"));

    let mut failing = heavy(&account, &work);
    failing
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-exit")
        .env("HARNESS_HEAVY_FIXTURE_EXIT", "19");
    let failing = run(&mut failing, Duration::from_secs(120));
    assert_eq!(
        failing.status.code(),
        Some(19),
        "a child's real exit code is preserved: {}",
        stderr(&failing)
    );
    assert!(stderr(&failing).contains("heavy: exited code=19"));

    let missing = Command::new(manager())
        .arg("heavy")
        .arg("--account")
        .arg(&account)
        .arg("--")
        .arg(work.join("absent-heavy-command.exe"))
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(127), "{}", stderr(&missing));
    assert!(
        stderr(&missing).contains("is unavailable"),
        "{}",
        stderr(&missing)
    );
    assert!(
        stderr(&missing).contains("never through a shell"),
        "{}",
        stderr(&missing)
    );

    // Every stop above released the slot for a subsequent permitted request.
    let mut released = heavy(&account, &work);
    released
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let released = run(&mut released, Duration::from_secs(120));
    assert_eq!(released.status.code(), Some(0), "{}", stderr(&released));
    assert!(
        !stderr(&released).contains("waiting for the account heavy-command slot"),
        "{}",
        stderr(&released)
    );

    // The local policy drives the bounded Job, and a deadline is reported as a
    // deadline instead of the command's own exit code.
    let bounded = budget_cli(
        &account,
        &["--memory-bytes", "1073741824", "--cpu-percent", "25"],
    );
    assert_eq!(bounded.status.code(), Some(0), "{}", stderr(&bounded));
    let mut limited = heavy(&account, &work);
    limited
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let limited = run(&mut limited, Duration::from_secs(120));
    let limited_stderr = stderr(&limited);
    assert_eq!(limited.status.code(), Some(0), "{limited_stderr}");
    assert!(
        limited_stderr.contains("memory_limit_bytes=1073741824")
            && limited_stderr.contains("cpu_percent=25"),
        "{limited_stderr}"
    );
    assert!(
        limited_stderr.contains("cpu_rate=3333"),
        "an intentional 25% host limit must be translated against the 75% parent, not applied raw: {limited_stderr}"
    );
    assert!(
        limited_stderr.contains("effective 24.9975% of host CPU"),
        "the effective host-relative value must be reported: {limited_stderr}"
    );

    let deadline = budget_cli(&account, &["--deadline-seconds", "1"]);
    assert_eq!(deadline.status.code(), Some(0), "{}", stderr(&deadline));
    let ended = work.join("deadline.ended");
    let mut timed = heavy(&account, &work);
    timed
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "30000")
        .env("HARNESS_HEAVY_FIXTURE_ENDED", &ended);
    let timed = run(&mut timed, Duration::from_secs(120));
    let timed_stderr = stderr(&timed);
    assert_eq!(timed.status.code(), Some(124), "{timed_stderr}");
    assert!(
        timed_stderr.contains("deadline of 1s expired"),
        "{timed_stderr}"
    );
    assert!(
        !ended.exists(),
        "a deadline must terminate the command tree before it finishes"
    );
    let released = budget_cli(&account, &["--deadline-seconds", "1800"]);
    assert_eq!(released.status.code(), Some(0), "{}", stderr(&released));
    let mut after = heavy(&account, &work);
    after
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let after = run(&mut after, Duration::from_secs(120));
    assert_eq!(
        after.status.code(),
        Some(0),
        "a deadline must release the slot: {}",
        stderr(&after)
    );
}

#[test]
fn cancelled_queue_waiter_and_interrupted_tree_release_the_slot() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();

    let holder_started = root.join("holder.started");
    let holder_ended = root.join("holder.ended");
    let mut holder = heavy(&account, &work);
    holder
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "2500")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &holder_started)
        .env("HARNESS_HEAVY_FIXTURE_ENDED", &holder_ended);
    let holder = holder.spawn().unwrap();
    wait_for_path(&holder_started, Duration::from_secs(30));

    // A queued caller that is stopped owns nothing: it neither blocks the
    // running holder nor the next permitted request.
    let mut waiter = heavy(&account, &work);
    waiter
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let mut waiter = waiter.spawn().unwrap();
    sleep(Duration::from_millis(800));
    assert!(
        waiter.try_wait().unwrap().is_none(),
        "the second caller must still be queued"
    );
    waiter.kill().unwrap();
    let cancelled = waiter.wait_with_output().unwrap();
    assert!(
        stderr(&cancelled).contains("waiting for the account heavy-command slot"),
        "{}",
        stderr(&cancelled)
    );

    let next_started = root.join("next.started");
    let mut next = heavy(&account, &work);
    next.env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &next_started);
    let next = finish(next.spawn().unwrap(), Duration::from_secs(120));
    let holder = finish(holder, Duration::from_secs(120));
    assert_eq!(holder.status.code(), Some(0), "{}", stderr(&holder));
    assert_eq!(next.status.code(), Some(0), "{}", stderr(&next));
    assert!(
        read_number(&holder_ended) < read_number(&next_started),
        "a cancelled waiter must not disturb the queue order"
    );

    // A normal root exit cleans its descendants before the slot is released.
    let clean_lock = root.join("clean.lock");
    let clean_ready = root.join("clean.ready");
    let mut clean = heavy(&account, &work);
    clean
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-tree")
        .env("HARNESS_HEAVY_FIXTURE_LOCK", &clean_lock)
        .env("HARNESS_HEAVY_FIXTURE_READY", &clean_ready);
    let clean = run(&mut clean, Duration::from_secs(120));
    assert_eq!(clean.status.code(), Some(0), "{}", stderr(&clean));
    assert!(
        wait_for_lock_free(&clean_lock, Duration::from_secs(15)),
        "a completed command left a descendant holding resources"
    );

    // An interrupted running caller: killing the wrapper must reap the whole
    // owned tree, release its resources and free the slot.
    let lock = root.join("tree.lock");
    let ready = root.join("tree.ready");
    let mut tree = heavy(&account, &work);
    tree.env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-tree")
        .env("HARNESS_HEAVY_FIXTURE_MS", "60000")
        .env("HARNESS_HEAVY_FIXTURE_LOCK", &lock)
        .env("HARNESS_HEAVY_FIXTURE_READY", &ready);
    let mut tree = tree.spawn().unwrap();
    wait_for_path(&ready, Duration::from_secs(30));
    assert!(
        !lock_free(&lock),
        "the descendant must hold the lock while the caller runs"
    );
    tree.kill().unwrap();
    let _ = tree.wait_with_output().unwrap();
    assert!(
        wait_for_lock_free(&lock, Duration::from_secs(15)),
        "an interrupted caller left descendants behind"
    );
    let mut after = heavy(&account, &work);
    after
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let after = run(&mut after, Duration::from_secs(120));
    assert_eq!(
        after.status.code(),
        Some(0),
        "an interrupted caller must release the slot: {}",
        stderr(&after)
    );
}

#[test]
fn invalid_local_policy_fails_before_the_command_starts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    let started = work.join("started");

    // Absent local policy: installed defaults, reported as such.
    let inspection = budget_cli(&account, &["--json"]);
    assert_eq!(inspection.status.code(), Some(0), "{}", stderr(&inspection));
    let effective: Value = serde_json::from_slice(&inspection.stdout).unwrap();
    let budget = Budget::default();
    assert_eq!(effective["schema"], 2);
    assert_eq!(effective["memory_bytes"], budget.memory_bytes);
    assert!(
        effective["cpu_percent"].is_null(),
        "the installed default is no per-operation limit: {effective}"
    );
    assert_eq!(effective["shared_cpu_percent"], SHARED_CPU_PERCENT);
    assert_eq!(effective["deadline_seconds"], budget.deadline_seconds);
    assert_eq!(effective["queue_wait_seconds"], budget.queue_wait_seconds);
    assert_eq!(effective["legacy_default_cpu_percent"], false);
    assert!(
        effective["cpu_policy"]
            .as_str()
            .unwrap()
            .contains("no per-operation CPU limit"),
        "{effective}"
    );
    assert_eq!(effective["source"], "defaults");
    assert!(!account.exists(), "an inspection creates no account state");

    // One owned account directory, then only malformed policies inside it.
    let seeded = budget_cli(&account, &["--deadline-seconds", "1800"]);
    assert_eq!(seeded.status.code(), Some(0), "{}", stderr(&seeded));
    for (policy, expected) in [
        (&b"{"[..], "budget.json"),
        (&b"{\"schema\":1,\"cpu_percent\":0.0}"[..], "cpu_percent"),
        (&b"{\"schema\":2}"[..], "schema"),
        (&b"{\"schema\":1,\"memory_gigabytes\":8}"[..], "policy"),
    ] {
        fs::write(account.join("budget.json"), policy).unwrap();
        let mut command = heavy(&account, &work);
        command
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
            .env("HARNESS_HEAVY_FIXTURE_MS", "10")
            .env("HARNESS_HEAVY_FIXTURE_STARTED", &started);
        let output = run(&mut command, Duration::from_secs(60));
        assert_eq!(
            output.status.code(),
            Some(2),
            "{}: {}",
            String::from_utf8_lossy(policy),
            stderr(&output)
        );
        assert!(
            stderr(&output).contains(expected),
            "{}: {}",
            String::from_utf8_lossy(policy),
            stderr(&output)
        );
        assert!(
            !started.exists(),
            "an invalid policy must fail before the command starts"
        );
    }

    // A preview reports without writing; a write applies to the same owner.
    fs::write(account.join("budget.json"), b"{\"schema\":1}").unwrap();
    let preview = budget_cli(&account, &["--deadline-seconds", "60", "--preview"]);
    assert_eq!(preview.status.code(), Some(0), "{}", stderr(&preview));
    assert_eq!(
        fs::read(account.join("budget.json")).unwrap(),
        b"{\"schema\":1}"
    );
    assert!(
        String::from_utf8_lossy(&preview.stdout).contains("deadline_seconds=60"),
        "{}",
        String::from_utf8_lossy(&preview.stdout)
    );
    let written = budget_cli(&account, &["--deadline-seconds", "60", "--json"]);
    assert_eq!(written.status.code(), Some(0), "{}", stderr(&written));
    let applied: Value = serde_json::from_slice(&written.stdout).unwrap();
    assert_eq!(applied["deadline_seconds"], 60);
    assert_eq!(applied["memory_bytes"], budget.memory_bytes);
    let mut command = heavy(&account, &work);
    command
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let output = run(&mut command, Duration::from_secs(60));
    let text = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(text.contains("deadline_seconds=60"), "{text}");

    // `--cpu-percent shared` returns the batch to the shared ceiling: the
    // report says so and the admitted Job carries no rate at all.
    let shared = budget_cli(&account, &["--cpu-percent", "shared", "--json"]);
    assert_eq!(shared.status.code(), Some(0), "{}", stderr(&shared));
    let shared: Value = serde_json::from_slice(&shared.stdout).unwrap();
    assert!(shared["cpu_percent"].is_null(), "{shared}");
    assert_eq!(shared["legacy_default_cpu_percent"], false);
    let mut command = heavy(&account, &work);
    command
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let output = run(&mut command, Duration::from_secs(60));
    let text = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(text.contains("cpu_rate=0"), "{text}");
    assert!(text.contains("no per-operation limit"), "{text}");

    // A deliberate 25% host limit is reported in host-relative terms, and a
    // legacy 50% recording is preserved but never silently treated as a new
    // default.
    let limited = budget_cli(&account, &["--cpu-percent", "25", "--json"]);
    assert_eq!(limited.status.code(), Some(0), "{}", stderr(&limited));
    let limited: Value = serde_json::from_slice(&limited.stdout).unwrap();
    assert_eq!(limited["cpu_percent"], 25.0);
    assert_eq!(limited["legacy_default_cpu_percent"], false);
    assert!(
        limited["cpu_policy"]
            .as_str()
            .unwrap()
            .contains("25% of host CPU"),
        "{limited}"
    );
    fs::write(
        account.join("budget.json"),
        b"{\"schema\":1,\"cpu_percent\":50.0}",
    )
    .unwrap();
    let legacy = budget_cli(&account, &["--json"]);
    assert_eq!(legacy.status.code(), Some(0), "{}", stderr(&legacy));
    let legacy: Value = serde_json::from_slice(&legacy.stdout).unwrap();
    assert_eq!(legacy["cpu_percent"], 50.0);
    assert_eq!(legacy["legacy_default_cpu_percent"], true);
    assert!(
        legacy["cpu_policy"]
            .as_str()
            .unwrap()
            .contains("retired 50% batch default"),
        "an ambiguous legacy value must be reported, not silently changed: {legacy}"
    );
}

#[test]
fn direct_heavy_call_joins_the_shared_budget_without_a_second_cap() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    let log = root.join("heavy.log");
    let payload_pid = root.join("payload.pid");
    let mut command = heavy_logged(&account, &work, &log);
    command
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "4000")
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &payload_pid);
    let child = command.spawn().unwrap();
    let payload = wait_for_payload(&payload_pid, Duration::from_secs(30));
    let text = wait_for_log(&log, "payload pid=", Duration::from_secs(30));
    let shared_job = field(&text, "shared account CPU budget job=");
    let heavy_job = field(&text, "job scope=aggregate name=");

    // Kernel readback asked from this test process: one account budget at the
    // installed ceiling, one lifecycle Job that keeps the memory and cleanup
    // contract without adding a CPU rate, and a payload that really belongs to
    // both instead of merely carrying a marker.
    assert_eq!(job_cpu_rate(&shared_job), shared_cpu_rate());
    assert_eq!(
        job_cpu_rate(&heavy_job),
        0,
        "the default policy must not add a second CPU cap"
    );
    let (memory, kill_on_close) = job_limits(&heavy_job);
    assert_eq!(memory, Budget::default().memory_bytes);
    assert!(kill_on_close);
    assert!(
        process_in_job(payload, &shared_job),
        "the payload must be a member of the shared account CPU budget"
    );
    assert!(
        process_in_job(payload, &heavy_job),
        "the payload must be a member of the owned lifecycle Job"
    );
    unsafe { CloseHandle(payload) };

    let output = finish(child, Duration::from_secs(120));
    assert_eq!(output.status.code(), Some(0), "{}", log_text(&log));
    let text = log_text(&log);
    assert!(text.contains("heavy: exited code=0"), "{text}");
    assert!(
        text.contains("kernel-verified member of the shared account CPU budget"),
        "the shared membership must be verified with the kernel: {text}"
    );
    assert!(
        text.contains("no per-operation limit; the shared account CPU ceiling is the CPU policy for this batch"),
        "{text}"
    );
}

#[test]
fn intentional_lower_limits_keep_host_relative_meaning() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    let policy = budget_cli(&account, &["--cpu-percent", "25"]);
    assert_eq!(policy.status.code(), Some(0), "{}", stderr(&policy));

    // A deliberately configured 25% of host CPU below the shared 75% ceiling:
    // the inner Job rate is relative to its verified parent, so the effective
    // host-relative ceiling is preserved instead of being multiplied into
    // 18.75% or applied raw as 25% of the parent.
    let log = root.join("limited.log");
    let payload_pid = root.join("limited.pid");
    let mut command = heavy_logged(&account, &work, &log);
    command
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "4000")
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &payload_pid);
    let child = command.spawn().unwrap();
    let payload = wait_for_payload(&payload_pid, Duration::from_secs(30));
    let text = wait_for_log(&log, "effective ", Duration::from_secs(30));
    let shared_job = field(&text, "shared account CPU budget job=");
    let heavy_job = field(&text, "job scope=aggregate name=");
    assert_eq!(job_cpu_rate(&shared_job), shared_cpu_rate());
    let inner = job_cpu_rate(&heavy_job);
    assert_eq!(
        inner, 3333,
        "25% of host CPU under a 75% parent is a third of the parent"
    );
    assert_ne!(
        inner, 2500,
        "the raw percentage must not be used as a parent rate"
    );
    assert_ne!(
        inner, 1875,
        "the default must not multiply nested percentages"
    );
    assert!(
        u64::from(inner) * u64::from(shared_cpu_rate()) / 10_000 <= 2500,
        "the translated cap must not exceed the documented host-relative ceiling"
    );
    let (memory, _) = job_limits(&heavy_job);
    assert_eq!(memory, Budget::default().memory_bytes);
    assert!(process_in_job(payload, &shared_job));
    unsafe { CloseHandle(payload) };
    let output = finish(child, Duration::from_secs(120));
    assert_eq!(output.status.code(), Some(0), "{}", log_text(&log));
    let text = log_text(&log);
    assert!(
        text.contains("per-operation limit 25% of host CPU; inner cpu_rate=3333 against the verified parent rate 75 (effective 24.9975% of host CPU)"),
        "the reported effective value must stay host-relative: {text}"
    );

    // A limit that is not lower than the shared ceiling adds no inner rate.
    let policy = budget_cli(&account, &["--cpu-percent", "90"]);
    assert_eq!(policy.status.code(), Some(0), "{}", stderr(&policy));
    let mut command = heavy(&account, &work);
    command
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let output = run(&mut command, Duration::from_secs(60));
    let text = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(text.contains("cpu_rate=0"), "{text}");
    assert!(
        text.contains("per-operation limit 90% of host CPU is not lower than the shared account ceiling 75%; the shared ceiling governs"),
        "{text}"
    );

    // A legacy 50% recording is preserved as a host-relative limit and
    // reported as ambiguous; it must not become a second 50% of the ceiling.
    fs::write(
        account.join("budget.json"),
        b"{\"schema\":1,\"cpu_percent\":50.0}",
    )
    .unwrap();
    let mut command = heavy(&account, &work);
    command
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let output = run(&mut command, Duration::from_secs(60));
    let text = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    assert!(
        text.contains("cpu_rate=6666"),
        "a legacy 50% must be translated to a host-relative 50%: {text}"
    );
    assert!(
        text.contains("retired 50% batch default"),
        "the ambiguous legacy value must be reported: {text}"
    );
}

#[test]
fn nested_call_inherits_one_aggregate_budget_and_proves_membership() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();

    // A real nested CLI call: the outer caller owns the lease and joins the
    // shared account budget; the inner caller adds containment only, because a
    // nested Windows Job's CPU rate is a proportion of its parent's rate.
    let nested_log = root.join("nested.log");
    let nested_started = root.join("nested.started");
    let nested_pid = root.join("nested.pid");
    let mut nested = Command::new(manager());
    nested
        .arg("heavy")
        .arg("--account")
        .arg(&account)
        .arg("--")
        .arg(manager())
        .arg("heavy")
        .arg("--account")
        .arg(&account)
        .arg("--")
        .arg(fixture_target())
        .current_dir(&work)
        .env(CPU_ACCOUNT_ENV, cpu_account(&account))
        .env_remove(CPU_PERCENT_ENV)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "2500")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &nested_started)
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &nested_pid)
        .stdout(Stdio::null())
        .stderr(Stdio::from(fs::File::create(&nested_log).unwrap()));
    let nested = nested.spawn().unwrap();
    let payload = wait_for_payload(&nested_pid, Duration::from_secs(30));
    let text = wait_for_log(&nested_log, "scope=containment", Duration::from_secs(30));
    let heavy_job = field(&text, "job scope=aggregate name=");
    let shared_job = field(&text, "shared account CPU budget job=");

    // Kernel evidence for the nested payload itself, asked while it runs: it is
    // a member of the one account budget and of the outer admitted Job, and the
    // outer Job adds no rate of its own.
    assert_eq!(job_cpu_rate(&shared_job), shared_cpu_rate());
    assert_eq!(
        job_cpu_rate(&heavy_job),
        0,
        "the admitted Job must not add a second CPU cap"
    );
    assert!(
        process_in_job(payload, &shared_job),
        "the nested payload must stay inside the shared account CPU budget"
    );
    assert!(
        process_in_job(payload, &heavy_job),
        "the nested payload must stay inside the admitted Job"
    );
    unsafe { CloseHandle(payload) };

    // While the admitted tree runs, a peer queues for the one account slot: the
    // queue and the shared budget lock are taken in one order only, so both the
    // nested tree and the queued peer make progress and complete.
    let peer_started = root.join("peer.started");
    let peer_ended = root.join("peer.ended");
    let mut peer = heavy(&account, &work);
    peer.env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &peer_started)
        .env("HARNESS_HEAVY_FIXTURE_ENDED", &peer_ended);
    let peer = finish(peer.spawn().unwrap(), Duration::from_secs(120));
    let nested = finish(nested, Duration::from_secs(120));
    let text = log_text(&nested_log);
    assert_eq!(nested.status.code(), Some(0), "{text}");
    assert!(text.contains("scope=aggregate"), "{text}");
    assert!(
        text.contains(&format!(
            "memory_limit_bytes={}",
            Budget::default().memory_bytes
        )),
        "{text}"
    );
    assert!(
        text.contains("job scope=aggregate") && text.contains("cpu_rate=0"),
        "the lease holder must keep the memory contract without a second CPU rate: {text}"
    );
    assert!(
        text.contains("this process is a verified member"),
        "the nested caller must prove membership in the admitted Job: {text}"
    );
    assert!(
        text.contains("kernel-verified member of the shared account CPU budget"),
        "the nested caller must verify its own shared membership with the kernel: {text}"
    );
    assert!(
        text.contains("scope=containment")
            && text.contains("memory_limit_bytes=0 cpu_rate=0 kill_on_close=true"),
        "the nested caller must not apply a second memory or CPU cap: {text}"
    );
    assert!(
        text.contains("this nested call adds no second CPU cap"),
        "{text}"
    );
    assert_eq!(peer.status.code(), Some(0), "{}", stderr(&peer));
    assert!(
        stderr(&peer).contains("waiting for the account heavy-command slot"),
        "the peer must queue for the one account slot: {}",
        stderr(&peer)
    );
    assert!(
        read_number(&peer_started) > read_number(&nested_started),
        "the queued peer must not start before the admitted tree's payload"
    );

    // Knowing a live holder's identity and the real admitted Job name is still
    // not containment: the marker must queue like any other caller.
    let holder_started = root.join("holder.started");
    let holder_ended = root.join("holder.ended");
    let mut holder = heavy(&account, &work);
    holder
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "2500")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &holder_started)
        .env("HARNESS_HEAVY_FIXTURE_ENDED", &holder_ended);
    let holder = holder.spawn().unwrap();
    wait_for_path(&holder_started, Duration::from_secs(30));
    let record: Value =
        serde_json::from_slice(&fs::read(account.join("holder.json")).unwrap()).unwrap();
    let forged = serde_json::json!({
        "schema": 1,
        "account": account.display().to_string(),
        "job": record["job"],
        "holder": {
            "pid": record["pid"],
            "creation_time": record["creation_time"],
        },
    })
    .to_string();
    let forged_started = root.join("forged.started");
    let mut forger = heavy(&account, &work);
    forger
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &forged_started)
        .env(LEASE_ENV, &forged);
    let forger = finish(forger.spawn().unwrap(), Duration::from_secs(120));
    let holder = finish(holder, Duration::from_secs(120));
    let forged_stderr = stderr(&forger);
    assert_eq!(holder.status.code(), Some(0), "{}", stderr(&holder));
    assert_eq!(forger.status.code(), Some(0), "{forged_stderr}");
    assert!(
        !forged_stderr.contains("inheriting the aggregate heavy-command budget"),
        "a live peer identity and a copied Job name are not containment: {forged_stderr}"
    );
    assert!(
        forged_stderr.contains("waiting for the account heavy-command slot"),
        "the forged marker must queue for the real slot: {forged_stderr}"
    );
    assert!(
        read_number(&holder_ended) < read_number(&forged_started),
        "the forged marker must run only after the real holder released the slot"
    );

    // A marker that names the shared account CPU budget Job instead of an
    // admitted heavy tree is refused as well, even with a live holder: the
    // shared group is not the admitted heavy-command tree.
    let (holder_pid, holder_creation) = current_identity();
    let mut shared_forger = heavy(&account, &work);
    shared_forger
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10")
        .env(
            "HARNESS_HEAVY_FIXTURE_STARTED",
            root.join("shared-forged.started"),
        )
        .env(
            LEASE_ENV,
            serde_json::json!({
                "schema": 1,
                "account": account.display().to_string(),
                "job": shared_job,
                "holder": {
                    "pid": holder_pid,
                    "creation_time": holder_creation,
                },
            })
            .to_string(),
        );
    let shared_forger = run(&mut shared_forger, Duration::from_secs(120));
    let shared_forged = stderr(&shared_forger);
    assert_eq!(shared_forger.status.code(), Some(0), "{shared_forged}");
    assert!(
        !shared_forged.contains("inheriting the aggregate heavy-command budget"),
        "naming the shared CPU budget Job is not heavy-command containment: {shared_forged}"
    );
    assert!(
        shared_forged.contains("scope=aggregate"),
        "a refused marker must take the real account slot: {shared_forged}"
    );
}

#[test]
fn native_build_waits_for_and_releases_the_shared_slot() {
    let temp = tempfile::Builder::new()
        .prefix("harness-heavy-build-")
        .tempdir()
        .unwrap();
    let root = temp.keep();
    let source = root.join("source");
    let state = root.join("native state");
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    fixture(&source);
    // A bounded nested probe inside the build: if the build's admission were not
    // reusable, this nested caller would report a queue wait instead.
    let policy = budget_cli(&account, &["--queue-wait-seconds", "20"]);
    assert_eq!(policy.status.code(), Some(0), "{}", stderr(&policy));
    let evidence = root.join("nested-heavy.txt");
    let holder_started = root.join("holder.started");
    let holder_log = root.join("holder.log");
    let mut holder = heavy_logged(&account, &work, &holder_log);
    holder
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "2500")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &holder_started);
    let holder = holder.spawn().unwrap();
    wait_for_path(&holder_started, Duration::from_secs(30));
    let holder_text = wait_for_log(
        &holder_log,
        "shared account CPU budget job=",
        Duration::from_secs(30),
    );
    let shared_job = field(&holder_text, "shared account CPU budget job=");
    assert_eq!(job_cpu_rate(&shared_job), shared_cpu_rate());

    let build = Command::new(manager())
        .arg("build")
        .arg("--source")
        .arg(&source)
        .arg("--state")
        .arg(&state)
        .env("CODEX_HARNESS_HEAVY_ACCOUNT", &account)
        .env(CPU_ACCOUNT_ENV, cpu_account(&account))
        .env_remove(CPU_PERCENT_ENV)
        .env("HARNESS_HEAVY_TEST_NESTED", manager())
        .env("HARNESS_HEAVY_TEST_EVIDENCE", &evidence)
        .env("HARNESS_HEAVY_TEST_SHARED_JOB", &shared_job)
        .output()
        .unwrap();
    let build_stderr = stderr(&build);
    assert!(
        build.status.success(),
        "{}; evidence {}",
        build_stderr,
        root.display()
    );
    assert!(
        build_stderr.contains("waiting for the account heavy-command slot"),
        "a native build must queue under the shared slot: {build_stderr}"
    );
    assert!(
        build_stderr.contains("holder pid="),
        "the build must observe the running heavy caller: {build_stderr}"
    );
    assert!(
        build_stderr.contains("job scope=aggregate"),
        "a direct build must own the named aggregate Job: {build_stderr}"
    );
    assert!(
        build_stderr.contains("joined the shared account CPU budget before any payload starts"),
        "a standalone build must join the common account group so its whole tree inherits it: {build_stderr}"
    );
    assert!(
        build_stderr.contains(&format!("shared account CPU budget job=\"{shared_job}\"")),
        "the build must join the same group as the rest of the account: {build_stderr}"
    );
    let nested = fs::read_to_string(&evidence).unwrap_or_default();
    assert!(nested.contains("marker=true"), "{nested}");
    assert!(nested.contains("exit=Some(0)"), "{nested}");
    assert!(
        nested.contains("verified member"),
        "the build's child must reuse the build's admission: {nested}"
    );
    assert!(
        nested.contains("shared_member=true"),
        "the build tree must be inside the shared account CPU budget, verified by the kernel from inside: {nested}"
    );
    assert!(
        nested.contains(&format!("shared_rate={}", shared_cpu_rate())),
        "the build tree must read the shared ceiling back from the kernel: {nested}"
    );
    assert!(
        !nested.contains("waiting for the account heavy-command slot"),
        "the build's child must not queue on its own admission: {nested}"
    );
    let prepared: Value = serde_json::from_slice(&build.stdout).unwrap();
    let published = Path::new(prepared["build"].as_str().unwrap());
    assert!(published.join("build.json").is_file());
    let holder = finish(holder, Duration::from_secs(120));
    assert_eq!(holder.status.code(), Some(0), "{}", stderr(&holder));

    // The build released the slot after all of its contained work exited.
    let mut after = heavy(&account, &work);
    after
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10");
    let after = run(&mut after, Duration::from_secs(60));
    let text = stderr(&after);
    assert_eq!(after.status.code(), Some(0), "{text}");
    assert!(
        !text.contains("waiting for the account heavy-command slot"),
        "a finished build must release the slot: {text}"
    );
    println!("heavy-command build evidence {}", root.display());
}

fn manager_source(program: &str) -> String {
    let dispatch = r#"
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == "finalize-build-v1") {
        if let Err(error) = harness_core::native_build::finalize(std::path::Path::new(&args[1])) {
            eprintln!("{error}"); std::process::exit(2);
        }
        return;
    }
    if args.first().is_some_and(|a| a == "check") {
        let source = args.windows(2).find(|w| w[0] == "--source").map(|w| std::path::Path::new(&w[1]));
        let build = args.windows(2).find(|w| w[0] == "--build").unwrap();
        let report = harness_core::build_identity::check(std::path::Path::new(&build[1]), source);
        println!("{}", serde_json::to_string(&report).unwrap());
        std::process::exit(i32::from(
            report.status != harness_core::build_identity::Health::Healthy,
        ));
    }
    if args.first().is_some_and(|a| a == "activate-build") {
        let state = args.windows(2).find(|w| w[0] == "--state").unwrap();
        let build = args.windows(2).find(|w| w[0] == "--build").unwrap();
        match harness_core::build_selection::activate(std::path::Path::new(&state[1]), std::path::Path::new(&build[1])) {
            Ok(result) => println!("{}", serde_json::to_string(&result).unwrap()),
            Err(error) => { eprintln!("{error}"); std::process::exit(2); }
        }
        return;
    }
"#;
    program.replacen("fn main() {", &format!("fn main() {{{dispatch}"), 1)
}

fn copy_source_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        assert!(!entry.file_type().unwrap().is_symlink());
        if path.is_dir() {
            copy_source_tree(&path, &destination.join(entry.file_name()));
        } else {
            fs::copy(&path, destination.join(entry.file_name())).unwrap();
        }
    }
}

/// The owned synthetic workspace used by the native build cases: real
/// harness-core and skill-evolution sources with a minimal manager that still
/// implements the finalization handoff.
fn fixture(source: &Path) {
    let schema = source.join(harness_core::build_identity::INSPECTION_SCHEMA);
    fs::create_dir_all(schema.parent().unwrap()).unwrap();
    fs::write(schema, "{}").unwrap();
    fs::create_dir_all(source.join("crates/manager/src")).unwrap();
    fs::create_dir_all(source.join("tools/rtk-adapter/src")).unwrap();
    fs::write(
        source.join("Cargo.toml"),
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
            .unwrap()
            .replace("crates/codex-harness", "crates/manager")
            .split("[profile.release]")
            .next()
            .unwrap()
            .to_owned()
            + "\n[profile.release]\nopt-level=0\n",
    )
    .unwrap();
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../harness-core");
    copy_source_tree(&core.join("src"), &source.join("crates/harness-core/src"));
    fs::copy(
        core.join("Cargo.toml"),
        source.join("crates/harness-core/Cargo.toml"),
    )
    .unwrap();
    let evolution = Path::new(env!("CARGO_MANIFEST_DIR")).join("../skill-evolution");
    copy_source_tree(
        &evolution.join("src"),
        &source.join("crates/skill-evolution/src"),
    );
    fs::copy(
        evolution.join("Cargo.toml"),
        source.join("crates/skill-evolution/Cargo.toml"),
    )
    .unwrap();
    for (directory, name) in [
        ("crates/manager", "codex-harness"),
        ("tools/rtk-adapter", "harness-rtk"),
        ("crates/token-audit", "token-audit"),
    ] {
        fs::create_dir_all(source.join(directory).join("src")).unwrap();
        fs::write(
            source.join(directory).join("Cargo.toml"),
            format!(
                "[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n{}",
                if name == "codex-harness" {
                    "[dependencies]\nharness-core={path='../harness-core'}\nserde_json.workspace=true\n"
                } else {
                    ""
                }
            ),
        )
        .unwrap();
        fs::write(
            source.join(directory).join("src/main.rs"),
            if name == "codex-harness" {
                manager_source("fn main() { println!(\"owned native fixture\"); }\n")
            } else {
                "fn main() {}\n".into()
            },
        )
        .unwrap();
    }
    let manifest = fs::read_to_string(source.join("Cargo.toml")).unwrap();
    let members = manifest
        .split("members = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap();
    for member in members.split_whitespace() {
        let member = member.trim_matches(|character| character == '"' || character == ',');
        if member.is_empty() {
            continue;
        }
        let directory = source.join(member);
        if directory.join("Cargo.toml").exists() {
            continue;
        }
        fs::create_dir_all(directory.join("src")).unwrap();
        fs::write(
            directory.join("Cargo.toml"),
            format!(
                "[package]\nname='{}'\nversion='0.1.0'\nedition='2024'\n",
                member.rsplit('/').next().unwrap_or(member)
            ),
        )
        .unwrap();
        fs::write(directory.join("src/lib.rs"), "").unwrap();
    }
    fs::create_dir_all(source.join("crates/manager/src/bin")).unwrap();
    for name in harness_core::build_identity::BINARIES {
        if !["codex-harness.exe", "harness-rtk.exe"].contains(name) {
            fs::write(
                source
                    .join("crates/manager/src/bin")
                    .join(name.replace(".exe", ".rs")),
                "fn main() {}\n",
            )
            .unwrap();
        }
    }
    // The manager's build script proves that the compilation tree inherits the
    // admission marker and can reuse the build's own admission without queueing,
    // and it asks the kernel whether this build tree really belongs to the
    // account CPU budget Job the owner printed (a name is not membership).
    fs::write(
        source.join("crates/manager/build.rs"),
        r#"
fn main() {
    let marker = std::env::var("CODEX_HARNESS_HEAVY_LEASE_V1").is_ok();
    let manager = std::env::var("HARNESS_HEAVY_TEST_NESTED").expect("nested manager path");
    let nested = std::process::Command::new(manager)
        .args(["heavy", "--", "cargo", "--version"])
        .output()
        .expect("nested heavy command");
    let shared = std::env::var("HARNESS_HEAVY_TEST_SHARED_JOB");
    let shared_member = shared.as_deref().map(job_member).unwrap_or(false);
    let shared_rate = shared.as_deref().map(job_rate).unwrap_or(0);
    let evidence = format!(
        "marker={marker} exit={:?} shared_member={shared_member} shared_rate={shared_rate} stderr={}",
        nested.status.code(),
        String::from_utf8_lossy(&nested.stderr).replace('\n', " | ")
    );
    if let Ok(path) = std::env::var("HARNESS_HEAVY_TEST_EVIDENCE") {
        std::fs::write(path, &evidence).expect("evidence file");
    }
    eprintln!("fixture-nested-heavy {evidence}");
    assert!(
        nested.status.success(),
        "nested heavy through the build failed: {evidence}"
    );
}

/// Documented JOB_OBJECT_QUERY right (winnt.h); a query-only handle can neither
/// assign, configure nor terminate the object it opens.
const JOB_OBJECT_QUERY: u32 = 0x0004;
const JOB_OBJECT_CPU_RATE_CONTROL_ENABLE: u32 = 0x1;
const JOB_OBJECT_CPU_RATE_CONTROL_CLASS: i32 = 15;

#[repr(C)]
struct CpuRateControl {
    control_flags: u32,
    cpu_rate: u32,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenJobObjectW(access: u32, inherit: i32, name: *const u16) -> *mut core::ffi::c_void;
    fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
    fn IsProcessInJob(
        process: *mut core::ffi::c_void,
        job: *mut core::ffi::c_void,
        result: *mut i32,
    ) -> i32;
    fn QueryInformationJobObject(
        job: *mut core::ffi::c_void,
        class: i32,
        info: *mut core::ffi::c_void,
        size: u32,
        returned: *mut u32,
    ) -> i32;
    fn GetCurrentProcess() -> *mut core::ffi::c_void;
}

fn open_job(name: &str) -> *mut core::ffi::c_void {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, wide.as_ptr()) }
}

fn job_member(name: &str) -> bool {
    let job = open_job(name);
    if job.is_null() {
        return false;
    }
    let mut member = 0;
    let queried = unsafe { IsProcessInJob(GetCurrentProcess(), job, &mut member) };
    unsafe { CloseHandle(job) };
    queried != 0 && member != 0
}

fn job_rate(name: &str) -> u32 {
    let job = open_job(name);
    if job.is_null() {
        return 0;
    }
    let mut info = CpuRateControl {
        control_flags: 0,
        cpu_rate: 0,
    };
    let queried = unsafe {
        QueryInformationJobObject(
            job,
            JOB_OBJECT_CPU_RATE_CONTROL_CLASS,
            (&raw mut info).cast(),
            std::mem::size_of::<CpuRateControl>() as u32,
            std::ptr::null_mut(),
        )
    };
    unsafe { CloseHandle(job) };
    if queried == 0 || info.control_flags & JOB_OBJECT_CPU_RATE_CONTROL_ENABLE == 0 {
        return 0;
    }
    info.cpu_rate
}
"#,
    )
    .unwrap();
    let lock = Command::new("cargo")
        .args(["generate-lockfile", "--offline"])
        .current_dir(source)
        .output()
        .unwrap();
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
}

/// Re-exec entry: join the named allowance, then run one heavy command.
#[test]
fn uncapped_heavy_caller() {
    let Ok(spec_path) = std::env::var("HARNESS_UNCAPPED_HEAVY_SPEC") else {
        return;
    };
    let spec: Value = serde_json::from_slice(&fs::read(&spec_path).unwrap()).unwrap();
    let job = spec["job"].as_str().unwrap();
    assert!(assign_current_to_job(job), "could not join {job}");
    assert!(
        current_process_in_job(job),
        "join was not observed by the kernel"
    );
    let mut command = std::process::Command::new(spec["program"].as_str().unwrap());
    command
        .args(
            spec["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|arg| arg.as_str().unwrap()),
        )
        .current_dir(spec["cwd"].as_str().unwrap())
        .env(CPU_ACCOUNT_ENV, spec["account"].as_str().unwrap());
    for (name, value) in spec["env"].as_object().unwrap() {
        command.env(name, value.as_str().unwrap());
    }
    let output = command.output().unwrap();
    fs::write(
        spec["result"].as_str().unwrap(),
        serde_json::to_vec(&serde_json::json!({
            "code": output.status.code(),
            "stderr": String::from_utf8_lossy(&output.stderr),
        }))
        .unwrap(),
    )
    .unwrap();
}

fn assign_current_to_job(name: &str) -> bool {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    unsafe {
        let job = OpenJobObjectW(0x0001, 0, wide.as_ptr());
        if job.is_null() {
            return false;
        }
        let assigned = AssignProcessToJobObject(job, GetCurrentProcess());
        CloseHandle(job);
        assigned != 0
    }
}

fn current_process_in_job(name: &str) -> bool {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    unsafe {
        let job = OpenJobObjectW(JOB_OBJECT_QUERY, 0, wide.as_ptr());
        if job.is_null() {
            return false;
        }
        let mut member = 0;
        let queried = IsProcessInJob(GetCurrentProcess(), job, &mut member);
        CloseHandle(job);
        queried != 0 && member != 0
    }
}

#[test]
fn uncapped_heavy_command_escapes_a_capped_caller_without_lifting_peers() {
    use harness_core::process::{Cancellation, Deadline, SharedCpuBudget};
    use harness_core::process_service::{self, SharedCpuCoverage};
    use std::collections::BTreeMap;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    let cpu = cpu_account(&account);
    let budget = SharedCpuBudget::acquire(&cpu, SHARED_CPU_PERCENT).unwrap();
    let job_name = budget.name().to_owned();
    let rate = budget.snapshot().unwrap().cpu_rate;
    assert_eq!(rate, shared_cpu_rate());

    let peer_ready = root.join("peer.ready");
    let peer_stop = root.join("peer.stop");
    let mut peer = std::process::Command::new(std::env::current_exe().unwrap());
    peer.args(["--exact", "capped_allowance_peer", "--test-threads=1"])
        .env("HARNESS_CAPPED_PEER_JOB", &job_name)
        .env("HARNESS_CAPPED_PEER_READY", &peer_ready)
        .env("HARNESS_CAPPED_PEER_STOP", &peer_stop)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut peer_child = peer.spawn().unwrap();
    wait_for_path(&peer_ready, Duration::from_secs(20));
    let peer_payload =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, peer_child.id()) };
    assert!(
        process_in_job(peer_payload, &job_name),
        "capped peer did not join the shared allowance"
    );

    let service_root = root.join("service");
    fs::create_dir_all(&service_root).unwrap();
    let mut environment = BTreeMap::new();
    environment.insert("SystemRoot".into(), std::env::var("SystemRoot").unwrap());
    environment.insert(CPU_ACCOUNT_ENV.into(), cpu.display().to_string());
    unsafe { std::env::set_var(CPU_ACCOUNT_ENV, &cpu) };
    let service = process_service::spawn(
        Path::new(env!("CARGO_BIN_EXE_harness-service-fixture")),
        &service_root,
        vec!["serve".into()],
        environment,
        Deadline::after(Duration::from_secs(30)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    unsafe { std::env::remove_var(CPU_ACCOUNT_ENV) };
    assert!(
        matches!(
            process_service::shared_cpu_coverage(&service, Some(&cpu)),
            SharedCpuCoverage::Covered { .. }
        ),
        "shared service was not admitted"
    );

    let started = root.join("exception.pid");
    let spec_path = root.join("heavy-spec.json");
    let result_path = root.join("heavy-result.json");
    let spec = serde_json::json!({
        "spec_path": spec_path,
        "job": job_name,
        "program": manager(),
        "cwd": work,
        "account": cpu,
        "result": result_path,
        "args": ["heavy", "--account", account, "--uncapped", "--", fixture_target()],
        "env": {
            "HARNESS_LAUNCH_FIXTURE_MODE": "heavy-hold",
            "HARNESS_HEAVY_FIXTURE_MS": "1500",
            "HARNESS_LAUNCH_FIXTURE_STARTED": started,
            "HARNESS_HEAVY_FIXTURE_STARTED": root.join("exception.started"),
            "HARNESS_HEAVY_FIXTURE_ENDED": root.join("exception.ended"),
            "CODEX_HOME": root.join("home"),
        },
    });
    fs::write(&spec_path, serde_json::to_vec(&spec).unwrap()).unwrap();
    let caller = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "uncapped_heavy_caller", "--test-threads=1"])
        .env("HARNESS_UNCAPPED_HEAVY_SPEC", &spec_path)
        .env(CPU_ACCOUNT_ENV, &cpu)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let payload = wait_for_payload(&started, Duration::from_secs(40));
    assert!(
        !process_in_job(payload, &job_name),
        "uncapped payload remained in the shared allowance"
    );
    assert!(
        process_in_job(peer_payload, &job_name),
        "the capped peer lost the shared allowance"
    );
    assert!(
        service.in_shared_cpu_budget(&budget).unwrap(),
        "shared service left the allowance during the exception"
    );
    assert_eq!(budget.snapshot().unwrap().cpu_rate, rate);
    let caller = caller.wait_with_output().unwrap();
    assert!(
        caller.status.success(),
        "capped heavy caller failed: {}",
        String::from_utf8_lossy(&caller.stderr)
    );
    let result: Value = serde_json::from_slice(&fs::read(&result_path).unwrap()).unwrap();
    let stderr = result["stderr"].as_str().unwrap();
    assert_eq!(result["code"], serde_json::json!(0), "{stderr}");
    assert!(
        stderr.contains("explicit uncapped invocation")
            && stderr.contains("can exceed")
            && stderr.contains("75%"),
        "{stderr}"
    );
    unsafe { CloseHandle(payload) };

    let next_pid = root.join("next.pid");
    let next_log = root.join("next.log");
    let mut next = heavy_logged(&account, &work, &next_log);
    next.env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "400")
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &next_pid)
        .env("HARNESS_HEAVY_FIXTURE_STARTED", root.join("next.started"))
        .env("HARNESS_HEAVY_FIXTURE_ENDED", root.join("next.ended"));
    let next_child = next.spawn().unwrap();
    let next_payload = wait_for_payload(&next_pid, Duration::from_secs(40));
    assert!(
        process_in_job(next_payload, &job_name),
        "the next default command was not capped"
    );
    let next_text = wait_for_log(&next_log, "heavy: exited code=0", Duration::from_secs(40));
    assert!(
        !next_text.contains("explicit uncapped invocation"),
        "the exception persisted: {next_text}"
    );
    let _ = next_child.wait_with_output();
    unsafe { CloseHandle(next_payload) };
    fs::write(&peer_stop, "stop").unwrap();
    let _ = peer_child.wait();
    unsafe { CloseHandle(peer_payload) };
    let _ = service.terminate(0);
}

/// Holds a kernel membership in the named allowance until the stop file appears.
#[test]
fn capped_allowance_peer() {
    let Ok(job) = std::env::var("HARNESS_CAPPED_PEER_JOB") else {
        return;
    };
    assert!(assign_current_to_job(&job), "peer could not join {job}");
    assert!(current_process_in_job(&job), "peer join was not observed");
    fs::write(std::env::var("HARNESS_CAPPED_PEER_READY").unwrap(), "ready").unwrap();
    let stop = std::env::var("HARNESS_CAPPED_PEER_STOP").unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !Path::new(&stop).exists() {
        assert!(Instant::now() < deadline, "capped peer was not stopped");
        sleep(Duration::from_millis(50));
    }
}

fn write_shared_cpu_policy(account: &Path, body: &[u8]) {
    let cpu = cpu_account(account);
    fs::create_dir_all(&cpu).unwrap();
    fs::write(cpu.join("shared-cpu-policy.json"), body).unwrap();
}

#[test]
fn preserved_policy_edit_changes_heavy_admission() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    write_shared_cpu_policy(
        &account,
        br#"{"schema":1,"ceiling_percent":40.0,"escape_hatch":"CODEX_HARNESS_CPU_PERCENT"}"#,
    );
    let log = root.join("heavy.log");
    let payload_pid = root.join("payload.pid");
    let mut command = heavy_logged(&account, &work, &log);
    command
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "20000")
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &payload_pid);
    let child = command.spawn().unwrap();
    let _child = KillOnDrop(child);
    let payload = wait_for_payload(&payload_pid, Duration::from_secs(30));
    let text = wait_for_log(
        &log,
        "shared account CPU budget job=",
        Duration::from_secs(30),
    );
    let shared_job = field(&text, "shared account CPU budget job=");
    assert_eq!(job_cpu_rate(&shared_job), 4_000, "{text}");
    assert!(
        process_in_job(payload, &shared_job),
        "the payload must be admitted at the preserved policy ceiling"
    );
    unsafe { CloseHandle(payload) };
}

#[test]
fn escape_hatch_wins_over_heavy_policy_record() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    write_shared_cpu_policy(&account, br#"{"schema":1,"ceiling_percent":40.0}"#);
    let log = root.join("heavy.log");
    let payload_pid = root.join("payload.pid");
    let mut command = heavy_logged(&account, &work, &log);
    command
        .env(CPU_PERCENT_ENV, "20")
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "20000")
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &payload_pid);
    let child = command.spawn().unwrap();
    let _child = KillOnDrop(child);
    let payload = wait_for_payload(&payload_pid, Duration::from_secs(30));
    let text = wait_for_log(
        &log,
        "shared account CPU budget job=",
        Duration::from_secs(30),
    );
    let shared_job = field(&text, "shared account CPU budget job=");
    assert_eq!(job_cpu_rate(&shared_job), 2_000, "{text}");
    assert!(process_in_job(payload, &shared_job));
    unsafe { CloseHandle(payload) };
}

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn malformed_policy_warns_without_substituting_heavy_ceiling() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    let body = b"{\"schema\":1}";
    write_shared_cpu_policy(&account, body);
    let mut command = heavy(&account, &work);
    let output = run(&mut command, Duration::from_secs(60));
    let err = stderr(&output);
    assert_eq!(output.status.code(), Some(0), "{err}");
    assert!(err.contains("no substitute ceiling"), "{err}");
    assert!(err.contains("cause") || err.contains("not usable"), "{err}");
    assert!(err.contains("recovery:"), "{err}");
    assert!(
        !err.contains("shared account CPU budget job="),
        "a malformed record must not establish a group: {err}"
    );
    assert!(
        !cpu_account(&account).join("cpu-budget.json").exists(),
        "fail-open must not write an ownership record for a substitute ceiling"
    );
    assert_eq!(
        fs::read(cpu_account(&account).join("shared-cpu-policy.json")).unwrap(),
        body
    );
}
