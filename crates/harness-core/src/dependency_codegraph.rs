//! Pinned CodeGraph 1.6.0 Windows x64 published-package identity.
//! Discovery, Check and inspect are read-only; staging is explicit.
#![cfg(windows)]

use crate::{
    dependency_archive, dependency_assets::Asset, dependency_discovery,
    dependency_package as package, native_build, registration_native::StagedFile,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

pub const PACKAGE: &str = "@colbymchenry/codegraph";
pub const VERSION: &str = "1.6.0";
pub const ARCHIVE_NAME: &str = "codegraph-win32-x64.zip";
pub const ARCHIVE_SHA256: &str = "cd76c3c3391f2d40abef12b142151950b6d77abc2d8429e648f89eaa90f5b68a";
pub const ARCHIVE_BYTES: u64 = 52_593_717;
pub const ASSET_ID: u64 = 531_072_961;
pub const REPOSITORY_ID: &str = "1137078255";
pub const NODE: &str = "node.exe";
pub const ENTRY: &str = "lib/dist/bin/codegraph.js";
pub const KERNEL: &str = "lib/kernel/codegraph-kernel.node";
pub const MANIFEST: &str = "lib/package.json";
pub const NODE_SHA256: &str = "b3094d0b49f9ad602262a9921551737bb97637c05dd357a06ae98188d7290aa3";
pub const ENTRY_SHA256: &str = "0836f2e963bf76163223f7802a94971cc6e2b74a594508770c4f5d3fcbad97a3";
pub const KERNEL_SHA256: &str = "99c105ca0ec6831375eb4445716f508de72bf73eadc419a71f3d4d25f903a394";
pub const MANIFEST_SHA256: &str =
    "0112ff636d89344bb56f2e7ff6146f4cc0e0030f196e943648b33d63457f5937";
pub const TREE_SHA256: &str = "0fc0d0a6a81fafaeb9acdc74788a04366ea6644bfc2209f34c16a2cdd442bde4";
pub const TREE_FILE_COUNT: u64 = 940;
pub const TREE_PAYLOAD_BYTES: u64 = 261_469_875;
const COMPANION: &str = "lib/dist/mcp/tools.js";
const COMPANION_SHA256: &str = "033a979e706e434aeebc7154201d06da8f2737fde763b5ba91d0618c863fc6a3";
const ARCHIVE_PREFIX: &str = "codegraph-win32-x64/";
const METADATA: &str = "https://api.github.com/repos/colbymchenry/codegraph/releases/tags/v1.6.0";
const DOWNLOAD: &str =
    "https://github.com/colbymchenry/codegraph/releases/download/v1.6.0/codegraph-win32-x64.zip";
const SOURCE: &str = "https://github.com/colbymchenry/codegraph";
const MAX_MANIFEST: usize = 8 * 1024 * 1024;

/// Verified published runtime. Construction requires pinned file identity.
pub struct InspectedCodeGraph {
    pub root: PathBuf,
    pub node: PathBuf,
    pub entry: PathBuf,
    pub kernel: PathBuf,
    pub version: String,
    pub package: String,
    pub receipt: Option<PathBuf>,
}

impl InspectedCodeGraph {
    pub fn command(&self) -> Vec<PathBuf> {
        vec![self.node.clone(), self.entry.clone()]
    }

    pub fn report(&self) -> Value {
        json!({
            "schema_version": 1,
            "operation": "codegraph-package-inspection",
            "status": "integrity-verified",
            "package": self.package,
            "version": self.version,
            "root": self.root,
            "node": self.node,
            "entry": self.entry,
            "kernel": self.kernel,
            "command": self.command(),
            "node_flags": ["--liftoff-only", "--disable-warning=ExperimentalWarning"],
            "archive_name": ARCHIVE_NAME,
            "archive_sha256": ARCHIVE_SHA256,
            "receipt": self.receipt,
            "identity": "pinned-release-files",
            "pinned": true,
            "read_only": true,
            "network_requested": false,
            "package_code_executed": false,
            "model_calls": 0,
            "activation_allowed": false,
            "runtime_compatibility": "not-probed"
        })
    }
}

pub fn is_package(name: &str) -> bool {
    name == PACKAGE || name == "codegraph"
}

pub fn spec() -> Value {
    json!({
        "id": "codegraph",
        "package": PACKAGE,
        "manager": "github-release",
        "source": SOURCE,
        "metadata": METADATA,
        "runtime": ["bundled Node"],
        "command": "codegraph",
        "required": true
    })
}

pub(crate) fn metadata_url(version: &str) -> Result<&'static str, &'static str> {
    if version != VERSION {
        return Err("unsupported-metadata-source");
    }
    Ok(METADATA)
}

pub(crate) fn select(
    bytes: &[u8],
    version: &str,
    architecture: &str,
) -> Result<Asset, &'static str> {
    metadata_url(version)?;
    if architecture != "x86_64" {
        return Err("unsupported-native-platform");
    }
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("metadata-output-too-large");
    }
    let release: Value = serde_json::from_slice(bytes).map_err(|_| "unsupported-native-release")?;
    if release["tag_name"] != format!("v{VERSION}")
        || release["draft"] != false
        || release["prerelease"] != false
    {
        return Err("unsupported-native-release");
    }
    let mut matches = release["assets"]
        .as_array()
        .ok_or("unsupported-native-release")?
        .iter()
        .filter(|asset| asset["name"] == ARCHIVE_NAME);
    let asset = matches.next().ok_or("native-asset-unavailable")?;
    let digest = asset["digest"]
        .as_str()
        .and_then(|value| value.strip_prefix("sha256:"))
        .ok_or("native-asset-digest-unavailable")?;
    let size = asset["size"].as_u64().ok_or("unsupported-native-release")?;
    let id = asset["id"].as_u64().ok_or("unsupported-native-release")?;
    if matches.next().is_some()
        || asset["state"] != "uploaded"
        || asset["browser_download_url"] != DOWNLOAD
        || id != ASSET_ID
        || size != ARCHIVE_BYTES
        || digest.to_ascii_lowercase() != ARCHIVE_SHA256
    {
        return Err("unsupported-native-release");
    }
    Ok(Asset {
        url: DOWNLOAD.to_owned(),
        sha256: ARCHIVE_SHA256.to_owned(),
        size: ARCHIVE_BYTES,
        id: ASSET_ID,
    })
}

pub(crate) fn parse_release(bytes: &[u8]) -> Result<String, &'static str> {
    select(bytes, VERSION, "x86_64")?;
    Ok(VERSION.to_owned())
}

pub(crate) fn strip_archive_prefix(name: &str) -> io::Result<&str> {
    name.strip_prefix(ARCHIVE_PREFIX)
        .filter(|relative| !relative.is_empty() && !relative.ends_with('/'))
        .ok_or_else(package::invalid)
}

/// Read-only verification of a staged receipt or an already unpacked published root.
/// Layout presence alone is not identity; pinned file digests must match.
pub fn inspect_package(root: &Path) -> io::Result<InspectedCodeGraph> {
    let root = dependency_discovery::local_path(root)?;
    if root.join("request.json").is_file() && root.join("manifest.json").is_file() {
        return inspect_receipt(&root);
    }
    inspect_published_root(&root, None)
}

pub(crate) fn observe(dirs: &[PathBuf]) -> io::Result<Vec<Value>> {
    let mut candidates = Vec::new();
    for dir in dirs {
        let root = package::resolved(dir)?;
        if !root.is_dir() {
            continue;
        }
        candidates.push(record_from_root(&root)?);
    }
    Ok(candidates)
}

pub(crate) fn missing() -> Value {
    json!({
        "id": "codegraph",
        "identity": PACKAGE,
        "manager": "github-release",
        "version": Value::Null,
        "executable": Value::Null,
        "command": [],
        "installation_root": Value::Null,
        "status": "missing",
        "ownership": "not-found",
        "provenance": {
            "source": SOURCE,
            "metadata": METADATA,
            "package_identity": PACKAGE,
            "archive_name": ARCHIVE_NAME,
            "archive_sha256": ARCHIVE_SHA256
        },
        "health": {
            "installed": false,
            "identity_verified": false,
            "integrity": "unknown",
            "callable": Value::Null,
            "checked_operations": []
        },
        "update_safe": false,
        "active_consumers": {"state": "not-checked", "processes": []},
        "evidence": [],
        "candidates": [],
        "verification": {"state": "unverified", "evidence": []}
    })
}

pub(crate) fn unpack(root: &Path, bytes: &[u8], expected_sha256: &str) -> io::Result<Value> {
    if format!("{:x}", Sha256::digest(bytes)) != expected_sha256
        || expected_sha256 != ARCHIVE_SHA256
        || bytes.len() as u64 != ARCHIVE_BYTES
    {
        return Err(io::Error::other("native-asset-integrity-rejected"));
    }
    let candidate = root.join("package");
    native_build::ordinary_ancestors(&candidate)?;
    fs::create_dir(&candidate)?;
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let mut seen_dirs = std::collections::BTreeSet::new();
    let summary = dependency_archive::visit_zip(bytes, |name, size, reader| {
        let relative = strip_archive_prefix(name)?;
        let target = candidate.join(relative).components().collect::<PathBuf>();
        let parent = target.parent().ok_or_else(package::invalid)?;
        native_build::ordinary_ancestors(parent)?;
        fs::create_dir_all(parent)?;
        if let Ok(stripped) = parent.strip_prefix(&candidate) {
            let mut acc = String::new();
            for component in stripped.components() {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(
                    component
                        .as_os_str()
                        .to_str()
                        .ok_or_else(package::invalid)?,
                );
                if seen_dirs.insert(acc.clone()) {
                    directories.push(acc.clone());
                }
            }
        }
        let mut capture = Capture {
            input: reader,
            hash: Sha256::new(),
        };
        StagedFile::from_reader(&target, &mut capture, size)?.commit()?;
        files.push(json!({
            "path": relative,
            "size": size,
            "sha256": format!("{:x}", capture.hash.finalize())
        }));
        Ok(())
    })
    .map_err(|_| io::Error::other("native-candidate-archive-rejected"))?;
    inspect_published_root(&candidate, None)?;
    Ok(json!({
        "files": files,
        "directories": directories,
        "file_count": summary.files,
        "payload_bytes": summary.bytes
    }))
}

struct Capture<'a> {
    input: &'a mut dyn Read,
    hash: Sha256,
}

impl Read for Capture<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(output)?;
        self.hash.update(&output[..count]);
        Ok(count)
    }
}

fn inspect_receipt(stage: &Path) -> io::Result<InspectedCodeGraph> {
    let request_bytes = fs::read(stage.join("request.json")).map_err(|_| package::invalid())?;
    if request_bytes.len() > 64 * 1024 {
        return Err(package::invalid());
    }
    let request: Value = serde_json::from_slice(&request_bytes).map_err(|_| package::invalid())?;
    if request["schema"] != 1 || request["package"] != PACKAGE || request["version"] != VERSION {
        return Err(package::invalid());
    }
    let manifest_bytes = fs::read(stage.join("manifest.json")).map_err(|_| package::invalid())?;
    if manifest_bytes.len() > MAX_MANIFEST {
        return Err(package::invalid());
    }
    let envelope: Value =
        serde_json::from_slice(&manifest_bytes).map_err(|_| package::invalid())?;
    let report = &envelope["report"];
    if report["operation"] != "codegraph-candidate-preparation"
        || report["status"] != "staged-unverified"
        || report["package"] != PACKAGE
        || report["version"] != VERSION
        || report["native_asset"]["archive_sha256"] != ARCHIVE_SHA256
        || report["native_asset"]["asset_id"] != ASSET_ID
        || report["native_asset"]["archive_bytes"] != ARCHIVE_BYTES
        || report["activation_allowed"] != false
        || report["package_code_executed"] != false
    {
        return Err(package::invalid());
    }
    let inspected =
        inspect_published_root(&stage.join("package"), Some(stage.join("manifest.json")))?;
    verify_receipt_pins(&envelope["contents"]["files"])?;
    Ok(inspected)
}

fn verify_receipt_pins(files: &Value) -> io::Result<()> {
    let files = files.as_array().ok_or_else(package::invalid)?;
    if files.len() as u64 != TREE_FILE_COUNT {
        return Err(package::invalid());
    }
    let mut ordered = files.to_vec();
    ordered.sort_by(|left, right| {
        left["path"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["path"].as_str().unwrap_or_default())
    });
    let mut payload = 0u64;
    let mut hasher = Sha256::new();
    let mut seen = BTreeMap::new();
    for file in &ordered {
        let path = file["path"].as_str().ok_or_else(package::invalid)?;
        let size = file["size"].as_u64().ok_or_else(package::invalid)?;
        let digest = file["sha256"].as_str().ok_or_else(package::invalid)?;
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(package::invalid());
        }
        let digest = digest.to_ascii_lowercase();
        if seen.insert(path.to_owned(), ()).is_some() {
            return Err(package::invalid());
        }
        payload = payload.checked_add(size).ok_or_else(package::invalid)?;
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(size.to_string().as_bytes());
        hasher.update([0]);
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    if payload != TREE_PAYLOAD_BYTES || format!("{:x}", hasher.finalize()) != TREE_SHA256 {
        return Err(package::invalid());
    }
    for (path, digest) in [
        (NODE, NODE_SHA256),
        (ENTRY, ENTRY_SHA256),
        (KERNEL, KERNEL_SHA256),
        (MANIFEST, MANIFEST_SHA256),
        (COMPANION, COMPANION_SHA256),
    ] {
        let file = files
            .iter()
            .find(|file| file["path"] == path)
            .ok_or_else(package::invalid)?;
        if file["sha256"] != digest {
            return Err(package::invalid());
        }
    }
    Ok(())
}

fn inspect_published_root(root: &Path, receipt: Option<PathBuf>) -> io::Result<InspectedCodeGraph> {
    let root = dependency_discovery::local_path(root)?;
    let observed = observe_published_tree(&root)?;
    if observed.len() as u64 != TREE_FILE_COUNT {
        return Err(package::invalid());
    }
    let mut payload = 0u64;
    let mut hasher = Sha256::new();
    for (path, size, digest) in &observed {
        payload = payload.checked_add(*size).ok_or_else(package::invalid)?;
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(size.to_string().as_bytes());
        hasher.update([0]);
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    if payload != TREE_PAYLOAD_BYTES || format!("{:x}", hasher.finalize()) != TREE_SHA256 {
        return Err(package::invalid());
    }
    let node = required_pinned(&root, NODE, NODE_SHA256)?;
    let entry = required_pinned(&root, ENTRY, ENTRY_SHA256)?;
    let kernel = required_pinned(&root, KERNEL, KERNEL_SHA256)?;
    let manifest_path = required_pinned(&root, MANIFEST, MANIFEST_SHA256)?;
    required_pinned(&root, COMPANION, COMPANION_SHA256)?;
    let Some(manifest) = package::read_json(&manifest_path)? else {
        return Err(package::invalid());
    };
    if manifest["name"] != PACKAGE || manifest["version"] != VERSION {
        return Err(package::invalid());
    }
    Ok(InspectedCodeGraph {
        root,
        node,
        entry,
        kernel,
        version: VERSION.to_owned(),
        package: PACKAGE.to_owned(),
        receipt,
    })
}

fn observe_published_tree(root: &Path) -> io::Result<Vec<(String, u64, String)>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        native_build::ordinary_ancestors(&directory)?;
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(package::invalid());
            }
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if !metadata.is_file() {
                return Err(package::invalid());
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| package::invalid())?
                .to_str()
                .ok_or_else(package::invalid)?
                .replace('\\', "/");
            if relative.is_empty() || files.contains_key(&relative) {
                return Err(package::invalid());
            }
            if !package::contained(&path, root) {
                return Err(package::invalid());
            }
            files.insert(relative, (metadata.len(), package::fingerprint(&path)?));
        }
    }
    Ok(files
        .into_iter()
        .map(|(path, (size, digest))| (path, size, digest))
        .collect())
}

fn required_pinned(root: &Path, relative: &str, digest: &str) -> io::Result<PathBuf> {
    let path = package::package_file(root, relative)?;
    if !path.is_file() || !package::contained(&path, root) {
        return Err(package::invalid());
    }
    if package::fingerprint(&path)? != digest {
        return Err(package::invalid());
    }
    Ok(path)
}

fn record_from_root(root: &Path) -> io::Result<Value> {
    match inspect_package(root) {
        Ok(package) => {
            let mut record = missing();
            record["version"] = json!(package.version);
            record["executable"] = json!(package.node);
            record["command"] = json!(package.command());
            record["installation_root"] = json!(package.root);
            record["status"] = json!("adopted");
            record["ownership"] = json!("adopted-shared");
            record["health"]["installed"] = json!(true);
            record["health"]["identity_verified"] = json!(true);
            record["health"]["integrity"] = json!("pinned-release-files");
            record["paths"] = json!({
                "node": package.node,
                "entry": package.entry,
                "kernel": package.kernel,
                "module_root": package.root
            });
            record["evidence"] = json!([
                {
                    "kind": "native-payload-fingerprint",
                    "path": package.node,
                    "sha256": NODE_SHA256
                },
                {
                    "kind": "entrypoint-fingerprint",
                    "path": package.entry,
                    "sha256": ENTRY_SHA256
                },
                {
                    "kind": "pinned-archive",
                    "archive_sha256": ARCHIVE_SHA256,
                    "asset_id": ASSET_ID
                }
                ,{
                    "kind": "pinned-tree",
                    "sha256": TREE_SHA256,
                    "file_count": TREE_FILE_COUNT,
                    "payload_bytes": TREE_PAYLOAD_BYTES
                }
            ]);
            Ok(record)
        }
        Err(_) => {
            let mut record = missing();
            record["installation_root"] = json!(root);
            record["status"] = json!("broken");
            record["ownership"] = json!("adopted-shared");
            record["health"]["integrity"] = json!("unverified-layout");
            record["evidence"] = json!([{
                "kind": "unverified-layout",
                "path": root,
                "detail": "Observed CodeGraph files are not the pinned 1.6.0 Windows release."
            }]);
            Ok(record)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release() -> Value {
        json!({
            "tag_name": "v1.6.0",
            "draft": false,
            "prerelease": false,
            "assets": [{
                "name": ARCHIVE_NAME,
                "id": ASSET_ID,
                "size": ARCHIVE_BYTES,
                "state": "uploaded",
                "digest": format!("sha256:{ARCHIVE_SHA256}"),
                "browser_download_url": DOWNLOAD
            }]
        })
    }

    #[test]
    fn pinned_windows_asset_rejects_checksum_and_platform_substitution() {
        let bytes = serde_json::to_vec(&release()).unwrap();
        let asset = select(&bytes, VERSION, "x86_64").unwrap();
        assert_eq!(asset.sha256, ARCHIVE_SHA256);
        assert_eq!(asset.size, ARCHIVE_BYTES);
        assert_eq!(asset.id, ASSET_ID);
        assert_eq!(parse_release(&bytes).unwrap(), VERSION);
        assert!(select(&bytes, "1.6.1", "x86_64").is_err());
        assert!(select(&bytes, VERSION, "aarch64").is_err());
        let mut wrong = release();
        wrong["assets"][0]["digest"] = json!(format!("sha256:{}", "a".repeat(64)));
        assert_eq!(
            select(&serde_json::to_vec(&wrong).unwrap(), VERSION, "x86_64")
                .err()
                .unwrap(),
            "unsupported-native-release"
        );
        let mut renamed = release();
        renamed["assets"][0]["name"] = json!("codegraph-win32-arm64.zip");
        assert!(select(&serde_json::to_vec(&renamed).unwrap(), VERSION, "x86_64").is_err());
    }

    #[test]
    fn observed_layout_without_pinned_hashes_is_not_identity() {
        let root = tempfile::tempdir().unwrap();
        let pkg = root.path().join("codegraph-win32-x64");
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
        assert!(inspect_package(&pkg).is_err());
        let record = record_from_root(&pkg).unwrap();
        assert_eq!(record["status"], "broken");
        assert_eq!(record["health"]["identity_verified"], false);
        assert_eq!(record["health"]["integrity"], "unverified-layout");
    }

    fn copy_dir(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for entry in fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let from = entry.path();
            let to = dst.join(entry.file_name());
            if from.is_dir() {
                copy_dir(&from, &to);
            } else {
                fs::copy(&from, &to).unwrap();
            }
        }
    }

    #[test]
    #[ignore = "requires CODEGRAPH_ACCEPTANCE_PACKAGE pointing to the pinned published tree"]
    fn official_tree_is_adopted_and_companion_mutation_or_removal_is_refused() {
        let official = PathBuf::from(
            std::env::var_os("CODEGRAPH_ACCEPTANCE_PACKAGE")
                .expect("explicit pinned published package"),
        );
        let inspected = inspect_package(&official).unwrap();
        assert_eq!(inspected.package, PACKAGE);
        assert_eq!(inspected.version, VERSION);
        assert_eq!(inspected.command().len(), 2);

        let root = tempfile::tempdir().unwrap();
        let pkg = root.path().join("codegraph-win32-x64");
        copy_dir(&official, &pkg);
        inspect_package(&pkg).unwrap();

        let companion = pkg.join(COMPANION.replace('/', "\\"));
        fs::write(&companion, b"mutated companion").unwrap();
        assert!(inspect_package(&pkg).is_err());
        fs::copy(official.join(COMPANION.replace('/', "\\")), &companion).unwrap();
        inspect_package(&pkg).unwrap();
        fs::remove_file(&companion).unwrap();
        assert!(inspect_package(&pkg).is_err());
    }

    #[test]
    fn receipt_with_incomplete_file_list_is_not_identity() {
        let files = json!([
            {"path": NODE, "size": 1, "sha256": NODE_SHA256},
            {"path": ENTRY, "size": 1, "sha256": ENTRY_SHA256},
            {"path": KERNEL, "size": 1, "sha256": KERNEL_SHA256},
            {"path": MANIFEST, "size": 1, "sha256": MANIFEST_SHA256}
        ]);
        assert!(verify_receipt_pins(&files).is_err());
    }

    #[test]
    fn unpack_rejects_checksum_mismatch_before_creating_a_candidate() {
        let root = tempfile::tempdir().unwrap();
        let err = unpack(root.path(), b"not-the-official-archive", ARCHIVE_SHA256).unwrap_err();
        assert!(err.to_string().contains("native-asset-integrity-rejected"));
        assert!(!root.path().join("package").exists());
    }

    #[test]
    fn archive_prefix_must_be_the_published_windows_root() {
        assert_eq!(
            strip_archive_prefix("codegraph-win32-x64/node.exe").unwrap(),
            "node.exe"
        );
        assert!(strip_archive_prefix("node.exe").is_err());
        assert!(strip_archive_prefix("codegraph-win32-x64/").is_err());
        assert!(strip_archive_prefix("other/node.exe").is_err());
    }
}
