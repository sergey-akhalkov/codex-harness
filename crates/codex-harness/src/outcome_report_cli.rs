//! Explicit local report formatting; never starts a process, model or hook.
use harness_core::outcome_report;
use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read},
    path::PathBuf,
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "outcome-report requires --input PATH [--markdown]; input must be a bounded valid attempt array or report",
    )
}

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    if args == [OsString::from("--help")] {
        println!(
            "codex-harness outcome-report --input PATH [--markdown]\nReads private local JSON (up to 8 MiB, 1024 attempts); retains attempts and comparison exclusions. No model calls or benefit claims."
        );
        return Ok(0);
    }
    let mut input = None;
    let mut markdown = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--input" && input.is_none() {
            input = Some(PathBuf::from(iter.next().ok_or_else(invalid)?));
        } else if arg == "--markdown" && !markdown {
            markdown = true;
        } else {
            return Err(invalid());
        }
    }
    let mut bytes = Vec::new();
    File::open(input.ok_or_else(invalid)?)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(invalid());
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let attempts = value
        .as_array()
        .or_else(|| value.get("attempts").and_then(|v| v.as_array()))
        .ok_or_else(invalid)?;
    let report = outcome_report::summarize_attempts(attempts)?;
    let result = if markdown {
        outcome_report::concise_report(&report)?
    } else {
        format!("{}\n", serde_json::to_string_pretty(&report)?)
    };
    if result.len() > 32 * 1024 * 1024 {
        return Err(invalid());
    }
    print!("{result}");
    Ok(0)
}
