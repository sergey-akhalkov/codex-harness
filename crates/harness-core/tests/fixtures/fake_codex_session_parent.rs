//! Upstream double that exits after leaving a child, like PowerShell Start-Process.
use std::{env, fs, process::Command, thread, time::Duration};

fn main() {
    if env::var_os("HARNESS_UPSTREAM_ORPHAN_CHILD").is_some() {
        if let Ok(ready) = env::var("HARNESS_UPSTREAM_ORPHAN_READY") {
            fs::write(ready, "ready").expect("ready");
        }
        thread::sleep(Duration::from_secs(45));
        std::process::exit(99);
    }
    let args: Vec<String> = env::args().skip(1).collect();
    let marker = args
        .windows(2)
        .find(|pair| pair[0] == "--orphan-marker")
        .map(|pair| pair[1].clone())
        .expect("missing --orphan-marker");
    let code = args
        .windows(2)
        .find(|pair| pair[0] == "--exit")
        .and_then(|pair| pair[1].parse().ok())
        .unwrap_or(0);
    let ready = format!("{marker}.ready");
    let mut child = Command::new(env::current_exe().expect("current exe"));
    child
        .env("HARNESS_UPSTREAM_ORPHAN_CHILD", "1")
        .env("HARNESS_UPSTREAM_ORPHAN_READY", &ready);
    let spawned = child.spawn().expect("detached helper");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !std::path::Path::new(&ready).is_file() {
        if std::time::Instant::now() >= deadline {
            panic!("detached helper did not become ready");
        }
        thread::sleep(Duration::from_millis(10));
    }
    fs::write(&marker, spawned.id().to_string()).expect("orphan pid");
    std::process::exit(code);
}
