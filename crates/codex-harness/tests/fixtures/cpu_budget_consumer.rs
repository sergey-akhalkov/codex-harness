//! Synthetic CPU workload for shared account CPU budget acceptance.
//!
//! The launcher forwards its own environment to the payload, so this fixture is
//! configured entirely through `HARNESS_CPU_FIXTURE_*` variables and ignores
//! its arguments. The `tree` role starts one `leaf` grandchild. Each process
//! reports its own kernel membership against the named account group before it
//! spins, so membership is observed from the start of its execution and needs
//! no private identity. Nothing here is a product entry point.
//!
//! Compiled with rustc by the native-launcher acceptance test; no dependencies.
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

// Small independent native oracles, intentionally not the implementation's job
// query wrappers. A query-only handle can neither terminate nor assign.
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
    fn IsProcessInJob(
        process: *mut std::ffi::c_void,
        job: *mut std::ffi::c_void,
        result: *mut i32,
    ) -> i32;
    fn OpenJobObjectW(access: u32, inherit: i32, name: *const u16) -> *mut std::ffi::c_void;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

const JOB_OBJECT_QUERY: u32 = 0x0004;
const READY_TIMEOUT: Duration = Duration::from_secs(20);

fn number(name: &str, fallback: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(fallback)
}

fn flag(name: &str, fallback: bool) -> bool {
    match env::var(name).ok().as_deref() {
        None => fallback,
        Some(value) => !matches!(value.trim(), "0" | "false" | "no"),
    }
}

/// Membership of this process in any job at all.
fn in_any_job() -> bool {
    let mut member = 0;
    unsafe {
        IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut member) != 0 && member != 0
    }
}

/// Membership of this process in exactly the named job object, read with a
/// right that grants neither termination nor assignment authority.
fn in_named_job(name: &str) -> bool {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    unsafe {
        let handle = OpenJobObjectW(JOB_OBJECT_QUERY, 0, wide.as_ptr());
        if handle.is_null() {
            return false;
        }
        let mut member = 0;
        let queried = IsProcessInJob(GetCurrentProcess(), handle, &mut member);
        CloseHandle(handle);
        queried != 0 && member != 0
    }
}

fn report(path: &Path, text: &str) -> io::Result<()> {
    let staging = path.with_extension("pending");
    fs::write(&staging, text)?;
    fs::rename(staging, path)
}

fn append(path: &Path, text: &str) {
    use std::io::Write;
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{text}");
    }
}

/// Saturating CPU load: one spinning thread per requested thread count.
fn spin(millis: u64, threads: u64) {
    let workers: Vec<_> = (0..threads)
        .map(|_| {
            thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_millis(millis);
                let mut work = 1u64;
                while Instant::now() < deadline {
                    for _ in 0..10_000 {
                        work = std::hint::black_box(
                            work.wrapping_mul(6364136223846793005).wrapping_add(1),
                        );
                    }
                }
                work
            })
        })
        .collect();
    for worker in workers {
        let _ = worker.join();
    }
}

fn wait_for_file(path: &Path) -> io::Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    while !path.is_file() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{} did not appear", path.display()),
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn wait_for_child(child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "grandchild outlived its fixture deadline",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn main() -> io::Result<()> {
    let role = env::var("HARNESS_CPU_FIXTURE_ROLE").unwrap_or_else(|_| "tree".into());
    let directory = PathBuf::from(
        env::var_os("HARNESS_CPU_FIXTURE_DIR")
            .ok_or_else(|| io::Error::other("HARNESS_CPU_FIXTURE_DIR is required"))?,
    );
    fs::create_dir_all(&directory)?;
    let threads = number("HARNESS_CPU_FIXTURE_THREADS", 2).max(1);
    let spin_ms = number("HARNESS_CPU_FIXTURE_SPIN_MS", 3000);
    let exit = number("HARNESS_CPU_FIXTURE_EXIT", 0) as i32;
    let member = env::var("HARNESS_CPU_FIXTURE_JOB")
        .ok()
        .map(|name| in_named_job(&name));
    if let Some(path) = env::var_os("HARNESS_CPU_FIXTURE_STARTS") {
        append(Path::new(&path), &format!("{role} {}", std::process::id()));
    }
    let mut grandchild = None;
    if role == "tree" && flag("HARNESS_CPU_FIXTURE_LEAF", true) {
        grandchild = Some(
            Command::new(env::current_exe()?)
                .env("HARNESS_CPU_FIXTURE_ROLE", "leaf")
                .env("HARNESS_CPU_FIXTURE_LEAF", "0")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?,
        );
    }
    let member = match member {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    };
    let child = grandchild
        .as_ref()
        .map_or("null".to_owned(), |child| child.id().to_string());
    report(
        &directory.join(format!("{role}.json")),
        &format!(
            "{{\"role\":\"{role}\",\"pid\":{pid},\"in_shared\":{member},\"in_any_job\":{any},\"threads\":{threads},\"spin_ms\":{spin_ms},\"child\":{child}}}\n",
            pid = std::process::id(),
            any = in_any_job(),
        ),
    )?;
    if grandchild.is_some() {
        wait_for_file(&directory.join("leaf.json"))?;
    }
    spin(spin_ms, threads);
    if let Some(child) = grandchild.as_mut() {
        wait_for_child(child)?;
    }
    std::process::exit(exit);
}
