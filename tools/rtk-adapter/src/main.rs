//! Explicit native-command boundary; the hook never guesses a caller's shell.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const RAW_LIMIT: usize = 4 * 1024 * 1024;
const KEEP_FILES: usize = 32;
const PACK_KEEP_FILES: usize = 64;
const PACK_KEEP_BYTES: u64 = 128 * 1024 * 1024;
const PACK_SCHEMA: u64 = 1;
const RECALL_DEFAULT_LIMIT: usize = 200;
const RECALL_MAX_LIMIT: usize = 2000;
const RECALL_MAX_BYTES: usize = 256 * 1024;
/// Reserve for the truncation marker and next-window hint, so the header plus
/// the windowed content plus those lines stay inside the emitted-byte bound.
const RECALL_METADATA_BYTES: usize = 1024;
const RECALL_USAGE: &str = "usage: harness-rtk recall <handle> [--offset N] [--limit M]";
static HANDLE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn filter_for(args: &[String]) -> Option<&'static str> {
    let name = Path::new(args.first()?)
        .file_stem()?
        .to_str()?
        .to_ascii_lowercase();
    let mut rest = &args[1..];
    if name == "git" {
        if rest.first().map(String::as_str) == Some("-C") {
            if rest.len() < 3 {
                return None;
            }
            rest = &rest[2..];
        }
        match rest.first()?.as_str() {
            "status" if rest.len() == 2 && matches!(rest[1].as_str(), "--short" | "-s") => {
                Some("git-status")
            }
            "log" => {
                let mut i = 1;
                while i < rest.len() {
                    let arg = &rest[i];
                    if matches!(arg.as_str(), "-n" | "--max-count") {
                        i += 1;
                        if rest.get(i)?.parse::<u32>().is_err() {
                            return None;
                        }
                    } else if arg
                        .strip_prefix("-n")
                        .or_else(|| arg.strip_prefix("--max-count="))
                        .is_some_and(|n| n.parse::<u32>().is_ok())
                        || arg == "HEAD"
                        || arg == "--all"
                    {
                    } else {
                        return None;
                    }
                    i += 1;
                }
                Some("git-log")
            }
            _ => None,
        }
    } else if name == "rg" {
        if !rest.iter().any(|a| a == "-n" || a == "--line-number") {
            return None;
        }
        if rest.iter().any(|a| {
            a.starts_with('-')
                && !matches!(
                    a.as_str(),
                    "-n" | "--line-number"
                        | "-H"
                        | "--with-filename"
                        | "-i"
                        | "--ignore-case"
                        | "-F"
                        | "--fixed-strings"
                        | "-S"
                        | "--smart-case"
                        | "--"
                )
        }) {
            return None;
        }
        Some("grep")
    } else {
        let pytest = if name == "pytest" {
            true
        } else if (matches!(name.as_str(), "python" | "python3")
            && rest.starts_with(&["-m".into(), "pytest".into()]))
            || (name == "uv" && rest.starts_with(&["run".into(), "pytest".into()]))
        {
            rest = &rest[2..];
            true
        } else {
            false
        };
        if pytest
            && rest.iter().all(|a| {
                !a.starts_with('-')
                    || matches!(
                        a.as_str(),
                        "-q" | "-v"
                            | "-vv"
                            | "-x"
                            | "--disable-warnings"
                            | "--maxfail=1"
                            | "--tb=short"
                            | "--tb=long"
                            | "--tb=auto"
                    )
            })
        {
            Some("pytest")
        } else if name == "cargo"
            && rest.first().map(String::as_str) == Some("test")
            && rest[1..].iter().all(|a| {
                !a.starts_with('-')
                    || matches!(
                        a.as_str(),
                        "-q" | "--quiet"
                            | "--lib"
                            | "--tests"
                            | "--bins"
                            | "--workspace"
                            | "--all-targets"
                    )
            })
        {
            Some("cargo-test")
        } else {
            None
        }
    }
}

fn hook() -> io::Result<()> {
    if env::var("HARNESS_RTK_DISABLE").as_deref() == Ok("1") {
        return Ok(());
    }
    let mut input = Vec::new();
    io::stdin().take(1_048_577).read_to_end(&mut input)?;
    if input.len() > 1_048_576 {
        return Ok(());
    }
    let Ok(value) = serde_json::from_slice::<Value>(&input) else {
        return Ok(());
    };
    if value["hook_event_name"] != "PreToolUse" || value["tool_name"] != "Bash" {
        return Ok(());
    }
    let Some(command) = value["tool_input"]["command"].as_str() else {
        return Ok(());
    };
    // Codex canonical input omits shell/tty. Replace only our explicit native
    // entry mode, preserving the entire shell-parsed argument tail verbatim.
    let Some(tail) = command.strip_prefix("harness-rtk.exe exec ") else {
        return Ok(());
    };
    if tail.is_empty() || tail.chars().any(|c| "$`;|&<>#\r\n\0".contains(c)) {
        return Ok(());
    }
    let executable = env::current_exe()?;
    if !executable.with_file_name("rtk.exe").is_file() {
        eprintln!("rtk: dependency unavailable; original command unchanged");
        return Ok(());
    }
    let rewritten = format!("harness-rtk.exe compact {tail}");
    let mut updated = value["tool_input"].clone();
    updated["command"] = rewritten.into();
    let result = json!({"hookSpecificOutput":{"hookEventName":"PreToolUse", "permissionDecision":"allow", "updatedInput":updated}});
    io::stdout().write_all(result.to_string().as_bytes())
}

fn codex_home() -> io::Result<PathBuf> {
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".codex")))
        .ok_or_else(|| io::Error::other("Codex home unavailable"))?;
    Ok(home)
}

fn raw_directory() -> io::Result<PathBuf> {
    let root = codex_home()?.join("harness/rtk/raw");
    fs::create_dir_all(&root)?;
    Ok(root)
}

/// Packed observations keep their own bounded area beside `raw/`, because the
/// 32-file raw keep window must keep serving the `[rtk raw: <path>]` contract.
fn pack_directory() -> io::Result<PathBuf> {
    let root = codex_home()?.join("harness/rtk/pack");
    fs::create_dir_all(&root)?;
    Ok(root)
}

fn save_raw(raw: &[u8]) -> io::Result<PathBuf> {
    let root = raw_directory()?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let path = root.join(format!("rtk-{stamp}-{}.log", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)?;
    file.write_all(raw)?;
    let mut files: Vec<_> = fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter(|f| {
            f.file_name().to_string_lossy().starts_with("rtk-")
                && f.path().extension().is_some_and(|e| e == "log")
        })
        .map(|f| f.path())
        .collect();
    files.sort();
    for old in files.iter().take(files.len().saturating_sub(KEEP_FILES)) {
        let _ = fs::remove_file(old); // Files in our private raw namespace; never recurse.
    }
    Ok(path)
}

/// One index record per packed observation. `created` orders retention and the
/// handle embeds the same moment, so eviction stays oldest-first.
#[derive(Clone)]
struct PackEntry {
    handle: String,
    source: String,
    digest: String,
    bytes: u64,
    lines: u64,
    created: u64,
}

impl PackEntry {
    fn to_value(&self) -> Value {
        json!({
            "handle": self.handle,
            "source": self.source,
            "digest": self.digest,
            "bytes": self.bytes,
            "lines": self.lines,
            "created": self.created,
        })
    }

    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            handle: value.get("handle")?.as_str()?.to_owned(),
            source: value.get("source")?.as_str()?.to_owned(),
            digest: value.get("digest")?.as_str()?.to_owned(),
            bytes: value.get("bytes")?.as_u64()?,
            lines: value.get("lines")?.as_u64()?,
            created: value.get("created")?.as_u64()?,
        })
    }
}

fn unix_nanos() -> io::Result<u128> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .map_err(io::Error::other)
}

fn digest_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn short_digest(digest: &str) -> &str {
    digest.get(..12).unwrap_or(digest)
}

/// 1-based line coordinates; a trailing newline does not open an empty last line.
fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

fn digits(value: usize) -> usize {
    value.ilog10() as usize + 1
}

fn read_pack_index(pack: &Path) -> io::Result<Vec<PackEntry>> {
    let index = pack.join("index.json");
    let bytes = match fs::read(&index) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::other(format!("pack index is unreadable: {error}")))?;
    let entries = value
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("pack index has no entry list"))?;
    // A damaged record is dropped instead of guessed: its handle can then only
    // be reported as unknown, never served from unverified metadata.
    Ok(entries.iter().filter_map(PackEntry::from_value).collect())
}

fn write_pack_index(pack: &Path, entries: &[PackEntry]) -> io::Result<()> {
    let temporary = pack.join("index.json.tmp");
    let value = json!({
        "schema": PACK_SCHEMA,
        "entries": entries.iter().map(PackEntry::to_value).collect::<Vec<_>>(),
    });
    fs::write(
        &temporary,
        serde_json::to_vec(&value).map_err(io::Error::other)?,
    )?;
    // Replacement rename: a torn write can never be read as the live index.
    fs::rename(temporary, pack.join("index.json"))
}

/// One `<handle>.log` per observation. Creation nanoseconds plus a process
/// sequence and `create_new` keep concurrent sessions from colliding on a name.
fn write_pack_file(pack: &Path, raw: &[u8]) -> io::Result<String> {
    for _ in 0..64 {
        let handle = format!(
            "ob-{:019}-{}-{:06}",
            unix_nanos()?,
            std::process::id(),
            HANDLE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let path = pack.join(format!("{handle}.log"));
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(raw) {
                    let _ = fs::remove_file(&path);
                    return Err(error);
                }
                return Ok(handle);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("observation handle allocation failed"))
}

/// Oldest-first eviction at 64 entries or 128 MiB, plus cleanup of pack files
/// whose index record is gone (an interrupted mint).
fn retain(pack: &Path, entries: &mut Vec<PackEntry>) -> io::Result<()> {
    for file in fs::read_dir(pack)?.filter_map(Result::ok) {
        let name = file.file_name();
        let name = name.to_string_lossy();
        let Some(handle) = name.strip_suffix(".log") else {
            continue;
        };
        if !entries.iter().any(|entry| entry.handle == handle) {
            let _ = fs::remove_file(file.path()); // Private pack namespace; never recurse.
        }
    }
    entries
        .sort_by(|left, right| (left.created, &left.handle).cmp(&(right.created, &right.handle)));
    let mut total: u64 = entries.iter().map(|entry| entry.bytes).sum();
    while entries.len() > PACK_KEEP_FILES || total > PACK_KEEP_BYTES {
        let oldest = entries.remove(0);
        total = total.saturating_sub(oldest.bytes);
        let _ = fs::remove_file(pack.join(format!("{}.log", oldest.handle)));
    }
    Ok(())
}

fn pack_store(raw: &[u8], source: &str) -> io::Result<PackEntry> {
    let pack = pack_directory()?;
    let handle = write_pack_file(&pack, raw)?;
    let entry = PackEntry {
        handle,
        source: source.to_owned(),
        digest: digest_hex(raw),
        bytes: raw.len() as u64,
        lines: split_lines(&String::from_utf8_lossy(raw)).len() as u64,
        created: (unix_nanos()? / 1_000_000_000) as u64,
    };
    let mut entries = read_pack_index(&pack)?;
    entries.push(entry.clone());
    retain(&pack, &mut entries)?;
    write_pack_index(&pack, &entries)?;
    Ok(entry)
}

/// Handle footer for the compressed path. `None` on any pack failure, so the
/// run keeps today's exact output and packing can never break compression.
fn pack_footer(raw: &[u8], source: &str) -> Option<String> {
    match pack_store(raw, source) {
        Ok(entry) => Some(format!(
            "[rtk pack: {handle} sha256:{short} | harness-rtk.exe recall {handle} --offset 1 --limit {RECALL_DEFAULT_LIMIT}]\n",
            handle = entry.handle,
            short = short_digest(&entry.digest),
        )),
        Err(error) => {
            eprintln!("rtk: {error}; observation handle not issued");
            None
        }
    }
}

fn compressed(raw: &[u8], filter: &str) -> io::Result<Vec<u8>> {
    let executable = env::current_exe()?.with_file_name("rtk.exe");
    let mut command = Command::new(executable);
    command
        .args(["pipe", "--filter", filter])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let data = raw.to_vec();
    let writer = thread::spawn(move || stdin.write_all(&data));
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take((RAW_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let errors = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(65536).read_to_end(&mut bytes).map(|_| bytes)
    });
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(2) {
            timed_out = true;
            let _ = child.kill();
            break child.wait()?;
        }
        thread::sleep(Duration::from_millis(2));
    };
    let _ = writer.join();
    let output = reader
        .join()
        .map_err(|_| io::Error::other("filter reader failed"))??;
    let error = errors
        .join()
        .map_err(|_| io::Error::other("filter stderr reader failed"))??;
    if timed_out
        || !status.success()
        || stderr_reports_diagnostics(&error)
        || output.len() > RAW_LIMIT
    {
        return Err(io::Error::other(if timed_out {
            "filter timed out"
        } else {
            "filter failed or returned diagnostics"
        }));
    }
    Ok(output)
}

/// The pinned `rtk.exe` prints a startup notice to stderr when its own global
/// hook is absent - the normal kit configuration, because the kit owns the
/// Codex hook instead of running `rtk init -g`. That one notice is not a filter
/// diagnostic: only lines carrying it are ignored, so every other stderr byte
/// keeps the compression path fail-closed.
fn stderr_reports_diagnostics(error: &[u8]) -> bool {
    String::from_utf8_lossy(error)
        .lines()
        .map(str::trim)
        .any(|line| {
            !line.is_empty() && !(line.starts_with("[rtk]") && line.contains("No hook installed"))
        })
}

fn filter(mut input: impl Read, name: &str, source: Option<&str>) -> io::Result<()> {
    let mut raw = Vec::new();
    (&mut input)
        .take((RAW_LIMIT + 1) as u64)
        .read_to_end(&mut raw)?;
    let mut output = io::stdout().lock();
    if raw.len() > RAW_LIMIT {
        eprintln!("rtk: stdout exceeds 4 MiB; raw passthrough without retention");
        output.write_all(&raw)?;
        io::copy(&mut input, &mut output)?;
        return Ok(());
    }
    if raw.len() < 500 || std::str::from_utf8(&raw).is_err() || raw.contains(&0) {
        return output.write_all(&raw);
    }
    let path = match save_raw(&raw) {
        Ok(path) => path,
        Err(_) => {
            eprintln!("rtk: raw capture unavailable; raw passthrough");
            return output.write_all(&raw);
        }
    };
    match compressed(&raw, name) {
        Ok(mut result) => {
            let mut footer = format!("\n[rtk raw: {}]\n", path.display());
            // A handle is minted only here: the compressed path where
            // `save_raw` already succeeded. The raw locator stays unchanged.
            if let Some(source) = source
                && let Some(packed) = pack_footer(&raw, source)
            {
                footer.push_str(&packed);
            }
            result.extend_from_slice(footer.as_bytes());
            if result.len() < raw.len() {
                return output.write_all(&result);
            }
        }
        Err(error) => eprintln!("rtk: {error}; raw passthrough"),
    }
    output.write_all(&raw)
}

fn usage_error(message: &str) -> io::Result<i32> {
    eprintln!("rtk: {message}; {RECALL_USAGE}");
    Ok(1)
}

fn unknown_handle(handle: &str) -> io::Result<i32> {
    eprintln!(
        "rtk: unknown observation handle {handle}; packed observations are evicted oldest-first beyond {PACK_KEEP_FILES} records or {} MiB, so rerun the command or read the `[rtk raw: <path>]` locator of its original footer",
        PACK_KEEP_BYTES / (1024 * 1024)
    );
    Ok(2)
}

/// Exact, bounded, digest-verified slice of a packed observation. This never
/// reruns the source command, starts a model or opens the network.
fn recall(arguments: &[std::ffi::OsString]) -> io::Result<i32> {
    let Some(text) = arguments
        .iter()
        .map(|value| value.to_str())
        .collect::<Option<Vec<_>>>()
    else {
        return usage_error("arguments must be valid UTF-8");
    };
    let mut handle: Option<&str> = None;
    let mut offset: usize = 1;
    let mut limit: usize = RECALL_DEFAULT_LIMIT;
    let mut index = 0;
    while index < text.len() {
        match text[index] {
            flag @ ("--offset" | "--limit") => {
                let value = text
                    .get(index + 1)
                    .and_then(|value| value.parse::<usize>().ok())
                    .filter(|value| *value >= 1);
                let Some(value) = value else {
                    return usage_error(&format!("{flag} requires a positive integer"));
                };
                index += 2;
                if flag == "--offset" {
                    offset = value;
                } else {
                    limit = value.min(RECALL_MAX_LIMIT);
                }
            }
            option if option.starts_with('-') => {
                return usage_error(&format!("unknown option {option}"));
            }
            value => {
                if handle.replace(value).is_some() {
                    return usage_error("exactly one observation handle is required");
                }
                index += 1;
            }
        }
    }
    let Some(handle) = handle else {
        return usage_error("an observation handle is required");
    };
    let pack = pack_directory()?;
    let entries = match read_pack_index(&pack) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!(
                "rtk: {error}; rerun the command or read the `[rtk raw: <path>]` locator of its original footer"
            );
            return Ok(1);
        }
    };
    let Some(entry) = entries.into_iter().find(|entry| entry.handle == handle) else {
        return unknown_handle(handle);
    };
    let bytes = match fs::read(pack.join(format!("{}.log", entry.handle))) {
        Ok(bytes) => bytes,
        // An index record without its content is an evicted observation.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return unknown_handle(handle),
        Err(error) => return Err(error),
    };
    if digest_hex(&bytes) != entry.digest {
        eprintln!(
            "rtk: observation {handle} failed its digest check, so no content is returned; rerun the command or read the `[rtk raw: <path>]` locator of its original footer"
        );
        return Ok(3);
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines = split_lines(&text);
    let total = lines.len();
    let mut response = String::new();
    response.push_str(&format!("[rtk pack: {}]\n", entry.handle));
    response.push_str(&format!("source: {}\n", entry.source));
    response.push_str(&format!(
        "digest: sha256 {} verified\n",
        short_digest(&entry.digest)
    ));
    response.push_str(&format!("stored: {total} lines, {} bytes\n", bytes.len()));
    let first = offset - 1;
    if first >= total {
        response.push_str(&format!(
            "window: none; offset {offset} is past the last stored line ({total})\n"
        ));
        response.push_str("next: end of observation\n");
    } else {
        let requested = (first + limit).min(total);
        let budget = RECALL_MAX_BYTES.saturating_sub(RECALL_METADATA_BYTES + response.len());
        let mut served = 0;
        let mut emitted = 0;
        for (position, line) in lines[first..requested].iter().enumerate() {
            // Emitted content carries its own line coordinate and newline.
            let cost = digits(first + position + 1) + 2 + line.len() + 1;
            if served + cost > budget {
                break;
            }
            served += cost;
            emitted += 1;
        }
        if emitted == 0 {
            response.push_str(&format!(
                "window: none; the next stored line alone exceeds the {RECALL_MAX_BYTES} byte window limit\n"
            ));
            response.push_str(
                "next: read the whole observation through its `[rtk raw: <path>]` locator\n",
            );
        } else {
            response.push_str(&format!(
                "window: lines {}-{} of {total}\n",
                offset,
                first + emitted
            ));
            for (position, line) in lines[first..first + emitted].iter().enumerate() {
                response.push_str(&format!("{}: {line}\n", first + position + 1));
            }
            if emitted < requested - first {
                response.push_str(&format!(
                    "[rtk pack: window truncated at {RECALL_MAX_BYTES} bytes]\n"
                ));
            }
            if first + emitted < total {
                response.push_str(&format!(
                    "next: harness-rtk.exe recall {handle} --offset {} --limit {limit}\n",
                    first + emitted + 1
                ));
            } else {
                response.push_str("next: end of observation\n");
            }
        }
    }
    io::stdout().lock().write_all(response.as_bytes())?;
    Ok(0)
}

fn run(arguments: &[std::ffi::OsString], compact: bool) -> io::Result<i32> {
    let executable = arguments
        .first()
        .ok_or_else(|| io::Error::other("native executable required"))?;
    let text_arguments: Option<Vec<String>> = arguments
        .iter()
        .map(|a| a.to_str().map(str::to_owned))
        .collect();
    let selected: Option<(&'static str, String)> = if compact
        && !io::stdout().is_terminal()
        && env::var("HARNESS_RTK_DISABLE").as_deref() != Ok("1")
    {
        text_arguments
            .as_deref()
            .and_then(|args| filter_for(args).map(|filter| (filter, args.join(" "))))
    } else {
        None
    };
    let mut command = Command::new(executable);
    command
        .args(&arguments[1..])
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit());
    if selected.is_some() {
        command.stdout(Stdio::piped());
    } else {
        command.stdout(Stdio::inherit());
    }
    let mut child = command.spawn()?;
    if let Some((name, source)) = &selected
        && let Err(error) = filter(child.stdout.take().unwrap(), name, Some(source))
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(child.wait()?.code().unwrap_or(1))
}

fn main() {
    let arguments: Vec<_> = env::args_os().collect();
    let result = match arguments.get(1).and_then(|a| a.to_str()) {
        Some("hook") => hook().map(|_| 0),
        Some("filter") if arguments.len() == 3 => {
            // No source command is known here, so this entry keeps the raw
            // locator only and never mints an observation handle.
            filter(io::stdin().lock(), &arguments[2].to_string_lossy(), None).map(|_| 0)
        }
        Some("recall") => recall(&arguments[2..]),
        Some("exec") => run(&arguments[2..], false),
        Some("compact") => run(&arguments[2..], true),
        _ => Err(io::Error::other(
            "usage: harness-rtk exec|compact EXECUTABLE ARGS... | hook | filter FILTER | recall HANDLE [--offset N] [--limit M]",
        )),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("rtk: {error}");
            std::process::exit(1);
        }
    }
}
