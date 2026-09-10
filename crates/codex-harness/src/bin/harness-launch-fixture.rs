//! Owned native upstream double. No model or network use.
#[path = "../discovery_fixture.rs"]
mod discovery_fixture;
#[path = "../outcome_case_fixture.rs"]
mod outcome_case_fixture;
#[path = "../outcome_fixture.rs"]
mod outcome_fixture;
#[path = "../outcome_process_checker_fixture.rs"]
mod outcome_process_checker_fixture;
use serde_json::json;
use std::{
    env,
    io::{self, IsTerminal, Read, Write},
    process::{Command, Stdio},
    time::Duration,
};

fn main() -> io::Result<()> {
    // A separate native bridge mode avoids leaking the fixture behavior into
    // the eventual upstream CLI, which receives the user's original arguments.
    if env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "config-overrides")
        && env::var("HARNESS_LAUNCH_FIXTURE_BRIDGE_MODE").as_deref() == Ok("hang")
    {
        std::fs::write(
            env::var_os("HARNESS_LAUNCH_FIXTURE_BRIDGE_STARTED").unwrap(),
            std::process::id().to_string(),
        )?;
        std::thread::sleep(Duration::from_secs(20));
        return Ok(());
    }
    if let Some(path) = env::var_os("HARNESS_LAUNCH_FIXTURE_STARTED") {
        std::fs::write(path, std::process::id().to_string())?;
    }
    if env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--outcome-case")
    {
        return outcome_case_fixture::run();
    }
    let mode = env::var("HARNESS_LAUNCH_FIXTURE_MODE").unwrap_or_else(|_| "report".into());
    match mode.as_str() {
        "dependency-version" => {
            if let Some(marker) = env::var_os("HARNESS_LAUNCH_FIXTURE_MARKER") {
                std::fs::write(
                    marker,
                    serde_json::to_vec(&json!({
                        "cwd":env::current_dir()?,"args":env::args().skip(1).collect::<Vec<_>>(),
                        "rustup_home":env::var("RUSTUP_HOME").ok(),"cargo_home":env::var("CARGO_HOME").ok(),
                        "auto_install":env::var("RUSTUP_AUTO_INSTALL").ok()
                    }))?,
                )?;
            }
            match env::var("HARNESS_DEPENDENCY_VERSION_FIXTURE").as_deref() {
                Ok("timeout") => std::thread::sleep(Duration::from_secs(20)),
                Ok("private-error") => {
                    println!("private version stdout sentinel");
                    eprintln!("private version stderr sentinel");
                    std::process::exit(23);
                }
                _ => println!("rust-analyzer 1.97.1 (owned fixture)"),
            }
            return Ok(());
        }
        "outcome" => return outcome_fixture::run(),
        "discovery" => return discovery_fixture::run(),
        "outcome-process-checker" => return outcome_process_checker_fixture::run(),
        "background" => {
            let child = Command::new(env::current_exe()?)
                .env("HARNESS_LAUNCH_FIXTURE_MODE", "delayed")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            println!("{}", child.id());
            return Ok(());
        }
        "delayed" => {
            std::thread::sleep(Duration::from_millis(500));
            std::fs::write(
                env::var_os("HARNESS_LAUNCH_FIXTURE_MARKER").unwrap(),
                "background completed",
            )?;
            return Ok(());
        }
        "ctrl-c" => {
            println!("upstream ready");
            io::stdout().flush()?;
            std::thread::sleep(Duration::from_secs(5));
            std::process::exit(29);
        }
        "interactive" => {
            println!("upstream prompt console={}", io::stdout().is_terminal());
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            println!("upstream echo:{line}");
            return Ok(());
        }
        _ => (),
    }
    let mut stdin = String::new();
    io::stdin().read_to_string(&mut stdin)?;
    let variables = [
        "CODEX_MANAGED_PACKAGE_ROOT",
        "CODEX_MANAGED_BY_NPM",
        "CODEX_MANAGED_BY_BUN",
        "CODEX_MANAGED_BY_PNPM",
        "CODEX_MANAGED_BY_VITE_PLUS",
        "HARNESS_LSP_WORKSPACE_ROOTS",
    ];
    let environment: serde_json::Map<String, serde_json::Value> = variables
        .iter()
        .map(|name| {
            (
                name.to_string(),
                env::var(name)
                    .ok()
                    .map_or(serde_json::Value::Null, |s| s.into()),
            )
        })
        .collect();
    println!(
        "{}",
        json!({"args": env::args().skip(1).collect::<Vec<_>>(), "stdin": stdin,
        "cwd": env::current_dir()?, "environment": environment})
    );
    eprintln!("upstream stderr");
    if mode == "nonzero" {
        std::process::exit(19);
    }
    Ok(())
}
