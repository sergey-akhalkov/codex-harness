//! Candidate-independent acceptance for exclusive executor dispatch claims.
//!
//! `harness-executor-fixture claim-oracle --workspace DIR [--exe PROGRAM]`
//! `harness-executor-fixture claim-oracle --workspace DIR --prepare
//!     --cargo PROGRAM --resource-owner PROGRAM`
//!
//! The checker drives the candidate's real `executor spawn` / `executor run`
//! entry points against an owned `file://` pool. It does not implement the
//! claim fix, open a model conversation, or treat a printed acceptance line
//! as proof that a host received an assignment.
//!
//! The overlap proof gates `git fetch` after the claim and before host
//! publication. A per-pool startup guard may hold the contender until that
//! gate is released; that wait is not a failure. After release the checker
//! requires a real model-free handoff. The independent app-server must have
//! received the expected assignment in the actual `turn/start` request; a
//! receipt or a constant host message is not delivery. Truncated, timed-out,
//! and unreaped observations fail, including a host that exits 0 after
//! exceeding the retained output bound. The checker joins every process it
//! started and does not treat an unresolved cleanup as success.
//! Piped capture is an explicit expectation: missing files are not treated as
//! a null stream. Null-stream children remain supported. Termination runs
//! through the owned job helper and cannot wait without a deadline. Recovery
//! records process identity and retained artifacts; it never advises killing
//! a pid that may already have been reused.

use harness_core::{
    build_identity,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, ProcessIdentity, StopReason},
    process_service::{self, ServiceProcess},
};
use serde_json::{Value, json};
use std::{
    env, fs, io,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

// control_fixture already includes this endpoint and is not editable here.
#[allow(clippy::duplicate_mod)]
#[path = "../../../tests/fixtures/control_endpoint.rs"]
mod held_endpoint;

pub(crate) const HOST_MODE: &str = "claim-oracle-host";

const USAGE: &str = "
harness-executor-fixture claim-oracle [--workspace DIRECTORY] [--exe PROGRAM]
                                      [--prepare --cargo PROGRAM --resource-owner PROGRAM]
  Candidate-independent acceptance for exclusive executor dispatch claims.
  Drives the workspace or supplied codex-harness through real executor spawn
  and executor run against an owned file:// pool. No model request is made.
  Exit 0 only when every case passed; the JSON report names each case.
  --prepare, with absolute existing --cargo and --resource-owner programs,
  builds a source-only workspace first through the shared resource owner,
  then runs the behavior cases against the CLI it built. A missing target
  directory is compiled; it is not treated as an already-prepared success.
  No program is resolved through PATH, and --prepare cannot be combined
  with --exe.
";

const CASE_BUILD: &str = "built-executable";
const CASE_IDENTITY: &str = "cli-identity";
const CASE_DIFFERENT_OWNER: &str = "one-slot-different-owner";
const CASE_SAME_OWNER: &str = "one-slot-same-owner";
const CASE_TWO_SLOTS: &str = "two-slot-distinct";
const CASE_CREATOR_EXIT: &str = "creator-exit-reclaims-clean";
const CASE_FAILURE: &str = "startup-failure-keeps-cause";
const CASE_LIVE: &str = "live-host-other-slot";
const CASE_UNCERTAIN: &str = "uncertain-lease-preserved";
const CASE_UNREVIEWED: &str = "unreviewed-work-preserved";
const CASE_ACCEPTANCE: &str = "no-acceptance-without-host";

const BEHAVIOR: [&str; 9] = [
    CASE_DIFFERENT_OWNER,
    CASE_SAME_OWNER,
    CASE_TWO_SLOTS,
    CASE_CREATOR_EXIT,
    CASE_FAILURE,
    CASE_LIVE,
    CASE_UNCERTAIN,
    CASE_UNREVIEWED,
    CASE_ACCEPTANCE,
];

const STEP_BUILD: &str = "native-build";
const STEP_FORMAT: &str = "fmt-check";
const STEP_CLIPPY: &str = "clippy";
const STEP_POOL: &str = "pool-spawn";
const FORMAT_WATCHDOG: Duration = Duration::from_secs(600);
const COMPILED_WATCHDOG: Duration = Duration::from_secs(6000);
const OUTPUT_LIMIT: usize = 48 * 1024;
const EXCERPT: usize = 500;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) fn run(args: &[std::ffi::OsString]) -> io::Result<i32> {
    let mut workspace = None;
    let mut executable = None;
    let mut prepare = false;
    let mut cargo = None;
    let mut resource_owner = None;
    let mut index = 0;
    while index < args.len() {
        let key = args[index]
            .to_str()
            .ok_or_else(|| invalid("invalid claim-oracle option"))?;
        match key {
            "--help" => {
                println!("{USAGE}");
                return Ok(0);
            }
            "--prepare" => {
                if prepare {
                    return Err(invalid("duplicate claim-oracle --prepare"));
                }
                prepare = true;
                index += 1;
            }
            "--workspace" | "--exe" | "--cargo" | "--resource-owner" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid(&format!("claim-oracle {key} needs a path")))?;
                match key {
                    "--workspace" => workspace = Some(PathBuf::from(value)),
                    "--exe" => executable = Some(PathBuf::from(value)),
                    "--cargo" => {
                        if cargo.replace(PathBuf::from(value)).is_some() {
                            return Err(invalid("duplicate claim-oracle --cargo"));
                        }
                    }
                    _ => {
                        if resource_owner.replace(PathBuf::from(value)).is_some() {
                            return Err(invalid("duplicate claim-oracle --resource-owner"));
                        }
                    }
                }
                index += 2;
            }
            _ => return Err(invalid(&format!("invalid claim-oracle option: {key}"))),
        }
    }
    if prepare && executable.is_some() {
        return Err(invalid(
            "claim-oracle --prepare builds the workspace's own CLI and cannot be combined with --exe (an explicit control executable)",
        ));
    }
    let requested_tools = match (prepare, cargo, resource_owner) {
        (false, None, None) => None,
        (false, _, _) => {
            return Err(invalid(
                "claim-oracle --cargo and --resource-owner are only valid together with --prepare",
            ));
        }
        (true, None, _) => {
            return Err(invalid(
                "claim-oracle --prepare needs --cargo with the absolute path of an existing Cargo executable",
            ));
        }
        (true, _, None) => {
            return Err(invalid(
                "claim-oracle --prepare needs --resource-owner with the absolute path of the installed codex-harness executable that owns the shared heavy command slot",
            ));
        }
        (true, Some(cargo), Some(owner)) => Some((cargo, owner)),
    };
    let workspace = match workspace {
        Some(path) => path,
        None => env::current_dir()?,
    };
    let workspace = workspace.canonicalize().map_err(|error| {
        invalid(&format!(
            "claim-oracle workspace {} is not readable: {error}",
            workspace.display()
        ))
    })?;
    if !workspace.is_dir() || !workspace.join("crates").is_dir() {
        return Err(invalid(
            "claim-oracle --workspace must be the root of a codex-harness checkout",
        ));
    }
    let tools = match requested_tools {
        Some((cargo, owner)) => Some(preparation_tools(&workspace, cargo, owner)?),
        None => None,
    };
    let (resolved, mut cases, preparation) = match &tools {
        Some(tools) => {
            let preparation = prepare_workspace(&workspace, tools);
            if preparation.passed {
                let built = workspace.join("target").join("debug").join(binary_name());
                let (resolved, cases) = resolve_cases(&workspace, Some(&built));
                (resolved, cases, Some(preparation))
            } else {
                let cause = preparation.failure_cause();
                (None, skipped_cases(&cause), Some(preparation))
            }
        }
        None => {
            let (resolved, cases) = resolve_cases(&workspace, executable.as_deref());
            (resolved, cases, None)
        }
    };
    let executable = resolved
        .as_ref()
        .map(|item| item.path.display().to_string());
    let executable_sha256 = resolved.as_ref().map(|item| item.sha256.clone());
    if let Some(resolved) = resolved.as_ref() {
        let identity = identity_case(resolved);
        let identity_ok = identity.passed;
        let cause = identity.detail.clone();
        cases.push(identity);
        if identity_ok {
            cases.extend(behavior_cases(resolved));
        } else {
            for id in BEHAVIOR {
                cases.push(Case::fail(
                    id,
                    format!("skipped after identity failure: {cause}"),
                ));
            }
        }
    }
    let passed = cases.iter().all(Case::passed);
    let report = json!({
        "schema": 1,
        "kind": "executor-claim-oracle",
        "workspace": workspace.display().to_string(),
        "executable": executable,
        "executableSha256": executable_sha256,
        "preparation": preparation.as_ref().map(Preparation::json),
        "cases": cases.iter().map(Case::json).collect::<Vec<_>>(),
        "passed": passed,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(io::Error::other)?
    );
    Ok(if passed { 0 } else { 1 })
}

/// Forward one `git` invocation from the gated stand-in. `fetch` waits while
/// the gate directory contains `hold`, then the real git runs. A timeout is
/// a non-zero exit and does not call git.
pub(crate) fn forward_git() -> io::Result<i32> {
    let real = env::var("HARNESS_CLAIM_ORACLE_GIT_REAL")
        .map_err(|_| invalid("claim-oracle git stand-in has no real git"))?;
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().and_then(|arg| arg.to_str()) == Some("fetch")
        && let Ok(gate) = env::var("HARNESS_CLAIM_ORACLE_GIT_GATE")
    {
        record_fetch(Path::new(&gate))?;
        wait_for_release(Path::new(&gate))?;
    }
    let mut command = Command::new(&real);
    command.args(&args);
    if let Ok(cwd) = env::current_dir() {
        command.current_dir(cwd);
    }
    if let Some(path) = env::var_os("HARNESS_CLAIM_ORACLE_GIT_PATH") {
        command.env("PATH", path);
    }
    command.env_remove("HARNESS_CLAIM_ORACLE_GIT_REAL");
    command.env_remove("HARNESS_CLAIM_ORACLE_GIT_GATE");
    no_window(&mut command);
    let status = command.status()?;
    Ok(status.code().unwrap_or(1))
}

fn record_fetch(gate: &Path) -> io::Result<()> {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let body = json!({
        "pid": std::process::id(),
        "cwd": cwd.to_string_lossy(),
        "args": env::args().skip(1).collect::<Vec<_>>(),
    });
    fs::write(
        gate.join(format!("entered-{}.json", std::process::id())),
        serde_json::to_vec(&body).map_err(io::Error::other)?,
    )
}

fn wait_for_release(gate: &Path) -> io::Result<()> {
    let hold = gate.join("hold");
    // Longer than the checker's contender bound, so a bounded wait is released
    // before this stand-in can turn the first fetch into a timeout failure.
    let deadline = Instant::now() + Duration::from_secs(90);
    while hold.exists() {
        if Instant::now() >= deadline {
            return Err(invalid(
                "claim-oracle git gate timed out before release; fetch was not forwarded and this is not success",
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "codex-harness.exe"
    } else {
        "codex-harness"
    }
}

struct Resolved {
    path: PathBuf,
    sha256: String,
}

fn resolve_cases(workspace: &Path, explicit: Option<&Path>) -> (Option<Resolved>, Vec<Case>) {
    match resolve_executable(workspace, explicit) {
        Ok(resolved) => {
            let detail = format!(
                "built executable {} sha256={}",
                resolved.path.display(),
                resolved.sha256
            );
            let resolved_case = Case::pass(CASE_BUILD, detail);
            (Some(resolved), vec![resolved_case])
        }
        Err(cause) => (None, skipped_cases(&cause.to_string())),
    }
}

fn skipped_cases(cause: &str) -> Vec<Case> {
    let mut cases = vec![Case::fail(
        CASE_BUILD,
        format!("executable was not accepted: {cause}"),
    )];
    cases.push(Case::fail(
        CASE_IDENTITY,
        format!("skipped after executable failure: {cause}"),
    ));
    for id in BEHAVIOR {
        cases.push(Case::fail(
            id,
            format!("skipped after executable failure: {cause}"),
        ));
    }
    cases
}

fn resolve_executable(workspace: &Path, explicit: Option<&Path>) -> io::Result<Resolved> {
    let path = match explicit {
        Some(path) => path.to_path_buf(),
        None => workspace.join("target").join("debug").join(binary_name()),
    };
    if !path.is_file() {
        return Err(invalid(&format!(
            "claim-oracle executable {} does not exist; a clean target is not a passing prebuilt result",
            path.display()
        )));
    }
    let path = path.canonicalize().map_err(|error| {
        invalid(&format!(
            "claim-oracle executable {} is not readable: {error}",
            path.display()
        ))
    })?;
    build_identity::ordinary(&path).map_err(|error| {
        invalid(&format!(
            "claim-oracle executable {} is not an ordinary file: {error}",
            path.display()
        ))
    })?;
    let sha256 = build_identity::hash_file(&path).map_err(|error| {
        invalid(&format!(
            "claim-oracle executable {} could not be hashed: {error}",
            path.display()
        ))
    })?;
    Ok(Resolved { path, sha256 })
}

struct PreparationTools {
    cargo: PathBuf,
    cargo_sha256: String,
    owner: PathBuf,
    owner_sha256: String,
}

fn preparation_tools(
    workspace: &Path,
    cargo: PathBuf,
    owner: PathBuf,
) -> io::Result<PreparationTools> {
    let cargo_sha256 = preparation_program(workspace, &cargo, "--cargo")?;
    let owner_sha256 = preparation_program(workspace, &owner, "--resource-owner")?;
    if cargo == owner {
        return Err(invalid(
            "claim-oracle --cargo and --resource-owner must name different programs",
        ));
    }
    Ok(PreparationTools {
        cargo,
        cargo_sha256,
        owner,
        owner_sha256,
    })
}

fn preparation_program(workspace: &Path, path: &Path, option: &str) -> io::Result<String> {
    if !path.is_absolute() {
        return Err(invalid(&format!(
            "claim-oracle {option} must be an absolute path to an existing program: {}",
            path.display()
        )));
    }
    let path = path.canonicalize().map_err(|error| {
        invalid(&format!(
            "claim-oracle {option} {} is not an existing program: {error}",
            path.display()
        ))
    })?;
    build_identity::ordinary(&path).map_err(|error| {
        invalid(&format!(
            "claim-oracle {option} {} is not an ordinary file: {error}",
            path.display()
        ))
    })?;
    if path.starts_with(workspace) {
        return Err(invalid(&format!(
            "claim-oracle {option} {} is inside the workspace; preparation tools are pinned outside the candidate's write scope",
            path.display()
        )));
    }
    build_identity::hash_file(&path).map_err(|error| {
        invalid(&format!(
            "claim-oracle {option} {} is unreadable: {error}",
            path.display()
        ))
    })
}

struct PreparationStep {
    id: &'static str,
    argv: Vec<String>,
    exit_code: Option<i32>,
    timed_out: bool,
    seconds: f64,
    deadline_seconds: u64,
    error: Option<String>,
    output: String,
}

impl PreparationStep {
    fn passed(&self) -> bool {
        self.error.is_none() && !self.timed_out && self.exit_code == Some(0)
    }

    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "command": self.argv,
            "exitCode": self.exit_code,
            "timedOut": self.timed_out,
            "seconds": self.seconds,
            "deadlineSeconds": self.deadline_seconds,
            "error": self.error,
            "output": self.output,
        })
    }
}

struct Preparation {
    cargo: PathBuf,
    cargo_sha256: String,
    owner: PathBuf,
    owner_sha256: String,
    target_dir: PathBuf,
    steps: Vec<PreparationStep>,
    passed: bool,
}

impl Preparation {
    fn failure_cause(&self) -> String {
        let Some(step) = self.steps.iter().find(|step| !step.passed()) else {
            return "preparation did not complete".to_owned();
        };
        match (&step.error, step.exit_code, step.timed_out) {
            (Some(error), _, _) => {
                format!("preparation step '{}' could not run: {error}", step.id)
            }
            (None, Some(code), true) => format!(
                "preparation step '{}' was ended after its {}s bound (exit {code}); timeout is not success",
                step.id, step.deadline_seconds
            ),
            (None, Some(code), false) => {
                format!("preparation step '{}' exited {code}", step.id)
            }
            (None, None, _) => format!(
                "preparation step '{}' ended without an exit status",
                step.id
            ),
        }
    }

    fn json(&self) -> Value {
        json!({
            "schema": 1,
            "mode": "native-cargo",
            "executedBy": "checker",
            "cargo": {"path": self.cargo.to_string_lossy(), "sha256": self.cargo_sha256},
            "resourceOwner": {"path": self.owner.to_string_lossy(), "sha256": self.owner_sha256},
            "environment": {"CARGO_TARGET_DIR": self.target_dir.to_string_lossy()},
            "steps": self.steps.iter().map(PreparationStep::json).collect::<Vec<_>>(),
            "passed": self.passed,
        })
    }
}

fn prepare_workspace(workspace: &Path, tools: &PreparationTools) -> Preparation {
    let target_dir = workspace.join("target");
    let mut steps = Vec::new();
    let mut passed = true;
    for (id, argv, watchdog) in preparation_steps(tools) {
        let step = run_preparation_step(workspace, &target_dir, id, argv, watchdog);
        let ok = step.passed();
        steps.push(step);
        if !ok {
            passed = false;
            break;
        }
    }
    Preparation {
        cargo: tools.cargo.clone(),
        cargo_sha256: tools.cargo_sha256.clone(),
        owner: tools.owner.clone(),
        owner_sha256: tools.owner_sha256.clone(),
        target_dir,
        steps,
        passed,
    }
}

fn preparation_steps(
    tools: &PreparationTools,
) -> Vec<(&'static str, Vec<std::ffi::OsString>, Duration)> {
    let cargo = &tools.cargo;
    let owner = &tools.owner;
    vec![
        (
            STEP_BUILD,
            heavy(
                owner,
                cargo,
                &[
                    "build",
                    "--locked",
                    "-p",
                    "codex-harness",
                    "--bin",
                    "codex-harness",
                    "--jobs",
                    "1",
                ],
            ),
            COMPILED_WATCHDOG,
        ),
        (
            STEP_FORMAT,
            argv(cargo, &["fmt", "-p", "codex-harness", "--", "--check"]),
            FORMAT_WATCHDOG,
        ),
        (
            STEP_CLIPPY,
            heavy(
                owner,
                cargo,
                &[
                    "clippy",
                    "--locked",
                    "-p",
                    "codex-harness",
                    "--all-targets",
                    "--jobs",
                    "1",
                    "--",
                    "-D",
                    "warnings",
                ],
            ),
            COMPILED_WATCHDOG,
        ),
        (
            STEP_POOL,
            heavy(
                owner,
                cargo,
                &[
                    "test",
                    "--locked",
                    "-p",
                    "codex-harness",
                    "--test",
                    "executor_spawn",
                    "--jobs",
                    "1",
                    "--",
                    "pooled_spawn_derives_and_synchronizes_a_slot_before_launch",
                    "--exact",
                    "--test-threads=1",
                ],
            ),
            COMPILED_WATCHDOG,
        ),
    ]
}

fn argv(program: &Path, arguments: &[&str]) -> Vec<std::ffi::OsString> {
    std::iter::once(program.as_os_str().to_owned())
        .chain(arguments.iter().map(|part| std::ffi::OsString::from(*part)))
        .collect()
}

fn heavy(owner: &Path, cargo: &Path, command: &[&str]) -> Vec<std::ffi::OsString> {
    let mut arguments = vec![
        owner.as_os_str().to_owned(),
        std::ffi::OsString::from("heavy"),
        std::ffi::OsString::from("--"),
        cargo.as_os_str().to_owned(),
    ];
    arguments.extend(command.iter().map(|part| std::ffi::OsString::from(*part)));
    arguments
}

fn run_preparation_step(
    workspace: &Path,
    target_dir: &Path,
    id: &'static str,
    argv: Vec<std::ffi::OsString>,
    watchdog: Duration,
) -> PreparationStep {
    let command: Vec<String> = argv
        .iter()
        .map(|part| part.to_string_lossy().into_owned())
        .collect();
    let deadline_seconds = watchdog.as_secs();
    let started = Instant::now();
    let Some((program, arguments)) = argv.split_first() else {
        return PreparationStep {
            id,
            argv: command,
            exit_code: None,
            timed_out: false,
            seconds: 0.0,
            deadline_seconds,
            error: Some("the step has no program".to_owned()),
            output: String::new(),
        };
    };
    let child = Command::new(program)
        .args(arguments)
        .current_dir(workspace)
        .env("CARGO_TARGET_DIR", target_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            return PreparationStep {
                id,
                argv: command,
                exit_code: None,
                timed_out: false,
                seconds: started.elapsed().as_secs_f64(),
                deadline_seconds,
                error: Some(error.to_string()),
                output: String::new(),
            };
        }
    };
    let mut timed_out = false;
    let mut kill_error = None;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() >= watchdog => {
                timed_out = true;
                let identity = observe_identity(child.id());
                if let Err(error) = kill_tree(child.id()) {
                    kill_error = Some(invalid(&combine_failure(
                        &format!("preparation step exceeded the {watchdog:?} watchdog"),
                        &recovery_identity(&identity, &error.to_string()),
                    )));
                }
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                return PreparationStep {
                    id,
                    argv: command,
                    exit_code: None,
                    timed_out,
                    seconds: started.elapsed().as_secs_f64(),
                    deadline_seconds,
                    error: Some(recovery(child.id(), &error.to_string())),
                    output: String::new(),
                };
            }
        }
    }
    if let Err(error) = ensure_reaped(&mut child, kill_error.as_ref()) {
        return PreparationStep {
            id,
            argv: command,
            exit_code: None,
            timed_out,
            seconds: started.elapsed().as_secs_f64(),
            deadline_seconds,
            error: Some(error.to_string()),
            output: String::new(),
        };
    }
    let output = child.wait_with_output();
    match output {
        Ok(output) => PreparationStep {
            id,
            argv: command,
            exit_code: output.status.code(),
            timed_out,
            seconds: started.elapsed().as_secs_f64(),
            deadline_seconds,
            error: None,
            output: tail(&format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )),
        },
        Err(error) => PreparationStep {
            id,
            argv: command,
            exit_code: None,
            timed_out,
            seconds: started.elapsed().as_secs_f64(),
            deadline_seconds,
            error: Some(error.to_string()),
            output: String::new(),
        },
    }
}

struct Case {
    id: &'static str,
    passed: bool,
    detail: String,
}

impl Case {
    fn pass(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            passed: true,
            detail: detail.into(),
        }
    }
    fn fail(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            passed: false,
            detail: detail.into(),
        }
    }
    fn passed(&self) -> bool {
        self.passed
    }
    fn json(&self) -> Value {
        json!({"id": self.id, "passed": self.passed, "detail": self.detail})
    }
}

fn identity_case(resolved: &Resolved) -> Case {
    let mut command = Command::new(&resolved.path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut command);
    clear_session(&mut command);
    let started = Instant::now();
    let Ok(mut child) = command.spawn() else {
        return Case::fail(CASE_IDENTITY, "could not start the candidate --version");
    };
    let watchdog = Duration::from_secs(20);
    let mut timed_out = false;
    let mut kill_error = None;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() >= watchdog => {
                timed_out = true;
                let identity = observe_identity(child.id());
                if let Err(error) = kill_tree(child.id()) {
                    kill_error = Some(invalid(&combine_failure(
                        "candidate --version exceeded its watchdog",
                        &recovery_identity(&identity, &error.to_string()),
                    )));
                }
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                return Case::fail(CASE_IDENTITY, recovery(child.id(), &error.to_string()));
            }
        }
    }
    if let Err(error) = ensure_reaped(&mut child, kill_error.as_ref()) {
        return Case::fail(
            CASE_IDENTITY,
            format!("candidate --version observation failed: {error}"),
        );
    }
    let Ok(output) = child.wait_with_output() else {
        return Case::fail(CASE_IDENTITY, "candidate --version produced no exit status");
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if timed_out {
        return Case::fail(
            CASE_IDENTITY,
            "candidate --version timed out; timeout is not success",
        );
    }
    if output.status.code() != Some(0) || !text.contains("codex-harness ") {
        return Case::fail(
            CASE_IDENTITY,
            format!(
                "candidate is not the codex-harness CLI (exit {:?}): {}",
                output.status.code(),
                excerpt(&text)
            ),
        );
    }
    Case::pass(
        CASE_IDENTITY,
        format!("codex-harness identity exit 0: {}", excerpt(&text)),
    )
}

fn behavior_cases(resolved: &Resolved) -> Vec<Case> {
    let Ok(git) = discover_git(&resolved.path) else {
        return BEHAVIOR
            .into_iter()
            .map(|id| Case::fail(id, "real git.exe was not found; the claim gate cannot run"))
            .collect();
    };
    vec![
        overlap_case(
            &resolved.path,
            &git,
            Overlap {
                id: CASE_DIFFERENT_OWNER,
                pool: 1,
                owner_b: "exec-claim-b",
                other_slot: false,
            },
        ),
        overlap_case(
            &resolved.path,
            &git,
            Overlap {
                id: CASE_SAME_OWNER,
                pool: 1,
                owner_b: "exec-claim-a",
                other_slot: false,
            },
        ),
        overlap_case(
            &resolved.path,
            &git,
            Overlap {
                id: CASE_TWO_SLOTS,
                pool: 2,
                owner_b: "exec-claim-b",
                other_slot: true,
            },
        ),
        creator_exit_case(&resolved.path, &git),
        failure_case(&resolved.path, &git),
        live_host_case(&resolved.path, &git),
        uncertain_case(&resolved.path, &git),
        unreviewed_case(&resolved.path, &git),
        acceptance_case(&resolved.path, &git),
    ]
}

struct Overlap {
    id: &'static str,
    pool: u32,
    owner_b: &'static str,
    other_slot: bool,
}

const HANDOFF_BOUND: Duration = Duration::from_secs(90);
const EXCLUSIVITY_BOUND: Duration = Duration::from_secs(3);
const TERMINAL_BOUND: Duration = Duration::from_secs(45);

#[derive(Clone)]
struct HostHold {
    session: String,
    release: PathBuf,
    /// Owned record of the app-server requests this host actually received.
    turn_record: PathBuf,
}

impl HostHold {
    fn new(lab: &Lab, role: &str, owner: &str) -> Self {
        Self {
            session: format!("claim-oracle-{role}-{owner}"),
            release: lab.root.join(format!("release-{role}")),
            turn_record: lab.root.join(format!("turn-{role}.json")),
        }
    }
}

const CLEANUP_BOUND: Duration = Duration::from_secs(5);
const TURN_RECORD_ENV: &str = "HARNESS_CLAIM_ORACLE_TURN_RECORD";
const ARTIFACT_RETAINED: &str = "owned artifacts retained at ";

#[derive(Clone, Copy)]
struct CaptureExpect {
    stdout: bool,
    stderr: bool,
}

const PIPED_CAPTURE: CaptureExpect = CaptureExpect {
    stdout: true,
    stderr: true,
};
const NULL_CAPTURE: CaptureExpect = CaptureExpect {
    stdout: false,
    stderr: false,
};

struct PublishedTurn {
    file: std::fs::File,
}

impl PublishedTurn {
    fn create(path: &Path, bytes: &[u8]) -> io::Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use std::os::windows::io::{FromRawHandle, OwnedHandle};
            use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
            use windows_sys::Win32::Storage::FileSystem::{
                CREATE_ALWAYS, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ,
            };
            const GENERIC_READ: u32 = 0x8000_0000;
            const GENERIC_WRITE: u32 = 0x4000_0000;
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            wide.push(0);
            // SAFETY: `wide` is a null-terminated path. A non-invalid handle is owned here
            // and transferred to `OwnedHandle`, which closes it. Share-read keeps the
            // checker able to read while a same-user control cannot replace the bytes.
            let handle = unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ,
                    std::ptr::null(),
                    CREATE_ALWAYS,
                    FILE_ATTRIBUTE_NORMAL,
                    std::ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let mut file = unsafe { std::fs::File::from(OwnedHandle::from_raw_handle(handle)) };
            use std::io::Write;
            file.write_all(bytes)?;
            file.flush()?;
            Ok(Self { file })
        }
        #[cfg(not(windows))]
        {
            fs::write(path, bytes)?;
            Ok(Self {
                file: fs::File::options().write(true).open(path)?,
            })
        }
    }

    fn replace(&mut self, bytes: &[u8]) -> io::Result<()> {
        use std::io::{Seek, SeekFrom, Write};
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(bytes)?;
        self.file.set_len(bytes.len() as u64)?;
        self.file.flush()
    }
}

fn record_observed_turns(
    path: &Path,
    session: &str,
    server: &held_endpoint::Server,
    published: &mut Option<PublishedTurn>,
) -> io::Result<()> {
    let record = json!({
        "session": session,
        "turnStarts": server.requests_for("turn/start"),
        "threadStarts": server.requests_for("thread/start"),
        "threadResumes": server.requests_for("thread/resume"),
    });
    let bytes = serde_json::to_vec(&record).map_err(io::Error::other)?;
    match published {
        Some(existing) => existing.replace(&bytes)?,
        None => *published = Some(PublishedTurn::create(path, &bytes)?),
    }
    Ok(())
}

fn input_contains_assignment(input: &Value, assignment: &str) -> bool {
    if assignment.is_empty() {
        return false;
    }
    match input {
        Value::String(text) => text.contains(assignment),
        Value::Array(items) => items.iter().any(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| !text.is_empty() && text.contains(assignment))
                || item
                    .as_str()
                    .is_some_and(|text| !text.is_empty() && text.contains(assignment))
        }),
        _ => false,
    }
}

/// `Ok(false)` means the independent server has not published a turn yet.
/// A parsed record that does not carry the expected assignment is a failure,
/// not a missing observation. The received payload is not copied into the result.
fn verify_delivered_turn(hold: &HostHold, assignment: &str) -> Result<bool, String> {
    if !hold.turn_record.is_file() {
        return Ok(false);
    }
    let Ok(bytes) = fs::read(&hold.turn_record) else {
        return Ok(false);
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(false);
    };
    if value["session"].as_str() != Some(hold.session.as_str()) {
        return Err(format!(
            "the independent app-server served a different session than {}",
            hold.session
        ));
    }
    let Some(turns) = value["turnStarts"].as_array() else {
        return Err("the independent app-server turn record has no turn/start payload".to_owned());
    };
    if turns.is_empty() {
        return Ok(false);
    }
    for turn in turns {
        if turn["params"]["threadId"].as_str() != Some(hold.session.as_str()) {
            return Err(format!(
                "turn/start addressed a different thread than session {}",
                hold.session
            ));
        }
        if !input_contains_assignment(&turn["params"]["input"], assignment) {
            return Err(
                "the independent app-server turn/start did not carry the expected assignment; a receipt is not delivery"
                    .to_owned(),
            );
        }
    }
    Ok(true)
}

fn recovery(pid: u32, cause: &str) -> String {
    recovery_identity(&observe_identity(pid), cause)
}

struct OwnedIdentity {
    pid: u32,
    creation_time: Option<u64>,
}

fn observe_identity(pid: u32) -> OwnedIdentity {
    OwnedIdentity {
        pid,
        creation_time: creation_time_of(pid),
    }
}

fn identity_text(identity: &OwnedIdentity) -> String {
    match identity.creation_time {
        Some(created) => format!("pid={} creation_time={created}", identity.pid),
        None => format!("pid={} creation_time=unverified", identity.pid),
    }
}

fn recovery_identity(identity: &OwnedIdentity, cause: &str) -> String {
    format!(
        "owned process cleanup is unresolved ({cause}); observed identity {}; do not terminate by pid alone because that pid may already belong to a different process; verify creation_time before any manual action",
        identity_text(identity)
    )
}

fn recovery_retained(identity: &OwnedIdentity, artifacts: &Path, cause: &str) -> String {
    format!(
        "{}; {ARTIFACT_RETAINED}{}",
        recovery_identity(identity, cause),
        artifacts.display()
    )
}

fn combine_failure(original: &str, cleanup: &str) -> String {
    format!("original error: {original}; cleanup error: {cleanup}")
}

fn creation_time_of(pid: u32) -> Option<u64> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
        use windows_sys::Win32::System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut created = FILETIME::default();
            let mut exited = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            let ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
            let _ = CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

fn unverified(label: &str, joined: &Joined) -> Vec<String> {
    let mut problems = Vec::new();
    if joined.timed_out {
        problems.push(format!(
            "{label} observation timed out; timeout is not a verified result"
        ));
    }
    if joined.truncated {
        problems.push(format!(
            "{label} output exceeded the retained bound despite exit {:?}; truncation is not success",
            joined.code
        ));
    }
    if joined.code.is_none() {
        problems.push(format!("{label} produced no verified exit"));
    }
    if let Some(error) = &joined.cleanup_error {
        problems.push(format!("{label}: {error}"));
    }
    problems
}

fn stopped_defects(stopped: &[Stopped]) -> Vec<String> {
    let mut problems = Vec::new();
    for item in stopped {
        problems.extend(unverified("owned process", &item.joined));
        if let Some(error) = &item.cleanup_error {
            problems.push(error.clone());
        }
    }
    problems
}

fn acceptance(passed: bool, detail: String, stopped: &[Stopped]) -> (bool, String) {
    if !passed {
        return (false, detail);
    }
    let defects = stopped_defects(stopped);
    if defects.is_empty() {
        (true, detail)
    } else {
        (false, defects.join("; "))
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

struct LiveHandoff {
    session: String,
    run: String,
    slot: u32,
    owner: String,
}

struct Stopped {
    joined: Joined,
    cleanup_error: Option<String>,
}

pub(crate) fn serve_held_app_server(args: &[std::ffi::OsString]) -> io::Result<i32> {
    let listen = held_option(args, "--listen")
        .ok_or_else(|| invalid("claim-oracle host requires --listen ws://127.0.0.1:PORT"))?;
    let port = listen
        .strip_prefix("ws://127.0.0.1:")
        .and_then(|text| text.parse::<u16>().ok())
        .ok_or_else(|| invalid(&format!("claim-oracle host cannot serve {listen}")))?;
    let token_file = held_option(args, "--ws-token-file")
        .ok_or_else(|| invalid("claim-oracle host requires --ws-token-file"))?;
    let session = env::var("HARNESS_IMPROVEMENT_FIXTURE_SESSION")
        .unwrap_or_else(|_| "claim-oracle-session".to_owned());
    let model = held_profile_model();
    let release = env::var("HARNESS_CLAIM_ORACLE_HOST_RELEASE")
        .ok()
        .map(PathBuf::from);
    let record_path = env::var(TURN_RECORD_ENV).ok().map(PathBuf::from);
    let turn = "claim-oracle-turn";
    let start_session = session.clone();
    let resume_session = session.clone();
    let read_session = session.clone();
    let start_model = model.clone();
    let resume_model = model;
    let server = held_endpoint::Server::start_on_with(
        port,
        held_endpoint::Bearer::File(PathBuf::from(token_file)),
        |server| {
            server.answer("initialize", held_endpoint::Answer::Result(json!({})));
            server.answer(
                "thread/start",
                held_endpoint::Answer::Result(json!({
                    "thread": {
                        "id": start_session,
                        "cwd": env::current_dir().ok(),
                        "turns": []
                    },
                    "model": start_model,
                })),
            );
            server.answer(
                "thread/resume",
                held_endpoint::Answer::Result(json!({
                    "thread": {
                        "id": resume_session,
                        "cwd": env::current_dir().ok(),
                        "turns": []
                    },
                    "model": resume_model,
                })),
            );
            server.answer("thread/name/set", held_endpoint::Answer::Result(json!({})));
            server.answer(
                "turn/start",
                held_endpoint::Answer::Result(json!({
                    "turn": {"id": turn, "status": "inProgress"}
                })),
            );
            server.answer(
                "thread/read",
                held_endpoint::Answer::Result(json!({
                    "thread": {
                        "id": read_session,
                        "turns": [{
                            "id": turn,
                            "status": "completed",
                            "items": [{
                                "id": "message-1",
                                "type": "agentMessage",
                                "text": "claim-oracle host accepted the assignment"
                            }]
                        }]
                    }
                })),
            );
        },
    );
    let until = Instant::now() + Duration::from_secs(180);
    let mut announced = false;
    let mut completed = false;
    let mut recorded = 0_usize;
    let mut published = None;
    while Instant::now() < until {
        let turns = server.requests_for("turn/start");
        if let Some(path) = &record_path
            && turns.len() != recorded
            && record_observed_turns(path, &session, &server, &mut published).is_ok()
        {
            recorded = turns.len();
        }
        // Announce only after the actual request is on disk. The constant
        // message lets a control finish a receipt; it is not proof of delivery.
        if !announced && recorded > 0 {
            server.push(json!({
                "method": "turn/started",
                "params": {
                    "threadId": session,
                    "turn": {"id": turn, "status": "inProgress"}
                }
            }));
            server.push(json!({
                "method": "item/completed",
                "params": {
                    "threadId": session,
                    "item": {
                        "id": "message-1",
                        "type": "agentMessage",
                        "text": "claim-oracle host accepted the assignment"
                    }
                }
            }));
            announced = true;
        }
        if announced && !completed && release.as_ref().is_some_and(|path| path.is_file()) {
            server.push(json!({
                "method": "turn/completed",
                "params": {
                    "threadId": session,
                    "turn": {"id": turn, "status": "completed"}
                }
            }));
            completed = true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(0)
}

fn held_option(args: &[std::ffi::OsString], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.to_str())
        .map(str::to_owned)
}

fn held_profile_model() -> String {
    let Some(home) = env::var_os("CODEX_HOME") else {
        return "deepseek-flash".to_owned();
    };
    let text = fs::read_to_string(PathBuf::from(home).join("ds.config.toml")).unwrap_or_default();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("model") else {
            continue;
        };
        let rest = rest.trim().trim_start_matches('=').trim();
        let rest = rest.trim_matches(['\'', '"']);
        if !rest.is_empty() && !rest.contains(' ') {
            return rest.to_owned();
        }
    }
    "deepseek-flash".to_owned()
}

fn install_model_free_launcher(lab: &Lab) -> io::Result<PathBuf> {
    let destination = lab.home.join("harness").join("bin").join("codex.exe");
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let source = env::current_exe()?;
    if destination.is_file() {
        let _ = fs::remove_file(&destination);
    }
    if fs::hard_link(&source, &destination).is_err() {
        fs::copy(&source, &destination)?;
    }
    Ok(destination)
}

fn spawn_owned(
    exe: &Path,
    lab: &Lab,
    gate: Option<&Path>,
    owner: &str,
    assignment: &str,
    host: Option<&HostHold>,
) -> io::Result<Child> {
    let mut command = Command::new(exe);
    command.args([
        "executor",
        "spawn",
        "--source",
        lab.source
            .to_str()
            .ok_or_else(|| invalid("source path is not unicode"))?,
        "--codex-home",
        lab.home
            .to_str()
            .ok_or_else(|| invalid("home path is not unicode"))?,
        "--profile",
        "ds",
        "--owner",
        owner,
        "--exec",
        assignment,
    ]);
    prepare_candidate(&mut command, lab);
    if let Some(gate) = gate {
        command.env("HARNESS_CLAIM_ORACLE_GIT_REAL", &lab.git);
        command.env("HARNESS_CLAIM_ORACLE_GIT_GATE", gate);
        command.env(
            "HARNESS_CLAIM_ORACLE_GIT_PATH",
            env::var_os("PATH").unwrap_or_default(),
        );
        let mut path = std::ffi::OsString::from(gate);
        path.push(if cfg!(windows) { ";" } else { ":" });
        path.push(env::var_os("PATH").unwrap_or_default());
        command.env("PATH", path);
    }
    if let Some(host) = host {
        command.env("HARNESS_EXECUTOR_FIXTURE_MODE", HOST_MODE);
        command.env("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE", HOST_MODE);
        command.env("HARNESS_IMPROVEMENT_FIXTURE_SESSION", &host.session);
        command.env("HARNESS_CLAIM_ORACLE_HOST_RELEASE", &host.release);
        command.env(TURN_RECORD_ENV, &host.turn_record);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut command);
    let mut child = command.spawn()?;
    let pid = child.id();
    pump(
        child.stdout.take(),
        lab.root.join(format!("out-{pid}.stdout")),
    );
    pump(
        child.stderr.take(),
        lab.root.join(format!("out-{pid}.stderr")),
    );
    Ok(child)
}

fn observation_failure(lab: &Lab, child: &Child, error: &io::Error) -> String {
    let identity = observe_identity(child.id());
    let original = error.to_string();
    match kill_tree(child.id()) {
        Ok(()) => combine_failure(
            &original,
            "termination helper finished after observation failed",
        ),
        Err(kill) => combine_failure(
            &original,
            &recovery_retained(&identity, &lab.root, &kill.to_string()),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn wait_live_handoff(
    lab: &Lab,
    child: &mut Child,
    exe: &Path,
    hold: &HostHold,
    slot: u32,
    owner: &str,
    assignment: &str,
    launcher: &Path,
    bound: Duration,
) -> Result<LiveHandoff, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(format!(
                    "dispatch exited {:?} before a live host accepted the assignment: {}",
                    status.code(),
                    excerpt(&captured(&lab.root, child.id()))
                ));
            }
            Ok(None) => {}
            Err(error) => return Err(observation_failure(lab, child, &error)),
        }
        match live_handoff(
            lab,
            child.id(),
            exe,
            hold,
            slot,
            owner,
            assignment,
            launcher,
        ) {
            Ok(Some(handoff)) => return Ok(handoff),
            Ok(None) => {}
            Err(error) => return Err(error),
        }
        if started.elapsed() >= bound {
            let why = match verify_delivered_turn(hold, assignment) {
                Ok(true) => "handoff identity was not live".to_owned(),
                Ok(false) => {
                    "the independent app-server recorded no verified turn/start".to_owned()
                }
                Err(error) => error,
            };
            return Err(format!(
                "timed out waiting for slot {slot} handoff of {owner}: {why}: {}",
                excerpt(&captured(&lab.root, child.id()))
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[allow(clippy::too_many_arguments)]
fn live_handoff(
    lab: &Lab,
    spawn_pid: u32,
    exe: &Path,
    hold: &HostHold,
    slot: u32,
    owner: &str,
    assignment: &str,
    launcher: &Path,
) -> Result<Option<LiveHandoff>, String> {
    let Some(receipt) = read_receipt(lab, slot) else {
        return Ok(None);
    };
    if let Some(recorded) = receipt["slot"]["owner"].as_str()
        && recorded != owner
    {
        return Err(format!("slot {slot} is bound to {recorded}, not {owner}"));
    }
    let Some(recorded_assignment) = receipt["control"]["assignment"].as_str() else {
        return Ok(None);
    };
    if !recorded_assignment.contains(assignment) {
        return Err(format!(
            "slot {slot} receipt assignment does not contain {assignment}"
        ));
    }
    if let Some(session) = receipt["observation"]["session"].as_str()
        && session != hold.session
    {
        return Err(format!(
            "slot {slot} host session was {session}, not {}",
            hold.session
        ));
    }
    let Some(lease) = read_lease_record(lab, slot) else {
        return Ok(None);
    };
    if lease["owner"].as_str() != Some(owner) || lease["index"].as_u64() != Some(u64::from(slot)) {
        return Err(format!(
            "slot {slot} lease does not belong to {owner}: {}",
            excerpt(&lease.to_string())
        ));
    }
    let Some(lease_pid) = lease["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
    else {
        return Ok(None);
    };
    // The dispatcher records a lease, then the hosted `executor run` replaces it
    // with its own process. A matching spawn pid is not yet the host.
    if lease_pid == spawn_pid || !process_is_live(&lease, exe) {
        return Ok(None);
    }
    let launcher_recorded = receipt["launcher"].as_str().unwrap_or_default();
    if !program_matches(Path::new(launcher_recorded), launcher) {
        return Err(format!(
            "slot {slot} launcher was {launcher_recorded}, not the owned model-free launcher {}",
            launcher.display()
        ));
    }
    if receipt["observation"]["state"].as_str() != Some("running") {
        return Ok(None);
    }
    if receipt["observation"]["session"].as_str() != Some(hold.session.as_str()) {
        return Ok(None);
    }
    let Some(run) = receipt["originatingLead"]["runGeneration"]
        .as_str()
        .filter(|run| !run.is_empty())
    else {
        return Ok(None);
    };
    let host = &receipt["observation"]["host"];
    let Some(host_pid) = host["pid"].as_u64().and_then(|pid| u32::try_from(pid).ok()) else {
        return Ok(None);
    };
    if host_pid != lease_pid || !process_is_live(host, exe) {
        return Ok(None);
    }
    if !program_matches(Path::new(host["program"].as_str().unwrap_or_default()), exe) {
        return Err(format!(
            "slot {slot} host image is not the candidate: {}",
            host["program"]
        ));
    }
    match verify_delivered_turn(hold, assignment) {
        Ok(true) => {}
        Ok(false) => return Ok(None),
        Err(error) => return Err(error),
    }
    Ok(Some(LiveHandoff {
        session: hold.session.clone(),
        run: run.to_owned(),
        slot,
        owner: owner.to_owned(),
    }))
}

fn process_is_live(record: &Value, fallback: &Path) -> bool {
    let Some(pid) = record["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
    else {
        return false;
    };
    let Some(created) = record["created"].as_u64() else {
        return false;
    };
    let program = record["program"]
        .as_str()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .unwrap_or_else(|| fallback.to_path_buf());
    let Ok(user) = process_service::current_user() else {
        return false;
    };
    matches!(
        ServiceProcess::inspect(
            ProcessIdentity {
                pid,
                creation_time: created,
            },
            &program,
            &user,
        ),
        Ok(Some(_))
    )
}

fn program_matches(recorded: &Path, expected: &Path) -> bool {
    if recorded.as_os_str().is_empty() {
        return false;
    }
    if same_path(recorded, expected) {
        return true;
    }
    match (recorded.canonicalize(), expected.canonicalize()) {
        (Ok(left), Ok(right)) => same_path(&left, &right),
        _ => false,
    }
}

fn read_receipt(lab: &Lab, slot: u32) -> Option<Value> {
    let path = pool_state(&lab.home)?.join(format!("spawn-{slot}.json"));
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn read_lease_record(lab: &Lab, slot: u32) -> Option<Value> {
    let path = pool_state(&lab.home)?.join(format!("lease-{slot}.json"));
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn release_host(hold: &HostHold) -> io::Result<()> {
    fs::write(&hold.release, b"release\n")
}

fn stop_now(lab: &Lab, child: Child) -> Stopped {
    stop_child(lab, child, PIPED_CAPTURE)
}

fn stop_null_streams(lab: &Lab, child: Child) -> Stopped {
    stop_child(lab, child, NULL_CAPTURE)
}

fn stop_child(lab: &Lab, child: Child, expect: CaptureExpect) -> Stopped {
    let pid = child.id();
    let identity = observe_identity(pid);
    let kill = kill_tree(pid);
    let mut joined = join_child(&lab.root, child, Duration::from_secs(15), expect);
    if let Err(error) = kill {
        let message = combine_failure(
            "stop was requested for an owned child",
            &recovery_retained(&identity, &lab.root, &error.to_string()),
        );
        joined.cleanup_error = Some(match joined.cleanup_error {
            Some(existing) => format!("{existing}; {message}"),
            None => message,
        });
    }
    Stopped {
        cleanup_error: None,
        joined,
    }
}

fn finish_held(lab: &Lab, hold: &HostHold, child: Child) -> Stopped {
    let release = release_host(hold);
    let joined = join_child(&lab.root, child, TERMINAL_BOUND, PIPED_CAPTURE);
    let cleanup_error = match release {
        Ok(()) => None,
        Err(error) => Some(format!(
            "host release for session {} failed: {error}; recovery: retry the owned release for that session",
            hold.session
        )),
    };
    Stopped {
        joined,
        cleanup_error,
    }
}

fn note_cleanup(detail: &str, stopped: &[Stopped]) -> String {
    let mut detail = detail.to_owned();
    for item in stopped {
        if let Some(error) = &item.cleanup_error {
            detail.push_str("; ");
            detail.push_str(error);
        }
        if let Some(error) = &item.joined.cleanup_error {
            detail.push_str("; ");
            detail.push_str(error);
        }
        if item.joined.timed_out {
            detail.push_str("; terminal observation timed out; timeout is not a verified result");
        }
        if item.joined.truncated {
            detail.push_str("; terminal output was truncated; truncation is not success");
        }
    }
    detail
}

fn handoff_line(handoff: &LiveHandoff) -> String {
    format!(
        "slot {} owner {} session {} run {} app-server consumed the expected assignment",
        handoff.slot, handoff.owner, handoff.session, handoff.run
    )
}

fn overlap_case(exe: &Path, git: &Path, spec: Overlap) -> Case {
    let Ok(lab) = Lab::create(&format!("overlap-{}", spec.id), spec.pool, git) else {
        return Case::fail(spec.id, "could not create the owned pool");
    };
    let gate = lab.root.join("gate");
    if let Err(error) = install_gate(&gate) {
        return finish(
            spec.id,
            false,
            format!("git gate was not installed: {error}"),
            &lab,
        );
    }
    let Ok(launcher) = install_model_free_launcher(&lab) else {
        return finish(
            spec.id,
            false,
            "model-free launcher was not installed",
            &lab,
        );
    };
    let hold_a = HostHold::new(&lab, "a", "exec-claim-a");
    let assignment_a = format!("claim-oracle:{}:exec-claim-a", spec.id);
    let mut started = match spawn_owned(
        exe,
        &lab,
        Some(&gate),
        "exec-claim-a",
        &assignment_a,
        Some(&hold_a),
    ) {
        Ok(child) => child,
        Err(error) => {
            return finish(
                spec.id,
                false,
                format!("first dispatch did not start: {error}"),
                &lab,
            );
        }
    };
    if let Err(error) = wait_fetch(&gate, Duration::from_secs(25)) {
        let stopped = stop_now(&lab, started);
        return finish(
            spec.id,
            false,
            note_cleanup(
                &format!("first dispatch did not reach the post-claim fetch gate: {error}"),
                &[stopped],
            ),
            &lab,
        );
    }
    let hold_b = HostHold::new(&lab, "b", spec.owner_b);
    let assignment_b = format!("claim-oracle:{}:{}", spec.id, spec.owner_b);
    let mut contender = match spawn_owned(
        exe,
        &lab,
        Some(&gate),
        spec.owner_b,
        &assignment_b,
        Some(&hold_b),
    ) {
        Ok(child) => child,
        Err(error) => {
            let stopped = stop_now(&lab, started);
            return finish(
                spec.id,
                false,
                note_cleanup(&format!("contender did not start: {error}"), &[stopped]),
                &lab,
            );
        }
    };
    let exclusive = exclusive_during_gate(&lab, &mut contender, &spec);
    if !exclusive.is_empty() {
        let stopped_b = stop_now(&lab, contender);
        let stopped_a = stop_now(&lab, started);
        return finish(
            spec.id,
            false,
            note_cleanup(&exclusive.join("; "), &[stopped_a, stopped_b]),
            &lab,
        );
    }
    if let Err(error) = fs::remove_file(gate.join("hold")) {
        let stopped_b = stop_now(&lab, contender);
        let stopped_a = stop_now(&lab, started);
        return finish(
            spec.id,
            false,
            note_cleanup(
                &format!("git gate was not released: {error}"),
                &[stopped_a, stopped_b],
            ),
            &lab,
        );
    }
    let handoff_a = match wait_live_handoff(
        &lab,
        &mut started,
        exe,
        &hold_a,
        1,
        "exec-claim-a",
        &assignment_a,
        &launcher,
        HANDOFF_BOUND,
    ) {
        Ok(handoff) => handoff,
        Err(error) => {
            let stopped_b = stop_now(&lab, contender);
            let stopped_a = stop_now(&lab, started);
            return finish(
                spec.id,
                false,
                note_cleanup(&error, &[stopped_a, stopped_b]),
                &lab,
            );
        }
    };
    if spec.other_slot {
        finish_two_slot(
            spec.id,
            &lab,
            exe,
            &launcher,
            started,
            contender,
            &hold_a,
            &hold_b,
            &assignment_b,
            spec.owner_b,
            &handoff_a,
        )
    } else {
        finish_one_slot(spec.id, &lab, started, contender, &hold_a, &handoff_a)
    }
}

fn exclusive_during_gate(lab: &Lab, contender: &mut Child, spec: &Overlap) -> Vec<String> {
    let started = Instant::now();
    while started.elapsed() < EXCLUSIVITY_BOUND {
        if contender.try_wait().ok().flatten().is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let during = read_slots(&lab.home);
    let fetches = fetch_entries(&lab.root.join("gate"));
    let slot_a = lab.slot(1);
    let fetches_a = fetches
        .iter()
        .filter(|entry| same_path(&entry.cwd, &slot_a))
        .count();
    let owner = slot_owner(&during, 1);
    let mut problems = Vec::new();
    if owner.as_deref() != Some("exec-claim-a") {
        problems.push(format!(
            "contender replaced the in-flight claim: owner is now {owner:?}"
        ));
    }
    if fetches_a != 1 {
        problems.push(format!(
            "slot 1 was entered by {fetches_a} post-claim fetches; one startup generation may fetch it"
        ));
    }
    if !spec.other_slot && (slot_owner(&during, 2).is_some() || lab.slot(2).exists()) {
        problems.push("one-slot pool created another executor tree".to_owned());
    }
    if spec.other_slot
        && let Some(owner_b) = slot_owner(&during, 2)
        && owner_b != spec.owner_b
    {
        problems.push(format!(
            "slot 2 was bound to {owner_b}, not {}",
            spec.owner_b
        ));
    }
    let text = captured(&lab.root, contender.id());
    if text.contains("executor dispatch accepted") {
        problems.push("contender published acceptance before a host existed".to_owned());
    }
    problems
}

fn finish_one_slot(
    id: &'static str,
    lab: &Lab,
    started: Child,
    contender: Child,
    hold_a: &HostHold,
    handoff_a: &LiveHandoff,
) -> Case {
    let mut contender = contender;
    let until = Instant::now() + Duration::from_secs(40);
    while Instant::now() < until && contender.try_wait().ok().flatten().is_none() {
        thread::sleep(Duration::from_millis(50));
    }
    let contender_running = contender.try_wait().ok().flatten().is_none();
    let stopped_b = if contender_running {
        stop_now(lab, contender)
    } else {
        Stopped {
            joined: join_child(&lab.root, contender, Duration::from_secs(5), PIPED_CAPTURE),
            cleanup_error: None,
        }
    };
    let mut problems = Vec::new();
    if contender_running {
        problems.push(
            "one-slot contender did not reach a terminal refusal while the first host was live"
                .to_owned(),
        );
    }
    if stopped_b.joined.code == Some(0)
        || stopped_b.joined.text.contains("executor dispatch accepted")
    {
        problems.push(format!(
            "one-slot contender reported acceptance without its own host: {}",
            excerpt(&stopped_b.joined.text)
        ));
    }
    if stopped_b.joined.timed_out || stopped_b.joined.truncated {
        problems.push(
            "contender observation was truncated or timed out; that is not success".to_owned(),
        );
    }
    if slot_owner(&read_slots(&lab.home), 1).as_deref() != Some("exec-claim-a") {
        problems.push(format!(
            "first host lost slot 1 to {:?}",
            slot_owner(&read_slots(&lab.home), 1)
        ));
    }
    if lab.slot(2).exists() {
        problems.push("one-slot pool created another executor tree".to_owned());
    }
    let stopped_a = finish_held(lab, hold_a, started);
    if stopped_a.joined.code.is_some_and(|code| code != 0) {
        problems.push(format!(
            "first host terminal outcome was exit {:?}: {}",
            stopped_a.joined.code,
            excerpt(&stopped_a.joined.text)
        ));
    }
    problems.extend(unverified("first host", &stopped_a.joined));
    problems.extend(unverified("contender", &stopped_b.joined));
    if let Some(error) = &stopped_a.cleanup_error {
        problems.push(error.clone());
    }
    if let Some(error) = &stopped_b.cleanup_error {
        problems.push(error.clone());
    }
    let passed = problems.is_empty();
    let detail = if passed {
        format!(
            "exclusive claim survived the gate; {} handed off and the contender refused without a second conversation",
            handoff_line(handoff_a)
        )
    } else {
        problems.join("; ")
    };
    let (passed, detail) = acceptance(passed, detail, &[stopped_a, stopped_b]);
    finish(id, passed, detail, lab)
}

#[allow(clippy::too_many_arguments)]
fn finish_two_slot(
    id: &'static str,
    lab: &Lab,
    exe: &Path,
    launcher: &Path,
    started: Child,
    mut contender: Child,
    hold_a: &HostHold,
    hold_b: &HostHold,
    assignment_b: &str,
    owner_b: &str,
    handoff_a: &LiveHandoff,
) -> Case {
    let mut problems = Vec::new();
    let contender_exited = contender.try_wait().ok().flatten().is_some();
    let (handoff_b, second_child, second_hold) = if contender_exited {
        let joined = join_child(&lab.root, contender, Duration::from_secs(5), PIPED_CAPTURE);
        problems.extend(unverified("pre-handoff contender", &joined));
        if joined.code == Some(0) || joined.text.contains("executor dispatch accepted") {
            problems.push(format!(
                "contender exited before handoff but reported acceptance: {}",
                excerpt(&joined.text)
            ));
        }
        let hold_c = HostHold::new(lab, "c", owner_b);
        match spawn_owned(exe, lab, None, owner_b, assignment_b, Some(&hold_c)) {
            Ok(mut child) => {
                let handoff = wait_live_handoff(
                    lab,
                    &mut child,
                    exe,
                    &hold_c,
                    2,
                    owner_b,
                    assignment_b,
                    launcher,
                    HANDOFF_BOUND,
                );
                (handoff, Some(child), hold_c)
            }
            Err(error) => {
                problems.push(format!(
                    "post-handoff dispatch did not start after the contender refused: {error}"
                ));
                (Err("no second host".to_owned()), None, hold_c)
            }
        }
    } else {
        let handoff = wait_live_handoff(
            lab,
            &mut contender,
            exe,
            hold_b,
            2,
            owner_b,
            assignment_b,
            launcher,
            HANDOFF_BOUND,
        );
        (handoff, Some(contender), hold_b.clone())
    };
    let both_live = handoff_b.as_ref().ok().is_some_and(|handoff_b| {
        read_lease_record(lab, 1).is_some_and(|lease| process_is_live(&lease, exe))
            && read_lease_record(lab, 2).is_some_and(|lease| process_is_live(&lease, exe))
            && handoff_b.session != handoff_a.session
            && handoff_b.run != handoff_a.run
            && handoff_b.slot == 2
    });
    if !both_live {
        problems.push(
            "distinct live conversations were not concurrent after the first handoff".to_owned(),
        );
    }
    if let Err(error) = &handoff_b {
        problems.push(error.clone());
    }
    let stopped_a = finish_held(lab, hold_a, started);
    let stopped_b = match second_child {
        Some(child) => finish_held(lab, &second_hold, child),
        None => Stopped {
            joined: Joined {
                code: None,
                text: "second host did not start".to_owned(),
                timed_out: false,
                truncated: false,
                cleanup_error: Some("second host did not start".to_owned()),
            },
            cleanup_error: None,
        },
    };
    if stopped_a.joined.code.is_some_and(|code| code != 0) {
        problems.push(format!(
            "first host terminal outcome was exit {:?}: {}",
            stopped_a.joined.code,
            excerpt(&stopped_a.joined.text)
        ));
    }
    if stopped_b.joined.code.is_some_and(|code| code != 0) {
        problems.push(format!(
            "second host terminal outcome was exit {:?}: {}",
            stopped_b.joined.code,
            excerpt(&stopped_b.joined.text)
        ));
    }
    problems.extend(unverified("first host", &stopped_a.joined));
    problems.extend(unverified("second host", &stopped_b.joined));
    if let Some(error) = &stopped_a.cleanup_error {
        problems.push(error.clone());
    }
    if let Some(error) = &stopped_b.cleanup_error {
        problems.push(error.clone());
    }
    let passed = problems.is_empty();
    let detail = if passed {
        format!(
            "startup wait was allowed; {} and {} stayed live together after handoff",
            handoff_line(handoff_a),
            handoff_b
                .as_ref()
                .ok()
                .map(handoff_line)
                .unwrap_or_else(|| "missing second handoff".to_owned())
        )
    } else {
        problems.join("; ")
    };
    let (passed, detail) = acceptance(passed, detail, &[stopped_a, stopped_b]);
    finish(id, passed, detail, lab)
}

fn creator_exit_case(exe: &Path, git: &Path) -> Case {
    let Ok(lab) = Lab::create("creator-exit", 1, git) else {
        return Case::fail(CASE_CREATOR_EXIT, "could not create the owned pool");
    };
    let gate = lab.root.join("gate");
    if let Err(error) = install_gate(&gate) {
        return finish(
            CASE_CREATOR_EXIT,
            false,
            format!("git gate was not installed: {error}"),
            &lab,
        );
    }
    let started = match spawn_dispatch(
        exe,
        &lab,
        &gate,
        true,
        "exec-claim-a",
        "claim-oracle:creator-exit:a",
    ) {
        Ok(child) => child,
        Err(error) => {
            return finish(
                CASE_CREATOR_EXIT,
                false,
                format!("creator did not start: {error}"),
                &lab,
            );
        }
    };
    if let Err(error) = wait_fetch(&gate, Duration::from_secs(25)) {
        let stopped = stop_now(&lab, started);
        return finish(
            CASE_CREATOR_EXIT,
            false,
            note_cleanup(
                &format!("creator did not reach the fetch gate: {error}"),
                &[stopped],
            ),
            &lab,
        );
    }
    let pid = started.id();
    let identity = observe_identity(pid);
    let kill = kill_tree(pid);
    let joined = join_child(&lab.root, started, CLEANUP_BOUND, PIPED_CAPTURE);
    let _ = fs::remove_file(gate.join("hold"));
    if kill.is_err() || joined.cleanup_error.is_some() || joined.timed_out || joined.code.is_none()
    {
        let mut cleanup = match &kill {
            Err(error) => recovery_retained(&identity, &lab.root, &error.to_string()),
            Ok(()) => recovery_retained(&identity, &lab.root, "creator tree could not be stopped"),
        };
        if let Some(error) = &joined.cleanup_error {
            cleanup = format!("{cleanup}; {error}");
        }
        let cause = combine_failure(
            "creator termination was requested after the fetch gate",
            &cleanup,
        );
        return finish(
            CASE_CREATOR_EXIT,
            false,
            format!("creator tree could not be stopped; cleanup failure is not success: {cause}"),
            &lab,
        );
    }
    let code = joined.code;
    if code == Some(0) {
        return finish(
            CASE_CREATOR_EXIT,
            false,
            "creator exit was rewritten as success",
            &lab,
        );
    }
    let next = match spawn_dispatch(
        exe,
        &lab,
        &gate,
        false,
        "exec-claim-b",
        "claim-oracle:creator-exit:b",
    ) {
        Ok(child) => child,
        Err(error) => {
            return finish(
                CASE_CREATOR_EXIT,
                false,
                format!("recovery dispatch did not start: {error}"),
                &lab,
            );
        }
    };
    let joined = join_child(&lab.root, next, Duration::from_secs(40), PIPED_CAPTURE);
    let owner = slot_owner(&read_slots(&lab.home), 1);
    let mut problems = Vec::new();
    problems.extend(unverified("recovery", &joined));
    if joined.code == Some(0) || joined.text.contains("executor dispatch accepted") {
        problems.push(format!(
            "recovery reported success ({:?}) without a host: {}",
            joined.code,
            excerpt(&joined.text)
        ));
    }
    if owner.as_deref() != Some("exec-claim-b") {
        problems.push(format!(
            "a confirmed-dead creator's clean reservation was not recoverable; owner is {owner:?}"
        ));
    }
    if !joined.text.contains("installed Codex launcher is missing") {
        problems.push(format!(
            "recovery did not preserve the original launcher failure: {}",
            excerpt(&joined.text)
        ));
    }
    let passed = problems.is_empty();
    let detail = if passed {
        format!(
            "creator exit {code:?} stayed a failure; clean reservation rebound to exec-claim-b with the original launcher error"
        )
    } else {
        problems.join("; ")
    };
    finish(CASE_CREATOR_EXIT, passed, detail, &lab)
}

fn failure_case(exe: &Path, git: &Path) -> Case {
    let Ok(lab) = Lab::create("failure", 1, git) else {
        return Case::fail(CASE_FAILURE, "could not create the owned pool");
    };
    let joined = match run_spawn(
        exe,
        &lab,
        "exec-claim-a",
        "claim-oracle:failure:a",
        Duration::from_secs(40),
    ) {
        Ok(joined) => joined,
        Err(error) => return finish(CASE_FAILURE, false, error, &lab),
    };
    let mut problems = Vec::new();
    problems.extend(unverified("startup", &joined));
    if joined.code == Some(0) {
        problems.push("missing launcher was reported as exit 0".to_owned());
    }
    if !joined.text.contains("installed Codex launcher is missing") {
        problems.push(format!(
            "original launcher error was not preserved: {}",
            excerpt(&joined.text)
        ));
    }
    if joined.text.contains("executor dispatch accepted") {
        problems.push("startup failure printed dispatch acceptance".to_owned());
    }
    let passed = problems.is_empty();
    let detail = if passed {
        format!(
            "exit {:?} preserved the launcher failure and did not become acceptance",
            joined.code
        )
    } else {
        problems.join("; ")
    };
    finish(CASE_FAILURE, passed, detail, &lab)
}

fn acceptance_case(exe: &Path, git: &Path) -> Case {
    let Ok(lab) = Lab::create("acceptance", 1, git) else {
        return Case::fail(CASE_ACCEPTANCE, "could not create the owned pool");
    };
    let joined = match run_spawn(
        exe,
        &lab,
        "exec-claim-a",
        "claim-oracle:acceptance:exact-assignment",
        Duration::from_secs(40),
    ) {
        Ok(joined) => joined,
        Err(error) => return finish(CASE_ACCEPTANCE, false, error, &lab),
    };
    let receipts = read_receipts(&lab.home);
    let accepted = joined.text.contains("executor dispatch accepted")
        || receipts.iter().any(receipt_claims_acceptance);
    let host_live = lease_is_live(&lab.home, exe, "exec-claim-a");
    let mut problems = Vec::new();
    problems.extend(unverified("acceptance observation", &joined));
    if accepted && !host_live {
        problems.push(
            "a receipt or dispatch line reported acceptance although no live host has this assignment"
                .to_owned(),
        );
    }
    if host_live
        && !receipts.iter().any(|receipt| {
            receipt["control"]["assignment"]
                .as_str()
                .is_some_and(|text| text.contains("claim-oracle:acceptance:exact-assignment"))
        })
    {
        problems.push("live host lease does not correspond to the spawned assignment".to_owned());
    }
    let passed = problems.is_empty();
    let detail = if passed {
        "no acceptance was reported for an assignment that never reached a host".to_owned()
    } else {
        problems.join("; ")
    };
    finish(CASE_ACCEPTANCE, passed, detail, &lab)
}

fn unreviewed_case(exe: &Path, git: &Path) -> Case {
    let Ok(lab) = Lab::create("unreviewed", 1, git) else {
        return Case::fail(CASE_UNREVIEWED, "could not create the owned pool");
    };
    let first = match run_spawn(
        exe,
        &lab,
        "exec-claim-a",
        "claim-oracle:unreviewed:a",
        Duration::from_secs(40),
    ) {
        Ok(joined) => joined,
        Err(error) => return finish(CASE_UNREVIEWED, false, error, &lab),
    };
    if first.code == Some(0) || !unverified("setup", &first).is_empty() {
        return finish(
            CASE_UNREVIEWED,
            false,
            format!(
                "setup dispatch was not a preserved failure: {}",
                excerpt(&first.text)
            ),
            &lab,
        );
    }
    let marker = lab.slot(1).join("UNREVIEWED_MARKER.txt");
    if let Err(error) = fs::write(&marker, "claim-oracle unreviewed work\n") {
        return finish(
            CASE_UNREVIEWED,
            false,
            format!("could not plant unreviewed work: {error}"),
            &lab,
        );
    }
    let second = match run_spawn(
        exe,
        &lab,
        "exec-claim-b",
        "claim-oracle:unreviewed:b",
        Duration::from_secs(40),
    ) {
        Ok(joined) => joined,
        Err(error) => return finish(CASE_UNREVIEWED, false, error, &lab),
    };
    let kept = fs::read_to_string(&marker).ok();
    let owner = slot_owner(&read_slots(&lab.home), 1);
    let mut problems = Vec::new();
    problems.extend(unverified("setup", &first));
    problems.extend(unverified("contender", &second));
    if kept.as_deref() != Some("claim-oracle unreviewed work\n") {
        problems.push("unreviewed work was deleted or rewritten".to_owned());
    }
    if owner.as_deref() == Some("exec-claim-b") {
        problems.push("contender took the unreviewed slot".to_owned());
    }
    if second.code == Some(0) {
        problems.push("contender reported success while unreviewed work remained".to_owned());
    }
    let passed = problems.is_empty();
    let detail = if passed {
        "unreviewed marker survived and the slot was not given to the contender".to_owned()
    } else {
        format!(
            "{}; contender: {}",
            problems.join("; "),
            excerpt(&second.text)
        )
    };
    finish(CASE_UNREVIEWED, passed, detail, &lab)
}

fn uncertain_case(exe: &Path, git: &Path) -> Case {
    let Ok(lab) = Lab::create("uncertain", 1, git) else {
        return Case::fail(CASE_UNCERTAIN, "could not create the owned pool");
    };
    let first = match run_spawn(
        exe,
        &lab,
        "exec-claim-a",
        "claim-oracle:uncertain:a",
        Duration::from_secs(40),
    ) {
        Ok(joined) => joined,
        Err(error) => return finish(CASE_UNCERTAIN, false, error, &lab),
    };
    if first.code == Some(0) || !unverified("setup", &first).is_empty() {
        return finish(
            CASE_UNCERTAIN,
            false,
            "setup dispatch did not fail closed",
            &lab,
        );
    }
    let sleeper = match spawn_sleeper() {
        Ok(child) => child,
        Err(error) => {
            return finish(
                CASE_UNCERTAIN,
                false,
                format!("sleeper did not start: {error}"),
                &lab,
            );
        }
    };
    let planted = plant_uncertain_lease(&lab, exe, sleeper.id());
    let second = match run_spawn(
        exe,
        &lab,
        "exec-claim-b",
        "claim-oracle:uncertain:b",
        Duration::from_secs(40),
    ) {
        Ok(joined) => joined,
        Err(error) => {
            let stopped = stop_null_streams(&lab, sleeper);
            return finish(
                CASE_UNCERTAIN,
                false,
                note_cleanup(&error, &[stopped]),
                &lab,
            );
        }
    };
    let owner = slot_owner(&read_slots(&lab.home), 1);
    let stopped_sleeper = stop_null_streams(&lab, sleeper);
    let mut problems = Vec::new();
    if let Err(error) = planted {
        problems.push(format!("uncertain lease was not planted: {error}"));
    }
    problems.extend(unverified("contender", &second));
    if owner.as_deref() != Some("exec-claim-a") {
        problems.push(format!(
            "uncertain identity was reclaimed; owner is {owner:?}"
        ));
    }
    if second.code == Some(0) {
        problems.push("contender reported success against an uncertain lease".to_owned());
    }
    problems.extend(unverified("sleeper", &stopped_sleeper.joined));
    if let Some(error) = &stopped_sleeper.cleanup_error {
        problems.push(error.clone());
    }
    let passed = problems.is_empty();
    let detail = if passed {
        "a live pid with a mismatched image was preserved and not reclaimed".to_owned()
    } else {
        problems.join("; ")
    };
    finish(CASE_UNCERTAIN, passed, detail, &lab)
}

fn live_host_case(exe: &Path, git: &Path) -> Case {
    let Ok(lab) = Lab::create("live-host", 2, git) else {
        return Case::fail(CASE_LIVE, "could not create the owned pool");
    };
    let Ok(launcher) = install_model_free_launcher(&lab) else {
        return finish(
            CASE_LIVE,
            false,
            "model-free launcher was not installed",
            &lab,
        );
    };
    let hold_a = HostHold::new(&lab, "a", "exec-live-a");
    let assignment_a = "claim-oracle:live:a";
    let mut first = match spawn_owned(exe, &lab, None, "exec-live-a", assignment_a, Some(&hold_a)) {
        Ok(child) => child,
        Err(error) => {
            return finish(
                CASE_LIVE,
                false,
                format!("first dispatch did not start: {error}"),
                &lab,
            );
        }
    };
    let handoff_a = match wait_live_handoff(
        &lab,
        &mut first,
        exe,
        &hold_a,
        1,
        "exec-live-a",
        assignment_a,
        &launcher,
        HANDOFF_BOUND,
    ) {
        Ok(handoff) => handoff,
        Err(error) => {
            let stopped = stop_now(&lab, first);
            return finish(CASE_LIVE, false, note_cleanup(&error, &[stopped]), &lab);
        }
    };
    let hold_b = HostHold::new(&lab, "b", "exec-live-b");
    let assignment_b = "claim-oracle:live:b";
    let mut second = match spawn_owned(exe, &lab, None, "exec-live-b", assignment_b, Some(&hold_b))
    {
        Ok(child) => child,
        Err(error) => {
            let stopped = stop_now(&lab, first);
            return finish(
                CASE_LIVE,
                false,
                note_cleanup(
                    &format!("second dispatch did not start: {error}"),
                    &[stopped],
                ),
                &lab,
            );
        }
    };
    let handoff_b = wait_live_handoff(
        &lab,
        &mut second,
        exe,
        &hold_b,
        2,
        "exec-live-b",
        assignment_b,
        &launcher,
        HANDOFF_BOUND,
    );
    let mut problems = Vec::new();
    let concurrent = handoff_b.as_ref().ok().is_some_and(|handoff_b| {
        read_lease_record(&lab, 1).is_some_and(|lease| process_is_live(&lease, exe))
            && read_lease_record(&lab, 2).is_some_and(|lease| process_is_live(&lease, exe))
            && handoff_b.run != handoff_a.run
            && handoff_b.session != handoff_a.session
            && slot_owner(&read_slots(&lab.home), 1).as_deref() == Some("exec-live-a")
    });
    if !concurrent {
        problems.push(
            "a live host did not leave another slot available for a concurrent conversation"
                .to_owned(),
        );
    }
    if let Err(error) = &handoff_b {
        problems.push(error.clone());
    }
    let third = spawn_owned(exe, &lab, None, "exec-live-c", "claim-oracle:live:c", None);
    let stopped_c = match third {
        Ok(child) => Some(stop_after_refusal(&lab, child)),
        Err(error) => {
            problems.push(format!("dispatch beyond the pool did not start: {error}"));
            None
        }
    };
    if let Some(stopped) = &stopped_c {
        if stopped.joined.code == Some(0) || lab.slot(3).exists() {
            problems
                .push("a dispatch beyond the pool reported success or created proj-wt3".to_owned());
        }
        problems.extend(unverified("dispatch beyond the pool", &stopped.joined));
        if let Some(error) = &stopped.cleanup_error {
            problems.push(error.clone());
        }
    }
    let stopped_a = finish_held(&lab, &hold_a, first);
    let stopped_b = finish_held(&lab, &hold_b, second);
    if stopped_a.joined.code.is_some_and(|code| code != 0) {
        problems.push(format!(
            "first host terminal outcome was exit {:?}: {}",
            stopped_a.joined.code,
            excerpt(&stopped_a.joined.text)
        ));
    }
    if stopped_b.joined.code.is_some_and(|code| code != 0) {
        problems.push(format!(
            "second host terminal outcome was exit {:?}: {}",
            stopped_b.joined.code,
            excerpt(&stopped_b.joined.text)
        ));
    }
    problems.extend(unverified("first host", &stopped_a.joined));
    problems.extend(unverified("second host", &stopped_b.joined));
    if let Some(error) = &stopped_a.cleanup_error {
        problems.push(error.clone());
    }
    if let Some(error) = &stopped_b.cleanup_error {
        problems.push(error.clone());
    }
    let mut stopped = vec![stopped_a, stopped_b];
    if let Some(stopped_c) = stopped_c {
        stopped.push(stopped_c);
    }
    let passed = problems.is_empty();
    let detail = if passed {
        format!(
            "native handoff {} stayed live while {} bound the other slot; no third tree was created",
            handoff_line(&handoff_a),
            handoff_b
                .as_ref()
                .ok()
                .map(handoff_line)
                .unwrap_or_else(|| "missing second handoff".to_owned())
        )
    } else {
        problems.join("; ")
    };
    let (passed, detail) = acceptance(passed, detail, &stopped);
    finish(CASE_LIVE, passed, detail, &lab)
}

fn stop_after_refusal(lab: &Lab, child: Child) -> Stopped {
    let pid = child.id();
    let identity = observe_identity(pid);
    let kill_after = {
        let joined = join_child(&lab.root, child, Duration::from_secs(25), PIPED_CAPTURE);
        if joined.timed_out {
            let kill = kill_tree(pid);
            (joined, kill)
        } else {
            (joined, Ok(()))
        }
    };
    let (mut joined, kill) = kill_after;
    if let Err(error) = kill {
        let message = combine_failure(
            "refusal cleanup exceeded its observation bound",
            &recovery_retained(&identity, &lab.root, &error.to_string()),
        );
        joined.cleanup_error = Some(match joined.cleanup_error {
            Some(existing) => format!("{existing}; {message}"),
            None => message,
        });
    }
    Stopped {
        cleanup_error: None,
        joined,
    }
}

struct Joined {
    code: Option<i32>,
    text: String,
    timed_out: bool,
    truncated: bool,
    cleanup_error: Option<String>,
}

fn run_spawn(
    exe: &Path,
    lab: &Lab,
    owner: &str,
    assignment: &str,
    bound: Duration,
) -> Result<Joined, String> {
    let child = spawn_dispatch(exe, lab, &lab.root.join("gate"), false, owner, assignment)
        .map_err(|error| format!("{owner} did not start: {error}"))?;
    Ok(join_child(&lab.root, child, bound, PIPED_CAPTURE))
}

fn join_child(root: &Path, mut child: Child, bound: Duration, expect: CaptureExpect) -> Joined {
    let pid = child.id();
    let identity = observe_identity(pid);
    let started = Instant::now();
    let mut timed_out = false;
    let mut cleanup_error = None;
    let original_timeout = format!("child exceeded the {bound:?} observation bound");
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() >= bound => {
                timed_out = true;
                if let Err(error) = kill_tree(pid) {
                    cleanup_error = Some(combine_failure(
                        &original_timeout,
                        &recovery_retained(&identity, root, &error.to_string()),
                    ));
                }
                break;
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                cleanup_error = Some(recovery_retained(&identity, root, &error.to_string()));
                break;
            }
        }
    }
    if timed_out || cleanup_error.is_some() {
        let reap_started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if reap_started.elapsed() >= CLEANUP_BOUND => {
                    let message = recovery_retained(
                        &identity,
                        root,
                        "still running after the termination deadline",
                    );
                    cleanup_error = Some(match cleanup_error {
                        Some(existing) => format!("{existing}; {message}"),
                        None => combine_failure(&original_timeout, &message),
                    });
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(error) => {
                    let message = recovery_retained(&identity, root, &error.to_string());
                    cleanup_error = Some(match cleanup_error {
                        Some(existing) => format!("{existing}; {message}"),
                        None => message,
                    });
                    break;
                }
            }
        }
    }
    let status = match child.try_wait() {
        Ok(Some(_)) => match child.wait() {
            Ok(status) => Some(status),
            Err(error) => {
                let message = recovery_retained(&identity, root, &error.to_string());
                cleanup_error = Some(match cleanup_error {
                    Some(existing) => format!("{existing}; {message}"),
                    None => message,
                });
                None
            }
        },
        Ok(None) => None,
        Err(error) => {
            let message = recovery_retained(&identity, root, &error.to_string());
            cleanup_error = Some(match cleanup_error {
                Some(existing) => format!("{existing}; {message}"),
                None => message,
            });
            None
        }
    };
    let capture = if status.is_some() {
        read_capture(root, &identity, expect)
    } else {
        Capture {
            text: captured(root, pid),
            truncated: capture_truncated(root, pid),
            error: None,
        }
    };
    if let Some(error) = capture.error {
        cleanup_error = Some(match cleanup_error {
            Some(existing) => format!("{existing}; {error}"),
            None => error,
        });
    }
    Joined {
        code: status.as_ref().and_then(|status| status.code()),
        text: tail(&capture.text),
        timed_out,
        truncated: capture.truncated,
        cleanup_error,
    }
}

struct Capture {
    text: String,
    truncated: bool,
    error: Option<String>,
}

fn read_capture(root: &Path, identity: &OwnedIdentity, expect: CaptureExpect) -> Capture {
    if !expect.stdout && !expect.stderr {
        return Capture {
            text: String::new(),
            truncated: false,
            error: None,
        };
    }
    let stdout = root.join(format!("out-{}.stdout", identity.pid));
    let stderr = root.join(format!("out-{}.stderr", identity.pid));
    let started = Instant::now();
    loop {
        let stdout_done = !expect.stdout || sidecar(&stdout, ".done").is_file();
        let stderr_done = !expect.stderr || sidecar(&stderr, ".done").is_file();
        if stdout_done && stderr_done {
            break;
        }
        if started.elapsed() >= CLEANUP_BOUND {
            let observed = observe_error(&stdout).or_else(|| observe_error(&stderr));
            let cause = match observed {
                Some(error) => format!(
                    "required piped capture did not finish ({error}); missing capture is not success even after exit 0"
                ),
                None => "required piped capture did not finish; missing capture is not success even after exit 0"
                    .to_owned(),
            };
            return Capture {
                text: captured(root, identity.pid),
                truncated: capture_truncated(root, identity.pid),
                error: Some(recovery_retained(identity, root, &cause)),
            };
        }
        thread::sleep(Duration::from_millis(20));
    }
    Capture {
        text: captured(root, identity.pid),
        truncated: capture_truncated(root, identity.pid),
        error: observe_error(&stdout).or_else(|| observe_error(&stderr)),
    }
}

fn capture_truncated(root: &Path, pid: u32) -> bool {
    let stdout = root.join(format!("out-{pid}.stdout"));
    let stderr = root.join(format!("out-{pid}.stderr"));
    sidecar(&stdout, ".truncated").is_file() || sidecar(&stderr, ".truncated").is_file()
}

fn observe_error(path: &Path) -> Option<String> {
    let path = sidecar(path, ".observe-error");
    let text = fs::read_to_string(path).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(format!("output observation failed: {text}"))
    }
}

fn spawn_dispatch(
    exe: &Path,
    lab: &Lab,
    gate: &Path,
    hold: bool,
    owner: &str,
    assignment: &str,
) -> io::Result<Child> {
    let mut command = Command::new(exe);
    command.args([
        "executor",
        "spawn",
        "--source",
        lab.source
            .to_str()
            .ok_or_else(|| invalid("source path is not unicode"))?,
        "--codex-home",
        lab.home
            .to_str()
            .ok_or_else(|| invalid("home path is not unicode"))?,
        "--profile",
        "ds",
        "--owner",
        owner,
        "--exec",
        assignment,
    ]);
    prepare_candidate(&mut command, lab);
    if hold {
        command.env("HARNESS_CLAIM_ORACLE_GIT_REAL", &lab.git);
        command.env("HARNESS_CLAIM_ORACLE_GIT_GATE", gate);
        command.env(
            "HARNESS_CLAIM_ORACLE_GIT_PATH",
            env::var_os("PATH").unwrap_or_default(),
        );
        let mut path = std::ffi::OsString::from(gate);
        path.push(if cfg!(windows) { ";" } else { ":" });
        path.push(env::var_os("PATH").unwrap_or_default());
        command.env("PATH", path);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut command);
    let mut child = command.spawn()?;
    let pid = child.id();
    pump(
        child.stdout.take(),
        lab.root.join(format!("out-{pid}.stdout")),
    );
    pump(
        child.stderr.take(),
        lab.root.join(format!("out-{pid}.stderr")),
    );
    Ok(child)
}

fn pump<R>(source: Option<R>, destination: PathBuf)
where
    R: std::io::Read + Send + 'static,
{
    let Some(mut source) = source else {
        let _ = fs::write(sidecar(&destination, ".done"), b"complete");
        return;
    };
    thread::spawn(move || {
        let mut file = match fs::File::create(&destination) {
            Ok(file) => Some(file),
            Err(error) => {
                let _ = fs::write(sidecar(&destination, ".observe-error"), error.to_string());
                None
            }
        };
        let mut buffer = [0_u8; 4096];
        let mut written = 0_usize;
        let mut truncated = false;
        loop {
            let count = match std::io::Read::read(&mut source, &mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) => {
                    let _ = fs::write(sidecar(&destination, ".observe-error"), error.to_string());
                    break;
                }
            };
            if let Some(output) = file.as_mut() {
                if written < OUTPUT_LIMIT {
                    let end = count.min(OUTPUT_LIMIT - written);
                    if output.write_all(&buffer[..end]).is_err() {
                        let _ = fs::write(
                            sidecar(&destination, ".observe-error"),
                            "output write failed",
                        );
                    } else {
                        written += end;
                        let _ = output.flush();
                    }
                    if end < count {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            } else {
                truncated = true;
            }
            if truncated {
                let _ = fs::write(sidecar(&destination, ".truncated"), b"1");
            }
        }
        if let Some(output) = file.as_mut() {
            let _ = output.flush();
        }
        let _ = fs::write(
            sidecar(&destination, ".done"),
            if truncated { "truncated" } else { "complete" },
        );
    });
}

fn captured(root: &Path, pid: u32) -> String {
    let stdout = fs::read_to_string(root.join(format!("out-{pid}.stdout"))).unwrap_or_default();
    let stderr = fs::read_to_string(root.join(format!("out-{pid}.stderr"))).unwrap_or_default();
    format!("{stdout}{stderr}")
}

fn prepare_candidate(command: &mut Command, lab: &Lab) {
    clear_session(command);
    command.env("CODEX_THREAD_ID", "claim-oracle-lead");
    command.env("CODEX_HOME", &lab.home);
    command.env("CODEX_HARNESS_CPU_ACCOUNT", &lab.account);
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.current_dir(&lab.root);
}

fn clear_session(command: &mut Command) {
    for name in [
        "HARNESS_EXECUTOR_SESSION",
        "HARNESS_EXECUTOR_RUN",
        "HARNESS_ORIGINATING_LEAD",
        "HARNESS_LEAD_THREAD",
        "HARNESS_LEAD_RECIPIENT",
        "HARNESS_EXECUTOR_FIXTURE_MODE",
        "HARNESS_CLAIM_ORACLE_GIT_REAL",
        "HARNESS_CLAIM_ORACLE_GIT_GATE",
        "WT_SESSION",
        "CODEX_SESSION_ID",
        "CODEX_THREAD_ID",
    ] {
        command.env_remove(name);
    }
}

fn install_gate(gate: &Path) -> io::Result<()> {
    fs::create_dir_all(gate)?;
    let git_name = if cfg!(windows) { "git.exe" } else { "git" };
    let destination = gate.join(git_name);
    let source = env::current_exe()?;
    if fs::hard_link(&source, &destination).is_err() {
        fs::copy(&source, &destination)?;
    }
    fs::write(gate.join("hold"), b"hold\n")?;
    Ok(())
}

fn wait_fetch(gate: &Path, bound: Duration) -> io::Result<()> {
    let started = Instant::now();
    while started.elapsed() < bound {
        if !fetch_entries(gate).is_empty() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    Err(invalid("timed out waiting for the post-claim fetch gate"))
}

struct FetchEntry {
    cwd: PathBuf,
}

fn fetch_entries(gate: &Path) -> Vec<FetchEntry> {
    let Ok(entries) = fs::read_dir(gate) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("entered-"))
        .filter_map(|entry| {
            let value: Value = serde_json::from_slice(&fs::read(entry.path()).ok()?).ok()?;
            Some(FetchEntry {
                cwd: PathBuf::from(value["cwd"].as_str()?),
            })
        })
        .collect()
}

fn spawn_sleeper() -> io::Result<Child> {
    let mut command = Command::new(r"C:\Windows\System32\ping.exe");
    command.args(["-n", "60", "127.0.0.1"]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut command);
    command.spawn()
}

fn plant_uncertain_lease(lab: &Lab, exe: &Path, pid: u32) -> io::Result<()> {
    let user = process_service::current_user()?;
    let sleeper = PathBuf::from(r"C:\Windows\System32\ping.exe");
    let identity = ServiceProcess::observe(pid, &sleeper, 0, &user)?.identity();
    let state = pool_state(&lab.home).ok_or_else(|| invalid("pool state is missing"))?;
    let record = read_slots(&lab.home)
        .into_iter()
        .find(|record| record["index"] == 1)
        .ok_or_else(|| invalid("slot 1 record is missing"))?;
    let lease = json!({
        "schema": 1,
        "owner": "exec-claim-a",
        "index": 1,
        "path": record["path"].as_str().unwrap_or_default(),
        "pid": identity.pid,
        "created": identity.creation_time,
        "program": exe,
    });
    fs::write(
        state.join("lease-1.json"),
        serde_json::to_vec_pretty(&lease).map_err(io::Error::other)?,
    )
}

fn lease_is_live(home: &Path, exe: &Path, owner: &str) -> bool {
    let Some(state) = pool_state(home) else {
        return false;
    };
    let Ok(bytes) = fs::read(state.join("lease-1.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    if value["owner"] != owner {
        return false;
    }
    let Some(pid) = value["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
    else {
        return false;
    };
    let Some(created) = value["created"].as_u64() else {
        return false;
    };
    let Ok(user) = process_service::current_user() else {
        return false;
    };
    let program = value["program"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| exe.to_path_buf());
    matches!(
        ServiceProcess::inspect(
            ProcessIdentity {
                pid,
                creation_time: created
            },
            &program,
            &user
        ),
        Ok(Some(_))
    )
}

fn receipt_claims_acceptance(receipt: &Value) -> bool {
    let assignment = receipt["control"]["assignment"].as_str().is_some();
    let state = receipt["observation"]["state"].as_str().unwrap_or("");
    assignment
        && (state.contains("accept")
            || receipt["visible"] == true && receipt["control"].is_object())
}

fn read_receipts(home: &Path) -> Vec<Value> {
    let Some(state) = pool_state(home) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(state) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("spawn-"))
        .filter_map(|entry| serde_json::from_slice(&fs::read(entry.path()).ok()?).ok())
        .collect()
}

fn read_slots(home: &Path) -> Vec<Value> {
    let Some(state) = pool_state(home) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&state) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("slot-") && name.ends_with(".json")
        })
        .filter_map(|entry| serde_json::from_slice(&fs::read(entry.path()).ok()?).ok())
        .collect()
}

fn slot_owner(records: &[Value], index: u32) -> Option<String> {
    records.iter().find_map(|record| {
        (record["index"] == index)
            .then(|| record["owner"].as_str().map(str::to_owned))
            .flatten()
    })
}

fn pool_state(home: &Path) -> Option<PathBuf> {
    let root = home.join("harness").join("executor-pool");
    let entries = fs::read_dir(root).ok()?;
    entries
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
}

struct Lab {
    root: PathBuf,
    source: PathBuf,
    home: PathBuf,
    account: PathBuf,
    git: PathBuf,
}

impl Lab {
    fn create(name: &str, pool: u32, git: &Path) -> io::Result<Self> {
        let root = env::temp_dir().join(format!(
            "claim-oracle-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root)?;
        let bare = root.join("remote.git");
        git_ok(
            git,
            &root,
            &[
                "init",
                "--bare",
                "-q",
                "--initial-branch=main",
                bare.to_str().ok_or_else(|| invalid("path"))?,
            ],
        )?;
        let seed = root.join("seed");
        git_ok(
            git,
            &root,
            &[
                "init",
                "-q",
                "--initial-branch=main",
                seed.to_str().ok_or_else(|| invalid("path"))?,
            ],
        )?;
        git_ok(
            git,
            &seed,
            &["config", "user.email", "claim-oracle@example.test"],
        )?;
        git_ok(git, &seed, &["config", "user.name", "Claim Oracle"])?;
        fs::write(seed.join("README.md"), "seed\n")?;
        git_ok(git, &seed, &["add", "."])?;
        git_ok(git, &seed, &["commit", "-qm", "seed"])?;
        let url = file_url(&bare)?;
        git_ok(git, &seed, &["remote", "add", "origin", &url])?;
        git_ok(git, &seed, &["push", "-q", "origin", "main"])?;
        let source = root.join("proj");
        git_ok(git, &root, &["clone", "-q", &url, "proj"])?;
        git_ok(
            git,
            &source,
            &["config", "user.email", "claim-oracle@example.test"],
        )?;
        git_ok(git, &source, &["config", "user.name", "Claim Oracle"])?;
        fs::create_dir_all(source.join("global"))?;
        fs::write(
            source.join("global").join("orchestration.toml"),
            format!(
                "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"ds\"\nexecutor_profiles = [\"ds\"]\nmax_concurrent_executors = {pool}\nvote_threshold = 3\nincubator_size_cap = 32\nfeedback_batch_limit = 8\nworktree_limit = 1\n"
            ),
        )?;
        let home = root.join("home");
        fs::create_dir_all(&home)?;
        fs::write(home.join("ds.config.toml"), "model = 'deepseek-flash'\n")?;
        let account = root.join("cpu-account");
        fs::create_dir_all(&account)?;
        Ok(Self {
            root,
            source,
            home,
            account,
            git: git.to_path_buf(),
        })
    }

    fn slot(&self, index: u32) -> PathBuf {
        self.root.join(format!("proj-wt{index}"))
    }
}

fn finish(id: &'static str, passed: bool, detail: impl Into<String>, lab: &Lab) -> Case {
    let detail = detail.into();
    if detail.contains(ARTIFACT_RETAINED) {
        return Case::fail(
            id,
            format!("{detail}; unresolved cleanup kept the owned artifacts and is not success"),
        );
    }
    let mut cleanup = fs::remove_dir_all(&lab.root);
    for _ in 0..8 {
        if cleanup.is_ok() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
        cleanup = fs::remove_dir_all(&lab.root);
    }
    match cleanup {
        Ok(()) => {
            if passed {
                Case::pass(id, detail)
            } else {
                Case::fail(id, detail)
            }
        }
        Err(error) => Case::fail(
            id,
            format!("{detail}; cleanup failed: {error}; cleanup failure is not success"),
        ),
    }
}

fn git_ok(git: &Path, cwd: &Path, args: &[&str]) -> io::Result<()> {
    let output = Command::new(git)
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(invalid(&format!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn file_url(path: &Path) -> io::Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| invalid("path is not unicode"))?;
    Ok(format!("file:///{}", text.replace('\\', "/")))
}

fn discover_git(own: &Path) -> io::Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            candidates.push(directory.join(if cfg!(windows) { "git.exe" } else { "git" }));
        }
    }
    if let Some(root) = env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(root).join(r"Git\cmd\git.exe"));
    }
    candidates.push(PathBuf::from(r"C:\Program Files\Git\cmd\git.exe"));
    candidates
        .into_iter()
        .find(|path| path.is_file() && path != own)
        .ok_or_else(|| invalid("real git.exe was not found"))
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.to_string_lossy().replace('/', "\\");
    let right = right.to_string_lossy().replace('/', "\\");
    left.eq_ignore_ascii_case(&right)
}

fn ensure_reaped(child: &mut Child, kill_error: Option<&io::Error>) -> io::Result<()> {
    let pid = child.id();
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return match kill_error {
                    Some(error) => Err(invalid(&format!(
                        "{error}; do not terminate by pid alone because that pid may already belong to a different process"
                    ))),
                    None => Ok(()),
                };
            }
            Ok(None) if started.elapsed() >= CLEANUP_BOUND => {
                let identity = observe_identity(pid);
                let cause = match kill_error {
                    Some(error) => format!("{error}; still running after the termination deadline"),
                    None => "still running after the termination deadline".to_owned(),
                };
                return Err(invalid(&recovery_identity(&identity, &cause)));
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                return Err(invalid(&recovery_identity(
                    &observe_identity(pid),
                    &error.to_string(),
                )));
            }
        }
    }
}

struct HelperRun {
    exit_code: u32,
    stdout: String,
    stderr: String,
}

fn system_executable(name: &str) -> PathBuf {
    let root = env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join(name)
}

fn terminate_bounded(
    program: &Path,
    args: &[std::ffi::OsString],
    deadline: Duration,
) -> io::Result<HelperRun> {
    #[cfg(windows)]
    {
        terminate_bounded_windows(program, args, deadline)
    }
    #[cfg(not(windows))]
    {
        let _ = (program, args, deadline);
        Err(invalid(
            "bounded termination helper requires the owned Windows job",
        ))
    }
}

#[cfg(windows)]
fn terminate_bounded_windows(
    program: &Path,
    args: &[std::ffi::OsString],
    deadline: Duration,
) -> io::Result<HelperRun> {
    if !program.is_file() {
        return Err(invalid(&format!(
            "termination helper program {} is missing",
            program.display()
        )));
    }
    let budget = deadline.max(Duration::from_millis(100));
    let started = Instant::now();
    let directory = env::temp_dir().join(format!(
        "claim-oracle-term-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&directory)?;
    let stdout_path = directory.join("stdout.txt");
    let stderr_path = directory.join("stderr.txt");
    let mut spec = CommandSpec::new(program);
    spec.args = args.to_vec();
    spec.stdout = Some(fs::File::create(&stdout_path)?);
    spec.stderr = Some(fs::File::create(&stderr_path)?);
    let job = match Job::new(Limits::default()) {
        Ok(job) => job,
        Err(error) => {
            return Err(invalid(&format!(
                "termination job was not created: {error}; helper artifacts retained at {}",
                directory.display()
            )));
        }
    };
    let process = match job.spawn(&spec) {
        Ok(process) => process,
        Err(error) => {
            return Err(invalid(&format!(
                "termination helper did not start: {error}; helper artifacts retained at {}",
                directory.display()
            )));
        }
    };
    let remaining = budget.saturating_sub(started.elapsed());
    if remaining < Duration::from_millis(50) {
        let _ = job.terminate(1, Duration::from_millis(200));
        return Err(helper_timeout(&directory, budget, started.elapsed()));
    }
    let cleanup = Duration::from_millis(300)
        .min(remaining / 3)
        .max(Duration::from_millis(50));
    let wait_for = remaining
        .saturating_sub(cleanup)
        .max(Duration::from_millis(1));
    let wait_deadline = match Deadline::after(wait_for) {
        Ok(deadline) => deadline,
        Err(error) => {
            let _ = job.terminate(1, cleanup);
            return Err(invalid(&format!(
                "termination deadline was rejected: {error}; helper artifacts retained at {}",
                directory.display()
            )));
        }
    };
    let outcome = job.wait(&process, wait_deadline, &Cancellation::default(), cleanup);
    let elapsed = started.elapsed();
    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
    match outcome {
        Ok(outcome)
            if outcome.reason == StopReason::Timeout
                || elapsed > budget + Duration::from_millis(750) =>
        {
            Err(helper_timeout(&directory, budget, elapsed))
        }
        Ok(outcome) => {
            let _ = fs::remove_dir_all(&directory);
            Ok(HelperRun {
                exit_code: outcome.process_exit_code,
                stdout,
                stderr,
            })
        }
        Err(error) => Err(invalid(&format!(
            "termination helper failed ({error}) after {elapsed:?}; helper artifacts retained at {}",
            directory.display()
        ))),
    }
}

fn helper_timeout(directory: &Path, budget: Duration, elapsed: Duration) -> io::Error {
    invalid(&format!(
        "termination helper exceeded {budget:?} after {elapsed:?}; helper artifacts retained at {}",
        directory.display()
    ))
}

fn kill_tree(pid: u32) -> io::Result<()> {
    let identity = observe_identity(pid);
    let args = [
        std::ffi::OsString::from("/F"),
        std::ffi::OsString::from("/T"),
        std::ffi::OsString::from("/PID"),
        std::ffi::OsString::from(pid.to_string()),
    ];
    let run = terminate_bounded(&system_executable("taskkill.exe"), &args, CLEANUP_BOUND).map_err(
        |error| {
            invalid(&format!(
                "{error}; {}",
                recovery_identity(&identity, "termination helper did not finish")
            ))
        },
    )?;
    let text = format!("{}{}", run.stdout, run.stderr);
    if run.exit_code == 0 {
        return Ok(());
    }
    if text.to_ascii_lowercase().contains("not found") || run.exit_code == 128 {
        return Ok(());
    }
    Err(invalid(&recovery_identity(
        &identity,
        &format!("taskkill failed: {}", text.trim()),
    )))
}

pub(crate) fn prove_cleanup(args: &[std::ffi::OsString]) -> io::Result<i32> {
    if !args.is_empty() {
        return Err(invalid("claim-oracle-cleanup takes no options"));
    }
    let missing = proof_value(prove_missing_capture());
    let delayed = proof_value(prove_delayed_capture());
    let null_streams = proof_value(prove_null_capture());
    let termination = proof_value(prove_termination_bound());
    let passed = missing.0 && delayed.0 && null_streams.0 && termination.0;
    let report = json!({
        "schema": 1,
        "kind": "executor-claim-oracle-cleanup",
        "passed": passed,
        "missingCapture": missing.1,
        "delayedCapture": delayed.1,
        "nullCapture": null_streams.1,
        "termination": termination.1,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(io::Error::other)?
    );
    Ok(if passed { 0 } else { 1 })
}

fn proof_value(result: io::Result<Value>) -> (bool, Value) {
    match result {
        Ok(value) => (value["ok"].as_bool().unwrap_or(false), value),
        Err(error) => (false, json!({ "ok": false, "error": error.to_string() })),
    }
}

fn probe_root(name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "claim-oracle-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0)
    ))
}

fn prove_missing_capture() -> io::Result<Value> {
    let root = probe_root("missing-capture");
    fs::create_dir_all(&root)?;
    let mut command = Command::new(system_executable("cmd.exe"));
    command.args(["/c", "exit", "0"]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut command);
    let mut child = command.spawn()?;
    let identity = observe_identity(child.id());
    let status = child.wait()?;
    let started = Instant::now();
    let capture = read_capture(&root, &identity, PIPED_CAPTURE);
    let elapsed = started.elapsed();
    let error = capture.error.clone().unwrap_or_default();
    let _ = fs::remove_dir_all(&root);
    let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    Ok(json!({
        "ok": status.code() == Some(0)
            && capture.error.is_some()
            && (4_500..12_000).contains(&elapsed_ms)
            && error.contains("missing capture is not success")
            && !error.contains("taskkill /F /T /PID"),
        "exitCode": status.code(),
        "accepted": capture.error.is_none(),
        "elapsedMs": elapsed_ms,
        "error": error,
    }))
}

fn prove_delayed_capture() -> io::Result<Value> {
    let root = probe_root("delayed-capture");
    fs::create_dir_all(&root)?;
    let identity = OwnedIdentity {
        pid: 7_000_001,
        creation_time: None,
    };
    let stdout = root.join(format!("out-{}.stdout", identity.pid));
    let stderr = root.join(format!("out-{}.stderr", identity.pid));
    let stdout_done = sidecar(&stdout, ".done");
    let stderr_done = sidecar(&stderr, ".done");
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(400));
        let _ = fs::write(&stdout, b"delayed-stdout");
        let _ = fs::write(&stdout_done, b"complete");
        let _ = fs::write(&stderr_done, b"complete");
    });
    let started = Instant::now();
    let capture = read_capture(&root, &identity, PIPED_CAPTURE);
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let text = capture.text.clone();
    let error = capture.error.clone().unwrap_or_default();
    let _ = fs::remove_dir_all(&root);
    Ok(json!({
        "ok": capture.error.is_none()
            && text.contains("delayed-stdout")
            && (350..5_000).contains(&elapsed_ms),
        "accepted": capture.error.is_none() && text.contains("delayed-stdout"),
        "elapsedMs": elapsed_ms,
        "text": text,
        "error": error,
    }))
}

fn prove_null_capture() -> io::Result<Value> {
    let root = probe_root("null-capture");
    fs::create_dir_all(&root)?;
    let mut command = Command::new(system_executable("cmd.exe"));
    command.args(["/c", "exit", "0"]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut command);
    let child = command.spawn()?;
    let started = Instant::now();
    let joined = join_child(&root, child, Duration::from_secs(5), NULL_CAPTURE);
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let error = joined.cleanup_error.clone().unwrap_or_default();
    let _ = fs::remove_dir_all(&root);
    Ok(json!({
        "ok": joined.code == Some(0)
            && joined.cleanup_error.is_none()
            && !joined.timed_out
            && elapsed_ms < 1_500,
        "accepted": joined.code == Some(0) && joined.cleanup_error.is_none(),
        "exitCode": joined.code,
        "elapsedMs": elapsed_ms,
        "error": error,
    }))
}

fn prove_termination_bound() -> io::Result<Value> {
    let original = "observation exceeded the caller deadline before termination";
    let deadline = Duration::from_millis(500);
    let args = [
        std::ffi::OsString::from("-n"),
        std::ffi::OsString::from("30"),
        std::ffi::OsString::from("127.0.0.1"),
    ];
    let started = Instant::now();
    let helper = terminate_bounded(&system_executable("ping.exe"), &args, deadline);
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (succeeded, cleanup) = match helper {
        Ok(run) => (true, format!("helper returned exit {}", run.exit_code)),
        Err(error) => (false, error.to_string()),
    };
    let artifacts_existed = cleanup
        .split("helper artifacts retained at ")
        .nth(1)
        .map(str::trim)
        .is_some_and(|path| Path::new(path).is_dir());
    if let Some(path) = cleanup
        .split("helper artifacts retained at ")
        .nth(1)
        .map(str::trim)
    {
        let _ = fs::remove_dir_all(path);
    }
    let combined = combine_failure(original, &cleanup);
    let quick = terminate_bounded(
        &system_executable("ping.exe"),
        &[
            std::ffi::OsString::from("-n"),
            std::ffi::OsString::from("1"),
            std::ffi::OsString::from("127.0.0.1"),
        ],
        Duration::from_secs(5),
    );
    let (quick_ok, quick_detail) = match &quick {
        Ok(run) => (
            run.exit_code == 0,
            format!("exit {} stderr={}", run.exit_code, run.stderr.trim()),
        ),
        Err(error) => (false, error.to_string()),
    };
    let tree = prove_tree_kill();
    let bounded = elapsed_ms < 5_000;
    let retained = combined.contains(original) && combined.contains("cleanup error:");
    let non_success = !succeeded && cleanup.contains("exceeded");
    Ok(json!({
        "ok": bounded && retained && non_success && artifacts_existed && quick_ok && tree.0,
        "succeeded": succeeded,
        "bounded": bounded,
        "retainedOriginal": retained,
        "elapsedMs": elapsed_ms,
        "deadlineMs": 500,
        "artifactsExisted": artifacts_existed,
        "helperCanSucceed": quick_ok,
        "helperSuccessDetail": quick_detail,
        "treeKillOk": tree.0,
        "treeKillBounded": tree.1 < 8_000,
        "treeKillElapsedMs": tree.1,
        "error": combined,
    }))
}

fn prove_tree_kill() -> (bool, u64) {
    let mut command = Command::new(system_executable("ping.exe"));
    command.args(["-n", "20", "127.0.0.1"]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    no_window(&mut command);
    let Ok(mut child) = command.spawn() else {
        return (false, u64::MAX);
    };
    let started = Instant::now();
    let killed = kill_tree(child.id());
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let reaped = reap_probe(&mut child);
    (killed.is_ok() && reaped && elapsed_ms < 8_000, elapsed_ms)
}

fn reap_probe(child: &mut Child) -> bool {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if started.elapsed() >= Duration::from_secs(2) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return false,
        }
    }
}

fn no_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

fn excerpt(text: &str) -> String {
    let mut excerpt: String = text.chars().take(EXCERPT).collect();
    if text.chars().count() > EXCERPT {
        excerpt.push('…');
    }
    excerpt.replace('\n', "\\n")
}

fn tail(text: &str) -> String {
    if text.len() <= OUTPUT_LIMIT {
        text.to_owned()
    } else {
        let start = text.len() - OUTPUT_LIMIT;
        format!("[truncated; truncation is not success]\n{}", &text[start..])
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
