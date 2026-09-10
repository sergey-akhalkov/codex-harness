//! Read-only inventory through the real manager; package files are inert data.
#![cfg(windows)]
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    home: PathBuf,
    bin: PathBuf,
}
impl Fixture {
    fn select_legacy_cbm(&self) {
        let path = self.source.join("global/code-tools.json");
        let mut catalogue: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let provider = catalogue["mcp"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["id"] == "codegraph")
            .unwrap();
        *provider = json!({"id":"codebase-memory","package":"codebase-memory-mcp",
            "manager":"npm","source":"https://github.com/DeusData/codebase-memory-mcp",
            "metadata":"https://registry.npmjs.org/codebase-memory-mcp/latest"});
        fs::write(path, serde_json::to_vec(&catalogue).unwrap()).unwrap();
    }
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("dependency-observation проверка-")
            .tempdir()
            .unwrap();
        let source = root.path().join("source");
        let home = root.path().join("absent-home");
        let bin = root.path().join("bin");
        fs::create_dir_all(source.join("global")).unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global/code-tools.json"),
            source.join("global/code-tools.json"),
        )
        .unwrap();
        fs::create_dir(&bin).unwrap();
        fs::write(bin.join("node.exe"), b"inert test runtime, not executable").unwrap();
        Self {
            root,
            source,
            home,
            bin,
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        command
            .args(["dependencies", "discover", "--source"])
            .arg(&self.source)
            .arg("--user-home")
            .arg(&self.home)
            .current_dir(self.root.path());
        command
    }
    fn observe(&self, extra: &[&str]) -> Value {
        let output = self
            .command()
            .args(extra)
            .env("PATH", &self.bin)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn npm(&self, prefix: &Path, name: &str, command: &str) -> PathBuf {
        let root = prefix.join("node_modules").join(name);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("package.json"),
            serde_json::to_vec(
                &json!({"name":name,"version":"1.2.3","bin":{(command):"entry.js"}}),
            )
            .unwrap(),
        )
        .unwrap();
        fs::write(root.join("entry.js"), "inert package entrypoint data").unwrap();
        root
    }
}
fn record<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["mcp"]
        .as_array()
        .unwrap()
        .iter()
        .chain(report["languages"].as_array().unwrap())
        .find(|row| row["id"] == id)
        .unwrap()
}

#[test]
fn explicit_plan_preserves_unresolved_sources_without_network_or_secret_output() {
    let fixture = Fixture::new();
    let path = fixture.source.join("global/code-tools.json");
    let mut catalogue: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for group in ["mcp", "languages"] {
        for spec in catalogue[group].as_array_mut().unwrap() {
            spec["metadata"] =
                json!("https://PRIVATE_CREDENTIAL@127.0.0.1/not-an-official-endpoint");
        }
    }
    fs::write(path, serde_json::to_vec(&catalogue).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["dependencies", "plan", "--source"])
        .arg(&fixture.source)
        .arg("--user-home")
        .arg(&fixture.home)
        .env("PATH", &fixture.bin)
        .current_dir(fixture.root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_CREDENTIAL"));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mode"], "plan");
    assert_eq!(report["model_calls"], 0);
    assert_eq!(report["read_only"], true);
    assert_eq!(report["network_requested"], true);
    let items = report["items"].as_array().unwrap();
    assert_eq!(items.len(), 6);
    for item in items {
        assert_eq!(item["release"]["state"], "unresolved");
        assert!(item["release"]["version"].is_null());
        assert!(item["release"]["checked_at"].as_str().is_some());
    }
    assert!(
        !fixture.home.exists(),
        "read-only planning created the absent dependency home"
    );
}

#[test]
#[ignore = "explicit public HTTPS release metadata for the six selected dependencies"]
fn actual_official_release_plan_retains_missing_home_and_requires_later_acceptance() {
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["dependencies", "plan", "--source"])
        .arg(&fixture.source)
        .arg("--user-home")
        .arg(&fixture.home)
        .env("PATH", &fixture.bin)
        .current_dir(fixture.root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mode"], "plan");
    assert_eq!(
        report["metadata_transport"]["client"],
        "Windows system curl"
    );
    assert_eq!(report["model_calls"], 0);
    assert_eq!(report["read_only"], true);
    let items = report["items"].as_array().unwrap();
    assert_eq!(items.len(), 6);
    for item in items {
        assert_eq!(
            item["release"]["state"], "checked",
            "{}: {}",
            item["id"], item["release"]
        );
        assert!(item["release"]["version"].as_str().is_some());
        assert_eq!(item["action"], "install-required");
    }
    assert!(!fixture.home.exists());
}

#[test]
fn foreign_home_never_adopts_ambient_runtime_or_creates_missing_directories() {
    let fixture = Fixture::new();
    fixture.npm(&fixture.bin, "basedpyright", "basedpyright-langserver");
    let output = fixture
        .command()
        .env("PATH", &fixture.bin)
        .env("NPM_CONFIG_PREFIX", &fixture.bin)
        .env("UV_TOOL_DIR", "Z:/invalid-ambient-uv")
        .env("RUSTUP_HOME", "Z:/invalid-ambient-rust")
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["processes_started"], 0);
    assert_eq!(report["model_calls"], 0);
    assert_eq!(report["languages"].as_array().unwrap().len(), 2);
    assert_eq!(report["mcp"].as_array().unwrap().len(), 4);
    for id in [
        "serena",
        "graphify",
        "codegraph",
        "nuphus",
        "python",
        "rust",
    ] {
        assert_eq!(record(&report, id)["status"], "missing", "{id}: {report}");
    }
    assert!(!fixture.home.exists());
    assert!(report["observed_at"].as_str().unwrap().ends_with('Z'));
}

#[test]
fn discover_reports_missing_codegraph_without_mutating_or_downloading() {
    let fixture = Fixture::new();
    let report = fixture.observe(&["--no-process-environment"]);
    assert_eq!(report["mcp"].as_array().unwrap().len(), 4);
    assert_eq!(
        report["mcp"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["id"] == "codegraph")
            .count(),
        1
    );
    assert!(
        !report["mcp"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "codebase-memory")
    );
    let row = record(&report, "codegraph");
    assert_eq!(row["status"], "missing");
    assert_eq!(row["identity"], "@colbymchenry/codegraph");
    assert_eq!(row["health"]["callable"], Value::Null);
    assert_eq!(
        report["release_checks"],
        "not-requested; explicit lifecycle operation required"
    );
    assert_eq!(report["processes_started"], 0);
}

#[test]
fn inspect_codegraph_rejects_synthetic_layout_without_network() {
    let fixture = Fixture::new();
    let pkg = fixture.root.path().join("fake-codegraph");
    fs::create_dir_all(pkg.join("lib/dist/bin")).unwrap();
    fs::create_dir_all(pkg.join("lib/kernel")).unwrap();
    fs::create_dir_all(pkg.join("lib/node_modules")).unwrap();
    fs::write(pkg.join("node.exe"), b"not-official-node").unwrap();
    fs::write(pkg.join("lib/dist/bin/codegraph.js"), b"not-official-entry").unwrap();
    fs::write(
        pkg.join("lib/kernel/codegraph-kernel.node"),
        b"not-official-kernel",
    )
    .unwrap();
    fs::write(
        pkg.join("lib/package.json"),
        br#"{"name":"@colbymchenry/codegraph","version":"1.6.0"}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["dependencies", "inspect-codegraph", "--package-root"])
        .arg(&pkg)
        .current_dir(fixture.root.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn native_payloads_are_observed_without_shims_or_runtimes_and_versions_must_match() {
    let fixture = Fixture::new();
    fixture.select_legacy_cbm();
    let prefix = fixture.home.join("AppData/Roaming/npm");
    let codebase = fixture.npm(&prefix, "codebase-memory-mcp", "codebase-memory-mcp");
    let nuphus = fixture.npm(&prefix, "@nuphus/nuphus-mcp", "nuphus-mcp");
    let absent = fixture.observe(&["--no-process-environment"]);
    assert_eq!(record(&absent, "codebase-memory")["status"], "broken");
    assert_eq!(record(&absent, "nuphus")["status"], "broken");
    fs::create_dir(codebase.join("bin")).unwrap();
    fs::write(
        codebase.join("bin/codebase-memory-mcp.exe"),
        b"inert codebase payload",
    )
    .unwrap();
    let companion = fixture.npm(&prefix, "@nuphus/nuphus-mcp-win32-x64", "unused");
    fs::create_dir(companion.join("bin")).unwrap();
    let native = companion.join("bin/nuphus-mcp.exe");
    fs::write(&native, b"inert native companion").unwrap();
    fs::write(companion.join("bin/onnxruntime.dll"), b"inert library").unwrap();
    let observed = fixture.observe(&["--no-process-environment"]);
    for id in ["codebase-memory", "nuphus"] {
        let row = record(&observed, id);
        assert_eq!(row["status"], "adopted");
        assert_eq!(row["command"].as_array().unwrap().len(), 1);
        assert!(row["command"][0].as_str().unwrap().ends_with(".exe"));
        assert_eq!(row["health"]["callable"], Value::Null);
        assert_eq!(row["update_safe"], false);
    }
    assert_eq!(
        record(&observed, "nuphus")["health"]["onnxruntime_exists"],
        true
    );
    fs::write(
        companion.join("bin/nuphus-mcp-schema-fixed.exe"),
        b"inert locally modified payload",
    )
    .unwrap();
    assert_eq!(
        record(&fixture.observe(&[]), "nuphus")["status"],
        "modified"
    );
    let manifest = companion.join("package.json");
    let mut changed: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    changed["version"] = json!("9.9.9");
    fs::write(&manifest, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert_eq!(record(&fixture.observe(&[]), "nuphus")["status"], "broken");
    assert!(native.exists() && nuphus.join("entry.js").exists());
}

#[test]
fn package_identity_escape_ambiguity_aliases_and_partial_failures_stay_distinct() {
    let fixture = Fixture::new();
    let first = fixture.home.join("AppData/Roaming/npm");
    let root = fixture.npm(&first, "basedpyright", "basedpyright-langserver");
    let second = fixture.root.path().join("second-npm");
    fixture.npm(&second, "basedpyright", "basedpyright-langserver");
    let output = fixture
        .command()
        .args(["--include-process-environment", "--npm-prefix"])
        .arg(&second)
        .env("PATH", &fixture.bin)
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record(&report, "python")["status"], "ambiguous");
    assert_eq!(record(&report, "python")["ownership"], "selection-required");
    let alias = fixture.root.path().join("alias-npm");
    std::os::windows::fs::symlink_dir(&first, &alias).unwrap();
    let output = fixture
        .command()
        .args(["--include-process-environment", "--npm-prefix"])
        .arg(&alias)
        .env("PATH", &fixture.bin)
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record(&report, "python")["status"], "adopted");
    assert_eq!(
        record(&report, "python")["candidates"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let manifest = root.join("package.json");
    let mut with_bom = vec![0xef, 0xbb, 0xbf];
    with_bom.extend(fs::read(&manifest).unwrap());
    with_bom.extend(b"\r\n");
    fs::write(&manifest, &with_bom).unwrap();
    let observed = fixture.observe(&["--include-process-environment"]);
    let identity = record(&observed, "python")["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .find(|evidence| evidence["kind"] == "npm-package-identity")
        .unwrap();
    assert_eq!(
        identity["sha256"],
        harness_core::build_identity::hash_bytes(&with_bom)
    );
    fs::write(&manifest,serde_json::to_vec(&json!({"name":"unrelated","version":"1.0","bin":{"basedpyright-langserver":"entry.js"}})).unwrap()).unwrap();
    assert_eq!(
        record(
            &fixture.observe(&["--include-process-environment"]),
            "python"
        )["status"],
        "missing"
    );
    let outside = fixture.root.path().join("private-outside.js");
    fs::write(&outside, "private outside payload sentinel").unwrap();
    fs::write(&manifest,serde_json::to_vec(&json!({"name":"basedpyright","version":"1.0","bin":{"basedpyright-langserver":outside}})).unwrap()).unwrap();
    let report = fixture.observe(&["--include-process-environment"]);
    assert_eq!(record(&report, "python")["status"], "broken");
    assert!(
        record(&report, "python")["command"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fs::write(&manifest, "{ invalid private parser sentinel").unwrap();
    let report = fixture.observe(&["--include-process-environment"]);
    assert_eq!(record(&report, "python")["status"], "incomplete");
    assert_eq!(record(&report, "graphify")["status"], "missing");
    assert!(!report.to_string().contains("private parser sentinel"));
    assert_eq!(
        fs::read_to_string(outside).unwrap(),
        "private outside payload sentinel"
    );
}

#[test]
fn rustup_metadata_and_saved_graph_are_read_without_executing_or_leaking_credentials() {
    let fixture = Fixture::new();
    let rustup = fixture.home.join(".rustup");
    let bin = rustup.join("toolchains/1.97.1-x86_64-pc-windows-msvc/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        rustup.join("settings.toml"),
        "default_toolchain = '1.97.1-x86_64-pc-windows-msvc'\n",
    )
    .unwrap();
    fs::write(bin.join("rust-analyzer.exe"), b"inert analyzer, never run").unwrap();
    let graph = fixture.root.path().join("owned-graph.json");
    fs::write(&graph, "owned inert graph").unwrap();
    let manifest = fixture.root.path().join("graph-service.json");
    fs::write(&manifest,serde_json::to_vec(&json!({"credentials":"private credential sentinel","graphify":{"configuration":{"graph":{"path":graph},"python":{"path":fixture.bin.join("node.exe")},"module":{"name":"graphify.serve","packageVersion":"1.2.3","token":"private module sentinel"},"headers":{"Authorization":"private authorization sentinel"}}}})).unwrap()).unwrap();
    let output = fixture
        .command()
        .arg("--graphify-manifest")
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(record(&report, "rust")["status"], "adopted");
    assert_eq!(
        record(&report, "rust")["health"]["identity_verified"],
        false
    );
    assert_eq!(record(&report, "rust")["version"], Value::Null);
    assert_eq!(
        record(&report, "graphify")["shared_service"]["graph_exists"],
        true
    );
    for private in [
        "private credential sentinel",
        "private module sentinel",
        "private authorization sentinel",
    ] {
        assert!(!String::from_utf8_lossy(&output.stdout).contains(private));
    }
    assert_eq!(fs::read_to_string(graph).unwrap(), "owned inert graph");
}

#[test]
fn uv_console_identity_record_modification_and_duplicate_distribution_are_observed() {
    let fixture = Fixture::new();
    let root = fixture.home.join("AppData/Roaming/uv/tools/serena-agent");
    let site = root.join("Lib/site-packages");
    let dist = site.join("serena_agent-1.7.0.dist-info");
    fs::create_dir_all(&dist).unwrap();
    fs::create_dir_all(site.join("serena")).unwrap();
    fs::create_dir_all(root.join("Scripts")).unwrap();
    fs::write(dist.join("METADATA"),"Metadata-Version: 2.4\nName: Serena_Agent\nVersion: 1.7.0\n\nprivate package description sentinel").unwrap();
    fs::write(
        dist.join("entry_points.txt"),
        "[console_scripts]\nserena = serena.cli:top_level\n",
    )
    .unwrap();
    fs::write(site.join("serena/cli.py"), b"abc").unwrap();
    fs::write(root.join("Scripts/python.exe"), b"inert Python runtime").unwrap();
    fs::write(
        root.join("Scripts/serena.exe"),
        b"inert external console entry",
    )
    .unwrap();
    fs::write(
        root.join("uv-receipt.toml"),
        "[tool]\nrequirements = [{name = 'serena-agent'}]\n",
    )
    .unwrap();
    // Published SHA-256 of the literal three bytes 'abc'; independent fixed oracle.
    fs::write(
        dist.join("RECORD"),
        "serena/cli.py,sha256=ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0,3\n",
    )
    .unwrap();
    let report = fixture.observe(&[]);
    let serena = record(&report, "serena");
    assert_eq!(serena["status"], "adopted", "{report}");
    assert_eq!(serena["health"]["integrity"], "record-matches");
    assert_eq!(serena["update_safe"], true);
    assert!(
        serena["command"][0]
            .as_str()
            .unwrap()
            .ends_with("serena.exe")
    );
    assert!(
        !report
            .to_string()
            .contains("private package description sentinel")
    );
    fs::write(site.join("serena/cli.py"), b"modified inert module").unwrap();
    let report = fixture.observe(&[]);
    assert_eq!(record(&report, "serena")["status"], "modified");
    assert_eq!(record(&report, "serena")["update_safe"], false);
    fs::write(site.join("serena/cli.py"), b"abc").unwrap();
    fs::write(
        dist.join("entry_points.txt"),
        "[console_scripts]\nserena = unrelated.module:main\n",
    )
    .unwrap();
    assert_eq!(record(&fixture.observe(&[]), "serena")["status"], "broken");
    fs::write(
        dist.join("entry_points.txt"),
        "[console_scripts]\nserena = serena.cli:top_level\n",
    )
    .unwrap();
    let duplicate = site.join("serena_agent-0.1.dist-info");
    fs::create_dir(&duplicate).unwrap();
    fs::write(
        duplicate.join("METADATA"),
        "Name: serena-agent\nVersion: 0.1\n",
    )
    .unwrap();
    assert_eq!(
        record(&fixture.observe(&[]), "serena")["status"],
        "incomplete"
    );
}

#[test]
fn version_execution_is_opt_in_isolated_bounded_and_withholds_failures() {
    let fixture = Fixture::new();
    let rustup = fixture.home.join(".rustup");
    let bin = rustup.join("toolchains/owned-fixture/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        rustup.join("settings.toml"),
        "default_toolchain = 'owned-fixture'\n",
    )
    .unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_harness-launch-fixture"),
        bin.join("rust-analyzer.exe"),
    )
    .unwrap();
    let marker = fixture.root.path().join("version.marker.json");
    let observed = fixture
        .command()
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "dependency-version")
        .env("HARNESS_LAUNCH_FIXTURE_MARKER", &marker)
        .output()
        .unwrap();
    assert!(observed.status.success());
    assert!(!marker.exists());
    let report: Value = serde_json::from_slice(&observed.stdout).unwrap();
    assert_eq!(report["processes_started"], 0);
    for scenario in ["success", "private-error", "timeout"] {
        let start = std::time::Instant::now();
        let observed = fixture
            .command()
            .arg("--probe-versions")
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "dependency-version")
            .env("HARNESS_DEPENDENCY_VERSION_FIXTURE", scenario)
            .env("HARNESS_LAUNCH_FIXTURE_MARKER", &marker)
            .output()
            .unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(15));
        assert!(observed.status.success(), "{:?}", observed.stderr);
        assert!(observed.stderr.is_empty());
        let report: Value = serde_json::from_slice(&observed.stdout).unwrap();
        let row = record(&report, "rust");
        if scenario == "success" {
            assert_eq!(row["version"], "rust-analyzer 1.97.1 (owned fixture)");
            assert_eq!(row["health"]["identity_verified"], true);
        } else {
            assert_eq!(row["version"], Value::Null);
        }
        assert!(!report.to_string().contains("private version"));
        let observation: Value = serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
        assert_eq!(observation["args"], json!(["--version"]));
        assert_eq!(observation["auto_install"], "0");
        let cwd = Path::new(observation["cwd"].as_str().unwrap());
        assert!(
            !cwd.exists(),
            "private process files were not removed: {cwd:?}"
        );
        assert!(Path::new(observation["rustup_home"].as_str().unwrap()).starts_with(cwd));
        assert!(Path::new(observation["cargo_home"].as_str().unwrap()).starts_with(cwd));
    }
    assert_eq!(
        fs::read_to_string(rustup.join("settings.toml")).unwrap(),
        "default_toolchain = 'owned-fixture'\n"
    );
}

#[test]
fn cli_process_observation_sees_owned_consumer_without_disclosing_arguments_or_stopping_it() {
    use std::process::Stdio;
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let fixture = Fixture::new();
    fixture.select_legacy_cbm();
    let prefix = fixture.home.join("AppData/Roaming/npm");
    let root = fixture.npm(&prefix, "codebase-memory-mcp", "codebase-memory-mcp");
    fs::create_dir(root.join("bin")).unwrap();
    let binary = root.join("bin/codebase-memory-mcp.exe");
    fs::copy(env!("CARGO_BIN_EXE_harness-launch-fixture"), &binary).unwrap();
    let pid_file = fixture.root.path().join("consumer.pid");
    let mut child = ChildGuard(
        Command::new(&binary)
            .args(["--private-token", "PRIVATE-CONSUMER-ARGUMENT-471b"])
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "report")
            .env("HARNESS_LAUNCH_FIXTURE_STARTED", &pid_file)
            .env("PRIVATE_CONSUMER_ENV", "PRIVATE-CONSUMER-ENVIRONMENT-726a")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !pid_file.exists() {
        assert!(std::time::Instant::now() < deadline);
        assert!(child.0.try_wait().unwrap().is_none());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let report = fixture.observe(&["--processes", "--no-process-environment"]);
    assert_eq!(report["processes_started"], 0);
    assert_eq!(report["process_inspection_requested"], true);
    let consumers = &record(&report, "codebase-memory")["active_consumers"];
    assert!(
        matches!(consumers["state"].as_str(), Some("observed" | "incomplete")),
        "{consumers}"
    );
    let process = consumers["processes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|process| process["pid"] == child.0.id())
        .expect("owned package consumer");
    assert!(process["creation_time"].as_u64().unwrap() > 0);
    assert_eq!(process.as_object().unwrap().len(), 4);
    for private in [
        "PRIVATE-CONSUMER-ARGUMENT-471b",
        "PRIVATE-CONSUMER-ENVIRONMENT-726a",
    ] {
        assert!(!report.to_string().contains(private));
    }
    assert!(
        child.0.try_wait().unwrap().is_none(),
        "observation stopped its consumer"
    );
    assert!(binary.exists());
}
