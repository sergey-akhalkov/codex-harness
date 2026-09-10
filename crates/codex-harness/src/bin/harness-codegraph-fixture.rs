//! Inert protocol peer for the native managed CodeGraph boundary.
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, Write},
    time::Duration,
};

fn main() -> io::Result<()> {
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some(harness_core::process_service::CREATE_ARGUMENT) {
        harness_core::process_service::create_helper_entry()
    }
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some(harness_core::process_service::RUN_ARGUMENT) {
        let args: Vec<_> = std::env::args().skip(2).collect();
        if args.len() != 5 || args[2] != "codegraph" {
            return Err(io::Error::other(
                "invalid CodeGraph fixture service arguments",
            ));
        }
        let guard = harness_core::process_service::ServiceGuard::enter(
            args[0].parse().map_err(io::Error::other)?,
            &args[1],
            harness_core::process::Limits {
                memory_bytes: Some(2 * 1024 * 1024 * 1024),
                cpu_percent: Some(25.0),
            },
        )?;
        return harness_core::codegraph_broker::serve(
            guard,
            &args[3],
            serde_json::from_str(&args[4])?,
        );
    }
    #[cfg(windows)]
    if matches!(
        std::env::args().nth(1).as_deref(),
        Some("--managed" | "--managed-shared")
    ) {
        let configuration = serde_json::from_str(
            &std::env::args()
                .nth(2)
                .ok_or_else(|| io::Error::other("missing managed fixture configuration"))?,
        )?;
        let (input, output) = harness_core::mcp_stdio::standard_files()?;
        let seconds = std::env::var("HARNESS_FIXTURE_CONNECTION_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| (1..=900).contains(value))
            .unwrap_or(120);
        if let Some(root) = std::env::args_os().nth(3) {
            return harness_core::codegraph_stdio::serve_shared(
                configuration,
                root.into(),
                input,
                output,
                &harness_core::process::Cancellation::default(),
                harness_core::process::Deadline::after(Duration::from_secs(seconds))?,
            );
        }
        return harness_core::codegraph_stdio::serve(
            configuration,
            input,
            output,
            &harness_core::process::Cancellation::default(),
            harness_core::process::Deadline::after(Duration::from_secs(seconds))?,
        );
    }
    let mut mode = std::env::args().nth(1).unwrap_or_else(|| "normal".into());
    if matches!(
        std::env::args().nth(2).as_deref(),
        Some("serve" | "index" | "sync")
    ) {
        mode = std::path::Path::new(&mode)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("normal")
            .to_string();
    }
    if mode == "per-project" {
        mode = fs::read_to_string(".fixture-mode")?.trim().to_owned();
        writeln!(
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("owned-worker-launches")?,
            "started"
        )?;
    }
    #[cfg(windows)]
    if mode == "per-project-cli" {
        mode = fs::read_to_string(".fixture-mode")?.trim().to_owned();
        if matches!(std::env::args().nth(2).as_deref(), Some("index" | "sync")) {
            if matches!(mode.as_str(), "cli-memory" | "cli-hang") {
                let root = std::env::current_dir()?;
                let marker = root.parent().unwrap().join(format!(
                    "{}.wrote",
                    root.file_name().unwrap().to_string_lossy()
                ));
                return fail_after_write(
                    if mode == "cli-memory" {
                        "write-memory-denial"
                    } else {
                        "write-hang"
                    },
                    &marker,
                );
            }
            println!("owned fixture finite sync complete");
            return Ok(());
        }
    }
    if mode == "--linger" {
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    if mode == "watch-failure" {
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(300));
            // Deliberately split a failure across OS pipe writes.
            eprint!("[CodeGraph MCP] Auto-sync er");
            io::stderr().flush().unwrap();
            std::thread::sleep(Duration::from_millis(20));
            eprintln!("ror: fixture partial generation");
        });
    }
    if mode == "cli-success" {
        println!("fixture completed");
        eprintln!("fixture warning retained");
        return Ok(());
    }
    if mode == "cli-failure" {
        eprintln!("fixture partial write failed");
        std::process::exit(7);
    }
    let _descendant = if mode == "descendant" {
        Some(
            std::process::Command::new(std::env::current_exe()?)
                .arg("--linger")
                .spawn()?,
        )
    } else {
        None
    };
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line?)?;
        let Some(id) = request.get("id") else {
            continue;
        };
        let result = if request["method"] == "initialize" {
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"codegraph-fixture","version":"1.6.0"}})
        } else {
            match mode.as_str() {
                #[cfg(windows)]
                "write-hang" | "write-memory-denial" => {
                    return fail_after_write(&mode, std::path::Path::new("owned-worker-wrote"));
                }
                "hang" => {
                    fs::write("owned-worker-started", b"waiting for cancellation")?;
                    std::thread::sleep(Duration::from_secs(60));
                    json!({})
                }
                "oversized" => {
                    println!("{}", "x".repeat(262145));
                    io::stdout().flush()?;
                    continue;
                }
                "malformed" => {
                    println!("{{broken json");
                    io::stdout().flush()?;
                    continue;
                }
                "wrong-id" => {
                    println!("{}", json!({"jsonrpc":"2.0","id":999999,"result":{}}));
                    io::stdout().flush()?;
                    continue;
                }
                "duplicate-key" => {
                    println!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{}},\"result\":{{}}}}");
                    io::stdout().flush()?;
                    continue;
                }
                "error" => {
                    eprintln!("fixture diagnostic: upstream operation failed");
                    json!({"isError":true,"content":[{"type":"text","text":"fixture failure"}]})
                }
                "large" => {
                    let count = fs::read_to_string("owned-query-count")
                        .ok()
                        .and_then(|text| text.parse::<u64>().ok())
                        .unwrap_or(0);
                    fs::write("owned-query-count", (count + 1).to_string())?;
                    json!({"content":[{"type":"text","text":format!("{}TAIL_ORACLE", "source λ\\n".repeat(3000))}],"warnings":["approximate edges"]})
                }
                "empty" => json!({}),
                "cli-pending" => {
                    fs::write("owned-pending.rs", "pub fn pending_source() {}\n")?;
                    std::thread::sleep(Duration::from_millis(150));
                    json!({"content":[{"type":"text","text":"saved graph answer; source changed during this query"}]})
                }
                "fanout" => {
                    json!({"content":[{"type":"text","text":"**Callers of entry — 3 distinct definitions (narrow with file)**\n\n**entry** — src/a.rs\n- caller_a\n\n**entry** — src/b.rs\n- caller_b\n\n**entry** — src/c.rs\n- caller_c"}]})
                }
                _ => {
                    json!({"content":[{"type":"text","text":format!("query={} — λ",request["params"])}]})
                }
            }
        };
        println!("{}", json!({"jsonrpc":"2.0","id":id,"result":result}));
        io::stdout().flush()?;
    }
    Ok(())
}

#[cfg(windows)]
fn fail_after_write(mode: &str, marker: &std::path::Path) -> io::Result<()> {
    let directory = std::env::var_os("CODEGRAPH_DIR")
        .ok_or_else(|| io::Error::other("explicit owned database required"))?;
    let database = std::env::current_dir()?
        .join(directory)
        .join("codegraph.db");
    harness_core::codegraph_store::exec(
        &database,
        "CREATE TABLE IF NOT EXISTS files(path TEXT); INSERT INTO files(path) VALUES('partial-refresh.rs')",
    )?;
    fs::write(marker, b"partial write observed")?;
    if mode == "write-memory-denial" {
        let mut allocation = Vec::<u8>::new();
        if allocation
            .try_reserve_exact(3usize * 1024 * 1024 * 1024)
            .is_err()
        {
            eprintln!(
                "owned fixture: Windows denied committed allocation above the 2 GiB Job limit"
            );
            std::process::exit(137);
        }
        return Err(io::Error::other("memory limit did not deny the allocation"));
    }
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}
