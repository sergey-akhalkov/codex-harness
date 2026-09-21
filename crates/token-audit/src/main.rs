//! Thin binary over the `token-audit` library.
use std::{env, io, io::Write, path::PathBuf, process::ExitCode};
use token_audit::{
    Format, SCHEMA_VERSION, ScanOptions, analyze, default_sessions_root, now, render_findings_json,
    render_findings_text, render_json, render_text, scan, write_private_sources,
};

const USAGE: &str = "\
token-audit report [--sessions DIR] [--days N] [--format json|text] [--private-sources PATH]
token-audit findings [--sessions DIR] [--days N] [--format json|text] [--all-bases]
token-audit baseline save|diff [--days N] [--format json|text]
Measured token-usage reports over local Codex rollout sessions: recorded counters, instruction bytes and coverage. No currency, quota or transcript content.
Project identities are hashed; --private-sources PATH records local source digests and raw project names there instead, outside tracked sources.
--days N bounds the scan to sessions with recorded activity within N days.
Exit codes: 0 success, 2 usage or input error, 3 command not implemented.";

const SECTION_BASELINE: &str = "section 4 (baseline)";

fn main() -> ExitCode {
    match run(&env::args().skip(1).collect::<Vec<_>>()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("token-audit: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> io::Result<ExitCode> {
    let Some(command) = args.first() else {
        return Err(invalid(USAGE));
    };
    match command.as_str() {
        "report" => report(&args[1..]),
        "findings" => findings(&args[1..]),
        "baseline" => baseline(&args[1..]),
        "-h" | "--help" | "help" => emit(USAGE).map(|()| ExitCode::SUCCESS),
        other => Err(invalid(format!("unknown command {other}\n{USAGE}"))),
    }
}

fn report(args: &[String]) -> io::Result<ExitCode> {
    if requests_help(args) {
        return emit(USAGE).map(|()| ExitCode::SUCCESS);
    }
    let options = Options::parse(args)?;
    if options.all_bases {
        return Err(invalid("--all-bases applies to findings only"));
    }
    let root = match &options.sessions {
        Some(root) => root.clone(),
        None => default_sessions_root().ok_or_else(|| {
            invalid("CODEX_HOME or USERPROFILE is required; pass --sessions DIRECTORY")
        })?,
    };
    if !root.exists() {
        return Err(invalid(format!(
            "sessions directory not found: {}; pass --sessions DIRECTORY or set CODEX_HOME",
            root.display()
        )));
    }
    let scanned = scan(&ScanOptions {
        sessions_root: root,
        days: options.days,
        generated_at: now(),
    })?;
    if let Some(path) = &options.private_sources {
        write_private_sources(path, &scanned)?;
    }
    let rendered = match options.format() {
        Format::Json => render_json(&scanned.report),
        Format::Text => render_text(&scanned.report),
    };
    emit(&rendered).map(|()| ExitCode::SUCCESS)
}

fn skeleton(command: &str, section: &str, args: &[String]) -> io::Result<ExitCode> {
    if requests_help(args) {
        return emit(USAGE).map(|()| ExitCode::SUCCESS);
    }
    let options = Options::parse(args)?;
    let message =
        format!("{command} is not implemented in this build (change token-audit {section})");
    let rendered = match options.format() {
        Format::Json => format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": SCHEMA_VERSION,
                "command": command,
                "status": "not_implemented",
                "section": section,
                "message": message,
            }))
            .map_err(io::Error::other)?
        ),
        Format::Text => {
            format!("token-audit {command}: not implemented ({section}); no results produced\n")
        }
    };
    emit(&rendered)?;
    Ok(ExitCode::from(3))
}

fn findings(args: &[String]) -> io::Result<ExitCode> {
    if requests_help(args) {
        return emit(USAGE).map(|()| ExitCode::SUCCESS);
    }
    let mut options = Options::parse(args)?;
    let all_bases = options.take_all_bases();
    let root = match &options.sessions {
        Some(root) => root.clone(),
        None => default_sessions_root().ok_or_else(|| {
            invalid("CODEX_HOME or USERPROFILE is required; pass --sessions DIRECTORY")
        })?,
    };
    if !root.exists() {
        return Err(invalid(format!(
            "sessions directory not found: {}; pass --sessions DIRECTORY or set CODEX_HOME",
            root.display()
        )));
    }
    let scanned = scan(&ScanOptions {
        sessions_root: root,
        days: options.days,
        generated_at: now(),
    })?;
    if let Some(path) = &options.private_sources {
        write_private_sources(path, &scanned)?;
    }
    let analyzed = analyze(&scanned.report, !all_bases);
    let rendered = match options.format() {
        Format::Json => render_findings_json(&analyzed),
        Format::Text => render_findings_text(&analyzed),
    };
    emit(&rendered).map(|()| ExitCode::SUCCESS)
}

fn baseline(args: &[String]) -> io::Result<ExitCode> {
    match args.first().map(String::as_str) {
        Some("save") => skeleton("baseline save", SECTION_BASELINE, &args[1..]),
        Some("diff") => skeleton("baseline diff", SECTION_BASELINE, &args[1..]),
        Some("-h" | "--help" | "help") => emit(USAGE).map(|()| ExitCode::SUCCESS),
        Some(other) => Err(invalid(format!(
            "unknown baseline subcommand {other}\n{USAGE}"
        ))),
        None => Err(invalid(format!("baseline requires save or diff\n{USAGE}"))),
    }
}

fn requests_help(args: &[String]) -> bool {
    args.iter()
        .any(|argument| matches!(argument.as_str(), "-h" | "--help" | "help"))
}

fn emit(text: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    stdout.flush()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[derive(Default)]
struct Options {
    sessions: Option<PathBuf>,
    days: Option<u32>,
    format: Option<Format>,
    private_sources: Option<PathBuf>,
    all_bases: bool,
}

impl Options {
    fn parse(args: &[String]) -> io::Result<Self> {
        let mut options = Self::default();
        let mut index = 0;
        while index < args.len() {
            let argument = &args[index];
            index += 1;
            let (flag, inline) = match argument.split_once('=') {
                Some((flag, value)) => (flag, Some(value.to_owned())),
                None => (argument.as_str(), None),
            };
            let mut value = || -> io::Result<String> {
                if let Some(value) = inline.clone() {
                    return Ok(value);
                }
                let value = args
                    .get(index)
                    .ok_or_else(|| invalid(format!("{flag} needs a value")))?;
                index += 1;
                Ok(value.clone())
            };
            match flag {
                "--sessions" => options.sessions = Some(PathBuf::from(value()?)),
                "--private-sources" => options.private_sources = Some(PathBuf::from(value()?)),
                "--all-bases" => options.all_bases = true,
                "--days" => {
                    let raw = value()?;
                    options.days = Some(
                        raw.parse::<u32>()
                            .ok()
                            .filter(|days| *days > 0)
                            .ok_or_else(|| invalid("--days needs a positive integer"))?,
                    );
                }
                "--format" => {
                    let raw = value()?;
                    options.format = Some(
                        Format::parse(&raw)
                            .ok_or_else(|| invalid("--format must be json or text"))?,
                    );
                }
                other => return Err(invalid(format!("unknown argument {other}\n{USAGE}"))),
            }
        }
        Ok(options)
    }

    /// Extracts the findings-only flag before shared validation runs.
    fn take_all_bases(&mut self) -> bool {
        std::mem::take(&mut self.all_bases)
    }

    fn format(&self) -> Format {
        self.format.unwrap_or(Format::Json)
    }
}
