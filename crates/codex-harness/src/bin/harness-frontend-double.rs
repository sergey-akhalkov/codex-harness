//! Owned frontend double for the executor observation checks.
//!
//! It is not a renderer and does not speak the app-server protocol. It records
//! the arguments the host actually passed, sets the console caption the host
//! uses as attachment evidence, and waits until the test releases it. The
//! production path launches the native Codex TUI instead.

use std::{
    env, fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn main() {
    let home = env::var_os("CODEX_HOME").map(PathBuf::from);
    if let Some(home) = &home {
        let _ = fs::write(
            home.join("frontend-argv.txt"),
            env::args().skip(1).fold(String::new(), |mut acc, arg| {
                if !acc.is_empty() {
                    acc.push('\n');
                }
                acc.push_str(&arg);
                acc
            }),
        );
    }
    let title = home
        .as_ref()
        .and_then(|path| fs::read_to_string(path.join("frontend-title.txt")).ok())
        .unwrap_or_default();
    let title = title.trim();
    if title.is_empty() {
        eprintln!("frontend double: frontend-title.txt is missing");
        std::process::exit(2);
    }
    if let Err(error) = harness_core::task_control::set_console_caption(&format!("{title} | ready"))
    {
        eprintln!("frontend double caption: {error}");
        std::process::exit(3);
    }
    let Some(home) = home else {
        std::process::exit(2);
    };
    let release = home.join("frontend-release");
    let until = Instant::now() + Duration::from_secs(90);
    while Instant::now() < until && !release.is_file() {
        thread::sleep(Duration::from_millis(40));
    }
}
