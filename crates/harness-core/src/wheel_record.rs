//! Read-only wheel RECORD observation. RECORD is local installation evidence,
//! not a trusted upstream signature. This module never executes package code
//! or mutates files.
#![cfg(windows)]

use crate::dependency_package::{contained, resolved};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

const RECORD_LIMIT: u64 = 4 * 1024 * 1024;
const ROW_LIMIT: usize = 32768;
const FIELD_LIMIT: usize = 32 * 1024;
const FILE_LIMIT: u64 = 128 * 1024 * 1024;
const AGGREGATE_LIMIT: u64 = 512 * 1024 * 1024;
const SHARE_READ: u32 = 1;

struct Observation {
    checked_files: u64,
    issues: Vec<Value>,
}

struct Failure {
    observation: Observation,
    reason: &'static str,
}

enum FieldState {
    Start,
    Unquoted,
    Quoted,
    AfterQuoted,
}

fn io_reason(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::NotFound => "FileNotFoundError",
        io::ErrorKind::PermissionDenied => "PermissionError",
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => "ValueError",
        _ => "OSError",
    }
}

fn issue(reason: &str, path: Option<&str>) -> Value {
    match path {
        Some(path) => json!({"reason": reason, "path": path}),
        None => json!({"reason": reason}),
    }
}

fn report(state: &str, observation: Observation, basis: String) -> Value {
    json!({
        "state": state,
        "checked_files": observation.checked_files,
        "issues": observation.issues,
        "basis": basis,
    })
}

fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ)
        .open(path)
}

fn canonical(path: &Path) -> Result<PathBuf, &'static str> {
    resolved(path).map_err(|error| io_reason(&error))
}

fn push_char(field: &mut String, ch: char) -> Result<(), &'static str> {
    field.push(ch);
    if field.len() > FIELD_LIMIT {
        Err("ValueError")
    } else {
        Ok(())
    }
}

fn finish_field(row: &mut Vec<String>, field: &mut String) -> Result<(), &'static str> {
    if field.contains('\0') || row.len() >= 8 {
        return Err("ValueError");
    }
    row.push(std::mem::take(field));
    Ok(())
}

fn finish_row(
    rows: &mut Vec<Vec<String>>,
    row: &mut Vec<String>,
    field: &mut String,
) -> Result<(), &'static str> {
    finish_field(row, field)?;
    if rows.len() >= ROW_LIMIT {
        return Err("ValueError");
    }
    rows.push(std::mem::take(row));
    Ok(())
}

fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, &'static str> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut state = FieldState::Start;
    let mut row_started = false;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match state {
            FieldState::Start | FieldState::Unquoted => match ch {
                '"' if matches!(state, FieldState::Start) => {
                    row_started = true;
                    state = FieldState::Quoted;
                }
                '"' => return Err("Error"),
                ',' => {
                    row_started = true;
                    finish_field(&mut row, &mut field)?;
                    state = FieldState::Start;
                }
                '\n' => {
                    if row_started {
                        finish_row(&mut rows, &mut row, &mut field)?;
                    }
                    state = FieldState::Start;
                    row_started = false;
                }
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    if row_started {
                        finish_row(&mut rows, &mut row, &mut field)?;
                    }
                    state = FieldState::Start;
                    row_started = false;
                }
                _ => {
                    row_started = true;
                    push_char(&mut field, ch)?;
                    state = FieldState::Unquoted;
                }
            },
            FieldState::Quoted => {
                row_started = true;
                if ch == '"' {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        push_char(&mut field, '"')?;
                    } else {
                        state = FieldState::AfterQuoted;
                    }
                } else {
                    push_char(&mut field, ch)?;
                }
            }
            FieldState::AfterQuoted => match ch {
                ',' => {
                    finish_field(&mut row, &mut field)?;
                    state = FieldState::Start;
                }
                '\n' => {
                    finish_row(&mut rows, &mut row, &mut field)?;
                    state = FieldState::Start;
                    row_started = false;
                }
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    finish_row(&mut rows, &mut row, &mut field)?;
                    state = FieldState::Start;
                    row_started = false;
                }
                _ => return Err("Error"),
            },
        }
    }
    if matches!(state, FieldState::Quoted) {
        return Err("Error");
    }
    if row_started {
        finish_row(&mut rows, &mut row, &mut field)?;
    }
    Ok(rows)
}

fn urlsafe_nopad(digest: &[u8; 32]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(43);
    let mut index = 0;
    while index + 3 <= digest.len() {
        let n = (u32::from(digest[index]) << 16)
            | (u32::from(digest[index + 1]) << 8)
            | u32::from(digest[index + 2]);
        out.push(TABLE[(n >> 18) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(n & 0x3f) as usize] as char);
        index += 3;
    }
    let n = (u32::from(digest[index]) << 16) | (u32::from(digest[index + 1]) << 8);
    out.push(TABLE[(n >> 18) as usize] as char);
    out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
    out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
    out
}

fn selected(name: &str, target: &Path, full: bool, package_dirs: &[&str]) -> bool {
    if full {
        return true;
    }
    let in_package = package_dirs.iter().any(|prefix| {
        name.strip_prefix(*prefix)
            .is_some_and(|rest| rest.starts_with('/'))
    });
    let python = target.extension().is_some_and(|ext| ext == "py");
    (python && in_package) || name.ends_with("/METADATA") || name.ends_with("/entry_points.txt")
}

fn hash_file(path: &Path, hashed: &mut u64) -> Result<[u8; 32], &'static str> {
    let mut file = open(path).map_err(|error| io_reason(&error))?;
    let metadata = file.metadata().map_err(|error| io_reason(&error))?;
    if !metadata.is_file() {
        return Err("OSError");
    }
    let len = metadata.len();
    if len > FILE_LIMIT || hashed.saturating_add(len) > AGGREGATE_LIMIT {
        return Err("ValueError");
    }
    *hashed += len;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut read_total = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(|error| io_reason(&error))?;
        if count == 0 {
            break;
        }
        read_total += count as u64;
        if read_total > FILE_LIMIT {
            return Err("ValueError");
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}

fn fail(observation: Observation, reason: &'static str) -> Result<Observation, Failure> {
    Err(Failure {
        observation,
        reason,
    })
}

fn observe(
    record: &Path,
    dist_info: &Path,
    install_root: &Path,
    full: bool,
    package_dirs: &[&str],
) -> Result<Observation, Failure> {
    let mut observation = Observation {
        checked_files: 0,
        issues: Vec::new(),
    };

    let metadata = match fs::metadata(record) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(observation),
        Err(error) => return fail(observation, io_reason(&error)),
    };
    if !metadata.is_file() {
        return Ok(observation);
    }

    let resolved_record = match canonical(record) {
        Ok(path) => path,
        Err(reason) => return fail(observation, reason),
    };
    let root = match canonical(install_root) {
        Ok(path) => path,
        Err(reason) => return fail(observation, reason),
    };
    if !contained(&resolved_record, &root) {
        observation
            .issues
            .push(issue("record-path-outside-installation", Some("RECORD")));
        return Ok(observation);
    }
    if metadata.len() > RECORD_LIMIT {
        return fail(observation, "ValueError");
    }

    let mut bytes = Vec::new();
    match open(&resolved_record) {
        Ok(file) => {
            if let Err(error) = file.take(RECORD_LIMIT + 1).read_to_end(&mut bytes) {
                return fail(observation, io_reason(&error));
            }
        }
        Err(error) => return fail(observation, io_reason(&error)),
    }
    if bytes.len() as u64 > RECORD_LIMIT {
        return fail(observation, "ValueError");
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => return fail(observation, "UnicodeDecodeError"),
    };
    let rows = match parse_csv(&text) {
        Ok(rows) => rows,
        Err(reason) => return fail(observation, reason),
    };

    let Some(package_root) = dist_info.parent() else {
        return fail(observation, "ValueError");
    };
    let mut hashed = 0u64;

    for row in rows {
        if row.len() < 3 {
            observation.issues.push(issue("malformed-record", None));
            continue;
        }
        if row.len() != 3 {
            return fail(observation, "ValueError");
        }
        let name = &row[0];
        let encoded = &row[1];
        let target = match canonical(&package_root.join(name)) {
            Ok(path) => path,
            Err(reason) => return fail(observation, reason),
        };
        if !contained(&target, &root) {
            observation
                .issues
                .push(issue("record-path-outside-installation", Some(name)));
            continue;
        }
        if !selected(name, &target, full, package_dirs) || encoded.is_empty() {
            continue;
        }
        let exists_file = match fs::metadata(&target) {
            Ok(meta) => meta.is_file(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return fail(observation, io_reason(&error)),
        };
        if !exists_file {
            observation
                .issues
                .push(issue("missing-recorded-file", Some(name)));
            continue;
        }
        let Some((algorithm, value)) = encoded.split_once('=') else {
            return fail(observation, "ValueError");
        };
        if algorithm != "sha256" {
            observation
                .issues
                .push(issue("unsupported-record-hash", Some(name)));
            continue;
        }
        let digest = match hash_file(&target, &mut hashed) {
            Ok(digest) => digest,
            Err(reason) => return fail(observation, reason),
        };
        observation.checked_files += 1;
        if urlsafe_nopad(&digest) != value {
            observation
                .issues
                .push(issue("record-hash-mismatch", Some(name)));
        }
    }
    Ok(observation)
}

/// Verify a wheel RECORD against files under `install_root`.
///
/// Default mode checks package-directory Python sources plus every
/// `/METADATA` and `/entry_points.txt` entry. Full mode checks every hashed
/// path. Empty hashes are skipped. RECORD remains local evidence only.
pub fn verify_record(
    dist_info: &Path,
    install_root: &Path,
    full: bool,
    package_dirs: &[&str],
) -> Value {
    let record = dist_info.join("RECORD");
    let basis = record.to_string_lossy().into_owned();
    match observe(&record, dist_info, install_root, full, package_dirs) {
        Ok(observation) => {
            let state = if !observation.issues.is_empty() {
                "modified"
            } else if observation.checked_files > 0 {
                "record-matches"
            } else {
                "unknown"
            };
            report(state, observation, basis)
        }
        Err(Failure {
            mut observation,
            reason,
        }) => {
            observation.issues.push(issue(reason, None));
            report("unknown", observation, basis)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::{symlink_dir, symlink_file};

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    fn digest_of(bytes: &[u8]) -> String {
        urlsafe_nopad(&Sha256::digest(bytes).into())
    }

    fn csv_field(name: &str) -> String {
        if name.contains(['"', ',', '\n', '\r']) {
            format!("\"{}\"", name.replace('"', "\"\""))
        } else {
            name.to_string()
        }
    }

    fn row(name: &str, bytes: &[u8]) -> String {
        format!(
            "{},sha256={},{}",
            csv_field(name),
            digest_of(bytes),
            bytes.len()
        )
    }

    fn reasons(value: &Value) -> Vec<String> {
        value["issues"]
            .as_array()
            .unwrap()
            .iter()
            .map(|issue| issue["reason"].as_str().unwrap().to_string())
            .collect()
    }

    struct Wheel {
        _root: tempfile::TempDir,
        env: PathBuf,
        site: PathBuf,
        dist: PathBuf,
    }

    impl Wheel {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let env = root.path().join("uv").join("pkg");
            let site = env.join("Lib").join("site-packages");
            let dist = site.join("pkg-1.0.dist-info");
            fs::create_dir_all(env.join("Scripts")).unwrap();
            fs::create_dir_all(&dist).unwrap();
            Self {
                _root: root,
                env,
                site,
                dist,
            }
        }

        fn verify(&self, full: bool, dirs: &[&str]) -> Value {
            verify_record(&self.dist, &self.env, full, dirs)
        }
    }

    #[test]
    fn empty_sha256_uses_urlsafe_base64_without_padding() {
        assert_eq!(
            urlsafe_nopad(&Sha256::digest(b"").into()),
            "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU"
        );
    }

    #[test]
    fn parse_csv_keeps_quoted_comma_quote_newline_and_empty_fields() {
        let rows = parse_csv(
            "\"serena/foo,bar.py\",\"sha256=abc\",\"12\"\n\"serena/a\"\"b.py\",,\n\"serena/x\ny.py\",sha256=z,1\n",
        )
        .unwrap();
        assert_eq!(rows[0], ["serena/foo,bar.py", "sha256=abc", "12"]);
        assert_eq!(rows[1], ["serena/a\"b.py", "", ""]);
        assert_eq!(rows[2], ["serena/x\ny.py", "sha256=z", "1"]);
        assert_eq!(
            parse_csv("good,row,here\n\"\"").unwrap(),
            vec![
                vec![
                    String::from("good"),
                    String::from("row"),
                    String::from("here"),
                ],
                vec![String::new()],
            ]
        );
        assert_eq!(
            parse_csv("serena/cli.py,,10\n").unwrap(),
            [["serena/cli.py", "", "10"]]
        );
        assert!(parse_csv("serena/\"cli\".py,sha256=abc,1").is_err());
        assert!(parse_csv("\"serena/cli.py\"x,sha256=abc,1").is_err());
        assert!(parse_csv("\"unclosed,sha256=abc,1\n").is_err());
    }

    #[test]
    fn missing_record_is_unknown_without_issues() {
        let wheel = Wheel::new();
        let report = wheel.verify(false, &["serena"]);
        assert_eq!(report["state"], "unknown");
        assert_eq!(report["checked_files"], 0);
        assert!(report["issues"].as_array().unwrap().is_empty());
        assert!(report["basis"].as_str().unwrap().ends_with("RECORD"));
    }

    #[test]
    fn default_selects_package_python_metadata_and_entry_points() {
        let wheel = Wheel::new();
        let py = b"# package source\n";
        let skipped_py = b"# other package\n";
        let data = b"native-or-data";
        let metadata = b"Name: pkg\nVersion: 1.0\n";
        let entry = b"[console_scripts]\nserena=serena.cli:top_level\n";
        let wheel_file = b"Wheel-Version: 1.0\n";
        write(&wheel.site.join("serena/cli.py"), py);
        write(&wheel.site.join("other/mod.py"), skipped_py);
        write(&wheel.site.join("serena/data.txt"), data);
        write(&wheel.dist.join("METADATA"), metadata);
        write(&wheel.dist.join("entry_points.txt"), entry);
        write(&wheel.dist.join("WHEEL"), wheel_file);
        write(
            &wheel.dist.join("RECORD"),
            format!(
                "{}\n{}\n{}\n{}\n{}\n{}\n",
                row("serena/cli.py", py),
                row("other/mod.py", skipped_py),
                row("serena/data.txt", data),
                row("pkg-1.0.dist-info/METADATA", metadata),
                row("pkg-1.0.dist-info/entry_points.txt", entry),
                row("pkg-1.0.dist-info/WHEEL", wheel_file),
            )
            .as_bytes(),
        );
        let report = wheel.verify(false, &["serena"]);
        assert_eq!(report["state"], "record-matches");
        assert_eq!(report["checked_files"], 3);
        write(&wheel.site.join("serena/data.txt"), b"changed-data");
        write(&wheel.site.join("other/mod.py"), b"changed-other");
        write(&wheel.dist.join("WHEEL"), b"changed-wheel");
        let skipped = wheel.verify(false, &["serena"]);
        assert_eq!(skipped["state"], "record-matches");
        assert_eq!(skipped["checked_files"], 3);
        let full = wheel.verify(true, &["serena"]);
        assert_eq!(full["state"], "modified");
        assert!(reasons(&full).contains(&"record-hash-mismatch".to_string()));
        assert!(full["checked_files"].as_u64().unwrap() >= 3);
    }

    #[test]
    fn empty_hashes_are_skipped_and_leave_unknown_when_nothing_checked() {
        let wheel = Wheel::new();
        write(&wheel.site.join("serena/cli.py"), b"print(1)\n");
        write(
            &wheel.dist.join("RECORD"),
            b"serena/cli.py,,10\npkg-1.0.dist-info/RECORD,,\n",
        );
        let report = wheel.verify(true, &["serena"]);
        assert_eq!(report["state"], "unknown");
        assert_eq!(report["checked_files"], 0);
        assert!(report["issues"].as_array().unwrap().is_empty());
    }

    #[test]
    fn mutation_and_missing_files_are_modified() {
        let wheel = Wheel::new();
        let py = b"original\n";
        write(&wheel.site.join("serena/cli.py"), py);
        write(
            &wheel.dist.join("RECORD"),
            format!(
                "{}\n{}\n",
                row("serena/cli.py", py),
                row("serena/gone.py", b"missing")
            )
            .as_bytes(),
        );
        let missing = wheel.verify(false, &["serena"]);
        assert_eq!(missing["state"], "modified");
        assert!(reasons(&missing).contains(&"missing-recorded-file".to_string()));
        write(&wheel.site.join("serena/cli.py"), b"mutated\n");
        write(
            &wheel.dist.join("RECORD"),
            format!("{}\n", row("serena/cli.py", py)).as_bytes(),
        );
        let mutated = wheel.verify(false, &["serena"]);
        assert_eq!(mutated["state"], "modified");
        assert_eq!(reasons(&mutated), ["record-hash-mismatch"]);
        assert_eq!(mutated["checked_files"], 1);
    }

    #[test]
    fn quoted_comma_path_is_hashed() {
        let wheel = Wheel::new();
        let py = b"quoted = True\n";
        write(&wheel.site.join("serena").join("foo,bar.py"), py);
        write(
            &wheel.dist.join("RECORD"),
            format!("{}\n", row("serena/foo,bar.py", py)).as_bytes(),
        );
        let report = wheel.verify(false, &["serena"]);
        assert_eq!(report["state"], "record-matches");
        assert_eq!(report["checked_files"], 1);
    }

    #[test]
    fn empty_quoted_eof_row_is_not_discarded_behind_a_match() {
        let wheel = Wheel::new();
        let py = b"ok\n";
        write(&wheel.site.join("serena/cli.py"), py);
        write(
            &wheel.dist.join("RECORD"),
            format!("{}\n\"\"", row("serena/cli.py", py)).as_bytes(),
        );
        let report = wheel.verify(false, &["serena"]);
        assert_eq!(report["state"], "modified");
        assert!(reasons(&report).contains(&"malformed-record".to_string()));
        assert_eq!(report["checked_files"], 1);
    }

    #[test]
    fn mid_field_quote_is_a_csv_error_not_a_healthy_match() {
        let wheel = Wheel::new();
        let py = b"ok\n";
        write(&wheel.site.join("serena/cli.py"), py);
        write(
            &wheel.dist.join("RECORD"),
            format!(
                "{}\nserena/\"cli\".py,sha256=abc,1\n",
                row("serena/cli.py", py)
            )
            .as_bytes(),
        );
        let report = wheel.verify(false, &["serena"]);
        assert_eq!(report["state"], "unknown");
        assert!(reasons(&report).contains(&"Error".to_string()));
        assert!(!report.to_string().contains("ok\\n"));
    }

    #[test]
    fn parent_paths_stay_valid_inside_scripts() {
        let wheel = Wheel::new();
        let exe = b"inert-script";
        write(&wheel.env.join("Scripts/tool.exe"), exe);
        write(
            &wheel.dist.join("RECORD"),
            format!("{}\n", row("../../Scripts/tool.exe", exe)).as_bytes(),
        );
        let skipped = wheel.verify(false, &["serena"]);
        assert_eq!(skipped["state"], "unknown");
        let report = wheel.verify(true, &["serena"]);
        assert_eq!(report["state"], "record-matches");
        assert_eq!(report["checked_files"], 1);
    }

    #[test]
    fn relative_escape_and_symlink_escape_are_outside() {
        let wheel = Wheel::new();
        let secret = wheel._root.path().join("outside/secret.py");
        write(&secret, b"secret-body");
        write(
            &wheel.dist.join("RECORD"),
            b"../../../../../outside-secret.py,sha256=ignored,2\n",
        );
        let relative = wheel.verify(true, &["serena"]);
        assert_eq!(relative["state"], "modified");
        assert!(reasons(&relative).contains(&"record-path-outside-installation".to_string()));
        assert!(!relative.to_string().contains("secret-body"));

        let linked = wheel.site.join("serena/linked.py");
        fs::create_dir_all(linked.parent().unwrap()).unwrap();
        symlink_file(&secret, &linked).unwrap();
        write(
            &wheel.dist.join("RECORD"),
            format!("{}\n", row("serena/linked.py", b"secret-body")).as_bytes(),
        );
        let linked_report = wheel.verify(false, &["serena"]);
        assert_eq!(linked_report["state"], "modified");
        assert_eq!(
            reasons(&linked_report),
            ["record-path-outside-installation"]
        );
        assert!(!linked_report.to_string().contains("secret-body"));

        let outside_pkg = wheel._root.path().join("outside-pkg");
        write(&outside_pkg.join("serena/__init__.py"), b"print(0)\n");
        let alias = wheel.site.join("escaped");
        symlink_dir(&outside_pkg, &alias).unwrap();
        write(
            &wheel.dist.join("RECORD"),
            format!("{}\n", row("escaped/serena/__init__.py", b"print(0)\n")).as_bytes(),
        );
        let dir_report = wheel.verify(true, &["escaped"]);
        assert_eq!(dir_report["state"], "modified");
        assert!(reasons(&dir_report).contains(&"record-path-outside-installation".to_string()));
    }

    #[test]
    fn resolved_record_inside_install_is_the_opened_file() {
        let wheel = Wheel::new();
        let py = b"ok\n";
        write(&wheel.site.join("serena/cli.py"), py);
        let real = wheel.dist.join("RECORD.real");
        write(&real, format!("{}\n", row("serena/cli.py", py)).as_bytes());
        symlink_file(&real, wheel.dist.join("RECORD")).unwrap();
        let report = wheel.verify(false, &["serena"]);
        assert_eq!(report["state"], "record-matches");
        assert_eq!(report["checked_files"], 1);
    }

    #[test]
    fn malformed_unsupported_and_legacy_exception_states() {
        let wheel = Wheel::new();
        let py = b"ok\n";
        write(&wheel.site.join("serena/cli.py"), py);
        write(&wheel.dist.join("RECORD"), b"only-one-column\n");
        let malformed = wheel.verify(true, &["serena"]);
        assert_eq!(malformed["state"], "modified");
        assert_eq!(reasons(&malformed), ["malformed-record"]);

        write(
            &wheel.dist.join("RECORD"),
            format!("serena/cli.py,md5=abcd,3\n{}\n", row("serena/cli.py", py)).as_bytes(),
        );
        let algo = wheel.verify(false, &["serena"]);
        assert_eq!(algo["state"], "modified");
        assert!(reasons(&algo).contains(&"unsupported-record-hash".to_string()));

        write(
            &wheel.dist.join("RECORD"),
            format!("broken\n{},extra,fields,here\n", row("serena/cli.py", py)).as_bytes(),
        );
        let extra = wheel.verify(false, &["serena"]);
        assert_eq!(extra["state"], "unknown");
        let extra_reasons = reasons(&extra);
        assert!(extra_reasons.contains(&"malformed-record".to_string()));
        assert!(extra_reasons.contains(&"ValueError".to_string()));

        write(&wheel.dist.join("RECORD"), b"serena/cli.py,sha256only,1\n");
        let missing_eq = wheel.verify(false, &["serena"]);
        assert_eq!(missing_eq["state"], "unknown");
        assert!(reasons(&missing_eq).contains(&"ValueError".to_string()));
    }

    #[test]
    fn bounds_and_parse_failures_stay_unknown_without_leaking() {
        let wheel = Wheel::new();
        write(
            &wheel.dist.join("RECORD"),
            &vec![b'x'; (RECORD_LIMIT as usize) + 1],
        );
        let huge = wheel.verify(true, &["serena"]);
        assert_eq!(huge["state"], "unknown");
        assert!(!huge["issues"].as_array().unwrap().is_empty());

        let too_many = (0..=ROW_LIMIT)
            .map(|_| "serena/cli.py,,0")
            .collect::<Vec<_>>()
            .join("\n");
        write(&wheel.dist.join("RECORD"), too_many.as_bytes());
        let rows = wheel.verify(true, &["serena"]);
        assert_eq!(rows["state"], "unknown");
        assert!(reasons(&rows).contains(&"ValueError".to_string()));

        write(&wheel.dist.join("RECORD"), b"\"unclosed,sha256=abc,1\n");
        let csv = wheel.verify(true, &["serena"]);
        assert_eq!(csv["state"], "unknown");
        assert!(reasons(&csv).contains(&"Error".to_string()));

        write(&wheel.dist.join("RECORD"), b"\xff\xfe raw-record-body\n");
        let utf8 = wheel.verify(true, &["serena"]);
        assert_eq!(utf8["state"], "unknown");
        assert!(reasons(&utf8).contains(&"UnicodeDecodeError".to_string()));
        assert!(!utf8.to_string().contains("raw-record-body"));

        let oversized = format!("{},sha256=ab,1\n", "a".repeat(FIELD_LIMIT + 1));
        write(&wheel.dist.join("RECORD"), oversized.as_bytes());
        let field = wheel.verify(true, &["serena"]);
        assert_eq!(field["state"], "unknown");
        assert!(reasons(&field).contains(&"ValueError".to_string()));

        let py = wheel.site.join("serena/big.py");
        write(&py, b"print(0)\n");
        let file = OpenOptions::new().write(true).open(&py).unwrap();
        file.set_len(FILE_LIMIT + 1).unwrap();
        drop(file);
        write(
            &wheel.dist.join("RECORD"),
            b"serena/big.py,sha256=47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU,1\n",
        );
        let file_bound = wheel.verify(false, &["serena"]);
        assert_eq!(file_bound["state"], "unknown");
        assert!(reasons(&file_bound).contains(&"ValueError".to_string()));

        let mut hashed = AGGREGATE_LIMIT;
        write(&wheel.site.join("serena/tiny.py"), b"x");
        assert_eq!(
            hash_file(&wheel.site.join("serena/tiny.py"), &mut hashed).unwrap_err(),
            "ValueError"
        );
    }
}
