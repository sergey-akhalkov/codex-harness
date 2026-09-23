//! `codex-harness heavy`: one bounded batch command under the account-wide slot.
//!
//! The account queue, its local machine budget and the bounded Windows Job are
//! owned by `harness_core::heavy_command`; this file is the CLI surface, the
//! diagnostics and the exit-code contract. The command runs directly, never
//! through a shell. Interactive sessions and model conversations stay outside
//! this slot.
#![cfg(windows)]

use harness_core::{
    heavy_command,
    process::{Cancellation, StopReason},
};
use serde_json::json;
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

/// The command's own exit code is passed through unchanged.
const EXIT_DEADLINE: i32 = 124;
const EXIT_MEMORY_LIMIT: i32 = 125;
const EXIT_CLEANUP: i32 = 126;
const EXIT_STARTUP: i32 = 127;
const EXIT_INTERRUPTED: i32 = 130;

const USAGE: &str = "\
codex-harness heavy [--account DIRECTORY] -- PROGRAM [ARGS...]
  Run one batch command under the account-wide heavy-command slot: one local
  machine budget, one serialized queue and one bounded Windows Job per admitted
  command. The command tree is terminated and the slot released before the next
  caller is admitted. Interactive sessions and model conversations belong
  outside this slot. Exit codes: the command's own code; 124 deadline or queue
  wait expired; 125 memory budget exceeded; 126 the command tree could not be
  cleaned; 127 the command could not be resolved or started; 130 interrupted.

codex-harness heavy budget [--account DIRECTORY] [--json]
  Show the effective local machine budget and where it comes from.

codex-harness heavy budget [--account DIRECTORY] [--memory-bytes N]
  [--cpu-percent P] [--deadline-seconds N] [--queue-wait-seconds N] [--preview]
  Update the local machine budget. Values not supplied keep their effective
  value; nothing is written with --preview. An invalid policy fails before any
  command starts. Machine values stay outside tracked configuration.";

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub fn run(args: &[OsString]) -> io::Result<i32> {
    let Some(first) = args.first() else {
        eprintln!("{USAGE}");
        return Ok(2);
    };
    if first == "--help" || first == "-h" {
        println!("{USAGE}");
        return Ok(0);
    }
    if first == "budget" {
        return budget(&args[1..]);
    }
    let mut option = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].to_str() {
            Some("--account") => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid("--account requires a directory"))?;
                if option.replace(PathBuf::from(value)).is_some() {
                    return Err(invalid("duplicate --account"));
                }
                index += 2;
            }
            Some("--") => break,
            Some(other) if other.starts_with("--") => {
                return Err(invalid(
                    "unknown heavy option; use codex-harness heavy --help",
                ));
            }
            _ => {
                return Err(invalid(
                    "expected '--' before the heavy command; use codex-harness heavy --help",
                ));
            }
        }
    }
    if args.get(index).is_none_or(|arg| arg != "--") {
        return Err(invalid(
            "expected '--' before the heavy command; use codex-harness heavy --help",
        ));
    }
    let (program, rest) = args[index + 1..]
        .split_first()
        .ok_or_else(|| invalid("heavy requires a PROGRAM after '--'"))?;
    let label = heavy_command::label(program, rest);
    let account = heavy_command::account_dir(option.as_deref())?;
    // Invalid local policy and an unresolvable program both fail before this
    // caller waits for the account slot.
    let budget = heavy_command::Budget::read(&account)?;
    let program = match heavy_command::resolve_program(program) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("heavy: {error}");
            return Ok(EXIT_STARTUP);
        }
    };
    eprintln!(
        "heavy: account={} memory_limit_bytes={} cpu_percent={} deadline_seconds={} queue_wait_seconds={}",
        account.display(),
        budget.memory_bytes,
        budget.cpu_percent,
        budget.deadline_seconds,
        budget.queue_wait_seconds
    );
    eprintln!("heavy: command={label}");
    let cancellation = Cancellation::default();
    let _interrupt = heavy_command::Interrupt::install(&cancellation)?;
    let admission = match heavy_command::Admission::acquire(
        &account,
        &budget,
        &label,
        &cancellation,
    ) {
        Ok(admission) => admission,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
            eprintln!(
                "heavy: interrupted while waiting for the account heavy-command slot; no command was started"
            );
            return Ok(EXIT_INTERRUPTED);
        }
        Err(error) if error.kind() == io::ErrorKind::TimedOut => {
            eprintln!(
                "heavy: queue wait limit of {}s expired; no command was started",
                budget.queue_wait_seconds
            );
            return Ok(EXIT_DEADLINE);
        }
        Err(error) => return Err(error),
    };
    let run = match heavy_command::execute(
        &budget,
        &account,
        &program,
        rest,
        &admission,
        &cancellation,
    ) {
        Ok(run) => run,
        Err(heavy_command::RunError::Start(error)) => {
            eprintln!("heavy: {error}");
            return Ok(EXIT_STARTUP);
        }
        Err(heavy_command::RunError::Cleanup(error)) => {
            eprintln!("heavy: {error}; no descendant of the command survives");
            return Ok(EXIT_CLEANUP);
        }
    };
    let elapsed = run.elapsed.as_millis();
    let outcome = run.outcome;
    let job = outcome.job;
    match outcome.reason {
        StopReason::Exited => {
            eprintln!(
                "heavy: exited code={} elapsed_ms={elapsed} memory_limit_bytes={} peak_memory_bytes={} cpu_rate={} kill_on_close={}",
                outcome.exit_code,
                job.memory_limit_bytes,
                job.peak_job_memory_bytes,
                job.cpu_rate,
                job.kill_on_close
            );
            Ok(outcome.exit_code as i32)
        }
        StopReason::Timeout => {
            eprintln!(
                "heavy: deadline of {}s expired after {elapsed} ms; bounded process tree terminated",
                budget.deadline_seconds
            );
            Ok(EXIT_DEADLINE)
        }
        StopReason::MemoryLimit => {
            eprintln!(
                "heavy: command tree exceeded the {} byte memory budget after {elapsed} ms; bounded process tree terminated",
                budget.memory_bytes
            );
            Ok(EXIT_MEMORY_LIMIT)
        }
        StopReason::Cancelled => {
            eprintln!("heavy: interrupted after {elapsed} ms; bounded process tree terminated");
            Ok(EXIT_INTERRUPTED)
        }
    }
}

/// Inspect or update the local machine budget. No command runs here.
fn budget(args: &[OsString]) -> io::Result<i32> {
    let mut account = None;
    let mut json = false;
    let mut preview = false;
    let mut updates: Vec<(String, OsString)> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let Some(option) = args[index].to_str() else {
            return Err(invalid("heavy budget options must be UTF-8"));
        };
        match option {
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(0);
            }
            "--json" => {
                json = true;
                index += 1;
            }
            "--preview" => {
                preview = true;
                index += 1;
            }
            "--account" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid("--account requires a directory"))?;
                if account.replace(PathBuf::from(value)).is_some() {
                    return Err(invalid("duplicate --account"));
                }
                index += 2;
            }
            "--memory-bytes" | "--cpu-percent" | "--deadline-seconds" | "--queue-wait-seconds" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| invalid(&format!("{option} requires a value")))?;
                updates.push((option.to_owned(), value.clone()));
                index += 2;
            }
            _ => {
                return Err(invalid(
                    "unknown heavy budget option; use codex-harness heavy --help",
                ));
            }
        }
    }
    let account = heavy_command::account_dir(account.as_deref())?;
    let effective = heavy_command::Budget::read(&account)?;
    if updates.is_empty() {
        let source = budget_source(&account);
        return report(&account, &effective, &source, json);
    }
    let mut updated = effective;
    for (name, value) in &updates {
        let text = value
            .to_str()
            .ok_or_else(|| invalid(&format!("{name} must be UTF-8")))?;
        match name.as_str() {
            "--memory-bytes" => {
                updated.memory_bytes = text
                    .parse()
                    .map_err(|_| invalid("--memory-bytes requires an integer"))?;
            }
            "--cpu-percent" => {
                updated.cpu_percent = text
                    .parse()
                    .map_err(|_| invalid("--cpu-percent requires a number"))?;
            }
            "--deadline-seconds" => {
                updated.deadline_seconds = text
                    .parse()
                    .map_err(|_| invalid("--deadline-seconds requires an integer"))?;
            }
            _ => {
                updated.queue_wait_seconds = text
                    .parse()
                    .map_err(|_| invalid("--queue-wait-seconds requires an integer"))?;
            }
        }
    }
    updated.validate("heavy-command budget")?;
    let path = heavy_command::policy_path(&account);
    if preview {
        eprintln!("heavy: preview; {} not written", path.display());
    } else {
        heavy_command::Budget::write(&account, &updated)?;
        eprintln!("heavy: local budget written to {}", path.display());
    }
    report(&account, &updated, &path.display().to_string(), json)
}

fn budget_source(account: &Path) -> String {
    let path = heavy_command::policy_path(account);
    if path.is_file() {
        path.display().to_string()
    } else {
        "defaults".into()
    }
}

fn report(
    account: &Path,
    budget: &heavy_command::Budget,
    source: &str,
    json: bool,
) -> io::Result<i32> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "schema": 1,
                "memory_bytes": budget.memory_bytes,
                "cpu_percent": budget.cpu_percent,
                "deadline_seconds": budget.deadline_seconds,
                "queue_wait_seconds": budget.queue_wait_seconds,
                "source": source,
            }))?
        );
    } else {
        println!(
            "memory_bytes={} cpu_percent={} deadline_seconds={} queue_wait_seconds={} source={source} account={}",
            budget.memory_bytes,
            budget.cpu_percent,
            budget.deadline_seconds,
            budget.queue_wait_seconds,
            account.display()
        );
    }
    Ok(0)
}
