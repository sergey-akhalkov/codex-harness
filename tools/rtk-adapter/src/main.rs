//! Explicit native-command boundary; the hook never guesses a caller's shell.
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const RAW_LIMIT: usize = 4 * 1024 * 1024;
const KEEP_FILES: usize = 32;

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

fn raw_directory() -> io::Result<PathBuf> {
    let home = env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".codex")))
        .ok_or_else(|| io::Error::other("Codex home unavailable"))?;
    let root = home.join("harness/rtk/raw");
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
    if timed_out || !status.success() || !error.is_empty() || output.len() > RAW_LIMIT {
        return Err(io::Error::other(if timed_out {
            "filter timed out"
        } else {
            "filter failed or returned diagnostics"
        }));
    }
    Ok(output)
}

fn filter(mut input: impl Read, name: &str) -> io::Result<()> {
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
            result.extend_from_slice(format!("\n[rtk raw: {}]\n", path.display()).as_bytes());
            if result.len() < raw.len() {
                return output.write_all(&result);
            }
        }
        Err(error) => eprintln!("rtk: {error}; raw passthrough"),
    }
    output.write_all(&raw)
}

fn run(arguments: &[std::ffi::OsString], compact: bool) -> io::Result<i32> {
    let executable = arguments
        .first()
        .ok_or_else(|| io::Error::other("native executable required"))?;
    let text_arguments: Option<Vec<String>> = arguments
        .iter()
        .map(|a| a.to_str().map(str::to_owned))
        .collect();
    let selected = if compact
        && !io::stdout().is_terminal()
        && env::var("HARNESS_RTK_DISABLE").as_deref() != Ok("1")
    {
        text_arguments.as_deref().and_then(filter_for)
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
    if let Some(name) = selected
        && let Err(error) = filter(child.stdout.take().unwrap(), name)
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
            filter(io::stdin().lock(), &arguments[2].to_string_lossy()).map(|_| 0)
        }
        Some("exec") => run(&arguments[2..], false),
        Some("compact") => run(&arguments[2..], true),
        _ => Err(io::Error::other(
            "usage: harness-rtk exec|compact EXECUTABLE ARGS... | hook | filter FILTER",
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
