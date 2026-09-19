use std::env;
use std::io::{self, Error, ErrorKind, Write};
use std::process::ExitCode;
use windows_disk_reclaim::{
    Method, ReclaimOptions, SnapshotOptions, bytes_human, default_categories, reclaim, snapshot,
};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> io::Result<()> {
    let command = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, usage()))?;
    match command {
        "snapshot" => snapshot_cmd(&args[1..]),
        "reclaim" => reclaim_cmd(&args[1..]),
        "categories" => categories_cmd(&args[1..]),
        "-h" | "--help" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        _ => Err(Error::new(ErrorKind::InvalidInput, usage())),
    }
}

fn usage() -> &'static str {
    "windows-disk-reclaim snapshot [--drive C] [--top 25] [--method auto|mft|walk] [--format json|text]\n\
     windows-disk-reclaim reclaim [--drive C] [--apply] [--empty-recycle] [--min-age-hours 48] [--format json|text]\n\
     windows-disk-reclaim categories [--drive C] [--format json|text]\n\
     Deletion never ranks by size. --apply only removes files inside the built-in allowlist."
}

struct Flags {
    drive: char,
    top: usize,
    method: Method,
    format: Format,
    apply: bool,
    empty_recycle: bool,
    min_age_hours: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Text,
}

fn parse_flags(args: &[String]) -> io::Result<Flags> {
    let mut flags = Flags {
        drive: 'C',
        top: 25,
        method: Method::Auto,
        format: Format::Json,
        apply: false,
        empty_recycle: false,
        min_age_hours: 48,
    };
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--drive" => {
                flags.drive = letter(value(args, &mut index)?)?;
            }
            "--top" => flags.top = parse_usize(value(args, &mut index)?, "--top")?,
            "--method" => flags.method = Method::parse(value(args, &mut index)?)?,
            "--format" => flags.format = parse_format(value(args, &mut index)?)?,
            "--min-age-hours" => {
                flags.min_age_hours = parse_u64(value(args, &mut index)?, "--min-age-hours")?
            }
            "--apply" => flags.apply = true,
            "--empty-recycle" => flags.empty_recycle = true,
            other => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!("unknown argument {other}"),
                ));
            }
        }
        index += 1;
    }
    Ok(flags)
}

fn value<'a>(args: &'a [String], index: &mut usize) -> io::Result<&'a str> {
    *index += 1;
    args.get(*index)
        .map(String::as_str)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "missing flag value"))
}

fn letter(value: &str) -> io::Result<char> {
    let mut chars = value.chars();
    let letter = chars.next().unwrap_or_default();
    if chars.next().is_none() && letter.is_ascii_alphabetic() {
        Ok(letter.to_ascii_uppercase())
    } else {
        Err(Error::new(
            ErrorKind::InvalidInput,
            "drive must be a single letter",
        ))
    }
}

fn parse_format(value: &str) -> io::Result<Format> {
    match value {
        "json" => Ok(Format::Json),
        "text" => Ok(Format::Text),
        _ => Err(Error::new(
            ErrorKind::InvalidInput,
            "format must be json or text",
        )),
    }
}

fn parse_usize(value: &str, flag: &str) -> io::Result<usize> {
    value.parse().map_err(|_| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("{flag} must be a positive integer"),
        )
    })
}

fn parse_u64(value: &str, flag: &str) -> io::Result<u64> {
    value.parse().map_err(|_| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("{flag} must be a non-negative integer"),
        )
    })
}

fn snapshot_cmd(args: &[String]) -> io::Result<()> {
    let flags = parse_flags(args)?;
    let report = snapshot(SnapshotOptions {
        drive: flags.drive,
        root: None,
        top: flags.top.max(1),
        method: flags.method,
    })?;
    emit(flags.format, &report, &snapshot_text(&report))
}

fn reclaim_cmd(args: &[String]) -> io::Result<()> {
    let flags = parse_flags(args)?;
    let report = reclaim(ReclaimOptions {
        drive: flags.drive,
        apply: flags.apply,
        min_age_hours: flags.min_age_hours,
        empty_recycle: flags.empty_recycle,
        categories: default_categories(flags.drive),
    })?;
    emit(flags.format, &report, &reclaim_text(&report))
}

fn categories_cmd(args: &[String]) -> io::Result<()> {
    let flags = parse_flags(args)?;
    let categories: Vec<_> = default_categories(flags.drive)
        .into_iter()
        .map(|item| {
            serde_json::json!({
                "id": item.id,
                "description": item.description,
                "min_age_hours": item.min_age_hours,
                "path": item.path,
            })
        })
        .collect();
    if flags.format == Format::Json {
        emit_json(&categories)
    } else {
        for item in default_categories(flags.drive) {
            println!("{}  {}  {}", item.id, item.path.display(), item.description);
        }
        Ok(())
    }
}

fn emit<T: serde::Serialize>(format: Format, value: &T, text: &str) -> io::Result<()> {
    match format {
        Format::Json => emit_json(value),
        Format::Text => {
            print!("{text}");
            Ok(())
        }
    }
}

fn emit_json<T: serde::Serialize>(value: &T) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value)
        .map_err(|err| Error::other(err.to_string()))?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn snapshot_text(report: &windows_disk_reclaim::SnapshotReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Drive {}: used {}  free {}  total {}  method={}  {} ms\n",
        report.drive,
        bytes_human(report.volume.used_bytes),
        bytes_human(report.volume.free_bytes),
        bytes_human(report.volume.total_bytes),
        report.method,
        report.elapsed_ms
    ));
    if !report.top_level.is_empty() {
        out.push_str("Top-level:\n");
        for entry in &report.top_level {
            out.push_str(&format!(
                "  {:>10}  {}\n",
                bytes_human(entry.bytes),
                entry.path
            ));
        }
    }
    out.push_str("Top directories:\n");
    for entry in &report.top_directories {
        out.push_str(&format!(
            "  {:>10}  {}\n",
            bytes_human(entry.bytes),
            entry.path
        ));
    }
    out.push_str("Top files:\n");
    for entry in &report.top_files {
        out.push_str(&format!(
            "  {:>10}  {}\n",
            bytes_human(entry.bytes),
            entry.path
        ));
    }
    out.push_str(&format!(
        "Allowlisted reclaimable: {}\n",
        bytes_human(report.reclaimable_bytes)
    ));
    for item in &report.reclaimable {
        if item.candidate_bytes == 0 {
            continue;
        }
        out.push_str(&format!(
            "  {:>10}  {}  {}\n",
            bytes_human(item.candidate_bytes),
            item.id,
            item.path
        ));
    }
    out
}

fn reclaim_text(report: &windows_disk_reclaim::ReclaimReport) -> String {
    let mut out = String::new();
    let action = if report.apply { "applied" } else { "dry-run" };
    out.push_str(&format!(
        "Drive {} {action}: candidates {} ({})  deleted {} ({})  skipped {}  recycle_emptied={}  {} ms\n",
        report.drive,
        report.candidate_files,
        bytes_human(report.candidate_bytes),
        report.deleted_files,
        bytes_human(report.freed_bytes),
        report.skipped_files,
        report.recycle_emptied,
        report.elapsed_ms
    ));
    for item in &report.categories {
        out.push_str(&format!(
            "  {}  {}  candidates {}  deleted {}\n",
            item.id,
            bytes_human(item.candidate_bytes),
            item.candidate_files,
            item.deleted_files
        ));
    }
    out
}
