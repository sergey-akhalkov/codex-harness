#![cfg(windows)]
use harness_core::{
    dependency_mcp_probe::{ProbeKind, catalogue, probe, probe_with_cancellation},
    process::Cancellation,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    os::windows::{
        fs::{OpenOptionsExt, symlink_file},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE},
    System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
};

struct Fixture {
    dir: tempfile::TempDir,
    executable: PathBuf,
    digest: String,
    kind: ProbeKind,
}
impl Fixture {
    fn new(mode: &str, kind: ProbeKind) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("dependency-mcp-probe-кириллица ")
            .tempdir()
            .unwrap();
        let executable = dir.path().join("fixture.exe");
        fs::copy(env!("CARGO_BIN_EXE_harness-mcp-probe-fixture"), &executable).unwrap();
        let digest = format!("{:x}", Sha256::digest(fs::read(&executable).unwrap()));
        fs::write(executable.with_extension("json"), serde_json::to_vec(&json!({"mode":mode,"kind":if kind==ProbeKind::CodebaseMemory {"cbm"} else {"nuphus"}})).unwrap()).unwrap();
        Self {
            dir,
            executable,
            digest,
            kind,
        }
    }
    fn run(&self) -> io::Result<Value> {
        probe(&self.executable, self.kind, &self.digest)
    }
    fn catalogue(&self) -> io::Result<Value> {
        let config_path = self.executable.with_extension("json");
        let mut config: Value = serde_json::from_slice(&fs::read(&config_path)?)?;
        config["catalogue_only"] = json!(true);
        fs::write(config_path, serde_json::to_vec(&config)?)?;
        catalogue(
            &self.executable,
            self.kind,
            &self.digest,
            &Cancellation::default(),
        )
    }
    fn receipt(&self) -> Value {
        serde_json::from_slice(&fs::read(self.executable.with_extension("receipt.json")).unwrap())
            .unwrap()
    }
    fn removed(&self) {
        let receipt = self.receipt();
        assert!(
            !Path::new(receipt["cwd"].as_str().unwrap()).exists(),
            "private state survived"
        );
        assert!(self.dir.path().is_dir(), "foreign test parent removed");
    }
    fn failure(&self, reason: &str) {
        let start = Instant::now();
        let error = self.run().expect_err(reason);
        assert!(
            error.to_string().contains(reason),
            "expected {reason}, got {error}"
        );
        assert!(!error.to_string().contains("SECRET_FOREIGN_TOKEN"));
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "fast failure stalled"
        );
        self.removed();
    }
}

#[test]
fn catalogue_preserves_complete_definitions_without_executing_tools() {
    for kind in [ProbeKind::CodebaseMemory, ProbeKind::Nuphus] {
        for mode in ["valid", "fragmented", "paged"] {
            let fixture = Fixture::new(mode, kind);
            let report = fixture
                .catalogue()
                .unwrap_or_else(|error| panic!("{kind:?}/{mode}: {error}"));
            assert_eq!(report["state"], "catalogue-read");
            assert_eq!(report["tool_calls_executed"], false);
            let definitions = report["tools"].as_array().unwrap();
            assert_eq!(
                definitions.len(),
                report["tool_count"].as_u64().unwrap() as usize
            );
            assert!(!definitions.is_empty());
            assert!(
                definitions
                    .iter()
                    .all(|tool| tool["description"] == "SECRET_FOREIGN_TOKEN"
                        && tool["inputSchema"].is_object())
            );
            let receipt = fixture.receipt();
            assert_eq!(receipt["stdin_eof"], true);
            assert!(receipt["calls"].as_array().unwrap().iter().all(|call| {
                ["initialize", "notifications/initialized", "tools/list"]
                    .contains(&call.as_str().unwrap())
            }));
            fixture.removed();
        }
    }
    for mode in [
        "duplicate-tool",
        "bad-schema",
        "cursor-loop",
        "duplicate-key",
        "malformed",
    ] {
        let fixture = Fixture::new(mode, ProbeKind::CodebaseMemory);
        let error = fixture.catalogue().unwrap_err();
        assert!(!error.to_string().contains("SECRET_FOREIGN_TOKEN"));
        fixture.removed();
    }
}

#[test]
fn nuphus_negotiates_catalogue_and_reports_only_private_bounded_summary() {
    for mode in ["valid", "fragmented", "paged", "fixed-schema"] {
        let fixture = Fixture::new(mode, ProbeKind::Nuphus);
        let summary = fixture.run().unwrap();
        assert_eq!(summary["state"], "protocol-ready");
        assert_eq!(summary["tool_count"], 38);
        assert_eq!(
            summary["selector_ref_branch_types_missing"],
            if mode == "fixed-schema" { 0 } else { 6 }
        );
        assert_eq!(summary["isolation"]["owned_tree_stopped"], true);
        assert_eq!(summary["limits"]["job_memory_bytes"], 512 * 1024 * 1024u64);
        assert_eq!(summary["schema_sha256"].as_str().unwrap().len(), 64);
        let encoded = summary.to_string();
        for foreign in [
            "SECRET_FOREIGN_TOKEN",
            fixture.dir.path().to_str().unwrap(),
            "instructions",
            "query_result",
        ] {
            assert!(!encoded.contains(foreign));
        }
        let receipt = fixture.receipt();
        for flag in [
            "in_job_at_entry",
            "private_directories",
            "owner_acl",
            "child_acl",
            "no_models",
            "ambient_absent",
            "no_breakaway",
            "kill_on_close",
            "cpu_hard_cap",
            "stdin_eof",
        ] {
            assert_eq!(receipt[flag], true, "{mode}: {flag}");
        }
        assert_eq!(receipt["memory_limit"], 512 * 1024 * 1024u64);
        assert_eq!(receipt["cpu_rate"], 2500);
        let calls = receipt["calls"].as_array().unwrap();
        assert_eq!(calls[0], "initialize");
        assert_eq!(calls[1], "notifications/initialized");
        assert!(calls[2..].iter().all(|c| c == "tools/list"));
        fixture.removed();
    }
}

#[test]
fn codebase_preserves_disposable_index_project_schema_and_exact_query_oracles() {
    for mode in ["valid", "text-payload"] {
        let fixture = Fixture::new(mode, ProbeKind::CodebaseMemory);
        let summary = fixture.run().unwrap();
        assert_eq!(summary["state"], "passed");
        assert_eq!(summary["isolated_project_count"], 1);
        assert_eq!(
            summary["checked_operations"],
            json!([
                "initialize",
                "notifications/initialized",
                "tools/list",
                "index_repository",
                "list_projects",
                "get_graph_schema",
                "query_graph"
            ])
        );
        assert_eq!(fixture.receipt()["memory_limit"], 2 * 1024 * 1024 * 1024u64);
        fixture.removed();
    }
    for (mode, reason) in [
        ("index-error", "tool operation failed"),
        ("multiple-projects", "project isolation failed"),
        ("wrong-root", "project root mismatch"),
        ("query-spoof", "graph query failed"),
        ("missing-mode", "index mode contract"),
    ] {
        Fixture::new(mode, ProbeKind::CodebaseMemory).failure(reason);
    }
}

#[test]
fn delayed_private_state_handle_release_still_removes_owned_tree() {
    let fixture = Fixture::new("hold-state", ProbeKind::CodebaseMemory);
    let executable = fixture.executable.clone();
    let digest = fixture.digest.clone();
    let worker = std::thread::spawn(move || probe(&executable, ProbeKind::CodebaseMemory, &digest));
    let receipt = wait_receipt(&fixture.executable);
    let cwd = PathBuf::from(receipt["cwd"].as_str().unwrap());
    let hold = cwd.join("held-by-test");
    let start = Instant::now();
    let handle = loop {
        if let Ok(file) = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&hold)
        {
            break file;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "hold file never appeared"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    fs::write(cwd.join("hold-ready"), b"ready").unwrap();
    let child = owned_handle(receipt["pid"].as_u64().unwrap());
    assert_eq!(
        unsafe { WaitForSingleObject(child.as_raw_handle(), 15_000) },
        WAIT_OBJECT_0
    );
    // Keep the first private-state removal failing with a sharing violation, then
    // release inside the five-second cleanup retry budget.
    std::thread::sleep(Duration::from_millis(400));
    drop(handle);
    let summary = worker.join().unwrap().unwrap();
    assert_eq!(summary["state"], "passed");
    assert_eq!(summary["isolation"]["temporary_state_removed"], true);
    assert_eq!(summary["isolation"]["private_cleanup_retried"], true);
    assert!(summary["isolation"]["private_cleanup_initial_os_error"].is_i64());
    fixture.removed();
}

#[test]
fn primary_protocol_error_keeps_failed_cleanup_visible() {
    let fixture = Fixture::new("hold-state-query-spoof", ProbeKind::CodebaseMemory);
    let executable = fixture.executable.clone();
    let digest = fixture.digest.clone();
    let worker = std::thread::spawn(move || probe(&executable, ProbeKind::CodebaseMemory, &digest));
    let receipt = wait_receipt(&fixture.executable);
    let cwd = PathBuf::from(receipt["cwd"].as_str().unwrap());
    let until = Instant::now() + Duration::from_secs(5);
    let handle = loop {
        if let Ok(file) = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(cwd.join("held-by-test"))
        {
            break file;
        }
        assert!(Instant::now() < until, "hold file never appeared");
        std::thread::sleep(Duration::from_millis(10));
    };
    fs::write(cwd.join("hold-ready"), b"ready").unwrap();
    let error = worker.join().unwrap().unwrap_err();
    let retained = cwd.join("held-by-test").exists();
    drop(handle);
    // This exact root came from the owned fixture's receipt and was held above.
    fs::remove_dir_all(&cwd).unwrap();
    fixture.removed();
    assert!(retained);
    assert!(error.to_string().contains("disposable graph query failed"));
    assert!(error.to_string().contains("cleanup failed"), "{error}");
    assert!(error.to_string().contains("retained for recovery"));
    assert!(!error.to_string().contains("SECRET_FOREIGN_TOKEN"));
}

#[test]
fn rejects_wrong_ids_server_requests_errors_and_ambiguous_json() {
    for (mode, reason) in [
        ("wrong-id", "response identity"),
        ("string-id", "response identity"),
        ("server-request", "response identity"),
        ("duplicate-key", "malformed JSON"),
        ("error", "returned an error"),
        ("protocol", "initialize contract"),
        ("missing-capability", "initialize contract"),
    ] {
        Fixture::new(mode, ProbeKind::Nuphus).failure(reason);
    }
}

#[test]
fn validates_full_catalogue_and_schema_without_calling_nuphus_tools() {
    for (mode, reason) in [
        ("missing-tool", "38-tool contract"),
        ("renamed-tool", "38-tool contract"),
        ("duplicate-tool", "duplicate or excessive"),
        ("bad-schema", "input schema"),
        ("bad-alias", "alternatives incompatible"),
        ("cursor-loop", "pagination limit"),
    ] {
        Fixture::new(mode, ProbeKind::Nuphus).failure(reason);
    }
}

#[test]
fn refuses_malformed_oversized_incomplete_and_trailing_output() {
    for (mode, reason) in [
        ("malformed", "malformed JSON"),
        ("invalid-utf8", "malformed JSON"),
        ("incomplete", "incomplete output"),
        ("oversized", "stdout byte limit"),
        ("stderr-flood", "stderr byte limit"),
        ("stdout-flood", "stdout byte limit"),
        ("notifications", "notification limit"),
        ("trailing-id", "after completion"),
        ("trailing-partial", "incomplete output"),
        ("early-eof", "EOF before response"),
        ("early-nonzero", "did not exit successfully"),
        ("late-nonzero", "did not exit successfully"),
    ] {
        Fixture::new(mode, ProbeKind::Nuphus).failure(reason);
    }
}

fn owned_handle(pid: u64) -> OwnedHandle {
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid as u32) };
    assert!(!handle.is_null());
    unsafe { OwnedHandle::from_raw_handle(handle) }
}
fn wait_receipt(executable: &Path) -> Value {
    let start = Instant::now();
    loop {
        if let Ok(bytes) = fs::read(executable.with_extension("receipt.json"))
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        {
            return value;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "fixture never entered"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn cancellation_stops_only_owned_root_and_descendant_with_inherited_pipes() {
    let fixture = Fixture::new("cancel", ProbeKind::Nuphus);
    let mut foreign = Command::new(env!("CARGO_BIN_EXE_harness-mcp-probe-fixture"))
        .arg("--linger")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let cancel = Cancellation::default();
    let executable = fixture.executable.clone();
    let digest = fixture.digest.clone();
    let token = cancel.clone();
    let worker = std::thread::spawn(move || {
        probe_with_cancellation(&executable, ProbeKind::Nuphus, &digest, &token)
    });
    let receipt = wait_receipt(&fixture.executable);
    let root = owned_handle(receipt["pid"].as_u64().unwrap());
    let child = owned_handle(receipt["child_pid"].as_u64().unwrap());
    // Retained process objects make the cleanup oracle independent of PID reuse.
    assert_eq!(
        unsafe { WaitForSingleObject(child.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(&fixture.executable)
            .is_err(),
        "artifact writable during probe"
    );
    let moved = fixture.dir.path().with_extension("moved");
    let renamed = fs::rename(fixture.dir.path(), &moved).is_ok();
    if renamed {
        fs::rename(&moved, fixture.dir.path()).unwrap();
    }
    assert!(!renamed, "artifact ancestor was not pinned");
    let start = Instant::now();
    cancel.cancel();
    let result = worker.join().unwrap();
    let foreign_alive = foreign.try_wait().unwrap().is_none();
    let _ = foreign.kill();
    let _ = foreign.wait();
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert!(start.elapsed() < Duration::from_secs(8));
    assert!(foreign_alive);
    for handle in [root, child] {
        assert_eq!(
            unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
            WAIT_OBJECT_0
        );
    }
    fixture.removed();
}

#[test]
fn deadline_bounds_silent_child_and_descendant_without_pipe_deadlock() {
    let fixture = Fixture::new("deadline", ProbeKind::Nuphus);
    let executable = fixture.executable.clone();
    let digest = fixture.digest.clone();
    let start = Instant::now();
    let worker = std::thread::spawn(move || probe(&executable, ProbeKind::Nuphus, &digest));
    let receipt = wait_receipt(&fixture.executable);
    let root = owned_handle(receipt["pid"].as_u64().unwrap());
    let child = owned_handle(receipt["child_pid"].as_u64().unwrap());
    assert_eq!(
        worker.join().unwrap().unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert!(
        start.elapsed() >= Duration::from_secs(24) && start.elapsed() < Duration::from_secs(33)
    );
    for handle in [root, child] {
        assert_eq!(
            unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
            WAIT_OBJECT_0
        );
    }
    fixture.removed();
}

#[test]
fn memory_limit_is_observed_and_owned_tree_removed() {
    let fixture = Fixture::new("memory", ProbeKind::Nuphus);
    fixture.failure("memory limit");
    assert_eq!(fixture.receipt()["allocation_denied"], true);
}

#[test]
fn normal_eof_and_early_root_exit_clean_surviving_descendants() {
    for mode in ["valid-tree", "root-exit"] {
        let fixture = Fixture::new(mode, ProbeKind::Nuphus);
        assert_eq!(
            fixture.run().unwrap()["isolation"]["owned_tree_stopped"],
            true
        );
        fixture.removed();
    }
}

#[test]
fn artifact_gate_refuses_missing_digest_mismatch_scripts_links_and_pre_cancel() {
    let fixture = Fixture::new("valid", ProbeKind::Nuphus);
    for digest in ["", "not-a-digest", &"0".repeat(64)] {
        assert!(probe(&fixture.executable, fixture.kind, digest).is_err());
    }
    assert!(probe(Path::new("fixture.exe"), fixture.kind, &fixture.digest).is_err());
    let script = fixture.dir.path().join("script.cmd");
    fs::write(&script, "exit /b 0").unwrap();
    let digest = format!("{:x}", Sha256::digest(fs::read(&script).unwrap()));
    assert!(probe(&script, fixture.kind, &digest).is_err());
    let fake = fixture.dir.path().join("script.exe");
    fs::write(&fake, vec![b'x'; 256]).unwrap();
    let digest = format!("{:x}", Sha256::digest(fs::read(&fake).unwrap()));
    assert!(probe(&fake, fixture.kind, &digest).is_err());
    let link = fixture.dir.path().join("linked.exe");
    symlink_file(&fixture.executable, &link).unwrap();
    assert!(probe(&link, fixture.kind, &fixture.digest).is_err());
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        probe_with_cancellation(&fixture.executable, fixture.kind, &fixture.digest, &cancel)
            .unwrap_err()
            .kind(),
        io::ErrorKind::Interrupted
    );
    assert!(!fixture.executable.with_extension("receipt.json").exists());
}

#[test]
fn ambient_tokens_browser_endpoint_and_proxy_are_removed_in_real_child() {
    let fixture = Fixture::new("valid", ProbeKind::Nuphus);
    let output = Command::new(&fixture.executable)
        .arg("--probe")
        .arg(&fixture.digest)
        .env("HARNESS_MCP_SECRET", "SECRET_FOREIGN_TOKEN")
        .env("OPENAI_API_KEY", "SECRET_FOREIGN_TOKEN")
        .env("NUPHUS_MCP_BROWSER_CDP_URL", "http://invalid.fixture/")
        .env("HTTP_PROXY", "http://invalid.fixture/")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["state"], "protocol-ready");
    assert!(output.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SECRET_FOREIGN_TOKEN"));
    assert_eq!(fixture.receipt()["ambient_absent"], true);
    fixture.removed();
}

#[test]
fn admits_canonical_windows_native_paths_without_path_search() {
    let fixture = Fixture::new("valid", ProbeKind::Nuphus);
    let summary = probe(
        &fixture.executable.canonicalize().unwrap(),
        fixture.kind,
        &fixture.digest,
    )
    .unwrap();
    assert_eq!(summary["state"], "protocol-ready");
    fixture.removed();
}

#[test]
#[ignore = "explicit installed artifact only; requires caller-audited original path and digest"]
fn installed_original_protocol_probe() {
    let executable =
        std::env::var_os("HARNESS_MCP_PROBE_EXE").expect("explicit native path required");
    let digest =
        std::env::var("HARNESS_MCP_PROBE_SHA256").expect("explicit audited digest required");
    let kind = match std::env::var("HARNESS_MCP_PROBE_KIND").as_deref() {
        Ok("codebase-memory") => ProbeKind::CodebaseMemory,
        Ok("nuphus") => ProbeKind::Nuphus,
        _ => panic!("explicit kind required"),
    };
    let summary = probe(Path::new(&executable), kind, &digest).unwrap();
    println!("{summary}");
}
