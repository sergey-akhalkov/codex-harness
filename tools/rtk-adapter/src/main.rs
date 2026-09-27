//! Explicit native-command boundary; the hook never guesses a caller's shell.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Per-stream capture bound. Crossing it commits that run to raw passthrough:
/// the buffered prefix is flushed and the remainder streams live.
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
/// Spacing of the bounded progress notice while a compact run holds a long
/// command's output back for presentation. `HARNESS_RTK_PROGRESS_SECONDS`
/// overrides it; zero disables the notice.
const PROGRESS_SECONDS: u64 = 10;
/// Hard bound on progress notices per run, so a very long command stays quiet
/// beyond a fixed count even when the interval is short.
const PROGRESS_NOTICES: usize = 20;
/// Bounded local decision records behind `harness-rtk.exe diagnostics`.
const DIAGNOSTIC_KEEP: usize = 32;
const DIAGNOSTIC_DEFAULT: usize = 8;
const DIAGNOSTIC_USAGE: &str = "usage: harness-rtk.exe diagnostics [--last N] [--json]";
/// Below this size the retained original would cost more than it saves.
const RETENTION_FLOOR: usize = 500;
static HANDLE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The documented Cargo verification verbs. `test` keeps the pinned RTK
/// `cargo-test` filter for the test harness on stdout. Installed RTK 0.48.0
/// exposes no `cargo-check`/`cargo-build`/`cargo-clippy` pipe filter (its
/// filter list carries `cargo-test` only, and its test filter drops diagnostic
/// headers and mislabels a check run), so every other Cargo stream uses the
/// adapter's own bounded presentation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CargoVerb {
    Test,
    Check,
    Build,
    Clippy,
}

impl CargoVerb {
    fn label(self) -> &'static str {
        match self {
            CargoVerb::Test => "test",
            CargoVerb::Check => "check",
            CargoVerb::Build => "build",
            CargoVerb::Clippy => "clippy",
        }
    }
}

/// A selected compression path. `Pipe` keeps the single-stream behavior of the
/// allowlisted git/rg/pytest commands; `Cargo` runs the dual-stream Cargo path.
enum Selection {
    Pipe {
        filter: &'static str,
        command: String,
    },
    Cargo {
        verb: CargoVerb,
        command: String,
    },
}

/// Why a `compact` invocation did not enter a compression path. The reason is
/// recorded locally, so a hook rewrite that changed nothing stays visible
/// instead of looking like applied compression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bypass {
    UnsupportedCommand,
    UnsupportedFlags,
    MachineFormat,
    Disabled,
    Interactive,
}

impl Bypass {
    fn reason(self) -> &'static str {
        match self {
            Bypass::UnsupportedCommand => "unsupported-command",
            Bypass::UnsupportedFlags => "unsupported-flags",
            Bypass::MachineFormat => "machine-format",
            Bypass::Disabled => "disabled",
            Bypass::Interactive => "interactive",
        }
    }
}

fn select(args: &[String]) -> Result<Selection, Bypass> {
    let name = Path::new(args.first().ok_or(Bypass::UnsupportedCommand)?)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or(Bypass::UnsupportedCommand)?;
    let rest = &args[1..];
    let command = args.join(" ");
    if name == "cargo" {
        let verb = cargo_verb(rest)?;
        return Ok(Selection::Cargo { verb, command });
    }
    if name == "git" {
        return git_selection(rest, command);
    }
    if name == "rg" {
        if !rest.iter().any(|a| a == "-n" || a == "--line-number") {
            return Err(Bypass::UnsupportedFlags);
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
            return Err(Bypass::UnsupportedFlags);
        }
        return Ok(Selection::Pipe {
            filter: "grep",
            command,
        });
    }
    let mut pytest_args = rest;
    let pytest = if name == "pytest" {
        true
    } else if (matches!(name.as_str(), "python" | "python3")
        && pytest_args.starts_with(&["-m".into(), "pytest".into()]))
        || (name == "uv" && pytest_args.starts_with(&["run".into(), "pytest".into()]))
    {
        pytest_args = &pytest_args[2..];
        true
    } else {
        false
    };
    if !pytest {
        return Err(Bypass::UnsupportedCommand);
    }
    if pytest_args.iter().all(|a| {
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
    }) {
        return Ok(Selection::Pipe {
            filter: "pytest",
            command,
        });
    }
    Err(Bypass::UnsupportedFlags)
}

fn git_selection(rest: &[String], command: String) -> Result<Selection, Bypass> {
    let mut rest = rest;
    if rest.first().map(String::as_str) == Some("-C") {
        if rest.len() < 3 {
            return Err(Bypass::UnsupportedFlags);
        }
        rest = &rest[2..];
    }
    let Some(subcommand) = rest.first().map(String::as_str) else {
        return Err(Bypass::UnsupportedCommand);
    };
    match subcommand {
        "status" if rest.len() == 2 && matches!(rest[1].as_str(), "--short" | "-s") => {
            Ok(Selection::Pipe {
                filter: "git-status",
                command,
            })
        }
        // Exact status/diff reads are machine output and stay raw on purpose.
        "status" | "diff" | "show"
            if rest.iter().any(|arg| {
                arg.starts_with("--porcelain")
                    || arg == "--json"
                    || arg.starts_with("--format")
                    || arg == "-z"
            }) =>
        {
            Err(Bypass::MachineFormat)
        }
        "log" => {
            let mut index = 1;
            while index < rest.len() {
                let arg = &rest[index];
                if matches!(arg.as_str(), "-n" | "--max-count") {
                    index += 1;
                    if rest
                        .get(index)
                        .is_none_or(|value| value.parse::<u32>().is_err())
                    {
                        return Err(Bypass::UnsupportedFlags);
                    }
                } else if arg
                    .strip_prefix("-n")
                    .or_else(|| arg.strip_prefix("--max-count="))
                    .is_some_and(|n| n.parse::<u32>().is_ok())
                    || arg == "HEAD"
                    || arg == "--all"
                {
                } else {
                    return Err(Bypass::UnsupportedFlags);
                }
                index += 1;
            }
            Ok(Selection::Pipe {
                filter: "git-log",
                command,
            })
        }
        _ => Err(Bypass::UnsupportedCommand),
    }
}

/// Job limits and package/workspace selection are the only positional shapes
/// Cargo accepts before `--`; anything unrecognized can change the output
/// format, so the invocation stays raw rather than being summarized wrongly.
fn cargo_verb(rest: &[String]) -> Result<CargoVerb, Bypass> {
    let verb = match rest.first().map(String::as_str) {
        Some("test") => CargoVerb::Test,
        Some("check") => CargoVerb::Check,
        Some("build") => CargoVerb::Build,
        Some("clippy") => CargoVerb::Clippy,
        _ => return Err(Bypass::UnsupportedCommand),
    };
    let (cargo_args, tail) = match rest[1..].iter().position(|arg| arg == "--") {
        Some(separator) => (&rest[1..separator + 1], &rest[separator + 2..]),
        None => (&rest[1..], &[][..]),
    };
    let mut positional = 0;
    let mut index = 0;
    while index < cargo_args.len() {
        let arg = cargo_args[index].as_str();
        match arg {
            "--workspace" | "--all" | "--locked" | "--offline" | "--release" | "--quiet" | "-q"
            | "--all-targets" | "--lib" | "--bins" | "--tests" | "--benches" | "--examples"
            | "--doc" => {}
            "--package" | "-p" | "--exclude" | "--target" | "--target-dir" => {
                index += 1;
                if cargo_args.get(index).is_none() {
                    return Err(Bypass::UnsupportedFlags);
                }
            }
            "--jobs" | "-j" => {
                index += 1;
                if !cargo_args.get(index).is_some_and(|value| job_count(value)) {
                    return Err(Bypass::UnsupportedFlags);
                }
            }
            "--message-format" | "--json" => return Err(Bypass::MachineFormat),
            value if value.starts_with("--message-format") => return Err(Bypass::MachineFormat),
            value if value.starts_with("--jobs=") => {
                if !value
                    .split_once('=')
                    .is_some_and(|(_, count)| job_count(count))
                {
                    return Err(Bypass::UnsupportedFlags);
                }
            }
            value if value.starts_with("-j") && value.len() > 2 => {
                if !job_count(&value[2..]) {
                    return Err(Bypass::UnsupportedFlags);
                }
            }
            value if value.starts_with('-') => return Err(Bypass::UnsupportedFlags),
            _ => {
                positional += 1;
                if verb != CargoVerb::Test || positional > 1 {
                    return Err(Bypass::UnsupportedFlags);
                }
            }
        }
        index += 1;
    }
    let mut index = 0;
    while index < tail.len() {
        let arg = tail[index].as_str();
        match verb {
            // Downstream test-harness arguments. `--nocapture` interleaves the
            // program's own output into the harness stream, so it stays raw.
            CargoVerb::Test => match arg {
                "--test-threads" | "--skip" => {
                    index += 1;
                    if tail.get(index).is_none() {
                        return Err(Bypass::UnsupportedFlags);
                    }
                }
                "--exact" | "--ignored" | "--include-ignored" | "--show-output" | "--quiet" => {}
                value if value.starts_with("--test-threads=") => {
                    if value
                        .split_once('=')
                        .is_none_or(|(_, count)| count.parse::<usize>().is_err())
                    {
                        return Err(Bypass::UnsupportedFlags);
                    }
                }
                value if value.starts_with('-') => return Err(Bypass::UnsupportedFlags),
                _ => {}
            },
            // Downstream lint arguments: `-D warnings`, `--deny clippy::all`.
            CargoVerb::Clippy => match arg {
                "-D" | "-W" | "-A" | "-F" | "--deny" | "--warn" | "--allow" | "--forbid" => {
                    index += 1;
                    if tail.get(index).is_none() {
                        return Err(Bypass::UnsupportedFlags);
                    }
                }
                // Joined form: `-Dwarnings`, `-Wclippy::all`.
                value
                    if value.len() > 2
                        && matches!(value.as_bytes(), [b'-', b'D' | b'W' | b'A' | b'F', ..]) => {}
                value if value.starts_with('-') => return Err(Bypass::UnsupportedFlags),
                _ => return Err(Bypass::UnsupportedFlags),
            },
            CargoVerb::Check | CargoVerb::Build => {
                if !arg.is_empty() {
                    return Err(Bypass::UnsupportedFlags);
                }
            }
        }
        index += 1;
    }
    Ok(verb)
}

fn job_count(value: &str) -> bool {
    value.parse::<usize>().is_ok_and(|count| count >= 1)
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

/// One retained archive per captured stream. `suffix` is empty for stdout and
/// `.err` for stderr, so a whole-file re-read of either stream stays
/// byte-exact and the keep window covers both.
fn save_raw(raw: &[u8], suffix: &str) -> io::Result<PathBuf> {
    let root = raw_directory()?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let path = root.join(format!("rtk-{stamp}-{}{suffix}.log", std::process::id()));
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

/// Packed-observation line for one stream, with the handle it minted. `None`
/// on any pack failure, so the run keeps today's exact compact output and
/// packing can never break compression. The marker keeps a stdout handle and a
/// stderr handle distinguishable in the same footer.
fn pack_line(raw: &[u8], source: &str, marker: &str) -> Option<(String, String)> {
    match pack_store(raw, source) {
        Ok(entry) => Some((
            format!(
                "[rtk pack{marker}: {handle} sha256:{short} | harness-rtk.exe recall {handle} --offset 1 --limit {RECALL_DEFAULT_LIMIT}]\n",
                handle = entry.handle,
                short = short_digest(&entry.digest),
            ),
            entry.handle,
        )),
        Err(error) => {
            eprintln!("rtk: {error}; observation handle not issued");
            None
        }
    }
}

/// The two streams a compact Cargo run accounts for separately.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dest {
    Out,
    Err,
}

impl Dest {
    fn name(self) -> &'static str {
        match self {
            Dest::Out => "stdout",
            Dest::Err => "stderr",
        }
    }

    /// Raw-archive suffix: stdout keeps today's plain `rtk-<stamp>-<pid>.log`.
    fn suffix(self) -> &'static str {
        match self {
            Dest::Out => "",
            Dest::Err => ".err",
        }
    }

    /// Marker naming the original stream in locators, pack lines and packed
    /// sources, so retained evidence never loses its stream identity.
    fn marker(self) -> &'static str {
        match self {
            Dest::Out => "",
            Dest::Err => "[stderr]",
        }
    }

    fn write(self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Dest::Out => io::stdout().lock().write_all(bytes),
            Dest::Err => io::stderr().lock().write_all(bytes),
        }
    }
}

/// One captured stream: the bytes held for a bounded presentation, plus whether
/// the capture bound was crossed, which forwards the remainder live.
struct Capture {
    buffer: Vec<u8>,
    streamed: bool,
}

/// What one stream's retained evidence records. `None` byte fields mean the
/// adapter never captured that stream, so a missing measurement stays
/// unavailable instead of being reported as zero.
struct StreamOutcome {
    stream: &'static str,
    decision: &'static str,
    reason: &'static str,
    raw_bytes: Option<u64>,
    presented_bytes: Option<u64>,
    raw_path: Option<PathBuf>,
    handle: Option<String>,
    footer: Option<String>,
}

impl StreamOutcome {
    fn applied(
        dest: Dest,
        raw: usize,
        presented: usize,
        raw_path: PathBuf,
        handle: Option<String>,
        footer: String,
    ) -> Self {
        Self {
            stream: dest.name(),
            decision: "applied",
            reason: "applied",
            raw_bytes: Some(raw as u64),
            presented_bytes: Some(presented as u64),
            raw_path: Some(raw_path),
            handle,
            footer: Some(footer),
        }
    }

    fn bypassed_measured(dest: Dest, reason: &'static str, raw: usize, presented: usize) -> Self {
        Self {
            stream: dest.name(),
            decision: "bypassed",
            reason,
            raw_bytes: Some(raw as u64),
            presented_bytes: Some(presented as u64),
            raw_path: None,
            handle: None,
            footer: None,
        }
    }

    /// A bypass whose byte volume was never captured, such as an interactive or
    /// oversized run: the measurement stays explicitly unavailable.
    fn bypassed(dest: Dest, reason: &'static str) -> Self {
        Self {
            stream: dest.name(),
            decision: "bypassed",
            reason,
            raw_bytes: None,
            presented_bytes: None,
            raw_path: None,
            handle: None,
            footer: None,
        }
    }
}

fn filter_reason(error: &io::Error) -> &'static str {
    if error.kind() == io::ErrorKind::TimedOut {
        "filter-timeout"
    } else {
        "filter-failed"
    }
}

/// `TimedOut` marks the filter's own deadline so the recorded reason can tell a
/// timeout apart from a failing or noisy filter.
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
        return Err(if timed_out {
            io::Error::new(io::ErrorKind::TimedOut, "filter timed out")
        } else {
            io::Error::other("filter failed or returned diagnostics")
        });
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

/// Single-stream path for the allowlisted git/rg/pytest commands: today's
/// behavior, returning the decision for the bounded local record.
fn filter(mut input: impl Read, name: &str, source: Option<&str>) -> io::Result<StreamOutcome> {
    let mut raw = Vec::new();
    (&mut input)
        .take((RAW_LIMIT + 1) as u64)
        .read_to_end(&mut raw)?;
    if raw.len() > RAW_LIMIT {
        eprintln!("rtk: stdout exceeds 4 MiB; raw passthrough without retention");
        Dest::Out.write(&raw)?;
        io::copy(&mut input, &mut io::stdout().lock())?;
        return Ok(StreamOutcome::bypassed(Dest::Out, "oversize"));
    }
    if raw.len() < RETENTION_FLOOR || std::str::from_utf8(&raw).is_err() || raw.contains(&0) {
        Dest::Out.write(&raw)?;
        let reason = if raw.len() < RETENTION_FLOOR {
            "short-output"
        } else {
            "binary-output"
        };
        return Ok(StreamOutcome::bypassed_measured(
            Dest::Out,
            reason,
            raw.len(),
            raw.len(),
        ));
    }
    let path = match save_raw(&raw, Dest::Out.suffix()) {
        Ok(path) => path,
        Err(_) => {
            eprintln!("rtk: raw capture unavailable; raw passthrough");
            Dest::Out.write(&raw)?;
            return Ok(StreamOutcome::bypassed_measured(
                Dest::Out,
                "retention-unavailable",
                raw.len(),
                raw.len(),
            ));
        }
    };
    match compressed(&raw, name) {
        // Packing belongs to the emitted compact result: when the filter does
        // not shrink the output, the run stays plain raw and must not leave an
        // orphaned pack entry whose handle was never presented.
        Ok(mut result) if result.len() < raw.len() => {
            let mut footer = format!("\n[rtk raw: {}]\n", path.display());
            // A handle is minted only here: the compressed path where
            // `save_raw` already succeeded. The raw locator stays unchanged.
            let handle = source
                .and_then(|source| pack_line(&raw, source, Dest::Out.marker()))
                .map(|(packed, handle)| {
                    footer.push_str(&packed);
                    handle
                });
            let presented = result.len();
            result.extend_from_slice(footer.as_bytes());
            Dest::Out.write(&result)?;
            Ok(StreamOutcome::applied(
                Dest::Out,
                raw.len(),
                presented,
                path,
                handle,
                footer,
            ))
        }
        Ok(_) => {
            Dest::Out.write(&raw)?;
            Ok(StreamOutcome::bypassed_measured(
                Dest::Out,
                "non-shrinking",
                raw.len(),
                raw.len(),
            ))
        }
        Err(error) => {
            eprintln!("rtk: {error}; raw passthrough");
            Dest::Out.write(&raw)?;
            Ok(StreamOutcome::bypassed_measured(
                Dest::Out,
                filter_reason(&error),
                raw.len(),
                raw.len(),
            ))
        }
    }
}

/// Bounded state shared with the capture threads: how much output is held in
/// memory (for the progress notice) and whether any stream left the bound.
#[derive(Default)]
struct Wire {
    captured: AtomicUsize,
    overflow: AtomicBool,
}

/// Read one stream to EOF. Up to `RAW_LIMIT` bytes are held for a bounded
/// presentation; crossing the bound flushes the buffer to the destination and
/// forwards the remainder live, because the raw payload must stay usable even
/// where retention is refused.
fn drain(
    mut reader: impl Read,
    dest: Dest,
    wire: &Wire,
    overflow_notice: &AtomicBool,
) -> io::Result<Capture> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut streamed = false;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            return Ok(Capture { buffer, streamed });
        }
        let part = &chunk[..count];
        if streamed || buffer.len() + count > RAW_LIMIT {
            if !streamed {
                streamed = true;
                wire.overflow.store(true, Ordering::Relaxed);
                if !overflow_notice.swap(true, Ordering::Relaxed) {
                    eprintln!(
                        "rtk: {} exceeds 4 MiB; raw passthrough without retention; this stream continues live",
                        dest.name()
                    );
                }
                dest.write(&buffer)?;
                buffer = Vec::new();
            }
            dest.write(part)?;
        } else {
            buffer.extend_from_slice(part);
            wire.captured.fetch_add(count, Ordering::Relaxed);
        }
    }
}

fn progress_seconds() -> Option<Duration> {
    let seconds = env::var("HARNESS_RTK_PROGRESS_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(PROGRESS_SECONDS);
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

/// Bounded progress visibility while a compact run holds a long command's
/// output back for presentation: one measured line per interval, hard-capped,
/// and none once the run is already streaming raw.
fn progress_notices(wire: &Wire, stop: &AtomicBool) {
    let Some(interval) = progress_seconds() else {
        return;
    };
    let mut last = Instant::now();
    let mut printed = 0;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if printed >= PROGRESS_NOTICES || wire.overflow.load(Ordering::Relaxed) {
            continue;
        }
        let captured = wire.captured.load(Ordering::Relaxed);
        if captured > 0 && last.elapsed() >= interval {
            last = Instant::now();
            printed += 1;
            eprintln!(
                "rtk: compact capture still running; {captured} bytes captured so far (progress notice {printed}/{PROGRESS_NOTICES})"
            );
        }
    }
}

/// Cargo status words that carry progress rather than diagnostics.
const CARGO_STATUS_WORDS: [&str; 21] = [
    "Compiling",
    "Checking",
    "Fresh",
    "Building",
    "Finished",
    "Running",
    "Doc-tests",
    "Downloading",
    "Downloaded",
    "Updating",
    "Blocking",
    "Blocked",
    "Locking",
    "Locked",
    "Dirty",
    "Fetching",
    "Installing",
    "Removing",
    "Adding",
    "Waiting",
    "Executable",
];

/// A Cargo status line: its status word sits inside Cargo's right-aligned
/// 12-column status field, so an indented diagnostic body or a code excerpt
/// never matches.
fn cargo_status_line(line: &str) -> bool {
    let body = line.trim_start_matches(' ');
    let indent = line.len() - body.len();
    if indent == 0 || indent > 12 {
        return false;
    }
    let word = body
        .split([' ', '\t', '\r', '\n'])
        .next()
        .unwrap_or_default();
    CARGO_STATUS_WORDS.contains(&word)
        && body
            .as_bytes()
            .get(word.len())
            .is_none_or(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

/// Remove recognized Cargo progress lines and keep every other line
/// byte-verbatim, including unknown diagnostic blocks. `None` when nothing
/// would be removed or the result would not be smaller, so the caller keeps the
/// raw stream.
fn native_cargo_presentation(raw: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(raw).ok()?;
    if raw.contains(&0) {
        return None;
    }
    let mut output = String::with_capacity(raw.len());
    let mut elided = 0usize;
    for line in text.split_inclusive('\n') {
        if cargo_status_line(line) {
            elided += 1;
            continue;
        }
        output.push_str(line);
    }
    if elided == 0 || output.len() >= raw.len() {
        return None;
    }
    Some(output.into_bytes())
}

/// Decide one bounded capture. An applied presentation is written to the
/// stream's own destination with no footer; the caller emits the locator and
/// handle lines on stdout afterwards.
fn present_stream(
    dest: Dest,
    capture: &Capture,
    verb: CargoVerb,
    command: &str,
) -> io::Result<StreamOutcome> {
    let raw = &capture.buffer;
    if raw.is_empty() {
        return Ok(StreamOutcome::bypassed_measured(dest, "empty-output", 0, 0));
    }
    if raw.len() < RETENTION_FLOOR {
        dest.write(raw)?;
        return Ok(StreamOutcome::bypassed_measured(
            dest,
            "short-output",
            raw.len(),
            raw.len(),
        ));
    }
    if std::str::from_utf8(raw).is_err() || raw.contains(&0) {
        dest.write(raw)?;
        return Ok(StreamOutcome::bypassed_measured(
            dest,
            "binary-output",
            raw.len(),
            raw.len(),
        ));
    }
    let presentation = match (dest, verb) {
        (Dest::Out, CargoVerb::Test) => match compressed(raw, "cargo-test") {
            Ok(result) if result.len() < raw.len() => Some(result),
            Ok(_) => None,
            Err(error) => {
                eprintln!("rtk: {error}; raw passthrough");
                dest.write(raw)?;
                return Ok(StreamOutcome::bypassed_measured(
                    dest,
                    filter_reason(&error),
                    raw.len(),
                    raw.len(),
                ));
            }
        },
        _ => native_cargo_presentation(raw),
    };
    let Some(presentation) = presentation else {
        dest.write(raw)?;
        return Ok(StreamOutcome::bypassed_measured(
            dest,
            "non-shrinking",
            raw.len(),
            raw.len(),
        ));
    };
    let raw_path = match save_raw(raw, dest.suffix()) {
        Ok(path) => path,
        Err(_) => {
            eprintln!("rtk: raw capture unavailable; raw passthrough");
            dest.write(raw)?;
            return Ok(StreamOutcome::bypassed_measured(
                dest,
                "retention-unavailable",
                raw.len(),
                raw.len(),
            ));
        }
    };
    let marker = dest.marker();
    let mut footer = format!("\n[rtk raw{marker}: {}]\n", raw_path.display());
    let source = format!("{command} {marker}");
    let handle = pack_line(raw, source.trim_end(), marker).map(|(packed, handle)| {
        footer.push_str(&packed);
        handle
    });
    dest.write(&presentation)?;
    Ok(StreamOutcome::applied(
        dest,
        raw.len(),
        presentation.len(),
        raw_path,
        handle,
        footer,
    ))
}

/// Dual-stream Cargo path: drain both pipes concurrently, decide each stream
/// independently, keep every raw original reachable and preserve the child's
/// exit code. Cargo runs exactly once; presentation never re-executes it.
fn run_cargo(
    executable: &OsStr,
    args: &[OsString],
    verb: CargoVerb,
    command: &str,
) -> io::Result<i32> {
    let mut line = Command::new(executable);
    line.args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = line.spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let wire = Arc::new(Wire::default());
    let stop = Arc::new(AtomicBool::new(false));
    let overflow_notice = Arc::new(AtomicBool::new(false));
    let out_thread = thread::spawn({
        let wire = Arc::clone(&wire);
        let notice = Arc::clone(&overflow_notice);
        move || drain(stdout, Dest::Out, &wire, &notice)
    });
    let err_thread = thread::spawn({
        let wire = Arc::clone(&wire);
        let notice = Arc::clone(&overflow_notice);
        move || drain(stderr, Dest::Err, &wire, &notice)
    });
    let progress = thread::spawn({
        let wire = Arc::clone(&wire);
        let stop = Arc::clone(&stop);
        move || progress_notices(&wire, &stop)
    });
    let code = child.wait()?.code().unwrap_or(1);
    stop.store(true, Ordering::Relaxed);
    let out = out_thread
        .join()
        .map_err(|_| io::Error::other("stdout capture failed"))??;
    let err = err_thread
        .join()
        .map_err(|_| io::Error::other("stderr capture failed"))??;
    let _ = progress.join();
    let notice = env::var("HARNESS_RTK_DIAGNOSTIC").as_deref() == Ok("1");
    if wire.overflow.load(Ordering::Relaxed) {
        // Already committed to raw passthrough: flush whatever the other stream
        // still holds, and record the limit without inventing measurements.
        if !out.streamed {
            Dest::Out.write(&out.buffer)?;
        }
        if !err.streamed {
            Dest::Err.write(&err.buffer)?;
        }
        let record = RunRecord::new(
            command,
            &format!("cargo-{}", verb.label()),
            "bypassed",
            "oversize",
            vec![
                StreamOutcome::bypassed(Dest::Out, "oversize"),
                StreamOutcome::bypassed(Dest::Err, "oversize"),
            ],
            None,
            Some(code),
        );
        if notice {
            record.report();
        }
        record.store();
        return Ok(code);
    }
    let mut outcomes = vec![
        present_stream(Dest::Out, &out, verb, command)?,
        present_stream(Dest::Err, &err, verb, command)?,
    ];
    let mut footer = String::new();
    for outcome in &mut outcomes {
        if let Some(text) = outcome.footer.take() {
            footer.push_str(&text);
        }
    }
    if !footer.is_empty() {
        // The child's own result stays visible in a compacted presentation.
        if code != 0 {
            footer.push_str(&format!("[rtk cargo exit: {code}]\n"));
        }
        Dest::Out.write(footer.as_bytes())?;
    }
    let applied = outcomes.iter().any(|outcome| outcome.decision == "applied");
    let reason = outcomes
        .iter()
        .map(|outcome| outcome.reason)
        .find(|reason| !matches!(*reason, "applied" | "empty-output"))
        .unwrap_or("empty-output");
    let record = RunRecord::new(
        command,
        &format!("cargo-{}", verb.label()),
        if applied { "applied" } else { "bypassed" },
        if applied { "applied" } else { reason },
        outcomes,
        Some(footer.len() as u64).filter(|bytes| *bytes > 0),
        Some(code),
    );
    if notice {
        record.report();
    }
    record.store();
    Ok(code)
}

/// One bounded local decision record for an explicitly inspected invocation.
/// Byte counts stay bytes: no token, quota or subscription claim is derived
/// from them, a missing measurement stays unavailable, and a failed write never
/// changes the command's own result.
struct RunRecord {
    command: String,
    selection: String,
    decision: &'static str,
    reason: &'static str,
    streams: Vec<StreamOutcome>,
    footer_bytes: Option<u64>,
    exit_code: Option<i32>,
}

impl RunRecord {
    fn new(
        command: &str,
        selection: &str,
        decision: &'static str,
        reason: &'static str,
        streams: Vec<StreamOutcome>,
        footer_bytes: Option<u64>,
        exit_code: Option<i32>,
    ) -> Self {
        Self {
            command: command.to_owned(),
            selection: selection.to_owned(),
            decision,
            reason,
            streams,
            footer_bytes,
            exit_code,
        }
    }

    /// Complete presentation size: every stream's delivered payload plus the
    /// footer lines. `None` while any part of it was never captured.
    fn presentation_bytes(&self) -> Option<u64> {
        if self.streams.is_empty() {
            // A bypassed invocation whose streams were inherited rather than
            // captured has no measurement at all: reporting zero would claim
            // the child wrote nothing.
            return None;
        }
        let streams: Option<u64> = self
            .streams
            .iter()
            .map(|stream| stream.presented_bytes)
            .sum();
        Some(streams? + self.footer_bytes.unwrap_or(0))
    }

    fn to_value(&self) -> Value {
        json!({
            "created": unix_seconds(),
            "mode": "compact",
            "selection": self.selection,
            "command": self.command,
            "decision": self.decision,
            "reason": self.reason,
            "streams": self.streams.iter().map(|stream| json!({
                "stream": stream.stream,
                "decision": stream.decision,
                "reason": stream.reason,
                "raw_bytes": stream.raw_bytes,
                "presented_bytes": stream.presented_bytes,
                "raw": stream.raw_path.as_ref().map(|path| path.display().to_string()),
                "handle": stream.handle,
            })).collect::<Vec<_>>(),
            "presentation_bytes": self.presentation_bytes(),
            "footer_bytes": self.footer_bytes,
            "exit_code": self.exit_code,
            "tokens": Value::Null,
            "token_measurement": "unavailable",
        })
    }

    /// One bounded line on stderr, only when the caller opts in. Routine runs
    /// stay silent, and no raw or machine payload is ever changed by this.
    fn report(&self) {
        let streams: Vec<String> = self
            .streams
            .iter()
            .map(|stream| match (stream.raw_bytes, stream.presented_bytes) {
                (Some(raw), Some(presented)) => format!(
                    "{} {} ({}, {raw} B -> {presented} B)",
                    stream.stream, stream.decision, stream.reason
                ),
                _ => format!(
                    "{} {} ({}, bytes unavailable)",
                    stream.stream, stream.decision, stream.reason
                ),
            })
            .collect();
        let presentation = self
            .presentation_bytes()
            .map(|bytes| format!("{bytes} B"))
            .unwrap_or_else(|| "unavailable".to_owned());
        eprintln!(
            "rtk: compact `{}` -> {} ({}); {}; presentation {presentation}; tokens unavailable",
            self.command,
            self.decision,
            self.reason,
            streams.join("; ")
        );
    }

    fn store(&self) {
        if let Err(error) = write_record(self.to_value()) {
            // A record is evidence, never a gate: the run keeps its result.
            if env::var("HARNESS_RTK_DIAGNOSTIC").as_deref() == Ok("1") {
                eprintln!("rtk: decision record unavailable: {error}");
            }
        }
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn diagnostics_path() -> io::Result<PathBuf> {
    Ok(codex_home()?.join("harness/rtk/diagnostics/records.json"))
}

/// Bounded retention for decision records: the newest `DIAGNOSTIC_KEEP` stay,
/// oldest first, and a missing or damaged record file is never allowed to fail
/// the command that produced it.
fn write_record(entry: Value) -> io::Result<()> {
    let path = diagnostics_path()?;
    let Some(root) = path.parent() else {
        return Err(io::Error::other("diagnostic path has no directory"));
    };
    fs::create_dir_all(root)?;
    let mut records: Vec<Value> = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|value| value.get("records").and_then(Value::as_array).cloned())
            .unwrap_or_default(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    records.push(entry);
    let excess = records.len().saturating_sub(DIAGNOSTIC_KEEP);
    records.drain(..excess);
    let value = json!({"schema": 1, "records": records});
    let temporary = root.join("records.json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec(&value).map_err(io::Error::other)?,
    )?;
    // Replacement rename: a torn write can never be read as the live record.
    fs::rename(temporary, &path)
}

/// Civil UTC timestamp without a calendar dependency.
fn utc_stamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let time = seconds % 86_400;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_position + 2) / 5 + 1;
    let month = month_position + if month_position < 10 { 3 } else { -9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60
    )
}

fn render_record(record: &Value) -> String {
    let stamp = record
        .get("created")
        .and_then(Value::as_u64)
        .map(utc_stamp)
        .unwrap_or_else(|| "unknown-time".to_owned());
    let mut line = format!(
        "{stamp} {} ({}) `{}`",
        record
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
        record
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
        record.get("command").and_then(Value::as_str).unwrap_or("?")
    );
    for stream in record
        .get("streams")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = stream
            .get("stream")
            .and_then(Value::as_str)
            .unwrap_or("stream");
        let decision = stream
            .get("decision")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let reason = stream
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        match (
            stream.get("raw_bytes").and_then(Value::as_u64),
            stream.get("presented_bytes").and_then(Value::as_u64),
        ) {
            (Some(raw), Some(presented)) => {
                line.push_str(&format!(
                    "; {name} {decision} ({reason}) {raw} B -> {presented} B"
                ));
            }
            _ => line.push_str(&format!("; {name} {decision} ({reason}) bytes unavailable")),
        }
        if let Some(handle) = stream.get("handle").and_then(Value::as_str) {
            line.push_str(&format!(" handle {handle}"));
        }
    }
    match record.get("presentation_bytes").and_then(Value::as_u64) {
        Some(bytes) => line.push_str(&format!("; presentation {bytes} B")),
        None => line.push_str("; presentation unavailable"),
    }
    if let Some(code) = record.get("exit_code").and_then(Value::as_i64) {
        line.push_str(&format!("; child exit {code}"));
    }
    line.push_str("; tokens unavailable");
    line
}

/// Read the bounded local decision records. Inspection only: it starts no
/// command, model or network request.
fn diagnostics(arguments: &[OsString]) -> io::Result<i32> {
    let mut last = DIAGNOSTIC_DEFAULT;
    let mut json_output = false;
    let mut index = 0;
    while index < arguments.len() {
        let Some(option) = arguments[index].to_str() else {
            eprintln!("rtk: arguments must be valid UTF-8; {DIAGNOSTIC_USAGE}");
            return Ok(1);
        };
        match option {
            "--json" => json_output = true,
            "--last" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .and_then(|value| value.to_str())
                    .and_then(|value| value.parse::<usize>().ok());
                let Some(value) = value.filter(|value| *value >= 1) else {
                    eprintln!("rtk: --last requires a positive integer; {DIAGNOSTIC_USAGE}");
                    return Ok(1);
                };
                last = value.min(DIAGNOSTIC_KEEP);
            }
            other => {
                eprintln!("rtk: unknown option {other}; {DIAGNOSTIC_USAGE}");
                return Ok(1);
            }
        }
        index += 1;
    }
    let records = match fs::read(diagnostics_path()?) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|value| value.get("records").and_then(Value::as_array).cloned())
        {
            Some(records) => records,
            None => {
                eprintln!("rtk: decision record is unreadable; the next compact run replaces it");
                return Ok(1);
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    if records.is_empty() {
        eprintln!("rtk: no compact-run decision records yet");
        return Ok(0);
    }
    let window = &records[records.len().saturating_sub(last)..];
    let mut output = io::stdout().lock();
    if json_output {
        let value = json!({"schema": 1, "records": window});
        output.write_all(
            serde_json::to_string_pretty(&value)
                .map_err(io::Error::other)?
                .as_bytes(),
        )?;
        output.write_all(b"\n")?;
        return Ok(0);
    }
    writeln!(
        output,
        "[rtk diagnostics: {} of {} records]",
        window.len(),
        records.len()
    )?;
    for record in window {
        writeln!(output, "{}", render_record(record))?;
    }
    Ok(0)
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

/// Run the native program once. `compact` selects the compression path when
/// stdout is captured; the `exec` entry stays a raw passthrough. Either way the
/// child keeps its own arguments, cwd, environment, stdin and exit status.
fn run(arguments: &[OsString], compact: bool) -> io::Result<i32> {
    let executable = arguments
        .first()
        .ok_or_else(|| io::Error::other("native executable required"))?;
    if !compact {
        return run_inherited(executable, &arguments[1..]);
    }
    let text_arguments: Option<Vec<String>> = arguments
        .iter()
        .map(|a| a.to_str().map(str::to_owned))
        .collect();
    let command_text = text_arguments
        .as_deref()
        .map(|args| args.join(" "))
        .unwrap_or_else(|| "<non-utf8 arguments>".to_owned());
    let selection = if env::var("HARNESS_RTK_DISABLE").as_deref() == Ok("1") {
        Err(Bypass::Disabled)
    } else if io::stdout().is_terminal() {
        Err(Bypass::Interactive)
    } else {
        text_arguments
            .as_deref()
            .map_or(Err(Bypass::UnsupportedCommand), select)
    };
    match selection {
        Ok(Selection::Cargo { verb, command }) => {
            run_cargo(executable, &arguments[1..], verb, &command)
        }
        Ok(Selection::Pipe { filter, command }) => {
            run_pipe(executable, &arguments[1..], filter, &command)
        }
        Err(reason) => {
            let code = run_inherited(executable, &arguments[1..])?;
            let record = RunRecord::new(
                &command_text,
                "bypassed",
                "bypassed",
                reason.reason(),
                Vec::new(),
                None,
                Some(code),
            );
            if env::var("HARNESS_RTK_DIAGNOSTIC").as_deref() == Ok("1") {
                record.report();
            }
            record.store();
            Ok(code)
        }
    }
}

fn run_inherited(executable: &OsStr, args: &[OsString]) -> io::Result<i32> {
    let status = Command::new(executable)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(status.code().unwrap_or(1))
}

/// Allowlisted single-stream compression: stdout is captured and filtered
/// while the child's stderr and exit status stay untouched.
fn run_pipe(
    executable: &OsStr,
    args: &[OsString],
    name: &'static str,
    command: &str,
) -> io::Result<i32> {
    let mut line = Command::new(executable);
    line.args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = line.spawn()?;
    let outcome = match filter(child.stdout.take().unwrap(), name, Some(command)) {
        Ok(outcome) => outcome,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let code = child.wait()?.code().unwrap_or(1);
    let footer_bytes = outcome
        .footer
        .as_ref()
        .map(|footer| footer.len() as u64)
        .filter(|bytes| *bytes > 0);
    let record = RunRecord::new(
        command,
        &format!("pipe:{name}"),
        outcome.decision,
        outcome.reason,
        vec![outcome],
        footer_bytes,
        Some(code),
    );
    if env::var("HARNESS_RTK_DIAGNOSTIC").as_deref() == Ok("1") {
        record.report();
    }
    record.store();
    Ok(code)
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
        Some("diagnostics") => diagnostics(&arguments[2..]),
        Some("exec") => run(&arguments[2..], false),
        Some("compact") => run(&arguments[2..], true),
        _ => Err(io::Error::other(
            "usage: harness-rtk exec|compact EXECUTABLE ARGS... | hook | filter FILTER | recall HANDLE [--offset N] [--limit M] | diagnostics [--last N] [--json]",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn verb(args: &[&str]) -> Result<&'static str, Bypass> {
        let owned: Vec<String> = args.iter().map(|value| (*value).to_owned()).collect();
        cargo_verb(&owned).map(|verb| verb.label())
    }

    /// The presentation's whole safety property: a status line is progress, and
    /// a diagnostic or a code excerpt is never mistaken for one.
    #[test]
    fn cargo_status_lines_are_progress_and_nothing_else() {
        for line in [
            "   Compiling demo v0.1.0 (/tmp/demo)\n",
            "    Checking demo v0.1.0\n",
            "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s\n",
            "     Running unittests src/lib.rs (target/debug/deps/demo-1)\n",
            "   Doc-tests demo\n",
            "    Building [=====>     ] 3/30\n",
        ] {
            assert!(cargo_status_line(line), "{line:?}");
        }
        for line in [
            "error[E0308]: mismatched types\n",
            "warning: unused variable: `unused_factor`\n",
            " --> src/lib.rs:7:9\n",
            "  |         ^^^^^^^^^^^^^ help: if this is intentional, prefix it\n",
            "running 2 tests\n",
            "test result: ok. 2 passed; 0 failed; 0 ignored\n",
            "Compiling demo starts at column one, so it is not a status line\n",
            "             Compiling sits past the 12-column status field\n",
            "Finished\n",
            "For more information about this error, try `rustc --explain E0308`.\n",
            "   1 |     let unused_factor = 3;\n",
            "error: could not compile `synthetic-broken` (lib) due to 1 previous error\n",
        ] {
            assert!(!cargo_status_line(line), "{line:?}");
        }
    }

    #[test]
    fn cargo_argv_selection_covers_the_documented_corpus() {
        assert_eq!(
            verb(&[
                "test",
                "--workspace",
                "--locked",
                "--jobs",
                "1",
                "--",
                "--test-threads=1"
            ]),
            Ok("test")
        );
        assert_eq!(
            verb(&["check", "-j4", "--target-dir", "target/x"]),
            Ok("check")
        );
        assert_eq!(
            verb(&["build", "--release", "-j", "2", "--all-targets"]),
            Ok("build")
        );
        assert_eq!(verb(&["clippy", "--", "-D", "warnings"]), Ok("clippy"));
        assert_eq!(verb(&["test", "a_filter_name"]), Ok("test"));
        assert_eq!(verb(&["test", "--json"]), Err(Bypass::MachineFormat));
        assert_eq!(
            verb(&["check", "--message-format", "json"]),
            Err(Bypass::MachineFormat)
        );
        assert_eq!(verb(&["doc"]), Err(Bypass::UnsupportedCommand));
        assert_eq!(verb(&["test", "-v"]), Err(Bypass::UnsupportedFlags));
        assert_eq!(verb(&["check", "-p"]), Err(Bypass::UnsupportedFlags));
        assert_eq!(
            verb(&["check", "two", "positionals"]),
            Err(Bypass::UnsupportedFlags)
        );
        assert_eq!(
            verb(&["test", "--", "--nocapture"]),
            Err(Bypass::UnsupportedFlags)
        );
        assert_eq!(
            verb(&["clippy", "--", "--fix"]),
            Err(Bypass::UnsupportedFlags)
        );
        assert_eq!(
            verb(&["build", "--", "-Zunstable"]),
            Err(Bypass::UnsupportedFlags)
        );
    }

    #[test]
    fn utc_stamps_are_civil_dates() {
        assert_eq!(utc_stamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_stamp(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(utc_stamp(1_800_000_000), "2027-01-15T08:00:00Z");
    }
}
