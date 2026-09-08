//! Native Codex argument policy. Never parse a prompt or reconstruct a shell command.
use std::{
    ffi::{OsStr, OsString},
    io,
    path::{Path, PathBuf},
};

const VALUES: &[&str] = &[
    "-c",
    "--config",
    "--enable",
    "--disable",
    "--remote",
    "--remote-auth-token-env",
    "-m",
    "--model",
    "--local-provider",
    "-p",
    "--profile",
    "-s",
    "--sandbox",
    "-C",
    "--cd",
    "--add-dir",
    "-a",
    "--ask-for-approval",
    "--thread-source",
    "--output-schema",
    "--color",
    "-o",
    "--output-last-message",
    "--base",
    "--commit",
    "--title",
];
const COMMANDS: &[&str] = &[
    "agents",
    "exec",
    "e",
    "review",
    "login",
    "logout",
    "mcp",
    "plugin",
    "mcp-server",
    "app-server",
    "remote-control",
    "app",
    "completion",
    "update",
    "doctor",
    "sandbox",
    "debug",
    "apply",
    "a",
    "resume",
    "queue",
    "archive",
    "delete",
    "migrate-rollouts",
    "unarchive",
    "fork",
    "cloud",
    "exec-server",
    "features",
    "help",
];

fn option(arg: &str, name: &str) -> bool {
    arg == name || arg.strip_prefix(name).is_some_and(|s| s.starts_with('='))
}
fn image(arg: &str) -> bool {
    option(arg, "--image") || arg == "-i" || (arg.starts_with("-i") && arg.len() > 2)
}
fn skip_images(args: &[OsString], index: &mut usize) {
    while args
        .get(*index + 1)
        .is_some_and(|s| !s.to_string_lossy().starts_with('-'))
    {
        *index += 1;
    }
}
fn effort_config(arg: &OsStr) -> bool {
    arg.to_str()
        .and_then(|s| s.trim_start().strip_prefix("model_reasoning_effort"))
        .is_some_and(|s| s.trim_start().starts_with('='))
}

pub fn task_arguments(args: &[OsString]) -> io::Result<Vec<OsString>> {
    let Some(first) = args.first().and_then(|s| s.to_str()) else {
        return Ok(args.to_vec());
    };
    if !option(first, "--harness-effort") {
        return Ok(args.to_vec());
    }
    let (choice, offset) = if let Some(choice) = first.strip_prefix("--harness-effort=") {
        (choice, 1)
    } else {
        (args.get(1).and_then(|s| s.to_str()).unwrap_or(""), 2)
    };
    let effort = match choice.to_ascii_lowercase().as_str() {
        "routine" => "low",
        "standard" => "high",
        "demanding" => "xhigh",
        _ => {
            return Err(io::Error::other(
                "--harness-effort requires routine, standard or demanding.",
            ));
        }
    };
    let rest = &args[offset..];
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].to_str().unwrap_or("");
        if arg == "--" {
            break;
        }
        if option(arg, "--profile") || arg.starts_with("-p") || option(arg, "--remote") {
            return Ok(rest.to_vec());
        }
        if matches!(arg, "-c" | "--config") {
            if rest.get(i + 1).is_some_and(|s| effort_config(s)) {
                return Ok(rest.to_vec());
            }
            i += 2;
            continue;
        }
        if arg
            .strip_prefix("--config=")
            .or_else(|| arg.strip_prefix("-c"))
            .is_some_and(|s| effort_config(OsStr::new(s)))
        {
            return Ok(rest.to_vec());
        }
        if VALUES.contains(&arg) {
            i += 2;
            continue;
        }
        if image(arg) {
            skip_images(rest, &mut i);
        }
        i += 1;
    }
    let mut result = vec![
        "-c".into(),
        format!("model_reasoning_effort=\"{effort}\"").into(),
    ];
    result.extend_from_slice(rest);
    Ok(result)
}

pub fn profile_arguments(args: &[OsString]) -> Vec<OsString> {
    let mut command = None;
    let mut debug = None;
    let mut positional = false;
    let mut following = 0;
    let mut preserve = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_str().unwrap_or("");
        if arg == "--" {
            break;
        }
        if matches!(arg, "-h" | "--help" | "-V" | "--version")
            || arg
                .strip_prefix('-')
                .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c == 'h' || c == 'V'))
            || option(arg, "--profile")
            || arg.starts_with("-p")
            || option(arg, "--remote")
        {
            preserve = true;
        }
        if VALUES.contains(&arg) {
            i += 2;
            continue;
        }
        if image(arg) {
            skip_images(args, &mut i);
            i += 1;
            continue;
        }
        if arg.starts_with('-') && arg != "-" {
            i += 1;
            continue;
        }
        if !positional {
            positional = true;
            if COMMANDS.contains(&arg) {
                command = Some(arg);
            }
        } else {
            following += 1;
            if following == 1 {
                if matches!(command, Some("exec" | "e")) && arg == "help" {
                    preserve = true;
                }
                if command == Some("debug") {
                    debug = Some(arg);
                }
            }
        }
        i += 1;
    }
    let session = matches!(
        command,
        None | Some("exec" | "e" | "review" | "resume" | "fork")
    ) || (command == Some("debug") && debug == Some("prompt-input"));
    let mut result = Vec::new();
    if session && !preserve {
        result.extend(["--profile".into(), "harness".into()]);
    }
    result.extend_from_slice(args);
    result
}

pub fn additional_roots(args: &[OsString], cwd: &Path) -> Vec<PathBuf> {
    let mut directory = cwd.to_path_buf();
    let mut roots = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_str().unwrap_or("");
        if arg == "--" {
            break;
        }
        if option(arg, "--remote") {
            return Vec::new();
        }
        if matches!(arg, "--add-dir" | "-C" | "--cd") {
            let Some(value) = args.get(i + 1) else {
                return Vec::new();
            };
            if arg == "--add-dir" {
                roots.push(PathBuf::from(value));
            } else {
                directory = value.into();
            }
            i += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--add-dir=") {
            roots.push(value.into());
        } else if let Some(value) = arg
            .strip_prefix("--cd=")
            .or_else(|| arg.strip_prefix("-C").filter(|s| !s.is_empty()))
        {
            directory = value.into();
        } else if VALUES.contains(&arg) {
            i += 2;
            continue;
        } else if image(arg) {
            skip_images(args, &mut i);
        }
        i += 1;
    }
    let directory = if directory.is_absolute() {
        directory
    } else {
        cwd.join(directory)
    };
    let mut result = Vec::new();
    for root in roots {
        let path = if root.is_absolute() {
            root
        } else {
            directory.join(root)
        };
        if let Ok(path) = path.canonicalize()
            && path.is_dir()
            && !result.contains(&path)
        {
            result.push(path);
        }
    }
    result
}
