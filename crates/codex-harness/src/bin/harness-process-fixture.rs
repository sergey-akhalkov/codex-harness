//! Model-free owned native process fixture. Arguments: ROLE ARTIFACT [ARGS...].
//! All mutations are confined to the explicitly supplied test artifacts.

#[cfg(windows)]
mod fixture {
    use harness_core::process::{
        Cancellation, CommandSpec, Deadline, ExclusiveFileLock, Job, Limits,
    };
    use serde_json::json;
    use std::ffi::c_void;
    use std::io;
    use std::path::Path;
    use std::time::{Duration, Instant};

    // Small independent native oracles, intentionally not the implementation's
    // job-query/memory wrappers. No additional CLI dependency is required.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn IsProcessInJob(process: *mut c_void, job: *mut c_void, result: *mut i32) -> i32;
        fn VirtualAlloc(
            address: *const c_void,
            bytes: usize,
            kind: u32,
            protection: u32,
        ) -> *mut c_void;
        fn VirtualFree(address: *mut c_void, bytes: usize, kind: u32) -> i32;
    }

    fn record(path: &Path, value: serde_json::Value) -> io::Result<()> {
        let staging = path.with_extension("pending");
        std::fs::write(&staging, serde_json::to_vec(&value)?)?;
        std::fs::rename(staging, path)
    }

    fn pause() -> ! {
        // A fixture has its own finite escape hatch even if its test is broken.
        std::thread::sleep(Duration::from_secs(45));
        std::process::exit(99);
    }

    fn ready(path: &Path) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !path.is_file() {
            if Instant::now() >= deadline {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "fixture readiness"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn child_command(role: &str, artifact: &Path) -> io::Result<CommandSpec> {
        let mut command = CommandSpec::new(std::env::current_exe()?);
        command.args = vec![role.into(), artifact.into()];
        Ok(command)
    }

    pub fn run() -> io::Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let role = args.first().and_then(|v| v.to_str()).unwrap_or("");
        let artifact = Path::new(
            args.get(1)
                .ok_or_else(|| io::Error::other("ROLE ARTIFACT required"))?,
        );
        match role {
            "streams" => {
                use std::io::{Read, Write};
                let mut input = Vec::new();
                std::io::stdin().read_to_end(&mut input)?;
                std::io::stdout().write_all(&input)?;
                std::io::stderr().write_all(b"separate stderr\n")?;
                record(
                    artifact,
                    json!({"pid": std::process::id(), "custom": std::env::var("HARNESS_PROCESS_STREAM_TEST").ok(), "system_root": std::env::var("SystemRoot").ok()}),
                )?;
                std::process::exit(23);
            }
            "exit-code" => {
                let code: u32 = args
                    .get(2)
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| io::Error::other("exit code required"))?
                    .parse()
                    .map_err(io::Error::other)?;
                record(artifact, json!({"pid": std::process::id(), "exit": code}))?;
                std::process::exit(code as i32);
            }
            "report" | "hold" => {
                if role == "report" {
                    use std::io::Read;
                    let mut input = String::new();
                    std::io::stdin().read_to_string(&mut input)?;
                    if !input.is_empty() {
                        return Err(io::Error::other("expected stdin EOF"));
                    }
                    println!("fixture stdout");
                    eprintln!("fixture stderr");
                }
                let mut in_job = 0;
                if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) }
                    == 0
                {
                    return Err(io::Error::last_os_error());
                }
                record(
                    artifact,
                    json!({"pid": std::process::id(), "in_job": in_job != 0,
                    "args": args.iter().skip(2).map(|s| s.to_string_lossy()).collect::<Vec<_>>(), "cwd": std::env::current_dir()?, "system_root": std::env::var("SystemRoot").ok()}),
                )?;
                if role == "hold" {
                    pause();
                }
                std::process::exit(17);
            }
            "tree-exit" | "tree-hold" => {
                let grandchild_artifact = artifact.with_extension("grandchild.json");
                // Standard Rust spawn is the oracle for inherited containment.
                let mut child = std::process::Command::new(std::env::current_exe()?)
                    .arg("hold")
                    .arg(&grandchild_artifact)
                    .spawn()?;
                ready(&grandchild_artifact)?;
                record(
                    artifact,
                    json!({"pid": std::process::id(), "grandchild": child.id()}),
                )?;
                if role == "tree-hold" {
                    let _ = child.wait();
                }
                // This root deliberately exits while its child remains alive.
                std::process::exit(17);
            }
            "owner-suspended" | "owner-running" | "owner-exit" => {
                let job = Job::new(Limits::default())?;
                let child_artifact = artifact.with_extension("child.json");
                let suspended = job.spawn_suspended(&child_command("hold", &child_artifact)?)?;
                let identity = suspended.process().identity();
                if role == "owner-suspended" {
                    record(
                        artifact,
                        json!({"pid": identity.pid, "creation_time": identity.creation_time}),
                    )?;
                    pause();
                }
                let _child = suspended.resume()?;
                ready(&child_artifact)?;
                record(
                    artifact,
                    json!({"pid": identity.pid, "creation_time": identity.creation_time}),
                )?;
                if role == "owner-exit" {
                    ready(&artifact.with_extension("exit"))?;
                    // Exit bypasses Rust destructors: the OS must close the sole
                    // non-inherited job handle and terminate its members.
                    std::process::exit(0);
                }
                pause();
            }
            "allocate" | "allocate-hold" => {
                let requested: usize = args
                    .get(2)
                    .and_then(|s| s.to_str())
                    .unwrap_or("128")
                    .parse()
                    .map_err(io::Error::other)?;
                if requested > 256 {
                    return Err(io::Error::other(
                        "fixture allocation exceeds 256 MiB ceiling",
                    ));
                }
                let mut pages = Vec::new();
                let mut error = None;
                for _ in 0..requested / 4 {
                    let page =
                        unsafe { VirtualAlloc(std::ptr::null(), 4 * 1024 * 1024, 0x3000, 4) };
                    if page.is_null() {
                        error = io::Error::last_os_error().raw_os_error();
                        break;
                    }
                    unsafe {
                        std::ptr::write_volatile(page.cast::<u8>(), 1);
                    }
                    pages.push(page);
                }
                record(
                    artifact,
                    json!({"pid": std::process::id(), "allocated_mib": pages.len() * 4, "error": error}),
                )?;
                if role == "allocate-hold" {
                    pause();
                }
                // Leave time for the independent native memory snapshot before
                // successful allocations are released; release gate is owned.
                ready(&artifact.with_extension("release"))?;
                for page in pages {
                    unsafe {
                        VirtualFree(page, 0, 0x8000);
                    }
                }
            }
            "cpu" => {
                let start = Instant::now();
                let mut work = 1u64;
                while start.elapsed() < Duration::from_secs(3) {
                    for _ in 0..10000 {
                        work = std::hint::black_box(
                            work.wrapping_mul(6364136223846793005).wrapping_add(1),
                        );
                    }
                }
                record(
                    artifact,
                    json!({"pid": std::process::id(), "wall_ms": start.elapsed().as_millis(), "work": work}),
                )?;
            }
            "lock-try" | "lock-wait" | "lock-hold" => {
                let lock_path = Path::new(
                    args.get(2)
                        .ok_or_else(|| io::Error::other("lock path required"))?,
                );
                if role == "lock-try" {
                    let lock = ExclusiveFileLock::try_acquire(lock_path)?;
                    record(artifact, json!({"acquired": lock.is_some()}))?;
                } else {
                    let _lock = ExclusiveFileLock::acquire(
                        lock_path,
                        Deadline::after(Duration::from_secs(10))?,
                        &Cancellation::default(),
                    )?;
                    record(artifact, json!({"acquired": true}))?;
                    if role == "lock-hold" {
                        pause();
                    }
                }
            }
            _ => return Err(io::Error::other("unknown fixture role")),
        }
        Ok(())
    }
}

fn main() {
    #[cfg(windows)]
    if let Err(error) = fixture::run() {
        eprintln!("process fixture: {error}");
        std::process::exit(2);
    }
    #[cfg(not(windows))]
    {
        eprintln!("process fixture requires Windows 10+");
        std::process::exit(2);
    }
}
