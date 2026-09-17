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
    time::Duration,
};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn write_registry(root: &Path, status: &str, version: &str, console: &Path) -> PathBuf {
    let path = root.join("code-tools.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "mcp": [{
                "id": "serena",
                "identity": "serena-agent",
                "version": version,
                "status": status,
                "paths": {"console_entrypoint": console}
            }],
            "languages": [{
                "id": "rust", "serena_id": "rust", "status": "adopted",
                "paths": {"executable": console}
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

fn dummy_console(root: &Path) -> PathBuf {
    let path = root.join("serena.exe");
    fs::write(&path, b"not-executed").unwrap();
    path
}

fn owned_launch(root: &Path, registry: PathBuf, console: PathBuf) -> Launch {
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    Launch {
        serena: console,
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
    let console = dummy_console(root.path());
    let launch = owned_launch(
        root.path(),
        root.path().join("missing-registry.json"),
        console,
    );
    let error = serena::command(&launch).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error_text(&error).contains("missing"));
}

#[test]
fn incompatible_version_is_rejected_before_child_start() {
    let root = tempfile::tempdir().unwrap();
    let console = dummy_console(root.path());
    let registry = write_registry(root.path(), "adopted", "0.0.0", &console);
    let error = serena::command(&owned_launch(root.path(), registry, console)).unwrap_err();
    assert!(error_text(&error).contains("Reassess the Serena adapter"));
}

#[test]
fn missing_console_entrypoint_is_rejected_before_child_start() {
    let root = tempfile::tempdir().unwrap();
    let console = root.path().join("missing-serena.exe");
    let registry = write_registry(root.path(), "adopted", "1.7.0", &console);
    let error = serena::command(&owned_launch(root.path(), registry, console)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn incompatible_status_is_rejected_before_child_start() {
    let root = tempfile::tempdir().unwrap();
    let console = dummy_console(root.path());
    let registry = write_registry(root.path(), "missing", "1.7.0", &console);
    let error = serena::command(&owned_launch(root.path(), registry, console)).unwrap_err();
    assert!(error_text(&error).contains("explicit provisioning"));
}

#[test]
fn command_uses_the_generated_owned_home_without_spawning() {
    let root = tempfile::tempdir().unwrap();
    let console = dummy_console(root.path());
    let registry = write_registry(root.path(), "adopted", "1.7.0", &console);
    let launch = owned_launch(root.path(), registry, console.clone());
    let command = serena::command(&launch).unwrap();
    assert_eq!(command.program, console);
    let expected = launch.home.join("harness/serena-home/workers");
    let serena_home = PathBuf::from(
        command
            .env
            .get(OsStr::new("SERENA_HOME"))
            .cloned()
            .flatten()
            .expect("worker home is configured"),
    );
    assert!(serena_home.starts_with(&expected), "{serena_home:?}");
    assert!(serena_home.is_dir());
    let generated = fs::read_to_string(serena_home.join("serena_config.yml")).unwrap();
    assert!(generated.contains("rust"), "{generated}");
    assert!(generated.contains("ls_base_cmd"), "{generated}");
}

#[test]
fn unadopted_project_language_is_refused_before_child_start() {
    let root = tempfile::tempdir().unwrap();
    let console = dummy_console(root.path());
    let registry = write_registry(root.path(), "adopted", "1.7.0", &console);
    let launch = owned_launch(root.path(), registry, console);
    fs::write(launch.project.join("App.csproj"), b"<Project/>").unwrap();
    let error = serena::command(&launch).unwrap_err();
    assert!(error_text(&error).contains("csharp"), "{error}");
}

fn adopted_registry() -> Value {
    let path = PathBuf::from(
        std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").expect("explicit adopted registry"),
    );
    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap()
}

fn adopted_console(inventory: &Value) -> PathBuf {
    PathBuf::from(
        inventory["mcp"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "serena")
            .unwrap()["paths"]["console_entrypoint"]
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
fn generated_configuration_pins_the_adopted_backends_without_touching_the_user_config() {
    let inventory = adopted_registry();
    let console = adopted_console(&inventory);
    let root = tempfile::tempdir().unwrap();
    let project = crate_project(root.path(), "config probe", 7);
    let registry = PathBuf::from(std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").unwrap());
    let shared_config =
        PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".serena/serena_config.yml");
    let before = optional_hash(&shared_config);
    let launch = Launch {
        serena: console,
        registry,
        project,
        home: root.path().join("home"),
    };
    serena::command(&launch).unwrap();
    let generated =
        fs::read_to_string(launch.home.join("harness/serena-home/serena_config.yml")).unwrap();
    assert!(generated.contains("rust:"), "{generated}");
    assert!(generated.contains("ls_base_cmd"), "{generated}");
    assert!(generated.contains("rust-analyzer"), "{generated}");
    assert_eq!(optional_hash(&shared_config), before);
}

#[test]
#[ignore = "requires explicit HARNESS_CODE_TOOLS_REGISTRY for the adopted Serena package"]
fn rust_session_isolates_two_owned_projects_and_preserves_shared_config() {
    let inventory = adopted_registry();
    let console = adopted_console(&inventory);
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
        ("serena", optional_hash(&console)),
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
            serena: console.clone(),
            registry: registry.clone(),
            project: alpha_project,
            home: root.path().join("home-alpha"),
        },
        &cancel,
    )
    .unwrap();
    let mut beta = Session::start(
        &Launch {
            serena: console.clone(),
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
            "serena" => console.as_path(),
            "rust-analyzer" => rust_analyzer.as_path(),
            _ => unreachable!(),
        };
        assert_eq!(hash, optional_hash(path), "{label} changed");
    }
}

#[test]
#[ignore = "requires explicit HARNESS_CODE_TOOLS_REGISTRY for the adopted Serena package"]
fn shared_pool_reuses_one_worker_and_isolates_projects() {
    use harness_core::serena_route;
    use harness_core::serena_shared::{self, Pool};

    let inventory = adopted_registry();
    let console = adopted_console(&inventory);
    let root = tempfile::tempdir().unwrap();
    eprintln!("Serena shared-pool probe root: {}", root.path().display());
    let alpha_project = crate_project(root.path(), "shared alpha", 301);
    let beta_project = crate_project(root.path(), "shared beta", 302);
    let registry = PathBuf::from(std::env::var_os("HARNESS_CODE_TOOLS_REGISTRY").unwrap());
    let codex_home = root.path().join("codex-home");
    let launch = Launch {
        serena: console,
        registry: registry.clone(),
        project: root.path().to_path_buf(),
        home: codex_home,
    };
    let cancel = Cancellation::default();
    let factory = serena_shared::session_factory(launch, cancel).unwrap();
    let policy = serena_route::policy(&repo()).unwrap();
    let serena_home =
        harness_core::serena_configuration::prepare(&registry, &root.path().join("codex-home"))
            .unwrap();
    let mut pool = Pool::new(policy, serena_home, factory).unwrap();
    let initialize = json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {"name": "shared-pool-probe", "version": "0.1.0"}
    });
    let managed = |project: &Path| {
        vec![
            "start-mcp-server".to_string(),
            "--project".to_string(),
            project.to_string_lossy().into_owned(),
            "--context".to_string(),
            "codex".to_string(),
        ]
    };
    let deadline = || Deadline::after(Duration::from_secs(240)).unwrap();
    let client_a = format!("{:032x}", 41_u64);
    let client_b = format!("{:032x}", 42_u64);
    let client_c = format!("{:032x}", 43_u64);
    let args_a: Vec<std::ffi::OsString> = managed(&alpha_project)
        .iter()
        .map(std::ffi::OsString::from)
        .collect();
    let args_b: Vec<std::ffi::OsString> = managed(&alpha_project)
        .iter()
        .map(std::ffi::OsString::from)
        .collect();
    let args_c: Vec<std::ffi::OsString> = managed(&beta_project)
        .iter()
        .map(std::ffi::OsString::from)
        .collect();
    let connected_a = pool
        .connect(
            &client_a,
            &args_a,
            &alpha_project,
            initialize.clone(),
            deadline(),
        )
        .unwrap();
    assert_eq!(
        connected_a["message"]["result"]["serverInfo"]["name"],
        "Serena"
    );
    let connected_b = pool
        .connect(
            &client_b,
            &args_b,
            &alpha_project,
            initialize.clone(),
            deadline(),
        )
        .unwrap();
    // Two clients of one project share a single native worker.
    assert_eq!(
        connected_a["message"]["result"]["serverInfo"],
        connected_b["message"]["result"]["serverInfo"],
    );
    assert_eq!(pool.worker_count(), 1);
    let shared_identity = pool.status()["workers"][0]["pid"].clone();
    let _ = pool
        .connect(
            &client_c,
            &args_c,
            &beta_project,
            initialize.clone(),
            deadline(),
        )
        .unwrap();
    assert_eq!(pool.worker_count(), 2);
    assert_eq!(pool.status()["workers"].as_array().unwrap().len(), 2);

    let call = |pool: &mut Pool, client: &str, project: &Path| {
        let route = serena_route::Route {
            project: Some(project.to_path_buf()),
            cwd: project.to_path_buf(),
            arguments: vec![
                "start-mcp-server".into(),
                "--context".into(),
                "codex".into(),
            ],
            removed_projects: Vec::new(),
            mutation_owner: None,
        };
        let result = pool
            .rpc(
                client,
                "tools/call",
                json!({
                    "name": "find_symbol",
                    "arguments": {
                        "relative_path": "src/lib.rs",
                        "name_path_pattern": "shared",
                        "include_body": true
                    }
                }),
                route,
                initialize.clone(),
                deadline(),
            )
            .unwrap();
        result["message"]["result"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item["text"].as_str())
            .collect::<String>()
    };
    let alpha_a = call(&mut pool, &client_a, &alpha_project);
    let alpha_b = call(&mut pool, &client_b, &alpha_project);
    let beta_c = call(&mut pool, &client_c, &beta_project);
    assert!(alpha_a.contains("301"), "{alpha_a}");
    assert!(alpha_b.contains("301"), "{alpha_b}");
    assert!(beta_c.contains("302"), "{beta_c}");
    // The alpha worker still serves after the beta client used its own worker.
    assert_eq!(pool.status()["workers"].as_array().unwrap().len(), 2);
    // Disconnecting one client of a project keeps the shared worker.
    assert!(pool.disconnect(&client_a));
    let alpha_b2 = call(&mut pool, &client_b, &alpha_project);
    assert!(alpha_b2.contains("301"), "{alpha_b2}");
    let alpha_still_shared = pool.status()["workers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|worker| worker["pid"] == shared_identity);
    assert!(alpha_still_shared);
    pool.close().unwrap();
}
