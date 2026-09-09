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
    pub state: PathBuf,
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
    if registration.schema != 1 || !registration.state.is_absolute() {
        return Err(fail("unsupported native launch registration"));
    }
    let selected = build_selection::selected(&registration.state)?;
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
    command.args(launcher::profile_arguments(&task_args));
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
