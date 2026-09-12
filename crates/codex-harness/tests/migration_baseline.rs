//! Model-free current-path baseline capture for migration task 1.3.
//! cargo test --locked -p codex-harness --test migration_baseline -- --test-threads=1 --nocapture
//! Private receipts stay under the printed owned TEMP root. Candidate
//! comparisons belong to task 9.4 and must reuse this method unchanged.
#![cfg(windows)]

use harness_core::{
    cancellable_pipe::{CancellablePipe, anonymous_pipe},
    console::{ConsoleSession, ConsoleSpec},
    mcp_protocol::{Decoder, Message, READ_CHUNK},
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, OwnedProcess, StopReason},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const SAMPLES: usize = 5;
const WARM_DISCARD: usize = 1;
const DESKTOP_PWSH: &str = r"C:\Program Files\PowerShell\7\pwsh.exe";
const CLEANUP: Duration = Duration::from_secs(5);

fn checkout() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn desktop_pwsh() -> PathBuf {
    if let Some(path) = env::var_os("HARNESS_ACCEPTANCE_POWERSHELL") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return path;
        }
    }
    let path = PathBuf::from(DESKTOP_PWSH);
    assert!(
        path.is_file(),
        "desktop PowerShell is required at {DESKTOP_PWSH}"
    );
    path
}

fn sha256_file(path: &Path) -> String {
    harness_core::build_identity::hash_file(path).unwrap()
}

fn looks_like_codex_dir(entry: &Path) -> bool {
    ["codex.exe", "codex.cmd", "codex.ps1", "codex.bat"]
        .iter()
        .any(|name| entry.join(name).is_file())
}

fn warmup_powershell() {
    let status = Command::new(desktop_pwsh())
        .args(["-NoLogo", "-NoProfile", "-Command", "exit 0"])
        .status()
        .unwrap();
    assert!(status.success(), "desktop PowerShell warmup failed");
}

fn rtk_adapter() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_codex-harness")).with_file_name("harness-rtk.exe");
    assert!(
        path.is_file(),
        "build the RTK adapter first: cargo build --locked -p harness-rtk; missing {}",
        path.display()
    );
    path
}

fn git_output(args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(checkout())
        .output()
        .unwrap()
}

fn identity() -> Value {
    let rustc_out = Command::new("rustc").arg("-vV").output().unwrap();
    let cargo_out = Command::new("cargo").arg("-vV").output().unwrap();
    let rustc = String::from_utf8_lossy(&rustc_out.stdout);
    let cargo = String::from_utf8_lossy(&cargo_out.stdout);
    let field = |text: &str, key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or("unknown")
            .to_string()
    };
    json!({
        "head": String::from_utf8_lossy(&git_output(&["rev-parse", "HEAD"]).stdout).trim(),
        "dirty_index": !git_output(&["diff-index", "--quiet", "HEAD", "--"]).status.success(),
        "cargo_lock_sha256": sha256_file(&checkout().join("Cargo.lock")),
        "rustc": field(&rustc, "release: "),
        "rustc_commit": field(&rustc, "commit-hash: "),
        "host": field(&rustc, "host: "),
        "cargo": cargo.lines().next().unwrap_or("unknown"),
        "target": "x86_64-pc-windows-msvc",
        "profile": "test",
        "captured_unix_seconds": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        "binaries": {
            "codex-harness.exe": sha256_file(Path::new(env!("CARGO_BIN_EXE_codex-harness"))),
            "codex.exe": sha256_file(Path::new(env!("CARGO_BIN_EXE_codex"))),
            "harness-rtk.exe": sha256_file(&rtk_adapter()),
            "harness-process-fixture.exe": sha256_file(Path::new(env!("CARGO_BIN_EXE_harness-process-fixture"))),
            "harness-console-fixture.exe": sha256_file(Path::new(env!("CARGO_BIN_EXE_harness-console-fixture"))),
            "harness-mcp-probe-fixture.exe": sha256_file(Path::new(env!("CARGO_BIN_EXE_harness-mcp-probe-fixture"))),
            "harness-launch-fixture.exe": sha256_file(Path::new(env!("CARGO_BIN_EXE_harness-launch-fixture"))),
        }
    })
}

fn method() -> Value {
    json!({
        "samples": SAMPLES,
        "warm_discard": WARM_DISCARD,
        "clock": "std::time::Instant monotonic wall time of the owned child",
        "aggregation": "median of remaining samples after discarding the first cold observation from the comparison set; cold and warm are both recorded",
        "dispersion": "sample range and mean absolute deviation from the median",
        "resources": "JobSnapshot peak_job_memory_bytes is recorded separately and never substituted for granted private commit",
        "noise_tolerance": {
            "material_if": "candidate_median - baseline_median > 100ms AND > 10% of baseline_median",
            "absolute_ms": 100.0,
            "relative": 0.10,
            "established_from": "baseline before candidate results",
            "externally_dominated": "provider, network, model and live global MCP/service timings are disclosed, not used as owned-boundary acceleration evidence"
        },
        "oracles": "nonzero exit, mixed Check/Diagnose rejection, missing core Check, MCP incomplete input, process timeout 124, console cancel 130, foreign process preservation, RTK malformed-hook silence",
        "candidate_gate": "task 9.4 must reuse this method, sample count, scenarios and noise boundary; Rust usage alone is not acceleration evidence"
    })
}

fn stats(samples: &[f64]) -> Value {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2]
    } else {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    };
    let mad = sorted.iter().map(|v| (v - median).abs()).sum::<f64>() / sorted.len() as f64;
    json!({"n": samples.len(), "median_ms": median, "min_ms": sorted.first().copied().unwrap_or(0.0), "max_ms": sorted.last().copied().unwrap_or(0.0), "mad_ms": mad, "samples_ms": samples})
}

fn sample_case(name: &str, mut run: impl FnMut() -> Value) -> Value {
    let mut observations = Vec::new();
    for i in 0..SAMPLES {
        let started = Instant::now();
        let oracle = run();
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        observations.push(json!({"i": i, "ms": ms, "oracle": oracle, "phase": if i == 0 { "cold" } else { "warm" }}));
    }
    let warm: Vec<f64> = observations
        .iter()
        .skip(WARM_DISCARD)
        .map(|row| row["ms"].as_f64().unwrap())
        .collect();
    let stable = observations.iter().all(|row| {
        row["oracle"]["passed"] == observations[0]["oracle"]["passed"]
            && row["oracle"]["exit"] == observations[0]["oracle"]["exit"]
    });
    let mut oracle = observations[0]["oracle"].clone();
    if !stable {
        oracle["passed"] = json!(false);
        oracle["unstable"] = json!(true);
    }
    json!({"name": name, "oracle": oracle, "cold_ms": observations[0]["ms"], "warm": stats(&warm), "observations": observations})
}

#[test]
fn capture_current_path_baselines_with_identity_oracles_and_noise_method() {
    let evidence = tempfile::Builder::new()
        .prefix("harness-baseline-migration-")
        .tempdir()
        .unwrap()
        .keep();
    println!("baseline evidence: {}", evidence.display());
    warmup_powershell();
    let cases = vec![
        native_launch_case(),
        script_launch_case(),
        missing_core_check(),
        script_missing_check(),
        diagnose_case(),
        missing_build_check(),
        mcp_case(),
        process_case(),
        console_case(),
        subscription_case(),
        rtk_case(),
    ];
    let failed: Vec<_> = cases
        .iter()
        .filter(|case| case["oracle"]["passed"] != true)
        .map(|case| case["name"].as_str().unwrap().to_owned())
        .collect();
    let report = json!({
        "schema": 1,
        "task": "1.3",
        "identity": identity(),
        "method": method(),
        "cases": cases,
        "failed": failed,
        "candidate_results_recorded": false,
        "limits": "model-free owned targets only; live global MCP/subscription service and provider timings are excluded from the owned comparison set"
    });
    fs::write(
        evidence.join("baseline.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"evidence": evidence, "failed": failed})).unwrap()
    );
    assert!(failed.is_empty(), "baseline oracles failed: {failed:?}");
}

fn native_launch_case() -> Value {
    let root = tempfile::Builder::new()
        .prefix("native-launch-baseline-")
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
        source
            .join(harness_core::build_identity::INSPECTION_SCHEMA)
            .parent()
            .unwrap()
            .to_owned(),
        build.clone(),
    ] {
        fs::create_dir_all(path).unwrap();
    }
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/one/src/lib.rs",
        harness_core::build_identity::INSPECTION_SCHEMA,
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
    let launcher = build.join("codex.exe");
    let upstream = root.path().join("upstream.exe");
    fs::copy(env!("CARGO_BIN_EXE_codex"), &launcher).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_harness-launch-fixture"), &upstream).unwrap();
    let mut binaries = BTreeMap::new();
    for name in harness_core::build_identity::BINARIES {
        let path = build.join(name);
        if *name != "codex.exe" {
            fs::write(&path, name).unwrap();
        }
        binaries.insert(
            name.to_string(),
            harness_core::build_identity::hash_file(&path).unwrap(),
        );
    }
    let record = harness_core::build_identity::BuildRecord {
        schema: harness_core::build_identity::SCHEMA,
        source_root: source.clone(),
        source: harness_core::build_identity::source_identity(&source).unwrap(),
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
    harness_core::build_selection::activate(&state, &build).unwrap();
    fs::write(home.join("harness/native-launch.json"), serde_json::to_vec(&json!({"schema":1,"state":state,"upstream":{"executable":upstream,"sha256":harness_core::build_identity::hash_file(&upstream).unwrap(),"package":null}})).unwrap()).unwrap();
    let args = [
        "--harness-effort",
        "routine",
        "exec",
        "",
        "проверка",
        "trailing",
        "literal",
    ];
    sample_case("launch.native_argv_unicode_stdin_nonzero", || {
        let mut child = Command::new(&launcher)
            .args(args)
            .current_dir(root.path())
            .env("CODEX_HOME", &home)
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
        let report: Value = serde_json::from_slice(&output.stdout).unwrap_or(json!({}));
        json!({"passed": output.status.code() == Some(19) && String::from_utf8_lossy(&output.stderr).trim() == "upstream stderr" && report["stdin"] == "первая строка\nsecond line\n", "exit": output.status.code()})
    })
}

fn script_launch_case() -> Value {
    let root = tempfile::Builder::new()
        .prefix("script-launch-baseline-")
        .tempdir()
        .unwrap();
    let home = root.path().join("home");
    let source = root.path().join("source");
    let workspace = root.path().join("workspace");
    let bin = root.path().join("bin");
    let launchers = home.join("harness/launchers/owned/codex.ps1");
    let tools = source.join("tools");
    fs::create_dir_all(launchers.parent().unwrap()).unwrap();
    fs::create_dir_all(&tools).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&bin).unwrap();
    let origin = checkout();
    fs::write(
        &launchers,
        fs::read(origin.join("tools/codex.ps1")).unwrap(),
    )
    .unwrap();
    fs::write(
        tools.join("codex.ps1"),
        fs::read(origin.join("tools/codex.ps1")).unwrap(),
    )
    .unwrap();
    let launcher = bin.join("codex.ps1");
    std::os::windows::fs::symlink_file(&launchers, &launcher).unwrap();
    let upstream = root.path().join("upstream.exe");
    fs::copy(env!("CARGO_BIN_EXE_harness-launch-fixture"), &upstream).unwrap();
    fs::copy(&upstream, bin.join("codex.exe")).unwrap();
    fs::write(home.join("harness/installation.json"), serde_json::to_vec(&json!({"schemaVersion":1,"sourceRoot":source,"codexHome":home,"userHome":root.path().join("user"),"codexCommand":upstream,"profileName":"harness","pathScope":"Process","pathAdded":false,"versions":{},"links":[],"launcherSource":launchers,"configBridge":env!("CARGO_BIN_EXE_codex-harness")})).unwrap()).unwrap();
    let isolated_path = {
        let mut entries = vec![bin.clone()];
        entries.extend(
            env::split_paths(&env::var_os("PATH").unwrap()).filter(|entry| {
                let text = entry.to_string_lossy().to_ascii_lowercase();
                !text.contains("windowsapps") && entry != &bin && !looks_like_codex_dir(entry)
            }),
        );
        env::join_paths(entries).unwrap()
    };
    let runner = root.path().join("stdin-runner.ps1");
    fs::write(&runner, "[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)\n[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)\n$OutputEncoding = [Console]::OutputEncoding\n[string[]]$forwarded = @(ConvertFrom-Json $env:HARNESS_SCRIPT_ARGUMENTS)\n$input | & $env:HARNESS_SCRIPT_LAUNCHER @forwarded\nexit $LASTEXITCODE\n").unwrap();
    let args =
        serde_json::to_string(&["exec", "", "проверка", "trailing", "literal", "--"]).unwrap();
    sample_case(
        "launch.script_fallback_missing_module_unicode_nonzero",
        || {
            let mut child = Command::new(desktop_pwsh())
                .args(["-NoLogo", "-NoProfile", "-File"])
                .arg(&runner)
                .current_dir(&workspace)
                .env("CODEX_HOME", &home)
                .env("PATH", &isolated_path)
                .env("HARNESS_SCRIPT_LAUNCHER", &launcher)
                .env("HARNESS_SCRIPT_ARGUMENTS", &args)
                .env("HARNESS_LAUNCH_FIXTURE_MODE", "nonzero")
                .env("PYTHONUTF8", "1")
                .env("POWERSHELL_TELEMETRY_OPTOUT", "1")
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
            let stderr = String::from_utf8_lossy(&output.stderr);
            let report: Value = serde_json::from_slice(&output.stdout).unwrap_or(json!({}));
            let stdin = report["stdin"].as_str().unwrap_or("").replace("\r\n", "\n");
            json!({"passed": output.status.code() == Some(19) && stderr.contains("Harness unavailable") && stdin == "первая строка\nsecond line\n", "exit": output.status.code()})
        },
    )
}

fn missing_core_check() -> Value {
    sample_case("check.core_missing_installation", || {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("uninstalled-home");
        let user = root.path().join("user");
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["check", "--core-only", "--codex-home"])
            .arg(&home)
            .arg("--user-home")
            .arg(&user)
            .current_dir(root.path())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        json!({"passed": !output.status.success() && !home.exists() && !user.exists(), "exit": output.status.code()})
    })
}

fn script_missing_check() -> Value {
    sample_case("check.script_missing_owned_homes", || {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        let user = root.path().join("user");
        fs::create_dir_all(&user).unwrap();
        let output = Command::new(desktop_pwsh())
            .args(["-NoLogo", "-NoProfile", "-File"])
            .arg(checkout().join("install.ps1"))
            .args([
                "-CoreOnly",
                "-Mode",
                "Check",
                "-PathScope",
                "Process",
                "-CodexHome",
            ])
            .arg(&home)
            .arg("-UserHome")
            .arg(&user)
            .current_dir(checkout())
            .output()
            .unwrap();
        json!({"passed": !output.status.success(), "exit": output.status.code()})
    })
}

fn diagnose_case() -> Value {
    sample_case(
        "check.diagnose_owned_report_and_mixed_selector_refusal",
        || {
            let root = tempfile::Builder::new()
                .prefix("source-observation-baseline-")
                .tempdir()
                .unwrap();
            let home = root.path().join("home");
            let user = root.path().join("user");
            let project = root.path().join("project");
            let source = root.path().join("source");
            for dir in [&home, &user, &project] {
                fs::create_dir(dir).unwrap();
            }
            fs::write(
                home.join("config.toml"),
                "model = 'gpt-6-astra'\ncheck_for_update_on_startup = false\n",
            )
            .unwrap();
            fs::write(
                home.join("harness.config.toml"),
                "model = 'gpt-6-astra'\nmodel_reasoning_effort = 'xhigh'\n",
            )
            .unwrap();
            for relative in ["global/agents", "skills/one"] {
                fs::create_dir_all(source.join(relative)).unwrap();
            }
            fs::write(
                source.join("global/profile.toml"),
                "model = 'gpt-6-astra'\nmodel_reasoning_effort = 'xhigh'\n",
            )
            .unwrap();
            fs::write(
                source.join("global/instructions.md"),
                "Owned source observation acceptance.\n",
            )
            .unwrap();
            for name in ["hooks.json", "token-hooks.json"] {
                fs::write(source.join("global").join(name), "{}").unwrap();
            }
            fs::write(source.join("skills/one/SKILL.md"), "---\nname: one\ndescription: Owned inert skill data.\n---\nPreserve source data.\n").unwrap();
            fs::write(source.join("global/kit.json"), serde_json::to_vec(&json!({"schema":1,"profile_name":"harness","profile":"global/profile.toml","instructions":"global/instructions.md","skills":"skills","agents":"global/agents","hooks":"global/hooks.json","token_hooks":"global/token-hooks.json"})).unwrap()).unwrap();
            let upstream = Path::new(env!("CARGO_BIN_EXE_harness-launch-fixture"));
            let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
                .args(["check", "--diagnose", "--source"])
                .arg(&source)
                .arg("--codex-home")
                .arg(&home)
                .arg("--user-home")
                .arg(&user)
                .arg("--project")
                .arg(&project)
                .arg("--upstream")
                .arg(upstream)
                .env("HARNESS_LAUNCH_FIXTURE_MODE", "discovery")
                .env("HARNESS_DISCOVERY_FIXTURE", "init-error")
                .current_dir(root.path())
                .output()
                .unwrap();
            let report: Value = serde_json::from_slice(&output.stdout).unwrap_or(json!({}));
            let pid_file = root.path().join("unexpected-check-start.pid");
            let mixed = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
                .args(["check", "--diagnose", "--core-only", "--source"])
                .arg(&source)
                .arg("--codex-home")
                .arg(&home)
                .arg("--user-home")
                .arg(&user)
                .arg("--project")
                .arg(&project)
                .arg("--upstream")
                .arg(upstream)
                .env("HARNESS_LAUNCH_FIXTURE_STARTED", &pid_file)
                .current_dir(root.path())
                .output()
                .unwrap();
            json!({"passed": output.status.success() && report["schemaVersion"] == 1 && report["model_calls"] == 0 && !mixed.status.success() && mixed.stdout.is_empty() && !pid_file.exists(), "exit": output.status.code(), "mixed_exit": mixed.status.code(), "status": report["status"]})
        },
    )
}

fn missing_build_check() -> Value {
    sample_case("check.build_missing_metadata", || {
        let root = tempfile::tempdir().unwrap();
        let build = root.path().join("missing-build");
        fs::create_dir(&build).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["check", "--build"])
            .arg(&build)
            .output()
            .unwrap();
        let report: Value = serde_json::from_slice(&output.stdout).unwrap_or(json!({}));
        json!({"passed": !output.status.success() && report["runtime_allowed"] == false, "exit": output.status.code(), "status": report["status"]})
    })
}

struct Mcp {
    job: Option<Job>,
    child: OwnedProcess,
    input: Option<CancellablePipe>,
    output: CancellablePipe,
    decoder: Decoder,
    cancel: Cancellation,
    stderr: PathBuf,
}

impl Mcp {
    fn start(root: &Path) -> Self {
        let (input, writer) = anonymous_pipe(4096).unwrap();
        let (reader, output) = anonymous_pipe(4096).unwrap();
        let mut command = CommandSpec::new(env!("CARGO_BIN_EXE_harness-mcp-probe-fixture"));
        command.args.push("--stdio-session".into());
        command.current_dir = Some(root.to_path_buf());
        command.stdin = Some(input);
        command.stdout = Some(output);
        command.stderr = Some(fs::File::create(root.join("stderr")).unwrap());
        let job = Job::new(Limits {
            memory_bytes: Some(512 * 1024 * 1024),
            cpu_percent: Some(25.0),
        })
        .unwrap();
        let child = job.spawn(&command).unwrap();
        drop(command);
        let cancel = Cancellation::default();
        Self {
            job: Some(job),
            child,
            input: Some(CancellablePipe::writer(writer, cancel.clone()).unwrap()),
            output: CancellablePipe::reader(reader, cancel.clone()).unwrap(),
            decoder: Decoder::default(),
            cancel,
            stderr: root.join("stderr"),
        }
    }
    fn send_bytes(&mut self, bytes: &[u8]) {
        self.input
            .as_mut()
            .unwrap()
            .write_all(
                bytes,
                Deadline::after(Duration::from_secs(3)).unwrap(),
                &self.cancel,
            )
            .unwrap();
    }
    fn send(&mut self, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        self.send_bytes(&bytes);
    }
    fn reply(&mut self) -> Value {
        let deadline = Deadline::after(Duration::from_secs(8)).unwrap();
        loop {
            if let Some(message) = self.decoder.next_message().unwrap() {
                return message.into_value();
            }
            let bytes = self
                .output
                .read(READ_CHUNK, deadline, &self.cancel)
                .unwrap();
            assert!(!bytes.is_empty(), "unexpected EOF");
            self.decoder.push(&bytes).unwrap();
        }
    }
    fn wait(mut self, expected: Option<u32>) -> harness_core::process::Outcome {
        let outcome = self
            .job
            .take()
            .unwrap()
            .wait(
                &self.child,
                Deadline::after(Duration::from_secs(8)).unwrap(),
                &self.cancel,
                Duration::from_secs(3),
            )
            .unwrap();
        if let Some(expected) = expected {
            assert_eq!(
                outcome.exit_code,
                expected,
                "{}",
                fs::read_to_string(&self.stderr).unwrap_or_default()
            );
        }
        outcome
    }
}

fn mcp_case() -> Value {
    sample_case(
        "mcp.stdio_unicode_ids_output_purity_and_incomplete_failure",
        || {
            let root = tempfile::tempdir().unwrap();
            let mut server = Mcp::start(root.path());
            server.send(json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"owned-client","version":"1"}}}));
            assert_eq!(server.reply()["id"], "init");
            server.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
            let mut ids_ok = true;
            for id in [json!(1), json!("1")] {
                let frame = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"echo","arguments":{"message":"кириллица"}}});
                let bytes = Message::parse(&serde_json::to_vec(&frame).unwrap())
                    .unwrap()
                    .encode()
                    .unwrap();
                for byte in bytes {
                    server.send_bytes(&[byte]);
                }
                let reply = server.reply();
                ids_ok &= reply["id"] == id
                    && reply["result"]["structuredContent"]["message"] == "кириллица";
            }
            server.send(json!({"jsonrpc":"2.0","id":"list","method":"tools/list"}));
            let tools = server.reply()["result"]["tools"].as_array().unwrap().len();
            drop(server.input.take());
            let stderr = fs::read_to_string(&server.stderr).unwrap_or_default();
            let clean = server.wait(Some(0));
            let fail_root = tempfile::tempdir().unwrap();
            let mut failing = Mcp::start(fail_root.path());
            failing.send_bytes(br#"{"jsonrpc":"2.0""#);
            drop(failing.input.take());
            let fail = failing.wait(None);
            json!({"passed": ids_ok && tools == 5 && stderr.is_empty() && clean.reason == StopReason::Exited && fail.reason == StopReason::Exited && fail.exit_code != 0, "exit": 0, "tools": tools, "incomplete_exit": fail.exit_code})
        },
    )
}

fn process_case() -> Value {
    sample_case("process.timeout_streams_and_foreign_preservation", || {
        let root = tempfile::Builder::new()
            .prefix("harness-baseline-process-")
            .tempdir()
            .unwrap()
            .keep();
        let fixture = PathBuf::from(env!("CARGO_BIN_EXE_harness-process-fixture"));
        let foreign_marker = root.join("foreign.json");
        let mut foreign = Command::new(&fixture)
            .args(["hold", foreign_marker.to_str().unwrap()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let ready = Instant::now() + Duration::from_secs(10);
        while !foreign_marker.is_file() && Instant::now() < ready {
            std::thread::sleep(Duration::from_millis(10));
        }
        let job = Job::new(Limits::default()).unwrap();
        let marker = root.join("tree.json");
        let mut spec = CommandSpec::new(&fixture);
        spec.args = vec!["tree-hold".into(), marker.clone().into()];
        let child = job.spawn(&spec).unwrap();
        let receipt_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match fs::read(&marker) {
                Ok(_) => break,
                Err(_) if Instant::now() < receipt_deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("{error}"),
            }
        }
        let outcome = job
            .wait(
                &child,
                Deadline::after(Duration::from_millis(100)).unwrap(),
                &Cancellation::default(),
                CLEANUP,
            )
            .unwrap();
        let foreign_alive = matches!(foreign.try_wait(), Ok(None));
        let _ = foreign.kill();
        let _ = foreign.wait();
        let stream_job = Job::new(Limits::default()).unwrap();
        let stream_marker = root.join("stream.json");
        let mut stream = CommandSpec::new(&fixture);
        stream.args = vec!["streams".into(), stream_marker.clone().into()];
        stream
            .env
            .insert("HARNESS_PROCESS_STREAM_TEST".into(), Some("маркер".into()));
        fs::write(root.join("stdin.txt"), "hello stdin").unwrap();
        stream.stdin = Some(fs::File::open(root.join("stdin.txt")).unwrap());
        stream.stdout = Some(fs::File::create(root.join("stdout.txt")).unwrap());
        stream.stderr = Some(fs::File::create(root.join("stderr.txt")).unwrap());
        let stream_child = stream_job.spawn(&stream).unwrap();
        let stream_out = stream_job
            .wait(
                &stream_child,
                Deadline::after(Duration::from_secs(10)).unwrap(),
                &Cancellation::default(),
                CLEANUP,
            )
            .unwrap();
        json!({"passed": outcome.reason == StopReason::Timeout && outcome.exit_code == 124 && outcome.job.active_processes == 0 && foreign_alive && stream_out.exit_code == 23, "exit": outcome.exit_code, "stream_exit": stream_out.exit_code, "peak_job_memory_bytes": outcome.job.peak_job_memory_bytes})
    })
}

fn console_case() -> Value {
    sample_case("console.unicode_stdin_nonzero_and_cancellation", || {
        let root = tempfile::Builder::new()
            .prefix("harness-baseline-console-")
            .tempdir()
            .unwrap()
            .keep();
        let fixture = PathBuf::from(env!("CARGO_BIN_EXE_harness-console-fixture"));
        let marker = root.join("result.json");
        let mut command = CommandSpec::new(&fixture);
        command.args = vec!["report".into(), marker.clone().into(), "кириллица".into()];
        command.current_dir = Some(root.clone());
        command
            .env
            .insert("HARNESS_CONSOLE_MARKER".into(), Some("маркер".into()));
        let session = ConsoleSession::spawn(ConsoleSpec::new(command)).unwrap();
        session.send("first line\rвторая строка\r\x1a\r").unwrap();
        let result = session
            .wait(
                Deadline::after(Duration::from_secs(15)).unwrap(),
                &Cancellation::default(),
                CLEANUP,
            )
            .unwrap();
        let receipt: Value =
            serde_json::from_slice(&fs::read(&marker).unwrap_or_default()).unwrap_or(json!({}));
        let mut fail = CommandSpec::new(&fixture);
        fail.args = vec!["nonzero".into()];
        let fail_out = ConsoleSession::spawn(ConsoleSpec::new(fail))
            .unwrap()
            .wait(
                Deadline::after(Duration::from_secs(10)).unwrap(),
                &Cancellation::default(),
                CLEANUP,
            )
            .unwrap();
        let mut hold = CommandSpec::new(&fixture);
        hold.args = vec!["hold".into(), root.join("hold.json").into()];
        let cancel = Cancellation::default();
        cancel.cancel();
        let cancelled = ConsoleSession::spawn(ConsoleSpec::new(hold))
            .unwrap()
            .wait(
                Deadline::after(Duration::from_secs(10)).unwrap(),
                &cancel,
                CLEANUP,
            )
            .unwrap();
        json!({"passed": result.outcome.reason == StopReason::Exited && fail_out.outcome.exit_code == 19 && cancelled.outcome.reason == StopReason::Cancelled && cancelled.outcome.exit_code == 130 && receipt["marker"] == "маркер", "exit": result.outcome.exit_code, "nonzero_exit": fail_out.outcome.exit_code, "cancel_exit": cancelled.outcome.exit_code})
    })
}

fn resolve_node() -> PathBuf {
    env::split_paths(&env::var_os("PATH").unwrap())
        .filter(|entry| {
            !entry
                .to_string_lossy()
                .to_ascii_lowercase()
                .contains("windowsapps")
        })
        .map(|entry| entry.join("node.exe"))
        .find(|path| path.is_file())
        .expect("node.exe on PATH outside WindowsApps")
}

fn subscription_case() -> Value {
    let node = resolve_node();
    sample_case(
        "subscription.bounded_node_fixture_nonzero_and_timeout",
        || {
            let root = tempfile::Builder::new()
                .prefix("harness-baseline-subscription-")
                .tempdir()
                .unwrap();
            let fixture = root.path().join("fixture.cjs");
            fs::write(&fixture, "const fs=require('node:fs');\nconst [mode,...args]=process.argv.slice(2);\nif(mode==='normal'){console.log(JSON.stringify({argv:args}));console.error('fixture stderr');process.exitCode=7;}\nelse if(mode==='linger'){fs.writeFileSync(args[0], String(process.pid)); setInterval(()=>{},1000);}\n").unwrap();
            let request = root.path().join("normal.request.json");
            fs::write(&request, serde_json::to_vec(&json!({"executable": node, "arguments": [fixture, "normal", "проверка"], "workingDirectory": root.path(), "stdoutPath": root.path().join("normal.stdout"), "stderrPath": root.path().join("normal.stderr"), "memoryLimitMiB": 160, "timeoutSeconds": 10, "environment": {"CODEX_BOUNDED_TEST": "маркер"}})).unwrap()).unwrap();
            let result_path = root.path().join("normal.result.json");
            let output = Command::new(desktop_pwsh())
                .args(["-NoLogo", "-NoProfile", "-File"])
                .arg(checkout().join("tools/opencodex-process.ps1"))
                .arg("-RequestPath")
                .arg(&request)
                .arg("-ResultPath")
                .arg(&result_path)
                .output()
                .unwrap();
            let result: Value = serde_json::from_slice(&fs::read(&result_path).unwrap_or_default())
                .unwrap_or(json!({}));
            let linger_request = root.path().join("linger.request.json");
            fs::write(&linger_request, serde_json::to_vec(&json!({"executable": node, "arguments": [fixture, "linger", root.path().join("linger.pid")], "workingDirectory": root.path(), "stdoutPath": root.path().join("linger.stdout"), "stderrPath": root.path().join("linger.stderr"), "memoryLimitMiB": 160, "timeoutSeconds": 1, "environment": {}})).unwrap()).unwrap();
            let linger = Command::new(desktop_pwsh())
                .args(["-NoLogo", "-NoProfile", "-File"])
                .arg(checkout().join("tools/opencodex-process.ps1"))
                .arg("-RequestPath")
                .arg(&linger_request)
                .output()
                .unwrap();
            json!({"passed": output.status.code() == Some(7) && (result["ExitCode"] == 7 || result["exitCode"] == 7) && linger.status.code() != Some(0), "exit": output.status.code(), "timeout_exit": linger.status.code()})
        },
    )
}

fn rtk_case() -> Value {
    sample_case("rtk.exec_once_hook_rewrite_and_malformed_silence", || {
        let root = tempfile::tempdir().unwrap();
        let adapter = rtk_adapter();
        let local = root.path().join("harness-rtk.exe");
        fs::copy(&adapter, &local).unwrap();
        fs::write(
            local.with_file_name("rtk.exe"),
            b"not the real rtk dependency",
        )
        .unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let _ = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&workspace)
            .status()
            .unwrap();
        let exec = Command::new(&adapter)
            .args(["exec", "git", "status", "--short"])
            .current_dir(&workspace)
            .output()
            .unwrap();
        let mut hook = Command::new(&local)
            .arg("hook")
            .current_dir(&workspace)
            .env("CODEX_HOME", root.path().join("codex"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        hook.stdin.take().unwrap().write_all(serde_json::to_vec(&json!({"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"harness-rtk.exe exec git status --short"}})).unwrap().as_slice()).unwrap();
        let hook_out = hook.wait_with_output().unwrap();
        let rewritten = serde_json::from_slice::<Value>(&hook_out.stdout).unwrap_or(json!({}));
        let mut malformed = Command::new(&adapter)
            .arg("hook")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        malformed
            .stdin
            .take()
            .unwrap()
            .write_all(b"not-json")
            .unwrap();
        let malformed_out = malformed.wait_with_output().unwrap();
        json!({"passed": exec.status.success() && hook_out.status.success() && rewritten["hookSpecificOutput"]["updatedInput"]["command"] == "harness-rtk.exe compact git status --short" && malformed_out.status.success() && malformed_out.stdout.is_empty(), "exit": exec.status.code(), "hook_exit": hook_out.status.code(), "malformed_exit": malformed_out.status.code()})
    })
}
