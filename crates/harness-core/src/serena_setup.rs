//! Explicit prerequisite preparation for the independent Serena plugin.
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
};

pub const REGISTRY_NAME: &str = "code-tools.json";

#[derive(Clone, Debug)]
pub struct Request {
    pub catalogue: PathBuf,
    pub user_home: PathBuf,
    pub state: PathBuf,
}

pub fn registry_path(state: &Path) -> PathBuf {
    state.join(REGISTRY_NAME)
}

/// Read-only readiness observation. This never creates state or acquires a package.
pub fn inspect(request: &Request) -> io::Result<Value> {
    let registry = registry_path(&request.state);
    if registry.is_file() {
        let value: Value = serde_json::from_slice(&std::fs::read(&registry)?)?;
        return Ok(json!({
            "schema_version": 1,
            "operation": "serena-setup-inspect",
            "status": "ready",
            "ready": true,
            "registry": registry,
            "identities": {
                "serena": value["mcp"].as_array().into_iter().flatten()
                    .find(|row| row["id"] == "serena")
                    .and_then(|row| row["version"].as_str()),
                "python": value["languages"].as_array().into_iter().flatten()
                    .find(|row| row["id"] == "python")
                    .and_then(|row| row["version"].as_str())
            },
            "model_calls": 0,
            "mutated": false
        }));
    }
    Ok(json!({
        "schema_version": 1,
        "operation": "serena-setup-inspect",
        "status": "preparation-required",
        "ready": false,
        "registry": registry,
        "reason": "Semantic prerequisites have not been prepared.",
        "model_calls": 0,
        "mutated": false
    }))
}

pub fn prepare(_request: &Request) -> io::Result<Value> {
    Err(io::Error::other(
        "explicit Serena dependency preparation is unavailable in this build",
    ))
}
