//! Real heavy-command CLI cases in owned account roots, without models or globals.
//!
//! Every case uses a synthetic account directory, so the machine queue and its
//! budget are exercised through the same owner while unrelated consumer state
//! stays untouched.
#![cfg(windows)]
use harness_core::heavy_command::{Budget, LEASE_ENV};
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Child, Command, Output, Stdio},
    thread::sleep,
    time::{Duration, Instant},
};

fn manager() -> &'static str {
    env!("CARGO_BIN_EXE_codex-harness")
}

fn fixture_target() -> &'static str {
    env!("CARGO_BIN_EXE_harness-launch-fixture")
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
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
    let first_started = root.join("first.started");
    let first_ended = root.join("first.ended");
    let second_started = root.join("second.started");
    let first_ended_evidence = first_ended.clone();

    let mut first = heavy(&account, &root.join("checkout-one"));
    first
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "1500")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &first_started)
        .env("HARNESS_HEAVY_FIXTURE_ENDED", &first_ended)
        .env("CODEX_HOME", root.join("home-one"));
    let first = first.spawn().unwrap();
    wait_for_path(&first_started, Duration::from_secs(30));

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
    let first_stderr = stderr(&first);
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
    assert_eq!(effective["memory_bytes"], budget.memory_bytes);
    assert_eq!(effective["cpu_percent"], budget.cpu_percent);
    assert_eq!(effective["deadline_seconds"], budget.deadline_seconds);
    assert_eq!(effective["queue_wait_seconds"], budget.queue_wait_seconds);
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
}

#[test]
fn nested_call_inherits_one_aggregate_budget_and_proves_membership() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let account = root.join("account");
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();

    // A real nested CLI call: the outer caller owns the lease and applies the
    // aggregate budget; the inner caller adds containment only, because a
    // nested Windows Job's CPU rate is a proportion of its parent's rate.
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
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "10")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let nested = finish(nested.spawn().unwrap(), Duration::from_secs(120));
    let text = stderr(&nested);
    assert_eq!(nested.status.code(), Some(0), "{text}");
    assert!(text.contains("scope=aggregate"), "{text}");
    assert!(
        text.contains(&format!(
            "memory_limit_bytes={}",
            Budget::default().memory_bytes
        )),
        "{text}"
    );
    assert!(text.contains("cpu_rate=5000"), "{text}");
    assert!(
        text.contains("this process is a verified member"),
        "the nested caller must prove membership in the admitted Job: {text}"
    );
    assert!(
        text.contains("scope=containment")
            && text.contains("memory_limit_bytes=0 cpu_rate=0 kill_on_close=true"),
        "the nested caller must not apply a second memory or CPU cap: {text}"
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
        !forged_stderr.contains("verified member"),
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
    let holder_started = root.join("holder.started");
    let mut holder = heavy(&account, &work);
    holder
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "2500")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &holder_started);
    let holder = holder.spawn().unwrap();
    wait_for_path(&holder_started, Duration::from_secs(30));

    let build = Command::new(manager())
        .arg("build")
        .arg("--source")
        .arg(&source)
        .arg("--state")
        .arg(&state)
        .env("CODEX_HARNESS_HEAVY_ACCOUNT", &account)
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
