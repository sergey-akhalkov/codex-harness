//! Real process-case checks with an independently owned sentinel.
use super::{bounded, invalid, read_json, run_program, scoped_file};
use harness_core::process::{CommandSpec, Job, Limits};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) fn verify(
    workspace: &Path,
    evidence: &Path,
    checks: &mut BTreeMap<String, bool>,
    runs: &mut Vec<Value>,
) -> io::Result<()> {
    let sentinel = evidence.join("sentinel.exe");
    let source_hash = crate::outcome_prepare::copy_executable(&env::current_exe()?, &sentinel)?;
    let job = Job::new(Limits {
        memory_bytes: Some(128 * 1024 * 1024),
        cpu_percent: Some(50.0),
    })?;
    let mut spec = CommandSpec::new(&sentinel);
    spec.args = vec!["--outcome-case".into(), "sentinel".into()];
    spec.current_dir = Some(evidence.to_owned());
    let suspended = job.spawn_suspended(&spec)?;
    if !job.contains(suspended.process())? {
        return Err(invalid());
    }
    let child = suspended.resume()?;
    let before = child.exit_code()?;
    let checked = check_targets(workspace, evidence, checks, runs);
    // Past the target descendant's six-second delayed write deadline, including
    // a checker failure. This is an owned synchronous acceptance wait.
    std::thread::sleep(Duration::from_secs(7));
    checks.insert(
        "descendant_cleaned".into(),
        fs::symlink_metadata(workspace.join("descendant-survived.txt"))
            .is_err_and(|error| error.kind() == io::ErrorKind::NotFound),
    );
    let after = child.exit_code();
    checks.insert(
        "unrelated_process_survives".into(),
        before.is_none() && after.as_ref().is_ok_and(Option::is_none),
    );
    let cleanup = job.terminate(130, Duration::from_secs(5));
    checks.insert(
        "sentinel_cleaned".into(),
        cleanup.as_ref().is_ok_and(|s| s.active_processes == 0),
    );
    runs.push(
        json!({"role":"independent-sentinel","executable_sha256":source_hash,
        "alive_before":before.is_none(),"alive_after":after.as_ref().is_ok_and(Option::is_none),
        "cleanup":cleanup.as_ref().ok(),"cleanup_error":cleanup.as_ref().err().map(|e|
            json!({"kind":format!("{:?}",e.kind()),"raw_os_error":e.raw_os_error()}))}),
    );
    checked?;
    after?;
    cleanup?;
    Ok(())
}

fn check_targets(
    workspace: &Path,
    evidence: &Path,
    checks: &mut BTreeMap<String, bool>,
    runs: &mut Vec<Value>,
) -> io::Result<()> {
    let checker = scoped_file(workspace, "check_process.exe")?;
    if fs::metadata(&checker)?.len() > 128 * 1024 * 1024 {
        return Err(invalid());
    }
    let audit = workspace.join("process-audit.jsonl");
    let previous_audit = match fs::symlink_metadata(&audit) {
        Ok(_) => bounded(&audit, 8 * 1024 * 1024)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    let result_path = workspace.join("process-results.json");
    match fs::symlink_metadata(&result_path) {
        Ok(_) => {
            let previous = scoped_file(workspace, "process-results.json")?;
            // Both exact paths are within the validated case or freshly created
            // oracle root. Keep the prior result rather than accepting stale data.
            fs::rename(previous, evidence.join("previous-process-results.json"))?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    let executed = run_program(
        workspace,
        evidence,
        "candidate-checker",
        &checker,
        &[],
        45,
        runs,
    )?;
    checks.insert(
        "executable_check".into(),
        executed["status"] == "exited"
            && executed.pointer("/native/ExitCode").and_then(Value::as_u64) == Some(0),
    );
    let observations = read_json(
        &scoped_file(workspace, "process-results.json")?,
        1024 * 1024,
    )?;
    let current_audit = bounded(&audit, 8 * 1024 * 1024)?;
    let retained = current_audit.starts_with(&previous_audit);
    checks.insert("audit_history_retained".into(), retained);
    if !retained {
        return Err(invalid());
    }
    let fresh =
        std::str::from_utf8(&current_audit[previous_audit.len()..]).map_err(|_| invalid())?;
    let events: Vec<Value> = fresh
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .map_err(|_| invalid())?;
    checks.insert(
        "all_real_targets_executed".into(),
        ["flood", "fail", "no-ready", "hang"]
            .into_iter()
            .all(|mode| {
                events
                    .iter()
                    .any(|row| row["mode"] == mode && row["event"] == "start")
            }),
    );
    checks.insert(
        "hang_did_not_exit_naturally".into(),
        !events.iter().any(|row| {
            row["event"] == "natural-end" && (row["mode"] == "hang" || row["mode"] == "no-ready")
        }),
    );
    for (mode, status, code) in [
        ("flood", "exited", Some(0)),
        ("fail", "exited", Some(7)),
        ("no-ready", "readiness-timeout", None),
        ("hang", "timeout", None),
    ] {
        let row = &observations[mode];
        let expected_code = code.map_or(Value::Null, |c| json!(c));
        checks.insert(
            format!("{mode}_status"),
            row["status"] == status && row.get("exit_code") == Some(&expected_code),
        );
    }
    for (stream, byte) in [("stdout", b'a'), ("stderr", b'b')] {
        let name = observations["flood"]
            .get(format!("{stream}_path"))
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        let path = stream_path(workspace, name)?;
        let bytes = bounded(&path, 2097152)?;
        checks.insert(
            format!("complete_{stream}"),
            bytes.len() == 2097152 && bytes.iter().all(|b| *b == byte),
        );
    }
    Ok(())
}

fn stream_path(workspace: &Path, name: &str) -> io::Result<PathBuf> {
    let supplied = Path::new(name);
    let path = if supplied.is_absolute() {
        harness_core::inventory::ordinary_parents(supplied)?;
        harness_core::build_identity::ordinary(supplied)?;
        supplied.to_owned()
    } else {
        scoped_file(workspace, name)?
    };
    let resolved = path.canonicalize()?;
    let temporary = env::temp_dir().canonicalize()?;
    if !resolved.starts_with(&temporary) || resolved == temporary {
        return Err(invalid());
    }
    Ok(path)
}
