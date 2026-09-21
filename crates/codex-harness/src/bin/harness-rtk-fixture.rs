//! Inert double for the pinned RTK `pipe --filter` contract and for one
//! allowlisted source command, so adapter acceptance needs no RTK download and
//! no network. Tests copy it as `rtk.exe` (compression double) or as the
//! command under test. It reads stdin, writes stdout, and keeps stderr empty.

use std::io::{self, Read, Write};

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
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input)?;
            let lines = input.iter().filter(|byte| **byte == b'\n').count();
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
        other => Err(io::Error::other(format!(
            "unsupported fixture role {other}"
        ))),
    }
}
