//! Explicit plan-action dispatcher for native dependency candidates.
//! Check and preview stay read-only. Mutation stages and selects only the
//! native slots; remaining backends stay pending for Python apply_selected.
#![cfg(windows)]

use crate::{
    build_identity,
    dependency_discovery::{self, Request as DiscoveryRequest, local_path},
    dependency_plan, dependency_releases, dependency_selection, dependency_stage, inventory,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

const NATIVE_BACKEND_PENDING: &str =
    "Required backend-specific provisioning/update remains on Python apply_selected until ported.";

pub struct Request {
    pub source: PathBuf,
    pub user_home: PathBuf,
    pub state: PathBuf,
    pub manager: PathBuf,
    pub preview: bool,
    pub check: bool,
    pub node: Option<PathBuf>,
    pub node_sha256: Option<String>,
}

fn invalid() -> io::Error {
    io::Error::other("invalid dependency apply options")
}

fn conflict(message: &str) -> io::Error {
    io::Error::other(message)
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

fn native_slot(id: &str) -> Option<&'static str> {
    match id {
        "codegraph" => Some("codegraph"),
        "python" => Some("basedpyright"),
        _ => None,
    }
}

fn staging_package(id: &str) -> Option<&'static str> {
    match id {
        "codegraph" => Some(crate::dependency_discovery::dependency_codegraph::PACKAGE),
        "python" => Some("basedpyright"),
        _ => None,
    }
}

fn pinned_node(request: &Request) -> io::Result<Option<(PathBuf, String)>> {
    match (&request.node, &request.node_sha256) {
        (None, None) => Ok(None),
        (Some(path), Some(digest)) => {
            let path = local_path(path)?;
            let digest = digest.to_ascii_lowercase();
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(invalid());
            }
            if build_identity::hash_file(&path)? != digest {
                return Err(invalid());
            }
            Ok(Some((path, digest)))
        }
        _ => Err(invalid()),
    }
}

fn complete_state(state: &str) -> bool {
    matches!(
        state,
        "reused" | "updated" | "conditional-absent" | "retained-compatible" | "previewed"
    )
}

fn result_item(id: &str, state: &str, extra: Value) -> Value {
    let mut result = extra;
    if !result.is_object() {
        result = json!({});
    }
    result["id"] = json!(id);
    result["state"] = json!(state);
    result
}

fn pending(id: &str, reason: &str, plan: &Value) -> Value {
    result_item(
        id,
        "pending",
        json!({"reason": reason, "plan": plan, "packages_acquired": false}),
    )
}

fn apply_native(request: &Request, item: &Value, activating: bool) -> io::Result<Value> {
    let id = item["id"].as_str().ok_or_else(invalid)?;
    let action = item["action"].as_str().unwrap_or("");
    let version = item["installed_version"].clone();
    match action {
        "conditional-absent" => Ok(result_item(
            id,
            "conditional-absent",
            json!({"packages_acquired": false}),
        )),
        "reuse" => Ok(result_item(
            id,
            "reused",
            json!({"version": version, "packages_acquired": false}),
        )),
        "update-pending-consumers" => Ok(result_item(
            id,
            "retained-compatible",
            json!({
                "version": version,
                "available_version": item["release"]["version"],
                "update_state": "pending-consumers",
                "packages_acquired": false,
                "reason": "Installed shared dependency retained while active consumers use it; no process is stopped."
            }),
        )),
        "stage-compatible-update" if id == "rust" => Ok(result_item(
            id,
            "retained-compatible",
            json!({
                "version": version,
                "available_cohort": item["release"]["version"],
                "update_state": "held-toolchain-policy",
                "packages_acquired": false,
                "reason": "The adopted rustup component belongs to the installed compiler cohort. Updating that toolchain as a side effect would change project/compiler behavior; preserve it. Official component metadata itself uses placeholder 0.0.0."
            }),
        )),
        "preserve-and-audit" | "metadata-unresolved" => Ok(pending(
            id,
            item["reason"]
                .as_str()
                .unwrap_or("Required backend-specific provisioning/update remains unfinished."),
            item,
        )),
        "install-required" | "stage-compatible-update" => {
            if !activating {
                return Ok(result_item(
                    id,
                    "previewed",
                    json!({
                        "action": action,
                        "packages_acquired": false,
                        "reason": "Check and preview do not stage, select or acquire packages."
                    }),
                ));
            }
            let Some(slot) = native_slot(id) else {
                return Ok(pending(id, NATIVE_BACKEND_PENDING, item));
            };
            let Some(package) = staging_package(id) else {
                return Ok(pending(id, NATIVE_BACKEND_PENDING, item));
            };
            let Some(release) = item["release"]["version"].as_str() else {
                return Ok(pending(
                    id,
                    "Compatible version selection is unresolved; no package is acquired.",
                    item,
                ));
            };
            let node = if id == "python" {
                match pinned_node(request)? {
                    Some(node) => Some(node),
                    None => {
                        return Ok(pending(
                            id,
                            "BasedPyright native apply requires an explicit Node path and digest; no package is acquired.",
                            item,
                        ));
                    }
                }
            } else {
                None
            };
            let staged =
                dependency_stage::prepare(&request.manager, package, release, &request.state)?;
            let digest = staged["manifest_sha256"]
                .as_str()
                .ok_or_else(|| io::Error::other("native stage omitted its identity"))?;
            let stage = PathBuf::from(
                staged["stage"]
                    .as_str()
                    .ok_or_else(|| io::Error::other("native stage omitted its root"))?,
            );
            let selected = dependency_selection::activate(
                &request.state,
                slot,
                &stage,
                digest,
                node.as_ref()
                    .map(|(path, digest)| (path.as_path(), digest.as_str())),
            )?;
            Ok(result_item(
                id,
                "updated",
                json!({
                    "version": release,
                    "slot": slot,
                    "packages_acquired": true,
                    "global_registration_changed": selected["global_registration_changed"],
                    "selection": selected,
                    "stage": staged
                }),
            ))
        }
        _ => Ok(pending(id, NATIVE_BACKEND_PENDING, item)),
    }
}

/// Apply explicit plan actions. Check/preview never create homes, stage
/// candidates or stop shared OpenCode consumers.
pub fn apply(request: &Request) -> io::Result<Value> {
    let source = local_path(&request.source)?;
    let home = local_path(&request.user_home)?;
    let state = local_path(&request.state)?;
    let manager = local_path(&request.manager)?;
    let read_only = request.preview || request.check;
    let catalogue = source.join("global/code-tools.json");
    if !catalogue.is_file() {
        return Err(invalid());
    }
    let discovery = DiscoveryRequest {
        catalogue,
        user_home: home.clone(),
        processes: !read_only,
        ..DiscoveryRequest::default()
    };
    let planned = if read_only {
        let inventory = dependency_discovery::discover(&discovery)?;
        let mut releases = BTreeMap::new();
        for group in ["mcp", "languages"] {
            for spec in inventory
                .get(group)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(id) = spec.get("id").and_then(Value::as_str) {
                    releases.insert(
                        id.to_owned(),
                        json!({"source":Value::Null,"state":"unresolved","version":Value::Null,"reason":"metadata-not-requested"}),
                    );
                }
            }
        }
        let catalogue_bytes = fs::read(&discovery.catalogue)?;
        let catalogue: Value = serde_json::from_slice(&catalogue_bytes)?;
        dependency_plan::plan(&catalogue, &inventory, &releases)?
    } else {
        dependency_releases::plan(&discovery)?
    };
    if read_only {
        let owned = Request {
            source: source.clone(),
            user_home: home.clone(),
            state: state.clone(),
            manager: manager.clone(),
            preview: request.preview,
            check: request.check,
            node: request.node.clone(),
            node_sha256: request.node_sha256.clone(),
        };
        let results: Vec<Value> = planned["items"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|item| apply_native(&owned, item, false))
            .collect::<io::Result<_>>()?;
        return Ok(json!({
            "schema_version": 1,
            "operation": "apply",
            "read_only": true,
            "preview": request.preview,
            "check": request.check,
            "model_calls": 0,
            "packages_acquired": false,
            "mutated": false,
            "results": results,
            "complete": results.iter().all(|row| complete_state(row["state"].as_str().unwrap_or(""))),
            "all_updates_applied": false,
            "note": "Check and preview do not stage, select, acquire packages or stop shared consumers."
        }));
    }
    let opencode = snapshot_opencode_cache(&home)?;
    let mut results = Vec::new();
    for item in planned["items"].as_array().into_iter().flatten() {
        results.push(apply_native(request, item, true)?);
    }
    preserved_opencode_cache(&opencode)?;
    let complete = results
        .iter()
        .all(|row| complete_state(row["state"].as_str().unwrap_or("")));
    let all_updates_applied = !results.iter().any(|row| {
        row.get("update_state").is_some()
            || matches!(row["state"].as_str(), Some("pending" | "failed"))
    });
    Ok(json!({
        "schema_version": 1,
        "operation": "apply",
        "read_only": false,
        "preview": false,
        "check": false,
        "model_calls": 0,
        "packages_acquired": results.iter().any(|row| row["packages_acquired"] == true),
        "mutated": results.iter().any(|row| row["state"] == "updated"),
        "results": results,
        "complete": complete,
        "all_updates_applied": all_updates_applied,
        "note": "Native slots may be staged and selected; remaining backends stay pending for Python apply_selected. Installed-unverified records require real consumer checks."
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn request(root: &Path, preview: bool, check: bool) -> Request {
        Request {
            source: repo(),
            user_home: root.join("user"),
            state: root.join("state"),
            manager: std::env::current_exe().unwrap(),
            preview,
            check,
            node: None,
            node_sha256: None,
        }
    }

    fn write_opencode(root: &Path) -> PathBuf {
        let clangd = root.join("user/.cache/opencode/bin/clangd_20/bin");
        fs::create_dir_all(&clangd).unwrap();
        let binary = clangd.join("clangd.exe");
        fs::write(&binary, b"shared-opencode-cache").unwrap();
        binary
    }

    #[test]
    fn check_and_preview_are_silent_and_preserve_opencode_cache() {
        let root = tempfile::tempdir().unwrap();
        let cache = write_opencode(root.path());
        let before = fs::read(&cache).unwrap();
        for (preview, check) in [(true, false), (false, true)] {
            let report = apply(&request(root.path(), preview, check)).unwrap();
            assert_eq!(report["read_only"], true);
            assert_eq!(report["mutated"], false);
            assert_eq!(report["packages_acquired"], false);
            assert_eq!(report["model_calls"], 0);
            assert!(!root.path().join("state").exists());
            assert!(!root.path().join("user/.codex").exists());
            assert_eq!(fs::read(&cache).unwrap(), before);
            let results = report["results"].as_array().unwrap();
            assert!(!results.is_empty());
            for item in results {
                assert_ne!(item["state"], "updated");
                assert_eq!(item["packages_acquired"], false);
            }
        }
    }

    #[test]
    fn apply_native_helper_does_not_stop_shared_consumers_or_claim_python_backends() {
        let rust = apply_native(
            &request(Path::new("D:/unused"), false, false),
            &json!({
                "id":"rust",
                "action":"install-required",
                "identity":"rust-analyzer",
                "installed_version":"1.97.1",
                "release":{"state":"checked","version":"1.98.0"}
            }),
            true,
        )
        .unwrap();
        assert_eq!(rust["state"], "pending");
        assert_eq!(rust["packages_acquired"], false);
        let consumers = apply_native(
            &request(Path::new("D:/unused"), false, false),
            &json!({
                "id":"nuphus",
                "action":"update-pending-consumers",
                "installed_version":"0.2.1",
                "release":{"state":"checked","version":"0.2.2"}
            }),
            true,
        )
        .unwrap();
        assert_eq!(consumers["state"], "retained-compatible");
        assert_eq!(consumers["update_state"], "pending-consumers");
        assert!(
            consumers["reason"]
                .as_str()
                .unwrap()
                .contains("no process is stopped")
        );
    }

    #[test]
    fn basedpyright_node_digest_mismatch_is_refused_without_acquiring() {
        let root = tempfile::tempdir().unwrap();
        let node = root.path().join("node.exe");
        fs::write(&node, b"owned-node-fixture").unwrap();
        let mut request = request(root.path(), false, false);
        request.node = Some(node);
        request.node_sha256 = Some("0".repeat(64));
        assert!(
            apply_native(
                &request,
                &json!({
                    "id":"python",
                    "action":"install-required",
                    "identity":"basedpyright",
                    "release":{"state":"checked","version":"1.39.10"}
                }),
                true,
            )
            .is_err()
        );
        assert!(!root.path().join("state").exists());
    }

    #[test]
    fn non_codegraph_backends_stay_pending_for_python() {
        let result = apply_native(
            &request(Path::new("D:/unused"), false, false),
            &json!({
                "id":"python",
                "action":"install-required",
                "identity":"basedpyright",
                "release":{"state":"checked","version":"1.39.10"}
            }),
            true,
        )
        .unwrap();
        assert_eq!(result["state"], "pending");
        assert_eq!(result["packages_acquired"], false);
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .contains("explicit Node path and digest")
        );
        let nuphus = apply_native(
            &request(Path::new("D:/unused"), false, false),
            &json!({
                "id":"nuphus",
                "action":"install-required",
                "identity":"@nuphus/nuphus-mcp",
                "release":{"state":"checked","version":"0.2.2"}
            }),
            true,
        )
        .unwrap();
        assert_eq!(nuphus["state"], "pending");
        assert_eq!(nuphus["packages_acquired"], false);
    }

    #[test]
    fn reuse_and_absent_actions_do_not_acquire_packages() {
        let reused = apply_native(
            &request(Path::new("D:/unused"), false, false),
            &json!({"id":"serena","action":"reuse","installed_version":"1.7.0"}),
            true,
        )
        .unwrap();
        assert_eq!(reused["state"], "reused");
        assert_eq!(reused["packages_acquired"], false);
        let absent = apply_native(
            &request(Path::new("D:/unused"), false, false),
            &json!({"id":"qml","action":"conditional-absent"}),
            true,
        )
        .unwrap();
        assert_eq!(absent["state"], "conditional-absent");
    }

    #[test]
    fn rust_stage_compatible_update_is_held_to_the_installed_cohort() {
        let result = apply_native(
            &request(Path::new("D:/unused"), false, false),
            &json!({
                "id":"rust",
                "action":"stage-compatible-update",
                "installed_version":"1.97.1",
                "release":{"state":"checked","version":"1.98.0"}
            }),
            true,
        )
        .unwrap();
        assert_eq!(result["state"], "retained-compatible");
        assert_eq!(result["update_state"], "held-toolchain-policy");
        assert_eq!(result["packages_acquired"], false);
        assert_eq!(result["available_cohort"], "1.98.0");
    }
}
