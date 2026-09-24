//! Actual installed OpenCode launch entry point on owned targets: shared account
//! CPU admission, native forwarding and the owned registration lifecycle.
#![cfg(windows)]
use harness_core::{
    build_identity::{self, BINARIES, BuildRecord, INSPECTION_SCHEMA, SCHEMA},
    build_selection,
    console::{ConsoleSession, ConsoleSpec},
    core_install,
    native_launcher::{self, OpenCodeUpstream},
    process::{
        Cancellation, CommandSpec, Deadline, SHARED_CPU_PERCENT, SharedCpuBudget, StopReason,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

/// Account override used by every fixture launch: an ordinary launch must not
/// touch the developer's real account state from a test, and the lowered test
/// ceiling stays scoped to this account directory.
const CPU_ACCOUNT_ENV: &str = "CODEX_HARNESS_CPU_ACCOUNT";
/// Machine-local ceiling override read by the launcher before admission.
const CPU_PERCENT_ENV: &str = "CODEX_HARNESS_CPU_PERCENT";
/// A relative account location is refused by the launcher, so a misconfigured
/// machine-local value must not silently create a second allowance.
const RELATIVE_ACCOUNT: &str = "relative-cpu-account";
/// Lowered rate for measurement: the spinning demand stays many times the
/// allowance, so a working cap is unambiguous.
const TEST_RATE: f64 = 0.5;
/// Leading text of every CPU fail-open notice. It is asserted on the diagnostic
/// channel and never on stdout.
const CAP_WARNING: &str = "codex-harness: shared agent CPU cap not verified";
/// Leading text of the warned fallback for an unusable registration record.
const REGISTRATION_NOTICE: &str = "codex-harness: OpenCode launch registration unavailable";

struct Fixture {
    root: tempfile::TempDir,
    home: PathBuf,
    state: PathBuf,
    build: PathBuf,
    launcher: PathBuf,
    codex: PathBuf,
    account: PathBuf,
    record: PathBuf,
    empty_path: PathBuf,
}

impl Fixture {
    /// One installed native build holding both launchers, one owned harness home
    /// and one isolated account budget directory.
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("opencode-launch проверка-")
            .tempdir()
            .unwrap();
        let home = root.path().join("home");
        let source = root.path().join("source");
        let state = root.path().join("state");
        let build = state.join("builds/fixture");
        for path in [
            home.join("harness"),
            source.join("global"),
            source.join("crates/one/src"),
            source.join("tools/rtk-adapter/src"),
            source.join(INSPECTION_SCHEMA).parent().unwrap().to_owned(),
            build.clone(),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        for name in [
            "Cargo.toml",
            "Cargo.lock",
            "crates/one/src/lib.rs",
            "tools/rtk-adapter/src/lib.rs",
            INSPECTION_SCHEMA,
        ] {
            fs::write(source.join(name), "fixture").unwrap();
        }
        fs::write(
            source.join("global/harness.config.toml"),
            "approval_policy = 'never'\n",
        )
        .unwrap();
        fs::write(source.join("global/kit.json"), serde_json::to_vec(&json!({"schema":1,"profile_name":"harness","profile":"global/harness.config.toml","instructions":"AGENTS.md","skills":"skills","agents":"agents","hooks":"hooks.json","token_hooks":"rtk-hooks.json"})).unwrap()).unwrap();
        fs::write(state.join("owner"), "codex-harness-native-state-v1\n").unwrap();
        let launcher = build.join("opencode.exe");
        let codex = build.join("codex.exe");
        fs::copy(env!("CARGO_BIN_EXE_opencode"), &launcher).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_codex"), &codex).unwrap();
        let mut binaries = BTreeMap::new();
        for name in BINARIES {
            let path = build.join(name);
            if *name != "codex.exe" {
                fs::write(&path, name).unwrap();
            }
            binaries.insert(name.to_string(), build_identity::hash_file(&path).unwrap());
        }
        let record = BuildRecord {
            schema: SCHEMA,
            source_root: source.clone(),
            source: build_identity::source_identity(&source).unwrap(),
            rustc: "fixture".into(),
            cargo: "fixture".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
            binaries,
        };
        fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        build_selection::activate(&state, &build).unwrap();
        let empty_path = root.path().join("empty-path");
        fs::create_dir_all(&empty_path).unwrap();
        Self {
            record: home.join("harness/opencode-launch.json"),
            account: root.path().join("cpu-account"),
            root,
            home,
            state,
            build,
            launcher,
            codex,
            empty_path,
        }
    }

    /// Write the owned record with the bytes the installer produces.
    fn record_upstream(&self, executable: &Path) {
        let upstream = OpenCodeUpstream {
            executable: executable.canonicalize().unwrap(),
            sha256: build_identity::hash_file(executable).unwrap(),
        };
        fs::write(
            &self.record,
            native_launcher::opencode_record_bytes(&upstream).unwrap(),
        )
        .unwrap();
    }

    /// The kit's OpenCode launcher with this fixture's environment. `PATH` never
    /// carries the developer's ambient commands: a fallback must resolve exactly
    /// the owned fixture directory a test supplies.
    fn command(&self) -> Command {
        let mut command = Command::new(&self.launcher);
        command
            .current_dir(self.root.path())
            .env("CODEX_HOME", &self.home)
            .env(CPU_ACCOUNT_ENV, &self.account)
            .env_remove(CPU_PERCENT_ENV)
            .env_remove("HARNESS_LAUNCH_FIXTURE_MODE")
            .env("PATH", &self.empty_path);
        command
    }

    /// The Codex launcher of the same build, so one test proves both agents join
    /// one account allowance.
    fn codex_command(&self) -> Command {
        let mut command = Command::new(&self.codex);
        command
            .current_dir(self.root.path())
            .env("CODEX_HOME", &self.home)
            .env(CPU_ACCOUNT_ENV, &self.account)
            .env_remove(CPU_PERCENT_ENV)
            .env_remove("HARNESS_LAUNCH_FIXTURE_MODE")
            .env("PATH", &self.empty_path);
        command
    }

    /// Register the same synthetic payload for the Codex launcher.
    fn register_codex_consumer(&self, consumer: &Path) {
        fs::write(
            self.home.join("harness/native-launch.json"),
            serde_json::to_vec(&json!({
                "schema": 1,
                "state": self.state,
                "upstream": {
                    "executable": consumer,
                    "sha256": build_identity::hash_file(consumer).unwrap(),
                    "package": null
                }
            }))
            .unwrap(),
        )
        .unwrap();
    }

    /// One real console session of the OpenCode launcher.
    fn console(&self, mode: &str) -> ConsoleSession {
        let mut spec = CommandSpec::new(&self.launcher);
        spec.current_dir = Some(self.root.path().to_owned());
        spec.env.insert(
            "CODEX_HOME".into(),
            Some(self.home.clone().into_os_string()),
        );
        spec.env.insert(
            CPU_ACCOUNT_ENV.into(),
            Some(self.account.clone().into_os_string()),
        );
        // Absent means "inherit the caller's ceiling override", which a test
        // must not pick up from the developer's environment.
        spec.env.insert(CPU_PERCENT_ENV.into(), None);
        spec.env.insert(
            "PATH".into(),
            Some(self.empty_path.clone().into_os_string()),
        );
        spec.env
            .insert("HARNESS_LAUNCH_FIXTURE_MODE".into(), Some(mode.into()));
        ConsoleSession::spawn(ConsoleSpec::new(spec)).unwrap()
    }
}

fn host_logical_processors() -> u32 {
    std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1)
}

/// Spinning threads per process. The spinning processes must demand several
/// times the lowered allowance, or the measurement proves nothing; the count
/// grows with the host only when a fixed count would not.
fn spinner_threads(cpus: u32, percent: f64) -> u64 {
    (percent / 100.0 * f64::from(cpus) * 2.0).ceil().max(2.0) as u64
}

/// Kernel rate units of the shared budget: 0.01% steps, like `Limits`.
fn cpu_rate_units(percent: f64) -> u32 {
    (percent * 100.0).floor() as u32
}

fn receipt(path: &Path) -> Value {
    let bytes =
        fs::read(path).unwrap_or_else(|error| panic!("{} is missing: {error}", path.display()));
    serde_json::from_slice(&bytes).unwrap()
}

fn wait_for_path(path: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "{} did not appear",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Bounded wait for kernel membership accounting: admission is observed from the
/// object itself, not from payload cooperation.
fn wait_for_members(budget: &SharedCpuBudget, expected: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if budget.snapshot().unwrap().active_processes >= expected {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Bounded wait for a drained group, so a session's exit cannot leave its tree
/// behind in the shared accounting object.
fn wait_for_empty(budget: &SharedCpuBudget) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if budget.snapshot().unwrap().active_processes == 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Observable account state: names for a directory, bytes for a plain file,
/// nothing when absent. Deliberately format-agnostic: the point is that a
/// degraded launch persists and changes no machine-local budget state.
fn account_state(account: &Path) -> String {
    match fs::metadata(account) {
        Err(_) => "absent".into(),
        Ok(metadata) if metadata.is_dir() => {
            let mut names: Vec<_> = fs::read_dir(account)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            format!("dir {names:?}")
        }
        Ok(_) => format!(
            "file {:?}",
            String::from_utf8_lossy(&fs::read(account).unwrap())
        ),
    }
}

fn wait_for(session: &ConsoleSession, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.transcript().contains(text) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        session.transcript().contains(text),
        "{}",
        session.transcript()
    );
}

fn rustc() -> PathBuf {
    let output = Command::new("where.exe")
        .arg("rustc.exe")
        .output()
        .expect("where.exe runs");
    let found = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(PathBuf::from)
        .find(|path| path.is_file());
    found.unwrap_or_else(|| {
        let home = env::var_os("USERPROFILE").expect("USERPROFILE");
        PathBuf::from(home).join(".cargo/bin/rustc.exe")
    })
}

/// The synthetic direct executable used by the shared-budget acceptance:
/// compiled once per test binary with the toolchain that already builds this
/// test, so no product entry point and no external dependency is involved.
fn consumer() -> &'static Path {
    static CONSUMER: OnceLock<PathBuf> = OnceLock::new();
    CONSUMER.get_or_init(|| {
        let root = tempfile::Builder::new()
            .prefix("cpu-budget-consumer-")
            .tempdir()
            .unwrap()
            .keep();
        let source = root.join("cpu_budget_consumer.rs");
        fs::write(&source, include_str!("fixtures/cpu_budget_consumer.rs")).unwrap();
        let executable = root.join("cpu-budget-consumer.exe");
        let compiled = Command::new(rustc())
            .arg(&source)
            .arg("--edition=2024")
            .arg("-o")
            .arg(&executable)
            .current_dir(&root)
            .stdin(Stdio::null())
            .output()
            .expect("rustc runs");
        assert!(
            compiled.status.success(),
            "cpu budget consumer fixture did not compile: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        assert!(executable.is_file(), "{}", executable.display());
        executable
    })
}

/// The synthetic installed OpenCode: a native executable that reports its own
/// kernel membership and starts a grandchild, so admission is proven from the
/// start of each process's execution.
struct Consumer {
    directory: PathBuf,
    starts: PathBuf,
}

impl Consumer {
    fn new(root: &Path, name: &str) -> Self {
        let directory = root.join(format!("consumer-{name}"));
        fs::create_dir_all(&directory).unwrap();
        Self {
            starts: directory.join("starts.txt"),
            directory,
        }
    }

    fn configure(
        &self,
        command: &mut Command,
        job: &str,
        threads: u64,
        spin_ms: u64,
        leaf: bool,
        exit: i32,
    ) {
        command
            .env("HARNESS_CPU_FIXTURE_DIR", &self.directory)
            .env("HARNESS_CPU_FIXTURE_JOB", job)
            .env("HARNESS_CPU_FIXTURE_THREADS", threads.to_string())
            .env("HARNESS_CPU_FIXTURE_SPIN_MS", spin_ms.to_string())
            .env("HARNESS_CPU_FIXTURE_LEAF", if leaf { "1" } else { "0" })
            .env("HARNESS_CPU_FIXTURE_EXIT", exit.to_string())
            .env("HARNESS_CPU_FIXTURE_STARTS", &self.starts);
    }

    fn tree(&self) -> Value {
        receipt(&self.directory.join("tree.json"))
    }

    fn leaf(&self) -> Value {
        receipt(&self.directory.join("leaf.json"))
    }

    fn starts(&self) -> Vec<String> {
        fs::read_to_string(&self.starts)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// The installed OpenCode payload of the forwarding, fallback and interactive
/// cases: a native double that reports the arguments, streams and environment it
/// actually received.
fn forwarding_upstream(f: &Fixture) -> PathBuf {
    let directory = f.root.path().join("installed");
    fs::create_dir_all(&directory).unwrap();
    let upstream = directory.join("opencode.exe");
    fs::copy(env!("CARGO_BIN_EXE_harness-launch-fixture"), &upstream).unwrap();
    upstream
}

/// The upstream one owned record pins, as the installer and launcher write it.
fn pinned_record(record: &Path) -> OpenCodeUpstream {
    let registration: native_launcher::OpenCodeRegistration =
        serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    assert_eq!(registration.schema, native_launcher::OPENCODE_RECORD_SCHEMA);
    registration.upstream
}

#[test]
fn installed_opencode_session_admits_payload_and_grandchildren_under_a_lowered_test_rate() {
    let f = Fixture::new();
    let cpus = host_logical_processors();
    let threads = spinner_threads(cpus, TEST_RATE);
    let spin_ms = 3000;
    // One explicit isolated account directory, established at a lowered rate
    // before the session starts: the launcher must join this exact object.
    let budget = SharedCpuBudget::acquire(&f.account, TEST_RATE).unwrap();
    let name = budget.name().to_owned();
    assert_eq!(
        budget.snapshot().unwrap().cpu_rate,
        cpu_rate_units(TEST_RATE)
    );
    f.record_upstream(consumer());
    let payload = Consumer::new(f.root.path(), "admitted");
    let mut command = f.command();
    command.env(CPU_PERCENT_ENV, TEST_RATE.to_string());
    payload.configure(&mut command, &name, threads, spin_ms, true, 21);
    let before = budget.snapshot().unwrap();
    let started = Instant::now();
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code(),
        Some(21),
        "exit status must survive the ordered spawn: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "the account budget must not write to stdout: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    for notice in [CAP_WARNING, REGISTRATION_NOTICE] {
        assert!(
            !stderr.contains(notice),
            "an admitted and pinned launch must not warn: {stderr}"
        );
    }
    // The direct executable and its grandchild each read their own kernel
    // membership in the named account object at the start of their execution.
    let tree = payload.tree();
    let leaf = payload.leaf();
    assert_eq!(tree["role"], json!("tree"), "{tree}");
    assert_eq!(leaf["role"], json!("leaf"), "{leaf}");
    assert_eq!(tree["in_shared"], json!(true), "{tree}");
    assert_eq!(leaf["in_shared"], json!(true), "{leaf}");
    assert_eq!(tree["in_any_job"], json!(true), "{tree}");
    assert_eq!(leaf["in_any_job"], json!(true), "{leaf}");
    assert_eq!(tree["child"], leaf["pid"], "{tree} {leaf}");
    let starts = payload.starts();
    assert_eq!(starts.len(), 2, "one start per payload process: {starts:?}");
    assert!(starts[0].starts_with("tree "), "{starts:?}");
    assert!(starts[1].starts_with("leaf "), "{starts:?}");
    // Declared bound: the group may consume the lowered share of total host CPU
    // capacity over the measured wall clock (rate * logical processors * elapsed),
    // with 25% slack for the kernel rate cycle, timer granularity and sampling.
    // Demand is the two single-threaded spinners' unthrottled CPU seconds, which
    // must stay far above the allowance for this to prove anything at all.
    let snapshot = budget.snapshot().unwrap();
    let measured = snapshot
        .cpu_time
        .saturating_sub(before.cpu_time)
        .as_secs_f64();
    let allowance = TEST_RATE / 100.0 * f64::from(cpus) * elapsed.as_secs_f64();
    let demand = 2.0 * threads as f64 * (spin_ms as f64 / 1000.0);
    eprintln!(
        "opencode cpu budget evidence: rate={TEST_RATE}% cpus={cpus} threads_per_process={threads} elapsed={:.2}s measured_cpu={measured:.3}s allowance={allowance:.3}s demand={demand:.3}s",
        elapsed.as_secs_f64()
    );
    assert!(
        demand > allowance * 2.0,
        "workload demand {demand}s must exceed the allowance {allowance}s"
    );
    assert!(
        measured <= allowance * 1.25 + 0.05,
        "measured aggregate {measured}s exceeded the lowered ceiling: allowance {allowance}s over {elapsed:?}"
    );
    assert!(
        measured < demand * 0.5,
        "measured aggregate {measured}s shows no throttling against demand {demand}s"
    );
    assert_eq!(snapshot.cpu_rate, cpu_rate_units(TEST_RATE));
    assert!(snapshot.cpu_hard_cap);
    assert!(
        wait_for_empty(&budget),
        "the isolated account still holds members after the session exited"
    );
}

#[test]
fn installed_opencode_launch_forwards_native_inputs_without_codex_arguments_or_environment() {
    let f = Fixture::new();
    let _budget = SharedCpuBudget::acquire(&f.account, SHARED_CPU_PERCENT).unwrap();
    f.record_upstream(&forwarding_upstream(&f));
    let args = [
        "--harness-effort",
        "routine",
        "exec",
        "",
        "проверка \"кавычки\"",
        "trailing\\",
        "$() `literal` ; &",
    ];
    let mut child = f
        .command()
        .args(args)
        // The ambient environment of a test host may already carry Codex
        // package-manager hints; an OpenCode launch must neither add nor rewrite
        // them, so this run removes the optional ones entirely.
        .env_remove("CODEX_MANAGED_PACKAGE_ROOT")
        .env_remove("CODEX_MANAGED_BY_NPM")
        .env_remove("CODEX_MANAGED_BY_BUN")
        .env_remove("HARNESS_LSP_WORKSPACE_ROOTS")
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("первая строка\nsecond line\n".as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(19));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.trim(), "upstream stderr", "{stderr}");
    assert!(!stderr.contains(REGISTRATION_NOTICE), "{stderr}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    // Arguments reach OpenCode unchanged: no Codex profile, effort, model or
    // workspace-root policy is translated or injected.
    assert_eq!(report["args"], json!(args), "{report}");
    assert_eq!(report["stdin"], "первая строка\nsecond line\n");
    assert_eq!(
        Path::new(report["cwd"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        f.root.path().canonicalize().unwrap()
    );
    for name in [
        "CODEX_MANAGED_PACKAGE_ROOT",
        "CODEX_MANAGED_BY_NPM",
        "CODEX_MANAGED_BY_BUN",
        "CODEX_MANAGED_BY_PNPM",
        "CODEX_MANAGED_BY_VITE_PLUS",
        "HARNESS_LSP_WORKSPACE_ROOTS",
    ] {
        assert!(
            report["environment"][name].is_null(),
            "{name} must not be injected into an OpenCode launch: {report}"
        );
    }
    // An inherited Codex variable is forwarded unchanged rather than rewritten
    // or removed: OpenCode keeps its own environment.
    let output = f
        .command()
        .args(args)
        .env("CODEX_MANAGED_PACKAGE_ROOT", "inherited-root")
        .env("CODEX_MANAGED_BY_NPM", "1")
        .env("CODEX_MANAGED_BY_BUN", "inherited-bun")
        .env("HARNESS_LSP_WORKSPACE_ROOTS", "[\"inherited\"]")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["args"], json!(args), "{report}");
    assert_eq!(
        report["environment"]["CODEX_MANAGED_PACKAGE_ROOT"],
        "inherited-root"
    );
    assert_eq!(report["environment"]["CODEX_MANAGED_BY_NPM"], "1");
    assert_eq!(
        report["environment"]["CODEX_MANAGED_BY_BUN"],
        "inherited-bun"
    );
    assert_eq!(
        report["environment"]["HARNESS_LSP_WORKSPACE_ROOTS"],
        "[\"inherited\"]"
    );
}

#[test]
fn opencode_and_codex_sessions_share_one_budget_and_opencode_stays_capped_when_the_peer_exits() {
    let f = Fixture::new();
    let cpus = host_logical_processors();
    let threads = spinner_threads(cpus, TEST_RATE);
    let budget = SharedCpuBudget::acquire(&f.account, TEST_RATE).unwrap();
    let name = budget.name().to_owned();
    f.record_upstream(consumer());
    f.register_codex_consumer(consumer());
    // The OpenCode session is the longer participant.
    let opencode = Consumer::new(f.root.path(), "opencode");
    let mut opencode_command = f.command();
    opencode_command.env(CPU_PERCENT_ENV, TEST_RATE.to_string());
    opencode.configure(&mut opencode_command, &name, threads, 8000, true, 0);
    let mut opencode_child = opencode_command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(
        &opencode.directory.join("tree.json"),
        Duration::from_secs(30),
    );
    wait_for_path(
        &opencode.directory.join("leaf.json"),
        Duration::from_secs(30),
    );
    assert_eq!(
        opencode.tree()["in_shared"],
        json!(true),
        "{}",
        opencode.tree()
    );
    assert_eq!(
        opencode.leaf()["in_shared"],
        json!(true),
        "{}",
        opencode.leaf()
    );
    // A Codex session started independently joins that same object.
    let peer = Consumer::new(f.root.path(), "codex-peer");
    let mut peer_command = f.codex_command();
    peer_command.env(CPU_PERCENT_ENV, TEST_RATE.to_string());
    peer.configure(&mut peer_command, &name, threads, 3000, false, 0);
    let peer_child = peer_command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&peer.directory.join("tree.json"), Duration::from_secs(30));
    assert_eq!(peer.tree()["in_shared"], json!(true), "{}", peer.tree());
    assert_eq!(
        budget.snapshot().unwrap().cpu_rate,
        cpu_rate_units(TEST_RATE),
        "both agents must join one account allowance"
    );
    assert!(
        wait_for_members(&budget, 2, Duration::from_secs(10)),
        "the two agent sessions are not simultaneous members of the account group"
    );
    // An unrelated direct process of the same account stays outside the group.
    let outside = Consumer::new(f.root.path(), "outside");
    let mut outside_command = Command::new(consumer());
    outside.configure(&mut outside_command, &name, 1, 0, false, 0);
    let outside_output = outside_command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(outside_output.status.success());
    assert_eq!(
        outside.tree()["in_shared"],
        json!(false),
        "{}",
        outside.tree()
    );
    // The Codex peer exits first. The OpenCode session keeps its membership and
    // its own aggregate work stays inside the shared ceiling.
    let peer_output = peer_child.wait_with_output().unwrap();
    assert_eq!(
        peer_output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&peer_output.stderr)
    );
    assert!(
        opencode_child.try_wait().unwrap().is_none(),
        "the OpenCode session ended with its peer"
    );
    let window = Duration::from_millis(2500);
    let before = budget.snapshot().unwrap();
    let started = Instant::now();
    std::thread::sleep(window);
    let after = budget.snapshot().unwrap();
    let elapsed = started.elapsed();
    let measured = after.cpu_time.saturating_sub(before.cpu_time).as_secs_f64();
    let allowance = TEST_RATE / 100.0 * f64::from(cpus) * elapsed.as_secs_f64();
    let demand = threads as f64 * (window.as_secs_f64());
    eprintln!(
        "shared budget evidence after the Codex peer exited: rate={TEST_RATE}% cpus={cpus} threads={threads} elapsed={:.2}s measured_cpu={measured:.3}s allowance={allowance:.3}s demand={demand:.3}s",
        elapsed.as_secs_f64()
    );
    assert!(
        demand > allowance * 2.0,
        "workload demand {demand}s must exceed the allowance {allowance}s"
    );
    assert!(
        measured <= allowance * 1.25 + 0.05,
        "the surviving OpenCode session exceeded the shared ceiling after its peer exited: {measured}s over {elapsed:?}"
    );
    assert!(
        wait_for_members(&budget, 1, Duration::from_secs(5)),
        "the surviving session left the account group"
    );
    let output = opencode_child.wait_with_output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(!stderr.contains(CAP_WARNING), "{stderr}");
    assert!(
        wait_for_empty(&budget),
        "the account group still holds members after both sessions exited"
    );
}

/// One ordinary OpenCode launch of the registered forwarding fixture.
fn launch_session(f: &Fixture, args: &[&str], failure: Option<&str>) -> (i32, Value, String) {
    let mut command = f.command();
    match failure {
        None => {}
        Some("ceiling") => {
            command.env(CPU_PERCENT_ENV, "eighty");
        }
        Some("storage") => {
            command.env(CPU_ACCOUNT_ENV, RELATIVE_ACCOUNT);
        }
        Some("account") => {}
        Some(other) => panic!("unknown failure mode {other}"),
    }
    let mut child = command
        .args(args)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("первая строка\nsecond line\n".as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let report = serde_json::from_str(&stdout).unwrap_or_else(|error| {
        panic!("stdout is not the payload's own report ({error}): {stdout:?}")
    });
    (
        output.status.code().unwrap_or(-1),
        report,
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[test]
fn cpu_admission_failure_warns_once_and_preserves_native_inputs_and_exit() {
    let args = [
        "exec",
        "",
        "проверка \"кавычки\"",
        "trailing\\",
        "$() `literal` ; &",
    ];
    for mode in ["ceiling", "storage", "account"] {
        let f = Fixture::new();
        f.record_upstream(&forwarding_upstream(&f));
        let (code, admitted, stderr) = launch_session(&f, &args, None);
        assert_eq!(code, 19, "{mode}: {stderr}");
        assert!(
            !stderr.contains(CAP_WARNING),
            "{mode}: the admitted reference launch warned: {stderr}"
        );
        match mode {
            "ceiling" => {}
            "storage" => {}
            "account" => {
                fs::remove_dir_all(&f.account).unwrap();
                fs::write(&f.account, b"not a directory").unwrap();
            }
            _ => unreachable!(),
        }
        let account_before = account_state(&f.account);
        let (code, failed_open, stderr) = launch_session(&f, &args, Some(mode));
        assert_eq!(code, 19, "{mode}: {stderr}");
        assert_eq!(
            failed_open, admitted,
            "{mode}: the degraded launch changed the payload's arguments, cwd, streams or environment"
        );
        // One visible warning on the diagnostic channel, naming the requested
        // ceiling, the failed stage, the cause, the scope and the recovery.
        assert_eq!(
            stderr.matches(CAP_WARNING).count(),
            1,
            "{mode}: exactly one fallback notice: {stderr}"
        );
        let requested = match mode {
            "ceiling" => "requested ceiling eighty% of host CPU",
            "storage" | "account" => "requested ceiling 75% of host CPU",
            _ => unreachable!(),
        };
        assert!(stderr.contains(requested), "{mode}: {stderr}");
        let stage = match mode {
            "ceiling" => "failed stage: ceiling configuration",
            "storage" => "failed stage: account storage",
            "account" => "failed stage: budget admission",
            _ => unreachable!(),
        };
        for marker in [stage, "cause: ", "scope: ", "recovery: ", "upstream stderr"] {
            assert!(
                stderr.contains(marker),
                "{mode}: missing {marker:?}: {stderr}"
            );
        }
        // The degraded OpenCode launch stays on its own fail-open contract: no
        // Codex override notice, no registration fallback, no persistence.
        assert!(
            !stderr.contains("without harness overrides"),
            "{mode}: an OpenCode launch must not take the Codex override path: {stderr}"
        );
        assert!(
            !stderr.contains(REGISTRATION_NOTICE),
            "{mode}: the registration record is intact here: {stderr}"
        );
        assert_eq!(
            account_state(&f.account),
            account_before,
            "{mode}: the degraded launch changed machine-local budget state"
        );
        assert!(
            !f.root.path().join(RELATIVE_ACCOUNT).exists(),
            "{mode}: a relative account location was created"
        );
    }
}

#[test]
fn unusable_registration_warns_and_resolves_the_path_opencode_without_recursion() {
    let f = Fixture::new();
    let upstream = forwarding_upstream(&f);
    // A decoy copy of the kit's own launcher holds the first search position.
    // Resolution must skip it instead of re-running itself or another kit command.
    let commands = f.home.join("harness/bin");
    fs::create_dir_all(&commands).unwrap();
    fs::copy(&f.launcher, commands.join("opencode.exe")).unwrap();
    let search =
        env::join_paths([commands.clone(), upstream.parent().unwrap().to_owned()]).unwrap();
    let started = f.root.path().join("started.txt");
    let args = ["exec", "still shared"];

    // (a) No record at all: one visible notice, then the installed OpenCode.
    let output = f
        .command()
        .env("PATH", &search)
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &started)
        .args(args)
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert_eq!(
        stderr.matches(REGISTRATION_NOTICE).count(),
        1,
        "exactly one registration notice: {stderr}"
    );
    for marker in [
        "the record is absent",
        "launching the PATH-resolved OpenCode executable",
        "scope: ",
        "recovery: ",
    ] {
        assert!(stderr.contains(marker), "missing {marker:?}: {stderr}");
    }
    assert!(
        !stderr.contains(CAP_WARNING),
        "the shared cap is attempted independently of the record: {stderr}"
    );
    assert!(started.is_file(), "the installed OpenCode never started");
    assert_eq!(
        stdout.lines().count(),
        1,
        "the payload ran exactly once: {stdout}"
    );
    let report: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["args"], json!(args), "{report}");
    assert_eq!(report["stdin"], "");

    // (b) A foreign record is preserved and treated as unusable.
    fs::write(&f.record, b"foreign record bytes").unwrap();
    fs::remove_file(&started).unwrap();
    let output = f
        .command()
        .env("PATH", &search)
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &started)
        .args(args)
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert_eq!(stderr.matches(REGISTRATION_NOTICE).count(), 1, "{stderr}");
    assert!(
        stderr.contains("the record is not a schema 1 OpenCode registration"),
        "{stderr}"
    );
    assert!(started.is_file(), "{}", started.display());
    assert_eq!(
        fs::read(&f.record).unwrap(),
        b"foreign record bytes",
        "a foreign record must be preserved"
    );

    // (c) A pinned executable that no longer resolves is an ordinary launch
    // error: no substitute program is started.
    f.record_upstream(&upstream);
    fs::remove_file(&upstream).unwrap();
    fs::remove_file(&started).unwrap();
    let output = f
        .command()
        .env("PATH", &search)
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &started)
        .args(args)
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(output.stdout.is_empty(), "{stderr}");
    assert!(stderr.contains("explicit update required"), "{stderr}");
    assert!(
        !started.exists(),
        "a missing registered executable must not be substituted"
    );
}

#[test]
fn registration_identity_rules_refuse_harness_launchers_and_prove_package_identity() {
    let root = tempfile::Builder::new()
        .prefix("opencode-resolution-")
        .tempdir()
        .unwrap();
    let harness_bin = root.path().join("harness/bin");
    let copies = root.path().join("copies");
    let tools = root.path().join("tools");
    let prefix = root.path().join("npm-prefix");
    let other_prefix = root.path().join("npm-other");
    let bun = root.path().join(".bun");
    for path in [
        harness_bin.clone(),
        copies.clone(),
        tools.clone(),
        prefix.join("node_modules/opencode-ai/bin"),
        other_prefix.join("node_modules/opencode-ai/bin"),
        bun.join("bin"),
        bun.join("install/global/node_modules/opencode-ai/bin"),
    ] {
        fs::create_dir_all(path).unwrap();
    }
    let launcher = env!("CARGO_BIN_EXE_opencode");
    fs::copy(launcher, harness_bin.join("opencode.exe")).unwrap();
    // Byte-identical content elsewhere must be refused by digest, not only path.
    fs::copy(launcher, copies.join("opencode.exe")).unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_harness-launch-fixture"),
        tools.join("opencode.exe"),
    )
    .unwrap();
    let excluded = vec![harness_bin.clone(), harness_bin.join("opencode.exe")];
    // Only harness-owned entries on the search path: nothing is adopted.
    assert!(
        native_launcher::opencode_resolve(None, Some(harness_bin.as_os_str()), &excluded)
            .unwrap()
            .is_none()
    );
    assert!(
        native_launcher::opencode_resolve(None, Some(copies.as_os_str()), &excluded)
            .unwrap()
            .is_none(),
        "a copy of the harness launcher is refused by content"
    );
    // A plain installed native OpenCode is adopted.
    let adopted =
        native_launcher::opencode_resolve(Some(&tools.join("opencode.exe")), None, &excluded)
            .unwrap()
            .expect("explicit selection adopts an installed native OpenCode");
    assert_eq!(
        adopted.executable,
        tools.join("opencode.exe").canonicalize().unwrap()
    );
    assert_eq!(
        adopted.sha256,
        build_identity::hash_file(&adopted.executable).unwrap()
    );
    // An explicit harness entry point is refused on the same rules.
    assert!(
        native_launcher::opencode_resolve(Some(&harness_bin.join("opencode.exe")), None, &excluded)
            .unwrap()
            .is_none()
    );
    // A command shim is adopted only through a verified package layout, and the
    // verified package payload is preferred over the shim itself.
    fs::write(prefix.join("opencode.cmd"), "shim is never executed").unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_harness-launch-fixture"),
        prefix.join("node_modules/opencode-ai/bin/opencode.exe"),
    )
    .unwrap();
    fs::write(
        prefix.join("node_modules/opencode-ai/package.json"),
        r#"{"name":"opencode-ai","version":"1.18.29","bin":{"opencode":"./bin/opencode.exe"}}"#,
    )
    .unwrap();
    let packaged = native_launcher::opencode_resolve(None, Some(prefix.as_os_str()), &excluded)
        .unwrap()
        .expect("a verified package layout is adopted");
    assert_eq!(
        packaged.executable,
        prefix
            .join("node_modules/opencode-ai/bin/opencode.exe")
            .canonicalize()
            .unwrap()
    );
    // The same layout with a different package identity is not adopted.
    fs::write(other_prefix.join("opencode.cmd"), "shim is never executed").unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_harness-launch-fixture"),
        other_prefix.join("node_modules/opencode-ai/bin/opencode.exe"),
    )
    .unwrap();
    fs::write(
        other_prefix.join("node_modules/opencode-ai/package.json"),
        r#"{"name":"not-opencode","version":"1.18.29","bin":{"opencode":"./bin/opencode.exe"}}"#,
    )
    .unwrap();
    assert!(
        native_launcher::opencode_resolve(None, Some(other_prefix.as_os_str()), &excluded)
            .unwrap()
            .is_none(),
        "an unrelated package identity is never adopted"
    );
    // The bun global layout observed on this host: the launcher under
    // `<home>/.bun/bin` resolves to the verified global package payload.
    fs::copy(
        env!("CARGO_BIN_EXE_harness-launch-fixture"),
        bun.join("bin/opencode.exe"),
    )
    .unwrap();
    let bun_payload = bun.join("install/global/node_modules/opencode-ai/bin/opencode.exe");
    fs::copy(env!("CARGO_BIN_EXE_codex"), &bun_payload).unwrap();
    fs::write(
        bun.join("install/global/node_modules/opencode-ai/package.json"),
        r#"{"name":"opencode-ai","version":"1.18.29","bin":{"opencode":"./bin/opencode.exe"}}"#,
    )
    .unwrap();
    let adopted_bun =
        native_launcher::opencode_resolve(None, Some(bun.join("bin").as_os_str()), &excluded)
            .unwrap()
            .expect("the bun launcher resolves to its verified package payload");
    assert_eq!(adopted_bun.executable, bun_payload.canonicalize().unwrap());

    // The launcher itself refuses a record that names a harness command, by
    // path and by content.
    let f = Fixture::new();
    for pinned in [f.launcher.clone(), copies.join("opencode.exe")] {
        f.record_upstream(&pinned);
        let output = f.command().arg("--version").output().unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.code(), Some(1), "{stderr}");
        assert!(output.stdout.is_empty(), "{stderr}");
        assert!(
            stderr.contains("points at a harness launcher; explicit repair required"),
            "{stderr}"
        );
    }
}

#[test]
fn opencode_console_interaction_and_ctrl_c_return_the_payload_exit_code() {
    let f = Fixture::new();
    f.record_upstream(&forwarding_upstream(&f));
    let budget = SharedCpuBudget::acquire(&f.account, SHARED_CPU_PERCENT).unwrap();
    assert_eq!(
        budget.snapshot().unwrap().cpu_rate,
        cpu_rate_units(SHARED_CPU_PERCENT)
    );
    let session = f.console("interactive");
    wait_for(&session, "upstream prompt console=true");
    // The interactive route either admits the payload or reports the shared cap
    // as unverified; what must never happen is a silent skip.
    let transcript = session.transcript();
    assert!(
        wait_for_members(&budget, 1, Duration::from_secs(3)) || transcript.contains(CAP_WARNING),
        "the interactive session neither joined the account group nor reported the shared cap as unverified: {transcript}"
    );
    session.send("console input\r\n").unwrap();
    wait_for(&session, "upstream echo:console input");
    let result = session
        .wait(
            Deadline::after(Duration::from_secs(5)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0);
    let session = f.console("ctrl-c");
    wait_for(&session, "upstream ready");
    session.send("\u{3}").unwrap();
    let result = session
        .wait(
            Deadline::after(Duration::from_secs(5)).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    assert_eq!(result.outcome.exit_code, 0xc000013a);
}

#[test]
fn opencode_registration_is_pinned_refreshed_and_idempotently_reinstalled() {
    let f = Fixture::new();
    let installed = f.root.path().join("installed");
    fs::create_dir_all(&installed).unwrap();
    let upstream = installed.join("opencode.exe");
    fs::copy(env!("CARGO_BIN_EXE_harness-launch-fixture"), &upstream).unwrap();
    let installed_canonical = upstream.canonicalize().unwrap();
    let search = OsString::from(installed.as_os_str());
    let configure = |preview: bool| {
        core_install::configure_opencode(&f.home, Some(&f.build), Some(&search), preview).unwrap()
    };
    // Preview reports the planned publication and writes nothing.
    let preview = configure(true);
    assert_eq!(preview.action, "created", "{preview:?}");
    assert!(preview.registered);
    assert_eq!(
        preview.executable.as_deref(),
        Some(installed_canonical.as_path())
    );
    assert_eq!(preview.record, f.record);
    assert!(!f.record.exists(), "preview must not publish");

    // Install publishes the pinned record the launcher consumes.
    let created = configure(false);
    assert_eq!(created.action, "created", "{created:?}");
    assert!(created.registered);
    let pinned = pinned_record(&f.record);
    assert_eq!(pinned.executable, installed_canonical);
    assert_eq!(pinned.sha256, build_identity::hash_file(&upstream).unwrap());
    let output = f
        .command()
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(19));

    // Reinstalling an unchanged installation is idempotent: no staged object,
    // no rewrite, the same bytes.
    let before = fs::read(&f.record).unwrap();
    let again = configure(false);
    assert_eq!(again.action, "unchanged", "{again:?}");
    assert_eq!(fs::read(&f.record).unwrap(), before);
    let staged: Vec<_> = fs::read_dir(f.home.join("harness"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "opencode-launch.json")
        .collect();
    assert!(
        staged.is_empty(),
        "an idempotent reinstall leaves no staging artifact: {staged:?}"
    );

    // An updated installed OpenCode refreshes the pinned digest instead of
    // failing the launch: OpenCode updates stay independent of this kit.
    let original_digest = pinned.sha256.clone();
    fs::write(&upstream, b"updated opencode payload").unwrap();
    let refreshed = configure(false);
    assert_eq!(refreshed.action, "updated", "{refreshed:?}");
    let pinned = pinned_record(&f.record);
    assert_eq!(pinned.sha256, build_identity::hash_file(&upstream).unwrap());
    assert_ne!(pinned.sha256, original_digest);
    let updated = fs::read(&f.record).unwrap();

    // Removing the record is recoverable: the launcher still works with a
    // warning, and the next install pins it again.
    fs::remove_file(&f.record).unwrap();
    let recovered = configure(false);
    assert_eq!(recovered.action, "created", "{recovered:?}");
    assert_eq!(fs::read(&f.record).unwrap(), updated);

    // A foreign record belongs to someone else and is neither replaced nor
    // reported as registered.
    fs::write(&f.record, b"foreign record bytes").unwrap();
    let foreign = configure(false);
    assert_eq!(foreign.action, "foreign", "{foreign:?}");
    assert!(!foreign.registered);
    assert_eq!(fs::read(&f.record).unwrap(), b"foreign record bytes");

    // A machine without an installed OpenCode keeps working: no record is
    // created and the Codex installation is not affected.
    fs::remove_file(&f.record).unwrap();
    let empty = f.root.path().join("empty-path");
    let absent =
        core_install::configure_opencode(&f.home, Some(&f.build), Some(empty.as_os_str()), false)
            .unwrap();
    assert_eq!(absent.action, "absent", "{absent:?}");
    assert!(!absent.registered);
    assert!(!f.record.exists());
}

/// Host interface check for the OpenCode installation of this machine: the
/// production resolution rules must adopt the installed payload through its
/// verified package layout, the install must pin it in an owned temporary home,
/// and the kit launcher must forward a non-interactive invocation inside a
/// temporary account allowance. Opt-in: it uses the real installed agent and a
/// temporary home and account, and issues only `--version` (no model call).
#[test]
#[ignore = "explicit installed OpenCode on this host; temporary home and account"]
fn installed_host_opencode_is_resolved_pinned_and_launched_by_the_kit_entry_point() {
    let root = tempfile::Builder::new()
        .prefix("opencode-host-")
        .tempdir()
        .unwrap();
    let home = root.path().join("home");
    let account = root.path().join("account");
    let build = root.path().join("build");
    fs::create_dir_all(home.join("harness")).unwrap();
    fs::create_dir_all(&build).unwrap();
    let launcher = build.join("opencode.exe");
    fs::copy(env!("CARGO_BIN_EXE_opencode"), &launcher).unwrap();
    let resolved = native_launcher::opencode_resolve(
        None,
        env::var_os("PATH").as_deref(),
        std::slice::from_ref(&build),
    )
    .unwrap()
    .expect("the installed OpenCode is adopted through its verified package layout");
    assert!(
        resolved.executable.is_file(),
        "{}",
        resolved.executable.display()
    );
    assert_eq!(
        resolved.sha256,
        build_identity::hash_file(&resolved.executable).unwrap()
    );
    let published = core_install::configure_opencode(
        &home,
        Some(&build),
        env::var_os("PATH").as_deref(),
        false,
    )
    .unwrap();
    assert!(published.registered, "{published:?}");
    assert_eq!(
        published.executable.as_deref(),
        Some(resolved.executable.as_path())
    );
    assert_eq!(
        pinned_record(&native_launcher::opencode_record(&home)).executable,
        resolved.executable
    );
    let output = Command::new(&launcher)
        .arg("--version")
        .env("CODEX_HOME", &home)
        .env(CPU_ACCOUNT_ENV, &account)
        .env_remove(CPU_PERCENT_ENV)
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    assert!(
        stdout.trim().split('.').count() >= 2,
        "the installed OpenCode reports a version: {stdout:?}"
    );
    assert!(!stderr.contains(REGISTRATION_NOTICE), "{stderr}");
    assert!(!stderr.contains(CAP_WARNING), "{stderr}");
    eprintln!(
        "installed OpenCode interface: pinned={} version={}",
        resolved.executable.display(),
        stdout.trim()
    );
}
