//! Run a host-owned native acceptance command against a real task checkout.
//! Candidate reports are never inputs. The unchanged supervisor supplies this
//! request and the frozen oracle identities, outside the candidate write scope.
use super::{exited_zero, now, run_program};
use crate::outcome_run::write_new;
use harness_core::{build_identity, inventory};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: u32,
    kind: String,
    case_root: PathBuf,
    task_contract_sha256: String,
    oracle: Oracle,
    timeout_seconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Oracle {
    program: PathBuf,
    program_sha256: String,
    /// Literal argv; {workspace} is replaced by the checked task root.
    arguments: Vec<String>,
    /// Additional oracle inputs, all outside the candidate checkout.
    inputs: BTreeMap<PathBuf, String>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn external(path: &Path, workspace: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid("oracle inputs must have explicit absolute paths"));
    }
    inventory::ordinary_parents(path)?;
    build_identity::ordinary(path)?;
    let path = path.canonicalize()?;
    if !path.is_file() || path.starts_with(workspace) {
        return Err(invalid(
            "oracle inputs must be regular files outside the candidate checkout",
        ));
    }
    Ok(path)
}

/// Deny writes and replacement while the independent check is running, rather
/// than accepting a candidate-controlled hash or success marker afterwards.
fn hold(path: &Path, expected: &str) -> io::Result<fs::File> {
    if !digest(expected) {
        return Err(invalid("invalid frozen oracle digest"));
    }
    use std::os::windows::fs::OpenOptionsExt;
    let file = fs::OpenOptions::new().read(true).share_mode(1).open(path)?;
    if build_identity::hash_file(path)? != expected.to_ascii_lowercase() {
        return Err(invalid(
            "frozen oracle input changed; no check was dispatched",
        ));
    }
    Ok(file)
}

pub(super) fn run(request_path: &Path, bytes: &[u8]) -> io::Result<i32> {
    let request: Request = serde_json::from_slice(bytes)?;
    if request.schema != 1
        || request.kind != "real-task"
        || !digest(&request.task_contract_sha256)
        || request.timeout_seconds == 0
        || request.timeout_seconds > 86_400
        || request.oracle.arguments.len() > 256
        || request
            .oracle
            .arguments
            .iter()
            .any(|argument| argument.len() > 32_768 || argument.contains('\0'))
        || request.oracle.inputs.len() > 256
    {
        return Err(invalid("invalid real-task acceptance request"));
    }
    inventory::ordinary_parents(&request.case_root)?;
    build_identity::ordinary(&request.case_root)?;
    let workspace = request.case_root.canonicalize()?;
    if !request.case_root.is_absolute() || !workspace.is_dir() {
        return Err(invalid(
            "case_root must be an explicit existing task directory",
        ));
    }
    let request_path = external(request_path, &workspace)?;
    let mut guards = vec![hold(&request_path, &build_identity::hash_bytes(bytes))?];
    let program = external(&request.oracle.program, &workspace)?;
    guards.push(hold(&program, &request.oracle.program_sha256)?);
    for (path, expected) in &request.oracle.inputs {
        guards.push(hold(&external(path, &workspace)?, expected)?);
    }
    let evidence = tempfile::Builder::new()
        .prefix("codex-real-task-oracle-")
        .tempdir()?
        .keep();
    if evidence.starts_with(&workspace) {
        return Err(invalid(
            "oracle evidence must be outside the candidate checkout",
        ));
    }
    let arguments: Vec<String> = request
        .oracle
        .arguments
        .iter()
        .map(|argument| argument.replace("{workspace}", &workspace.to_string_lossy()))
        .collect();
    let references: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let mut record = json!({
        "schema":1, "id":"outcome", "kind":"real-task", "executed":true,
        "checker_executed":false, "passed":false, "exit_code":1, "model_calls":0,
        "started_at":now(), "ended_at":null, "evidence":evidence.join("oracle.json"),
        "task_contract_sha256":request.task_contract_sha256,
        "oracle_program_sha256":request.oracle.program_sha256,
        "oracle_inputs":request.oracle.inputs,
    });
    write_new(&evidence.join("oracle-started.json"), &record)?;
    let mut runs = Vec::new();
    match run_program(
        &workspace,
        &evidence,
        "independent-check",
        &program,
        &references,
        request.timeout_seconds,
        &mut runs,
    ) {
        Ok(result) => {
            record["checker_executed"] = json!(true);
            record["passed"] = json!(exited_zero(&result));
        }
        Err(error) => {
            write_new(
                &evidence.join("failure.json"),
                &json!({
                    "kind":format!("{:?}",error.kind()), "message":error.to_string(),
                    "raw_os_error":error.raw_os_error(),
                }),
            )?;
            record["failure"] =
                json!("independent checker did not complete; inspect retained failure.json");
        }
    }
    let passed = record["passed"] == true;
    record["exit_code"] = json!(if passed { 0 } else { 1 });
    record["ended_at"] = json!(now());
    record["runs"] = json!(runs);
    write_new(&evidence.join("oracle.json"), &record)?;
    // Retain guards through publication of the parent-owned result.
    drop(guards);
    println!("{}", serde_json::to_string_pretty(&record)?);
    Ok(if passed { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_acceptance_input_refuses_writes_and_replacement() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("acceptance.txt");
        fs::write(&path, "frozen acceptance").unwrap();
        let guard = hold(&path, &build_identity::hash_file(&path).unwrap()).unwrap();
        assert!(fs::write(&path, "weakened acceptance").is_err());
        assert!(fs::remove_file(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "frozen acceptance");
        drop(guard);
        fs::write(&path, "owner can edit after the check").unwrap();
    }
}
