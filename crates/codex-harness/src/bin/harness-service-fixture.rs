//! Owned integration fixture for native independent service startup. No global state.
#[cfg(windows)]
fn run() -> std::io::Result<()> {
    use harness_core::{
        process::{Cancellation, Deadline, Limits},
        process_service::{self, CREATE_ARGUMENT, RUN_ARGUMENT, ServiceGuard},
    };
    use std::{
        collections::BTreeMap,
        fs,
        io::{self, Read, Write},
        path::PathBuf,
        time::{Duration, Instant},
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(CREATE_ARGUMENT) if args.len() == 1 => {
            // Owned test marker simulates a stuck native helper before COM.
            // The production helper has no test modes or ambient overrides.
            if std::env::current_dir()?.join("hold-helper").is_file() {
                fs::write("helper-started", std::process::id().to_string())?;
                std::thread::sleep(Duration::from_secs(120));
            }
            process_service::create_helper_entry()
        }
        Some(RUN_ARGUMENT) if args.len() >= 4 => {
            let until = args[1]
                .parse()
                .map_err(|_| io::Error::other("invalid fixture deadline"))?;
            let mut guard = ServiceGuard::enter(
                until,
                &args[2],
                Limits {
                    memory_bytes: Some(256 * 1024 * 1024),
                    cpu_percent: Some(25.0),
                },
            )?;
            let mode = &args[3];
            let root = std::env::current_dir()?;
            let log = fs::File::create_new(root.join("service.log"))?;
            guard.redirect_standard_streams(&log)?;
            println!("Owned service Unicode log: проверка 日本");
            eprintln!("Owned service stderr: проверка 日本");
            let mut empty = [0u8; 1];
            assert_eq!(io::stdin().read(&mut empty)?, 0);
            if !["serve", "never-ready", "never-ready-locked-stderr", "crash"]
                .contains(&mode.as_str())
            {
                guard.exit(93);
            }
            let executable = std::env::current_exe()?;
            let mut child = std::process::Command::new(&executable)
                .arg("--leaf")
                .current_dir(&root)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            let snapshot = guard.job().snapshot()?;
            let endpoint = serde_json::json!({"pid":std::process::id(), "child":child.id(),
                "arguments":args[4..], "cwd":root, "sentinel":std::env::var("HARNESS_SERVICE_FIXTURE_SENTINEL").ok(),
                "ambient":std::env::var("HARNESS_SERVICE_AMBIENT_ONLY").ok(), "job":snapshot});
            fs::write(root.join("service.json"), serde_json::to_vec(&endpoint)?)?;
            if mode == "never-ready-locked-stderr" {
                // Fixture-only escape hatch for the failing watchdog baseline.
                // Exit 99 is never an accepted service outcome.
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_secs(10));
                    std::process::exit(99);
                });
                let stderr = io::stderr();
                let _locked = stderr.lock();
                std::thread::sleep(Duration::from_secs(120));
                guard.exit(92);
            }
            if mode == "never-ready" {
                std::thread::sleep(Duration::from_secs(120));
                guard.exit(92);
            }
            if mode == "crash" {
                while !root.join("crash-now").exists() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                panic!("owned startup crash before readiness");
            }
            fs::write(root.join("endpoint.json"), serde_json::to_vec(&endpoint)?)?;
            guard.mark_ready()?;
            let stop = Instant::now() + Duration::from_secs(20);
            let mut handled = String::new();
            while Instant::now() < stop && !root.join("stop").exists() {
                if let Ok(request) = fs::read_to_string(root.join("request"))
                    && request != handled
                {
                    fs::write(root.join("response"), &request)?;
                    handled = request;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            // Deliberately leave the child alive: the self-owned Job must reclaim it.
            let _ = child.try_wait();
            guard.exit(0);
        }
        Some("--leaf") => {
            std::thread::sleep(Duration::from_secs(120));
        }
        Some("--start") if args.len() >= 3 => {
            let root = PathBuf::from(&args[1]);
            let mut environment = BTreeMap::new();
            for name in ["SystemRoot", "TEMP", "TMP"] {
                if let Ok(value) = std::env::var(name) {
                    environment.insert(name.into(), value);
                }
            }
            environment.insert(
                "HARNESS_SERVICE_FIXTURE_SENTINEL".into(),
                "owned-private-значение-日本".into(),
            );
            let mode = &args[2];
            let timeout = if mode.starts_with("never-ready") {
                4
            } else {
                30
            };
            let service = process_service::spawn(
                &std::env::current_exe()?,
                &root,
                args[2..].to_vec(),
                environment,
                Deadline::after(Duration::from_secs(timeout))?,
                &Cancellation::default(),
            )?;
            println!(
                "{}",
                serde_json::json!({"pid":service.identity().pid, "creation_time":service.identity().creation_time})
            );
            io::stdout().flush()?;
            // The test kills this client Job while its independently owned service runs.
            let mut input = Vec::new();
            io::stdin().take(1).read_to_end(&mut input)?;
        }
        _ => return Err(io::Error::other("invalid service fixture arguments")),
    }
    Ok(())
}
fn main() {
    #[cfg(windows)]
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(91);
    }
    #[cfg(not(windows))]
    std::process::exit(2);
}
