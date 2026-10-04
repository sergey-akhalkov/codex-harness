//! Candidate-independent acceptance for the workload that isolates parallel
//! `harness-core` lib test state: three consecutive full parallel lib runs and
//! then every discovered `codex-harness` test target, each driven through
//! plain `cargo` (no heavy wrapper, no model call, no network).
//!
//! `harness-executor-fixture parallel-lib-oracle --request FILE
//!   --request-sha256 HEX [--workspace DIR] [--discover]`
//! verifies the pinned digest of the frozen schema 1 request
//! `{schema, source_root, lib_repetitions = 3, regression_targets = "auto"}`,
//! discovers the `codex-harness` test targets from the workspace's own
//! `crates/codex-harness/Cargo.toml` (explicit `[[test]]` entries plus Cargo's
//! automatic `tests/` discovery, so an unsplit and a split target list are
//! read by the same logic), then runs the repeated lib suites followed by one
//! serialized `cargo test --test <name>` per discovered target, in order,
//! stopping at the first failed run. Exit code 0 means every run exited 0; any
//! failure prints a bounded JSON verdict naming the failed command and its
//! exit code and exits 1. `--discover` prints the discovered target list
//! without running anything. `--workspace`, when given, must resolve to the
//! request's `source_root`: the frozen request and the task root the
//! supervisor materialized cannot disagree.
//!
//! The checker resolves `cargo` from the explicit `CARGO` environment value
//! (when it is an absolute ordinary file) or from the process `PATH`; it never
//! runs through the heavy-command wrapper, and every build lands in the
//! checked workspace's own `target` directory so an ambient `CARGO_TARGET_DIR`
//! cannot move the artifacts out of the run under test.

use harness_core::{build_identity, inventory};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::Instant,
};

const USAGE: &str = "\
harness-executor-fixture parallel-lib-oracle --request FILE --request-sha256 HEX
                                      [--workspace DIR] [--discover]
  Candidate-independent acceptance for the parallel harness-core lib
  isolation workload. Verifies the frozen schema 1 request {schema,
  source_root, lib_repetitions = 3, regression_targets = \"auto\"}, discovers
  the codex-harness test targets from the workspace manifest, then runs three
  consecutive full parallel harness-core lib suites and one serialized plain
  cargo run per discovered target. Exit 0 only when every run exited 0; the
  bounded JSON verdict names the first failed command and its exit code.
  --discover prints the discovered targets without running them.";

/// The only request schema this checker reads.
const SCHEMA: u32 = 1;
/// The frozen number of consecutive full parallel lib suites.
const LIB_REPETITIONS: u32 = 3;
/// The only accepted regression-target selection: discover them.
const AUTO_TARGETS: &str = "auto";
/// Bounded size of the frozen request document.
const REQUEST_LIMIT: u64 = 64 * 1024;
/// Bounded size of the manifest the discovery reads.
const MANIFEST_LIMIT: u64 = 1024 * 1024;
/// Retained tail, per stream, of one run's output.
const RETAINED_BYTES: usize = 64 * 1024;
/// Excerpt limit of the failing run's output in the verdict.
const EXCERPT_LIMIT: usize = 4000;
/// Session identity of the surrounding caller must not leak into the suites.
const CLEARED_ENV: [&str; 10] = [
    "HARNESS_EXECUTOR_SESSION",
    "HARNESS_EXECUTOR_RUN",
    "HARNESS_ORIGINATING_LEAD",
    "CODEX_SESSION_ID",
    "CODEX_THREAD_ID",
    "CODEX_CI",
    "WT_SESSION",
    "HARNESS_EXECUTOR_FIXTURE_MODE",
    "HARNESS_EXECUTOR_FIXTURE_SESSION",
    "HARNESS_EXECUTOR_FIXTURE_STARTED",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: u32,
    source_root: PathBuf,
    lib_repetitions: u32,
    regression_targets: String,
}

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    let mut request_path: Option<PathBuf> = None;
    let mut request_sha256: Option<String> = None;
    let mut workspace: Option<PathBuf> = None;
    let mut discover_only = false;
    let mut index = 0;
    while index < args.len() {
        let key = args[index]
            .to_str()
            .ok_or_else(|| invalid("invalid parallel-lib-oracle option"))?;
        match key {
            "--help" => {
                println!("{USAGE}");
                return Ok(0);
            }
            "--discover" => {
                if discover_only {
                    return Err(invalid("duplicate parallel-lib-oracle --discover"));
                }
                discover_only = true;
                index += 1;
            }
            "--request" | "--request-sha256" | "--workspace" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid(format!("parallel-lib-oracle {key} needs a value")))?;
                match key {
                    "--request" => request_path = Some(PathBuf::from(value)),
                    "--request-sha256" => {
                        request_sha256 = Some(
                            value
                                .to_str()
                                .ok_or_else(|| invalid("the request digest is not Unicode"))?
                                .to_owned(),
                        );
                    }
                    _ => workspace = Some(PathBuf::from(value)),
                }
                index += 2;
            }
            _ => {
                return Err(invalid(format!(
                    "invalid parallel-lib-oracle option: {key}"
                )));
            }
        }
    }
    let request_path =
        request_path.ok_or_else(|| invalid("parallel-lib-oracle needs --request FILE"))?;
    let request_sha256 =
        request_sha256.ok_or_else(|| invalid("parallel-lib-oracle needs --request-sha256 HEX"))?;
    let request = read_request(&request_path, &request_sha256)?;
    let source_root = resolve_source_root(&request, workspace.as_deref())?;
    let cargo = resolve_cargo()?;
    let targets = discover_targets(&source_root)?;
    if discover_only {
        let verdict = report(&source_root, &cargo, &targets, Vec::new(), None, true);
        println!("{}", serde_json::to_string_pretty(&verdict)?);
        return Ok(0);
    }
    let mut steps: Vec<Value> = Vec::new();
    let mut failure: Option<Value> = None;
    for repetition in 1..=request.lib_repetitions {
        let argv = vec![
            "test".to_owned(),
            "-p".to_owned(),
            "harness-core".to_owned(),
            "--lib".to_owned(),
        ];
        let step = run_step(
            &cargo,
            &source_root,
            &format!("lib-run-{repetition}"),
            &argv,
        );
        let passed = step.passed();
        let record = step.json();
        steps.push(record);
        if !passed {
            failure = Some(step.failure_json());
            break;
        }
    }
    if failure.is_none() {
        for name in &targets {
            let argv = vec![
                "test".to_owned(),
                "-p".to_owned(),
                "codex-harness".to_owned(),
                "--test".to_owned(),
                name.clone(),
            ];
            let step = run_step(&cargo, &source_root, &format!("target:{name}"), &argv);
            let passed = step.passed();
            let record = step.json();
            steps.push(record);
            if !passed {
                failure = Some(step.failure_json());
                break;
            }
        }
    }
    let passed =
        failure.is_none() && steps.len() == (request.lib_repetitions as usize + targets.len());
    let verdict = report(&source_root, &cargo, &targets, steps, failure, false);
    let serialized = serde_json::to_string_pretty(&verdict)?;
    println!("{serialized}");
    Ok(if passed { 0 } else { 1 })
}

/// Read the frozen request after verifying its pinned digest, then parse it
/// strictly: unknown fields, a different schema, another repetition count or
/// another target selection are refused before any cargo invocation.
fn read_request(path: &Path, expected: &str) -> io::Result<Request> {
    if !path.is_absolute() {
        return Err(invalid(
            "the parallel-lib request must be an explicit absolute file",
        ));
    }
    inventory::ordinary_parents(path)?;
    build_identity::ordinary(path)?;
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > REQUEST_LIMIT {
        return Err(invalid(
            "the parallel-lib request must be a bounded ordinary file",
        ));
    }
    let bytes = fs::read(path)?;
    let expected = expected.trim();
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(
            "the parallel-lib request digest must be a 64 character hexadecimal sha256",
        ));
    }
    if !build_identity::hash_bytes(&bytes).eq_ignore_ascii_case(expected) {
        return Err(invalid(
            "the frozen parallel-lib request changed; no suite was dispatched",
        ));
    }
    let request: Request = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("the parallel-lib request is not a schema 1 request document"))?;
    if request.schema != SCHEMA
        || request.lib_repetitions != LIB_REPETITIONS
        || request.regression_targets != AUTO_TARGETS
        || !request.source_root.is_absolute()
    {
        return Err(invalid(
            "the parallel-lib request must declare schema 1, an absolute source_root, lib_repetitions = 3 and regression_targets = \"auto\"",
        ));
    }
    Ok(request)
}

/// Resolve the checked tree: the request's `source_root`, an existing ordinary
/// directory, cross-checked against an explicit `--workspace` when one is
/// supplied so a stale or redirected request cannot test another tree.
fn resolve_source_root(request: &Request, workspace: Option<&Path>) -> io::Result<PathBuf> {
    inventory::ordinary_parents(&request.source_root)?;
    build_identity::ordinary(&request.source_root)?;
    let source_root = request.source_root.canonicalize()?;
    if !source_root.is_dir() {
        return Err(invalid(
            "the parallel-lib source_root must be an existing directory",
        ));
    }
    if let Some(workspace) = workspace {
        inventory::ordinary_parents(workspace)?;
        build_identity::ordinary(workspace)?;
        let workspace = workspace.canonicalize()?;
        if workspace != source_root {
            return Err(invalid(
                "the parallel-lib request source_root differs from the materialized workspace; refusing to check another tree",
            ));
        }
    }
    Ok(source_root)
}

/// The plain cargo program the suites run through: the explicit `CARGO`
/// environment value when it names an absolute file, otherwise the first
/// `cargo` on the process `PATH`. The discovered path is kept as found: on an
/// installation whose `cargo.exe` is the rustup proxy link, canonicalizing
/// would resolve to `rustup.exe` and the toolchain manager would refuse
/// cargo's arguments, so the invoked name must stay `cargo`.
fn resolve_cargo() -> io::Result<PathBuf> {
    let usable = |candidate: &Path| candidate.is_absolute() && candidate.is_file();
    if let Some(program) = env::var_os("CARGO").map(PathBuf::from)
        && !program.as_os_str().is_empty()
        && usable(&program)
    {
        return std::path::absolute(&program);
    }
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            for name in ["cargo.exe", "cargo"] {
                let candidate = directory.join(name);
                if usable(&candidate) {
                    return std::path::absolute(&candidate);
                }
            }
        }
    }
    Err(invalid(
        "no plain cargo program is resolvable for the parallel-lib checks",
    ))
}

/// Discover the `codex-harness` test targets from the workspace's own
/// manifest: explicit `[[test]]` entries in declaration order, then Cargo's
/// automatic `tests/` discovery (default `autotests = true`) for every test
/// file no explicit entry claims. An unsplit list (only automatic targets)
/// and a split list (explicit entries, possibly with a different name) are
/// read by this one path.
fn discover_targets(source_root: &Path) -> io::Result<Vec<String>> {
    let manifest = source_root.join("crates/codex-harness/Cargo.toml");
    let metadata = fs::metadata(&manifest).map_err(|error| {
        invalid(format!(
            "the codex-harness manifest is unavailable at {} ({error})",
            manifest.display()
        ))
    })?;
    if !metadata.is_file() || metadata.len() > MANIFEST_LIMIT {
        return Err(invalid("the codex-harness manifest is not a bounded file"));
    }
    let text = fs::read_to_string(&manifest)
        .map_err(|_| invalid("the codex-harness manifest is not UTF-8"))?;
    let table: toml::Table = toml::from_str(&text)
        .map_err(|_| invalid("the codex-harness manifest is not a valid TOML table"))?;
    let mut names: Vec<String> = Vec::new();
    let mut claimed: Vec<PathBuf> = Vec::new();
    if let Some(entries) = table.get("test").and_then(toml::Value::as_array) {
        for entry in entries {
            let entry = entry
                .as_table()
                .ok_or_else(|| invalid("a codex-harness [[test]] entry is not a TOML table"))?;
            if entry.get("test").and_then(toml::Value::as_bool) == Some(false) {
                continue;
            }
            let name = entry
                .get("name")
                .and_then(toml::Value::as_str)
                .map(str::to_owned);
            let path = entry
                .get("path")
                .and_then(toml::Value::as_str)
                .map(PathBuf::from);
            let (name, path) = match (name, path) {
                (Some(name), Some(path)) => (name, path),
                (Some(name), None) => {
                    let path = PathBuf::from("tests").join(format!("{name}.rs"));
                    (name, path)
                }
                (None, Some(path)) => {
                    let name = path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("a codex-harness [[test]] path has no name"))?;
                    (name, path)
                }
                (None, None) => {
                    return Err(invalid(
                        "a codex-harness [[test]] entry names neither a target nor a path",
                    ));
                }
            };
            claimed.push(path);
            names.push(name);
        }
    }
    let autotests = table
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("autotests"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);
    if autotests {
        let tests = source_root.join("crates/codex-harness/tests");
        let mut automatic: Vec<String> = Vec::new();
        if tests.is_dir() {
            for entry in fs::read_dir(&tests)? {
                let entry = entry?;
                let path = entry.path();
                let relative = PathBuf::from("tests").join(entry.file_name());
                if path.is_file() {
                    if path.extension().is_some_and(|extension| extension == "rs")
                        && !claimed.contains(&relative)
                        && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
                    {
                        automatic.push(name.to_owned());
                    }
                } else if path.is_dir()
                    && path.join("main.rs").is_file()
                    && !claimed.contains(&relative.join("main.rs"))
                    && let Some(name) = path.file_name().and_then(|stem| stem.to_str())
                {
                    automatic.push(name.to_owned());
                }
            }
        }
        automatic.sort();
        for name in automatic {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    if names.is_empty() {
        return Err(invalid(
            "the codex-harness manifest discovers no test targets to run",
        ));
    }
    Ok(names)
}

struct Step {
    id: String,
    argv: Vec<String>,
    exit_code: Option<i32>,
    error: Option<String>,
    elapsed_ms: u64,
    output: Option<String>,
}

impl Step {
    fn passed(&self) -> bool {
        self.error.is_none() && self.exit_code == Some(0)
    }

    fn cause(&self) -> String {
        match (&self.error, self.exit_code) {
            (Some(error), _) => format!("run '{}' could not start: {error}", self.id),
            (None, Some(code)) => format!("run '{}' exited {code}", self.id),
            (None, None) => format!("run '{}' ended without an exit status", self.id),
        }
    }

    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "command": self.argv,
            "status": if self.error.is_some() { "spawn-failed" } else { "exited" },
            "exitCode": self.exit_code,
            "elapsedMs": self.elapsed_ms,
            "output": self.output,
        })
    }

    fn failure_json(&self) -> Value {
        json!({
            "id": self.id,
            "command": self.argv,
            "exitCode": self.exit_code,
            "cause": self.cause(),
        })
    }
}

/// Run one plain cargo command in the checked workspace. Output is retained
/// as a bounded tail per stream, so a failing suite's cause reaches the
/// verdict without an unbounded report; the workspace's own `target`
/// directory receives the build output.
fn run_step(cargo: &Path, source_root: &Path, id: &str, argv: &[String]) -> Step {
    let started = Instant::now();
    let mut command = Command::new(cargo);
    command
        .args(argv)
        .current_dir(source_root)
        .env("CARGO_TARGET_DIR", source_root.join("target"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in CLEARED_ENV {
        command.env_remove(name);
    }
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Step {
                id: id.to_owned(),
                argv: once_cargo(cargo, argv),
                exit_code: None,
                error: Some(error.to_string()),
                elapsed_ms: started.elapsed().as_millis() as u64,
                output: None,
            };
        }
    };
    let captured = capture(child);
    let mut watched = captured.stdout;
    if !captured.stderr.trim().is_empty() {
        watched.push_str("\n[stderr] ");
        watched.push_str(&captured.stderr);
    }
    let passed = captured.error.is_none() && captured.exit_code == Some(0);
    Step {
        id: id.to_owned(),
        argv: once_cargo(cargo, argv),
        exit_code: captured.exit_code,
        error: captured.error,
        elapsed_ms: started.elapsed().as_millis() as u64,
        output: (!passed).then(|| tail_excerpt(&watched, EXCERPT_LIMIT)),
    }
}

/// The reported command is the literal argv the checker executed, with the
/// resolved cargo program first.
fn once_cargo(cargo: &Path, argv: &[String]) -> Vec<String> {
    let mut command = vec![cargo.to_string_lossy().into_owned()];
    command.extend(argv.iter().cloned());
    command
}

struct Captured {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    error: Option<String>,
}

/// Drain both streams on their own threads, retaining a bounded tail each, and
/// then join the child. A stream read error or a lost exit status is reported,
/// never swallowed.
fn capture(mut child: Child) -> Captured {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = stdout.map(|pipe| thread::spawn(move || read_tail(pipe)));
    let stderr_reader = stderr.map(|pipe| thread::spawn(move || read_tail(pipe)));
    let status = child.wait();
    let (mut error, mut stdout_text, mut stderr_text) = (None, String::new(), String::new());
    if let Some(reader) = stdout_reader {
        match reader.join() {
            Ok((output, stream_error)) => {
                stdout_text = output;
                if error.is_none() {
                    error = stream_error;
                }
            }
            Err(_) => error = error.or(Some("the stdout reader panicked".to_owned())),
        }
    }
    if let Some(reader) = stderr_reader {
        match reader.join() {
            Ok((output, stream_error)) => {
                stderr_text = output;
                if error.is_none() {
                    error = stream_error;
                }
            }
            Err(_) => error = error.or(Some("the stderr reader panicked".to_owned())),
        }
    }
    let (exit_code, status_error) = match status {
        Ok(status) => (status.code(), None),
        Err(wait_error) => (None, Some(wait_error.to_string())),
    };
    let error = error.or(status_error).or_else(|| {
        exit_code
            .is_none()
            .then(|| "the run ended without an exit status".to_owned())
    });
    Captured {
        exit_code,
        stdout: stdout_text,
        stderr: stderr_text,
        error,
    }
}

fn read_tail(mut pipe: impl io::Read) -> (String, Option<String>) {
    let mut kept: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                kept.extend_from_slice(&buffer[..read]);
                if kept.len() > RETAINED_BYTES {
                    let excess = kept.len() - RETAINED_BYTES;
                    kept.drain(..excess);
                }
            }
            Err(error) => {
                return (
                    String::from_utf8_lossy(&kept).into_owned(),
                    Some(error.to_string()),
                );
            }
        }
    }
    (String::from_utf8_lossy(&kept).into_owned(), None)
}

/// The end of a run's retained output, where a test failure summary lives,
/// bounded for the verdict.
fn tail_excerpt(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    let total = trimmed.chars().count();
    if total <= limit {
        return trimmed.to_owned();
    }
    let mut excerpt = String::from("…");
    excerpt.extend(trimmed.chars().skip(total - limit));
    excerpt
}

fn report(
    source_root: &Path,
    cargo: &Path,
    targets: &[String],
    steps: Vec<Value>,
    failure: Option<Value>,
    discover_only: bool,
) -> Value {
    json!({
        "schema": 1,
        "kind": "parallel-lib-oracle",
        "sourceRoot": source_root.to_string_lossy(),
        "cargo": cargo.to_string_lossy(),
        "libRepetitions": LIB_REPETITIONS,
        "regressionTargets": AUTO_TARGETS,
        "targets": targets,
        "discoverOnly": discover_only,
        "steps": steps,
        // Discovery is not an acceptance verdict: no suite ran.
        "passed": if discover_only { Value::Null } else { json!(failure.is_none()) },
        "failure": failure,
        "modelCalls": 0,
    })
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_string())
}
