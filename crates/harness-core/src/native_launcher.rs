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
    Ok((command, registration.task_control))
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

pub fn run(executable: &Path, home: &Path, args: &[OsString]) -> io::Result<i32> {
    let (mut command, task_control) = prepared_command(executable, home, args)?;
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
    // Unlike bounded helper jobs, ordinary upstream sessions may deliberately
    // leave managed background processes alive. Inherit streams and the console
    // directly and do not impose a kill-on-wrapper-close job on the CLI.
    let status = command.status()?;
    status
        .code()
        .ok_or_else(|| fail("upstream terminated without an exit code"))
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
    fn command_refuses_self_recursion_and_wrong_upstream() {
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

        let foreign = fixture.root.join("foreign.exe");
        fs::write(&foreign, b"not-the-registered-upstream").unwrap();
        registration["upstream"]["executable"] = json!(foreign);
        fs::write(
            fixture.home.join("harness/native-launch.json"),
            serde_json::to_vec_pretty(&registration).unwrap(),
        )
        .unwrap();
        let error = command(&fixture.launcher, &fixture.home, &[]).unwrap_err();
        assert!(
            error.to_string().contains("upstream executable changed"),
            "{}",
            error
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
}
