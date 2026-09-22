//! Thin binary over the `token-audit` library.
use std::{env, io, io::Write, path::PathBuf, process::ExitCode};
use token_audit::{
    BaselineDiff, Detail, Format, RETENTION_LIMIT, RetainedKind, SCHEMA_VERSION, ScanOptions,
    analyze, baseline_diff, default_baseline_directory, default_retention_directory,
    default_sessions_root, now, render_findings_json, render_findings_text, render_json,
    render_text, resolve_baseline, retain_detail, save_baseline, scan, select_finding,
    select_session, write_private_sources,
};

const USAGE: &str = "\
token-audit report [--sessions DIR] [--days N] [--format json|text] [--private-sources PATH]
token-audit findings [--sessions DIR] [--days N] [--format json|text] [--all-bases]
token-audit baseline save [--sessions DIR] [--days N] [--format json|text]
token-audit baseline diff [--sessions DIR] [--days N] [--format json|text] [--baseline NAME|latest]
token-audit detail --report PATH --session ID
token-audit detail --findings PATH --finding ID
Measured token-usage reports over local Codex rollout sessions: recorded counters, instruction bytes and coverage. No currency, quota or transcript content.
Project identities are hashed; --private-sources PATH records local source digests and raw project names there instead, outside tracked sources.
--days N bounds the scan to sessions with recorded activity within N days.
--format json prints the complete machine contract. --format text prints a bounded ranked summary and retains its complete same-scan JSON under CODEX_HOME/harness/token-audit/reports (newest 20 per kind); the summary names that path.
detail reads one session or finding record from the retained JSON named by a summary: no session rescan, no model call, no network. An expired or evicted record is an explicit error.
Exit codes: 0 success, 2 usage or input error, 3 command not implemented.";

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
        "detail" => detail(&args[1..]),
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
        Format::Text => {
            let detail = retain_complete(RetainedKind::Report, &render_json(&scanned.report));
            render_text(&scanned.report, &detail)
        }
    };
    emit(&rendered).map(|()| ExitCode::SUCCESS)
}

fn baseline(args: &[String]) -> io::Result<ExitCode> {
    match args.first().map(String::as_str) {
        Some("save") => baseline_run(&args[1..], false),
        Some("diff") => baseline_run(&args[1..], true),
        Some("-h" | "--help" | "help") => emit(USAGE).map(|()| ExitCode::SUCCESS),
        Some(other) => Err(invalid(format!(
            "unknown baseline subcommand {other}\n{USAGE}"
        ))),
        None => Err(invalid(format!("baseline requires save or diff\n{USAGE}"))),
    }
}

fn findings(args: &[String]) -> io::Result<ExitCode> {
    if requests_help(args) {
        return emit(USAGE).map(|()| ExitCode::SUCCESS);
    }
    let mut options = Options::parse(args)?;
    let all_bases = options.take_all_bases();
    if options.baseline.is_some() {
        return Err(invalid("--baseline applies to baseline diff only"));
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
    let analyzed = analyze(&scanned.report, !all_bases);
    let rendered = match options.format() {
        Format::Json => render_findings_json(&analyzed),
        Format::Text => {
            let detail = retain_complete(RetainedKind::Findings, &render_findings_json(&analyzed));
            render_findings_text(&analyzed, &detail)
        }
    };
    emit(&rendered).map(|()| ExitCode::SUCCESS)
}

/// Retains the complete same-scan JSON that a bounded presentation summarizes.
fn retain_complete(kind: RetainedKind, complete_json: &str) -> Detail {
    let Some(directory) = default_retention_directory() else {
        return Detail::Unavailable(
            "CODEX_HOME or USERPROFILE is required to retain the complete report".to_owned(),
        );
    };
    retain_detail(&directory, kind, complete_json)
        .unwrap_or_else(|error| Detail::Unavailable(error.to_string()))
}

/// One bounded record read over a retained complete report.
enum DetailRequest {
    Session { path: PathBuf, id: String },
    Finding { path: PathBuf, id: String },
}

fn detail(args: &[String]) -> io::Result<ExitCode> {
    if requests_help(args) {
        return emit(USAGE).map(|()| ExitCode::SUCCESS);
    }
    let record = match parse_detail(args)? {
        DetailRequest::Session { path, id } => select_session(&path, &id)?,
        DetailRequest::Finding { path, id } => select_finding(&path, &id)?,
    };
    let rendered = serde_json::to_string_pretty(&record).map_err(io::Error::other)?;
    emit(&format!("{rendered}\n")).map(|()| ExitCode::SUCCESS)
}

fn parse_detail(args: &[String]) -> io::Result<DetailRequest> {
    let mut report = None;
    let mut findings = None;
    let mut session = None;
    let mut finding = None;
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
            "--report" => report = Some(PathBuf::from(value()?)),
            "--findings" => findings = Some(PathBuf::from(value()?)),
            "--session" => session = Some(value()?),
            "--finding" => finding = Some(value()?),
            other => return Err(invalid(format!("unknown argument {other}\n{USAGE}"))),
        }
    }
    match (report, findings, session, finding) {
        (Some(path), None, Some(id), None) => Ok(DetailRequest::Session { path, id }),
        (None, Some(path), None, Some(id)) => Ok(DetailRequest::Finding { path, id }),
        _ => Err(invalid(format!(
            "detail needs --report PATH with --session ID, or --findings PATH with --finding ID; retention keeps the newest {RETENTION_LIMIT} files per kind\n{USAGE}"
        ))),
    }
}

fn baseline_run(args: &[String], diff_mode: bool) -> io::Result<ExitCode> {
    if requests_help(args) {
        return emit(USAGE).map(|()| ExitCode::SUCCESS);
    }
    let mut options = Options::parse(args)?;
    if options.all_bases {
        return Err(invalid("--all-bases applies to findings only"));
    }
    let requested = options.take_baseline();
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
    let directory = default_baseline_directory().ok_or_else(|| {
        invalid("CODEX_HOME or USERPROFILE is required for the baseline directory")
    })?;
    let scanned = scan(&ScanOptions {
        sessions_root: root,
        days: options.days,
        generated_at: now(),
    })?;
    if !diff_mode {
        let name = save_baseline(&directory, &scanned)?;
        let rendered = match options.format() {
            Format::Json => serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": SCHEMA_VERSION,
                "command": "baseline save",
                "baseline": name,
                "directory": directory.display().to_string(),
            }))
            .map_err(io::Error::other)?,
            Format::Text => format!(
                "token-audit baseline save  {}\n  directory {}\n",
                name,
                directory.display()
            ),
        };
        return emit(&rendered).map(|()| ExitCode::SUCCESS);
    }
    let requested = requested.as_deref().or(Some("latest"));
    let path = resolve_baseline(&directory, requested)?;
    let name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("baseline")
        .to_owned();
    let analyzed = baseline_diff(&path, &name, &scanned)?;
    let rendered = match options.format() {
        Format::Json => serde_json::to_string_pretty(&analyzed)
            .map(|rendered| format!("{rendered}\n"))
            .map_err(io::Error::other)?,
        Format::Text => render_baseline_text(&analyzed),
    };
    emit(&rendered).map(|()| ExitCode::SUCCESS)
}

fn render_baseline_text(diff: &BaselineDiff) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "token-audit baseline diff  generated={}  baseline={}\n",
        diff.generated_at, diff.baseline
    ));
    if !diff.compatible {
        out.push_str(&format!(
            "incompatible snapshot: {}\n",
            diff.incompatibility.as_deref().unwrap_or("unknown")
        ));
    }
    out.push_str(&format!(
        "totals sessions {} -> {}  total_tokens {:?} -> {:?}  delta {:?}\n",
        diff.totals.baseline_sessions,
        diff.totals.current_sessions,
        diff.totals.baseline_total_tokens,
        diff.totals.current_total_tokens,
        diff.totals.delta_total_tokens
    ));
    for movement in &diff.sessions {
        out.push_str(&format!(
            "session {} status={} total {:?} -> {:?} delta {:?}\n",
            movement.session_id,
            movement.status,
            movement.baseline_total_tokens,
            movement.current_total_tokens,
            movement.delta_total_tokens
        ));
    }
    for (label, buckets) in [
        ("project", &diff.by_project),
        ("model", &diff.by_model),
        ("day", &diff.by_day),
    ] {
        for movement in buckets {
            out.push_str(&format!(
                "{label} {} sessions {} -> {} total {:?} -> {:?} delta {:?}\n",
                movement.key,
                movement.baseline_sessions,
                movement.current_sessions,
                movement.baseline_total_tokens,
                movement.current_total_tokens,
                movement.delta_total_tokens
            ));
        }
    }
    out.push_str(&format!("limitation {}\n", diff.limitation));
    out
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
    baseline: Option<String>,
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
                "--baseline" => options.baseline = Some(value()?),
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

    /// Extracts the baseline-selection flag shared by the diff subcommand.
    fn take_baseline(&mut self) -> Option<String> {
        self.baseline.take()
    }

    fn format(&self) -> Format {
        self.format.unwrap_or(Format::Json)
    }
}
