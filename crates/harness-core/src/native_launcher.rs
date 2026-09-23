//! Ordinary Codex launch: verify the selected build, then inherit the caller's
//! streams and console. The session process tree is owned by a kill-on-close
//! Job that reaps it after an abnormal launcher death; on an ordinary exit the
//! upstream-managed background processes keep their own lifetime.
//! The payload is created into the account shared CPU budget (outer) and this
//! session's lifecycle Job (inner) when that budget can be established; when it
//! cannot, the launch warns on stderr and starts the same payload once outside
//! the shared allowance instead of failing.
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
    let mut command = Command::new(manager);
    command.args(["xai-responses-shim"]);
    command.stdin(std::process::Stdio::null());
    command.stdout(std::process::Stdio::null());
    command.stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    // The returned handle is dropped: the shim must outlive this launcher.
    drop(spawn_background(&mut command)?);
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
    }
    Err(fail("xAI compatibility shim did not become ready"))
}

/// Spawn the resident shim without transferring the caller's stdio handles.
/// `Stdio::null()` replaces the child's std handles but the caller's own
/// inheritable stdout/stderr handles are still duplicated into the child on
/// Windows, so a piped `codex` invocation would stay open until the shim
/// exits. The shim may outlive every session, so clear the inherit flag for
/// the duration of its creation and restore it afterwards.
#[cfg(windows)]
fn spawn_background(command: &mut Command) -> io::Result<std::process::Child> {
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, HANDLE_FLAG_INHERIT, SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    let mut cleared = Vec::new();
    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        let handle = unsafe { GetStdHandle(id) };
        if handle.is_null() {
            continue;
        }
        let mut flags = 0u32;
        if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
            continue;
        }
        if flags & HANDLE_FLAG_INHERIT != 0
            && unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } != 0
        {
            cleared.push(handle);
        }
    }
    let spawned = command.spawn();
    for handle in cleared {
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
    }
    spawned
}

#[cfg(not(windows))]
fn spawn_background(command: &mut Command) -> io::Result<std::process::Child> {
    command.spawn()
}

/// Reuse a shim only when it is the selected build's shim. A shim from a
/// superseded build is retired first, otherwise fixes in the selected build
/// would never reach new sessions while any Codex process kept the old one
/// alive. A listener without identity reporting (a pre-identity shim) is
/// reused as before, with an explicit notice, because it cannot be replaced
/// without breaking a session that may still depend on it.
fn ensure_xai_shim(manager: &Path) -> io::Result<()> {
    ensure_xai_shim_on(crate::xai_responses_shim::DEFAULT_PORT, manager)
}

fn ensure_xai_shim_on(port: u16, manager: &Path) -> io::Result<()> {
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

/// Machine-local ceiling override for the shared account CPU budget, expressed
/// like [`crate::process::SHARED_CPU_PERCENT`] as a percent of total host CPU.
/// The value never comes from tracked source: an absent override keeps the
/// installed default and an unusable one degrades to the warned fallback
/// instead of silently changing the requested policy.
#[cfg(windows)]
const CPU_PERCENT_ENV: &str = "CODEX_HARNESS_CPU_PERCENT";

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
    use crate::process::{
        CPU_BUDGET_ACCOUNT_ENV, Cancellation, Deadline, SHARED_CPU_PERCENT, SharedCpuBudget,
        cpu_budget_directory,
    };
    let (requested, percent) = match env::var_os(CPU_PERCENT_ENV).filter(|value| !value.is_empty())
    {
        None => (
            format!("{SHARED_CPU_PERCENT}% of host CPU"),
            SHARED_CPU_PERCENT,
        ),
        Some(value) => {
            let text = value.to_string_lossy().into_owned();
            let requested = format!("{text}% of host CPU");
            match text.trim().parse::<f64>() {
                Ok(percent) => (requested, percent),
                Err(_) => {
                    warn_cpu_fallback(
                        &requested,
                        "ceiling configuration",
                        &format!("{CPU_PERCENT_ENV} does not hold a number"),
                        &format!(
                            "set {CPU_PERCENT_ENV} to a percentage within 0.01..=100 or unset it to use the installed {SHARED_CPU_PERCENT}% default"
                        ),
                    );
                    return CpuAdmission {
                        requested,
                        budget: None,
                    };
                }
            }
        }
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
    let (command, task_control) = prepared_command(executable, home, args)?;
    if launcher::xai_shim_requested(args) {
        let manager = executable
            .canonicalize()?
            .parent()
            .ok_or_else(|| fail("launcher has no build directory"))?
            .join("codex-harness.exe");
        ensure_xai_shim(&manager)?;
    }
    #[cfg(windows)]
    let _console = ConsoleHandler::install()?;
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
        sync::{Arc, mpsc},
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
        assert!(ensure_xai_shim_on(fixture.port, &manager).is_ok());
        assert!(!fixture.retired.load(std::sync::atomic::Ordering::SeqCst));
        let _ = shim_control_request(fixture.port, "POST", crate::xai_responses_shim::RETIRE_PATH);
        fixture.join();
    }

    #[test]
    fn shim_from_another_build_is_retired_before_start() {
        let root = tempfile::tempdir().unwrap();
        let manager = root.path().join("codex-harness.exe");
        let mut fixture = ShimFixture::start(r"C:\superseded-build\codex-harness.exe");
        let error = ensure_xai_shim_on(fixture.port, &manager).unwrap_err();
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

    #[test]
    fn background_spawn_does_not_keep_the_caller_pipeline_open() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::HANDLE_FLAG_INHERIT;
        use windows_sys::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE, SetStdHandle};
        let compile = tempfile::tempdir().unwrap();
        let child_exe = compile_fixture(
            compile.path(),
            "long-lived-child.exe",
            include_str!("../tests/fixtures/long_lived_child.rs"),
        );
        // A fresh pipe stands in for the pipeline a script or shell would use
        // to capture `codex` output. Its write end becomes this process's
        // stdout, which is inheritable exactly like the real capture pipe.
        let (reader, writer) = crate::cancellable_pipe::anonymous_pipe(4096).unwrap();
        assert_ne!(
            unsafe {
                windows_sys::Win32::Foundation::SetHandleInformation(
                    writer.as_raw_handle(),
                    HANDLE_FLAG_INHERIT,
                    HANDLE_FLAG_INHERIT,
                )
            },
            0
        );
        let previous = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        assert_ne!(
            unsafe { SetStdHandle(STD_OUTPUT_HANDLE, writer.as_raw_handle()) },
            0
        );
        let mut command = Command::new(&child_exe);
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        let spawned = spawn_background(&mut command);
        assert_ne!(unsafe { SetStdHandle(STD_OUTPUT_HANDLE, previous) }, 0);
        let mut child = spawned.expect("fixture child must start");
        drop(writer);

        // EOF must arrive while the child is still alive: the child did not
        // inherit the write end. A blocking read would mean the caller's
        // pipeline stays open until the shim exits.
        let (signal, wait) = mpsc::channel();
        let reader_thread = std::thread::spawn(move || {
            let mut reader = reader;
            let mut chunk = [0u8; 256];
            loop {
                match std::io::Read::read(&mut reader, &mut chunk) {
                    Ok(0) => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
            let _ = signal.send(());
        });
        let closed = wait.recv_timeout(Duration::from_secs(5));
        child.kill().ok();
        let _ = child.wait_with_output();
        assert!(
            closed.is_ok(),
            "the background child kept the caller's output pipe open"
        );
        reader_thread.join().unwrap();
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
