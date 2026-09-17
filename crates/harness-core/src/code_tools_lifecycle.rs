//! Native code-tools-only Check/preview/Install/Update. Check and preview are
//! read-only. Update rewrites owned MCP registrations from adopted inventory
//! without acquiring packages or stopping shared OpenCode consumers.
#![cfg(windows)]

use crate::{
    codegraph_integration,
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
    env, fs, io,
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

/// The documented discovery inputs for a connection's dependency owner. The
/// invoking environment describes the current user's adopted packages, so it
/// is only consulted when that user owns the dependency home.
fn discovery_request(
    source: &Path,
    dependency: &Path,
) -> io::Result<dependency_discovery::Request> {
    let current = env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|path| dependency_discovery::local_path(&path))
        .transpose()?;
    let include = current
        .as_ref()
        .is_some_and(|current| crate::dependency_package::same_path(current, dependency));
    let selected = |variable: &str| {
        if include {
            env::var_os(variable).map(PathBuf::from)
        } else {
            None
        }
    };
    Ok(dependency_discovery::Request {
        catalogue: source.join("global/code-tools.json"),
        user_home: dependency.to_owned(),
        npm_prefixes: if include {
            env::var_os("NPM_CONFIG_PREFIX")
                .map(PathBuf::from)
                .into_iter()
                .collect()
        } else {
            Vec::new()
        },
        uv_tools_dir: selected("UV_TOOL_DIR"),
        serena_cache: None,
        rustup_home: selected("RUSTUP_HOME"),
        path: if include { env::var_os("PATH") } else { None },
        nuphus_models: selected("NUPHUS_MODELS_DIR"),
        codegraph_roots: Vec::new(),
        full_records: false,
        probe_versions: false,
        processes: false,
    })
}

/// Registrations recorded by the last successful connection. They stay the
/// desired selection unless a fresh native projection replaces them, so an
/// Update never rewrites an installed native connection back to a seam.
fn recorded_registrations(home: &Path) -> io::Result<Map<String, Value>> {
    let path = home.join("harness/code-tools-registration.json");
    inventory::ordinary_parents(&path)?;
    match fs::read(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(error),
        Ok(bytes) => {
            let state: Value = serde_json::from_slice(&bytes)
                .map_err(|_| conflict("code-tools registration is not JSON; preserving it"))?;
            Ok(state["registrations"]
                .as_object()
                .cloned()
                .unwrap_or_default())
        }
    }
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
    let inventory = dependency_discovery::discover(&discovery_request(&source, &dependency)?)?;
    // The recorded selection stays authoritative until a fresh native
    // projection replaces the same name.
    let desired = recorded_registrations(&home)?;
    let mut retained_for_graph = json!({"registrations": desired});
    let opencode = snapshot_opencode_cache(&dependency)?;
    let serena_adopted = inventory["mcp"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| item["id"] == "serena" && adopted(item));
    if matches!(request.mode, Mode::Install | Mode::Update) && !request.preview && !serena_adopted {
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
    if activating && let Some(manager) = request.manager.as_deref() {
        let prepared = codegraph_integration::prepare(
            &codegraph_integration::Request {
                codex_home: home.clone(),
                dependency_state: dependency.join(".cache/coding-agents-harness-codegraph"),
                package_root: None,
                source_root: Some(source.clone()),
                inventory: Some(inventory.clone()),
                mode: if request.mode == Mode::Update {
                    "Check".into()
                } else {
                    "Install".into()
                },
            },
            manager,
        )?;
        // Fresh native projections replace the recorded or seam connection for
        // the same name; names without a projection keep their selection.
        for (name, spec) in prepared
            .get("registrations")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
        {
            retained_for_graph["registrations"][name] = spec.clone();
        }
    }
    // Mutating modes never silently drop a managed connection: every managed
    // tool must carry a verified native registration.
    if activating {
        for name in ["serena", "nuphus", "codegraph"] {
            if retained_for_graph["registrations"][name].is_null() {
                return Err(conflict(&format!(
                    "{name} has no verified native connection; explicit provisioning is required."
                )));
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

    fn write_native_serena(home: &Path, spec: &Value, connection: bool) -> Vec<u8> {
        fs::write(
            home.join("harness/code-tools-registration.json"),
            serde_json::to_vec_pretty(&json!({
                "schema_version": 1,
                "registrations": {"serena": spec}
            }))
            .unwrap(),
        )
        .unwrap();
        let config = if connection {
            format!(
                "[mcp_servers.serena]\nargs = [\"mcp\", \"serena\", \"--serena\", 'D:/serena.exe']\ncommand = 'D:/mgr.exe'\nenv = {{ CODEX_HOME = '{}' }}\nstartup_timeout_sec = 30\ntool_timeout_sec = 660\n",
                home.display()
            )
        } else {
            "model = 'gpt-6-astra'\n".to_owned()
        };
        fs::write(home.join("config.toml"), config).unwrap();
        fs::read(home.join("harness/code-tools-registration.json")).unwrap()
    }

    fn serena_operations(report: &Report) -> Vec<String> {
        report.registration["operations"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|operation| operation["name"] == "serena")
            .filter_map(|operation| operation["action"].as_str().map(str::to_owned))
            .collect()
    }

    #[test]
    fn recorded_native_connection_is_the_desired_selection() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex");
        let user = root.path().join("user");
        fs::create_dir_all(home.join("harness")).unwrap();
        let native = json!({
            "command": "D:/mgr.exe",
            "args": ["mcp", "serena", "--serena", "D:/serena.exe"],
            "env": {"CODEX_HOME": home.to_string_lossy()},
            "startup_timeout_sec": 30,
            "tool_timeout_sec": 660,
        });
        write_native_serena(&home, &native, true);
        let checked = run(&Request {
            source: repo(),
            codex_home: home.clone(),
            user_home: user.clone(),
            dependency_user_home: user.clone(),
            mode: Mode::Check,
            preview: false,
            manager: None,
        })
        .unwrap();
        // The recorded native connection matches the configuration: a Check
        // must not plan to rewrite it back to the transitional seam.
        assert!(
            serena_operations(&checked).is_empty(),
            "{}",
            checked.registration
        );

        // The recorded selection is the desired state even when nothing is
        // found on this machine: a recorded seam is still planned for the
        // missing connection instead of silently dropping the server.
        let seam = json!({
            "command": "D:/python.exe",
            "args": ["-B", "-u", "D:/launch.py", "serena", "--registry", "D:/code-tools.json"],
            "env": {"CODEX_HOME": home.to_string_lossy()},
        });
        let before = write_native_serena(&home, &seam, false);
        let checked = run(&Request {
            source: repo(),
            codex_home: home.clone(),
            user_home: user.clone(),
            dependency_user_home: user,
            mode: Mode::Check,
            preview: false,
            manager: None,
        })
        .unwrap();
        assert_eq!(serena_operations(&checked), vec!["register".to_owned()]);
        assert_eq!(
            fs::read(home.join("harness/code-tools-registration.json")).unwrap(),
            before,
            "Check must not mutate the recorded selection"
        );
    }
}
