//! Explicit preparation of an owned, unactivated npm package candidate.
#![cfg(windows)]
use crate::{
    dependency_archive, dependency_assets, dependency_audit, dependency_discovery,
    dependency_fetch::Client,
    native_build,
    process::{CommandSpec, StopReason},
    registration_native::{ReadGuard, StagedFile},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::Duration,
};

const MAX_DOCUMENT: u64 = 16 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 128 * 1024 * 1024;
const MAX_MANIFEST: usize = 8 * 1024 * 1024;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "dependency staging input or candidate was rejected",
    )
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: u32,
    state: PathBuf,
    package: String,
    version: String,
}

fn read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut guard = ReadGuard::open(path)?;
    let mut bytes = Vec::new();
    (&mut guard.file).take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(bytes)
}

struct Capture<'a> {
    input: &'a mut dyn Read,
    hash: Sha256,
    manifest: Option<Vec<u8>>,
}
impl Read for Capture<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let count = self.input.read(output)?;
        self.hash.update(&output[..count]);
        if let Some(manifest) = &mut self.manifest {
            if manifest.len() + count > 4 * 1024 * 1024 {
                return Err(invalid());
            }
            manifest.extend_from_slice(&output[..count]);
        }
        Ok(count)
    }
}

fn unpack(
    root: &Path,
    package: &str,
    version: &str,
    bytes: &[u8],
    integrity: &str,
) -> io::Result<Value> {
    dependency_archive::verify_sri(bytes, integrity)?;
    let candidate = root.join("package");
    // The parent owns this new scope; existing data is never adopted or merged.
    fs::create_dir(&candidate)?;
    let mut files = Vec::new();
    let mut directories = Vec::new();
    let manifest_budget = std::cell::Cell::new(0usize);
    let charge = |path: &str| -> io::Result<()> {
        let next = manifest_budget
            .get()
            .checked_add(path.len() + 256)
            .ok_or_else(invalid)?;
        if next > MAX_MANIFEST {
            return Err(invalid());
        }
        manifest_budget.set(next);
        Ok(())
    };
    let mut manifest_found = false;
    let summary = dependency_archive::visit_npm_tar_with_directories(
        bytes,
        |path| {
            if path.is_empty() {
                return Ok(());
            }
            charge(path)?;
            let directory = candidate.join(path).components().collect::<PathBuf>();
            native_build::ordinary_ancestors(&directory)?;
            fs::create_dir_all(&directory)?;
            native_build::ordinary_ancestors(&directory)?;
            directories.push(path.to_owned());
            Ok(())
        },
        |path, size, reader| {
            charge(path)?;
            let target = candidate.join(path).components().collect::<PathBuf>();
            let parent = target.parent().ok_or_else(invalid)?;
            native_build::ordinary_ancestors(parent)?;
            fs::create_dir_all(parent)?;
            let mut capture = Capture {
                input: reader,
                hash: Sha256::new(),
                manifest: (path == "package.json").then(Vec::new),
            };
            let staged = StagedFile::from_reader(&target, &mut capture, size)?;
            if let Some(manifest) = &capture.manifest {
                if dependency_audit::identity(manifest)? != (package.to_owned(), version.to_owned())
                {
                    return Err(invalid());
                }
                manifest_found = true;
            }
            let hash = format!("{:x}", capture.hash.finalize());
            // This commits one file only inside the owned tentative candidate.
            // No installation can use it until whole-archive and later runtime
            // acceptance plus a separate journaled promotion have succeeded.
            staged.commit()?;
            files.push(json!({"path":path,"size":size,"sha256":hash}));
            Ok(())
        },
    )?;
    if !manifest_found {
        return Err(invalid());
    }
    Ok(
        json!({"files":files,"directories":directories,"file_count":summary.files,"payload_bytes":summary.bytes}),
    )
}

fn codebase_binary(
    client: &Client,
    root: &Path,
    version: &str,
    listing: &mut Value,
) -> io::Result<Value> {
    let url = dependency_assets::metadata_url(version).map_err(io::Error::other)?;
    let metadata = client
        .fetch_validated(&url, MAX_DOCUMENT)
        .map_err(io::Error::other)?;
    let asset = dependency_assets::select(&metadata, version, std::env::consts::ARCH)
        .map_err(io::Error::other)?;
    let bytes = client.github_asset(&asset).map_err(io::Error::other)?;
    if bytes.len() as u64 != asset.size || format!("{:x}", Sha256::digest(&bytes)) != asset.sha256 {
        return Err(io::Error::other("native-asset-integrity-rejected"));
    }
    let mut binary = None;
    dependency_archive::visit_zip(&bytes, |name: &str, size: u64, reader: &mut dyn Read| {
        if name.rsplit('/').next() != Some("codebase-memory-mcp.exe") { return Ok(()); }
        if binary.is_some() { return Err(invalid()); }
        let destination = root.join("package/bin/codebase-memory-mcp.exe").components().collect::<PathBuf>();
        native_build::ordinary_ancestors(destination.parent().ok_or_else(invalid)?)?;
        fs::create_dir_all(destination.parent().ok_or_else(invalid)?)?;
        let mut capture = Capture { input:reader, hash:Sha256::new(), manifest:None };
        StagedFile::from_reader(&destination, &mut capture, size)?.commit()?;
        binary = Some(json!({"path":"bin/codebase-memory-mcp.exe","size":size,"sha256":format!("{:x}",capture.hash.finalize())}));
        Ok(())
    }).map_err(|_| io::Error::other("native-candidate-archive-rejected"))?;
    let binary = binary.ok_or_else(|| io::Error::other("native-candidate-executable-missing"))?;
    let size = binary["size"].as_u64().ok_or_else(invalid)?;
    listing["files"]
        .as_array_mut()
        .ok_or_else(invalid)?
        .push(binary);
    listing["file_count"] = json!(listing["file_count"].as_u64().ok_or_else(invalid)? + 1);
    listing["payload_bytes"] = json!(listing["payload_bytes"].as_u64().ok_or_else(invalid)? + size);
    Ok(
        json!({"metadata":url,"metadata_sha256":format!("{:x}",Sha256::digest(&metadata)),"asset_id":asset.id,"archive":asset.url,"archive_sha256":asset.sha256,"archive_bytes":asset.size}),
    )
}

/// Internal bounded manager consumer. No foreign entrypoint is executed here.
pub fn worker(root: &Path) -> io::Result<Value> {
    dependency_audit::worker_limits()?;
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(120));
        std::process::exit(124);
    });
    let root = dependency_discovery::local_path(root).map_err(|_| invalid())?;
    let request_bytes = read(&root.join("request.json"), 64 * 1024)?;
    let request: Request = serde_json::from_slice(&request_bytes).map_err(|_| invalid())?;
    let state = dependency_discovery::local_path(&request.state).map_err(|_| invalid())?;
    if request.schema != 1
        || root.parent() != Some(state.join("dependency-staging").as_path())
        || root
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| !name.starts_with("candidate-"))
        || fs::read_dir(&root)?.count() != 1
    {
        return Err(invalid());
    }
    native_build::verify_owned_state(&state)?;
    let (metadata_url, _) = dependency_audit::endpoints(&request.package, &request.version)?;
    let client = Client::new().map_err(io::Error::other)?;
    let metadata = client
        .fetch_validated(&metadata_url, MAX_DOCUMENT)
        .map_err(io::Error::other)?;
    let (archive_url, integrity) =
        dependency_audit::release(&metadata, &request.package, &request.version)?;
    let bytes = client
        .fetch_validated(&archive_url, MAX_ARCHIVE)
        .map_err(io::Error::other)?;
    let mut listing = unpack(
        &root,
        &request.package,
        &request.version,
        &bytes,
        &integrity,
    )
    .map_err(|_| io::Error::other("npm-candidate-archive-rejected"))?;
    let native_asset = if request.package == "codebase-memory-mcp" {
        codebase_binary(&client, &root, &request.version, &mut listing)?
    } else {
        Value::Null
    };
    let mut report = json!({
        "schema_version":1,"operation":"npm-candidate-preparation","status":"staged-unverified",
        "package":request.package,"version":request.version,"stage":root,"candidate":root.join("package"),
        "source":{"metadata":metadata_url,"archive":archive_url,"integrity":integrity,"metadata_sha256":format!("{:x}",Sha256::digest(&metadata)),"archive_sha256":format!("{:x}",Sha256::digest(&bytes)),"client_version":client.version},"native_asset":native_asset,
        "request_sha256":format!("{:x}",Sha256::digest(&request_bytes)),
        "file_count":listing["file_count"],"payload_bytes":listing["payload_bytes"],
        "activation_allowed":false,"runtime_compatibility":"not-probed","package_code_executed":false,"model_calls":0
    });
    let manifest = serde_json::to_vec(&json!({"report":report,"contents":listing}))?;
    if manifest.len() > MAX_MANIFEST {
        return Err(invalid());
    }
    StagedFile::create(&root.join("manifest.json"), &manifest)?.commit()?;
    report["manifest_sha256"] = json!(format!("{:x}", Sha256::digest(&manifest)));
    Ok(report)
}

/// Explicit preparation can initialize an empty native owned state. It never
/// overwrites an installation, selects an active candidate or runs package code.
pub fn prepare(manager: &Path, package: &str, version: &str, state: &Path) -> io::Result<Value> {
    dependency_audit::endpoints(package, version)?;
    let state = dependency_discovery::local_path(state).map_err(|_| invalid())?;
    native_build::owner_root(&state)?;
    let _lock = native_build::lock_owned_state(&state)?;
    let _owner = ReadGuard::open(&state.join("owner"))?;
    let staging = state.join("dependency-staging");
    native_build::ordinary_ancestors(&staging)?;
    fs::create_dir_all(&staging)?;
    let stage = tempfile::Builder::new()
        .prefix("candidate-")
        .tempdir_in(&staging)?;
    let request = Request {
        schema: 1,
        state,
        package: package.to_owned(),
        version: version.to_owned(),
    };
    StagedFile::create(
        &stage.path().join("request.json"),
        &serde_json::to_vec(&request)?,
    )?
    .commit()?;
    let private = tempfile::Builder::new()
        .prefix("harness-dependency-stage-")
        .tempdir()?;
    let result = (|| {
        let manager = manager.canonicalize()?;
        let _manager = ReadGuard::open(&manager)?;
        let mut command = CommandSpec::new(&manager);
        command.current_dir = Some(private.path().to_owned());
        for key in ["TEMP", "TMP"] {
            command
                .env
                .insert(key.into(), Some(private.path().as_os_str().to_owned()));
        }
        command.args.extend([
            "dependency-stage-worker-v1".into(),
            stage.path().as_os_str().to_owned(),
        ]);
        let stdout = private.path().join("stdout");
        let stderr = private.path().join("stderr");
        let outcome = native_build::invoke_management(
            command,
            &stderr,
            Some(&stdout),
            Duration::from_secs(125),
        )
        .map_err(|_| invalid())?;
        if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
            let error = read(&stderr, 16 * 1024).unwrap_or_default();
            let code = if outcome.reason != StopReason::Exited || outcome.exit_code == 124 {
                "worker-deadline-or-resource-limit"
            } else {
                dependency_audit::failure_code(&error)
            };
            return Err(io::Error::other(format!(
                "dependency staging {code}; installation preserved"
            )));
        }
        let report: Value =
            serde_json::from_slice(&read(&stdout, 64 * 1024)?).map_err(|_| invalid())?;
        let manifest = read(&stage.path().join("manifest.json"), MAX_MANIFEST as u64)?;
        if report["operation"] != "npm-candidate-preparation"
            || report["schema_version"] != 1
            || report["status"] != "staged-unverified"
            || report["activation_allowed"] != false
            || report["package"] != package
            || report["version"] != version
            || report["manifest_sha256"] != format!("{:x}", Sha256::digest(&manifest))
        {
            return Err(invalid());
        }
        Ok(report)
    })();
    private
        .close()
        .map_err(|_| io::Error::other("dependency staging temporary cleanup failed"))?;
    match result {
        Ok(report) => {
            let _ = stage.keep();
            Ok(report)
        }
        Err(error) => {
            stage.close().map_err(|_| {
                io::Error::other(
                    "failed candidate cleanup requires recovery; installation preserved",
                )
            })?;
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use flate2::{Compression, write::GzEncoder};
    use sha2::Sha512;

    fn archive() -> (Vec<u8>, String) {
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut directory = tar::Header::new_gnu();
        directory.set_entry_type(tar::EntryType::Directory);
        directory.set_size(0);
        directory.set_mode(0o755);
        directory.set_cksum();
        tar.append_data(&mut directory, "package/empty/nested/", io::empty())
            .unwrap();
        for (name, bytes) in [
            (
                "package/package.json",
                br#"{"name":"@nuphus/nuphus-mcp","version":"0.2.2"}"#.as_slice(),
            ),
            ("package/bin/tool.js", b"inert archive content".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, name, bytes).unwrap();
        }
        let bytes = tar.into_inner().unwrap().finish().unwrap();
        let integrity = format!("sha512-{}", STANDARD.encode(Sha512::digest(&bytes)));
        (bytes, integrity)
    }

    #[test]
    fn owned_unpack_preserves_empty_directories_and_file_bytes() {
        let root = tempfile::tempdir().unwrap();
        let (bytes, integrity) = archive();
        let result = unpack(
            root.path(),
            "@nuphus/nuphus-mcp",
            "0.2.2",
            &bytes,
            &integrity,
        )
        .unwrap();
        assert_eq!(result["file_count"], 2);
        assert!(root.path().join("package/empty/nested").is_dir());
        assert_eq!(
            fs::read(root.path().join("package/bin/tool.js")).unwrap(),
            b"inert archive content"
        );
        assert!(
            unpack(
                root.path(),
                "@nuphus/nuphus-mcp",
                "0.2.2",
                &bytes,
                &integrity
            )
            .is_err()
        );
        assert_eq!(
            fs::read(root.path().join("package/bin/tool.js")).unwrap(),
            b"inert archive content"
        );
    }

    #[test]
    fn invalid_integrity_creates_no_candidate_and_identity_mismatch_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let (mut bytes, integrity) = archive();
        bytes[13] ^= 1;
        assert!(
            unpack(
                root.path(),
                "@nuphus/nuphus-mcp",
                "0.2.2",
                &bytes,
                &integrity
            )
            .is_err()
        );
        assert!(!root.path().join("package").exists());
        let (bytes, integrity) = archive();
        assert!(
            unpack(
                root.path(),
                "@nuphus/nuphus-mcp",
                "0.2.3",
                &bytes,
                &integrity
            )
            .is_err()
        );
    }

    #[test]
    fn streamed_large_payload_is_invisible_until_commit_and_failures_roll_back() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("large.bin");
        let length = 20 * 1024 * 1024;
        let mut input = io::repeat(b'R').take(length);
        let staged = StagedFile::from_reader(&target, &mut input, length).unwrap();
        assert!(fs::read(&target).is_err());
        staged.commit().unwrap();
        assert_eq!(fs::metadata(&target).unwrap().len(), length);
        let mut file = fs::File::open(&target).unwrap();
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            assert!(buffer[..count].iter().all(|byte| *byte == b'R'));
        }
        for (name, input, length) in [
            ("short", b"123".as_slice(), 4),
            ("long", b"12345".as_slice(), 4),
        ] {
            let path = root.path().join(name);
            assert!(StagedFile::from_reader(&path, &mut io::Cursor::new(input), length).is_err());
            assert!(!path.exists());
        }
        let foreign = root.path().join("foreign");
        fs::write(&foreign, b"preserve").unwrap();
        assert!(StagedFile::from_reader(&foreign, &mut io::empty(), 0).is_err());
        assert_eq!(fs::read(&foreign).unwrap(), b"preserve");
        assert!(
            StagedFile::from_reader(
                &root.path().join("oversized"),
                &mut io::empty(),
                512 * 1024 * 1024 + 1
            )
            .is_err()
        );
        assert!(!root.path().join("oversized").exists());
    }
}
