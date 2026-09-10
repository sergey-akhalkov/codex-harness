//! CodeGraph preparation for the existing transactional installation dispatcher.
//! This owner acquires/qualifies packages and emits desired configuration; the
//! dispatcher coordinates native registration and the retained registry writes.
#![cfg(windows)]
use crate::{
    dependency_discovery::{self, local_path},
    dependency_selection, dependency_stage, native_build,
    registration_native::FileGuard,
};
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
};

pub struct Request {
    pub codex_home: PathBuf,
    pub dependency_state: PathBuf,
    pub package_root: Option<PathBuf>,
    pub mode: String,
}

fn previous_package(home: &Path) -> io::Result<Option<PathBuf>> {
    let path = home.join("harness/code-tools.json");
    let bytes = match FileGuard::read_regular(&path) {
        Ok((_guard, bytes)) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let registry: Value = serde_json::from_slice(&bytes)?;
    Ok(registry["mcp"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|record| record["id"] == "codegraph")
        .and_then(|record| record["paths"]["package_root"].as_str())
        .map(PathBuf::from))
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
            "unsupported CodeGraph integration mode",
        ));
    }
    let home = local_path(&request.codex_home)?;
    let manager = local_path(manager)?;
    let state = local_path(&request.dependency_state)?;
    let read_only = request.mode == "Check";
    let explicit = request.package_root.clone().or(previous_package(&home)?);
    if request.mode == "Recover" {
        if !state.exists() {
            return Ok(json!({"status":"not-pending","model_calls":0}));
        }
        return dependency_selection::recover(&state, "codegraph");
    }
    let mut package = explicit;
    if package.is_none() && state.exists() {
        let selected = dependency_selection::selected(&state, "codegraph")?;
        package = selected["candidate"]["candidate"]
            .as_str()
            .map(PathBuf::from);
    }
    if package.is_none() {
        if read_only {
            return Ok(
                json!({"status":"missing","mcp":[],"registrations":{},"retired":[],
                "read_only":true,"reason":"CodeGraph requires explicit package preparation","model_calls":0}),
            );
        }
        native_build::owner_root(&state)?;
        let staged = dependency_stage::prepare(
            &manager,
            dependency_discovery::dependency_codegraph::PACKAGE,
            dependency_discovery::dependency_codegraph::VERSION,
            &state,
        )?;
        let stage = PathBuf::from(
            staged["stage"]
                .as_str()
                .ok_or_else(|| io::Error::other("CodeGraph stage omitted its root"))?,
        );
        let digest = staged["manifest_sha256"]
            .as_str()
            .ok_or_else(|| io::Error::other("CodeGraph stage omitted its identity"))?;
        dependency_selection::activate(&state, "codegraph", &stage, digest, None)?;
        package = Some(stage.join("package"));
    }
    let inspected = dependency_discovery::inspect_package(&package.unwrap())?;
    let qualification = if read_only {
        None
    } else {
        Some(crate::codegraph_runtime::probe_package(&inspected.root)?)
    };
    let registration = json!({"command":manager,"args":["mcp","codegraph","--package-root",inspected.root],
        "env":{"CODEX_HOME":home},"startup_timeout_sec":30,"tool_timeout_sec":660});
    Ok(
        json!({"schema_version":1,"status":"prepared","read_only":read_only,"model_calls":0,
        "registrations":{"codegraph":registration},"retired":["codebase-memory"],
        "mcp":[{"id":"codegraph","package":inspected.package,"manager":"native",
            "status":"adopted","version":inspected.version,"installation_root":inspected.root,
            "paths":{"package_root":inspected.root,"node":inspected.node,"entry":inspected.entry,"manager":manager},
            "health":{"identity_verified":true,"runtime_verification":if read_only {"not-requested"} else {"protocol-probed"}},
            "provenance":{"archive_sha256":dependency_discovery::dependency_codegraph::ARCHIVE_SHA256,
                "tree_sha256":dependency_discovery::dependency_codegraph::TREE_SHA256}}],
        "qualification":qualification,
        "resources":{"status":"native-managed","provider":"codegraph",
            "owner":"Rust runtime enforces fixed Job, admission, operation and storage limits",
            "legacy_state":"preserved","runtime_verification":"not-established-by-configuration"},
        "activation":"prepared only; combined registration and acceptance belong to the installer"}),
    )
}
