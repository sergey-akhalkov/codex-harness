//! Version 1 transfer from a bounded compiler owner to the freshly built manager.
//! The caller owns the lock and publication; this process only fills staging.
use super::{TARGET, ordinary_ancestors, verify_compiled_inputs};
use crate::build_identity::{self, BINARIES, BuildRecord, SCHEMA, SourceIdentity};
use crate::process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const REQUEST: &str = "finalize-request.json";
const RESPONSE: &str = "finalize-result.json";
const MAX_RECEIPT: u64 = 4 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub schema: u32,
    pub source: PathBuf,
    pub target: PathBuf,
    pub staging: PathBuf,
    pub before: SourceIdentity,
    pub rustc: String,
    pub cargo: String,
    pub build_target: String,
    pub profile: String,
    pub manager_sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    schema: u32,
    request_sha256: String,
    record_sha256: String,
    manager_sha256: String,
    source_identity: String,
    healthy: bool,
}

pub(super) fn bounded_bytes(path: &Path) -> io::Result<Vec<u8>> {
    ordinary_ancestors(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_RECEIPT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECEIPT {
        return Err(io::Error::other(
            "Native handoff receipt exceeds its limit.",
        ));
    }
    Ok(bytes)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    Ok(serde_json::from_slice(&bounded_bytes(path)?)?)
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    ordinary_ancestors(path)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn known_before(current: &SourceIdentity, before: &SourceIdentity) -> io::Result<()> {
    if before.sha256 != build_identity::hash_bytes(&serde_json::to_vec(&before.files)?)
        || current
            .files
            .iter()
            .any(|(path, hash)| before.files.get(path) != Some(hash))
    {
        return Err(io::Error::other(
            "Fresh consumer inputs lack matching pre-compilation evidence; explicit current Cargo bootstrap is required.",
        ));
    }
    Ok(())
}

fn validate_paths(request_path: &Path, request: &Request) -> io::Result<()> {
    for path in [&request.source, &request.target, &request.staging] {
        if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
            return Err(io::Error::other(
                "Handoff paths must be absolute and normalized.",
            ));
        }
        ordinary_ancestors(path)?;
        if !path.is_dir() {
            return Err(io::Error::other("Handoff directory is unavailable."));
        }
    }
    let source = request.source.canonicalize()?;
    let target = request.target.canonicalize()?;
    let staging = request.staging.canonicalize()?;
    if request_path.canonicalize()? != staging.join(REQUEST)
        || source.starts_with(&target)
        || target.starts_with(&source)
        || source.starts_with(&staging)
        || staging.starts_with(&source)
        || target.starts_with(&staging)
        || staging.starts_with(&target)
    {
        return Err(io::Error::other(
            "Native handoff roots overlap or request is misplaced.",
        ));
    }
    let staging_root = staging
        .parent()
        .ok_or_else(|| io::Error::other("Missing staging root."))?;
    let state = staging_root
        .parent()
        .ok_or_else(|| io::Error::other("Missing state root."))?;
    if staging_root
        .file_name()
        .is_none_or(|name| name != "staging")
    {
        return Err(io::Error::other(
            "Handoff target is not an owned staging directory.",
        ));
    }
    super::verify_owned_state(state)?;
    let manager = target.join(TARGET).join("release/codex-harness.exe");
    if std::env::current_exe()?.canonicalize()? != manager.canonicalize()?
        || build_identity::hash_file(&manager)? != request.manager_sha256
    {
        return Err(io::Error::other(
            "Finalizer is not the just-compiled manager.",
        ));
    }
    Ok(())
}

/// Internal entry point. No Cargo, lock acquisition, activation or publication.
pub(super) fn finalize(request_path: &Path) -> io::Result<()> {
    let request_bytes = bounded_bytes(request_path)?;
    let request: Request = serde_json::from_slice(&request_bytes)?;
    if request.schema != 1 || request.build_target != TARGET || request.profile != "release" {
        return Err(io::Error::other(
            "Unsupported native finalization protocol.",
        ));
    }
    validate_paths(request_path, &request)?;
    let current = build_identity::source_identity(&request.source)?;
    known_before(&current, &request.before)?;
    verify_compiled_inputs(&request.target, &request.source, &current)?;
    let mut binaries = BTreeMap::new();
    for name in BINARIES {
        let built = request.target.join(TARGET).join("release").join(name);
        ordinary_ancestors(&built)?;
        let destination = request.staging.join(name);
        ordinary_ancestors(&destination)?;
        let mut input = fs::File::open(&built)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        binaries.insert((*name).to_owned(), build_identity::hash_file(&destination)?);
    }
    if binaries.get("codex-harness.exe") != Some(&request.manager_sha256) {
        return Err(io::Error::other(
            "Compiled manager changed during finalization.",
        ));
    }
    let record = BuildRecord {
        schema: SCHEMA,
        source_root: request.source.clone(),
        source: current.clone(),
        rustc: request.rustc,
        cargo: request.cargo,
        target: request.build_target,
        profile: request.profile,
        binaries,
    };
    let record_bytes = serde_json::to_vec_pretty(&record)?;
    write_new(&request.staging.join("build.json"), &record_bytes)?;
    if build_identity::source_identity(&request.source)? != current
        || bounded_bytes(request_path)? != request_bytes
        || !build_identity::check(&request.staging, Some(&request.source)).runtime_allowed
    {
        return Err(io::Error::other(
            "Fresh consumer verification failed; staging retained.",
        ));
    }
    let response = Response {
        schema: 1,
        request_sha256: build_identity::hash_bytes(&request_bytes),
        record_sha256: build_identity::hash_bytes(&record_bytes),
        manager_sha256: request.manager_sha256,
        source_identity: current.sha256,
        healthy: true,
    };
    write_new(
        &request.staging.join(RESPONSE),
        &serde_json::to_vec_pretty(&response)?,
    )
}

pub(super) fn run(request: Request) -> io::Result<BuildRecord> {
    let staging = request.staging.clone();
    run_with_timeout(request, Duration::from_secs(60)).map_err(|error| {
        io::Error::other(format!(
            "Candidate finalization was not accepted: {error}; retained {}",
            staging.display()
        ))
    })
}

fn run_with_timeout(request: Request, timeout: Duration) -> io::Result<BuildRecord> {
    let request_bytes = serde_json::to_vec_pretty(&request)?;
    let path = request.staging.join(REQUEST);
    write_new(&path, &request_bytes)?;
    let log_path = request.staging.join("finalize.log");
    let mut command = CommandSpec::new(
        request
            .target
            .join(TARGET)
            .join("release/codex-harness.exe"),
    );
    command.args = vec!["finalize-build-v1".into(), path.as_os_str().to_owned()];
    command.current_dir = Some(request.source.clone());
    let outcome = invoke(command, &log_path, None, timeout)?;
    if outcome.reason != StopReason::Exited || outcome.exit_code != 0 {
        return Err(io::Error::other(format!(
            "Fresh manager finalization failed (exit {}, {:?}); active installation preserved. See {}",
            outcome.exit_code,
            outcome.reason,
            log_path.display()
        )));
    }
    let response: Response = read_json(&request.staging.join(RESPONSE))?;
    let record_bytes = bounded_bytes(&request.staging.join("build.json"))?;
    let record = build_identity::verify_record_integrity(&request.staging)?;
    known_before(&record.source, &request.before)?;
    if response.schema != 1
        || !response.healthy
        || response.request_sha256 != build_identity::hash_bytes(&request_bytes)
        || bounded_bytes(&path)? != request_bytes
        || response.record_sha256 != build_identity::hash_bytes(&record_bytes)
        || response.manager_sha256 != request.manager_sha256
        || record.binaries.get("codex-harness.exe") != Some(&request.manager_sha256)
        || response.source_identity != record.source.sha256
        || record.source_root != request.source
        || record.rustc != request.rustc
        || record.cargo != request.cargo
        || record.target != request.build_target
        || record.profile != request.profile
    {
        return Err(io::Error::other(
            "Fresh manager handoff response is inconsistent; staging retained.",
        ));
    }
    Ok(record)
}

/// Explicit management subprocesses retain their bounded output in owned files.
pub(crate) fn invoke(
    mut command: CommandSpec,
    log_path: &Path,
    stdout_path: Option<&Path>,
    timeout: Duration,
) -> io::Result<crate::process::Outcome> {
    let deadline = Deadline::after(timeout)?;
    ordinary_ancestors(log_path)?;
    let log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(log_path)?;
    command.stdout = Some(if let Some(path) = stdout_path {
        ordinary_ancestors(path)?;
        OpenOptions::new().write(true).create_new(true).open(path)?
    } else {
        log.try_clone()?
    });
    command.stderr = Some(log);
    let job = Job::new(Limits {
        memory_bytes: Some(512 * 1024 * 1024),
        cpu_percent: Some(50.0),
    })?;
    let child = job.spawn(&command)?;
    let cancellation = Cancellation::default();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker_cancel = cancellation.clone();
    let paths: Vec<_> = std::iter::once(log_path.to_owned())
        .chain(stdout_path.map(Path::to_owned))
        .collect();
    let watcher = std::thread::spawn(move || {
        while !worker_stop.load(Ordering::Acquire) {
            for path in &paths {
                if fs::metadata(path).map_or(true, |m| m.len() > MAX_RECEIPT) {
                    worker_cancel.cancel();
                    return false;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        paths
            .iter()
            .all(|p| fs::metadata(p).is_ok_and(|m| m.len() <= MAX_RECEIPT))
    });
    let result = job.wait(&child, deadline, &cancellation, Duration::from_secs(1));
    stop.store(true, Ordering::Release);
    let output_ok = watcher
        .join()
        .map_err(|_| io::Error::other("Native output watcher failed."))?;
    drop(command);
    if !output_ok {
        return Err(io::Error::other(format!(
            "Native management output exceeded its bound or became unavailable; see {}",
            log_path.display()
        )));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn actual_finalizer_failures_cannot_supply_an_accepted_record() {
        let temp = tempfile::Builder::new()
            .prefix("harness-handoff-rejections-")
            .tempdir()
            .unwrap();
        let root = temp.keep();
        let fixture = root.join("fixture.rs");
        fs::write(&fixture, include_str!("../tests/fixtures/handoff.rs")).unwrap();
        let target = root.join("target");
        let release = target.join(TARGET).join("release");
        fs::create_dir_all(&release).unwrap();
        let manager = release.join("codex-harness.exe");
        let mut command =
            CommandSpec::new(super::super::resolve_tool(std::ffi::OsStr::new("rustc")).unwrap());
        command.args = vec![
            fixture.as_os_str().to_owned(),
            "--edition=2024".into(),
            "-o".into(),
            manager.as_os_str().to_owned(),
        ];
        let built = invoke(
            command,
            &root.join("compile.log"),
            None,
            Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!((built.reason, built.exit_code), (StopReason::Exited, 0));
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        let files = BTreeMap::from([("Cargo.toml".into(), build_identity::hash_bytes(b"fixture"))]);
        let before = SourceIdentity {
            sha256: build_identity::hash_bytes(&serde_json::to_vec(&files).unwrap()),
            files,
        };
        let mut errors = BTreeMap::new();
        for mode in [
            "missing",
            "malformed",
            "reject",
            "timeout",
            "flood",
            "bad-binding",
            "bad-record",
            "false-verdict",
            "changed-request",
        ] {
            let staging = root.join(mode);
            fs::create_dir(&staging).unwrap();
            let request = Request {
                schema: 1,
                source: source.clone(),
                target: target.clone(),
                staging: staging.clone(),
                before: before.clone(),
                rustc: "fixture".into(),
                cargo: "fixture".into(),
                build_target: TARGET.into(),
                profile: "release".into(),
                manager_sha256: build_identity::hash_file(&manager).unwrap(),
            };
            let response_case = [
                "bad-binding",
                "bad-record",
                "false-verdict",
                "changed-request",
            ]
            .contains(&mode);
            if response_case {
                fs::copy(&manager, staging.join("codex-harness.exe")).unwrap();
                let record = BuildRecord {
                    schema: 1,
                    source_root: source.clone(),
                    source: before.clone(),
                    rustc: request.rustc.clone(),
                    cargo: request.cargo.clone(),
                    target: TARGET.into(),
                    profile: "release".into(),
                    binaries: BTreeMap::from([(
                        "codex-harness.exe".into(),
                        request.manager_sha256.clone(),
                    )]),
                };
                let record_bytes = serde_json::to_vec_pretty(&record).unwrap();
                let response = Response {
                    schema: 1,
                    request_sha256: if mode == "bad-binding" {
                        "wrong-request".into()
                    } else {
                        build_identity::hash_bytes(&serde_json::to_vec_pretty(&request).unwrap())
                    },
                    record_sha256: build_identity::hash_bytes(&record_bytes),
                    manager_sha256: request.manager_sha256.clone(),
                    source_identity: before.sha256.clone(),
                    healthy: mode != "false-verdict",
                };
                fs::write(staging.join("record-template.json"), record_bytes).unwrap();
                fs::write(
                    staging.join("response-template.json"),
                    serde_json::to_vec_pretty(&response).unwrap(),
                )
                .unwrap();
            }
            let error = run_with_timeout(
                request,
                if mode == "timeout" {
                    Duration::from_millis(250)
                } else {
                    Duration::from_secs(10)
                },
            )
            .unwrap_err();
            assert_eq!(staging.join("build.json").exists(), response_case);
            assert!(staging.join(REQUEST).exists());
            let message = error.to_string();
            match mode {
                "reject" => assert!(message.contains("exit 7"), "{message}"),
                "timeout" => assert!(message.contains("Timeout"), "{message}"),
                "flood" => assert!(message.contains("output exceeded"), "{message}"),
                _ if response_case => {
                    assert!(message.contains("inconsistent"), "{mode}: {message}")
                }
                _ => (),
            }
            errors.insert(mode, message);
        }
        fs::write(
            root.join("rejections.json"),
            serde_json::to_vec_pretty(&errors).unwrap(),
        )
        .unwrap();
        println!("handoff rejection evidence {}", root.display());
    }

    #[test]
    fn consumer_may_remove_inputs_but_cannot_invent_precompile_evidence() {
        let mut before = SourceIdentity {
            sha256: String::new(),
            files: BTreeMap::from([
                ("crates/core/src/lib.rs".into(), "old-source".into()),
                ("live-schema.json".into(), "old-data".into()),
            ]),
        };
        before.sha256 = build_identity::hash_bytes(&serde_json::to_vec(&before.files).unwrap());
        let mut current = before.clone();
        current.files.remove("live-schema.json");
        assert!(known_before(&current, &before).is_ok());
        current
            .files
            .insert("new-input.rs".into(), "unobserved".into());
        assert!(known_before(&current, &before).is_err());
        current.files.remove("new-input.rs");
        current
            .files
            .insert("crates/core/src/lib.rs".into(), "changed".into());
        assert!(known_before(&current, &before).is_err());
        before.sha256.clear();
        assert!(known_before(&current, &before).is_err());
    }
}
