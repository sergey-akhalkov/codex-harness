//! Native MCP preparation for the existing transactional installation dispatcher.
//! This owner acquires/qualifies packages and emits desired configuration; the
//! dispatcher coordinates native registration and the retained registry writes.
#![cfg(windows)]
use crate::{build_identity, dependency_discovery::local_path, registration_native::FileGuard};
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
};

pub struct Request {
    pub codex_home: PathBuf,
    pub source_root: Option<PathBuf>,
    /// Adoption inventory for the same installation when the caller already
    /// discovered it. The CLI projection falls back to the on-disk registry.
    pub inventory: Option<Value>,
    pub mode: String,
}

/// The registered MCP command stays the stable manager link so a new Codex CLI
/// session resolves whatever manager the last Install/Update delivered. A
/// registration that is about to be written requires the link target to be an
/// integrity-verified build, otherwise it is refused instead of pinning a
/// missing, altered or foreign executable. Read-only modes keep the path
/// informational and unresolved.
fn manager_command(manager: &Path, writing: bool) -> io::Result<PathBuf> {
    let path = std::path::absolute(manager)?;
    if !matches!(
        path.components().next(),
        Some(std::path::Component::Prefix(prefix))
            if matches!(prefix.kind(), std::path::Prefix::Disk(_))
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The manager command must be an absolute local path.",
        ));
    }
    if !writing {
        return Ok(path);
    }
    if !path.is_file() {
        return Err(io::Error::other(
            "The manager command is missing from the installation; run core Install/Update first.",
        ));
    }
    let resolved = crate::dependency_package::resolved(&path)?;
    let verified = resolved
        .file_name()
        .is_some_and(|name| name == "codex-harness.exe")
        && resolved
            .parent()
            .is_some_and(|build| build_identity::verify_record_integrity(build).is_ok());
    if !verified {
        return Err(io::Error::other(
            "The manager command does not resolve into an integrity-verified native build.",
        ));
    }
    Ok(path)
}

/// The native Serena registration for an adopted interpreter. The projection
/// only switches the connection when every launch input resolves; otherwise
/// the retained planner keeps the current seam registration.
fn adopted<'a>(inventory: &'a Value, id: &str) -> Option<&'a Value> {
    inventory["mcp"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|record| record["id"] == id)
}

/// Adoption evidence for one managed tool: the caller's fresh discovery when
/// present, otherwise the registry written by the last connection.
fn adoption(home: &Path, inventory: Option<&Value>, id: &str) -> io::Result<Option<Value>> {
    if let Some(inventory) = inventory {
        return Ok(adopted(inventory, id).cloned());
    }
    let registry_path = home.join("harness/code-tools.json");
    let bytes = match FileGuard::read_regular(&registry_path) {
        Ok((_guard, bytes)) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let registry: Value = serde_json::from_slice(&bytes)?;
    Ok(adopted(&registry, id).cloned())
}

fn serena_registration(
    home: &Path,
    manager: &Path,
    source: &Path,
    inventory: Option<&Value>,
) -> io::Result<Option<Value>> {
    let Some(record) = adoption(home, inventory, "serena")? else {
        return Ok(None);
    };
    let registry_path = home.join("harness/code-tools.json");
    let Some(console) = record["paths"]["console_entrypoint"]
        .as_str()
        .or_else(|| record["executable"].as_str())
        .map(PathBuf::from)
    else {
        return Ok(None);
    };
    if !console.is_file() {
        return Ok(None);
    }
    Ok(Some(json!({
        "command": manager,
        "args": ["mcp", "serena",
            "--serena", console,
            "--registry", registry_path,
            "--codex-home", home,
            "--source-root", source,
            "--connection-seconds", "86400"],
        "env": {"CODEX_HOME": home},
        "startup_timeout_sec": 30,
        "tool_timeout_sec": 660,
    })))
}

/// The native Nuphus registration for the adopted audited original binary.
/// The projection switches the connection only when that upstream executable
/// resolves; a locally rewritten variant keeps the current seam registration
/// instead of pinning an unaudited binary.
fn nuphus_registration(
    home: &Path,
    manager: &Path,
    source: &Path,
    inventory: Option<&Value>,
) -> io::Result<Option<Value>> {
    let Some(record) = adoption(home, inventory, "nuphus")? else {
        return Ok(None);
    };
    let Some(executable) = record["paths"]["original_native_executable"]
        .as_str()
        .map(PathBuf::from)
    else {
        return Ok(None);
    };
    if !executable.is_file() {
        return Ok(None);
    }
    let digest = crate::build_identity::hash_file(&executable)?;
    Ok(Some(json!({
        "command": manager,
        "args": ["mcp", "nuphus",
            "--executable", executable,
            "--expected-digest", digest,
            "--codex-home", home,
            "--account", home.join("harness/nuphus"),
            "--source-root", source,
            "--connection-seconds", "86400"],
        "env": {"CODEX_HOME": home},
        "startup_timeout_sec": 30,
        "tool_timeout_sec": 660,
    })))
}

/// Check emits only observed identity and desired configuration. Install/Update
/// may explicitly acquire a published candidate; they preserve adopted trees.
pub fn prepare(request: &Request, manager: &Path) -> io::Result<Value> {
    if !matches!(
        request.mode.as_str(),
        "Install" | "Update" | "Check" | "Recover"
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported MCP preparation mode",
        ));
    }
    let home = local_path(&request.codex_home)?;
    let writes_registrations = matches!(request.mode.as_str(), "Install" | "Update");
    let manager = manager_command(manager, writes_registrations)?;
    let read_only = request.mode == "Check";
    if request.mode == "Recover" {
        return Ok(json!({"status":"not-pending","model_calls":0}));
    }
    let mut registrations = serde_json::Map::new();
    if let Some(source) = &request.source_root {
        let source = local_path(source)?;
        if let Some(serena) =
            serena_registration(&home, &manager, &source, request.inventory.as_ref())?
        {
            registrations.insert("serena".into(), serena);
        }
        if let Some(nuphus) =
            nuphus_registration(&home, &manager, &source, request.inventory.as_ref())?
        {
            registrations.insert("nuphus".into(), nuphus);
        }
    }
    if registrations.is_empty() {
        return Ok(
            json!({"schema_version":1,"status":"missing","mcp":[],"registrations":{},
            "retired":["codebase-memory","graphify","codegraph"],"read_only":read_only,
            "reason":"no adopted native MCP projection is available","model_calls":0}),
        );
    }
    // Retired names stay listed so Update removes owned pre-retirement
    // registrations; see `docs/evidence/legacy-mcp-retirement.json`.
    Ok(
        json!({"schema_version":1,"status":"prepared","read_only":read_only,"model_calls":0,
        "registrations":registrations,"retired":["codebase-memory","graphify","codegraph"],"mcp":[],
        "activation":"prepared only; combined registration and acceptance belong to the installer"}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;

    fn verified_build(root: &Path) -> PathBuf {
        let source = root.join("source");
        let build = root.join("state/builds/aaaa0000aaaa0000-1500-1");
        std::fs::create_dir_all(source.join("crates/one/src")).unwrap();
        std::fs::create_dir_all(source.join("tools/rtk-adapter/src")).unwrap();
        std::fs::create_dir_all(&build).unwrap();
        for file in [
            "Cargo.toml",
            "Cargo.lock",
            "crates/one/src/lib.rs",
            "tools/rtk-adapter/src/lib.rs",
        ] {
            std::fs::write(source.join(file), "fixture").unwrap();
        }
        let mut binaries = std::collections::BTreeMap::new();
        for name in build_identity::BINARIES {
            let path = build.join(name);
            std::fs::write(&path, name).unwrap();
            binaries.insert(
                (*name).to_owned(),
                build_identity::hash_file(&path).unwrap(),
            );
        }
        let record = build_identity::BuildRecord {
            schema: build_identity::SCHEMA,
            source_root: source.clone(),
            source: build_identity::source_identity(&source).unwrap(),
            rustc: "fixture".into(),
            cargo: "fixture".into(),
            target: "x86_64-pc-windows-msvc".into(),
            profile: "release".into(),
            binaries,
        };
        std::fs::write(
            build.join("build.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        build
    }

    #[test]
    fn manager_command_keeps_the_stable_link_and_requires_a_verified_build() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex-home");
        let build = verified_build(root.path());
        let link = home.join("harness/bin/codex-harness.exe");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::windows::fs::symlink_file(build.join("codex-harness.exe"), &link).unwrap();

        // A written registration keeps the link and validates only its target.
        assert_eq!(manager_command(&link, true).unwrap(), link);
        // Read-only modes keep the informational path unresolved.
        assert_eq!(manager_command(&link, false).unwrap(), link);
        let missing = home.join("harness/bin/missing.exe");
        assert!(manager_command(&missing, true).is_err());
        assert!(manager_command(&missing, false).is_ok());
        // An altered target is refused instead of being recorded.
        std::fs::write(build.join("codex-harness.exe"), b"altered").unwrap();
        let error = manager_command(&link, true).unwrap_err();
        assert!(error.to_string().contains("integrity-verified"), "{error}");
    }

    #[test]
    fn serena_projection_requires_an_adopted_console_entrypoint() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex-home");
        let source = root.path().join("source");
        std::fs::create_dir_all(home.join("harness")).unwrap();
        let console = root.path().join("serena.exe");
        std::fs::write(&console, b"fixture console").unwrap();

        // Without a registry record the projection stays with the seam.
        assert!(
            serena_registration(&home, Path::new("D:/mgr.exe"), &source, None)
                .unwrap()
                .is_none()
        );

        std::fs::write(
            home.join("harness/code-tools.json"),
            serde_json::to_vec(&serde_json::json!({
                "mcp": [{
                    "id": "serena",
                    "paths": {"console_entrypoint": console}
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let registration = serena_registration(&home, Path::new("D:/mgr.exe"), &source, None)
            .unwrap()
            .expect("adopted interpreter switches the connection");
        assert_eq!(registration["command"], "D:/mgr.exe");
        let arguments: Vec<&str> = registration["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str())
            .collect();
        assert_eq!(
            arguments[..4],
            ["mcp", "serena", "--serena", &console.to_string_lossy()]
        );
        assert!(arguments.contains(&"--registry"));
        assert!(arguments.contains(&"--codex-home"));
        assert!(arguments.contains(&"--source-root"));
        assert!(arguments.contains(&"--connection-seconds"));
        assert_eq!(
            registration["env"]["CODEX_HOME"],
            home.to_string_lossy().into_owned()
        );
        assert_eq!(registration["startup_timeout_sec"], 30);
        assert_eq!(registration["tool_timeout_sec"], 660);

        // A missing console entry point keeps the current seam registration.
        std::fs::remove_file(&console).unwrap();
        assert!(
            serena_registration(&home, Path::new("D:/mgr.exe"), &source, None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn native_projections_use_fresh_adoption_inventory_without_a_registry_file() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("codex-home");
        let source = root.path().join("source");
        let console = root.path().join("serena.exe");
        std::fs::write(&console, b"fixture console").unwrap();
        let original = root.path().join("nuphus-mcp.exe");
        std::fs::write(&original, b"fixture original").unwrap();
        let digest = crate::build_identity::hash_file(&original).unwrap();
        let inventory = serde_json::json!({"mcp": [
            {"id": "serena", "status": "adopted", "paths": {"console_entrypoint": console}},
            {"id": "nuphus", "status": "modified",
             "paths": {"original_native_executable": original,
                       "native_executable": root.path().join("nuphus-mcp-schema-fixed.exe")}},
        ]});

        // A fresh connection has no registry file yet; the inventory alone
        // switches both connections to their native projections.
        let serena = serena_registration(&home, Path::new("D:/mgr.exe"), &source, Some(&inventory))
            .unwrap()
            .expect("adopted console entry point projects from the inventory");
        assert_eq!(serena["command"], "D:/mgr.exe");
        assert_eq!(serena["args"][1], "serena");
        assert_eq!(serena["args"][3], console.to_string_lossy().into_owned());
        let nuphus = nuphus_registration(&home, Path::new("D:/mgr.exe"), &source, Some(&inventory))
            .unwrap()
            .expect("adopted original projects from the inventory");
        assert_eq!(nuphus["command"], "D:/mgr.exe");
        assert_eq!(nuphus["args"][1], "nuphus");
        assert_eq!(nuphus["args"][3], original.to_string_lossy().into_owned());
        assert_eq!(nuphus["args"][5], digest);
        assert_eq!(nuphus["args"][7], home.to_string_lossy().into_owned());
        assert_eq!(
            nuphus["args"][9],
            home.join("harness/nuphus").to_string_lossy().into_owned()
        );

        // A locally rewritten variant without the audited original keeps the
        // current seam instead of pinning an unaudited executable.
        let rewritten = serde_json::json!({"mcp": [
            {"id": "nuphus", "status": "modified",
             "paths": {"native_executable": root.path().join("nuphus-mcp-schema-fixed.exe")}},
        ]});
        assert!(
            nuphus_registration(&home, Path::new("D:/mgr.exe"), &source, Some(&rewritten))
                .unwrap()
                .is_none()
        );
    }
}
