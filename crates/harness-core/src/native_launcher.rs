//! Ordinary Codex launch: verify the selected build, then inherit the caller's
//! streams and console. Upstream owns its persistent background-process lifetime.
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
        let shared = matches!(
            build_identity::check(&selected, None).status,
            build_identity::Health::Healthy
        );
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

fn notice_degraded_session(task_args: &[OsString]) {
    let classified = launcher::profile_arguments(task_args);
    if classified.len() != task_args.len() {
        eprintln!("{DEGRADED_NOTICE}");
    }
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
pub fn command(executable: &Path, home: &Path, args: &[OsString]) -> io::Result<Command> {
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
    if target_hash != upstream.sha256 {
        return Err(fail(
            "upstream executable changed; explicit update is required",
        ));
    }
    let task_args = launcher::task_arguments(args)?;
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
    if let Some(package) = upstream.package {
        if !package.root.is_absolute() {
            return Err(fail("managed package root must be absolute"));
        }
        let root = package.root.canonicalize()?;
        let manifest = root.join("package.json");
        if build_identity::hash_file(&manifest)? != package.manifest_sha256 {
            return Err(fail(
                "upstream package metadata changed; explicit update required",
            ));
        }
        let mut data = Vec::new();
        File::open(&manifest)?
            .take(REGISTRATION_LIMIT + 1)
            .read_to_end(&mut data)?;
        let metadata: serde_json::Value =
            serde_json::from_slice(&data).map_err(|_| fail("invalid upstream package metadata"))?;
        if data.len() as u64 > REGISTRATION_LIMIT
            || metadata.get("name").and_then(|v| v.as_str()) != Some("@openai/codex")
        {
            return Err(fail("unexpected upstream package identity"));
        }
        for variable in MANAGERS {
            command.env_remove(variable);
        }
        command.env("CODEX_MANAGED_PACKAGE_ROOT", root);
        command.env(package.manager.variable(), "1");
    }
    Ok(command)
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

pub fn run(executable: &Path, home: &Path, args: &[OsString]) -> io::Result<i32> {
    let mut command = command(executable, home, args)?;
    #[cfg(windows)]
    let _console = ConsoleHandler::install()?;
    // Unlike bounded helper jobs, ordinary upstream sessions may deliberately
    // leave managed background processes alive. Inherit streams and the console
    // directly and do not impose a kill-on-wrapper-close job on the CLI.
    let status = command.status()?;
    status
        .code()
        .ok_or_else(|| fail("upstream terminated without an exit code"))
}
