//! Native reproduce-regression process observer. Fixture roles are compiled
//! into this executable so tests never spawn Python or PowerShell helpers.

use codex_harness::regression;

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).and_then(|arg| arg.to_str()) == Some("--fixture") {
        match run_fixture(&args[2..]) {
            Ok(code) => std::process::exit(code),
            Err(error) => {
                eprintln!("harness-observe fixture: {error}");
                std::process::exit(2);
            }
        }
    }
    match regression::run_cli(&args[1..]) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("harness-observe: {error}");
            std::process::exit(2);
        }
    }
}

fn run_fixture(args: &[std::ffi::OsString]) -> io::Result<i32> {
    let role = args
        .first()
        .and_then(|arg| arg.to_str())
        .ok_or_else(|| io::Error::other("fixture role required"))?;
    let extra: Vec<String> = args
        .iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    match role {
        "ok" => {
            signal_ready()?;
            echo_stdin()?;
            println!("fixture stdout");
            eprintln!("fixture stderr");
            Ok(0)
        }
        "link-report" => {
            let root = case_root()?;
            let foreign = extra
                .first()
                .ok_or_else(|| io::Error::other("foreign path required"))?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(foreign, root.join("report.json"))?;
            #[cfg(not(windows))]
            std::os::unix::fs::symlink(foreign, root.join("report.json"))?;
            signal_ready()?;
            Ok(0)
        }
        "echo-args" => {
            signal_ready()?;
            println!("{}", extra.join("|"));
            Ok(0)
        }
        "fail" => {
            eprintln!("original failure");
            Ok(7)
        }
        "no-ready" => {
            println!("no readiness");
            Ok(0)
        }
        "oversize-ready" => {
            fs::write(case_root()?.join("ready.txt"), "READY".repeat(20))?;
            std::thread::sleep(Duration::from_secs(45));
            std::process::exit(99);
        }
        "hold" => {
            std::thread::sleep(Duration::from_secs(45));
            std::process::exit(99);
        }
        "ready-hold" => {
            signal_ready()?;
            std::thread::sleep(Duration::from_secs(45));
            std::process::exit(99);
        }
        "cancel-hold" => {
            signal_ready()?;
            wait_for_cancel()?;
            std::thread::sleep(Duration::from_secs(45));
            std::process::exit(99);
        }
        "flood" => {
            signal_ready()?;
            let chunk = vec![b'x'; 8192];
            let mut out = io::stdout().lock();
            for _ in 0..32 {
                out.write_all(&chunk)?;
            }
            out.flush()?;
            Ok(0)
        }
        "streams" => {
            signal_ready()?;
            let stdout = vec![b'a'; 2 * 1024 * 1024];
            let stderr = vec![b'b'; 2 * 1024 * 1024];
            let a = std::thread::spawn(move || io::stdout().write_all(&stdout));
            let b = std::thread::spawn(move || io::stderr().write_all(&stderr));
            a.join().map_err(|_| io::Error::other("stdout thread"))??;
            b.join().map_err(|_| io::Error::other("stderr thread"))??;
            Ok(0)
        }
        "tree-hold" => {
            let mut child = std::process::Command::new(std::env::current_exe()?)
                .args(["--fixture", "descendant"])
                .spawn()?;
            signal_ready()?;
            let _ = child.wait();
            Ok(0)
        }
        "descendant" => {
            std::thread::sleep(Duration::from_secs(5));
            if let Some(root) = std::env::var_os("PROCESS_CASE_ROOT") {
                fs::write(
                    Path::new(&root).join("descendant-after-timeout.txt"),
                    "leaked",
                )?;
            }
            Ok(0)
        }
        "cwd" => {
            signal_ready()?;
            println!("{}", std::env::current_dir()?.display());
            Ok(0)
        }
        "env" => {
            signal_ready()?;
            println!("{}", std::env::var("PROCESS_CASE_ROOT").unwrap_or_default());
            Ok(0)
        }
        "stdin" => {
            signal_ready()?;
            echo_stdin()?;
            Ok(0)
        }
        other => Err(io::Error::other(format!("unknown fixture role {other}"))),
    }
}

fn case_root() -> io::Result<std::path::PathBuf> {
    std::env::var_os("PROCESS_CASE_ROOT")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| io::Error::other("PROCESS_CASE_ROOT missing"))
}

fn signal_ready() -> io::Result<()> {
    fs::write(case_root()?.join("ready.txt"), "READY")
}

fn wait_for_cancel() -> io::Result<()> {
    let path = case_root()?.join("wait-cancel.txt");
    fs::write(&path, "waiting")?;
    Ok(())
}

fn echo_stdin() -> io::Result<()> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    if !input.is_empty() {
        io::stdout().write_all(&input)?;
        io::stdout().flush()?;
    }
    Ok(())
}
