//! Inert double for the pinned RTK `pipe --filter` contract and for the
//! allowlisted source commands, so adapter acceptance needs no RTK download and
//! no network. Tests copy it as `rtk.exe` (compression double) or as the
//! command under test. It reads stdin, writes stdout, and keeps stderr empty.

use std::{
    fs,
    io::{self, Read, Write},
    time::Duration,
};

/// Width of the synthetic command output: wide enough that a bounded recall
/// window reaches the adapter's byte clamp before its line limit.
const LOG_LINE_BYTES: usize = 160;
const LOG_DEFAULT_COUNT: usize = 60;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let role = args.first().map(String::as_str).unwrap_or_default();
    if let Err(error) = run(role, &args[1..]) {
        eprintln!("rtk fixture: {error}");
        std::process::exit(2);
    }
}

fn run(role: &str, args: &[String]) -> io::Result<()> {
    match role {
        // `rtk.exe pipe --filter <name>`: the compressor contract the adapter
        // depends on. `fixture-fail` reports a failing filter on purpose.
        "pipe" => {
            let name = match args {
                [flag, name] if flag == "--filter" => name.clone(),
                _ => return Err(io::Error::other("usage: pipe --filter NAME")),
            };
            if name == "fixture-fail" {
                return Err(io::Error::other("requested filter failure"));
            }
            if std::env::var_os("HARNESS_RTK_FIXTURE_BANNER").is_some() {
                eprintln!(
                    "[rtk] /!\\ No hook installed — run `rtk init -g` for automatic token savings"
                );
            }
            if let Some(message) = std::env::var_os("HARNESS_RTK_FIXTURE_STDERR") {
                eprintln!("{}", message.to_string_lossy());
            }
            if let Some(millis) = std::env::var("HARNESS_RTK_FIXTURE_PIPE_DELAY_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
            {
                std::thread::sleep(Duration::from_millis(millis));
            }
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input)?;
            let lines = input.iter().filter(|byte| **byte == b'\n').count();
            if std::env::var_os("HARNESS_RTK_FIXTURE_INFLATE").is_some() {
                // Deliberately larger than the input, so the adapter's
                // "smaller compressed result" gate rejects the filter output.
                let mut inflated = input.clone();
                inflated.extend_from_slice(b"\nfixture-pipe inflation tail that makes the filter output larger than the raw command output\n");
                return io::stdout().write_all(&inflated);
            }
            writeln!(
                io::stdout().lock(),
                "fixture-pipe {name}: {lines} lines, {} bytes",
                input.len()
            )
        }
        // `git.exe log [-n N] [--all] [HEAD]`: deterministic multi-line output.
        "log" => {
            let mut count = LOG_DEFAULT_COUNT;
            let mut rest = args.iter();
            while let Some(argument) = rest.next() {
                match argument.as_str() {
                    "-n" | "--max-count" => {
                        count = rest
                            .next()
                            .and_then(|value| value.parse().ok())
                            .ok_or_else(|| io::Error::other("-n requires a count"))?;
                    }
                    "HEAD" | "--all" => {}
                    other => match other.strip_prefix("--max-count=") {
                        Some(value) => count = value.parse().map_err(io::Error::other)?,
                        None => {
                            return Err(io::Error::other(format!(
                                "unsupported log argument {other}"
                            )));
                        }
                    },
                }
            }
            let mut out = io::stdout().lock();
            for index in 1..=count {
                let prefix = format!(
                    "fixture-log line {index} of {count} for the rtk adapter acceptance double"
                );
                let padding = LOG_LINE_BYTES.saturating_sub(prefix.len());
                writeln!(out, "{prefix}{}", ".".repeat(padding))?;
            }
            Ok(())
        }
        // `git.exe status [--short]`: stays below the adapter's retention floor.
        "status" => {
            let mut out = io::stdout().lock();
            writeln!(out, " M fixture-modified.txt")?;
            writeln!(out, "?? fixture-untracked.txt")?;
            Ok(())
        }
        // `cargo.exe test|check|build|clippy ARGS...`: inert verification
        // double whose two streams, exit status and timing come from the test
        // environment, while every Cargo argument is accepted and ignored.
        "test" | "check" | "build" | "clippy" => cargo_double(),
        // A missing role still counts as an invocation of the native command,
        // so a count over the ledger stays a count over child runs.
        other => {
            record_ledger()?;
            Err(io::Error::other(format!(
                "unsupported fixture role {other}"
            )))
        }
    }
}

/// The double writes stderr first when asked for bulk, because a reader that
/// drains only stdout would deadlock against a full stderr pipe; with a delay
/// it writes part of stdout, goes quiet and then finishes, which is the shape
/// the adapter's bounded progress notice exists for.
fn cargo_double() -> io::Result<()> {
    record_ledger()?;
    let stdout = stream_bytes(
        "HARNESS_RTK_FIXTURE_CARGO_STDOUT",
        "HARNESS_RTK_FIXTURE_CARGO_STDOUT_BYTES",
    )?;
    let stderr = stream_bytes(
        "HARNESS_RTK_FIXTURE_CARGO_STDERR",
        "HARNESS_RTK_FIXTURE_CARGO_STDERR_BYTES",
    )?;
    let delay = std::env::var("HARNESS_RTK_FIXTURE_CARGO_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    if delay > 0 {
        let split = stdout.len() / 2;
        let mut out = io::stdout().lock();
        out.write_all(&stdout[..split])?;
        out.flush()?;
        drop(out);
        std::thread::sleep(Duration::from_millis(delay));
        io::stdout().lock().write_all(&stdout[split..])?;
        io::stderr().lock().write_all(&stderr)?;
    } else {
        io::stderr().lock().write_all(&stderr)?;
        io::stdout().lock().write_all(&stdout)?;
    }
    let code = std::env::var("HARNESS_RTK_FIXTURE_CARGO_EXIT")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0);
    std::process::exit(code);
}

/// One ledger line per invocation of the native command under test. The
/// `rtk.exe` harness roles never record, so the ledger counts source commands.
fn record_ledger() -> io::Result<()> {
    if let Some(path) = std::env::var_os("HARNESS_RTK_FIXTURE_LEDGER") {
        let mut ledger = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(ledger, "run pid={}", std::process::id())?;
    }
    Ok(())
}

/// A stream is either the literal text of one environment variable or that many
/// bytes of synthetic compiler-shaped output.
fn stream_bytes(text: &str, size: &str) -> io::Result<Vec<u8>> {
    if let Some(value) = std::env::var_os(text) {
        return Ok(value.to_string_lossy().into_owned().into_bytes());
    }
    let Some(count) = std::env::var(size)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return Ok(Vec::new());
    };
    let mut bytes = Vec::with_capacity(count + 96);
    let mut line = 0usize;
    while bytes.len() < count {
        line += 1;
        writeln!(
            bytes,
            "fixture-cargo line {line} of synthetic verification output for the rtk adapter double"
        )?;
    }
    Ok(bytes)
}
