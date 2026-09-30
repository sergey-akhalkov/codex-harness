//! Thin binary over the `token-audit` library.
use std::{env, io, io::Write, path::PathBuf, process::ExitCode};
use token_audit::{
    BaselineDiff, Detail, Format, RETENTION_LIMIT, RetainedKind, SCHEMA_VERSION, ScanOptions,
    analyze, baseline_diff, default_baseline_directory, default_retention_directory,
    default_sessions_root, now, render_findings_json, render_findings_text, render_json,
    render_text, resolve_baseline, retain_detail, save_baseline, scan, select_finding,
    select_session, write_private_sources,
};

fn usage() -> String {
    format!(
        "token-audit report [--sessions DIR] [--days N] [--format json|text] [--private-sources PATH]
token-audit findings [--sessions DIR] [--days N] [--format json|text] [--all-bases]
token-audit baseline save [--sessions DIR] [--days N] [--format json|text]
token-audit baseline diff [--sessions DIR] [--days N] [--format json|text] [--baseline NAME|latest]
token-audit detail --report PATH --session ID
token-audit detail --findings PATH --finding ID
token-audit detail --diff PATH --session ID | --group KIND:KEY | --sessions | --groups KIND [--offset N] [--limit N]
Measured token-usage reports over local Codex rollout sessions: recorded counters, instruction bytes and coverage. No currency, quota or transcript content.
Project identities are hashed; --private-sources PATH records local source digests and raw project names there instead, outside tracked sources.
--days N bounds the scan to sessions with recorded activity within N days.
--format json prints the complete machine contract. --format text prints a bounded ranked summary and retains its complete same-scan JSON under CODEX_HOME/harness/token-audit/reports (newest 20 per kind); the summary names that path.
baseline save publishes an immutable uniquely named snapshot under CODEX_HOME/harness/token-audit/baselines and updates its latest pointer; diff accepts the returned name, its basename or latest.
baseline diff --format text is bounded: {} most significant movements per section inside {} bytes, with exact omitted counts; it retains the complete comparison and names its locator. --format json stays the complete machine contract.
detail reads one session, finding or movement record from the retained JSON named by a summary: no session rescan, no model call, no network. An expired or evicted record is an explicit error. KIND is project, model, effort or day.
Exit codes: 0 success, 2 usage or input error, 3 command not implemented.",
        BaselineDiff::TEXT_ROWS,
        BaselineDiff::TEXT_BYTES
    )
}

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
        return Err(invalid(usage()));
    };
    match command.as_str() {
        "report" => report(&args[1..]),
        "findings" => findings(&args[1..]),
        "baseline" => baseline(&args[1..]),
        "detail" => detail(&args[1..]),
        "-h" | "--help" | "help" => emit(&usage()).map(|()| ExitCode::SUCCESS),
        other => Err(invalid(format!("unknown command {other}\n{}", usage()))),
    }
}

fn report(args: &[String]) -> io::Result<ExitCode> {
    if requests_help(args) {
        return emit(&usage()).map(|()| ExitCode::SUCCESS);
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
        Some("-h" | "--help" | "help") => emit(&usage()).map(|()| ExitCode::SUCCESS),
        Some(other) => Err(invalid(format!(
            "unknown baseline subcommand {other}\n{}",
            usage()
        ))),
        None => Err(invalid(format!(
            "baseline requires save or diff\n{}",
            usage()
        ))),
    }
}

fn findings(args: &[String]) -> io::Result<ExitCode> {
    if requests_help(args) {
        return emit(&usage()).map(|()| ExitCode::SUCCESS);
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

/// One bounded record read over a retained complete report or comparison.
enum DetailRequest {
    Session {
        path: PathBuf,
        id: String,
    },
    Finding {
        path: PathBuf,
        id: String,
    },
    DiffSession {
        path: PathBuf,
        id: String,
    },
    DiffGroup {
        path: PathBuf,
        kind: String,
        key: String,
    },
    DiffPage {
        path: PathBuf,
        kind: Option<String>,
        offset: usize,
        limit: usize,
    },
}

fn detail(args: &[String]) -> io::Result<ExitCode> {
    if requests_help(args) {
        return emit(&usage()).map(|()| ExitCode::SUCCESS);
    }
    let record = match parse_detail(args)? {
        DetailRequest::Session { path, id } => select_session(&path, &id)?,
        DetailRequest::Finding { path, id } => select_finding(&path, &id)?,
        DetailRequest::DiffSession { path, id } => {
            let comparison = BaselineDiff::open(&path)?;
            BaselineDiff::select_session(&comparison, &id)?
        }
        DetailRequest::DiffGroup { path, kind, key } => {
            let comparison = BaselineDiff::open(&path)?;
            BaselineDiff::select_group(&comparison, &kind, &key)?
        }
        DetailRequest::DiffPage {
            path,
            kind,
            offset,
            limit,
        } => {
            let comparison = BaselineDiff::open(&path)?;
            match &kind {
                Some(kind) => BaselineDiff::page_groups(&comparison, kind, offset, limit)?,
                None => BaselineDiff::page_sessions(&comparison, offset, limit)?,
            }
        }
    };
    let rendered = serde_json::to_string_pretty(&record).map_err(io::Error::other)?;
    emit(&format!("{rendered}\n")).map(|()| ExitCode::SUCCESS)
}

fn parse_detail(args: &[String]) -> io::Result<DetailRequest> {
    let mut report = None;
    let mut findings = None;
    let mut diff = None;
    let mut session = None;
    let mut finding = None;
    let mut group = None;
    let mut sessions = false;
    let mut groups = None;
    let mut offset = None;
    let mut limit = None;
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
            "--diff" => diff = Some(PathBuf::from(value()?)),
            "--session" => session = Some(value()?),
            "--finding" => finding = Some(value()?),
            "--group" => {
                let raw = value()?;
                let (kind, key) = raw
                    .split_once(':')
                    .filter(|(kind, key)| !kind.is_empty() && !key.is_empty())
                    .ok_or_else(|| {
                        invalid("--group needs KIND:KEY, for example project:KEY or day:2026-09-20")
                    })?;
                group = Some((kind.to_owned(), key.to_owned()));
            }
            "--sessions" => sessions = true,
            "--groups" => groups = Some(value()?),
            "--offset" => offset = Some(parse_movement_offset(&value()?)?),
            "--limit" => limit = Some(parse_movement_limit(&value()?)?),
            other => return Err(invalid(format!("unknown argument {other}\n{}", usage()))),
        }
    }
    let single = offset.is_none() && limit.is_none();
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(BaselineDiff::DETAIL_LIMIT_DEFAULT);
    match (
        report, findings, diff, session, finding, group, sessions, groups,
    ) {
        (Some(path), None, None, Some(id), None, None, false, None) if single => {
            Ok(DetailRequest::Session { path, id })
        }
        (None, Some(path), None, None, Some(id), None, false, None) if single => {
            Ok(DetailRequest::Finding { path, id })
        }
        (None, None, Some(path), Some(id), None, None, false, None) if single => {
            Ok(DetailRequest::DiffSession { path, id })
        }
        (None, None, Some(path), None, None, Some((kind, key)), false, None) if single => {
            Ok(DetailRequest::DiffGroup { path, kind, key })
        }
        (None, None, Some(path), None, None, None, true, None) => Ok(DetailRequest::DiffPage {
            path,
            kind: None,
            offset,
            limit,
        }),
        (None, None, Some(path), None, None, None, false, Some(kind)) => {
            Ok(DetailRequest::DiffPage {
                path,
                kind: Some(kind),
                offset,
                limit,
            })
        }
        _ => Err(invalid(format!(
            "detail needs --report PATH with --session ID, --findings PATH with --finding ID, or --diff PATH with --session ID, --group KIND:KEY, --sessions or --groups KIND; retention keeps the newest {RETENTION_LIMIT} files per kind\n{}",
            usage()
        ))),
    }
}

fn parse_movement_offset(raw: &str) -> io::Result<usize> {
    raw.parse::<usize>()
        .map_err(|_| invalid("--offset needs a non-negative integer"))
}

fn parse_movement_limit(raw: &str) -> io::Result<usize> {
    raw.parse::<usize>()
        .ok()
        .filter(|limit| (1..=BaselineDiff::DETAIL_LIMIT_MAX).contains(limit))
        .ok_or_else(|| {
            invalid(format!(
                "--limit needs an integer between 1 and {}",
                BaselineDiff::DETAIL_LIMIT_MAX
            ))
        })
}

fn baseline_run(args: &[String], diff_mode: bool) -> io::Result<ExitCode> {
    if requests_help(args) {
        return emit(&usage()).map(|()| ExitCode::SUCCESS);
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
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("baseline")
        .to_owned();
    let analyzed = baseline_diff(&path, &name, &scanned)?;
    let rendered = match options.format() {
        Format::Json => serde_json::to_string_pretty(&analyzed)
            .map(|rendered| format!("{rendered}\n"))
            .map_err(io::Error::other)?,
        Format::Text => {
            let detail = analyzed.retain_complete();
            analyzed.render_text(&detail)
        }
    };
    emit(&rendered).map(|()| ExitCode::SUCCESS)
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
                other => return Err(invalid(format!("unknown argument {other}\n{}", usage()))),
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
