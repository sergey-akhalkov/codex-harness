//! Native code-tools-only Check/preview/Install/Update. Check and preview are
//! read-only. Update rewrites owned MCP registrations from adopted inventory
//! without acquiring packages or stopping shared OpenCode consumers.
#![cfg(windows)]

use crate::{
    build_identity, codegraph_integration,
    codegraph_registration::{self, RegistrationRequest},
    config_file::ConfigSnapshot,
    dependency_discovery::{self, local_path},
    installation_lock::InstallationLocks,
    installation_state::normal,
    inventory,
    registration_native::StagedFile,
};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Install,
    Update,
    Check,
    Disconnect,
    Recover,
}

pub struct Request {
    pub source: PathBuf,
    pub codex_home: PathBuf,
    pub user_home: PathBuf,
    pub dependency_user_home: PathBuf,
    pub mode: Mode,
    pub preview: bool,
    pub manager: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub status: String,
    pub model_calls: u32,
    pub mutated: bool,
    pub registration: Value,
    pub inventory_servers: Vec<String>,
}

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
}

fn absent(path: &Path) -> io::Result<()> {
    inventory::ordinary_parents(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(conflict(&format!(
            "pending code-tools transaction; preserve it and run Recover: {}",
            path.display()
        ))),
    }
}

fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    match ConfigSnapshot::read(path) {
        Ok(snapshot) => {
            snapshot.replace(&bytes)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            StagedFile::create(path, &bytes)?.commit()
        }
        Err(error) => Err(error),
    }
}

fn adopted(item: &Value) -> bool {
    matches!(
        item["status"].as_str(),
        Some("adopted" | "present" | "installed" | "ready" | "verified")
    )
}

fn python_spec(source: &Path, home: &Path, python: &Path, name: &str) -> io::Result<Value> {
    Ok(json!({
        "command": local_path(python)?.to_str().ok_or_else(|| conflict("python path is not UTF-8"))?,
        "args": [
            "-B",
            "-u",
            local_path(&source.join("tools/code-tools/launch.py"))?
                .to_str()
                .ok_or_else(|| conflict("launch path is not UTF-8"))?,
            name,
            "--registry",
            local_path(&home.join("harness/code-tools.json"))?
                .to_str()
                .ok_or_else(|| conflict("registry path is not UTF-8"))?
        ],
        "env": {"CODEX_HOME": local_path(home)?.to_str().ok_or_else(|| conflict("home path is not UTF-8"))?}
    }))
}

fn retained_from_inventory(source: &Path, home: &Path, inventory: &Value) -> io::Result<Value> {
    let mut retained = Map::new();
    for name in ["serena", "graphify", "nuphus"] {
        let Some(item) = inventory["mcp"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["id"] == name))
        else {
            continue;
        };
        if !adopted(item) {
            continue;
        }
        let python = item["paths"]["python"]
            .as_str()
            .ok_or_else(|| conflict(&format!("{name} is adopted but has no python path")))?;
        retained.insert(
            name.into(),
            python_spec(source, home, Path::new(python), name)?,
        );
    }
    Ok(Value::Object(retained))
}

fn snapshot_opencode_cache(user: &Path) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
    let root = user.join(".cache/opencode/bin");
    inventory::ordinary_parents(&root.join("probe"))?;
    let mut snapshots = Vec::new();
    collect_opencode_files(&root, &mut snapshots)?;
    Ok(snapshots)
}

fn collect_opencode_files(path: &Path, snapshots: &mut Vec<(PathBuf, Vec<u8>)>) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(conflict(
                "OpenCode shared cache uses a reparse path; preserving existing consumers.",
            ));
        }
        Ok(meta) if meta.is_file() => {
            snapshots.push((path.to_path_buf(), fs::read(path)?));
            return Ok(());
        }
        Ok(_) => {}
    }
    for entry in fs::read_dir(path)? {
        collect_opencode_files(&entry?.path(), snapshots)?;
    }
    Ok(())
}

fn preserved_opencode_cache(before: &[(PathBuf, Vec<u8>)]) -> io::Result<()> {
    for (path, bytes) in before {
        match fs::read(path) {
            Ok(current) if current == *bytes => {}
            Ok(_) => {
                return Err(conflict(
                    "OpenCode shared cache changed; preserving existing consumers.",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(conflict(
                    "OpenCode shared cache changed; preserving existing consumers.",
                ));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn existing_codegraph(home: &Path) -> io::Result<Option<Value>> {
    let path = home.join("harness/code-tools-registration.json");
    inventory::ordinary_parents(&path)?;
    match fs::read(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
        Ok(bytes) => {
            let state: Value = serde_json::from_slice(&bytes)
                .map_err(|_| conflict("code-tools registration is not JSON; preserving it"))?;
            Ok(state["registrations"].get("codegraph").cloned())
        }
    }
}

pub fn run(request: &Request) -> io::Result<Report> {
    let source = local_path(&request.source)?;
    let home = local_path(&request.codex_home)?;
    let user = normal(&request.user_home)?;
    let dependency = normal(&request.dependency_user_home)?;
    let _locks = InstallationLocks::acquire(&user, &dependency)?;
    if request.mode != Mode::Recover {
        for name in [
            "harness/activation-pending.json",
            "harness/code-tools-registration-pending.json",
            "harness/code-tools-files-pending.json",
            "harness/tool-resources-pending.json",
        ] {
            absent(&home.join(name))?;
        }
    }
    let launch = source.join("tools/code-tools/launch.py");
    if !launch.is_file() {
        return Err(conflict("code-tools launch entry is missing"));
    }
    let inventory = dependency_discovery::discover(&dependency_discovery::Request {
        catalogue: source.join("global/code-tools.json"),
        user_home: dependency.clone(),
        ..dependency_discovery::Request::default()
    })?;
    let retained = retained_from_inventory(&source, &home, &inventory)?;
    let mut retained_for_graph = json!({"registrations": retained});
    if let Some(graph) = existing_codegraph(&home)? {
        retained_for_graph["registrations"]["codegraph"] = graph.clone();
    }
    let opencode = snapshot_opencode_cache(&dependency)?;
    if matches!(request.mode, Mode::Install | Mode::Update)
        && !request.preview
        && retained
            .as_object()
            .is_some_and(|map| !map.contains_key("serena"))
    {
        return Err(conflict(
            "Serena dependency is missing or incompatible; explicit provisioning is required.",
        ));
    }
    let activating = matches!(request.mode, Mode::Install | Mode::Update) && !request.preview;
    if request.mode == Mode::Recover {
        let registration = codegraph_registration::apply(&RegistrationRequest {
            codex_home: home.clone(),
            mode: "Recover".into(),
            command: None,
            package_root: None,
            defer_commit: false,
            preview: request.preview,
            retained_registrations: None,
        })?;
        return Ok(Report {
            status: registration["status"]
                .as_str()
                .unwrap_or("no-pending-registration")
                .into(),
            model_calls: 0,
            mutated: !request.preview,
            registration,
            inventory_servers: Vec::new(),
        });
    }
    if activating {
        if let Some(manager) = request.manager.as_deref() {
            let prepared = codegraph_integration::prepare(
                &codegraph_integration::Request {
                    codex_home: home.clone(),
                    dependency_state: dependency.join(".cache/coding-agents-harness-codegraph"),
                    package_root: None,
                    mode: if request.mode == Mode::Update {
                        "Check".into()
                    } else {
                        "Install".into()
                    },
                },
                manager,
            )?;
            if let Some(graph) = prepared
                .get("registrations")
                .and_then(|value| value.get("codegraph"))
            {
                retained_for_graph["registrations"]["codegraph"] = graph.clone();
            }
        }
    }
    let registration = codegraph_registration::apply(&RegistrationRequest {
        codex_home: home.clone(),
        mode: match request.mode {
            Mode::Install if !request.preview => "Install".into(),
            Mode::Update if !request.preview => "Update".into(),
            Mode::Disconnect => "Disconnect".into(),
            _ => "Check".into(),
        },
        command: retained_for_graph["registrations"]["codegraph"]["command"]
            .as_str()
            .map(PathBuf::from),
        package_root: retained_for_graph["registrations"]["codegraph"]["args"]
            .as_array()
            .and_then(|args| args.get(3))
            .and_then(Value::as_str)
            .map(PathBuf::from),
        defer_commit: false,
        preview: request.preview,
        retained_registrations: Some(retained_for_graph),
    })?;
    let servers = inventory["mcp"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect();
    if request.mode == Mode::Check || request.preview {
        return Ok(Report {
            status: registration["status"]
                .as_str()
                .unwrap_or("preview-registration")
                .into(),
            model_calls: 0,
            mutated: false,
            registration,
            inventory_servers: servers,
        });
    }
    if request.mode == Mode::Disconnect {
        return Ok(Report {
            status: registration["status"]
                .as_str()
                .unwrap_or("disconnected")
                .into(),
            model_calls: 0,
            mutated: !request.preview,
            registration,
            inventory_servers: servers,
        });
    }
    let mut registry = inventory.clone();
    registry["schema_version"] = json!(1);
    write_json(&home.join("harness/code-tools.json"), &registry)?;
    write_json(
        &home.join("harness/lsp-servers.json"),
        &json!({"schema_version": 1, "servers": {}}),
    )?;
    let _ = build_identity::hash_file(&launch)?;
    preserved_opencode_cache(&opencode)?;
    Ok(Report {
        status: registration["status"]
            .as_str()
            .unwrap_or("connected")
            .into(),
        model_calls: 0,
        mutated: true,
        registration,
        inventory_servers: servers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn check_and_preview_do_not_create_or_mutate_unrelated_state() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        fs::create_dir_all(home.join("harness")).unwrap();
        let foreign = home.join("harness/installation.json");
        fs::write(&foreign, b"{\"schemaVersion\":1}").unwrap();
        let before = fs::read(&foreign).unwrap();
        let request = Request {
            source: repo(),
            codex_home: home.clone(),
            user_home: root.path().join("user"),
            dependency_user_home: root.path().join("dependency"),
            mode: Mode::Check,
            preview: false,
            manager: None,
        };
        let checked = run(&request).unwrap();
        assert_eq!(checked.model_calls, 0);
        assert!(!checked.mutated);
        assert_eq!(fs::read(&foreign).unwrap(), before);
        let mut preview = request;
        preview.mode = Mode::Install;
        preview.preview = true;
        let previewed = run(&preview).unwrap();
        assert!(!previewed.mutated);
        assert_eq!(fs::read(&foreign).unwrap(), before);
        assert!(!home.join("harness/code-tools.json").exists());
    }

    #[test]
    fn pending_code_tools_transaction_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        fs::create_dir_all(home.join("harness")).unwrap();
        let pending = home.join("harness/code-tools-registration-pending.json");
        fs::write(&pending, b"{\"owner\":\"preserve\"}").unwrap();
        let before = fs::read(&pending).unwrap();
        let error = run(&Request {
            source: repo(),
            codex_home: home,
            user_home: root.path().join("user"),
            dependency_user_home: root.path().join("dependency"),
            mode: Mode::Install,
            preview: true,
            manager: None,
        })
        .unwrap_err();
        assert!(error.to_string().contains("pending code-tools"));
        assert_eq!(fs::read(&pending).unwrap(), before);
    }

    #[test]
    fn preview_preserves_unrelated_token_workflow_record() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        fs::create_dir_all(home.join("harness")).unwrap();
        let unrelated = home.join("harness/token-workflow.json");
        fs::write(&unrelated, br#"{"enabled":true,"links":[]}"#).unwrap();
        let before = fs::read(&unrelated).unwrap();
        let previewed = run(&Request {
            source: repo(),
            codex_home: home.clone(),
            user_home: root.path().join("user"),
            dependency_user_home: root.path().join("dependency"),
            mode: Mode::Install,
            preview: true,
            manager: None,
        })
        .unwrap();
        assert!(!previewed.mutated);
        assert_eq!(fs::read(&unrelated).unwrap(), before);
        assert!(!home.join("harness/code-tools.json").exists());
    }

    #[test]
    fn update_preview_is_silent_and_preserves_opencode_cache() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        let user = root.path().join("user");
        fs::create_dir_all(home.join("harness")).unwrap();
        let cache = user.join(".cache/opencode/bin");
        fs::create_dir_all(&cache).unwrap();
        let wrapper = cache.join("keep.js");
        fs::write(&wrapper, b"opencode-wrapper").unwrap();
        let report = run(&Request {
            source: repo(),
            codex_home: home.clone(),
            user_home: user.clone(),
            dependency_user_home: user.clone(),
            mode: Mode::Update,
            preview: true,
            manager: None,
        })
        .unwrap();
        assert!(!report.mutated);
        assert_eq!(fs::read(&wrapper).unwrap(), b"opencode-wrapper");
        assert!(!home.join("harness/code-tools.json").exists());
    }

    #[test]
    fn update_preview_preserves_nested_opencode_clangd_cache() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        let user = root.path().join("user");
        fs::create_dir_all(home.join("harness")).unwrap();
        let clangd = user.join(".cache/opencode/bin/clangd_20/bin");
        fs::create_dir_all(&clangd).unwrap();
        let binary = clangd.join("clangd.exe");
        fs::write(&binary, b"opencode-clangd").unwrap();
        let report = run(&Request {
            source: repo(),
            codex_home: home.clone(),
            user_home: user.clone(),
            dependency_user_home: user,
            mode: Mode::Update,
            preview: true,
            manager: None,
        })
        .unwrap();
        assert!(!report.mutated);
        assert_eq!(fs::read(&binary).unwrap(), b"opencode-clangd");
        assert!(!home.join("harness/code-tools.json").exists());
    }

    #[test]
    fn update_without_serena_is_rejected_before_mutation() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        let user = root.path().join("user");
        fs::create_dir_all(home.join("harness")).unwrap();
        let cache = user.join(".cache/opencode/bin");
        fs::create_dir_all(&cache).unwrap();
        let wrapper = cache.join("keep.js");
        fs::write(&wrapper, b"opencode-wrapper").unwrap();
        let error = run(&Request {
            source: repo(),
            codex_home: home.clone(),
            user_home: user.clone(),
            dependency_user_home: user,
            mode: Mode::Update,
            preview: false,
            manager: None,
        })
        .unwrap_err();
        assert!(error.to_string().contains("explicit provisioning"));
        assert_eq!(fs::read(&wrapper).unwrap(), b"opencode-wrapper");
        assert!(!home.join("harness/code-tools.json").exists());
    }
}
