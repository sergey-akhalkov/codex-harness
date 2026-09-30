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
/// Bounded store transaction: one publisher at a time, and a bounded wait so a
/// stuck holder cannot stall the observed command.
const STORE_LOCK_TIMEOUT: Duration = Duration::from_secs(2);
const STORE_LOCK_BACKOFF: Duration = Duration::from_millis(5);
/// Staging area inside the pack directory, so publication is a same-volume
/// rename and a crashed writer's leftovers are recoverable under the lock.
const PACK_STAGING: &str = "staging";
const STORE_LOCK_FILE: &str = "store.lock";

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

/// Deterministic test seam for the two-process publication, interruption and
/// eviction reproductions. At a named point the process announces itself on
/// stderr and then blocks until the owning test unlocks
/// `<point>.release`; process death releases that lock too, so a lost test can
/// never wedge an adapter that set `HARNESS_RTK_TEST_BARRIER_DIR`.
fn test_barrier(point: &str) {
    let Some(directory) = env::var_os("HARNESS_RTK_TEST_BARRIER_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let _ = fs::write(directory.join(format!("{point}.reached")), b"");
    eprintln!("rtk: test barrier {point} reached");
    let Ok(release) = fs::OpenOptions::new()
        .read(true)
        .open(directory.join(format!("{point}.release")))
    else {
        return;
    };
    let _ = release.lock();
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
///
/// The path is planned before anything is written, so the complete
/// presentation can be measured with the exact locator it would carry; the
/// archive itself is created only when the compact form is actually delivered.
fn planned_raw_path(suffix: &str, stamp: u128) -> io::Result<PathBuf> {
    let root = raw_directory()?;
    Ok(root.join(format!("rtk-{stamp}-{}{suffix}.log", std::process::id())))
}

fn save_raw_at(path: &Path, raw: &[u8]) -> io::Result<()> {
    let Some(root) = path.parent() else {
        return Err(io::Error::other("raw archive path has no directory"));
    };
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
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
    Ok(())
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

    /// Strict record reader: every field is required with its exact type, and
    /// an invalid record fails the whole index instead of being dropped, so
    /// nothing unvalidated can reach path construction or retention.
    fn from_value(value: &Value) -> Result<Self, String> {
        let handle = value
            .get("handle")
            .and_then(Value::as_str)
            .ok_or_else(|| "the handle is not a string".to_owned())?;
        if !valid_handle(handle) {
            return Err(format!("handle {handle:?} is not an observation handle"));
        }
        let source = value
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| "the source is not a string".to_owned())?;
        let digest = value
            .get("digest")
            .and_then(Value::as_str)
            .ok_or_else(|| "the digest is not a string".to_owned())?;
        if !valid_digest(digest) {
            return Err(format!("digest {digest:?} is not a SHA-256 hex digest"));
        }
        let bytes = value
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| "the byte size is not a non-negative integer".to_owned())?;
        let lines = value
            .get("lines")
            .and_then(Value::as_u64)
            .ok_or_else(|| "the line count is not a non-negative integer".to_owned())?;
        let created = value
            .get("created")
            .and_then(Value::as_u64)
            .ok_or_else(|| "the creation time is not a non-negative integer".to_owned())?;
        Ok(Self {
            handle: handle.to_owned(),
            source: source.to_owned(),
            digest: digest.to_owned(),
            bytes,
            lines,
            created,
        })
    }
}

/// The minted observation handle grammar: `ob-<19-digit nanos>-<pid>-<sequence>`.
/// No other shape may ever be turned into a path, so separators, parent-relative
/// names and absolute paths cannot enter recall or retention.
fn valid_handle(handle: &str) -> bool {
    let Some(rest) = handle.strip_prefix("ob-") else {
        return false;
    };
    let mut parts = rest.split('-');
    let (Some(nanos), Some(pid), Some(sequence), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    nanos.len() == 19
        && nanos.bytes().all(|byte| byte.is_ascii_digit())
        && !pid.is_empty()
        && pid.len() <= 10
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && sequence.len() >= 6
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    if value.get("schema").and_then(Value::as_u64) != Some(PACK_SCHEMA) {
        return Err(io::Error::other(
            "pack index has an unsupported schema; retained evidence is left untouched",
        ));
    }
    let entries = value
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("pack index has no entry list"))?;
    // Every record must validate: a damaged or unknown record makes the whole
    // index unusable, because serving or purging from a reduced view would
    // destroy evidence the dropped records still name.
    let mut records: Vec<PackEntry> = Vec::with_capacity(entries.len());
    for (position, entry) in entries.iter().enumerate() {
        let record = PackEntry::from_value(entry).map_err(|error| {
            io::Error::other(format!(
                "pack index record {position} is invalid ({error}); retained evidence is left untouched"
            ))
        })?;
        if records
            .iter()
            .any(|existing| existing.handle == record.handle)
        {
            return Err(io::Error::other(format!(
                "pack index record {position} repeats a handle; retained evidence is left untouched"
            )));
        }
        records.push(record);
    }
    let mut total: u64 = 0;
    for record in &records {
        total = total.checked_add(record.bytes).ok_or_else(|| {
            io::Error::other(
                "pack index retained size is not representable; retained evidence is left untouched",
            )
        })?;
    }
    Ok(records)
}

fn write_pack_index(pack: &Path, entries: &[PackEntry]) -> io::Result<()> {
    // A unique temporary name keeps a concurrent publisher (including an older
    // adapter build that still uses a common name) from tearing this write.
    let temporary = pack.join(format!(
        "index-{}-{}.tmp",
        std::process::id(),
        HANDLE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let value = json!({
        "schema": PACK_SCHEMA,
        "entries": entries.iter().map(PackEntry::to_value).collect::<Vec<_>>(),
    });
    if let Err(error) = fs::write(
        &temporary,
        serde_json::to_vec(&value).map_err(io::Error::other)?,
    ) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    // Replacement rename: a torn write can never be read as the live index.
    match fs::rename(&temporary, pack.join("index.json")) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

/// Mint an observation name from one creation stamp. The name is decided
/// before anything is written, so the delivered footer and the compression
/// ledger use exactly the handle the store publishes.
fn mint_handle(nanos: u128) -> String {
    format!(
        "ob-{nanos:019}-{}-{:06}",
        std::process::id(),
        HANDLE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

/// The handle line a compact presentation advertises; the marker keeps a
/// stdout handle and a stderr handle distinguishable in the same footer.
fn pack_line_text(handle: &str, digest: &str, marker: &str) -> String {
    format!(
        "[rtk pack{marker}: {handle} sha256:{short} | harness-rtk.exe recall {handle} --offset 1 --limit {RECALL_DEFAULT_LIMIT}]\n",
        short = short_digest(digest),
    )
}

/// One publisher at a time. The lock lives on an open file handle, so process
/// death releases it and recovery never has to trust a reusable process id.
struct StoreLock {
    _file: fs::File,
}

impl StoreLock {
    fn acquire(pack: &Path) -> io::Result<Self> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(pack.join(STORE_LOCK_FILE))?;
        let deadline = Instant::now() + STORE_LOCK_TIMEOUT;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(fs::TryLockError::WouldBlock) => {
                    if Instant::now() >= deadline {
                        return Err(io::Error::other(
                            "observation store is busy; another writer holds it",
                        ));
                    }
                    thread::sleep(STORE_LOCK_BACKOFF);
                }
                Err(fs::TryLockError::Error(error)) => return Err(error),
            }
        }
    }
}

/// Write the payload into `staging/` while the store lock is held. Any staging
/// file that recovery can still see therefore belongs to a writer that died
/// inside its transaction; a live writer is either holding the lock or still
/// waiting for it, and never owns an uncommitted staging file.
fn stage_observation(pack: &Path, handle: &str, raw: &[u8]) -> io::Result<PathBuf> {
    let staging = pack.join(PACK_STAGING);
    fs::create_dir_all(&staging)?;
    let path = staging.join(format!("{handle}.log"));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)?;
    if let Err(error) = file.write_all(raw) {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}

/// Remove only state that an interrupted transaction can leave behind: staging
/// leftovers and committed `*.log` files the validated index does not name.
/// Runs under the store lock, where neither can belong to a live writer, and
/// never follows a reparse point or recurses into a directory.
fn recover_store(pack: &Path, entries: &[PackEntry]) {
    if let Ok(listing) = fs::read_dir(pack.join(PACK_STAGING)) {
        for file in listing.filter_map(Result::ok) {
            let Ok(kind) = file.file_type() else {
                continue;
            };
            if kind.is_symlink() || !kind.is_file() {
                continue;
            }
            let _ = fs::remove_file(file.path());
        }
    }
    let Ok(listing) = fs::read_dir(pack) else {
        return;
    };
    for file in listing.filter_map(Result::ok) {
        let name = file.file_name();
        let name = name.to_string_lossy();
        let Some(handle) = name.strip_suffix(".log") else {
            continue;
        };
        if entries.iter().any(|entry| entry.handle == handle) {
            continue;
        }
        let Ok(kind) = file.file_type() else {
            continue;
        };
        if kind.is_symlink() || !kind.is_file() {
            continue;
        }
        let _ = fs::remove_file(file.path()); // Private pack namespace; never recurse.
    }
}

/// Oldest-first eviction at 64 entries or 128 MiB. The plan is computed before
/// the index is rewritten and its file removals happen only after that commit.
fn retention_plan(entries: &mut Vec<PackEntry>) -> io::Result<Vec<PackEntry>> {
    entries
        .sort_by(|left, right| (left.created, &left.handle).cmp(&(right.created, &right.handle)));
    let mut total = 0u64;
    for entry in entries.iter() {
        total = total
            .checked_add(entry.bytes)
            .ok_or_else(|| io::Error::other("pack retention arithmetic is not representable"))?;
    }
    let mut evicted = Vec::new();
    while entries.len() > PACK_KEEP_FILES || total > PACK_KEEP_BYTES {
        let oldest = entries.remove(0);
        total = total.saturating_sub(oldest.bytes);
        evicted.push(oldest);
    }
    Ok(evicted)
}

/// Commit one observation in a single short store transaction: validate the
/// index, recover interrupted state, stage and rename the content, publish the
/// index, then evict. Validation failures abort before any file is removed, so
/// damaged metadata can never authorize destructive retention. Command
/// execution and presentation never run inside the critical section.
fn commit_observation(
    pack: &Path,
    handle: &str,
    digest: &str,
    raw: &[u8],
    source: &str,
    created: u64,
) -> io::Result<PackEntry> {
    test_barrier("store-before-lock");
    let _lock = StoreLock::acquire(pack)?;
    let mut entries = read_pack_index(pack)?;
    recover_store(pack, &entries);
    let staged = stage_observation(pack, handle, raw)?;
    test_barrier("store-staged");
    let final_path = pack.join(format!("{handle}.log"));
    if fs::symlink_metadata(&final_path).is_ok() {
        let _ = fs::remove_file(&staged);
        return Err(io::Error::other(format!(
            "observation handle {handle} already has committed content"
        )));
    }
    if let Err(error) = fs::rename(&staged, &final_path) {
        let _ = fs::remove_file(&staged);
        return Err(error);
    }
    let entry = PackEntry {
        handle: handle.to_owned(),
        source: source.to_owned(),
        digest: digest.to_owned(),
        bytes: raw.len() as u64,
        lines: split_lines(&String::from_utf8_lossy(raw)).len() as u64,
        created,
    };
    entries.push(entry.clone());
    let evicted = match retention_plan(&mut entries) {
        Ok(evicted) => evicted,
        Err(error) => {
            let _ = fs::remove_file(&final_path);
            return Err(error);
        }
    };
    if let Err(error) = write_pack_index(pack, &entries) {
        let _ = fs::remove_file(&final_path);
        return Err(error);
    }
    test_barrier("store-committed");
    for old in evicted {
        let _ = fs::remove_file(pack.join(format!("{}.log", old.handle)));
    }
    Ok(entry)
}

/// One prepared compact presentation. Every field is exact: the compression
/// ledger uses these bytes, and delivery writes exactly them (minus a pack line
/// only when publication fails, which strictly shrinks the footer).
struct CompactPlan {
    body: Vec<u8>,
    raw_path: PathBuf,
    locator: String,
    evidence: Evidence,
}

/// Where a prepared stream's observation evidence stands when the presentation
/// is selected for delivery.
enum Evidence {
    /// The observation is committed under the store lock.
    Observation(ObservationPlan),
    /// The pack area itself is unavailable; the failure is reported only if the
    /// compact form is delivered, so a raw fallback stays as quiet as today.
    Unavailable(String),
    /// No source command is known, as in the `filter` entry point; today's
    /// behavior mints no handle there.
    Omitted,
}

struct ObservationPlan {
    pack: PathBuf,
    handle: String,
    digest: String,
    pack_line: String,
    source: String,
    created: u64,
}

impl CompactPlan {
    fn footer_len(&self) -> usize {
        self.locator.len()
            + match &self.evidence {
                Evidence::Observation(observation) => observation.pack_line.len(),
                Evidence::Unavailable(_) | Evidence::Omitted => 0,
            }
    }

    fn output_len(&self) -> usize {
        self.body.len() + self.footer_len()
    }
}

/// What one compact delivery actually wrote and advertised. The body is not
/// written here: the caller publishes bodies and footers only after every
/// stream's evidence state is known.
struct Delivery {
    presented: usize,
    footer: String,
    handle: Option<String>,
    raw_path: PathBuf,
}

impl Delivery {
    fn output_len(&self) -> usize {
        self.presented + self.footer.len()
    }
}

fn locator_text(dest: Dest, raw_path: &Path) -> String {
    format!("\n[rtk raw{}: {}]\n", dest.marker(), raw_path.display())
}

/// Prepare a complete compact presentation for one stream: the filtered body,
/// the planned raw-archive locator, and the exact handle line it could
/// advertise. Nothing is written or published here, so a raw fallback leaves
/// no orphaned observation.
fn plan_compact(
    dest: Dest,
    body: Vec<u8>,
    raw: &[u8],
    source: Option<&str>,
    notices: &Notices,
) -> Option<CompactPlan> {
    let Ok(nanos) = unix_nanos() else {
        notices.emit("rtk: raw capture unavailable; raw passthrough");
        return None;
    };
    let Ok(raw_path) = planned_raw_path(dest.suffix(), nanos) else {
        notices.emit("rtk: raw capture unavailable; raw passthrough");
        return None;
    };
    let locator = locator_text(dest, &raw_path);
    let evidence = match source {
        None => Evidence::Omitted,
        Some(source) => match pack_directory() {
            Err(error) => Evidence::Unavailable(error.to_string()),
            Ok(pack) => {
                let handle = mint_handle(nanos);
                let digest = digest_hex(raw);
                let pack_line = pack_line_text(&handle, &digest, dest.marker());
                Evidence::Observation(ObservationPlan {
                    pack,
                    handle,
                    digest,
                    pack_line,
                    source: source.to_owned(),
                    created: (nanos / 1_000_000_000) as u64,
                })
            }
        },
    };
    Some(CompactPlan {
        body,
        raw_path,
        locator,
        evidence,
    })
}

/// Deliver one prepared compact stream: write its raw archive and commit its
/// observation, reporting the footer and handle that may be emitted. `None`
/// means retention itself is unavailable, so the caller keeps the raw stream.
fn deliver_compact(plan: &CompactPlan, raw: &[u8], notices: &Notices) -> Option<Delivery> {
    if save_raw_at(&plan.raw_path, raw).is_err() {
        notices.emit("rtk: raw capture unavailable; raw passthrough");
        return None;
    }
    let mut footer = plan.locator.clone();
    let mut handle = None;
    match &plan.evidence {
        Evidence::Observation(observation) => {
            match commit_observation(
                &observation.pack,
                &observation.handle,
                &observation.digest,
                raw,
                &observation.source,
                observation.created,
            ) {
                Ok(_) => {
                    footer.push_str(&observation.pack_line);
                    handle = Some(observation.handle.clone());
                }
                Err(error) => notices.emit(&format!("rtk: {error}; observation handle not issued")),
            }
        }
        Evidence::Unavailable(message) => {
            notices.emit(&format!("rtk: {message}; observation handle not issued"));
        }
        Evidence::Omitted => {}
    }
    Some(Delivery {
        presented: plan.body.len(),
        footer,
        handle,
        raw_path: plan.raw_path.clone(),
    })
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

/// Adapter-emitted status text for one compact invocation. Every byte the
/// adapter itself prints participates in the compression ledger, so overhead
/// that makes the delivered result larger than the raw stream is visible
/// instead of hidden in an uncounted channel.
#[derive(Clone, Default)]
struct Notices {
    bytes: Arc<AtomicUsize>,
}

impl Notices {
    fn emit(&self, message: &str) {
        let _ = writeln!(io::stderr(), "{message}");
        self.bytes.fetch_add(message.len() + 1, Ordering::Relaxed);
    }

    fn measured(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed) as u64
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
/// behavior, returning the decision for the bounded local record. The caller
/// owns the notices ledger so the complete presentation accounts for every
/// byte the adapter itself emits.
fn filter(
    mut input: impl Read,
    name: &str,
    source: Option<&str>,
    notices: &Notices,
) -> io::Result<StreamOutcome> {
    let mut raw = Vec::new();
    (&mut input)
        .take((RAW_LIMIT + 1) as u64)
        .read_to_end(&mut raw)?;
    if raw.len() > RAW_LIMIT {
        notices.emit("rtk: stdout exceeds 4 MiB; raw passthrough without retention");
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
    // The complete candidate is prepared first, including the locator and
    // handle text the footer would carry, so the compression decision uses the
    // same bytes later reported as delivered.
    let plan = match compressed(&raw, name) {
        Ok(result) if result.len() < raw.len() => {
            match plan_compact(Dest::Out, result, &raw, source, notices) {
                Some(plan) => plan,
                None => {
                    Dest::Out.write(&raw)?;
                    return Ok(StreamOutcome::bypassed_measured(
                        Dest::Out,
                        "retention-unavailable",
                        raw.len(),
                        raw.len(),
                    ));
                }
            }
        }
        Ok(_) => {
            Dest::Out.write(&raw)?;
            return Ok(StreamOutcome::bypassed_measured(
                Dest::Out,
                "non-shrinking",
                raw.len(),
                raw.len(),
            ));
        }
        Err(error) => {
            notices.emit(&format!("rtk: {error}; raw passthrough"));
            Dest::Out.write(&raw)?;
            return Ok(StreamOutcome::bypassed_measured(
                Dest::Out,
                filter_reason(&error),
                raw.len(),
                raw.len(),
            ));
        }
    };
    if plan.output_len() + notices.measured() as usize >= raw.len() {
        Dest::Out.write(&raw)?;
        return Ok(StreamOutcome::bypassed_measured(
            Dest::Out,
            "non-shrinking",
            raw.len(),
            raw.len(),
        ));
    }
    let Some(delivery) = deliver_compact(&plan, &raw, notices) else {
        Dest::Out.write(&raw)?;
        return Ok(StreamOutcome::bypassed_measured(
            Dest::Out,
            "retention-unavailable",
            raw.len(),
            raw.len(),
        ));
    };
    // A failed publication replaces the pack line with one shorter notice, but
    // the exact ledger is re-checked so `applied` always means fewer bytes.
    if delivery.output_len() + notices.measured() as usize >= raw.len() {
        Dest::Out.write(&raw)?;
        return Ok(StreamOutcome::bypassed_measured(
            Dest::Out,
            "non-shrinking",
            raw.len(),
            raw.len(),
        ));
    }
    Dest::Out.write(&plan.body)?;
    Dest::Out.write(delivery.footer.as_bytes())?;
    Ok(StreamOutcome::applied(
        Dest::Out,
        raw.len(),
        delivery.presented,
        delivery.raw_path,
        delivery.handle,
        delivery.footer,
    ))
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
    notices: &Notices,
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
                    notices.emit(&format!(
                        "rtk: {} exceeds 4 MiB; raw passthrough without retention; this stream continues live",
                        dest.name()
                    ));
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
fn progress_notices(wire: &Wire, stop: &AtomicBool, notices: &Notices) {
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
            notices.emit(&format!(
                "rtk: compact capture still running; {captured} bytes captured so far (progress notice {printed}/{PROGRESS_NOTICES})"
            ));
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

/// A Cargo status line: Cargo writes `{:>12} {}`, so the status word ends
/// exactly at column 12 and a space opens the message. Checking that exact
/// documented layout - not just an indentation range - keeps user output and
/// diagnostic noise byte-preserved even when it happens to contain a status
/// word.
fn cargo_status_line(line: &str) -> bool {
    let body = line.trim_start_matches(' ');
    let indent = line.len() - body.len();
    let Some(word) = body.split(' ').next() else {
        return false;
    };
    indent + word.len() == 12
        && CARGO_STATUS_WORDS.contains(&word)
        && body.as_bytes().get(word.len()) == Some(&b' ')
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

/// One prepared stream of the dual-stream Cargo path. Nothing has been written
/// yet: the run-level decision needs both complete presentations first, so a
/// raw fallback never leaves an orphaned observation or a half-emitted body.
enum StreamAttempt {
    Raw { reason: &'static str },
    Compact(Box<CompactPlan>),
}

/// The reason a prepared stream contributes to the run when its compact form
/// is not the representation the run delivers.
fn attempt_reason(attempt: &StreamAttempt) -> &'static str {
    match attempt {
        StreamAttempt::Raw { reason } => reason,
        StreamAttempt::Compact(_) => "non-shrinking",
    }
}

/// Prepare one bounded capture: run the selected filter, then plan the complete
/// compact presentation including the locator and handle line it would carry.
/// The raw stream is never written here.
fn prepare_stream(
    dest: Dest,
    capture: &Capture,
    verb: CargoVerb,
    command: &str,
    notices: &Notices,
) -> StreamAttempt {
    let raw = &capture.buffer;
    if raw.is_empty() {
        return StreamAttempt::Raw {
            reason: "empty-output",
        };
    }
    if raw.len() < RETENTION_FLOOR {
        return StreamAttempt::Raw {
            reason: "short-output",
        };
    }
    if std::str::from_utf8(raw).is_err() || raw.contains(&0) {
        return StreamAttempt::Raw {
            reason: "binary-output",
        };
    }
    let body = match (dest, verb) {
        (Dest::Out, CargoVerb::Test) => match compressed(raw, "cargo-test") {
            Ok(result) if result.len() < raw.len() => Some(result),
            Ok(_) => None,
            Err(error) => {
                notices.emit(&format!("rtk: {error}; raw passthrough"));
                return StreamAttempt::Raw {
                    reason: filter_reason(&error),
                };
            }
        },
        _ => native_cargo_presentation(raw),
    };
    let Some(body) = body else {
        return StreamAttempt::Raw {
            reason: "non-shrinking",
        };
    };
    let marker = dest.marker();
    let source = format!("{command} {marker}");
    match plan_compact(dest, body, raw, Some(source.trim_end()), notices) {
        Some(plan) => StreamAttempt::Compact(Box::new(plan)),
        None => StreamAttempt::Raw {
            reason: "retention-unavailable",
        },
    }
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
    let notices = Notices::default();
    let out_thread = thread::spawn({
        let wire = Arc::clone(&wire);
        let notice = Arc::clone(&overflow_notice);
        let notices = notices.clone();
        move || drain(stdout, Dest::Out, &wire, &notice, &notices)
    });
    let err_thread = thread::spawn({
        let wire = Arc::clone(&wire);
        let notice = Arc::clone(&overflow_notice);
        let notices = notices.clone();
        move || drain(stderr, Dest::Err, &wire, &notice, &notices)
    });
    let progress = thread::spawn({
        let wire = Arc::clone(&wire);
        let stop = Arc::clone(&stop);
        let notices = notices.clone();
        move || progress_notices(&wire, &stop, &notices)
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
    let announcement = env::var("HARNESS_RTK_DIAGNOSTIC").as_deref() == Ok("1");
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
            Some(code),
        )
        .adapter_bytes(Some(notices.measured()));
        if announcement {
            record.report();
        }
        record.store();
        return Ok(code);
    }
    // Prepare both complete presentations before deciding, so the labeled
    // decision covers every byte the run can emit: both bodies, locators,
    // handle text, the exit notice and the adapter's own measured notices.
    let attempts = [
        prepare_stream(Dest::Out, &out, verb, command, &notices),
        prepare_stream(Dest::Err, &err, verb, command, &notices),
    ];
    let captures = [&out, &err];
    let destinations = [Dest::Out, Dest::Err];
    let raw_total = out.buffer.len() + err.buffer.len();
    let exit_notice = if code == 0 {
        String::new()
    } else {
        format!("[rtk cargo exit: {code}]\n")
    };
    let planned: usize = attempts
        .iter()
        .zip(captures)
        .map(|(attempt, capture)| match attempt {
            StreamAttempt::Compact(plan) => plan.output_len(),
            StreamAttempt::Raw { .. } => capture.buffer.len(),
        })
        .sum();
    let has_compact = attempts
        .iter()
        .any(|attempt| matches!(attempt, StreamAttempt::Compact(_)));
    if !has_compact || planned + exit_notice.len() + notices.measured() as usize >= raw_total {
        // The complete accounted presentation is not smaller than the captured
        // originals: both streams stay byte-raw and nothing is published.
        Dest::Out.write(&out.buffer)?;
        Dest::Err.write(&err.buffer)?;
        let outcomes = raw_outcomes(&attempts, &captures, destinations);
        return Ok(finish_run(
            command,
            verb,
            code,
            outcomes,
            None,
            notices.measured(),
            announcement,
        ));
    }
    // Deliver evidence next; a publication failure replaces a pack line with
    // one shorter notice, and the exact ledger is re-checked before the label
    // is applied, so `applied` always means fewer emitted bytes.
    let deliveries = [
        match &attempts[0] {
            StreamAttempt::Compact(plan) => deliver_compact(plan, &out.buffer, &notices),
            StreamAttempt::Raw { .. } => None,
        },
        match &attempts[1] {
            StreamAttempt::Compact(plan) => deliver_compact(plan, &err.buffer, &notices),
            StreamAttempt::Raw { .. } => None,
        },
    ];
    let delivered: usize = deliveries
        .iter()
        .zip(captures)
        .map(|(delivery, capture)| match delivery {
            Some(delivery) => delivery.output_len(),
            None => capture.buffer.len(),
        })
        .sum();
    let any_delivered = deliveries.iter().any(Option::is_some);
    if !any_delivered {
        Dest::Out.write(&out.buffer)?;
        Dest::Err.write(&err.buffer)?;
        let outcomes = raw_outcomes(&attempts, &captures, destinations);
        return Ok(finish_run(
            command,
            verb,
            code,
            outcomes,
            None,
            notices.measured(),
            announcement,
        ));
    }
    let with_exit = delivered + exit_notice.len() + notices.measured() as usize;
    if with_exit >= raw_total {
        // A storage fallback grew the ledger past the raw originals; the run
        // reports raw delivery and never advertises an unshown handle.
        Dest::Out.write(&out.buffer)?;
        Dest::Err.write(&err.buffer)?;
        let outcomes = raw_outcomes(&attempts, &captures, destinations);
        return Ok(finish_run(
            command,
            verb,
            code,
            outcomes,
            None,
            notices.measured(),
            announcement,
        ));
    }
    let mut outcomes = Vec::with_capacity(2);
    let mut footer = String::new();
    for index in 0..2 {
        match (&attempts[index], &deliveries[index]) {
            (StreamAttempt::Compact(plan), Some(delivery)) => {
                destinations[index].write(&plan.body)?;
                footer.push_str(&delivery.footer);
                outcomes.push(StreamOutcome::applied(
                    destinations[index],
                    captures[index].buffer.len(),
                    delivery.presented,
                    delivery.raw_path.clone(),
                    delivery.handle.clone(),
                    delivery.footer.clone(),
                ));
            }
            _ => {
                destinations[index].write(&captures[index].buffer)?;
                outcomes.push(StreamOutcome::bypassed_measured(
                    destinations[index],
                    attempt_reason(&attempts[index]),
                    captures[index].buffer.len(),
                    captures[index].buffer.len(),
                ));
            }
        }
    }
    if !footer.is_empty() {
        // The child's own result stays visible in a compacted presentation.
        if code != 0 {
            footer.push_str(&exit_notice);
        }
        Dest::Out.write(footer.as_bytes())?;
    }
    let footer_bytes = (!footer.is_empty()).then_some(footer.len() as u64);
    Ok(finish_run(
        command,
        verb,
        code,
        outcomes,
        footer_bytes,
        notices.measured(),
        announcement,
    ))
}

/// The all-raw outcome pair for a run whose complete presentation does not
/// shrink, with each stream keeping its own honest reason.
fn raw_outcomes(
    attempts: &[StreamAttempt; 2],
    captures: &[&Capture; 2],
    destinations: [Dest; 2],
) -> Vec<StreamOutcome> {
    (0..2)
        .map(|index| {
            StreamOutcome::bypassed_measured(
                destinations[index],
                attempt_reason(&attempts[index]),
                captures[index].buffer.len(),
                captures[index].buffer.len(),
            )
        })
        .collect()
}

/// Record one finished compact run and return the child's exit code. The
/// record is evidence, never a gate: a failed write never changes the result.
fn finish_run(
    command: &str,
    verb: CargoVerb,
    code: i32,
    outcomes: Vec<StreamOutcome>,
    footer_bytes: Option<u64>,
    adapter_bytes: u64,
    announcement: bool,
) -> i32 {
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
        Some(code),
    )
    .footer_bytes(footer_bytes)
    .adapter_bytes(Some(adapter_bytes));
    if announcement {
        record.report();
    }
    record.store();
    code
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
    /// Adapter-emitted progress/status/error text. `None` for an invocation
    /// whose streams were never captured, where no measurement is claimed.
    adapter_bytes: Option<u64>,
    exit_code: Option<i32>,
}

impl RunRecord {
    fn new(
        command: &str,
        selection: &str,
        decision: &'static str,
        reason: &'static str,
        streams: Vec<StreamOutcome>,
        exit_code: Option<i32>,
    ) -> Self {
        Self {
            command: command.to_owned(),
            selection: selection.to_owned(),
            decision,
            reason,
            streams,
            footer_bytes: None,
            adapter_bytes: None,
            exit_code,
        }
    }

    /// The footer text emitted after the presented bodies, when any.
    fn footer_bytes(mut self, bytes: Option<u64>) -> Self {
        self.footer_bytes = bytes;
        self
    }

    /// The adapter's own emitted notice bytes for this run.
    fn adapter_bytes(mut self, bytes: Option<u64>) -> Self {
        self.adapter_bytes = bytes;
        self
    }

    /// Complete presentation size: every stream's delivered payload, the footer
    /// lines and the adapter's own emitted notices. `None` while any part of it
    /// was never captured.
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
        Some(streams? + self.footer_bytes.unwrap_or(0) + self.adapter_bytes.unwrap_or(0))
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
            "adapter_bytes": self.adapter_bytes,
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
        let adapter = self
            .adapter_bytes
            .filter(|bytes| *bytes > 0)
            .map(|bytes| format!("; {bytes} B adapter notices"))
            .unwrap_or_default();
        eprintln!(
            "rtk: compact `{}` -> {} ({}); {}; presentation {presentation}{adapter}; tokens unavailable",
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
    // The requested name must match the minted grammar before any path is
    // constructed from it; anything else can only be an unknown handle.
    if !valid_handle(handle) {
        return unknown_handle(handle);
    }
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
    // Deterministic seam for the reader/eviction overlap reproduction: the
    // committed observation is known, and the test may now evict it before the
    // content read below.
    test_barrier("recall-indexed");
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
    let notices = Notices::default();
    let outcome = match filter(child.stdout.take().unwrap(), name, Some(command), &notices) {
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
        Some(code),
    )
    .footer_bytes(footer_bytes)
    .adapter_bytes(Some(notices.measured()));
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
            filter(
                io::stdin().lock(),
                &arguments[2].to_string_lossy(),
                None,
                &Notices::default(),
            )
            .map(|_| 0)
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
    /// a diagnostic, a code excerpt or misaligned user text is never mistaken
    /// for one. Cargo writes `{:>12} {}`, so the status word must end exactly at
    /// column 12 and a space must open the message.
    #[test]
    fn cargo_status_lines_are_progress_and_nothing_else() {
        for line in [
            "   Compiling demo v0.1.0 (/tmp/demo)\n",
            "    Checking demo v0.1.0\n",
            "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.11s\n",
            "     Running unittests src/lib.rs (target/debug/deps/demo-1)\n",
            "   Doc-tests demo\n",
            "    Building [=====>     ] 3/30\n",
            "       Fresh demo v0.1.0\n",
            " Downloading crates ...\n",
            "  Downloaded demo v0.1.0\n",
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
            // Wrong width for the word: user text that merely starts with a
            // status word stays byte-preserved.
            "Compiling demo starts at column one, so it is not a status line\n",
            "  Compiling demo is one column short of the status field\n",
            "    Compiling demo is one column past the status field\n",
            "    Doc-tests demo is not Cargo's aligned doc-test line\n",
            "   Compiling\n",
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
