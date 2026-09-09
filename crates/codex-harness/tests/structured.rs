//! Exercise harness-inspect against deterministic owned Rust fixtures.
//! No models, live services or global mutations.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const PROMPT: &str = "Inspect input.txt. Literal $() `quotes` 'single' \"double\" ; & Unicode: проверка\nSecond line.";

fn inspect() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_harness-inspect"))
}

fn git() -> PathBuf {
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap()) {
        let mut candidate = directory.join("git.exe");
        if candidate.is_file() {
            return candidate;
        }
        candidate = directory.join("git");
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("git.exe required for fixture repositories");
}

fn init_case(root: &Path, mode: &str) -> PathBuf {
    let case = root.join(mode);
    fs::create_dir_all(&case).unwrap();
    let git = git();
    assert!(
        Command::new(&git)
            .args(["init", "-q"])
            .current_dir(&case)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new(&git)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ])
            .current_dir(&case)
            .status()
            .unwrap()
            .success()
    );
    fs::write(case.join("input.txt"), "independent expected fact").unwrap();
    case
}

fn run_case(root: &Path, mode: &str, expected: &str, timeout: u64, output_limit: u64) -> Value {
    let case = init_case(root, mode);
    let before = fs::read(case.join("input.txt")).unwrap();
    let prompt = root.join(format!("{mode}-prompt.txt"));
    fs::write(&prompt, PROMPT).unwrap();
    let inspect = inspect();
    let schema = root.join(format!("{mode}-schema.json"));
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.agents/skills/structured-codex-run/assets/inspection.schema.json"),
        &schema,
    )
    .unwrap();
    if mode == "live-schema" || mode == "unsupported-schema" {
        let mut value: Value = serde_json::from_slice(&fs::read(&schema).unwrap()).unwrap();
        if mode == "live-schema" {
            value["properties"]["findings"]["items"]["properties"]["line"]["type"] =
                "string".into();
        } else {
            value["properties"]["run_id"]["pattern"] = "^expected$".into();
        }
        fs::write(&schema, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    let command_json = serde_json::to_string(&[
        inspect.to_string_lossy().into_owned(),
        "--fixture".into(),
        mode.to_owned(),
    ])
    .unwrap();
    let oracle_json = serde_json::to_string(&[
        inspect.to_string_lossy().into_owned(),
        "--fixture".into(),
        if mode == "oracle-mutates" {
            "oracle-mutate".into()
        } else {
            "oracle".into()
        },
    ])
    .unwrap();
    let completed = Command::new(&inspect)
        .args([
            "--cwd",
            case.to_str().unwrap(),
            "--schema",
            schema.to_str().unwrap(),
            "--prompt-file",
            prompt.to_str().unwrap(),
            "--command-json",
            &command_json,
            "--oracle-json",
            &oracle_json,
            "--model",
            "fixture-model",
            "--provider",
            "fixture-provider",
            "--subscription",
            "deterministic-no-model",
            "--input",
            "input.txt",
            "--timeout",
            &timeout.to_string(),
            "--output-limit",
            &output_limit.to_string(),
        ])
        .output()
        .unwrap();
    fs::write(
        root.join(format!("{mode}-cli.txt")),
        [completed.stdout.as_slice(), completed.stderr.as_slice()].concat(),
    )
    .unwrap();
    let code = completed.status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 1,
        "{}{}",
        String::from_utf8_lossy(&completed.stdout),
        String::from_utf8_lossy(&completed.stderr)
    );
    let result: Value = serde_json::from_slice(&completed.stdout).unwrap();
    fs::write(
        root.join(format!("{mode}-result.json")),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    assert_eq!(result["status"], expected, "{result}");
    assert_eq!(code, if expected == "success" { 0 } else { 1 });
    let evidence = PathBuf::from(result["evidence_root"].as_str().unwrap());
    assert!(!evidence.join("inspection.schema.json").exists());
    if mode == "unsupported-schema" {
        assert!(!evidence.join("process.json").exists());
        assert!(!evidence.join("captured.json").exists());
        assert_eq!(result["error"], "unsupported inspection schema contract");
        return result;
    }
    let contract: Value =
        serde_json::from_slice(&fs::read(evidence.join("contract.json")).unwrap()).unwrap();
    assert_eq!(
        Path::new(contract["schema_path"].as_str().unwrap()),
        schema.canonicalize().unwrap()
    );
    assert!(evidence.join("process.json").is_file());
    assert!(evidence.join("events.jsonl").is_file());
    assert!(evidence.join("stderr.txt").is_file());
    assert_ne!(evidence, case);
    let capture: Value =
        serde_json::from_slice(&fs::read(evidence.join("captured.json")).unwrap()).unwrap();
    let stdin = capture["stdin"].as_str().unwrap();
    assert!(stdin.starts_with(&format!("{PROMPT}\n\n")));
    let argv: Vec<_> = capture["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    assert!(!argv.iter().any(|arg| arg.contains(PROMPT)));
    assert_eq!(argv.last().map(String::as_str), Some("-"));
    let sandbox = argv.iter().position(|arg| arg == "--sandbox").unwrap();
    assert_eq!(argv[sandbox + 1], "read-only");
    let last_message = argv
        .iter()
        .position(|arg| arg == "--output-last-message")
        .unwrap();
    assert_eq!(
        PathBuf::from(&argv[last_message + 1]),
        evidence.join("final.json")
    );
    let schema = argv
        .iter()
        .position(|arg| arg == "--output-schema")
        .unwrap();
    assert!(PathBuf::from(&argv[schema + 1]).is_file());
    if mode != "changed" && mode != "oracle-mutates" {
        assert_eq!(fs::read(case.join("input.txt")).unwrap(), before);
    }
    if matches!(
        mode,
        "timeout" | "terminated" | "stale" | "wrong" | "malformed" | "final-limit"
    ) {
        assert!(
            evidence.join("final.json").is_file(),
            "Partial output must remain"
        );
    }
    result
}

#[test]
fn oracle_mutation_cannot_certify_a_read_only_inspection() {
    let root = tempfile::Builder::new()
        .prefix("structured-oracle-mutation-")
        .tempdir()
        .unwrap()
        .keep();
    eprintln!("oracle mutation evidence: {}", root.display());
    run_case(&root, "oracle-mutates", "inputs-changed", 15, 1_048_576);
}

#[test]
fn success_preserves_independent_inputs_and_distinct_evidence() {
    let root = tempfile::Builder::new()
        .prefix("structured-checks-")
        .tempdir()
        .unwrap();
    run_case(root.path(), "success", "success", 15, 1_048_576);
}

#[test]
fn source_schema_is_live_bounded_and_must_remain_unchanged() {
    let root = tempfile::Builder::new()
        .prefix("structured-schema-")
        .tempdir()
        .unwrap();
    run_case(root.path(), "live-schema", "schema-invalid", 15, 1_048_576);
    let changed = run_case(
        root.path(),
        "schema-changed",
        "schema-changed",
        15,
        1_048_576,
    );
    assert_eq!(changed["schema_changed"], true);
    run_case(
        root.path(),
        "unsupported-schema",
        "infrastructure-failure",
        15,
        1_048_576,
    );
}

#[test]
fn negative_process_and_oracle_statuses_are_distinct() {
    let root = tempfile::Builder::new()
        .prefix("structured-checks-")
        .tempdir()
        .unwrap();
    for (mode, status) in [
        ("auth", "auth-failure"),
        ("process", "process-failure"),
        ("terminated", "terminated"),
        ("missing", "missing-json"),
        ("malformed", "malformed-json"),
        ("stale", "stale-json"),
        ("schema", "schema-invalid"),
        ("wrong", "wrong-answer"),
        ("incomplete", "task-incomplete"),
        ("task-failure", "task-failure"),
        ("events", "malformed-events"),
        ("unresolved", "unresolved-issues"),
        ("changed", "inputs-changed"),
    ] {
        run_case(root.path(), mode, status, 15, 1_048_576);
    }
}

#[test]
fn timeout_and_output_limits_retain_partial_evidence() {
    let root = tempfile::Builder::new()
        .prefix("structured-checks-")
        .tempdir()
        .unwrap();
    run_case(root.path(), "timeout", "timeout", 2, 1_048_576);
    run_case(root.path(), "output", "output-limit", 15, 1024);
    run_case(root.path(), "final-limit", "output-limit", 15, 1024);
}

#[test]
fn run_ids_are_fresh_across_invocations() {
    let root = tempfile::Builder::new()
        .prefix("structured-checks-")
        .tempdir()
        .unwrap();
    let first = run_case(root.path(), "success-a", "success", 15, 1_048_576);
    let second = run_case(root.path(), "success-b", "success", 15, 1_048_576);
    assert_ne!(first["run_id"], second["run_id"]);
    assert_eq!(first["run_id"].as_str().unwrap().len(), 32);
}

#[test]
fn timeout_receipt_is_distinct_from_oracle_and_keeps_partial_files() {
    let root = tempfile::Builder::new()
        .prefix("structured-checks-")
        .tempdir()
        .unwrap();
    let result = run_case(root.path(), "timeout", "timeout", 2, 1_048_576);
    let evidence = PathBuf::from(result["evidence_root"].as_str().unwrap());
    let process: Value =
        serde_json::from_slice(&fs::read(evidence.join("process.json")).unwrap()).unwrap();
    assert_eq!(process["status"], "timeout");
    assert_eq!(process["native"]["ExitCode"], 124);
    assert_eq!(process["outcome"]["exit_code"], 124);
    assert_eq!(process["outcome"]["reason"], "Timeout");
    assert!(evidence.join("events.jsonl").is_file());
    assert!(evidence.join("final.json").is_file());
    assert_ne!(result["status"], "oracle-failure");
    assert_ne!(result["status"], "wrong-answer");
}

const _KEEP_DURATION: Duration = Duration::from_secs(1);
