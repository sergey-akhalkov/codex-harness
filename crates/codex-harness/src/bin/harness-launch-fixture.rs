//! Owned native upstream double. No model or network use.
#[path = "../outcome_fixture.rs"]
mod outcome_fixture;
#[path = "../discovery_fixture.rs"]
mod discovery_fixture;
#[path = "../outcome_case_fixture.rs"]
mod outcome_case_fixture;
use serde_json::json;
use std::{
    env,
    io::{self, IsTerminal, Read, Write},
    process::{Command, Stdio},
    time::Duration,
};

fn main() -> io::Result<()> {
    if env::args_os().nth(1).is_some_and(|arg| arg == "--outcome-case") {
        return outcome_case_fixture::run();
    }
    let mode = env::var("HARNESS_LAUNCH_FIXTURE_MODE").unwrap_or_else(|_| "report".into());
    match mode.as_str() {
        "outcome" => return outcome_fixture::run(),
        "discovery" => return discovery_fixture::run(),
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
