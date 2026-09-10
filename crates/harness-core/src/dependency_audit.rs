//! Explicit, read-only comparison with an exact official npm release.
//! Package bytes are data; no package manager or package entrypoint is run.
#![cfg(windows)]
use crate::{
    dependency_archive, dependency_discovery,
    dependency_fetch::Client,
    native_build,
    process::{CommandSpec, StopReason},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    path::Path,
    time::Duration,
};
use windows_sys::Win32::System::JobObjects::{
    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectCpuRateControlInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject,
};

const DOCUMENT_LIMIT: u64 = 4 * 1024 * 1024;
const ARCHIVE_LIMIT: u64 = 128 * 1024 * 1024;
const REPORT_LIMIT: u64 = 1024 * 1024;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "dependency audit input, source or observation was rejected",
    )
}

/// The selected catalogue's npm tools and Nuphus's two Windows payloads only.
pub(crate) fn endpoints(name: &str, version: &str) -> io::Result<(String, String)> {
    if !matches!(
        name,
        "codebase-memory-mcp"
            | "basedpyright"
            | "@nuphus/nuphus-mcp"
            | "@nuphus/nuphus-mcp-win32-x64"
            | "@nuphus/nuphus-mcp-win32-arm64"
    ) || version.len() > 64
    {
        return Err(invalid());
    }
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|p| {
            p.is_empty()
                || p.len() > 10
                || (p.len() > 1 && p.starts_with('0'))
                || !p.bytes().all(|b| b.is_ascii_digit())
                || p.parse::<u32>().is_err()
        })
    {
        return Err(invalid());
    }
    let basename = name.rsplit('/').next().ok_or_else(invalid)?;
    Ok((
        format!(
            "https://registry.npmjs.org/{}/{version}",
            name.replace('/', "%2f")
        ),
        format!("https://registry.npmjs.org/{name}/-/{basename}-{version}.tgz"),
    ))
}

/// Hold ordinary ancestors against rename and read one non-reparse file while
/// refusing concurrent writers/deletion. These guards never grant write rights.
struct Observation {
    guard: crate::registration_native::ReadGuard,
}

impl Observation {
    fn open(path: &Path) -> io::Result<Self> {
        Ok(Self {
            guard: crate::registration_native::ReadGuard::open(
                &path.components().collect::<std::path::PathBuf>(),
            )?,
        })
    }

    fn bytes(&mut self, limit: u64) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        (&mut self.guard.file)
            .take(limit + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(invalid());
        }
        Ok(bytes)
    }

    fn hash(&mut self, limit: u64) -> io::Result<(u64, String)> {
        let mut count = 0;
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let read = self.guard.file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            count += read as u64;
            if count > limit {
                return Err(invalid());
            }
            hash.update(&buffer[..read]);
        }
        Ok((count, format!("{:x}", hash.finalize())))
    }
}

pub(crate) fn identity(bytes: &[u8]) -> io::Result<(String, String)> {
    let document: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let name = document["name"].as_str().ok_or_else(invalid)?;
    let version = document["version"].as_str().ok_or_else(invalid)?;
    endpoints(name, version)?;
    Ok((name.to_owned(), version.to_owned()))
}

pub(crate) fn release(metadata: &[u8], name: &str, version: &str) -> io::Result<(String, String)> {
    if metadata.len() as u64 > 16 * 1024 * 1024 {
        return Err(invalid());
    }
    let document: Value = serde_json::from_slice(metadata).map_err(|_| invalid())?;
    let (_, archive_url) = endpoints(name, version)?;
    let integrity = document["dist"]["integrity"].as_str().ok_or_else(invalid)?;
    if document["name"] != name
        || document["version"] != version
        || document["dist"]["tarball"] != archive_url
        || integrity.len() > 4096
    {
        return Err(invalid());
    }
    Ok((archive_url, integrity.to_owned()))
}

fn compare(
    root: &Path,
    name: &str,
    version: &str,
    archive: &[u8],
    integrity: &str,
) -> io::Result<Value> {
    // Verify the same compressed bytes before a parser or callback sees them.
    dependency_archive::verify_sri(archive, integrity)?;
    let mut manifest_found = false;
    let (mut matched, mut modified, mut missing, mut unavailable) = (0, 0, 0, 0);
    let mut differences = Vec::new();
    let mut listing_hash = Sha256::new();
    let summary = dependency_archive::visit_npm_tar(
        archive,
        &mut |path: &str, size: u64, reader: &mut dyn Read| {
            let mut expected = Sha256::new();
            let mut manifest = Vec::new();
            let mut buffer = [0; 64 * 1024];
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                expected.update(&buffer[..read]);
                if path == "package.json" {
                    if manifest.len() + read > DOCUMENT_LIMIT as usize {
                        return Err(invalid());
                    }
                    manifest.extend_from_slice(&buffer[..read]);
                }
            }
            if path == "package.json" {
                if identity(&manifest)? != (name.to_owned(), version.to_owned()) {
                    return Err(invalid());
                }
                manifest_found = true;
            }
            let expected = format!("{:x}", expected.finalize());
            listing_hash.update((path.len() as u64).to_le_bytes());
            listing_hash.update(path.as_bytes());
            listing_hash.update(size.to_le_bytes());
            listing_hash.update(expected.as_bytes());
            let (state, observed) = match Observation::open(&root.join(path))
                .and_then(|mut file| file.hash(ARCHIVE_LIMIT))
            {
                Ok((length, hash)) if length == size && hash == expected => {
                    matched += 1;
                    ("matching", Some(hash))
                }
                Ok((_, hash)) => {
                    modified += 1;
                    ("modified", Some(hash))
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    missing += 1;
                    ("missing", None)
                }
                Err(_) => {
                    unavailable += 1;
                    ("unavailable", None)
                }
            };
            if state != "matching" && differences.len() < 128 {
                differences.push(json!({"path":path,"state":state,"expected_sha256":expected,"observed_sha256":observed}));
            }
            Ok(())
        },
    )?;
    if !manifest_found {
        return Err(invalid());
    }
    Ok(json!({
        "status": if missing + modified + unavailable == 0 { "upstream-files-match" } else { "differences-observed" },
        "files":summary.files,"bytes":summary.bytes,"matching":matched,"modified":modified,
        "missing":missing,"unavailable":unavailable,"differences_truncated":modified + missing + unavailable > differences.len(),
        "differences":differences,"archive_sha256":format!("{:x}",Sha256::digest(archive)),
        "ordered_file_manifest_sha256":format!("{:x}",listing_hash.finalize()),
        "extra_files":"not-enumerated","snapshot":"individual guarded file observations; not an installation-wide transaction",
        "activation_allowed":false
    }))
}

/// Refuse a manually invoked worker without the required OS bounds. The normal
/// parent assigns the job atomically at process creation and enforces its deadline.
pub(crate) fn worker_limits() -> io::Result<()> {
    let mut memory = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
    let okay = unsafe {
        QueryInformationJobObject(
            std::ptr::null_mut(),
            JobObjectExtendedLimitInformation,
            (&mut memory as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&memory) as u32,
            std::ptr::null_mut(),
        ) != 0
            && QueryInformationJobObject(
                std::ptr::null_mut(),
                JobObjectCpuRateControlInformation,
                (&mut cpu as *mut JOBOBJECT_CPU_RATE_CONTROL_INFORMATION).cast(),
                std::mem::size_of_val(&cpu) as u32,
                std::ptr::null_mut(),
            ) != 0
    };
    let flags = JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let cpu_flags = JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
    if !okay
        || memory.BasicLimitInformation.LimitFlags & flags != flags
        || memory.JobMemoryLimit == 0
        || memory.JobMemoryLimit > 512 * 1024 * 1024
        || cpu.ControlFlags != cpu_flags
        || unsafe { cpu.Anonymous.CpuRate } == 0
        || unsafe { cpu.Anonymous.CpuRate } > 5000
    {
        return Err(io::Error::other(
            "dependency audit worker requires its bounded management job",
        ));
    }
    Ok(())
}

/// Internal native manager entrypoint. This must only receive an owned parent
/// invocation; the job check and independent deadline also fail closed on misuse.
pub fn worker(package_root: &Path) -> io::Result<Value> {
    worker_limits()?;
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(120));
        std::process::exit(124);
    });
    let root = dependency_discovery::local_path(package_root).map_err(|_| invalid())?;
    let mut installed = Observation::open(&root.join("package.json")).map_err(|_| invalid())?;
    let manifest = installed.bytes(DOCUMENT_LIMIT).map_err(|_| invalid())?;
    let (name, version) = identity(&manifest)?;
    let (metadata_url, _) = endpoints(&name, &version)?;
    let client = Client::new().map_err(io::Error::other)?;
    let metadata = client
        .fetch_validated(&metadata_url, 16 * 1024 * 1024)
        .map_err(io::Error::other)?;
    let (archive_url, integrity) = release(&metadata, &name, &version)?;
    let archive = client
        .fetch_validated(&archive_url, ARCHIVE_LIMIT)
        .map_err(io::Error::other)?;
    let mut report =
        compare(&root, &name, &version, &archive, &integrity).map_err(|_| invalid())?;
    report["schema_version"] = json!(1);
    report["operation"] = json!("npm-official-file-audit");
    report["package"] = json!(name);
    report["version"] = json!(version);
    report["package_root"] = json!(root);
    report["installed_manifest_sha256"] = json!(format!("{:x}", Sha256::digest(&manifest)));
    report["metadata_sha256"] = json!(format!("{:x}", Sha256::digest(&metadata)));
    report["source"] = json!({"metadata":metadata_url,"archive":archive_url,"integrity":integrity,"client":"system-curl","client_version":client.version});
    report["read_only"] = json!(true);
    report["network_requested"] = json!(true);
    report["model_calls"] = json!(0);
    report["package_code_executed"] = json!(false);
    // Keep installed's manifest and ancestor leases alive through all comparison.
    drop(installed);
    Ok(report)
}

pub(crate) fn failure_code(stderr: &[u8]) -> &'static str {
    let Some(message) = std::str::from_utf8(stderr)
        .ok()
        .and_then(|text| text.trim_end().strip_prefix("codex-harness: "))
    else {
        return "worker-failed";
    };
    match message {
        "metadata-http-error" | "metadata-http-status-rejected" => "official-source-http-error",
        "metadata-tls-error" => "official-source-tls-error",
        "metadata-redirect-rejected" => "official-source-redirect-rejected",
        "metadata-output-too-large" => "official-source-size-limit",
        "metadata-deadline" | "metadata-deadline-or-resource-limit" => {
            "official-source-deadline-or-resource-limit"
        }
        "system-curl-unavailable" | "system-curl-version-unsupported" => {
            "system-curl-unavailable-or-unsupported"
        }
        "metadata-transfer-failed" => "official-source-transfer-failed",
        "dependency audit input, source or observation was rejected" => {
            "input-source-or-archive-rejected"
        }
        "dependency staging input or candidate was rejected" => "staging-input-or-archive-rejected",
        "native-asset-integrity-rejected" => "native-asset-integrity-rejected",
        "native-candidate-archive-rejected" => "native-candidate-archive-rejected",
        "native-candidate-executable-missing" => "native-candidate-executable-missing",
        "npm-candidate-archive-rejected" => "npm-candidate-archive-rejected",
        "unsupported-native-release"
        | "unsupported-native-platform"
        | "native-asset-unavailable"
        | "native-asset-digest-unavailable" => "native-asset-incompatible-or-unavailable",
        "dependency audit worker requires its bounded management job" => {
            "worker-limits-unavailable"
        }
        _ => "worker-failed",
    }
}

/// Explicit audit starts exactly this manager under the existing management
/// limits. stdout/stderr and downloads are temporary and removed on all outcomes.
pub fn audit(manager: &Path, package_root: &Path) -> io::Result<Value> {
    let root = dependency_discovery::local_path(package_root).map_err(|_| invalid())?;
    let mut manifest = Observation::open(&root.join("package.json")).map_err(|_| invalid())?;
    identity(&manifest.bytes(DOCUMENT_LIMIT).map_err(|_| invalid())?)?;
    let manager = manager.canonicalize()?;
    let _executable = Observation::open(&manager)?;
    let private = tempfile::Builder::new()
        .prefix("harness-dependency-audit-")
        .tempdir()?;
    let result = (|| {
        let mut command = CommandSpec::new(&manager);
        command.current_dir = Some(private.path().to_owned());
        // Every nested curl/version/download temp directory belongs to the
        // parent's cleanup scope, even if this worker hits its deadline.
        for key in ["TEMP", "TMP"] {
            command
                .env
                .insert(key.into(), Some(private.path().as_os_str().to_owned()));
        }
        command
            .args
            .extend(["dependency-audit-worker-v1".into(), root.into_os_string()]);
        let stdout = private.path().join("stdout");
        let outcome = native_build::invoke_management(
            command,
            &private.path().join("stderr"),
            Some(&stdout),
            Duration::from_secs(125),
        )
        .map_err(|_| io::Error::other("dependency audit worker failed; package preserved"))?;
        if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
            let stderr = Observation::open(&private.path().join("stderr"))?
                .bytes(16 * 1024)
                .unwrap_or_default();
            let code = if outcome.reason != StopReason::Exited || outcome.exit_code == 124 {
                "worker-deadline-or-resource-limit"
            } else {
                failure_code(&stderr)
            };
            return Err(io::Error::other(format!(
                "dependency audit {code}; package preserved"
            )));
        }
        let bytes = Observation::open(&stdout)?.bytes(REPORT_LIMIT)?;
        let mut report: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if report["operation"] != "npm-official-file-audit"
            || report["schema_version"] != 1
            || report["read_only"] != true
            || report["activation_allowed"] != false
        {
            return Err(invalid());
        }
        report["limits"] = json!({"job_memory_bytes":512*1024*1024u64,"job_cpu_percent":50,"worker_deadline_seconds":120,"output_limit_bytes":REPORT_LIMIT});
        Ok(report)
    })();
    private
        .close()
        .map_err(|_| io::Error::other("dependency audit temporary cleanup failed"))?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use flate2::{Compression, write::GzEncoder};
    use sha2::Sha512;
    use std::{fs, fs::OpenOptions, os::windows::fs::symlink_file};

    fn archive(files: &[(&str, &[u8])]) -> (Vec<u8>, String) {
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, format!("package/{name}"), *bytes)
                .unwrap();
        }
        let bytes = tar.into_inner().unwrap().finish().unwrap();
        let integrity = format!("sha512-{}", STANDARD.encode(Sha512::digest(&bytes)));
        (bytes, integrity)
    }

    const MANIFEST: &[u8] = br#"{"name":"basedpyright","version":"1.2.3"}"#;

    #[test]
    fn worker_failure_codes_never_forward_private_output() {
        assert_eq!(
            failure_code(b"codex-harness: metadata-http-error\r\n"),
            "official-source-http-error"
        );
        for output in [
            b"PRIVATE-CREDENTIAL".as_slice(),
            b"codex-harness: metadata-http-error\nPRIVATE-CREDENTIAL",
            b"codex-harness: unrecognized PRIVATE-CREDENTIAL",
        ] {
            assert_eq!(failure_code(output), "worker-failed");
        }
        for code in [
            "native-asset-integrity-rejected",
            "native-candidate-archive-rejected",
            "native-candidate-executable-missing",
            "npm-candidate-archive-rejected",
        ] {
            assert_eq!(
                failure_code(format!("codex-harness: {code}\r\n").as_bytes()),
                code
            );
            assert_eq!(
                failure_code(format!("codex-harness: {code}\nPRIVATE-CREDENTIAL").as_bytes()),
                "worker-failed"
            );
        }
    }

    #[test]
    fn exact_sources_reject_unselected_identity_and_url_substitution() {
        assert_eq!(
            endpoints("@nuphus/nuphus-mcp-win32-x64", "0.2.2")
                .unwrap()
                .1,
            "https://registry.npmjs.org/@nuphus/nuphus-mcp-win32-x64/-/nuphus-mcp-win32-x64-0.2.2.tgz"
        );
        for version in [
            "latest",
            "1.2.3-beta",
            "01.2.3",
            "1.2.3/secret",
            "1.2",
            "1.2.3?secret",
            "1.2.3 ",
            "4294967296.1.1",
        ] {
            assert!(endpoints("basedpyright", version).is_err());
        }
        assert!(endpoints("PRIVATE-CREDENTIAL", "1.2.3").is_err());
        let (_, archive_url) = endpoints("basedpyright", "1.2.3").unwrap();
        let mut metadata = json!({"name":"basedpyright","version":"1.2.3","dist":{"tarball":archive_url,"integrity":"sha512-placeholder"}});
        assert!(
            release(
                &serde_json::to_vec(&metadata).unwrap(),
                "basedpyright",
                "1.2.3"
            )
            .is_ok()
        );
        for url in [
            "https://PRIVATE@registry.npmjs.org/basedpyright/-/basedpyright-1.2.3.tgz",
            "https://registry.npmjs.org/basedpyright/-/basedpyright-1.2.4.tgz",
            "http://registry.npmjs.org/basedpyright/-/basedpyright-1.2.3.tgz",
        ] {
            metadata["dist"]["tarball"] = json!(url);
            let error = release(
                &serde_json::to_vec(&metadata).unwrap(),
                "basedpyright",
                "1.2.3",
            )
            .unwrap_err();
            assert!(!error.to_string().contains("PRIVATE"));
        }
    }

    #[test]
    fn reports_exact_modified_missing_and_unavailable_without_writing() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("package.json"), MANIFEST).unwrap();
        fs::write(root.path().join("changed.js"), b"local change").unwrap();
        fs::create_dir(root.path().join("directory.js")).unwrap();
        fs::write(root.path().join("extra.js"), b"extra retained").unwrap();
        let (bytes, integrity) = archive(&[
            ("package.json", MANIFEST),
            ("changed.js", b"official"),
            ("missing.js", b"missing"),
            ("directory.js", b"file"),
        ]);
        let report = compare(root.path(), "basedpyright", "1.2.3", &bytes, &integrity).unwrap();
        assert_eq!(report["status"], "differences-observed");
        for key in ["matching", "modified", "missing", "unavailable"] {
            assert_eq!(report[key], 1);
        }
        assert_eq!(report["extra_files"], "not-enumerated");
        assert_eq!(report["activation_allowed"], false);
        assert_eq!(
            fs::read(root.path().join("changed.js")).unwrap(),
            b"local change"
        );
        assert_eq!(
            fs::read(root.path().join("extra.js")).unwrap(),
            b"extra retained"
        );
        assert!(!root.path().join("missing.js").exists());
    }

    #[test]
    fn comparison_requires_integrity_and_in_archive_package_identity() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("package.json"), MANIFEST).unwrap();
        let (mut bytes, integrity) = archive(&[("package.json", MANIFEST)]);
        let report = compare(root.path(), "basedpyright", "1.2.3", &bytes, &integrity).unwrap();
        assert_eq!(report["status"], "upstream-files-match");
        assert!(compare(root.path(), "basedpyright", "1.2.4", &bytes, &integrity).is_err());
        bytes[12] ^= 1;
        assert!(compare(root.path(), "basedpyright", "1.2.3", &bytes, &integrity).is_err());
        let (bytes, integrity) = archive(&[("README", b"no manifest")]);
        assert!(compare(root.path(), "basedpyright", "1.2.3", &bytes, &integrity).is_err());
    }

    #[test]
    fn observation_refuses_links_writers_and_directory_replacement() {
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("package");
        fs::create_dir(&package).unwrap();
        fs::write(package.join("package.json"), MANIFEST).unwrap();
        let _lease = Observation::open(&package.join("package.json")).unwrap();
        assert!(
            OpenOptions::new()
                .write(true)
                .open(package.join("package.json"))
                .is_err()
        );
        assert!(fs::rename(&package, root.path().join("moved")).is_err());
        fs::write(root.path().join("outside"), b"private bytes").unwrap();
        symlink_file(root.path().join("outside"), package.join("linked")).unwrap();
        assert!(Observation::open(&package.join("linked")).is_err());
    }

    #[test]
    fn nested_archive_slashes_resolve_to_the_same_opened_windows_file() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("package.json"), MANIFEST).unwrap();
        fs::create_dir_all(root.path().join("bin/nested")).unwrap();
        fs::write(
            root.path().join("bin/nested/file.js"),
            b"official nested file",
        )
        .unwrap();
        let (bytes, integrity) = archive(&[
            ("package.json", MANIFEST),
            ("bin/nested/file.js", b"official nested file"),
        ]);
        let report = compare(root.path(), "basedpyright", "1.2.3", &bytes, &integrity).unwrap();
        assert_eq!(report["status"], "upstream-files-match");
        assert_eq!(report["matching"], 2);
    }
}
