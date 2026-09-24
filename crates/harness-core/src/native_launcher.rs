//! Ordinary Codex launch: verify the selected build, then inherit the caller's
//! streams and console. The session process tree is owned by a kill-on-close
//! Job that reaps it after an abnormal launcher death; on an ordinary exit the
//! upstream-managed background processes keep their own lifetime.
//! The payload is created into the account shared CPU budget (outer) and this
//! session's lifecycle Job (inner) when that budget can be established; when it
//! cannot, the launch warns on stderr and starts the same payload once outside
//! the shared allowance instead of failing.
//! `--harness-cpu uncapped` is a per-invocation exception: it is not saved, it
//! does not raise the allowance for other sessions or shared services, and a
//! caller already inside the allowance relaunches that one payload through the
//! existing job-free sibling anchor so the exception is not inherited.
//! Independently started kit services are not members of that job.
use crate::{build_identity, build_selection, launcher};
use serde::{Deserialize, Serialize};
use std::{
    env,
    ffi::OsString,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
};

const REGISTRATION_LIMIT: u64 = 65536;
const MANAGERS: &[&str] = &[
    "CODEX_MANAGED_BY_NPM",
    "CODEX_MANAGED_BY_BUN",
    "CODEX_MANAGED_BY_PNPM",
    "CODEX_MANAGED_BY_VITE_PLUS",
];

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub schema: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<PathBuf>,
    #[serde(default)]
    pub task_control: bool,
    pub upstream: Upstream,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Upstream {
    pub executable: PathBuf,
    pub sha256: String,
    pub package: Option<Package>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub root: PathBuf,
    pub manifest_sha256: String,
    pub manager: PackageManager,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackageManager {
    Npm,
    Bun,
    Pnpm,
    VitePlus,
}

impl PackageManager {
    fn variable(&self) -> &'static str {
        match self {
            Self::Npm => MANAGERS[0],
            Self::Bun => MANAGERS[1],
            Self::Pnpm => MANAGERS[2],
            Self::VitePlus => MANAGERS[3],
        }
    }
}

fn fail(message: &'static str) -> io::Error {
    io::Error::other(message)
}

const DEGRADED_NOTICE: &str = "codex-harness: shared harness unavailable; launching registered Codex without harness overrides";

fn registered_runtime(registration: &Registration) -> io::Result<(PathBuf, bool)> {
    let selected = match (
        registration.schema,
        &registration.state,
        &registration.build,
    ) {
        (1, Some(state), None) if state.is_absolute() => match build_selection::selected(state) {
            Ok(build) => return Ok((build, true)),
            Err(error) => schema_one_build(state).ok_or(error)?,
        },
        (2, None, Some(build)) if build.is_absolute() => build.clone(),
        _ => return Err(fail("unsupported native launch registration")),
    };
    if launcher_identity_matches(&selected)? {
        // Launch admission follows recorded binary integrity: a stale or
        // unreachable checkout keeps the harness overrides of the delivered
        // build, and only damaged or unsupported inputs degrade to upstream.
        let shared = build_identity::check(&selected, None).runtime_allowed;
        Ok((selected.canonicalize()?, shared))
    } else {
        Err(fail(
            "registered native build is stale, missing or altered; explicit update required",
        ))
    }
}

fn schema_one_build(state: &Path) -> Option<PathBuf> {
    crate::native_build::verify_owned_state(state).ok()?;
    if std::fs::metadata(state.join("build-selection-journal.json")).is_ok() {
        return None;
    }
    let bytes = std::fs::read(state.join("active-build.json")).ok()?;
    let pointer: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if pointer.get("schema")?.as_u64()? != 1 {
        return None;
    }
    let name = pointer.get("build")?.as_str()?;
    if Path::new(name).components().count() != 1 {
        return None;
    }
    let expected = pointer.get("record_sha256")?.as_str()?;
    let build = state.join("builds").join(name);
    if build_identity::hash_file(&build.join("build.json"))
        .ok()?
        .as_str()
        != expected
    {
        return None;
    }
    Some(build)
}

fn launcher_identity_matches(build: &Path) -> io::Result<bool> {
    let record = match build_identity::read_record(build) {
        Ok(record) => record,
        Err(_) => return Ok(false),
    };
    let Some(expected) = record.binaries.get("codex.exe") else {
        return Ok(false);
    };
    let launcher = build.join("codex.exe");
    Ok(build_identity::ordinary(&launcher).is_ok()
        && build_identity::hash_file(&launcher).is_ok_and(|actual| actual == *expected))
}

fn shared_config_args(build: &Path, home: &Path) -> io::Result<Vec<OsString>> {
    let record = build_identity::read_record(build)?;
    let bytes = std::fs::read(record.source_root.join("global/kit.json"))?;
    let manifest: crate::inventory::Manifest =
        serde_json::from_slice(&bytes).map_err(|_| fail("invalid live kit manifest"))?;
    crate::portable_config::overrides(&record.source_root.join(manifest.profile), home)
}

/// Best-effort model a plain session start would use for per-model effort
/// defaults. Read failures degrade to no injection, never a blocked launch.
fn session_model(build: &Path, home: &Path) -> Option<String> {
    let record = build_identity::read_record(build).ok()?;
    let bytes = std::fs::read(record.source_root.join("global/kit.json")).ok()?;
    let manifest: crate::inventory::Manifest = serde_json::from_slice(&bytes).ok()?;
    crate::portable_config::effective_default_model(
        &record.source_root.join(manifest.profile),
        home,
    )
    .ok()
    .flatten()
}

fn notice_degraded_session(task_args: &[OsString]) {
    let classified = launcher::profile_arguments(task_args);
    if classified.len() != task_args.len() {
        eprintln!("{DEGRADED_NOTICE}");
    }
}

/// Apply npm/bun/pnpm identity when the registered package is still
/// `@openai/codex`. A changed digest is an upstream update, not a skip.
fn managed_package_env(package: &Package) -> Option<(PathBuf, &'static str)> {
    if !package.root.is_absolute() {
        return None;
    }
    let root = package.root.canonicalize().ok()?;
    let manifest = root.join("package.json");
    let mut data = Vec::new();
    File::open(&manifest)
        .ok()?
        .take(REGISTRATION_LIMIT + 1)
        .read_to_end(&mut data)
        .ok()?;
    if data.len() as u64 > REGISTRATION_LIMIT {
        return None;
    }
    let metadata: serde_json::Value = serde_json::from_slice(&data).ok()?;
    if metadata.get("name").and_then(|v| v.as_str()) != Some("@openai/codex") {
        return None;
    }
    Some((root, package.manager.variable()))
}

pub fn codex_home() -> io::Result<PathBuf> {
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".codex")))
        .ok_or_else(|| fail("CODEX_HOME or USERPROFILE is required"))?;
    home.canonicalize()
}

/// Registration is produced by explicit installation. Ordinary launch performs
/// no discovery, compilation, package acquisition or registration mutation.
fn prepared_command(
    executable: &Path,
    home: &Path,
    args: &[OsString],
) -> io::Result<(Command, bool)> {
    let mut bytes = Vec::new();
    File::open(home.join("harness/native-launch.json"))?
        .take(REGISTRATION_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > REGISTRATION_LIMIT {
        return Err(fail("native launch registration exceeds its bound"));
    }
    let registration: Registration = serde_json::from_slice(&bytes)
        .map_err(|_| fail("invalid native launch registration; explicit repair is required"))?;
    let (selected, shared) = registered_runtime(&registration)?;
    let executable = executable.canonicalize()?;
    if selected.join("codex.exe").canonicalize()? != executable {
        return Err(fail("this launcher is not the selected native build"));
    }
    let upstream = registration.upstream;
    if !upstream.executable.is_absolute()
        || !upstream
            .executable
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        return Err(fail(
            "upstream must be an explicitly registered native executable",
        ));
    }
    let target = upstream.executable.canonicalize()?;
    let target_hash = build_identity::hash_file(&target)?;
    if target == executable || target_hash == build_identity::hash_file(&executable)? {
        return Err(fail(
            "upstream points to a harness launcher; explicit repair required",
        ));
    }
    let task_args = launcher::task_arguments(args)?;
    let default_model = session_model(&selected, home);
    let task_args = launcher::per_model_effort(&task_args, default_model.as_deref());
    // Executor sessions must stay single-agent: the marker is inherited by
    // every Codex process an executor starts, including a raw nested `codex`
    // invocation, so the agent capability stays off for the whole tree.
    let task_args = launcher::executor_limited(
        task_args,
        env::var_os(crate::orchestration_config::EXECUTOR_SESSION_ENV).is_some(),
    );
    let roots = launcher::additional_roots(&task_args, &env::current_dir()?);
    let mut command = Command::new(target);
    let classified = launcher::profile_arguments(&task_args);
    if shared && classified.len() != task_args.len() {
        match shared_config_args(&selected, home) {
            Ok(overrides) => {
                command.args(overrides);
            }
            Err(_) => {
                notice_degraded_session(&task_args);
            }
        }
    } else if !shared {
        notice_degraded_session(&task_args);
    }
    command.args(&task_args);
    if roots.is_empty() {
        command.env_remove("HARNESS_LSP_WORKSPACE_ROOTS");
    } else {
        command.env(
            "HARNESS_LSP_WORKSPACE_ROOTS",
            serde_json::to_string(&roots)?,
        );
    }
    // Match the adopted upstream npm entry point: contradictory manager hints
    // must not leak into a package launch. Bare native installs inherit theirs.
    if let Some(package) = upstream.package
        && let Some((root, variable)) = managed_package_env(&package)
    {
        for name in MANAGERS {
            command.env_remove(name);
        }
        command.env("CODEX_MANAGED_PACKAGE_ROOT", root);
        command.env(variable, "1");
    }
    Ok((command, registration.task_control))
}

/// Result of asking the loopback xAI shim port what it is.
enum ShimProbe {
    /// Nothing is listening; a shim can be started.
    Free,
    /// A kit shim answers and reports the executable that serves it.
    Ours(PathBuf),
    /// Something else holds the port (another program, or a shim build from
    /// before identity reporting existed).
    Unknown,
}

fn same_executable(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

/// Send one bounded loopback control request to the shim port. A connection
/// error means the port is free; any other outcome is interpreted by the
/// caller.
fn shim_control_request(port: u16, method: &str, path: &str) -> io::Result<(u16, Vec<u8>)> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(200),
    )?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2)))?;
    stream.write_all(
        format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .as_bytes(),
    )?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 1024];
    while raw.len() < 8192 {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(size) => raw.extend_from_slice(&chunk[..size]),
            Err(_) => break,
        }
    }
    let (head, body) = match raw.windows(4).position(|window| window == b"\r\n\r\n") {
        Some(position) => (&raw[..position], raw[position + 4..].to_vec()),
        None => (raw.as_slice(), Vec::new()),
    };
    let status = String::from_utf8_lossy(head)
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    Ok((status, body))
}

fn shim_probe(port: u16) -> ShimProbe {
    match shim_control_request(port, "GET", crate::xai_responses_shim::IDENTITY_PATH) {
        Err(_) => ShimProbe::Free,
        Ok((status, body)) => {
            let identity = (status == 200)
                .then(|| serde_json::from_slice::<serde_json::Value>(&body).ok())
                .flatten()
                .filter(|value| {
                    value.get("harness").and_then(|v| v.as_str()) == Some("xai-responses-shim")
                        && value.get("schema").and_then(|v| v.as_u64()) == Some(1)
                });
            match identity
                .and_then(|value| value.get("exe").and_then(|v| v.as_str()).map(PathBuf::from))
            {
                Some(exe) => ShimProbe::Ours(exe),
                None => ShimProbe::Unknown,
            }
        }
    }
}

fn wait_for_shim_free(port: u16) -> io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(70);
    while std::time::Instant::now() < deadline {
        if matches!(shim_probe(port), ShimProbe::Free) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(fail(
        "the retired xAI compatibility shim did not release its port",
    ))
}

/// Start the selected build's shim and confirm the port is served by exactly
/// that executable, so a foreign or outdated listener is never mistaken for it.
fn spawn_xai_shim(port: u16, manager: &Path) -> io::Result<()> {
    #[cfg(windows)]
    let service = {
        use crate::process::{Cancellation, Deadline};
        // Preflight and tool commands run inside kill-on-close Jobs. A normal
        // detached spawn still inherits that Job and dies when its caller is
        // cleaned up. Use the existing independent service bootstrap, which
        // also prevents inheritance of the caller's output pipe handles.
        let environment = env::vars_os()
            .map(|(key, value)| {
                Ok((
                    key.into_string()
                        .map_err(|_| fail("non-Unicode service environment key"))?,
                    value
                        .into_string()
                        .map_err(|_| fail("non-Unicode service environment value"))?,
                ))
            })
            .collect::<io::Result<_>>()?;
        let manager = manager.canonicalize()?;
        let directory = manager
            .parent()
            .ok_or_else(|| fail("shim manager has no directory"))?;
        let spawned = crate::process_service::spawn(
            &manager,
            directory,
            vec![
                "xai-responses-shim".into(),
                "--port".into(),
                port.to_string(),
            ],
            environment,
            Deadline::after(std::time::Duration::from_secs(30))?,
            &Cancellation::default(),
        );
        match spawned {
            Ok(service) => service,
            Err(error) => {
                // Parallel cold starts can race to bind. A losing helper may exit
                // before its identity is observed; accept only a ready peer from
                // this exact build, otherwise preserve the bootstrap failure.
                if matches!(shim_probe(port), ShimProbe::Ours(exe) if same_executable(&exe, &manager))
                {
                    return Ok(());
                }
                return Err(error);
            }
        }
    };
    #[cfg(not(windows))]
    {
        let mut command = Command::new(manager);
        command.args(["xai-responses-shim", "--port", &port.to_string()]);
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        drop(command.spawn()?);
    }
    for _ in 0..250 {
        match shim_probe(port) {
            ShimProbe::Ours(exe) if same_executable(&exe, manager) => return Ok(()),
            ShimProbe::Ours(_) => {
                return Err(fail(
                    "the xAI compatibility shim port is served by another build",
                ));
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
        #[cfg(windows)]
        if !service.is_running()? {
            // The peer can bind between the preceding probe and this exit
            // observation. Recheck readiness after observing the losing child.
            if matches!(shim_probe(port), ShimProbe::Ours(exe) if same_executable(&exe, manager)) {
                return Ok(());
            }
            return Err(io::Error::other(format!(
                "xAI compatibility shim exited before readiness (exit {:?})",
                service.exit_code()?
            )));
        }
    }
    Err(fail("xAI compatibility shim did not become ready"))
}

/// Reuse a shim only when it is the selected build's shim. A shim from a
/// superseded build is retired first, otherwise fixes in the selected build
/// would never reach new sessions while any Codex process kept the old one
/// alive. A listener without identity reporting (a pre-identity shim) is
/// reused as before, with an explicit notice, because it cannot be replaced
/// without breaking a session that may still depend on it.
/// Call from the session host before spawning its owned child tree: the shim
/// is shared by sessions and must not inherit one app-server's cleanup Job.
/// The explicit loopback port also permits isolated lifecycle verification.
pub fn ensure_xai_shim(manager: &Path, port: u16) -> io::Result<()> {
    match shim_probe(port) {
        ShimProbe::Ours(exe) if same_executable(&exe, manager) => return Ok(()),
        ShimProbe::Ours(stale) => {
            eprintln!(
                "codex-harness: replacing the xAI shim from an earlier build ({}); sessions started from that build must be restarted",
                stale.display()
            );
            // The response may race the shim's exit; the released port is the
            // observable that matters.
            let _ = shim_control_request(port, "POST", crate::xai_responses_shim::RETIRE_PATH);
            wait_for_shim_free(port)?;
        }
        ShimProbe::Unknown => {
            eprintln!(
                "codex-harness: 127.0.0.1:{port} is held by an unidentified or pre-identity xAI shim; it is reused unchanged (restart all Codex sessions to replace it)"
            );
            return Ok(());
        }
        ShimProbe::Free => {}
    }
    if !manager.is_file() {
        return Err(fail("xAI shim manager is absent from the selected build"));
    }
    spawn_xai_shim(port, manager)
}

pub fn command(executable: &Path, home: &Path, args: &[OsString]) -> io::Result<Command> {
    prepared_command(executable, home, args).map(|(command, _)| command)
}

#[cfg(windows)]
unsafe extern "system" fn console_control(event: u32) -> i32 {
    use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT};
    // Both processes share the console and receive the event. Do not broadcast
    // it again; keep this wrapper alive to return the child's actual exit code.
    i32::from(matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT))
}

#[cfg(windows)]
struct ConsoleHandler;

#[cfg(windows)]
impl ConsoleHandler {
    fn install() -> io::Result<Self> {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
        if unsafe { SetConsoleCtrlHandler(Some(console_control), 1) } == 0 {
            let error = io::Error::last_os_error();
            // A pipe-only invocation can have no attached console.
            if error.raw_os_error() != Some(6) {
                return Err(error);
            }
        }
        Ok(Self)
    }
}

#[cfg(windows)]
impl Drop for ConsoleHandler {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(console_control), 0);
        }
    }
}

#[cfg(windows)]
fn interactive_spec(command: &Command) -> io::Result<crate::process::CommandSpec> {
    let program = PathBuf::from(command.get_program());
    if !program.is_absolute() {
        return Err(fail("upstream must be an absolute path"));
    }
    let mut spec = crate::process::CommandSpec::new(program);
    spec.args = command.get_args().map(|arg| arg.to_os_string()).collect();
    spec.current_dir = command.get_current_dir().map(|path| path.to_path_buf());
    spec.inherit_console = true;
    for (key, value) in command.get_envs() {
        spec.env
            .insert(key.to_os_string(), value.map(|value| value.to_os_string()));
    }
    Ok(spec)
}

// [`crate::heavy_command::SHARED_CPU_ESCAPE_HATCH`] is the per-launch ceiling
// override, expressed like [`crate::process::SHARED_CPU_PERCENT`] as a percent
// of total host CPU. It never comes from tracked source and wins over the
// machine-local policy record. An absent hatch reads that record; an absent
// record keeps the installed default. An unusable hatch or record degrades to
// the warned fallback instead of silently changing the requested ceiling.

/// Bound on the account critical section (one object create plus one small
/// ownership-record write) before this launch reports the failed stage and
/// proceeds. A stuck holder must delay a session start, never hang it.
#[cfg(windows)]
const CPU_ADMISSION_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(windows)]
const CPU_ACQUISITION_RECOVERY: &str = "repair the account budget state named in the cause (an unowned or conflicting shared job object, or an unreadable ownership record) and start the session again; the next launch retries the cap automatically";

/// The account group exists, but the kernel refused to create this payload
/// inside it. A dispatching hierarchy that already assigns the launcher to
/// another job can reject the ordered placement outright; the payload then
/// starts once in its session lifecycle Job alone.
#[cfg(windows)]
const CPU_PLACEMENT_RECOVERY: &str = "start the session from a plain interactive shell when the shared cap is required, and report the dispatching route so its job hierarchy can be reconciled; the next launch retries the cap automatically";

/// Visible degradation notice for one failed CPU-admission attempt. It names
/// the requested ceiling, the failed stage and cause, the affected scope and
/// the recovery action, and never claims that inherited limits were removed.
/// stderr only: stdout belongs to the payload and to machine protocols.
#[cfg(windows)]
fn warn_cpu_fallback(requested: &str, stage: &str, cause: &str, recovery: &str) {
    eprintln!(
        "codex-harness: shared agent CPU cap not verified: requested ceiling {requested}; failed stage: {stage}; cause: {cause}; scope: this session starts outside the verified account CPU group, so its own CPU use is not bounded by the shared allowance while other sessions keep theirs; recovery: {recovery}"
    );
}

#[cfg(windows)]
struct CpuAdmission {
    /// Requested ceiling as reported to the user, e.g. `75% of host CPU`.
    requested: String,
    /// The established account group, or `None` after a warned fail-open.
    budget: Option<crate::process::SharedCpuBudget>,
}

/// Join or establish this account's shared CPU budget for the payload that is
/// about to start. Fail-open contract: every setup, acquisition or readback
/// failure warns on stderr and yields no group, so the caller starts the
/// requested payload exactly once without the shared ceiling. Nothing is
/// persisted here, no build or update is required, and other sessions keep
/// their own handles, membership and settings.
#[cfg(windows)]
fn session_cpu_admission() -> CpuAdmission {
    use crate::heavy_command::{shared_cpu_escape_hatch, shared_cpu_policy_ceiling};
    use crate::process::{CPU_BUDGET_ACCOUNT_ENV, SHARED_CPU_PERCENT, cpu_budget_directory};
    if let Some(hatch) = shared_cpu_escape_hatch() {
        return match hatch {
            Ok(ceiling) => admit_shared_cpu(ceiling.percent, ceiling.requested),
            Err(fault) => warn_ceiling_fault(fault),
        };
    }
    let directory = match cpu_budget_directory(None) {
        Ok(directory) => directory,
        Err(error) => {
            let requested = format!("{SHARED_CPU_PERCENT}% of host CPU");
            warn_cpu_fallback(
                &requested,
                "account storage",
                &error.to_string(),
                &format!(
                    "point {CPU_BUDGET_ACCOUNT_ENV} at a writable absolute account directory or restore LOCALAPPDATA"
                ),
            );
            return CpuAdmission {
                requested,
                budget: None,
            };
        }
    };
    match shared_cpu_policy_ceiling(&directory) {
        Ok(ceiling) => admit_shared_cpu(ceiling.percent, ceiling.requested),
        Err(fault) => warn_ceiling_fault(fault),
    }
}

#[cfg(windows)]
fn warn_ceiling_fault(fault: crate::heavy_command::SharedCpuCeilingFault) -> CpuAdmission {
    warn_cpu_fallback(&fault.requested, fault.stage, &fault.cause, &fault.recovery);
    CpuAdmission {
        requested: fault.requested,
        budget: None,
    }
}

#[cfg(windows)]
fn admit_shared_cpu(percent: f64, requested: String) -> CpuAdmission {
    use crate::process::{
        CPU_BUDGET_ACCOUNT_ENV, Cancellation, Deadline, SharedCpuBudget, cpu_budget_directory,
    };
    let directory = match cpu_budget_directory(None) {
        Ok(directory) => directory,
        Err(error) => {
            warn_cpu_fallback(
                &requested,
                "account storage",
                &error.to_string(),
                &format!(
                    "point {CPU_BUDGET_ACCOUNT_ENV} at a writable absolute account directory or restore LOCALAPPDATA"
                ),
            );
            return CpuAdmission {
                requested,
                budget: None,
            };
        }
    };
    let deadline = match Deadline::after(CPU_ADMISSION_WAIT) {
        Ok(deadline) => deadline,
        Err(error) => {
            warn_cpu_fallback(
                &requested,
                "budget admission",
                &error.to_string(),
                CPU_ACQUISITION_RECOVERY,
            );
            return CpuAdmission {
                requested,
                budget: None,
            };
        }
    };
    let budget = match SharedCpuBudget::acquire_within(
        &directory,
        percent,
        deadline,
        &Cancellation::default(),
    ) {
        Ok(budget) => Some(budget),
        Err(error) => {
            warn_cpu_fallback(
                &requested,
                "budget admission",
                &error.to_string(),
                CPU_ACQUISITION_RECOVERY,
            );
            None
        }
    };
    CpuAdmission { requested, budget }
}

pub fn run(executable: &Path, home: &Path, args: &[OsString]) -> io::Result<i32> {
    // An unusable upstream executable must fail as an error, not as a desktop
    // loader dialog.
    crate::process::suppress_loader_dialogs();
    let (uncapped, args) = take_uncapped_session_selector(args)?;
    let (command, task_control) = prepared_command(executable, home, &args)?;
    if launcher::xai_shim_requested(&args) {
        let manager = executable
            .canonicalize()?
            .parent()
            .ok_or_else(|| fail("launcher has no build directory"))?
            .join("codex-harness.exe");
        ensure_xai_shim(&manager, crate::xai_responses_shim::DEFAULT_PORT)?;
    }
    #[cfg(windows)]
    let _console = ConsoleHandler::install()?;
    #[cfg(windows)]
    if uncapped {
        // An explicit exception does not enter task-control admission: that
        // owner joins the shared allowance before its payload exists.
        return launch_uncapped_session(&command);
    }
    #[cfg(windows)]
    if task_control
        && let Some(code) = crate::task_runtime::run(
            &command,
            &executable
                .canonicalize()?
                .parent()
                .ok_or_else(|| fail("launcher has no build directory"))?
                .join("codex-harness.exe"),
            home,
        )?
    {
        return Ok(code);
    }
    // Shared kit services are started as siblings before this wait. The session
    // Job owns the upstream CLI tree: an abnormal launcher death reaps it,
    // while an ordinary exit preserves upstream-managed background processes.
    #[cfg(windows)]
    let code = {
        let spec = interactive_spec(&command)?;
        let job = crate::process::Job::new(crate::process::Limits::default())?;
        // Admission happens before payload creation: the ordered pair keeps the
        // account CPU budget outside this session's lifecycle Job, so no
        // payload code runs outside the shared ceiling. When the budget cannot
        // be established, or when the kernel refuses to place this payload
        // inside it, the same lifecycle Job alone starts the requested payload
        // exactly once with its arguments, cwd, streams and exit status. Every
        // failed attempt below is terminated while still suspended, so no
        // payload code has run and no work is replayed or duplicated.
        let admission = session_cpu_admission();
        let child = match &admission.budget {
            Some(budget) => match budget.spawn(&job, &spec) {
                Ok(child) => child,
                Err(error) => {
                    warn_cpu_fallback(
                        &admission.requested,
                        "session placement",
                        &format!("the ordered payload placement was refused ({error})"),
                        CPU_PLACEMENT_RECOVERY,
                    );
                    job.spawn(&spec)?
                }
            },
            None => job.spawn(&spec)?,
        };
        // `admission` is held until this wait returns: while a member runs, the
        // named object stays this account's one shared allowance for any later
        // session, and closing one participant's handle changes nothing for
        // peers.
        job.wait_session_root(&child)? as i32
    };
    #[cfg(not(windows))]
    let code = command
        .status()?
        .code()
        .ok_or_else(|| fail("upstream terminated without an exit code"))?;
    Ok(code)
}

/// Leading session selector. `uncapped` is the only accepted mode; anything
/// else is a usage error rather than a silent return to the shared allowance.
/// The flag is consumed only in the harness-option prefix, including beside
/// `--harness-effort`, and is never forwarded to the payload.
pub fn take_uncapped_session_selector(args: &[OsString]) -> io::Result<(bool, Vec<OsString>)> {
    let mut uncapped = false;
    let mut kept = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let Some(text) = args[index].to_str() else {
            break;
        };
        if text == "--" {
            break;
        }
        if text == "--harness-cpu" || text.starts_with("--harness-cpu=") {
            let value = if let Some(value) = text.strip_prefix("--harness-cpu=") {
                value
            } else {
                index += 1;
                args.get(index).and_then(|arg| arg.to_str()).ok_or_else(|| {
                    fail("--harness-cpu requires uncapped; the shared allowance stays the default")
                })?
            };
            if !value.eq_ignore_ascii_case("uncapped") {
                return Err(fail(
                    "--harness-cpu requires uncapped; the shared allowance stays the default",
                ));
            }
            if uncapped {
                return Err(fail("duplicate --harness-cpu"));
            }
            uncapped = true;
            index += 1;
            continue;
        }
        if text == "--harness-effort" || text.starts_with("--harness-effort=") {
            kept.push(args[index].clone());
            if text == "--harness-effort" {
                index += 1;
                if let Some(value) = args.get(index) {
                    kept.push(value.clone());
                }
            }
            index += 1;
            continue;
        }
        break;
    }
    kept.extend_from_slice(&args[index..]);
    Ok((uncapped, kept))
}

/// stderr notice for one explicit exception. It names the shared ceiling and
/// the consequence for combined load, and it is not the fail-open warning.
pub fn uncapped_notice(scope: &str) -> String {
    format!(
        "codex-harness: explicit uncapped invocation: this {scope} runs outside the shared {} account CPU allowance; combined host agent load can exceed that ceiling while this exception runs; other sessions and shared services keep their allowance, and the next invocation without this selector is capped by default",
        configured_shared_ceiling_label()
    )
}

fn configured_shared_ceiling_label() -> String {
    #[cfg(windows)]
    {
        use crate::heavy_command::{
            shared_cpu_escape_hatch, shared_cpu_percent_label, shared_cpu_policy_ceiling,
        };
        use crate::process::{SHARED_CPU_PERCENT, cpu_budget_directory};
        if let Some(hatch) = shared_cpu_escape_hatch() {
            return match hatch {
                Ok(ceiling) => format!("{}%", shared_cpu_percent_label(ceiling.percent)),
                Err(_) => "configured".to_owned(),
            };
        }
        if let Ok(directory) = cpu_budget_directory(None)
            && let Ok(ceiling) = shared_cpu_policy_ceiling(&directory)
        {
            return format!("{}%", shared_cpu_percent_label(ceiling.percent));
        }
        if cpu_budget_directory(None).is_ok() {
            return "configured".to_owned();
        }
        format!("{SHARED_CPU_PERCENT}%")
    }
    #[cfg(not(windows))]
    {
        use crate::process::SHARED_CPU_PERCENT;
        format!("{SHARED_CPU_PERCENT}%")
    }
}

/// Wrapper entry. The process that owns the lifecycle Job waits on this
/// process; its exit code is the payload's. Not a service and not a session.
pub const CPU_EXCEPTION_LAUNCH: &str = "--harness-cpu-exception-launch";

const CPU_EXCEPTION_ANCHOR: &str = "cpu-exception-anchor";
const CPU_EXCEPTION_PIPE: &str = "HARNESS_CPU_EXCEPTION_PIPE";

#[cfg(windows)]
fn launch_uncapped_session(command: &Command) -> io::Result<i32> {
    let spec = interactive_spec(command)?;
    if process_in_any_job()? {
        // A child of this process would inherit the capped ancestry. The
        // named lifecycle Job is reapplied by the outside-group launcher.
        let name = format!(
            "CodingAgentsHarness.CpuException.{}",
            crate::broker_endpoint::random_key()?
        );
        let job = crate::process::Job::new_named(crate::process::Limits::default(), &name)?;
        let launch = spawn_uncapped(&name, &spec, "session")?;
        let code = launch.wrapper_job.wait_session_root(&launch.process)? as i32;
        drop(job);
        return Ok(code);
    }
    // This process is not in a job, so a direct child is already outside the
    // shared allowance. Do not assign that allowance for this invocation.
    eprintln!("{}", uncapped_notice("session"));
    let job = crate::process::Job::new(crate::process::Limits::default())?;
    let child = job.spawn(&spec)?;
    Ok(job.wait_session_root(&child)? as i32)
}

/// A bounded outside-group launcher and the job that contains only that launcher.
#[cfg(windows)]
pub struct UncappedLaunch {
    pub process: crate::process::OwnedProcess,
    pub wrapper_job: crate::process::Job,
}

/// Start `command` outside the caller's job ancestry and inside the named
/// lifecycle Job. The wrapper is not placed in that Job: the caller is already
/// in the shared allowance, and assigning such a process would nest the Job.
#[cfg(windows)]
pub fn spawn_uncapped(
    lifecycle_name: &str,
    command: &crate::process::CommandSpec,
    scope: &str,
) -> io::Result<UncappedLaunch> {
    let bootstrap = exception_bootstrap()?;
    let pipe_name = format!(
        r"\\.\pipe\CodingAgentsHarness.CpuException.{}",
        crate::broker_endpoint::random_key()?
    );
    let request = exception_request(command, lifecycle_name, scope)?;
    let pipe = create_request_pipe(&pipe_name)?;
    let mut wrapper = crate::process::CommandSpec::new(bootstrap);
    wrapper.args = vec![OsString::from(CPU_EXCEPTION_LAUNCH)];
    wrapper.current_dir = command.current_dir.clone();
    wrapper
        .env
        .insert(OsString::from(CPU_EXCEPTION_PIPE), Some(pipe_name.into()));
    if command.inherit_console {
        wrapper.inherit_console = true;
    } else {
        wrapper.stdin = command.stdin.as_ref().map(clone_file).transpose()?;
        wrapper.stdout = command.stdout.as_ref().map(clone_file).transpose()?;
        wrapper.stderr = command.stderr.as_ref().map(clone_file).transpose()?;
    }
    let wrapper_job = crate::process::Job::new(crate::process::Limits::default())?;
    let process = wrapper_job.spawn(&wrapper)?;
    if let Err(error) = write_exception_request(&pipe, &request) {
        drop(process);
        return Err(error);
    }
    Ok(UncappedLaunch {
        process,
        wrapper_job,
    })
}

#[cfg(windows)]
fn clone_file(file: &std::fs::File) -> io::Result<std::fs::File> {
    file.try_clone()
}

/// Hold the job-free sibling created by the existing service helper. It is a
/// creation parent only: it does not join the allowance and does not run a
/// payload. A request that is not this anchor is refused.
#[cfg(windows)]
pub fn hold_cpu_exception_anchor() -> ! {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.get(3).and_then(|arg| arg.to_str()) != Some(CPU_EXCEPTION_ANCHOR) {
        eprintln!("codex-harness: refusing an unrecognized bootstrap run request");
        std::process::exit(2);
    }
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(30) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::process::exit(0);
}

/// Read one bounded launch request and create its payload outside this
/// process's job ancestry. Exits with the payload's code.
#[cfg(windows)]
pub fn cpu_exception_launch_entry() -> i32 {
    match cpu_exception_launch() {
        Ok(code) => code,
        Err(error) => {
            eprintln!(
                "codex-harness: explicit uncapped invocation was not started ({error}); no payload ran"
            );
            127
        }
    }
}

#[cfg(windows)]
fn process_in_any_job() -> io::Result<bool> {
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut member = 0;
    if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut member) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(member != 0)
}

#[cfg(windows)]
fn exception_bootstrap() -> io::Result<PathBuf> {
    let current = env::current_exe()?.canonicalize()?;
    if current
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("codex.exe"))
    {
        return Ok(current);
    }
    let sibling = current
        .parent()
        .ok_or_else(|| io::Error::other("launcher has no directory"))?
        .join("codex.exe");
    if sibling.is_file() {
        return sibling.canonicalize();
    }
    Err(io::Error::other(
        "uncapped launch needs the codex bootstrap beside this executable",
    ))
}

#[cfg(windows)]
fn exception_request(
    command: &crate::process::CommandSpec,
    job: &str,
    scope: &str,
) -> io::Result<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;
    let units = |value: &std::ffi::OsStr| -> io::Result<Vec<u16>> {
        let encoded: Vec<u16> = value.encode_wide().collect();
        if encoded.contains(&0) {
            return Err(io::Error::other("uncapped launch input contains NUL"));
        }
        Ok(encoded)
    };
    let mut variables: Vec<_> = env::vars_os().collect();
    for (name, value) in &command.env {
        variables.retain(|(existing, _)| {
            !existing
                .to_string_lossy()
                .eq_ignore_ascii_case(&name.to_string_lossy())
        });
        if let Some(value) = value {
            variables.push((name.clone(), value.clone()));
        }
    }
    variables.sort_by(|left, right| left.0.cmp(&right.0));
    let environment = variables
        .into_iter()
        .map(|(name, value)| Ok((units(&name)?, units(&value)?)))
        .collect::<io::Result<Vec<_>>>()?;
    let shared_job = shared_cpu_job_name();
    let request = serde_json::json!({
        "program": units(command.program.as_os_str())?,
        "arguments": command.args.iter().map(|arg| units(arg)).collect::<io::Result<Vec<_>>>()?,
        "directory": command.current_dir.as_ref().map(|path| units(path.as_os_str())).transpose()?,
        "environment": environment,
        "job": job,
        "shared_job": shared_job,
        "scope": scope,
    });
    let bytes = serde_json::to_vec(&request)
        .map_err(|_| io::Error::other("uncapped launch request could not be encoded"))?;
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::other(
            "uncapped launch request exceeds its bound",
        ));
    }
    Ok(bytes)
}

#[cfg(windows)]
fn shared_cpu_job_name() -> Option<String> {
    use crate::heavy_command::{shared_cpu_escape_hatch, shared_cpu_policy_ceiling};
    use crate::process::{SharedCpuBudget, cpu_budget_directory};
    let directory = cpu_budget_directory(None).ok()?;
    let ceiling = if let Some(hatch) = shared_cpu_escape_hatch() {
        hatch.ok()?
    } else {
        shared_cpu_policy_ceiling(&directory).ok()?
    };
    SharedCpuBudget::acquire(&directory, ceiling.percent)
        .ok()
        .map(|budget| budget.name().to_owned())
}

#[cfg(windows)]
fn create_request_pipe(name: &str) -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    let mut wide: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().collect();
    wide.push(0);
    let handle = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            0x0000_0002,
            PIPE_TYPE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            64 * 1024,
            64 * 1024,
            0,
            std::ptr::null(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) })
}

#[cfg(windows)]
fn write_exception_request(
    pipe: &std::os::windows::io::OwnedHandle,
    request: &[u8],
) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_PIPE_CONNECTED;
    use windows_sys::Win32::Storage::FileSystem::WriteFile;
    use windows_sys::Win32::System::Pipes::ConnectNamedPipe;
    let handle = pipe.as_raw_handle();
    let (sender, receiver) = std::sync::mpsc::channel();
    let connect_handle = handle as isize;
    std::thread::spawn(move || {
        let connected = unsafe {
            ConnectNamedPipe(
                connect_handle as windows_sys::Win32::Foundation::HANDLE,
                std::ptr::null_mut(),
            )
        };
        let error = if connected == 0 {
            io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        let _ = sender.send((connected, error));
    });
    let (connected, error) = receiver
        .recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|_| {
            io::Error::other("uncapped launch did not connect to its outside-group launcher")
        })?;
    if connected == 0 && error != Some(ERROR_PIPE_CONNECTED as i32) {
        return Err(io::Error::other(
            "uncapped launch did not connect to its outside-group launcher",
        ));
    }
    let mut written = 0u32;
    let ok = unsafe {
        WriteFile(
            handle,
            request.as_ptr(),
            request.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 || written as usize != request.len() {
        return Err(io::Error::other(
            "uncapped launch request was not delivered",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn cpu_exception_launch() -> io::Result<i32> {
    use std::io::Read;
    use std::os::windows::ffi::OsStringExt;
    let pipe_name = env::var(CPU_EXCEPTION_PIPE)
        .map_err(|_| io::Error::other("uncapped launch is missing its request pipe"))?;
    let mut file = open_request_pipe(&pipe_name)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::other(
            "uncapped launch request exceeds its bound",
        ));
    }
    let request: ExceptionRequest = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::other("uncapped launch request is invalid"))?;
    if request.scope != "session" && request.scope != "command" {
        return Err(io::Error::other("uncapped launch scope is invalid"));
    }
    let program = std::ffi::OsString::from_wide(&request.program);
    let arguments = request
        .arguments
        .iter()
        .map(|arg| std::ffi::OsString::from_wide(arg))
        .collect::<Vec<_>>();
    let directory = request
        .directory
        .as_ref()
        .map(|path| std::ffi::OsString::from_wide(path));
    let anchor = start_exception_anchor()?;
    let code = create_uncapped_payload(
        &anchor,
        &PreparedPayload {
            program: &program,
            arguments: &arguments,
            directory: directory.as_deref(),
            environment: &request.environment,
            job_name: &request.job,
            shared_job: request.shared_job.as_deref(),
            scope: &request.scope,
        },
    )?;
    let _ = anchor.terminate(0);
    Ok(code)
}

#[cfg(windows)]
#[derive(Deserialize)]
struct ExceptionRequest {
    program: Vec<u16>,
    arguments: Vec<Vec<u16>>,
    directory: Option<Vec<u16>>,
    environment: Vec<(Vec<u16>, Vec<u16>)>,
    job: String,
    shared_job: Option<String>,
    scope: String,
}

#[cfg(windows)]
fn open_request_pipe(name: &str) -> io::Result<std::fs::File> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, OPEN_EXISTING,
    };
    let mut wide: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().collect();
    wide.push(0);
    let started = std::time::Instant::now();
    loop {
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::FILE_GENERIC_READ,
                FILE_SHARE_READ,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            return Ok(unsafe { std::fs::File::from_raw_handle(handle) });
        }
        if started.elapsed() > std::time::Duration::from_secs(15) {
            return Err(io::Error::other(
                "uncapped launch request pipe was not available",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// One job-free sibling of this bootstrap, created by the existing service
/// helper. The sibling is a creation parent only; closing it does not grant
/// cleanup authority over a payload created through it.
#[cfg(windows)]
fn start_exception_anchor() -> io::Result<crate::process_service::ServiceProcess> {
    use crate::cancellable_pipe::anonymous_pipe;
    use crate::process::{CommandSpec, Deadline, Job, Limits, StopReason};
    use crate::process_service::{self, CREATE_ARGUMENT};
    use std::io::Write;
    use std::time::Duration;
    let bootstrap = exception_bootstrap()?;
    let directory = env::current_dir()?.canonicalize().or_else(|_| {
        bootstrap
            .parent()
            .ok_or_else(|| io::Error::other("bootstrap has no directory"))
            .map(Path::to_path_buf)
    })?;
    let began = process_service::creation_clock();
    let user = process_service::current_user()?;
    let mut environment = std::collections::BTreeMap::new();
    match env::var("SystemRoot") {
        Ok(root) => {
            environment.insert("SystemRoot".to_owned(), root);
        }
        Err(_) => {
            environment.insert("HARNESS_CPU_EXCEPTION_ANCHOR".to_owned(), "1".to_owned());
        }
    }
    let request = serde_json::json!({
        "directory": directory,
        "arguments": [CPU_EXCEPTION_ANCHOR],
        "environment": environment,
        "startup_until": began / 10_000 + 20_000,
        "user": user,
    });
    let bytes = serde_json::to_vec(&request)
        .map_err(|_| io::Error::other("uncapped anchor request could not be encoded"))?;
    let (stdin, mut write) = anonymous_pipe(4096)?;
    let (read, stdout) = anonymous_pipe(4096)?;
    let mut command = CommandSpec::new(&bootstrap);
    command.args.push(CREATE_ARGUMENT.into());
    command.current_dir = Some(directory);
    command.stdin = Some(stdin);
    command.stdout = Some(stdout);
    let job = Job::new(Limits {
        memory_bytes: Some(256 * 1024 * 1024),
        cpu_percent: Some(25.0),
    })?;
    let child = job.spawn(&command)?;
    write.write_all(&bytes)?;
    drop(write);
    let cancel = crate::process::Cancellation::default();
    let deadline = Deadline::after(Duration::from_secs(20))?;
    let mut reader = crate::cancellable_pipe::CancellablePipe::reader(read, cancel.clone())?;
    let receipt_bytes = reader
        .read(1024, deadline, &cancel)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let outcome = job.wait(&child, deadline, &cancel, Duration::from_secs(5))?;
    if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        return Err(io::Error::other(
            "uncapped anchor helper did not finish inside its bound",
        ));
    }
    #[derive(Deserialize)]
    struct Receipt {
        pid: Option<u32>,
        error: Option<String>,
    }
    let receipt: Receipt = serde_json::from_slice(&receipt_bytes)
        .map_err(|_| io::Error::other("uncapped anchor helper receipt is invalid"))?;
    let pid = receipt.pid.ok_or_else(|| {
        io::Error::other(
            receipt
                .error
                .unwrap_or_else(|| "uncapped anchor helper returned no process".to_owned()),
        )
    })?;
    process_service::ServiceProcess::observe(pid, &bootstrap, began, &user)
}

#[cfg(windows)]
struct PreparedPayload<'a> {
    program: &'a std::ffi::OsStr,
    arguments: &'a [std::ffi::OsString],
    directory: Option<&'a std::ffi::OsStr>,
    environment: &'a [(Vec<u16>, Vec<u16>)],
    job_name: &'a str,
    shared_job: Option<&'a str>,
    scope: &'a str,
}

#[cfg(windows)]
fn create_uncapped_payload(
    anchor: &crate::process_service::ServiceProcess,
    payload: &PreparedPayload<'_>,
) -> io::Result<i32> {
    let program = payload.program;
    let arguments = payload.arguments;
    let directory = payload.directory;
    let environment = payload.environment;
    let job_name = payload.job_name;
    let shared_job = payload.shared_job;
    let scope = payload.scope;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
        DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
        InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
        PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS,
        PROCESS_CREATE_PROCESS, PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION,
        ResumeThread, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, INFINITE};
    const JOB_OBJECT_ASSIGN_PROCESS: u32 = 0x0001;
    const JOB_OBJECT_QUERY: u32 = 0x0004;
    const STARTF_USESTDHANDLES: u32 = 0x0000_0100;
    let wide = |value: &std::ffi::OsStr| -> io::Result<Vec<u16>> {
        let mut encoded: Vec<u16> = value.encode_wide().collect();
        if encoded.contains(&0) {
            return Err(io::Error::other("uncapped payload input contains NUL"));
        }
        encoded.push(0);
        Ok(encoded)
    };
    let lifecycle = open_named_job(job_name, JOB_OBJECT_ASSIGN_PROCESS | JOB_OBJECT_QUERY)?;
    let parent = open_process(
        anchor.identity().pid,
        PROCESS_CREATE_PROCESS | PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION,
    )?;
    let standards = [
        unsafe { GetStdHandle(STD_INPUT_HANDLE) },
        unsafe { GetStdHandle(STD_OUTPUT_HANDLE) },
        unsafe { GetStdHandle(STD_ERROR_HANDLE) },
    ];
    let nul = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("NUL")?;
    let mut remote = [std::ptr::null_mut(); 3];
    for (slot, handle) in remote.iter_mut().zip(standards) {
        let source =
            if handle.is_null() || handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                nul.as_raw_handle()
            } else {
                handle
            };
        let ok = unsafe {
            windows_sys::Win32::Foundation::DuplicateHandle(
                GetCurrentProcess(),
                source,
                parent.as_raw_handle(),
                slot,
                0,
                1,
                windows_sys::Win32::Foundation::DUPLICATE_SAME_ACCESS,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let mut bytes = 0usize;
    unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 3, 0, &mut bytes) };
    if bytes == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut attributes = vec![0u8; bytes];
    if unsafe {
        InitializeProcThreadAttributeList(attributes.as_mut_ptr().cast(), 3, 0, &mut bytes)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let parent_handle = parent.as_raw_handle();
    let jobs = [lifecycle.as_raw_handle()];
    let attribute_ok = unsafe {
        UpdateProcThreadAttribute(
            attributes.as_mut_ptr().cast(),
            0,
            PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
            (&parent_handle as *const HANDLE).cast(),
            std::mem::size_of::<HANDLE>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
            && UpdateProcThreadAttribute(
                attributes.as_mut_ptr().cast(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                remote.as_ptr().cast(),
                std::mem::size_of_val(&remote),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
            && UpdateProcThreadAttribute(
                attributes.as_mut_ptr().cast(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                jobs.as_ptr().cast(),
                std::mem::size_of_val(&jobs),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) != 0
    };
    if !attribute_ok {
        let error = io::Error::last_os_error();
        unsafe { DeleteProcThreadAttributeList(attributes.as_mut_ptr().cast()) };
        return Err(error);
    }
    let application = wide(program)?;
    let mut line = Vec::new();
    crate::process::quote_argument(program, &mut line)?;
    for arg in arguments {
        line.push(32);
        crate::process::quote_argument(arg, &mut line)?;
    }
    if line.len() >= 32767 {
        unsafe { DeleteProcThreadAttributeList(attributes.as_mut_ptr().cast()) };
        return Err(io::Error::other(
            "uncapped payload command line exceeds the Windows bound",
        ));
    }
    line.push(0);
    let directory = directory.map(wide).transpose()?;
    let environment = environment_block(environment)?;
    #[repr(C)]
    struct StartupInfoEx {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_count: u32,
        y_count: u32,
        fill: u32,
        flags: u32,
        show: u16,
        reserved2: u16,
        reserved3: *mut u8,
        stdin: HANDLE,
        stdout: HANDLE,
        stderr: HANDLE,
        attribute_list: *mut std::ffi::c_void,
    }
    #[repr(C)]
    struct ProcessInformation {
        process: HANDLE,
        thread: HANDLE,
        pid: u32,
        tid: u32,
    }
    let mut startup = StartupInfoEx {
        cb: std::mem::size_of::<StartupInfoEx>() as u32,
        reserved: std::ptr::null_mut(),
        desktop: std::ptr::null_mut(),
        title: std::ptr::null_mut(),
        x: 0,
        y: 0,
        x_size: 0,
        y_size: 0,
        x_count: 0,
        y_count: 0,
        fill: 0,
        flags: STARTF_USESTDHANDLES,
        show: 0,
        reserved2: 0,
        reserved3: std::ptr::null_mut(),
        stdin: remote[0],
        stdout: remote[1],
        stderr: remote[2],
        attribute_list: attributes.as_mut_ptr().cast(),
    };
    let mut info = ProcessInformation {
        process: std::ptr::null_mut(),
        thread: std::ptr::null_mut(),
        pid: 0,
        tid: 0,
    };
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_SUSPENDED
                | CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT,
            environment.as_ptr().cast(),
            directory
                .as_ref()
                .map_or(std::ptr::null(), |path| path.as_ptr()),
            &mut startup as *mut StartupInfoEx as *mut _,
            &mut info as *mut ProcessInformation as *mut _,
        )
    };
    unsafe { DeleteProcThreadAttributeList(attributes.as_mut_ptr().cast()) };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    let process = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(info.process) };
    let thread = unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(info.thread) };
    let outside = match shared_job {
        Some(name) => !process_in_named_job(process.as_raw_handle(), name)?,
        None => true,
    };
    let contained = process_in_named_job(process.as_raw_handle(), job_name)?;
    if !outside || !contained {
        unsafe { TerminateProcess(process.as_raw_handle(), 127) };
        let anchor_inside = match shared_job {
            Some(name) => process_in_named_job(parent.as_raw_handle(), name).unwrap_or(false),
            None => false,
        };
        return Err(io::Error::other(format!(
            "uncapped payload did not leave the shared CPU allowance (outside={outside}, lifecycle={contained}, anchor_inside={anchor_inside}, shared={shared_job:?}); it was not resumed"
        )));
    }
    eprintln!("{}", uncapped_notice(scope));
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        let error = io::Error::last_os_error();
        unsafe { TerminateProcess(process.as_raw_handle(), 127) };
        return Err(error);
    }
    if unsafe { WaitForSingleObject(process.as_raw_handle(), INFINITE) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut code = 0;
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // Normal completion preserves payload-owned background processes. An
    // abnormal caller death still closes the lifecycle Job with kill-on-close
    // armed, because this disarm has not run.
    if let Err(error) = disarm_lifecycle_job(job_name) {
        eprintln!(
            "codex-harness: warning: could not preserve background processes after the uncapped payload ({error})"
        );
    }
    Ok(code as i32)
}

#[cfg(windows)]
fn disarm_lifecycle_job(name: &str) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    let job = open_named_job(name, 0x0002 | 0x0004)?;
    let mut extended = unsafe { std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() };
    let queried = unsafe {
        windows_sys::Win32::System::JobObjects::QueryInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&mut extended as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            std::ptr::null_mut(),
        )
    };
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    extended.BasicLimitInformation.LimitFlags &= !JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let updated = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&extended as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if updated == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn open_named_job(name: &str, access: u32) -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::System::JobObjects::OpenJobObjectW;
    let mut wide: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().collect();
    wide.push(0);
    let handle = unsafe { OpenJobObjectW(access, 0, wide.as_ptr()) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) })
}

#[cfg(windows)]
fn open_process(pid: u32, access: u32) -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::System::Threading::OpenProcess;
    let handle = unsafe { OpenProcess(access, 0, pid) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(handle) })
}

#[cfg(windows)]
#[cfg(windows)]
fn process_in_named_job(
    process: windows_sys::Win32::Foundation::HANDLE,
    name: &str,
) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::IsProcessInJob;
    let job = open_named_job(name, 0x0004)?;
    let mut member = 0;
    if unsafe { IsProcessInJob(process, job.as_raw_handle(), &mut member) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(member != 0)
}

#[cfg(windows)]
fn environment_block(variables: &[(Vec<u16>, Vec<u16>)]) -> io::Result<Vec<u16>> {
    let mut block = Vec::new();
    for (name, value) in variables {
        if name.is_empty() || name.contains(&0) || value.contains(&0) {
            return Err(io::Error::other("uncapped payload environment is invalid"));
        }
        block.extend_from_slice(name);
        block.push(u16::from(b'='));
        block.extend_from_slice(value);
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::{
        build_identity,
        process::{CommandSpec, Deadline, Job, Limits, StopReason},
    };
    use serde_json::json;
    use std::{
        fs,
        io::Write,
        path::{Path, PathBuf},
        process::Stdio,
        sync::Arc,
        time::Duration,
    };

    fn rustc() -> PathBuf {
        let output = std::process::Command::new("where.exe")
            .arg("rustc.exe")
            .output()
            .unwrap();
        PathBuf::from(
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
    }

    fn pid_running(pid: u32) -> bool {
        use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        };
        unsafe {
            let handle = OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            );
            if handle.is_null() {
                return false;
            }
            let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
            CloseHandle(handle);
            running
        }
    }

    /// Loopback stand-in for a running xAI shim: answers the identity endpoint
    /// with the file the test wants it to report and honors retirement.
    struct ShimFixture {
        port: u16,
        retired: Arc<std::sync::atomic::AtomicBool>,
        worker: Option<std::thread::JoinHandle<()>>,
    }

    impl ShimFixture {
        fn start(exe: &str) -> Self {
            use std::io::{Read, Write};
            use std::net::TcpListener;
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            listener.set_nonblocking(true).unwrap();
            let retired = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let observed = Arc::clone(&retired);
            let exe = exe.to_string();
            let worker = std::thread::spawn(move || {
                loop {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            let mut buffer = [0u8; 2048];
                            let size = stream.read(&mut buffer).unwrap_or(0);
                            let request = String::from_utf8_lossy(&buffer[..size]).into_owned();
                            if request.starts_with("GET /__harness/xai-shim/identity") {
                                let body = json!({
                                    "harness": "xai-responses-shim",
                                    "schema": 1,
                                    "exe": exe,
                                })
                                .to_string();
                                let _ = stream.write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len()
                                )
                                .as_bytes(),
                            );
                            } else if request.starts_with("POST /__harness/xai-shim/retire") {
                                observed.store(true, std::sync::atomic::Ordering::SeqCst);
                                let body = "{\"retiring\":true}";
                                let _ = stream.write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                    body.len()
                                )
                                .as_bytes(),
                            );
                                return;
                            } else {
                                let _ = stream.write_all(
                                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
                                );
                            }
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(10)),
                    }
                }
            });
            Self {
                port,
                retired,
                worker: Some(worker),
            }
        }

        fn join(&mut self) {
            if let Some(worker) = self.worker.take() {
                worker.join().unwrap();
            }
        }
    }

    #[test]
    fn shim_probe_separates_identity_unknown_and_free_ports() {
        let mut ours = ShimFixture::start(r"C:\build-a\codex-harness.exe");
        match shim_probe(ours.port) {
            ShimProbe::Ours(exe) => {
                assert!(same_executable(
                    &exe,
                    Path::new(r"C:\build-a\codex-harness.exe")
                ))
            }
            other => panic!("expected identity, got {}", probe_name(&other)),
        }
        let _ = shim_control_request(ours.port, "POST", crate::xai_responses_shim::RETIRE_PATH);
        ours.join();

        // A responder that is not the kit shim must classify as unknown.
        use std::io::Write;
        use std::net::TcpListener;
        let foreign = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let foreign_port = foreign.local_addr().unwrap().port();
        let foreign_worker = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = foreign.accept() {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
        });
        assert!(matches!(shim_probe(foreign_port), ShimProbe::Unknown));
        foreign_worker.join().unwrap();

        // A dropped listener leaves the port free.
        let free = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let free_port = free.local_addr().unwrap().port();
        drop(free);
        assert!(matches!(shim_probe(free_port), ShimProbe::Free));
    }

    fn probe_name(probe: &ShimProbe) -> &'static str {
        match probe {
            ShimProbe::Free => "free",
            ShimProbe::Ours(_) => "ours",
            ShimProbe::Unknown => "unknown",
        }
    }

    #[test]
    fn matching_shim_is_reused_without_spawning() {
        let root = tempfile::tempdir().unwrap();
        let manager = root.path().join("codex-harness.exe");
        // Deliberately empty: an attempted launch of it would fail, so Ok()
        // proves the matching shim was reused.
        fs::write(&manager, b"").unwrap();
        let mut fixture = ShimFixture::start(&manager.display().to_string());
        assert!(ensure_xai_shim(&manager, fixture.port).is_ok());
        assert!(!fixture.retired.load(std::sync::atomic::Ordering::SeqCst));
        let _ = shim_control_request(fixture.port, "POST", crate::xai_responses_shim::RETIRE_PATH);
        fixture.join();
    }

    #[test]
    fn shim_from_another_build_is_retired_before_start() {
        let root = tempfile::tempdir().unwrap();
        let manager = root.path().join("codex-harness.exe");
        let mut fixture = ShimFixture::start(r"C:\superseded-build\codex-harness.exe");
        let error = ensure_xai_shim(&manager, fixture.port).unwrap_err();
        assert!(
            error.to_string().contains("absent from the selected build"),
            "{error}"
        );
        assert!(fixture.retired.load(std::sync::atomic::Ordering::SeqCst));
        fixture.join();
        assert!(
            matches!(shim_probe(fixture.port), ShimProbe::Free),
            "retired shim must release the port"
        );
    }

    fn compile_fixture(root: &Path, name: &str, source: &str) -> PathBuf {
        let stem = name.trim_end_matches(".exe");
        let output = std::path::absolute(root).unwrap().join(name);
        fs::write(root.join(format!("{stem}.rs")), source).unwrap();
        let mut command = CommandSpec::new(rustc());
        command.args = vec![
            root.join(format!("{stem}.rs")).into_os_string(),
            "--edition=2024".into(),
            "-o".into(),
            output.as_os_str().to_owned(),
        ];
        let log = fs::File::create(root.join(format!("{name}.compile.log"))).unwrap();
        command.stdout = Some(log.try_clone().unwrap());
        command.stderr = Some(log);
        let job = Job::new(Limits {
            memory_bytes: Some(512 * 1024 * 1024),
            cpu_percent: Some(50.0),
        })
        .unwrap();
        let child = job.spawn(&command).unwrap();
        let outcome = job
            .wait(
                &child,
                Deadline::after(Duration::from_secs(30)).unwrap(),
                &crate::process::Cancellation::default(),
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(
            (outcome.reason, outcome.exit_code),
            (StopReason::Exited, 0),
            "{name} compile evidence: {}",
            root.display()
        );
        output
    }

    struct Fixture {
        root: PathBuf,
        home: PathBuf,
        launcher: PathBuf,
        upstream: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::Builder::new()
                .prefix("native-launch проба-")
                .tempdir()
                .unwrap()
                .keep();
            let source = root.join("source");
            let build = root.join("build");
            let home = root.join("codex");
            for dir in [
                source.join("global/agents"),
                source.join("skills/one"),
                source.join("crates/one/src"),
                source.join("tools/rtk-adapter/src"),
                source
                    .join(build_identity::INSPECTION_SCHEMA)
                    .parent()
                    .unwrap()
                    .to_owned(),
                build.clone(),
                home.join("harness"),
            ] {
                fs::create_dir_all(dir).unwrap();
            }
            for file in [
                "Cargo.toml",
                "Cargo.lock",
                "crates/one/src/lib.rs",
                build_identity::INSPECTION_SCHEMA,
            ] {
                fs::write(source.join(file), b"fixture").unwrap();
            }
            fs::write(
                source.join("global/profile.toml"),
                "approval_policy = 'never'\nsandbox_mode = 'danger-full-access'\nmodel = 'gpt-6-astra'\n",
            )
            .unwrap();
            fs::write(
                source.join("global/kit.json"),
                serde_json::to_vec(&json!({
                    "schema":1,
                    "profile_name":"harness",
                    "profile":"global/profile.toml",
                    "instructions":"global/instructions.md",
                    "skills":"skills",
                    "agents":"global/agents",
                    "hooks":"global/hooks.json",
                    "token_hooks":"global/token-hooks.json"
                }))
                .unwrap(),
            )
            .unwrap();
            let compile = root.join("compile");
            fs::create_dir_all(&compile).unwrap();
            let launcher = compile_fixture(
                &compile,
                "codex.exe",
                include_str!("../tests/fixtures/fake_codex_launcher.rs"),
            );
            let upstream = compile_fixture(
                &compile,
                "upstream.exe",
                include_str!("../tests/fixtures/fake_codex_launch_target.rs"),
            );
            fs::copy(&launcher, build.join("codex.exe")).unwrap();
            for binary in build_identity::BINARIES
                .iter()
                .filter(|name| **name != "codex.exe")
            {
                fs::write(build.join(binary), binary.as_bytes()).unwrap();
            }
            let record = build_identity::BuildRecord {
                schema: build_identity::SCHEMA,
                source_root: source.clone(),
                source: build_identity::source_identity(&source).unwrap(),
                rustc: "fixture".into(),
                cargo: "fixture".into(),
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
            fs::write(
                home.join("harness/native-launch.json"),
                serde_json::to_vec_pretty(&Registration {
                    schema: 2,
                    state: None,
                    build: Some(build.clone()),
                    task_control: false,
                    upstream: Upstream {
                        executable: upstream.clone(),
                        sha256: build_identity::hash_file(&upstream).unwrap(),
                        package: None,
                    },
                })
                .unwrap(),
            )
            .unwrap();
            Self {
                root,
                home,
                launcher: build.join("codex.exe"),
                upstream,
            }
        }
    }

    #[test]
    fn command_refuses_self_recursion() {
        let fixture = Fixture::new();
        let mut registration: serde_json::Value = serde_json::from_slice(
            &fs::read(fixture.home.join("harness/native-launch.json")).unwrap(),
        )
        .unwrap();
        registration["upstream"]["executable"] = json!(fixture.launcher);
        registration["upstream"]["sha256"] =
            json!(build_identity::hash_file(&fixture.launcher).unwrap());
        fs::write(
            fixture.home.join("harness/native-launch.json"),
            serde_json::to_vec_pretty(&registration).unwrap(),
        )
        .unwrap();
        let error = command(&fixture.launcher, &fixture.home, &[]).unwrap_err();
        assert!(error.to_string().contains("harness launcher"), "{}", error);
    }

    #[test]
    fn command_launches_when_upstream_or_package_digest_changes() {
        let fixture = Fixture::new();
        let path = fixture.home.join("harness/native-launch.json");
        let mut registration: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        registration["upstream"]["sha256"] = json!("0".repeat(64));
        fs::write(&path, serde_json::to_vec_pretty(&registration).unwrap()).unwrap();
        let mut prepared = command(&fixture.launcher, &fixture.home, &["exec".into()]).unwrap();
        prepared.stdout(Stdio::piped()).stderr(Stdio::piped());
        let output = prepared.output().unwrap();
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("exec"), "{stdout}");
        assert_eq!(String::from_utf8(output.stderr).unwrap(), "ERR:ok");

        let package = fixture.root.join("package");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("package.json"), r#"{"name":"@openai/codex"}"#).unwrap();
        registration["upstream"]["sha256"] =
            json!(build_identity::hash_file(&fixture.upstream).unwrap());
        registration["upstream"]["package"] = json!({
            "root": package,
            "manifest_sha256": "0".repeat(64),
            "manager": "npm"
        });
        fs::write(&path, serde_json::to_vec_pretty(&registration).unwrap()).unwrap();
        let prepared = command(&fixture.launcher, &fixture.home, &["exec".into()]).unwrap();
        assert!(
            prepared.get_envs().any(|(key, value)| {
                key == "CODEX_MANAGED_BY_NPM" && value == Some(std::ffi::OsStr::new("1"))
            }),
            "managed npm identity must still be applied after a package digest change"
        );
    }

    #[test]
    fn command_refuses_a_different_selected_launcher() {
        let fixture = Fixture::new();
        let other = fixture.root.join("other-codex.exe");
        fs::copy(&fixture.launcher, &other).unwrap();
        let error = command(&other, &fixture.home, &[]).unwrap_err();
        assert!(
            error.to_string().contains("not the selected native build"),
            "{}",
            error
        );
    }

    #[test]
    fn isolated_launch_forwards_unicode_argv_stdin_streams_and_exit() {
        let fixture = Fixture::new();
        let mut prepared = command(
            &fixture.launcher,
            &fixture.home,
            &[
                "exec".into(),
                "путь с пробелами".into(),
                "quote\"inside".into(),
            ],
        )
        .unwrap();
        prepared
            .env("HARNESS_UPSTREAM_EXIT", "7")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = prepared.spawn().unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all("Кириллица\nsecond line".as_bytes())
            .unwrap();
        drop(child.stdin.take());
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(7));
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("exec\u{1f}путь с пробелами\u{1f}quote\"inside"),
            "{stdout}"
        );
        assert!(stdout.contains("approval_policy=\"never\""), "{stdout}");
        assert!(stdout.contains("STDIN:Кириллица\nsecond line"), "{stdout}");
        assert_eq!(String::from_utf8(output.stderr).unwrap(), "ERR:ok");
        assert!(fixture.upstream.exists());
    }

    #[test]
    fn task_effort_is_applied_before_the_registered_upstream() {
        let fixture = Fixture::new();
        let mut prepared = command(
            &fixture.launcher,
            &fixture.home,
            &[
                "--harness-effort".into(),
                "routine".into(),
                "exec".into(),
                "hello".into(),
            ],
        )
        .unwrap();
        prepared.stdout(Stdio::piped()).stderr(Stdio::piped());
        let output = prepared.output().unwrap();
        assert_eq!(output.status.code(), Some(0));
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("exec\u{1f}hello"), "{stdout}");
        assert!(stdout.contains("approval_policy=\"never\""), "{stdout}");
        assert!(stdout.contains("exec\u{1f}hello"), "{stdout}");
        assert!(
            stdout.contains("model_reasoning_effort=\"low\""),
            "{stdout}"
        );
    }

    #[test]
    fn run_preserves_detached_session_helper_after_an_ordinary_exit() {
        let fixture = Fixture::new();
        let compile = fixture.root.join("session-parent");
        fs::create_dir_all(&compile).unwrap();
        let parent = compile_fixture(
            &compile,
            "session-parent.exe",
            include_str!("../tests/fixtures/fake_codex_session_parent.rs"),
        );
        let path = fixture.home.join("harness/native-launch.json");
        let mut registration: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        registration["upstream"]["executable"] = json!(parent);
        registration["upstream"]["sha256"] = json!(build_identity::hash_file(&parent).unwrap());
        fs::write(&path, serde_json::to_vec_pretty(&registration).unwrap()).unwrap();
        let marker = fixture.root.join("orphan.pid");
        let code = run(
            &fixture.launcher,
            &fixture.home,
            &[
                "--orphan-marker".into(),
                marker.as_os_str().to_owned(),
                "--exit".into(),
                "7".into(),
            ],
        )
        .unwrap();
        assert_eq!(code, 7);
        let pid: u32 = fs::read_to_string(&marker).unwrap().trim().parse().unwrap();
        assert!(
            pid_running(pid),
            "ordinary launcher exit must preserve upstream-managed background processes"
        );
        // The session Job reaps the helper only after an abnormal launcher
        // death; clean the fixture up directly here.
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_TERMINATE, TerminateProcess,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !handle.is_null() {
                let _ = TerminateProcess(handle, 0);
                let _ = windows_sys::Win32::Foundation::CloseHandle(handle);
            }
        }
    }
}
