//! Independent checks of controlled cases. The host supplies the frozen setup;
//! the candidate's outcome.json is an assertion to verify, never the oracle.
#[path = "outcome_oracle_evidence.rs"]
mod evidence;
#[path = "outcome_oracle_process.rs"]
mod process;

use crate::outcome_run::{isolated, repository, write_new};
use codex_harness::regression::{CaseRequest, run_case};
use harness_core::build_identity::{hash_file, ordinary};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    case_root: PathBuf,
    setup: Setup,
    execution: PathBuf,
    arm: Arm,
}
#[derive(Deserialize)]
struct Setup {
    case_id: String,
    case_root: PathBuf,
    source_state: String,
    immutable: BTreeMap<String, String>,
    documents: BTreeMap<String, String>,
    fixture_executables: BTreeMap<String, String>,
}
#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Arm {
    Baseline,
    Candidate,
}

fn invalid() -> io::Error {
    io::Error::other("Invalid or incomplete controlled outcome evidence.")
}
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
pub fn run(args: &[OsString]) -> io::Result<i32> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!(
            "codex-harness outcome-oracle --request PATH\nChecks a controlled temporary case using host-frozen setup and native execution evidence. JSON and streams are private. No model calls."
        );
        return Ok(0);
    }
    if args.len() != 2 || args[0] != "--request" {
        return Err(invalid());
    }
    let request_path = PathBuf::from(&args[1]);
    let request: Request =
        serde_json::from_slice(&bounded(&request_path, 2 * 1024 * 1024)?).map_err(|_| invalid())?;
    let workspace = isolated(&request.case_root)?;
    // The host's immutable setup is kept outside the candidate's mutable case.
    if request_path.canonicalize()?.starts_with(&workspace) {
        return Err(invalid());
    }
    let private = tempfile::Builder::new()
        .prefix("codex-outcome-oracle-")
        .tempdir()?;
    let root = private.path().canonicalize()?;
    if root.starts_with(repository()) || root.starts_with(&workspace) {
        return Err(invalid());
    }
    let _ = private.keep();
    let mut record = json!({"id":"outcome","executed":true,"exit_code":1,"passed":false,
        "started_at":now(),"ended_at":null,"evidence":root.join("oracle.json"),"model_calls":0});
    write_new(&root.join("oracle-started.json"), &record)?;
    let mut checks = BTreeMap::new();
    let mut runs = Vec::new();
    let mut details = json!({});
    if let Err(error) = verify(
        &request,
        &workspace,
        &root,
        &mut checks,
        &mut runs,
        &mut details,
    ) {
        checks.insert("oracle_completed".to_owned(), false);
        write_new(
            &root.join("failure.json"),
            &json!({"kind":format!("{:?}",error.kind()),
            "raw_os_error":error.raw_os_error(),"message":error.to_string()}),
        )?;
        details["error"] = json!("Oracle incomplete; inspect private failure evidence.");
    }
    let passed = !checks.is_empty() && checks.values().all(|value| *value);
    details["checks"] = json!(checks);
    details["runs"] = json!(runs);
    record["details"] = details;
    record["passed"] = json!(passed);
    record["exit_code"] = json!(if passed { 0 } else { 1 });
    record["ended_at"] = json!(now());
    write_new(&root.join("oracle.json"), &record)?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    Ok(if passed { 0 } else { 1 })
}

fn validate_setup(setup: &Setup, workspace: &Path) -> io::Result<()> {
    let names: &[&str] = match setup.case_id.as_str() {
        "freshness" | "entrypoint" => &["README.md", "source.json", "case.exe"],
        "process" => &["README.md", "unrelated.txt", "case.exe", "observe.exe"],
        "missing" => &["verification.json"],
        "negative" => &["guide.md"],
        _ => return Err(invalid()),
    };
    if setup.source_state != "controlled-v3-rust"
        || isolated(&setup.case_root)? != workspace
        || setup.immutable.len() != names.len()
        || names
            .iter()
            .any(|name| !setup.immutable.contains_key(*name))
        || setup.documents.len() > 16
    {
        return Err(invalid());
    }
    for (name, hash) in setup.immutable.iter().chain(&setup.documents) {
        relative(name)?;
        if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(invalid());
        }
    }
    let executables: BTreeMap<_, _> = setup
        .immutable
        .iter()
        .filter(|(name, _)| name.ends_with(".exe"))
        .map(|(name, hash)| (name.clone(), hash.clone()))
        .collect();
    if setup.fixture_executables != executables {
        return Err(invalid());
    }
    Ok(())
}

fn immutable_matches(workspace: &Path, setup: &Setup) -> io::Result<bool> {
    for (name, expected) in &setup.immutable {
        let path = scoped_file(workspace, name)?;
        if fs::metadata(&path)?.len() > 128 * 1024 * 1024 || hash_file(&path)? != *expected {
            return Ok(false);
        }
    }
    Ok(true)
}

fn verify(
    request: &Request,
    workspace: &Path,
    root: &Path,
    checks: &mut BTreeMap<String, bool>,
    runs: &mut Vec<Value>,
    details: &mut Value,
) -> io::Result<()> {
    validate_setup(&request.setup, workspace)?;
    let native_root = request.execution.parent().ok_or_else(invalid)?;
    let native_root = isolated(native_root)?;
    if native_root.starts_with(workspace)
        || workspace.starts_with(&native_root)
        || request.execution.file_name() != Some(std::ffi::OsStr::new("native.json"))
    {
        return Err(invalid());
    }
    let native = read_json(&request.execution, 2 * 1024 * 1024)?;
    let native_request = read_json(&native_root.join("request.json"), 2 * 1024 * 1024)?;
    let field_path = |value: &Value, key: &str| -> io::Result<PathBuf> {
        Ok(PathBuf::from(
            value.get(key).and_then(Value::as_str).ok_or_else(invalid)?,
        ))
    };
    if isolated(&field_path(&native, "evidence_root")?)? != native_root
        || isolated(&field_path(&native_request, "workingDirectory")?)? != workspace
        || field_path(&native_request, "stdoutPath")?.canonicalize()?
            != native_root.join("events.jsonl").canonicalize()?
    {
        return Err(invalid());
    }
    checks.insert(
        "native_completed".into(),
        native["status"] == "completed"
            && native["turn_completed"] == true
            && native.pointer("/process/Status") == Some(&json!("exited"))
            && native.pointer("/process/ExitCode").and_then(Value::as_u64) == Some(0),
    );
    let immutable = immutable_matches(workspace, &request.setup)?;
    checks.insert("immutable_inputs".into(), immutable);
    // Never execute an altered supplied target or observer.
    if !immutable {
        return Err(invalid());
    }
    let observed = evidence::collect(&native_root, workspace, &request.setup.documents)?;
    checks.insert("complete_evidence".into(), observed.errors.is_empty());
    details["observation_errors"] = json!(observed.errors);
    details["documentation"] = json!({"bytes":observed.documentation.len(),
        "updated_markdown_bytes":observed.updated_documentation.len()});
    let uses: BTreeMap<_, _> = ["project-verification", "reproduce-regression"]
        .into_iter()
        .map(|name| (name, evidence::referenced_skill(&observed.items, name)))
        .collect();
    details["skill_use"] = json!(uses);
    details["skill_signal_scope"] = json!(
        "successful tool-input path reference; not proof of reading or applying instructions"
    );
    let case = request.setup.case_id.as_str();
    if request.arm == Arm::Candidate && case != "negative" {
        checks.insert(
            "positive_activation".into(),
            uses[if case == "process" {
                "reproduce-regression"
            } else {
                "project-verification"
            }],
        );
    } else {
        checks.insert(
            "negative_activation".into(),
            !uses.values().any(|value| *value),
        );
    }
    let report = read_json(&scoped_file(workspace, "outcome.json")?, 1024 * 1024)?;
    checks.insert(
        "result_record".into(),
        report.is_object()
            && report.get("command").is_some_and(nonempty)
            && report
                .get("scope")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty()),
    );
    checks.insert(
        "accurate_status".into(),
        report["status"]
            == if case == "missing" {
                "blocked"
            } else {
                "passed"
            },
    );
    match case {
        "freshness" | "entrypoint" => {
            // Snapshot before our independent invocation; our execution cannot
            // retroactively supply evidence that the candidate ran the product.
            let audit_path = workspace.join("execution-audit.jsonl");
            let rows: Vec<Value> = if audit_path.exists() {
                String::from_utf8(bounded(&audit_path, 1024 * 1024)?)
                    .map_err(|_| invalid())?
                    .lines()
                    .map(serde_json::from_str)
                    .collect::<Result<_, _>>()
                    .map_err(|_| invalid())?
            } else {
                Vec::new()
            };
            let first_build = rows
                .iter()
                .position(|row| row == &json!({"entrypoint":"build","version":2}));
            checks.insert("build_executed".into(), first_build.is_some());
            checks.insert(
                "cli_executed".into(),
                first_build.is_some_and(|index| {
                    rows.iter()
                        .skip(index + 1)
                        .any(|row| row == &json!({"entrypoint":"cli","version":2}))
                }),
            );
            details["native_direct_cli_command"] = json!(evidence::successful_command(
                &observed.items,
                &pattern(r"case\.exe[^\r\n]*--outcome-case\s+cli"),
                Some(&pattern(r"(?m)^2\r?$"))
            ));
            let cli = run_program(
                workspace,
                root,
                "product-cli",
                &scoped_file(workspace, "case.exe")?,
                &["--outcome-case", "cli", "--independent-oracle"],
                30,
                runs,
            )?;
            let stdout = cli
                .pointer("/streams/stdout/path")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?;
            checks.insert(
                "actual_entrypoint_v2".into(),
                exited_zero(&cli)
                    && String::from_utf8(bounded(Path::new(stdout), 1024)?)
                        .map_err(|_| invalid())?
                        .trim()
                        == "2",
            );
            checks.insert(
                "generated_matches_source".into(),
                read_json(&scoped_file(workspace, "built.json")?, 1024)?
                    == read_json(&scoped_file(workspace, "source.json")?, 1024)?,
            );
            if case == "freshness" {
                let record = String::from_utf8(bounded(
                    &scoped_file(workspace, "docs/validation.md")?,
                    1024 * 1024,
                )?)
                .map_err(|_| invalid())?;
                checks.insert(
                    "historical_record_retained".into(),
                    record.contains("source version 1") && record.contains("stdout 1"),
                );
                checks.insert(
                    "current_record_distinct".into(),
                    pattern(r"(?:version|верси\w*)\s*[:=]?\s*2|stdout\s*[:=]?\s*2")
                        .is_match(&record),
                );
                checks.insert(
                    "historical_limit".into(),
                    pattern(r"historic|earlier|previous|outdated|устар|истори|предыдущ")
                        .is_match(&record),
                );
            }
        }
        "process" => process::verify(workspace, root, checks, runs)?,
        "missing" => {
            let verification =
                read_json(&scoped_file(workspace, "verification.json")?, 1024 * 1024)?;
            let executable = verification
                .pointer("/command/0")
                .and_then(Value::as_str)
                .ok_or_else(invalid)?;
            let expected = workspace.join("unavailable-checker.exe");
            if Path::new(executable) != expected {
                return Err(invalid());
            }
            checks.insert(
                "prerequisite_still_absent".into(),
                fs::symlink_metadata(expected).is_err_and(|e| e.kind() == io::ErrorKind::NotFound),
            );
            checks.insert(
                "blocker_named".into(),
                report
                    .to_string()
                    .to_lowercase()
                    .contains("unavailable-checker.exe"),
            );
            checks.insert("no_substitute_or_install".into(), !observed.items.iter().any(|item|
                pattern(r"\b(?:pip|npm|uv|winget|choco)\s+(?:install|add)|Invoke-WebRequest|curl\s+https?").is_match(&evidence::tool_input(item))));
        }
        "negative" => {
            checks.insert(
                "exact_edit".into(),
                bounded(&scoped_file(workspace, "README.md")?, 1024 * 1024)?
                    == b"# Guide\n\nRun verification; see [details](guide.md).\n",
            );
            checks.insert(
                "local_link".into(),
                scoped_file(workspace, "guide.md").is_ok(),
            );
            checks.insert("no_unrelated_execution".into(), !observed.items.iter().any(|item|
                pattern(r"\b(?:pytest|jest|vitest|eslint)\b|npm(?:\.cmd)?\s+(?:run\s+)?(?:test|build|lint)|agent-delegation|outcome_suite|spawn_agent").is_match(&evidence::tool_input(item))));
            checks.insert(
                "no_delegation".into(),
                native
                    .get("children")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty),
            );
        }
        _ => return Err(invalid()),
    }
    checks.insert(
        "immutable_after_check".into(),
        immutable_matches(workspace, &request.setup)?,
    );
    Ok(())
}

fn pattern(text: &str) -> Regex {
    Regex::new(&format!("(?i){text}")).expect("static oracle pattern")
}
fn nonempty(value: &Value) -> bool {
    match value {
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(a) => {
            !a.is_empty()
                && a.iter()
                    .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
        }
        _ => false,
    }
}
fn exited_zero(value: &Value) -> bool {
    value["status"] == "exited"
        && value.pointer("/native/ExitCode").and_then(Value::as_u64) == Some(0)
}
fn relative(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.contains(':')
        || name.contains('\0')
        || Path::new(name)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(invalid());
    }
    Ok(())
}
pub(super) fn scoped_file(root: &Path, name: &str) -> io::Result<PathBuf> {
    relative(name)?;
    let path = root.join(name);
    harness_core::inventory::ordinary_parents(&path)?;
    ordinary(&path)?;
    if !path.is_file() || !path.canonicalize()?.starts_with(root.canonicalize()?) {
        return Err(invalid());
    }
    Ok(path)
}
pub(super) fn bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    harness_core::inventory::ordinary_parents(path)?;
    ordinary(path)?;
    if !path.is_file() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid());
    }
    Ok(bytes)
}
pub(super) fn read_json(path: &Path, limit: u64) -> io::Result<Value> {
    serde_json::from_slice(&bounded(path, limit)?).map_err(|_| invalid())
}
pub(super) fn run_program(
    workspace: &Path,
    evidence: &Path,
    label: &str,
    exe: &Path,
    args: &[&str],
    timeout: u64,
    runs: &mut Vec<Value>,
) -> io::Result<Value> {
    let value = run_case(CaseRequest {
        argv: std::iter::once(exe.to_str().ok_or_else(invalid)?.to_owned())
            .chain(args.iter().map(|s| (*s).to_owned()))
            .collect(),
        cwd: workspace.to_owned(),
        timeout,
        output_limit: 8 * 1024 * 1024,
        root: Some(evidence.join(label)),
        ..CaseRequest::default()
    })?;
    runs.push(value.clone());
    Ok(value)
}
