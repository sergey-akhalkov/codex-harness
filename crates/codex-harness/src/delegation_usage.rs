//! Explicit native rollout accounting. Never decodes reasoning/compaction state,
//! discovers descendants, selects a model, or converts tokens into billing.
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[path = "delegation_usage_markdown.rs"]
mod markdown;
#[path = "delegation_usage_rollout.rs"]
mod rollout;

const TOKEN_FIELDS: [&str; 5] = [
    "input_tokens",
    "cached_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
    "total_tokens",
];
type Usage = BTreeMap<String, Option<u64>>;

fn identifier(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    (text.len() <= 128
        && text
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_./:-".contains(&b)))
    .then(|| text.to_owned())
}

fn usage(raw: &Value) -> Usage {
    TOKEN_FIELDS
        .iter()
        .map(|key| ((*key).to_owned(), raw.get(key).and_then(Value::as_u64)))
        .collect()
}

fn unknown_usage() -> Usage {
    usage(&Value::Null)
}
fn list(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}
fn only(values: &BTreeSet<String>) -> Option<String> {
    (values.len() == 1).then(|| values.first().unwrap().clone())
}

fn timestamp(value: &Value) -> Option<DateTime<Utc>> {
    let text = value.as_str()?;
    if text.len() > 128 {
        return None;
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return Some(parsed.with_timezone(&Utc));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y%m%dT%H%M%S%.f",
    ] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(text, format) {
            return Some(parsed.and_utc());
        }
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)
        .map(|v| v.and_utc())
}

fn message_text(payload: &Value) -> String {
    if let Some(text) = payload["content"].as_str() {
        return text.to_owned();
    }
    let mut result = String::new();
    for item in list(&payload["content"]) {
        for key in ["text", "input_text"] {
            if let Some(text) = item[key].as_str() {
                result.push_str(text);
            }
        }
    }
    result
}

fn prefix(text: &str, count: usize) -> String {
    text.trim_start()
        .chars()
        .take(count)
        .collect::<String>()
        .to_lowercase()
}

fn continuations(triggers: &[Value], turns: &[Value], users: &[Value]) -> usize {
    let triggers: BTreeSet<_> = triggers.iter().filter_map(timestamp).collect();
    let turns: BTreeSet<_> = turns.iter().filter_map(timestamp).collect();
    let users: BTreeSet<_> = users.iter().filter_map(timestamp).collect();
    let mut claimed = BTreeSet::new();
    for trigger in triggers {
        let Some(turn) = turns
            .range((
                std::ops::Bound::Excluded(trigger),
                std::ops::Bound::Unbounded,
            ))
            .next()
        else {
            continue;
        };
        let user = users
            .range((
                std::ops::Bound::Excluded(trigger),
                std::ops::Bound::Unbounded,
            ))
            .next();
        if user.is_none_or(|user| user >= turn) {
            claimed.insert(*turn);
        }
    }
    claimed.len()
}

fn project_label(value: &str) -> Option<String> {
    if value.trim().is_empty() {
        return None;
    }
    Some(
        Path::new(value)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("workspace")
            .to_owned(),
    )
}

fn resolved(path: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == io::ErrorKind::NotFound => std::path::absolute(path),
        Err(error) => Err(error),
    }
}

fn total_rows(rows: &[Value], partial: bool) -> (Value, bool) {
    let mut result = Map::new();
    let mut overflow = false;
    for key in TOKEN_FIELDS {
        let values: Vec<_> = rows.iter().filter_map(|row| row[key].as_u64()).collect();
        let total = values.iter().try_fold(0u64, |sum, v| sum.checked_add(*v));
        overflow |= total.is_none();
        result.insert(
            key.to_owned(),
            if values.is_empty() && !rows.is_empty() {
                Value::Null
            } else {
                json!(total)
            },
        );
    }
    result.insert("thread_count".into(), json!(rows.len()));
    result.insert(
        "missing_usage".into(),
        json!(rows.iter().filter(|r| r["missing_usage"] == true).count()),
    );
    result.insert(
        "partial".into(),
        json!(partial || overflow || rows.iter().any(|r| r["partial"] == true)),
    );
    (Value::Object(result), overflow)
}

fn response_totals(threads: &[Value], partial: bool) -> (Value, bool) {
    let mut seen: BTreeMap<String, Usage> = BTreeMap::new();
    let mut conflicting = BTreeSet::new();
    for row in threads {
        conflicting.extend(
            list(&row["response_conflict_ids"])
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned),
        );
        if let Some(values) = row["response_usages"].as_object() {
            for (id, raw) in values {
                let next = usage(raw);
                if seen.get(id).is_some_and(|old| old != &next) {
                    seen.insert(id.clone(), unknown_usage());
                    conflicting.insert(id.clone());
                } else {
                    seen.entry(id.clone()).or_insert(next);
                }
            }
        }
    }
    let rows: Vec<_> = seen
        .values()
        .map(|u| {
            let mut row = json!(u);
            let missing = u.values().any(Option::is_none);
            row["missing_usage"] = missing.into();
            row["partial"] = missing.into();
            row
        })
        .collect();
    let (mut totals, overflow) = total_rows(&rows, partial || !conflicting.is_empty());
    totals["response_count"] = json!(seen.len());
    totals["conflicting_response_ids"] = json!(conflicting.len());
    (totals, overflow)
}

fn merge_duplicate(old: &mut Value, new: &Value) {
    let token_conflict = TOKEN_FIELDS.iter().any(|k| old[k] != new[k]);
    let identity_conflict = ["parent_id", "model", "reasoning", "provider"]
        .iter()
        .any(|k| old[k] != new[k]);
    if token_conflict || identity_conflict {
        for key in TOKEN_FIELDS {
            old[key] = Value::Null;
        }
        old["missing_usage"] = true.into();
    }
    old["partial"] = true.into();
    for key in ["parent_id", "model", "reasoning", "provider"] {
        if old[key] != new[key] {
            old[key] = Value::Null;
        }
    }
    let mut responses = old["response_usages"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut conflicts: BTreeSet<_> = list(&old["response_conflict_ids"])
        .iter()
        .chain(list(&new["response_conflict_ids"]))
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    if let Some(next) = new["response_usages"].as_object() {
        for (id, value) in next {
            if responses.get(id).is_some_and(|old| old != value) {
                responses.insert(id.clone(), json!(unknown_usage()));
                conflicts.insert(id.clone());
            } else {
                responses.entry(id.clone()).or_insert_with(|| value.clone());
            }
        }
    }
    old["response_conflict_ids"] = json!(conflicts);
    old["response_ids"] = json!(responses.keys().collect::<Vec<_>>());
    old["response_count"] = json!(responses.len());
    old["response_usages"] = responses.into();
}

pub(crate) fn summarize(paths: &[PathBuf]) -> (Value, Vec<File>) {
    let mut seen = BTreeSet::new();
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();
    let mut fingerprints = BTreeMap::new();
    let mut threads: Vec<Value> = Vec::new();
    let mut warnings = Vec::new();
    let mut inputs = Vec::new();
    for (ordinal, path) in paths.iter().enumerate() {
        let input = ordinal + 1;
        let path = match resolved(path) {
            Ok(p) => p,
            Err(_) => {
                warnings.push(json!({"code":"invalid_input_path","input":input}));
                continue;
            }
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let (row, fingerprint, problems, held) = rollout::read(&path);
        if let Some(file) = held.or_else(|| metadata_file(&path).ok()) {
            inputs.push(file);
        }
        for code in problems {
            warnings.push(json!({"code":code,"input":input,"thread_id":row["id"]}));
        }
        if let Some(id) = row["id"].as_str() {
            if let Some(&index) = ids.get(id) {
                if fingerprints.get(id) != Some(&fingerprint) {
                    merge_duplicate(&mut threads[index], &row);
                    warnings.push(
                        json!({"code":"conflicting_duplicate_id","input":input,"thread_id":id}),
                    );
                }
                continue;
            }
            ids.insert(id.to_owned(), threads.len());
            fingerprints.insert(id.to_owned(), fingerprint);
        }
        threads.push(row);
    }
    if paths.is_empty() {
        warnings.push(json!({"code":"no_inputs"}));
    }
    let mut missing = Vec::new();
    for row in &threads {
        for child in list(&row["spawned_child_ids"]) {
            if child.as_str().is_some_and(|id| !ids.contains_key(id)) {
                missing.push(json!({"parent_id":row["id"],"child_id":child}));
            }
        }
        if row["spawn_calls"].as_u64().is_some_and(|v| v > 0)
            && list(&row["spawned_child_ids"]).is_empty()
            && row["id"].is_string()
        {
            missing.push(json!({"parent_id":row["id"],"child_id":null}));
        }
    }
    for item in &missing {
        warnings.push(json!({"code":"missing_child","thread_id":item["parent_id"]}));
    }
    let intervals: Vec<_> = threads
        .iter()
        .filter_map(|row| {
            let a = timestamp(&row["first_timestamp"])?;
            let b = timestamp(&row["last_timestamp"])?;
            (a <= b).then_some((a, b))
        })
        .collect();
    let overlap = intervals
        .iter()
        .enumerate()
        .any(|(i, (a, b))| intervals[i + 1..].iter().any(|(c, d)| a <= d && c <= b));
    if overlap {
        warnings.push(json!({"code":"concurrent_or_overlapping_elapsed"}));
    }
    for row in &mut threads {
        if missing.iter().any(|item| item["parent_id"] == row["id"]) {
            row["partial"] = true.into();
        }
    }
    let mut partial = !warnings.is_empty();
    let mut providers = Map::new();
    for provider in ["OpenAI", "xai"] {
        providers.insert(
            provider.into(),
            total_rows(
                &threads
                    .iter()
                    .filter(|row| row["provider"] == provider)
                    .cloned()
                    .collect::<Vec<_>>(),
                partial,
            )
            .0,
        );
    }
    let (mut totals, token_overflow) = total_rows(&threads, partial);
    let (mut unattributed, _) = total_rows(
        &threads
            .iter()
            .filter(|row| row["provider"].is_null())
            .cloned()
            .collect::<Vec<_>>(),
        partial,
    );
    let (mut responses, response_overflow) = response_totals(&threads, partial);
    if token_overflow || response_overflow {
        warnings.push(json!({"code":"counter_total_overflow"}));
        partial = true;
        totals["partial"] = true.into();
        unattributed["partial"] = true.into();
        responses["partial"] = true.into();
        for provider in providers.values_mut() {
            provider["partial"] = true.into();
        }
    }
    (
        json!({"threads":threads,"by_provider":providers,"totals":totals,"unattributed":unattributed,"responses":responses,
        "missing_children":missing,"overlapping_elapsed":overlap,"limitation":"Observed tokens from supplied rollouts; not weekly quota, billing, or an exact saving.","warnings":warnings,"partial":partial}),
        inputs,
    )
}

fn private_sources(paths: &[PathBuf]) -> Vec<Value> {
    let mut records = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let record = || -> io::Result<Value> {
            let path = resolved(path)?;
            let mut file = File::open(&path)?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; 1024 * 1024];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hasher.update(&buffer[..count]);
            }
            let size = file.metadata()?.len();
            let suffix = path.extension().map_or(String::new(), |s| {
                format!(".{}", s.to_string_lossy().to_lowercase())
            });
            Ok(
                json!({"input":index+1,"sha256":format!("{:x}",hasher.finalize()),"bytes":size,"suffix":suffix}),
            )
        };
        if let Ok(record) = record() {
            records.push(record);
        }
    }
    records
}

#[cfg(windows)]
fn identity(file: &File) -> io::Result<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

#[cfg(unix)]
fn identity(file: &File) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let info = file.metadata()?;
    Ok((info.dev(), info.ino()))
}

fn metadata_file(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
        };
        OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

fn same_file(file: &File, path: &Path) -> io::Result<bool> {
    match metadata_file(path) {
        Ok(other) => Ok(identity(file)? == identity(&other)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

fn protect_destinations(paths: &[PathBuf], destinations: &[&PathBuf]) -> io::Result<()> {
    if destinations.is_empty() {
        return Ok(());
    }
    let inputs: Vec<_> = paths
        .iter()
        .filter_map(|path| resolved(path).ok())
        .collect();
    let mut names = Vec::new();
    for destination in destinations {
        let name = resolved(destination)?;
        if inputs.contains(&name) || names.contains(&name) {
            return Err(io::Error::other(
                "report destinations must not overwrite inputs or each other",
            ));
        }
        names.push(name);
        match metadata_file(destination) {
            Ok(file) => {
                for input in paths
                    .iter()
                    .chain(destinations.iter().take(names.len() - 1).copied())
                {
                    if same_file(&file, input)? {
                        return Err(io::Error::other(
                            "report destinations must not overwrite inputs or each other",
                        ));
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn open_output(path: &Path, inputs: &[PathBuf], source_handles: &[File]) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
        options.share_mode(FILE_SHARE_READ);
    }
    // Do not truncate by name. Compare the actual opened output object first;
    // a symlink/hardlink substitution between preflight and open is refused too.
    let file = options.open(path)?;
    let output_id = identity(&file)?;
    for source in source_handles {
        if identity(source)? == output_id {
            return Err(io::Error::other(
                "report output became a captured input rollout",
            ));
        }
    }
    for input in inputs {
        if same_file(&file, input)? {
            return Err(io::Error::other("report output became an input rollout"));
        }
    }
    Ok(file)
}

fn write_output(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.flush()
}

fn command(args: &[OsString]) -> io::Result<i32> {
    if args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help" || arg == "-h")
    {
        println!(
            "codex-harness delegation-usage [ROLLOUT ...] [--output PATH] [--format json|markdown] [--private-sources PATH]\nOnly supplied files are read. Unknown counters remain null. Private source hashes belong outside Git; no models are called."
        );
        return Ok(0);
    }
    let mut expanded = Vec::new();
    let mut after_separator = false;
    for arg in args {
        if arg == "--" {
            after_separator = true;
        }
        if !after_separator
            && let Some((key, value)) = arg.to_str().and_then(|s| s.split_once('='))
            && ["--output", "--format", "--private-sources"].contains(&key)
        {
            expanded.push(OsString::from(key));
            expanded.push(OsString::from(value));
            continue;
        }
        expanded.push(arg.clone());
    }
    let args = expanded.as_slice();
    let mut paths = Vec::new();
    let mut output = None;
    let mut private = None;
    let mut format = "json";
    let mut index = 0;
    let mut positional = false;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if !positional && arg == "--" {
            positional = true;
            continue;
        }
        if !positional
            && ["--output", "--format", "--private-sources"]
                .iter()
                .any(|key| arg == key)
        {
            let value = args
                .get(index)
                .ok_or_else(|| io::Error::other("missing report option value"))?;
            index += 1;
            if arg == "--output" {
                output = Some(PathBuf::from(value));
            } else if arg == "--private-sources" {
                private = Some(PathBuf::from(value));
            } else {
                format = value
                    .to_str()
                    .filter(|s| ["json", "markdown"].contains(s))
                    .ok_or_else(|| io::Error::other("invalid report format"))?;
            }
        } else if !positional && arg.to_string_lossy().starts_with('-') {
            return Err(io::Error::other("invalid report option"));
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    let destinations: Vec<_> = [output.as_ref(), private.as_ref()]
        .into_iter()
        .flatten()
        .collect();
    protect_destinations(&paths, &destinations)?;
    let (mut report, source_handles) = summarize(&paths);
    let source_bytes = if private.is_some() {
        let sources = private_sources(&paths);
        report["sources"] = json!(
            sources
                .iter()
                .map(|item| json!({"input":item["input"],"bytes":item["bytes"]}))
                .collect::<Vec<_>>()
        );
        Some(
            format!(
                "{}\n",
                serde_json::to_string_pretty(&json!({"sources":sources}))?
            )
            .into_bytes(),
        )
    } else {
        None
    };
    let rendered = if format == "markdown" {
        markdown::render(&report)
    } else {
        format!("{}\n", serde_json::to_string_pretty(&report)?)
    };
    // Acquire every output before writing either, preserving original data if
    // the two output paths have become aliases during report preparation.
    protect_destinations(&paths, &destinations)?;
    let mut files = Vec::new();
    for path in &destinations {
        let file = open_output(path, &paths, &source_handles)?;
        let id = identity(&file)?;
        if files.iter().any(|old| identity(old).ok() == Some(id)) {
            return Err(io::Error::other("report outputs became aliases"));
        }
        files.push(file);
    }
    if let Some(bytes) = source_bytes {
        write_output(files.last_mut().unwrap(), &bytes)?;
    }
    if output.is_some() {
        write_output(&mut files[0], rendered.as_bytes())?;
    } else {
        io::stdout().lock().write_all(rendered.as_bytes())?;
    }
    Ok(0)
}

pub(crate) fn run(args: &[OsString]) -> io::Result<i32> {
    match command(args) {
        Ok(code) => Ok(code),
        Err(_) => {
            eprintln!("Unable to read inputs or write the report.");
            Ok(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renamed_input_object_cannot_be_overwritten_as_output() {
        let root = tempfile::Builder::new()
            .prefix("harness-usage-rename-")
            .tempdir()
            .unwrap()
            .keep();
        let input = root.join("input.jsonl");
        let output = root.join("report.json");
        let original = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"owned\"}}\n";
        fs::write(&input, original).unwrap();
        let paths = vec![input.clone()];
        protect_destinations(&paths, &[&output]).unwrap();
        let (_report, source_handles) = summarize(&paths);
        fs::rename(&input, &output).unwrap();
        protect_destinations(&paths, &[&output]).unwrap();
        let result = open_output(&output, &paths, &source_handles)
            .and_then(|mut file| write_output(&mut file, b"candidate report"));
        println!("renamed input evidence: {}", root.display());
        assert!(result.is_err(), "report overwrote the renamed input object");
        assert_eq!(fs::read(&output).unwrap(), original);
    }

    #[test]
    fn counter_overflow_stays_unknown_and_partial() {
        let root = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for id in ["first", "second"] {
            let path = root.path().join(format!("{id}.jsonl"));
            let raw: BTreeMap<_, _> = TOKEN_FIELDS.iter().map(|key| (*key, u64::MAX)).collect();
            let events = [
                json!({"type":"session_meta","payload":{"id":id}}),
                json!({"type":"turn_context","payload":{"model":"gpt-6-astra","effort":"high"}}),
                json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":raw}}}),
                json!({"type":"token_usage_record","payload":{"response_id":id,"usage":raw}}),
            ];
            fs::write(
                &path,
                events
                    .iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .unwrap();
            paths.push(path);
        }
        let (report, _handles) = summarize(&paths);
        assert!(report["totals"]["total_tokens"].is_null());
        assert!(report["responses"]["total_tokens"].is_null());
        assert_eq!(report["partial"], true);
        assert!(
            list(&report["warnings"])
                .iter()
                .any(|v| v["code"] == "counter_total_overflow")
        );
    }

    #[test]
    fn oversized_record_is_skipped_without_losing_later_visible_usage() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("bounded.jsonl");
        let mut file = File::create(&path).unwrap();
        file.write_all(&vec![b'x'; 32 * 1024 * 1024 + 10]).unwrap();
        file.write_all(b"\n").unwrap();
        let raw: BTreeMap<_, _> = TOKEN_FIELDS.iter().map(|key| (*key, 10u64)).collect();
        for event in [
            json!({"type":"session_meta","payload":{"id":"owned"}}),
            json!({"type":"turn_context","payload":{"model":"gpt-6-astra","effort":"high"}}),
            json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":raw}}}),
        ] {
            writeln!(file, "{event}").unwrap();
        }
        drop(file);
        let (report, _handles) = summarize(&[path]);
        assert_eq!(report["totals"]["total_tokens"], 10);
        assert!(
            list(&report["warnings"])
                .iter()
                .any(|v| v["code"] == "oversized_jsonl_record")
        );
    }

    #[test]
    fn timestamps_support_native_offsets_and_naive_utc_without_executing_content() {
        assert_eq!(
            timestamp(&json!("2026-09-09T10:00:00+03:00")),
            timestamp(&json!("2026-09-09T07:00:00Z"))
        );
        assert_eq!(
            timestamp(&json!("2026-09-09 07:00:00")),
            timestamp(&json!("2026-09-09T07:00:00Z"))
        );
        assert!(timestamp(&json!("private-invalid-time")).is_none());
    }
}
