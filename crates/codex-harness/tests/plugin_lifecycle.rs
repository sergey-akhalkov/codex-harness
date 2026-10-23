#![cfg(windows)]

#[path = "fixtures/synthetic_role_build.rs"]
#[allow(dead_code)]
mod synthetic_role_build;

use harness_core::build_identity;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
};

const PLUGIN: &str = "executors@coding-agents-harness";
const SECRET_ASSIGNMENT: &str = "private-assignment-sentinel";
const SECRET_ENDPOINT: &str = "private-endpoint-token-sentinel";

struct PathRestore(Option<OsString>);

impl PathRestore {
    fn capture() -> Self {
        Self(env::var_os("PATH"))
    }
}

impl Drop for PathRestore {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            unsafe { env::set_var("PATH", path) };
        } else {
            unsafe { env::remove_var("PATH") };
        }
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Fixture {
    root: tempfile::TempDir,
    source: PathBuf,
    build: PathBuf,
    codex_home: PathBuf,
    user_home: PathBuf,
    upstream: PathBuf,
    cpu: PathBuf,
    heavy: PathBuf,
    retained: Vec<(PathBuf, Vec<u8>)>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("harness-plugin-lifecycle-")
            .tempdir()
            .unwrap();
        let source = synthetic_role_build::kit_source(root.path());
        fs::write(source.join("global/hooks.json"), b"{}\n").unwrap();
        fs::write(source.join("global/token-hooks.json"), b"{}\n").unwrap();
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for relative in [
            ".agents/plugins/marketplace.json",
            ".agents/plugins/executors/.codex-plugin/plugin.json",
            ".agents/plugins/executors/.mcp.json",
        ] {
            let destination = source.join(relative);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(repository.join(relative), destination).unwrap();
        }

        let build = root.path().join("build");
        fs::create_dir_all(&build).unwrap();
        for name in build_identity::BINARIES {
            let from = Path::new(env!("CARGO_BIN_EXE_codex")).with_file_name(name);
            if from.is_file() {
                fs::copy(&from, build.join(name))
                    .unwrap_or_else(|error| panic!("copy {name} from {}: {error}", from.display()));
            } else {
                // Only the launcher and manager run in this check; the other
                // recorded binaries must still satisfy build integrity.
                fs::write(build.join(name), name.as_bytes()).unwrap();
            }
        }
        let record = build_identity::BuildRecord {
            schema: build_identity::SCHEMA,
            source_root: source.clone(),
            source: build_identity::source_identity(&source).unwrap(),
            rustc: "plugin lifecycle acceptance".into(),
            cargo: "plugin lifecycle acceptance".into(),
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
                .collect::<BTreeMap<_, _>>(),
        };
        fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();

        let codex_home = root.path().join("codex-home");
        let user_home = root.path().join("user-home");
        fs::create_dir_all(&codex_home).unwrap();
        fs::create_dir_all(&user_home).unwrap();
        fs::write(
            codex_home.join("config.toml"),
            "model = \"keep-model\"\n\
             approval_policy = \"never\"\n\
             \n\
             [mcp_servers.foreign]\n\
             command = \"foreign.exe\"\n\
             args = [\"keep\"]\n\
             \n\
             [plugins.\"ponytail@coding-agents-harness\"]\n\
             enabled = true\n\
             \n\
             [plugins.\"unrelated@elsewhere\"]\n\
             enabled = false\n",
        )
        .unwrap();
        fs::write(
            codex_home.join("auth.json"),
            b"{\"sentinel\":\"keep-private-auth\"}",
        )
        .unwrap();

        let retained = root
            .path()
            .join("codex-home/harness/executor-pool/retained");
        fs::create_dir_all(&retained).unwrap();
        for (name, value) in [
            (
                "spawn-1.json",
                json!({
                    "state": "running",
                    "assignment": {"objective": SECRET_ASSIGNMENT},
                    "slot": 1
                }),
            ),
            (
                "endpoint-1.json",
                json!({
                    "threadId": "01a00000-0000-7000-8000-000000000001",
                    "token": SECRET_ENDPOINT
                }),
            ),
            (
                "assignment-1.json",
                json!({"schema": 1, "objective": SECRET_ASSIGNMENT}),
            ),
        ] {
            fs::write(
                retained.join(name),
                serde_json::to_vec_pretty(&value).unwrap(),
            )
            .unwrap();
        }
        let retained = ["spawn-1.json", "endpoint-1.json", "assignment-1.json"]
            .into_iter()
            .map(|name| {
                let path = retained.join(name);
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();

        let upstream = env::var_os("HARNESS_PLUGIN_CODEX_EXE")
            .or_else(|| env::var_os("HARNESS_CONTROL_CODEX_EXE"))
            .map(PathBuf::from)
            .expect(
                "set HARNESS_PLUGIN_CODEX_EXE (or HARNESS_CONTROL_CODEX_EXE) to a Codex CLI with native plugin commands",
            );
        let cpu = root.path().join("cpu-account");
        let heavy = root.path().join("heavy-account");
        Self {
            root,
            source,
            build,
            codex_home,
            user_home,
            upstream,
            cpu,
            heavy,
            retained,
        }
    }

    fn lifecycle(&self, verb: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        command
            .arg(verb)
            .args(["--core-only", "--source"])
            .arg(&self.source)
            .arg("--build")
            .arg(&self.build)
            .arg("--codex-home")
            .arg(&self.codex_home)
            .arg("--user-home")
            .arg(&self.user_home)
            .arg("--dependency-user-home")
            .arg(&self.user_home)
            .arg("--upstream")
            .arg(&self.upstream)
            .args(["--path-scope", "process", "--timeout-seconds", "120"])
            .env("CODEX_HARNESS_CPU_ACCOUNT", &self.cpu)
            .env("CODEX_HARNESS_HEAVY_ACCOUNT", &self.heavy)
            .env_remove("CODEX_HARNESS_CPU_PERCENT")
            .current_dir(&self.root);
        command.output().unwrap()
    }

    fn check(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["check", "--core-only", "--codex-home"])
            .arg(&self.codex_home)
            .arg("--user-home")
            .arg(&self.user_home)
            .arg("--dependency-user-home")
            .arg(&self.user_home)
            .args(["--timeout-seconds", "120"])
            .env("CODEX_HARNESS_CPU_ACCOUNT", &self.cpu)
            .env("CODEX_HARNESS_HEAVY_ACCOUNT", &self.heavy)
            .env_remove("CODEX_HARNESS_CPU_PERCENT")
            .env("PATH", self.installed_path())
            .current_dir(&self.root)
            .output()
            .unwrap()
    }

    fn recover(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["recover", "--core-only", "--codex-home"])
            .arg(&self.codex_home)
            .arg("--user-home")
            .arg(&self.user_home)
            .arg("--dependency-user-home")
            .arg(&self.user_home)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }

    fn disconnect(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["disconnect", "--core-only", "--codex-home"])
            .arg(&self.codex_home)
            .arg("--user-home")
            .arg(&self.user_home)
            .arg("--dependency-user-home")
            .arg(&self.user_home)
            .current_dir(&self.root)
            .output()
            .unwrap()
    }

    fn native(&self, args: &[&str]) -> Value {
        let output = Command::new(&self.upstream)
            .args(args)
            .env("CODEX_HOME", &self.codex_home)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn installed_path(&self) -> OsString {
        let mut paths = vec![self.codex_home.join("harness/bin")];
        if let Some(path) = env::var_os("PATH") {
            paths.extend(env::split_paths(&path));
        }
        env::join_paths(paths).unwrap()
    }

    fn assert_preserved(&self) {
        let config = fs::read_to_string(self.codex_home.join("config.toml")).unwrap();
        for required in [
            "model = \"keep-model\"",
            "[mcp_servers.foreign]",
            "command = \"foreign.exe\"",
            "[plugins.\"ponytail@coding-agents-harness\"]",
            "enabled = true",
            "[plugins.\"unrelated@elsewhere\"]",
            "enabled = false",
        ] {
            assert!(
                config.contains(required),
                "missing preserved config: {required}"
            );
        }
        assert_eq!(
            fs::read(self.codex_home.join("auth.json")).unwrap(),
            b"{\"sentinel\":\"keep-private-auth\"}"
        );
        for (path, bytes) in &self.retained {
            assert_eq!(
                &fs::read(path).unwrap(),
                bytes,
                "{} changed",
                path.display()
            );
        }
        let marketplaces = self.native(&["plugin", "marketplace", "list", "--json"]);
        assert!(
            marketplaces["marketplaces"]
                .as_array()
                .unwrap()
                .iter()
                .any(|marketplace| marketplace["name"] == "coding-agents-harness"),
            "the shared marketplace must remain"
        );
    }

    fn assert_plugin(&self, expected: bool) {
        let plugins = self.native(&[
            "plugin",
            "list",
            "--marketplace",
            "coding-agents-harness",
            "--json",
        ]);
        let installed = plugins["installed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|plugin| plugin["pluginId"] == PLUGIN);
        assert_eq!(installed, expected, "{plugins}");
        let mcp = self.native(&["mcp", "list", "--json"]);
        let server = mcp
            .as_array()
            .unwrap()
            .iter()
            .find(|server| server["name"] == "executors");
        assert_eq!(server.is_some(), expected, "{mcp}");
        if let Some(server) = server {
            assert_eq!(server["enabled"], true);
            assert_eq!(server["transport"]["type"], "stdio");
            assert_eq!(server["transport"]["command"], "codex-harness");
            assert_eq!(server["transport"]["args"], json!(["mcp", "executors"]));
        }
    }

    fn start_executor_mcp(&self) -> ChildGuard {
        let stderr = File::create(self.root.path().join("mcp.stderr")).unwrap();
        let child = Command::new("codex-harness")
            .args(["mcp", "executors"])
            .env("CODEX_HOME", &self.codex_home)
            .env("PATH", self.installed_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .spawn()
            .unwrap();
        ChildGuard(child)
    }

    fn assert_no_secret(&self, output: &Output) {
        for text in [
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ] {
            assert!(
                !text.contains(SECRET_ASSIGNMENT),
                "assignment leaked: {text}"
            );
            assert!(!text.contains(SECRET_ENDPOINT), "endpoint leaked: {text}");
            assert!(
                !text.contains("keep-private-auth"),
                "credential leaked: {text}"
            );
        }
    }
}

fn succeeded(output: Output) -> Value {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout is not JSON: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

#[test]
fn tracked_plugin_declaration_is_portable_and_uses_the_native_stdio_contract() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let marketplace =
        fs::read_to_string(repository.join(".agents/plugins/marketplace.json")).unwrap();
    assert!(marketplace.contains("\"name\": \"executors\""));
    assert!(marketplace.contains("\"path\": \"./.agents/plugins/executors\""));
    let manifest =
        fs::read_to_string(repository.join(".agents/plugins/executors/.codex-plugin/plugin.json"))
            .unwrap();
    assert!(manifest.contains("\"name\": \"executors\""));
    assert!(manifest.contains("\"mcpServers\": \"./.mcp.json\""));
    let mcp = fs::read_to_string(repository.join(".agents/plugins/executors/.mcp.json")).unwrap();
    assert!(mcp.contains("\"command\": \"codex-harness\""));
    assert!(mcp.contains("\"args\": [\"mcp\", \"executors\"]"));
    for text in [marketplace, manifest, mcp] {
        assert!(!text.contains("C:\\"));
        assert!(!text.contains("D:\\"));
    }
}

#[test]
fn unsupported_native_plugin_cli_is_reported_without_a_receipt() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("codex-home");
    fs::create_dir_all(&home).unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let report = harness_core::core_install::plugin_connect(
        &home,
        &repository,
        Path::new(env!("CARGO_BIN_EXE_codex")),
        false,
    )
    .unwrap();
    assert_eq!(report.status, "unsupported");
    assert_eq!(report.available, false);
    assert!(!home.join("harness/executors-plugin.json").exists());
}

#[test]
#[ignore = "requires HARNESS_PLUGIN_CODEX_EXE with native plugin commands; isolated homes only"]
fn install_update_check_recover_and_disconnect_preserve_unrelated_state() {
    let fixture = Fixture::new();
    eprintln!(
        "plugin lifecycle evidence: {}",
        fixture.root.path().display()
    );

    let installed = succeeded(fixture.lifecycle("install"));
    assert_eq!(installed["plugin"]["status"], "connected");
    assert_eq!(installed["plugin"]["available"], true);
    fixture.assert_plugin(true);
    fixture.assert_preserved();
    assert_eq!(installed["plugin"]["installed"], true);
    assert_eq!(installed["plugin"]["enabled"], true);

    let _path = PathRestore::capture();
    unsafe {
        env::set_var("PATH", fixture.installed_path());
    }
    let mut server = fixture.start_executor_mcp();
    {
        let stdin = server.0.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            "{}",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": {"name": "plugin-lifecycle-test", "version": "1"}
                }
            })
        )
        .unwrap();
        stdin.flush().unwrap();
        let mut stdout = BufReader::new(server.0.stdout.as_mut().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let initialized: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(initialized["result"]["serverInfo"]["name"], "executors");
        writeln!(
            server.0.stdin.as_mut().unwrap(),
            "{}",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .unwrap();
        writeln!(
            server.0.stdin.as_mut().unwrap(),
            "{}",
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
        )
        .unwrap();
        server.0.stdin.as_mut().unwrap().flush().unwrap();
        line.clear();
        stdout.read_line(&mut line).unwrap();
        let tools: Value = serde_json::from_str(&line).unwrap();
        assert!(
            tools["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "executor_dispatch"),
            "{tools}"
        );
    }
    drop(server);

    let repeated = succeeded(fixture.lifecycle("install"));
    assert_eq!(repeated["plugin"]["status"], "connected");
    assert_eq!(repeated["plugin"]["changed"], false);
    fixture.assert_plugin(true);
    fixture.assert_preserved();

    let updated = succeeded(fixture.lifecycle("update"));
    assert_eq!(updated["plugin"]["status"], "connected");
    assert_eq!(updated["plugin"]["changed"], false);
    fixture.assert_plugin(true);
    fixture.assert_preserved();

    let checked = fixture.check();
    fixture.assert_no_secret(&checked);
    let checked = succeeded(checked);
    assert_eq!(checked["status"], "connected");
    assert_eq!(checked["plugin"]["status"], "ready");
    assert_eq!(checked["plugin"]["available"], true);

    fixture.native(&["plugin", "remove", PLUGIN, "--json"]);
    let receipt_path = fixture.codex_home.join("harness/executors-plugin.json");
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt["state"] = json!("pending");
    receipt["desired"] = json!("connect");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    fixture.assert_plugin(false);

    let recovered = fixture.recover();
    fixture.assert_no_secret(&recovered);
    let recovered = succeeded(recovered);
    assert_eq!(recovered["plugin"]["status"], "recovered");
    assert_eq!(recovered["plugin"]["available"], true);
    fixture.assert_plugin(true);
    fixture.assert_preserved();

    let disconnected = fixture.disconnect();
    fixture.assert_no_secret(&disconnected);
    let disconnected = succeeded(disconnected);
    assert_eq!(disconnected["plugin"]["status"], "disconnected");
    fixture.assert_plugin(false);
    fixture.assert_preserved();

    // An interrupted disconnect is replayed from the same receipt after core
    // removal; the marketplace and retained work remain untouched.
    fixture.native(&["plugin", "add", PLUGIN, "--json"]);
    fixture.assert_plugin(true);
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt["state"] = json!("pending");
    receipt["desired"] = json!("disconnect");
    fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let recovered = succeeded(fixture.recover());
    assert_eq!(recovered["plugin"]["status"], "recovered");
    fixture.assert_plugin(false);
    fixture.assert_preserved();
}
