//! Native structured inspection entry point. Fixture modes are compiled into
//! this executable so tests never spawn Python or PowerShell helpers.

#[path = "../structured.rs"]
mod structured;

use serde_json::{Value, json};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).and_then(|arg| arg.to_str()) == Some("--fixture") {
        match run_fixture(&args[2..]) {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("harness-inspect fixture: {error}");
                std::process::exit(2);
            }
        }
    }
    match structured::run_cli(&args[1..]) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("harness-inspect: {error}");
            std::process::exit(2);
        }
    }
}

fn run_fixture(args: &[std::ffi::OsString]) -> io::Result<i32> {
    let mode = args
        .first()
        .and_then(|arg| arg.to_str())
        .ok_or_else(|| io::Error::other("fixture mode required"))?;
    if mode == "oracle" || mode == "oracle-mutate" {
        let final_path = args
            .last()
            .ok_or_else(|| io::Error::other("oracle requires the final JSON path"))?;
        let value: Value = serde_json::from_str(&fs::read_to_string(final_path)?)?;
        let expected = fs::read_to_string("input.txt")?;
        let findings = value
            .get("findings")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let good = findings.len() == 1
            && findings[0].get("path") == Some(&json!("input.txt"))
            && findings[0].get("line") == Some(&json!(1))
            && findings[0].get("evidence").and_then(Value::as_str) == Some(expected.as_str());
        if mode == "oracle-mutate" {
            fs::write("input.txt", "oracle changed the source")?;
        }
        return Ok(if good { 0 } else { 1 });
    }

    let argv: Vec<String> = args
        .iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let final_path = arg_value(&argv, "--output-last-message")?;
    let mut prompt = String::new();
    io::stdin().read_to_string(&mut prompt)?;
    let parent = Path::new(&final_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    fs::write(
        parent.join("captured.json"),
        serde_json::to_vec(&json!({"argv": argv, "stdin": prompt}))?,
    )?;
    let run_id = extract_run_id(&prompt)?;
    let mut value = json!({
        "run_id": run_id,
        "findings": [{
            "path": "input.txt",
            "line": 1,
            "description": "Inspected input",
            "evidence": fs::read_to_string("input.txt")?
        }],
        "unresolved_issues": []
    });
    match mode {
        "auth" => {
            eprintln!("authentication failed: 401 Unauthorized");
            return Ok(1);
        }
        "process" => {
            eprintln!("fixture process failure");
            return Ok(7);
        }
        "missing" => {
            println!("{}", json!({"type": "turn.completed"}));
            return Ok(0);
        }
        "wrong" => value["findings"][0]["evidence"] = json!("wrong answer"),
        "stale" => value["run_id"] = json!("previous-run"),
        "schema" => value["findings"][0]["line"] = json!(true),
        "unresolved" => value["unresolved_issues"] = json!(["inspection incomplete"]),
        _ => {}
    }
    let final_text = if mode == "malformed" {
        "{".into()
    } else {
        serde_json::to_string(&value)?
    };
    fs::write(&final_path, final_text)?;
    match mode {
        "timeout" => {
            println!("{}", json!({"type": "turn.completed"}));
            std::io::stdout().flush()?;
            std::thread::sleep(Duration::from_secs(30));
        }
        "terminated" => {
            println!("{}", json!({"type": "turn.completed"}));
            std::io::stdout().flush()?;
            terminate_self(0xC000013A)?;
        }
        "output" => {
            println!("{}", "x".repeat(8192));
            std::io::stdout().flush()?;
        }
        "final-limit" => {
            fs::write(&final_path, "x".repeat(8192))?;
            std::thread::sleep(Duration::from_secs(30));
        }
        "changed" => fs::write("input.txt", "changed")?,
        _ => {}
    }
    match mode {
        "events" => println!("{{"),
        "incomplete" => println!("{}", json!({"type": "turn.started"})),
        "task-failure" => println!(
            "{}",
            json!({"type": "turn.failed", "error": {"message": "task failed"}})
        ),
        _ => println!("{}", json!({"type": "turn.completed"})),
    }
    Ok(0)
}

fn terminate_self(code: u32) -> io::Result<()> {
    #[cfg(windows)]
    unsafe {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut core::ffi::c_void;
            fn TerminateProcess(process: *mut core::ffi::c_void, exit_code: u32) -> i32;
        }
        if TerminateProcess(GetCurrentProcess(), code) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(not(windows))]
    {
        let _ = code;
        std::process::exit(1);
    }
    Err(io::Error::other("Forced termination unexpectedly returned"))
}

fn arg_value(argv: &[String], name: &str) -> io::Result<PathBuf> {
    let index = argv
        .iter()
        .position(|arg| arg == name)
        .ok_or_else(|| io::Error::other(format!("{name} required")))?;
    argv.get(index + 1)
        .cloned()
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other(format!("{name} requires a value")))
}

fn extract_run_id(prompt: &str) -> io::Result<String> {
    let marker = "run_id exactly ";
    let start = prompt
        .find(marker)
        .ok_or_else(|| io::Error::other("run identity missing from stdin"))?
        + marker.len();
    let id: String = prompt[start..].chars().take(32).collect();
    if id.len() == 32 && id.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(id.to_ascii_lowercase())
    } else {
        Err(io::Error::other("run identity missing from stdin"))
    }
}
