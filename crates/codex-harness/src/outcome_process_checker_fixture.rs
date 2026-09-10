//! Model-free checker double. It invokes the supplied observer and real targets.
use codex_harness::regression::{CaseRequest, run_case};
use serde_json::{Value, json};
use std::{env, fs, io, path::PathBuf};

pub fn run() -> io::Result<()> {
    let root = env::current_exe()?
        .parent()
        .ok_or_else(|| io::Error::other("checker root"))?
        .canonicalize()?;
    if !root.starts_with(env::temp_dir().canonicalize()?) {
        return Err(io::Error::other("owned temporary checker required"));
    }
    let fault = env::var("HARNESS_OUTCOME_CHECKER_FAULT").unwrap_or_default();
    if fault == "stale" {
        return Ok(());
    }
    let mut results = json!({});
    for mode in ["flood", "fail", "no-ready", "hang"] {
        let mut argv = vec![
            root.join("observe.exe").to_string_lossy().into_owned(),
            "--cwd".into(),
            root.to_string_lossy().into_owned(),
            "--timeout".into(),
            if mode == "hang" || mode == "no-ready" {
                "2"
            } else {
                "15"
            }
            .into(),
        ];
        if mode == "no-ready" {
            argv.extend(["--ready-timeout".into(), "1".into()]);
        }
        argv.extend([
            "--".into(),
            root.join("case.exe").to_string_lossy().into_owned(),
            "--outcome-case".into(),
            mode.into(),
        ]);
        let observer = run_case(CaseRequest {
            argv,
            cwd: root.clone(),
            timeout: 25,
            output_limit: 1024 * 1024,
            ..CaseRequest::default()
        })?;
        if observer["status"] != "exited" {
            return Err(io::Error::other("observer did not exit"));
        }
        let output = observer
            .pointer("/streams/stdout/path")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("observer output absent"))?;
        let observed: Value = serde_json::from_slice(&fs::read(output)?)?;
        let code = if observed["status"] == "exited" {
            observed["native"]["ProcessExitCode"].clone()
        } else {
            Value::Null
        };
        results[mode] = json!({"status":observed["status"],"exit_code":code,
            "stdout_path":observed["streams"]["stdout"]["path"],"stderr_path":observed["streams"]["stderr"]["path"]});
    }
    if fault == "wrong-forced-code" {
        results["hang"]["exit_code"] = json!(124);
    }
    if fault == "truncated-stream" {
        let original = PathBuf::from(results["flood"]["stdout_path"].as_str().unwrap());
        let bytes = fs::read(original)?;
        let truncated = root.join("truncated-stdout.txt");
        fs::write(&truncated, &bytes[..bytes.len() - 1])?;
        results["flood"]["stdout_path"] = json!(truncated);
    }
    fs::write(
        root.join("process-results.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    Ok(())
}
