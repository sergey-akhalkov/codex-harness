//! Rust-controlled Serena boundary. Default cases fail before a child starts.
//! Real two-project semantic ops require HARNESS_CODE_TOOLS_REGISTRY.
#![cfg(windows)]

use harness_core::{
    build_identity::hash_file,
    process::{Cancellation, Deadline},
    serena::{self, Launch, Session},
};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn entry() -> PathBuf {
    repo().join("tools/code-tools/serena_entry.py")
}

fn write_registry(root: &Path, status: &str, version: &str, python: &Path) -> PathBuf {
    let path = root.join("code-tools.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "mcp": [{
                "id": "serena",
                "identity": "serena-agent",
                "version": version,
                "status": status,
                "paths": {"python": python}
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

fn dummy_python(root: &Path) -> PathBuf {
    let path = root.join("python.exe");
    fs::write(&path, b"not-executed").unwrap();
    path
}

fn owned_launch(root: &Path, registry: PathBuf, python: PathBuf) -> Launch {
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    Launch {
        python,
        entry: entry(),
        registry,
        project,
        home: root.join("home"),
    }
}

fn error_text(error: &io::Error) -> String {
    error.to_string()
}

#[test]
fn missing_registry_is_rejected_before_child_start() {
    let root = tempfile::tempdir().unwrap();
    let python = dummy_python(root.path());
    let launch = owned_launch(
        root.path(),
        root.path().join("missing-registry.json"),
        python,
    );
    let error = serena::command(&launch).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error_text(&error).contains("missing"));
}

#[test]
fn incompatible_version_is_rejected_before_child_start() {
    let root = tempfile::tempdir().unwrap();
    let python = dummy_python(root.path());
    let registry = write_registry(root.path(), "adopted", "0.0.0", &python);
    let error = serena::command(&owned_launch(root.path(), registry, python)).unwrap_err();
    assert!(error_text(&error).contains("Reassess the Serena adapter"));
}

#[test]
fn missing_python_is_rejected_before_child_start() {
    let root = tempfile::tempdir().unwrap();
    let python = root.path().join("missing-python.exe");
    let registry = write_registry(root.path(), "adopted", "1.7.0", &python);
    let error = serena::command(&owned_launch(root.path(), registry, python)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn incompatible_status_is_rejected_before_child_start() {
    let root = tempfile::tempdir().unwrap();
    let python = dummy_python(root.path());
    let registry = write_registry(root.path(), "missing", "1.7.0", &python);
    let error = serena::command(&owned_launch(root.path(), registry, python)).unwrap_err();
    assert!(error_text(&error).contains("explicit provisioning"));
}

#[test]
fn command_sets_owned_serena_home_without_spawning() {
    let root = tempfile::tempdir().unwrap();
    let python = dummy_python(root.path());
    let registry = write_registry(root.path(), "adopted", "1.7.0", &python);
    let launch = owned_launch(root.path(), registry, python);
    let command = serena::command(&launch).unwrap();
    let expected = launch.home.join("serena-home");
    assert_eq!(
        command
            .env
            .get(OsStr::new("SERENA_HOME"))
            .cloned()
            .flatten(),
        Some(expected.clone().into_os_string())
    );
    assert!(expected.is_dir());
    assert_eq!(
        command
            .env
            .get(OsStr::new("HARNESS_SERENA_SHARED_WORKER"))
            .cloned()
            .flatten(),
        None
    );
}

fn adopted_registry() -> Value {
    let path = PathBuf::from(
        std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").expect("explicit adopted registry"),
    );
    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap()
}

fn adopted_python(inventory: &Value) -> PathBuf {
    PathBuf::from(
        inventory["mcp"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "serena")
            .unwrap()["paths"]["python"]
            .as_str()
            .unwrap(),
    )
}

fn optional_hash(path: &Path) -> Option<String> {
    path.is_file().then(|| hash_file(path).unwrap())
}

fn crate_project(root: &Path, name: &str, marker: i32) -> PathBuf {
    let project = root.join(name);
    fs::create_dir_all(project.join(".serena")).unwrap();
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join(".serena/project.yml"),
        format!("project_name: '{name}'\nlanguage_servers:\n- rust\nencoding: utf-8\n"),
    )
    .unwrap();
    fs::write(
        project.join("Cargo.toml"),
        format!("[package]\nname = \"crate-{marker}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )
    .unwrap();
    fs::write(
        project.join("src/lib.rs"),
        format!(
            "pub fn shared() -> i32 {{ {marker} }}\npub fn local_use() -> i32 {{ shared() }}\n"
        ),
    )
    .unwrap();
    project
}

fn tool_text(session: &mut Session, name: &str, arguments: Value) -> String {
    session
        .tool_text(
            name,
            arguments,
            Deadline::after(Duration::from_secs(120)).unwrap(),
        )
        .unwrap_or_else(|error| {
            panic!(
                "{name} failed: {error}; stderr {}",
                fs::read_to_string(session.stderr_path()).unwrap_or_default()
            )
        })
}

#[test]
#[ignore = "requires explicit HARNESS_CODE_TOOLS_REGISTRY for the adopted Serena package"]
fn runtime_guard_rejects_upstream_install_without_mutation() {
    let inventory = adopted_registry();
    let python = adopted_python(&inventory);
    let root = tempfile::tempdir().unwrap();
    let probe = root.path().join("guard");
    fs::create_dir_all(&probe).unwrap();
    let inline = probe.join("deny_install.py");
    fs::write(
        &inline,
        r#"
import importlib.util, json, os, sys
from pathlib import Path
entry, registry, probe = sys.argv[1:4]
os.environ["CODEX_HOME"] = str(Path(probe) / "guard-codex-home")
spec = importlib.util.spec_from_file_location("serena_entry", entry)
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
inventory = json.loads(Path(registry).read_text(encoding="utf-8-sig"))
guard.install_runtime_guard(inventory)
from solidlsp.language_servers.common import RuntimeDependencyCollection
from solidlsp.ls import SolidLanguageServer
from solidlsp.ls_config import LanguageServerConfig, LanguageServerId
target = Path(probe) / "must-not-be-created"
try:
    RuntimeDependencyCollection([]).install(str(target))
except guard.RuntimeProvisioningDenied as error:
    assert "disabled during MCP sessions" in str(error)
else:
    raise SystemExit("Provisioning guard did not reject operation")
assert not target.exists()
try:
    SolidLanguageServer.create(LanguageServerConfig(ls_id=LanguageServerId.JAVA), probe)
except guard.RuntimeProvisioningDenied as error:
    assert "disabled during MCP sessions" in str(error)
else:
    raise SystemExit("Unmapped language constructor was not denied")
assert not target.exists()
print("PASS runtime install denial")
"#,
    )
    .unwrap();
    let status = Command::new(&python)
        .args(["-B", inline.to_str().unwrap(), entry().to_str().unwrap()])
        .arg(std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").unwrap())
        .arg(&probe)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "runtime provisioning guard failed; analog fixture tests/fixtures/code-tools-native/serena-guard-check.py still covers Pascal/PowerShell reuse when those languages are adopted"
    );
}

#[test]
#[ignore = "requires explicit HARNESS_CODE_TOOLS_REGISTRY for the adopted Serena package"]
fn rust_session_isolates_two_owned_projects_and_preserves_shared_config() {
    let inventory = adopted_registry();
    let python = adopted_python(&inventory);
    let rust_analyzer = inventory["languages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "rust")
        .and_then(|item| item["paths"]["executable"].as_str())
        .map(PathBuf::from)
        .expect("adopted rust-analyzer");
    let shared_config =
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".serena/serena_config.yml");
    let before = [
        ("shared-config", optional_hash(&shared_config)),
        ("python", optional_hash(&python)),
        ("entry", optional_hash(&entry())),
        ("rust-analyzer", optional_hash(&rust_analyzer)),
    ];
    let root = tempfile::tempdir().unwrap();
    eprintln!("Serena probe root: {}", root.path().display());
    let alpha_project = crate_project(root.path(), "проект alpha", 101);
    let beta_project = crate_project(root.path(), "project beta", 202);
    let registry = PathBuf::from(std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").unwrap());
    let cancel = Cancellation::default();
    let mut alpha = Session::start(
        &Launch {
            python: python.clone(),
            entry: entry(),
            registry: registry.clone(),
            project: alpha_project,
            home: root.path().join("home-alpha"),
        },
        &cancel,
    )
    .unwrap();
    let mut beta = Session::start(
        &Launch {
            python: python.clone(),
            entry: entry(),
            registry,
            project: beta_project,
            home: root.path().join("home-beta"),
        },
        &cancel,
    )
    .unwrap();
    let hello_a = alpha
        .initialize(Deadline::after(Duration::from_secs(180)).unwrap())
        .unwrap_or_else(|error| {
            panic!(
                "alpha init: {error}; {}",
                fs::read_to_string(alpha.stderr_path()).unwrap_or_default()
            )
        });
    let hello_b = beta
        .initialize(Deadline::after(Duration::from_secs(180)).unwrap())
        .unwrap_or_else(|error| {
            panic!(
                "beta init: {error}; {}",
                fs::read_to_string(beta.stderr_path()).unwrap_or_default()
            )
        });
    assert_eq!(hello_a["serverInfo"]["name"], "Serena");
    assert_eq!(hello_b["serverInfo"]["name"], "Serena");
    let tools = alpha
        .request(
            "tools/list",
            json!({}),
            Deadline::after(Duration::from_secs(60)).unwrap(),
        )
        .unwrap();
    let names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"get_diagnostics_for_file"));
    assert!(names.contains(&"find_symbol"));
    assert!(!names.contains(&"execute_shell_command"));
    for session in [&mut alpha, &mut beta] {
        let overview = tool_text(
            session,
            "get_symbols_overview",
            json!({"relative_path":"src/lib.rs"}),
        );
        assert!(overview.contains("shared") && overview.contains("local_use"));
    }
    let read_a = tool_text(
        &mut alpha,
        "find_symbol",
        json!({"relative_path":"src/lib.rs","name_path_pattern":"shared","include_body":true}),
    );
    let read_b = tool_text(
        &mut beta,
        "find_symbol",
        json!({"relative_path":"src/lib.rs","name_path_pattern":"shared","include_body":true}),
    );
    assert!(
        read_a.contains("101") && !read_a.contains("202"),
        "{read_a}"
    );
    assert!(
        read_b.contains("202") && !read_b.contains("101"),
        "{read_b}"
    );
    let _ = alpha.close().unwrap();
    let survivor = tool_text(
        &mut beta,
        "find_symbol",
        json!({"relative_path":"src/lib.rs","name_path_pattern":"shared","include_body":true}),
    );
    assert!(survivor.contains("202"));
    let _ = beta.close().unwrap();
    for (label, hash) in before {
        let path = match label {
            "shared-config" => shared_config.as_path(),
            "python" => python.as_path(),
            "entry" => &entry(),
            "rust-analyzer" => rust_analyzer.as_path(),
            _ => unreachable!(),
        };
        assert_eq!(hash, optional_hash(path), "{label} changed");
    }
}
