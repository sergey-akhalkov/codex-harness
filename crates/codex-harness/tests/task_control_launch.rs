//! Ordinary native entry point with an automatically owned controller/server.
//! Explicit native CLI input, synthetic provider; no subscription requests.
#![cfg(windows)]
#[path = "fixtures/control_responses.rs"]
mod control_responses;
use control_responses::tool_catalogue;
use harness_core::{
    broker_state::BrokerRoot,
    build_identity::{self, BuildRecord},
    console::{ConsoleSession, ConsoleSpec},
    process::{
        Cancellation, CommandSpec, Deadline, SHARED_CPU_PERCENT, SharedCpuBudget, StopReason,
    },
    process_service::{self, ServiceProcess},
};
use serde_json::{Value, json};
use std::{
    fs,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
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

const WAIT: Duration = Duration::from_secs(30);

/// Managed-session role composition markers. The fixture kit supplies the
/// owned developer text plus distinct lead and executor role bodies, and the
/// canonical provider requests must show the composed lead context, including
/// on the configured successor route, without the executor body.
const CONTROL_COMMON_ROLE_MARKER: &str = "CONTROL_COMMON_ROLE_MARKER owned kit developer guidance";
const CONTROL_LEAD_ROLE_MARKER: &str = "CONTROL_LEAD_ROLE_MARKER lead responsibility";
const CONTROL_EXECUTOR_ROLE_MARKER: &str = "CONTROL_EXECUTOR_ROLE_MARKER executor responsibility";

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        harness_core::task_runtime::request_stop(&self.0).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !self.0.join("closed.json").is_file() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[test]
#[ignore = "requires HARNESS_CONTROL_CODEX_EXE; owned native entry point, no model use"]
fn ordinary_launcher_starts_control_and_delivers_tool_result() {
    native_entry(EntryCase::Normal);
}

#[test]
#[ignore = "requires native CLI and parent/child desktop observations; synthetic Responses only"]
fn ordinary_launcher_opens_executor_before_its_first_request() {
    native_entry(EntryCase::Child);
}

#[test]
#[ignore = "requires native CLI, model catalog and three desktop observations; synthetic Responses only"]
fn ordinary_launcher_opens_two_distinct_executor_conversations() {
    native_entry(EntryCase::TwoChildren);
}

#[test]
#[ignore = "requires explicit native CLI and scoped minimize/restore/final desktop observations; synthetic responses only"]
fn ordinary_launcher_suspends_and_recovers_visible_conversation() {
    native_entry(EntryCase::ViewLoss);
}

#[test]
#[ignore = "requires explicit native CLI and scoped final desktop observation; synthetic quota refusal only"]
fn ordinary_launcher_records_quota_refusal_without_another_gpt_request() {
    native_entry(EntryCase::QuotaRefusal);
}

#[test]
#[ignore = "requires native CLI, explicit model catalog and both native chat observations; synthetic quota handoff"]
fn ordinary_launcher_hands_quota_refusal_to_visible_ds_lead() {
    native_entry(EntryCase::QuotaHandoff);
}

#[test]
#[ignore = "requires native CLI and helper pane observations; synthetic Responses only"]
fn ordinary_launcher_opens_helper_before_its_first_request() {
    native_entry(EntryCase::Helper);
}

#[test]
#[ignore = "requires native CLI and closed-window restore; synthetic Responses only"]
fn ordinary_launcher_restores_closed_executor_conversation() {
    native_entry(EntryCase::ViewClose);
}

#[test]
#[ignore = "requires native CLI; explicit user stop through the ordinary entry point; synthetic Responses only"]
fn ordinary_launcher_stops_on_explicit_user_stop() {
    native_entry(EntryCase::UserStop);
}

#[derive(Clone, Copy)]
enum EntryCase {
    Normal,
    Child,
    TwoChildren,
    ViewLoss,
    QuotaRefusal,
    QuotaHandoff,
    Helper,
    ViewClose,
    UserStop,
}

/// One independently started task-control service, as the ordinary launcher
/// starts it: through the `process_service` helper, which creates the service
/// as a WMI sibling that inherits no Job from this test process.
struct SessionService {
    session: PathBuf,
    service: ServiceProcess,
}

impl SessionService {
    /// Build one session tree: a private session root whose launch record names
    /// an owned native-agent double, and the account CPU budget this session
    /// must share with every other session of this Windows account.
    fn start(session: &Path, account: &Path, name: &str) -> Self {
        let manager = PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"));
        let upstream = PathBuf::from(env!("CARGO_BIN_EXE_harness-launch-fixture"));
        let workspace = session.join(format!("{name}-workspace"));
        fs::create_dir_all(&workspace).unwrap();
        let root = BrokerRoot::prepare().unwrap().keep();
        let launch = root.path().join("launch.json");
        fs::write(
            &launch,
            serde_json::to_vec(&json!({
                "schema": 1,
                "executable": upstream,
                "sha256": build_identity::hash_file(&upstream).unwrap(),
                "cwd": workspace,
                "arguments": [],
                "new_session": false,
            }))
            .unwrap(),
        )
        .unwrap();
        // The session's native server is an owned double that holds instead of
        // serving the control protocol, so this check observes admission
        // without a conversation.
        let mut environment: std::collections::BTreeMap<String, String> =
            std::env::vars().collect();
        environment.insert(
            "CODEX_HARNESS_CPU_ACCOUNT".into(),
            account.display().to_string(),
        );
        environment.insert("HARNESS_LAUNCH_FIXTURE_MODE".into(), "heavy-hold".into());
        environment.insert("HARNESS_HEAVY_FIXTURE_MS".into(), "20000".into());
        environment.insert(
            "HARNESS_LAUNCH_FIXTURE_STARTED".into(),
            session.join("native-server.pid").display().to_string(),
        );
        let service = process_service::spawn(
            &manager,
            root.path(),
            vec![
                "task-control".into(),
                build_identity::hash_file(&launch).unwrap(),
                "{}".into(),
            ],
            environment,
            Deadline::after(Duration::from_secs(60)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
        Self {
            session: session.to_path_buf(),
            service,
        }
    }

    /// The native server of this session, alive and observable by the pid it
    /// recorded for itself.
    fn native_server(&self, timeout: Duration) -> HANDLE {
        observe_payload(&self.session.join("native-server.pid"), timeout)
    }

    /// Ask the kernel whether this exact retained service belongs to the
    /// account allowance named by the test's verified budget handle.
    fn in_budget(&self, budget: &SharedCpuBudget) -> bool {
        self.service.in_shared_cpu_budget(budget).unwrap()
    }
}

impl Drop for SessionService {
    fn drop(&mut self) {
        // The session double keeps holding its native server; ending the
        // service reaps the tree through its own lifecycle Job.
        let _ = self.service.terminate(130);
    }
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

fn open_job(name: &str) -> HANDLE {
    let object: Vec<u16> = std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let handle = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, object.as_ptr()) };
    assert!(!handle.is_null(), "job object {name} is not observable");
    handle
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
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < timeout,
            "{} was not created within {timeout:?}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn observe_payload(marker: &Path, timeout: Duration) -> HANDLE {
    wait_for_path(marker, timeout);
    let pid: u32 = fs::read_to_string(marker)
        .unwrap()
        .trim()
        .parse()
        .expect("native server pid");
    observe_pid(pid, timeout)
}

/// One live process, opened by pid, so the kernel answers for the process that
/// was recorded instead of for a wrapper's claim about it.
fn observe_pid(pid: u32, timeout: Duration) -> HANDLE {
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
            "pid {pid} was not observable while it was running"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Independently started task-control sessions of different session trees share
/// one account CPU allowance: the second service joins the group the first
/// established, each session's native server is a kernel-verified member of
/// that one group at the delivered 75% hard cap, and an unrelated process of
/// the same account stays outside it.
#[test]
fn independently_started_task_control_sessions_share_one_account_cpu_budget() {
    let root = std::env::temp_dir().join(format!("task-control-budget-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let account = root.join("cpu-budget");
    let first = SessionService::start(&root.join("session-one"), &account, "first");
    let first_server = first.native_server(Duration::from_secs(30));
    // The second session starts while the first is already inside the group, so
    // joining an established allowance is what this check exercises.
    let second = SessionService::start(&root.join("session-two"), &account, "second");
    let second_server = second.native_server(Duration::from_secs(30));

    let budget = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
    let job = budget.name().to_owned();
    assert_eq!(
        job_cpu_rate(&job),
        (SHARED_CPU_PERCENT * 100.0) as u32,
        "the shared account group must carry the delivered 75% hard cap"
    );
    assert!(
        first.in_budget(&budget),
        "the first session's service must be inside the account allowance"
    );
    assert!(
        second.in_budget(&budget),
        "a second independently started session must join the established allowance"
    );
    assert!(
        process_in_job(first_server, &job) && process_in_job(second_server, &job),
        "both sessions' native servers must belong to the one account group"
    );
    // An unrelated process of the same account, started by the same terminal
    // that starts sessions, must stay outside the allowance.
    let unrelated_marker = root.join("unrelated.pid");
    let mut unrelated = Command::new(env!("CARGO_BIN_EXE_harness-launch-fixture"))
        .env_remove("CODEX_HARNESS_CPU_ACCOUNT")
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "20000")
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &unrelated_marker)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let unrelated_pid = observe_payload(&unrelated_marker, Duration::from_secs(30));
    assert!(
        !process_in_job(unrelated_pid, &job),
        "an unrelated process of the same account must stay outside the allowance"
    );
    assert!(
        !process_in_job(unsafe { GetCurrentProcess() }, &job),
        "starting a session must not enrol the caller or its terminal"
    );

    unsafe { CloseHandle(first_server) };
    unsafe { CloseHandle(second_server) };
    unsafe { CloseHandle(unrelated_pid) };
    let _ = unrelated.kill();
    let _ = unrelated.wait();
    drop(first);
    drop(second);
    let _ = fs::remove_dir_all(&root);
}

fn native_entry(case: EntryCase) {
    let pair = matches!(case, EntryCase::TwoChildren);
    let helper = matches!(case, EntryCase::Helper);
    let view_close = matches!(case, EntryCase::ViewClose);
    let child = matches!(
        case,
        EntryCase::Child | EntryCase::TwoChildren | EntryCase::Helper | EntryCase::ViewClose
    );
    let view_loss = matches!(case, EntryCase::ViewLoss);
    let user_stop = matches!(case, EntryCase::UserStop);
    let quota_refusal = matches!(case, EntryCase::QuotaRefusal);
    let quota_handoff = matches!(case, EntryCase::QuotaHandoff);
    assert_eq!(
        Path::new(env!("CARGO_BIN_EXE_codex"))
            .parent()
            .unwrap()
            .file_name()
            .unwrap(),
        "release",
        "run this installed-entry test with --release"
    );
    let upstream = PathBuf::from(
        std::env::var_os("HARNESS_CONTROL_CODEX_EXE").expect("explicit native Codex executable"),
    );
    let owned = BrokerRoot::prepare().unwrap().keep();
    let root = owned.path();
    fs::write(
        root.join("require-initial-view"),
        "verify before first provider response",
    )
    .unwrap();
    eprintln!("ordinary control evidence: {}", root.display());
    if pair {
        fs::write(root.join("two-executors"), "owned concurrent assignments").unwrap();
    }
    if helper {
        fs::write(root.join("helper-window"), "owned nested helper").unwrap();
    }
    if view_close {
        fs::write(root.join("close-view"), "owned closed conversation restore").unwrap();
    }
    if matches!(case, EntryCase::Normal) {
        fs::write(
            root.join("catalogue-probe"),
            "the ordinary lead must report its owned runtime catalogue",
        )
        .unwrap();
    }
    let home = root.join("home");
    let workspace = root.join("workspace");
    let build = root.join("build");
    let source = root.join("source");
    for directory in [
        home.join("harness"),
        workspace.clone(),
        build.clone(),
        source.join("global"),
        source.join("global/roles"),
        source.join("skills"),
        source.join("crates/fixture/src"),
        source.join("crates/harness-rtk/src"),
        source
            .join(build_identity::INSPECTION_SCHEMA)
            .parent()
            .unwrap()
            .into(),
    ] {
        fs::create_dir_all(directory).unwrap();
    }
    // The Normal case owns one synthetic unrelated MCP server: the first-party
    // probe fixture copied into the owned home beside its mode config. The
    // launched session must preserve this configuration and discover the
    // server's real tools; no other case configures one.
    let probe_executable = matches!(case, EntryCase::Normal).then(|| {
        let directory = home.join("mcp-probe");
        fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("harness-mcp-probe-fixture.exe");
        fs::copy(env!("CARGO_BIN_EXE_harness-mcp-probe-fixture"), &executable).unwrap();
        fs::write(
            executable.with_extension("json"),
            serde_json::to_vec(&json!({"mode": "valid", "kind": "nuphus"})).unwrap(),
        )
        .unwrap();
        executable
    });
    let probe_receipt = probe_executable
        .as_ref()
        .map(|executable| executable.with_extension("receipt.json"));
    for file in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/fixture/src/lib.rs",
        build_identity::INSPECTION_SCHEMA,
    ] {
        fs::write(source.join(file), "owned fixture source identity").unwrap();
    }
    fs::write(
        source.join("global/profile.toml"),
        format!(
            "approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\nmodel = 'gpt-6-astra'\ndeveloper_instructions = '{CONTROL_COMMON_ROLE_MARKER}'\n"
        ),
    )
    .unwrap();
    fs::write(source.join("global/kit.json"), serde_json::to_vec(&json!({"schema":1,"profile_name":"harness","profile":"global/profile.toml","instructions":"global/instructions.md","skills":"skills","agents":"global/agents","hooks":"global/hooks.json","token_hooks":"global/token-hooks.json"})).unwrap()).unwrap();
    // A managed session composes its role context from the kit sources, so
    // the fixture carries the common instructions, both role bodies and the
    // shared skill root beside the manifest.
    fs::write(
        source.join("global/instructions.md"),
        "Owned fixture common instructions.\n",
    )
    .unwrap();
    fs::write(
        source.join("global/roles.toml"),
        "schema = 1\nshared_skills = []\n[roles.lead]\ninstructions = 'global/roles/lead.md'\nskills = []\n[roles.executor]\ninstructions = 'global/roles/executor.md'\nskills = []\n",
    )
    .unwrap();
    fs::write(
        source.join("global/roles/lead.md"),
        format!(
            "# Lead responsibility\n\n{CONTROL_LEAD_ROLE_MARKER}: preserve the task, its constraints and acceptance.\n"
        ),
    )
    .unwrap();
    fs::write(
        source.join("global/roles/executor.md"),
        format!(
            "# Executor responsibility\n\n{CONTROL_EXECUTOR_ROLE_MARKER}: own the complete local work cycle.\n"
        ),
    )
    .unwrap();
    if quota_handoff {
        fs::write(
            source.join("global/orchestration.toml"),
            "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = 2\n",
        )
        .unwrap();
        fs::write(
            home.join("ds.config.toml"),
            "model = \"deepseek-flash\"\nmodel_provider = \"control_fixture\"\nmodel_reasoning_effort = \"max\"\n",
        )
        .unwrap();
    }
    // Real Cargo outputs; the owned registration only substitutes install paths.
    for (name, binary) in [
        ("codex.exe", env!("CARGO_BIN_EXE_codex")),
        ("codex-harness.exe", env!("CARGO_BIN_EXE_codex-harness")),
        ("harness-inspect.exe", env!("CARGO_BIN_EXE_harness-inspect")),
        ("harness-observe.exe", env!("CARGO_BIN_EXE_harness-observe")),
        (
            "harness-source-check.exe",
            env!("CARGO_BIN_EXE_harness-source-check"),
        ),
    ] {
        fs::copy(binary, build.join(name)).unwrap();
    }
    let rtk = Path::new(env!("CARGO_BIN_EXE_codex")).with_file_name("harness-rtk.exe");
    fs::copy(rtk, build.join("harness-rtk.exe"))
        .expect("build the workspace binaries with --release before this entry-point test");
    let audit = Path::new(env!("CARGO_BIN_EXE_codex")).with_file_name("token-audit.exe");
    fs::copy(audit, build.join("token-audit.exe"))
        .expect("build the workspace binaries with --release before this entry-point test");
    let record = BuildRecord {
        schema: build_identity::SCHEMA,
        source_root: source.clone(),
        source: build_identity::source_identity(&source).unwrap(),
        rustc: "fixture registration of Cargo outputs".into(),
        cargo: "fixture registration of Cargo outputs".into(),
        target: "x86_64-pc-windows-msvc".into(),
        profile: "release".into(),
        binaries: build_identity::BINARIES
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    build_identity::hash_file(&build.join(name)).unwrap(),
                )
            })
            .collect(),
    };
    fs::write(
        build.join("build.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    fs::write(home.join("harness/native-launch.json"), serde_json::to_vec(&json!({"schema":2,"build":build,"task_control":true,"upstream":{"executable":upstream,"sha256":build_identity::hash_file(&upstream).unwrap(),"package":null}})).unwrap()).unwrap();
    let responses = match case {
        EntryCase::ViewLoss | EntryCase::UserStop => {
            control_responses::Responses::with_view_loss(root.into())
        }
        EntryCase::QuotaRefusal => control_responses::Responses::with_quota_refusal(root.into()),
        EntryCase::QuotaHandoff => control_responses::Responses::with_quota_handoff(root.into()),
        EntryCase::Normal => control_responses::Responses::start(root.into(), true),
        EntryCase::Child | EntryCase::TwoChildren | EntryCase::Helper | EntryCase::ViewClose => {
            control_responses::Responses::start(root.into(), false)
        }
    };
    let trusted = serde_json::to_string(&workspace.to_string_lossy()).unwrap();
    let catalog_config = if quota_handoff || pair {
        let source = PathBuf::from(
            std::env::var_os("HARNESS_CONTROL_MODEL_CATALOG")
                .expect("explicit model metadata, without credentials"),
        );
        let catalog = home.join("catalog.json");
        fs::write(&catalog, pair_model_catalog(&source)).unwrap();
        format!(
            "model_catalog_json = {}",
            serde_json::to_string(&catalog.to_string_lossy()).unwrap()
        )
    } else {
        String::new()
    };
    let agents_config = if helper || pair {
        "[agents]\nmax_depth = 2\n"
    } else {
        ""
    };
    let multi_agent_v2 = if pair { "true" } else { "false" };
    // Only the Normal case carries the synthetic unrelated MCP server: the
    // absolute fixture path reaches the session through the exercised
    // launcher's own configuration handling.
    let unrelated_mcp = probe_executable
        .as_ref()
        .map(|executable| {
            format!(
                "[mcp_servers.unrelated_probe]\ncommand = {}\n",
                serde_json::to_string(&executable.to_string_lossy()).unwrap()
            )
        })
        .unwrap_or_default();
    fs::write(
        home.join("config.toml"),
        format!(
            r#"
model = "gpt-6-astra"
model_reasoning_effort = "low"
model_provider = "control_fixture"
{catalog_config}
approval_policy = "never"
sandbox_mode = "danger-full-access"
cli_auth_credentials_store = "file"
[model_providers.control_fixture]
name = "Owned native controller fixture"
base_url = "http://127.0.0.1:{}/v1"
wire_api = "responses"
env_key = "HARNESS_CONTROL_FIXTURE_KEY"
requires_openai_auth = false
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false
[features]
code_mode = false
shell_snapshot = false
hooks = false
multi_agent = true
multi_agent_v2 = {multi_agent_v2}
{agents_config}
[analytics]
enabled = false
[projects.{trusted}]
trust_level = "trusted"
{unrelated_mcp}"#,
            responses.port
        ),
    )
    .unwrap();
    let mut command = CommandSpec::new(build.join("codex.exe"));
    command.current_dir = Some(workspace.clone());
    command.env.insert("CODEX_HOME".into(), Some(home.into()));
    command.env.insert(
        "HARNESS_CONTROL_FIXTURE_KEY".into(),
        Some("synthetic-owned-fixture".into()),
    );
    // This acceptance may itself run as a ds executor session; its inherited
    // executor/session/role markers must not reach the ordinary launch under
    // observation. Only the owned child's environment is adjusted.
    for key in [
        "OPENAI_API_KEY",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_CONTEXT_ROLE",
        "HARNESS_CONTEXT_REVISION",
    ] {
        command.env.insert(key.into(), None);
    }
    let path = std::env::join_paths(
        std::env::split_paths(&std::env::var_os("PATH").unwrap()).filter(|path| {
            !path
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("windowsapps")
        }),
    )
    .unwrap();
    command.env.insert("PATH".into(), Some(path));
    command.args = vec![
        "--no-alt-screen".into(),
        if pair {
            "Delegate two independent owned proof tasks to the two configured ds executors.".into()
        } else if child {
            "Delegate the owned proof to one native child.".into()
        } else {
            "Perform the owned proof command and return its consumed result.".into()
        },
    ];
    let mut console = ConsoleSpec::new(command);
    console.size.columns = 180;
    let session = ConsoleSession::spawn(console).unwrap();
    let escapes = regex::Regex::new(r"\x1b\][^\x07]*(?:\x07)|\x1b\[[0-?]*[ -/]*[@-~]").unwrap();
    let until = Instant::now() + WAIT;
    let state = loop {
        let transcript = session.transcript().replace("\x1b[1C", " ");
        let plain = escapes.replace_all(&transcript, "");
        let state = plain
            .split("codex-harness: task control state:")
            .nth(1)
            .and_then(|tail| tail.split_once('\n'))
            .map(|(path, _)| PathBuf::from(path.trim()))
            .filter(|path| path.is_absolute() && path.join("launch.json").is_file());
        if state.is_some() || Instant::now() >= until {
            break state;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    fs::write(root.join("terminal.txt"), session.transcript()).unwrap();
    let _cleanup = state.clone().map(Cleanup);
    let state = state.expect("controller state location must be visible");
    fs::write(
        root.join("controller-state.json"),
        serde_json::to_vec(&json!({"root":state})).unwrap(),
    )
    .unwrap();
    observe_conversation_visibility(&state, &upstream);
    if view_loss {
        exercise_view_loss(root, &state, &workspace, &upstream);
    }
    if user_stop {
        exercise_user_stop(root, &state, &workspace);
    }
    if child {
        pause_synthetic_parent_goal(&state);
    }
    if view_close {
        exercise_view_close(root, &state, &workspace, &upstream);
    }
    let marker = if pair {
        control_responses::PARENT_FINAL.to_owned()
    } else if quota_refusal {
        quota_refusal_result(root, &state, &workspace)
    } else if user_stop {
        "explicit stop".to_owned()
    } else {
        control_responses::FINAL.to_owned()
    };
    let until = Instant::now()
        + if helper || view_close || pair {
            Duration::from_secs(90)
        } else {
            WAIT
        };
    while Instant::now() < until {
        if quota_handoff && let Ok(bytes) = fs::read(state.join("handoff.json")) {
            let transfer: Value = serde_json::from_slice(&bytes).unwrap();
            assert_ne!(
                transfer["transfer"]["phase"], "blocked",
                "handoff blocked: {}",
                transfer["transfer"]["error"]
            );
        }
        assert!(
            !state.join("client-closed.json").is_file(),
            "native conversation closed before its result; inspect client-closed.json"
        );
        if user_stop && state.join("closed.json").is_file() {
            break;
        }
        if !user_stop
            && state.join("view.json").is_file()
            && fs::read_to_string(state.join("task.json")).is_ok_and(|saved| {
                saved.contains(&marker)
                    && (!pair
                        || saved.contains("CONTROL_TOOL_RESULT_CONSUMED_ONE")
                            && saved.contains("CONTROL_TOOL_RESULT_CONSUMED_TWO"))
            })
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if user_stop {
        let closed: Value =
            serde_json::from_slice(&fs::read(state.join("closed.json")).unwrap()).unwrap();
        assert_eq!(closed["reason"], "explicit stop");
        assert_eq!(closed["stopped"], true);
        assert!(!root.join("provider-2.json").exists());
    } else {
        assert!(
            fs::read_to_string(state.join("task.json"))
                .unwrap()
                .contains(&marker),
            "controller must consume the result in the visible native session"
        );
    }
    if !quota_refusal {
        assert_eq!(
            fs::read_to_string(workspace.join("proof.txt")).unwrap(),
            "one"
        );
    }
    if pair {
        let saved = fs::read_to_string(state.join("task.json")).unwrap();
        assert!(
            saved.contains("CONTROL_TOOL_RESULT_CONSUMED_ONE")
                && saved.contains("CONTROL_TOOL_RESULT_CONSUMED_TWO")
        );
        assert_eq!(
            fs::read_to_string(workspace.join("proof-two.txt")).unwrap(),
            "one"
        );
    }
    let initial_view: Value =
        serde_json::from_slice(&fs::read(state.join("view.json")).unwrap()).unwrap();
    if pair {
        let saved: Value =
            serde_json::from_slice(&fs::read(state.join("task.json")).unwrap()).unwrap();
        assert!(
            saved["threads"][initial_view["threadId"].as_str().unwrap()]["turns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|turn| turn["items"].as_array().is_some_and(|items| items
                    .iter()
                    .any(|item| item["type"] == "agentMessage"
                        && item["text"] == control_responses::PARENT_FINAL))),
            "the leader's own native final must consume both children"
        );
    }
    let view = if child {
        let views: Value =
            serde_json::from_slice(&fs::read(state.join("additional-views.json")).unwrap())
                .unwrap();
        let children = views["threads"].as_object().unwrap();
        assert_eq!(children.len(), if pair || helper { 2 } else { 1 });
        if helper {
            let named: Value =
                serde_json::from_slice(&fs::read(state.join("child-view-requests.json")).unwrap())
                    .unwrap();
            let titles: Vec<_> = named
                .as_object()
                .unwrap()
                .values()
                .map(|value| value["title"].as_str().unwrap().to_owned())
                .collect();
            assert!(titles.iter().any(|title| title == "Executor 1"));
            assert!(titles.iter().any(|title| title == "Helper 1"));
        }
        let (id, window) = children.iter().next().unwrap();
        assert_ne!(id, initial_view["threadId"].as_str().unwrap());
        json!({"schema":1,"threadId":id,"window":window,"allExecutors":children})
    } else if quota_handoff {
        let leader: Value =
            serde_json::from_slice(&fs::read(state.join("leader.json")).unwrap()).unwrap();
        let views: Value =
            serde_json::from_slice(&fs::read(state.join("additional-views.json")).unwrap())
                .unwrap();
        let id = leader["threadId"].as_str().unwrap();
        assert_ne!(id, initial_view["threadId"].as_str().unwrap());
        assert!(root.join("successor-visible-at-request.json").is_file());
        json!({"schema":1,"threadId":id,"window":views["threads"][id]})
    } else {
        initial_view.clone()
    };
    fs::write(
        root.join("native-entry-result-ready.json"),
        serde_json::to_vec_pretty(&json!({"state":state,"view":view,"previousView":(quota_handoff || child).then_some(initial_view),"marker":marker})).unwrap(),
    )
    .unwrap();
    let snapshot: harness_core::task_view::Snapshot =
        serde_json::from_value(view["window"].clone()).unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    if !user_stop {
        assert!(
            snapshot.is_visible(&upstream, &user).unwrap(),
            "native conversation must remain visible for the final result"
        );
    }
    fs::write(
        root.join("native-entry-result-observed.json"),
        serde_json::to_vec_pretty(&json!({"finalVisible":!user_stop,"stopped":user_stop})).unwrap(),
    )
    .unwrap();
    let observed: Value =
        serde_json::from_slice(&fs::read(root.join("native-entry-result-observed.json")).unwrap())
            .unwrap();
    if user_stop {
        assert_eq!(observed["stopped"], true);
    } else {
        assert_eq!(observed["finalVisible"], true);
    }
    session.send("/quit").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    session.send("\r").unwrap();
    close_owned_views(&state);
    let result = session
        .wait(
            Deadline::after(WAIT).unwrap(),
            &Cancellation::default(),
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(result.outcome.reason, StopReason::Exited);
    if !user_stop {
        assert_eq!(result.outcome.exit_code, 0);
    }
    let until = Instant::now() + Duration::from_secs(5);
    while !state.join("closed.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        state.join("closed.json").is_file(),
        "idle controller must close after native TUI exit"
    );
    if !user_stop {
        let checkpoint = fs::read_to_string(state.join("task.json")).unwrap();
        assert!(
            checkpoint.contains(&marker),
            "controller must consume the final result"
        );
    }
    let runtime: Value =
        serde_json::from_slice(&fs::read(state.join("runtime.json")).unwrap()).unwrap();
    let identity = harness_core::process::ProcessIdentity {
        pid: runtime["process"]["pid"]
            .as_u64()
            .unwrap()
            .try_into()
            .unwrap(),
        creation_time: runtime["process"]["creationTime"].as_u64().unwrap(),
    };
    let user = harness_core::process_service::current_user().unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while harness_core::process_service::ServiceProcess::inspect(
        identity,
        &build.join("codex-harness.exe"),
        &user,
    )
    .unwrap()
    .is_some()
    {
        assert!(
            Instant::now() < until,
            "owned controller must exit after its completion receipt"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let provider: Value =
        serde_json::from_slice(&fs::read(root.join("provider-1.json")).unwrap()).unwrap();
    assert_eq!(provider["model"], "gpt-6-astra");
    assert_eq!(provider["reasoning"]["effort"], "low");
    let expected_requests = match case {
        EntryCase::Normal => 2,
        EntryCase::Child => 4,
        EntryCase::TwoChildren => 9,
        EntryCase::ViewLoss => 3,
        EntryCase::UserStop => 1,
        EntryCase::QuotaRefusal => 1,
        EntryCase::QuotaHandoff => 3,
        EntryCase::Helper => 7,
        EntryCase::ViewClose => 4,
    };
    for sequence in 1..=expected_requests {
        let exchange: Value = serde_json::from_slice(
            &fs::read(state.join(format!("gateway-exchange-{sequence}.json")))
                .expect("every native provider request must pass the installed task ingress"),
        )
        .unwrap();
        assert_eq!(exchange["completed"], true);
        assert_eq!(exchange["responseStarted"], true);
        assert!(exchange["errorKind"].is_null());
    }
    assert!(
        !state
            .join(format!("gateway-exchange-{}.json", expected_requests + 1))
            .exists()
    );
    assert!(
        root.join(format!("provider-{expected_requests}.json"))
            .is_file()
    );
    assert!(
        !root
            .join(format!("provider-{}.json", expected_requests + 1))
            .exists(),
        "named conversation must not issue an auxiliary model request"
    );
    // The ordinary main session proves the same composed lead context for a
    // direct tool task without delegation; the successor route must carry it
    // from its first request. Neither may leak the executor role text.
    if quota_handoff || matches!(case, EntryCase::Normal) {
        for sequence in 1..=expected_requests {
            let body = fs::read_to_string(root.join(format!("provider-{sequence}.json"))).unwrap();
            let route = match (quota_handoff, sequence) {
                (true, 1) => "first refused lead",
                (true, _) => "configured successor lead",
                (false, _) => "ordinary lead",
            };
            assert!(
                body.contains(CONTROL_COMMON_ROLE_MARKER),
                "{route} request {sequence} must carry the owned common developer text"
            );
            assert!(
                body.contains(CONTROL_LEAD_ROLE_MARKER),
                "{route} request {sequence} must carry the composed lead role text"
            );
            assert!(
                !body.contains(CONTROL_EXECUTOR_ROLE_MARKER),
                "{route} request {sequence} must not carry the executor role text"
            );
        }
    }
    // The ordinary lead's owned action ran through the session's own code-mode
    // `exec` orchestrator, so the session reported its own runtime tool names.
    // The preserved unrelated tools must survive while the competing native
    // codex_worktrees surface stays absent; a missing probe marker fails here
    // instead of passing vacuously.
    if matches!(case, EntryCase::Normal) {
        let receipt: Value = serde_json::from_slice(&fs::read(root.join("catalogue.json")).expect(
            "the ordinary lead must report its runtime tool catalogue before this case can pass",
        ))
        .unwrap();
        let names = receipt["names"]
            .as_array()
            .expect("the catalogue receipt must carry the probed runtime tool names")
            .iter()
            .map(|name| name.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert!(
            !names.is_empty(),
            "the ordinary lead's runtime catalogue must not be empty"
        );
        assert!(
            tool_catalogue::native_worktree_names(&names).is_empty(),
            "the competing native codex_worktrees surface must stay absent from the ordinary lead: {names:?}"
        );
        for preserved in [
            "exec_command",
            "apply_patch",
            "view_image",
            "write_stdin",
            "clock__curr_time",
        ] {
            assert!(
                names.iter().any(|name| name == preserved),
                "the ordinary lead's runtime catalogue must preserve the unrelated {preserved} tool: {names:?}"
            );
        }
        // The synthetic unrelated MCP server configured in this case's owned
        // home must survive the exercised launcher as a real discovery: its
        // tools reach the session's own runtime catalogue under the server's
        // model-visible namespace, and the server recorded the actual
        // initialize/tools-list handshake behind that namespace.
        let mcp_tools = names
            .iter()
            .filter(|name| name.starts_with("mcp__unrelated_probe__"))
            .collect::<Vec<_>>();
        assert!(
            !mcp_tools.is_empty(),
            "the ordinary lead's runtime catalogue must preserve the unrelated MCP server's discovered tools: {names:?}"
        );
        let receipt_path = probe_receipt
            .as_ref()
            .expect("the Normal case owns the synthetic unrelated MCP server");
        let deadline = Instant::now() + Duration::from_secs(5);
        let receipt: Value = loop {
            let complete = fs::read(receipt_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
            match complete {
                Some(value) => break value,
                None if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                None => panic!(
                    "the synthetic unrelated MCP server must leave a complete handshake receipt at {}",
                    receipt_path.display()
                ),
            }
        };
        let calls = receipt["calls"]
            .as_array()
            .expect("the MCP server receipt must record its received methods")
            .iter()
            .map(|call| call.as_str().unwrap())
            .collect::<Vec<_>>();
        for method in ["initialize", "notifications/initialized", "tools/list"] {
            assert!(
                calls.contains(&method),
                "the synthetic unrelated MCP server must record its real {method} handshake: {calls:?}"
            );
        }
        let negotiated = &receipt["initialize"];
        assert!(
            negotiated["protocolVersion"]
                .as_str()
                .is_some_and(|version| !version.is_empty()),
            "the recorded MCP handshake must carry the negotiated protocol revision: {negotiated}"
        );
        fs::write(
            root.join("mcp-probe.json"),
            serde_json::to_vec_pretty(&json!({
                "schema":1,
                "server":"unrelated_probe",
                "toolPrefix":"mcp__unrelated_probe__",
                "tools":mcp_tools,
                "handshake":receipt,
            }))
            .unwrap(),
        )
        .unwrap();
    }
    fs::write(root.join("launch-result.json"), serde_json::to_vec(&json!({"schema":1,"upstreamSha256":build_identity::hash_file(&upstream).unwrap(),"nativeEntrySha256":record.binaries["codex.exe"],"managerSha256":record.binaries["codex-harness.exe"],"initialModel":"gpt-6-astra","model":if quota_handoff {"deepseek-flash"} else {"gpt-6-astra"},"outcome":if quota_handoff {"quotaHandoff"} else if quota_refusal {"quotaRefused"} else {"toolResult"},"toolSideEffectExactlyOnce":(!quota_refusal).then_some(true),"requests":expected_requests,"finalVisible":true,"manualSecondaryStartup":false})).unwrap()).unwrap();
}

fn pair_model_catalog(source: &Path) -> Vec<u8> {
    let bytes = fs::read(source).expect("explicit model metadata, without credentials");
    let mut catalog = if bytes.is_empty() {
        json!({"models": []})
    } else {
        serde_json::from_slice::<Value>(&bytes)
            .expect("model catalog must be JSON metadata without credentials")
    };
    if !catalog["models"].is_array() {
        catalog = json!({"models": []});
    }
    let models = catalog["models"].as_array_mut().unwrap();
    for (slug, display, effort) in [
        ("gpt-6-astra", "GPT-6 Astra", "low"),
        ("deepseek-flash", "DeepSeek V4.1 Flash", "max"),
    ] {
        if !models.iter().any(|model| model["slug"] == slug) {
            models.push(json!({
                "slug": slug,
                "display_name": display,
                "description": display,
                "default_reasoning_level": effort,
                "supported_reasoning_levels": [
                    {"effort": "low", "description": "Light reasoning"},
                    {"effort": "high", "description": "Enhanced reasoning"},
                    {"effort": "max", "description": "Deep reasoning"}
                ],
                "shell_type": "shell_command",
                "visibility": "list",
                "supported_in_api": true,
                "priority": 0,
                "base_instructions": "",
                "supports_reasoning_summaries": true,
                "default_reasoning_summary": "none",
                "support_verbosity": false,
                "apply_patch_tool_type": "freeform",
                "truncation_policy": {"mode": "bytes", "limit": 10000},
                "context_window": 1048576,
                "max_context_window": 1048576,
                "effective_context_window_percent": 95,
                "supports_parallel_tool_calls": true,
                "experimental_supported_tools": [],
                "input_modalities": ["text"],
                "multi_agent_version": "v2"
            }));
        }
    }
    serde_json::to_vec(&catalog).unwrap()
}
fn pause_synthetic_parent_goal(state: &Path) {
    // The canned final cannot complete a native goal. Disable that fixture-only
    // scheduling loop, as in the native child/reconnect contract, while the
    // actual child continues through its own tool/result path.
    let until = Instant::now() + WAIT;
    while !state.join("child-view-requests.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        state.join("child-view-requests.json").is_file(),
        "native child must request its own window"
    );
    let endpoint: Value =
        serde_json::from_slice(&fs::read(state.join("endpoint.json")).unwrap()).unwrap();
    let mut connection = harness_core::task_control::ControlConnection::connect(
        endpoint["port"].as_u64().unwrap() as u16,
        endpoint["token"].as_str().unwrap(),
        WAIT,
    )
    .unwrap();
    for (id, method, params) in [
        (
            1,
            "initialize",
            json!({"clientInfo":{"name":"owned-child-fixture","version":"1"},"capabilities":{"experimentalApi":true}}),
        ),
        (
            2,
            "thread/resume",
            json!({"threadId":endpoint["thread_id"]}),
        ),
        (
            3,
            "thread/goal/set",
            json!({"threadId":endpoint["thread_id"],"objective":"Preserve the owned proof and consume the child result","status":"paused"}),
        ),
    ] {
        connection
            .send(&json!({"id":id,"method":method,"params":params}), WAIT)
            .unwrap();
        let until = Instant::now() + WAIT;
        loop {
            assert!(
                Instant::now() < until,
                "native fixture control reply deadline"
            );
            if let Some(value) = connection.receive(Duration::from_millis(100)).unwrap()
                && value["id"] == id
            {
                assert!(value.get("error").is_none(), "{method}: {value}");
                break;
            }
        }
        if id == 1 {
            connection
                .send(&json!({"method":"initialized"}), WAIT)
                .unwrap();
        }
    }
}

fn quota_refusal_result(root: &Path, state: &Path, workspace: &Path) -> String {
    let until = Instant::now() + WAIT;
    while !state.join("failures.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    let saved: Value = serde_json::from_slice(
        &fs::read(state.join("failures.json")).expect("native terminal refusal must be retained"),
    )
    .unwrap();
    let threads = saved["threads"].as_object().unwrap();
    assert_eq!(threads.len(), 1);
    let failure = &threads.values().next().unwrap()["failure"];
    assert_eq!(
        failure["cause"], "quota",
        "preserve the native cause instead of inferring quota from HTTP 429"
    );
    assert_eq!(failure["error"]["codexErrorInfo"], "usageLimitExceeded");
    assert!(!workspace.join("proof.txt").exists());
    std::thread::sleep(Duration::from_secs(2));
    assert!(!root.join("provider-2.json").exists());
    failure["error"]["message"].as_str().unwrap().to_owned()
}

fn exercise_user_stop(root: &Path, state: &Path, workspace: &Path) {
    let until = Instant::now() + WAIT;
    while !(state.join("view.json").is_file() && workspace.join("proof.txt").is_file())
        && Instant::now() < until
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one"
    );
    harness_core::task_runtime::request_stop(state).unwrap();
    let until = Instant::now() + WAIT;
    while !state.join("closed.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    let closed: Value = serde_json::from_slice(
        &fs::read(state.join("closed.json")).expect("explicit stop must close the controller"),
    )
    .unwrap();
    assert_eq!(closed["reason"], "explicit stop");
    assert_eq!(closed["stopped"], true);
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !root.join("provider-2.json").exists(),
        "explicit stop must not start another model request"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one"
    );
    fs::write(
        root.join("native-entry-stopped.json"),
        serde_json::to_vec(&json!({"state":state,"stopped":true,"requests":1})).unwrap(),
    )
    .unwrap();
}

fn exercise_view_loss(root: &Path, state: &Path, workspace: &Path, executable: &Path) {
    let until = Instant::now() + WAIT;
    while !(state.join("view.json").is_file() && workspace.join("proof.txt").is_file())
        && Instant::now() < until
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one"
    );
    let view: Value = serde_json::from_slice(&fs::read(state.join("view.json")).unwrap()).unwrap();
    let snapshot: harness_core::task_view::Snapshot =
        serde_json::from_value(view["window"].clone()).unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    // The injected fault targets only the exact conversation surface recorded
    // by the controller, and it is validated through the owned identity check
    // before any window is touched.
    assert!(
        snapshot.is_visible(executable, &user).unwrap(),
        "the owned conversation surface must be visible before the injected loss"
    );
    let target = window_identity(snapshot.window);
    fs::write(
        root.join("native-entry-partial-ready.json"),
        serde_json::to_vec(&json!({"state":state,"view":view,"target":target.clone()})).unwrap(),
    )
    .unwrap();
    eprintln!(
        "native partial effect ready; minimize the owned conversation: {}",
        root.display()
    );
    minimize_owned_conversation(&snapshot);
    // Observe the loss through the same owned-surface check the controller
    // uses; the injected minimize must remove this exact surface.
    let until = Instant::now() + WAIT;
    while Instant::now() < until && snapshot.is_visible(executable, &user).unwrap_or(true) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !snapshot.is_visible(executable, &user).unwrap_or(true),
        "the injected minimize must remove the owned conversation surface; inspect the target window identity"
    );
    let until = Instant::now() + WAIT;
    loop {
        let visibility: Value = fs::read(state.join("visibility.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        if visibility["visible"] == false
            && visibility["activeTurns"]
                .as_object()
                .is_some_and(|turns| turns.is_empty())
            && visibility["pendingRequests"]
                .as_object()
                .is_some_and(|pending| pending.is_empty())
            && visibility["recoveryPending"]
                .as_array()
                .is_some_and(|threads| threads.len() == 1)
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "view loss must interrupt and settle the native turn; inspect visibility.json"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    // Observe quiescence after native interruption, not only at the minimize instant.
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !root.join("provider-2.json").exists(),
        "no new model request while required view is unavailable"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one"
    );
    fs::write(
        root.join("native-entry-suspended-ready.json"),
        serde_json::to_vec(
            &json!({"state":state,"view":view,"requests":1,"partialEffectPreserved":true}),
        )
        .unwrap(),
    )
    .unwrap();
    eprintln!(
        "native interruption settled; restore the owned conversation: {}",
        root.display()
    );
    restore_owned_conversation(&snapshot);
    // Record the restoration only after observing it through the same
    // owned-surface check; an unobserved marker would fake the recovery.
    let until = Instant::now() + Duration::from_secs(90);
    while Instant::now() < until && !snapshot.is_visible(executable, &user).unwrap_or(false) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        snapshot.is_visible(executable, &user).unwrap_or(false),
        "the restored conversation must be visible through the owned-surface check"
    );
    fs::write(
        root.join("native-entry-view-restored.json"),
        serde_json::to_vec(
            &json!({"schema":1,"threadId":view["threadId"],"window":snapshot.window,"observedRestored":true,"target":target}),
        )
        .unwrap(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !root.join("provider-2.json").exists(),
        "restoring the view must not resume the model while its previous tool is still running"
    );
    let visibility: Value =
        serde_json::from_slice(&fs::read(state.join("visibility.json")).unwrap()).unwrap();
    assert!(
        visibility["activeOperations"]
            .as_object()
            .is_some_and(|operations| !operations.is_empty()),
        "the unfinished native tool must remain owned after turn interruption"
    );
    fs::write(workspace.join("finish-tool"), "release owned command").unwrap();
}

/// Bounded fault injection against the exact conversation surface recorded by
/// the controller: the same user-visible loss a manual minimize produces.
/// `Snapshot::is_visible` validated this handle immediately before the call,
/// and no other window is touched.
fn minimize_owned_conversation(snapshot: &harness_core::task_view::Snapshot) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_MINIMIZE, ShowWindow};
    unsafe {
        ShowWindow(snapshot.window as _, SW_MINIMIZE);
    }
}

/// Restore the same owned surface without activating or arranging any window.
fn restore_owned_conversation(snapshot: &harness_core::task_view::Snapshot) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_RESTORE, ShowWindow};
    unsafe {
        ShowWindow(snapshot.window as _, SW_RESTORE);
    }
}

/// Recorded identity of a fault target so the evidence names the exact window
/// that was minimized instead of an anonymous handle.
fn window_identity(window: usize) -> Value {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId};
    let mut pid = 0;
    let mut class = [0u16; 128];
    let copied = unsafe {
        GetWindowThreadProcessId(window as _, &mut pid);
        GetClassNameW(window as _, class.as_mut_ptr(), class.len() as i32)
    };
    json!({
        "window": window,
        "ownerPid": pid,
        "class": String::from_utf16_lossy(&class[..copied.max(0) as usize]),
    })
}

fn exercise_view_close(root: &Path, state: &Path, workspace: &Path, executable: &Path) {
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        if state.join("additional-views.json").is_file() && workspace.join("proof.txt").is_file() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(workspace.join("proof.txt")).unwrap(),
        "one"
    );
    let views: Value =
        serde_json::from_slice(&fs::read(state.join("additional-views.json")).unwrap()).unwrap();
    let children = views["threads"].as_object().unwrap();
    assert_eq!(children.len(), 1, "close-restore uses one executor pane");
    let (thread_id, window) = children.iter().next().unwrap();
    let thread_id = thread_id.clone();
    let snapshot: harness_core::task_view::Snapshot =
        serde_json::from_value(window.clone()).unwrap();
    let before = snapshot.process;
    let providers = (1..=16)
        .filter(|n| root.join(format!("provider-{n}.json")).is_file())
        .count();
    close_conversation(&snapshot);
    let until = Instant::now() + WAIT;
    loop {
        let visibility: Value = fs::read(state.join("visibility.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        let hidden = visibility["conversations"]
            .as_object()
            .is_some_and(|map| map.get(&thread_id) == Some(&json!(false)));
        if hidden
            && visibility["pendingRequests"]
                .as_object()
                .is_some_and(|pending| pending.is_empty())
        {
            break;
        }
        assert!(
            Instant::now() < until,
            "closed view must suspend that conversation; inspect visibility.json"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !root
            .join(format!("provider-{}.json", providers + 1))
            .exists(),
        "no new model request while the required view is closed"
    );
    let until = Instant::now() + Duration::from_secs(90);
    let restored = loop {
        assert!(
            Instant::now() < until,
            "closed conversation must restore a new visible window for the same thread"
        );
        if let Ok(bytes) = fs::read(state.join("additional-views.json")) {
            let views: Value = serde_json::from_slice(&bytes).unwrap();
            if let Ok(snapshot) = serde_json::from_value::<harness_core::task_view::Snapshot>(
                views["threads"][&thread_id].clone(),
            ) && snapshot.process != before
            {
                let user = harness_core::process_service::current_user().unwrap();
                if snapshot.is_visible(executable, &user).unwrap_or(false) {
                    break snapshot;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        state.join("view-restore.json").is_file(),
        "controller must report the closed conversation"
    );
    std::thread::sleep(Duration::from_secs(2));
    assert!(
        !root
            .join(format!("provider-{}.json", providers + 1))
            .exists(),
        "restoring the view must not resume the model while its previous tool is still running"
    );
    fs::write(
        root.join("native-entry-view-restored.json"),
        serde_json::to_vec(&json!({
            "threadId": thread_id,
            "previous": before,
            "restored": restored.process
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(workspace.join("finish-tool"), "release owned command").unwrap();
}

fn observe_conversation_visibility(state: &Path, executable: &Path) {
    let until = Instant::now() + WAIT;
    while !state.join("view.json").is_file() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    let view: Value = serde_json::from_slice(&fs::read(state.join("view.json")).unwrap()).unwrap();
    let snapshot: harness_core::task_view::Snapshot =
        serde_json::from_value(view["window"].clone()).unwrap();
    let user = harness_core::process_service::current_user().unwrap();
    // Visibility must come from the owning launcher. Observe it without
    // changing window position, size, activation or stacking order.
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if snapshot.is_visible(executable, &user).unwrap_or(false) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn close_owned_views(state: &Path) {
    let mut windows = Vec::new();
    if let Ok(bytes) = fs::read(state.join("view.json")) {
        let view: Value = serde_json::from_slice(&bytes).unwrap();
        windows.push(view["window"].clone());
    }
    if let Ok(bytes) = fs::read(state.join("additional-views.json")) {
        let views: Value = serde_json::from_slice(&bytes).unwrap();
        if let Some(threads) = views["threads"].as_object() {
            windows.extend(threads.values().cloned());
        }
    }
    for window in windows {
        if let Ok(snapshot) = serde_json::from_value::<harness_core::task_view::Snapshot>(window) {
            close_conversation(&snapshot);
        }
    }
}

fn close_conversation(snapshot: &harness_core::task_view::Snapshot) {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        TerminateProcess,
    };
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            0,
            snapshot.process.pid,
        );
        if handle.is_null() {
            return;
        }
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut created = zero;
        let mut exit = zero;
        let mut kernel = zero;
        let mut user = zero;
        if GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user) == 0 {
            CloseHandle(handle);
            return;
        }
        let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
        if ticks != snapshot.process.creation_time {
            CloseHandle(handle);
            return;
        }
        let _ = TerminateProcess(handle, 0);
        CloseHandle(handle);
    }
}

/// Marker env of the session-owner check below; the child of that check is this
/// same test binary running one filtered case.
const OWNER_CHILD_ENV: &str = "HARNESS_OWNED_TASK_OWNER_CHILD";

/// The task-control session owner joins the account CPU allowance before it
/// starts the service and the native server of its session, so the whole
/// session tree shares the one account ceiling. The owner is a throwaway child
/// of this test binary, so the session it owns cannot outlive the check.
#[test]
fn task_control_session_owner_joins_the_account_budget_before_its_service() {
    let root = std::env::temp_dir().join(format!("task-control-owner-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let account = root.join("cpu-budget");
    let workspace = root.join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let payload_marker = root.join("native-server.pid");
    let manager = PathBuf::from(env!("CARGO_BIN_EXE_codex-harness"));
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_harness-launch-fixture"));

    if std::env::var_os(OWNER_CHILD_ENV).is_some() {
        // The owned native-agent double holds instead of serving the control
        // protocol, so this session reaches its native start and then reports
        // that its controller never became ready - without opening a
        // conversation or a desktop window.
        let mut command = Command::new(&fixture);
        command
            .arg("continue the owned task-control assignment")
            .current_dir(&workspace)
            .env("CODEX_HOME", root.join("home"));
        let outcome = harness_core::task_runtime::run(&command, &manager, &root.join("home"));
        eprintln!("owned owner outcome: {outcome:?}");
        return;
    }

    let owner = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "task_control_session_owner_joins_the_account_budget_before_its_service",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(OWNER_CHILD_ENV, "1")
        .env("CODEX_HARNESS_CPU_ACCOUNT", &account)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "4000")
        .env("HARNESS_LAUNCH_FIXTURE_STARTED", &payload_marker)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let payload = observe_payload(&payload_marker, Duration::from_secs(30));
    let budget = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
    let job = budget.name().to_owned();
    assert_eq!(
        job_cpu_rate(&job),
        (SHARED_CPU_PERCENT * 100.0) as u32,
        "the session's account group must carry the delivered 75% hard cap"
    );
    let owner_handle = observe_pid(owner.id(), Duration::from_secs(30));
    assert!(
        process_in_job(owner_handle, &job),
        "the session owner must join the account allowance before its service"
    );
    assert!(
        process_in_job(payload, &job),
        "the session's native server must be a kernel-verified member of the same group"
    );
    let finished = owner.wait_with_output().unwrap();
    assert!(
        finished.status.success(),
        "the owned session owner reported its own outcome: {}",
        String::from_utf8_lossy(&finished.stderr)
    );
    let reported = format!(
        "{}{}",
        String::from_utf8_lossy(&finished.stdout),
        String::from_utf8_lossy(&finished.stderr)
    );
    assert!(
        reported.contains("owned owner outcome: Err"),
        "the owner must reach its controller start and report the owned double's outcome: {reported}"
    );
    assert!(
        !reported.contains("could not admit") && !reported.contains("runs outside the shared"),
        "a session that joins the allowance must not report degraded coverage: {reported}"
    );
    unsafe { CloseHandle(owner_handle) };
    unsafe { CloseHandle(payload) };
    let _ = fs::remove_dir_all(&root);
}
