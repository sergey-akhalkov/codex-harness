#![cfg(windows)]

use harness_core::core_check::{CpuBudgetStatus, inspect_cpu_budget};
use harness_core::process::{CommandSpec, Job, Limits, SharedCpuBudget};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

fn assert_inspection_is_idle(status: &CpuBudgetStatus) {
    assert_eq!(status.action, "inspected");
    assert!(!status.wrote_policy);
    assert!(!status.created_job);
    assert_eq!(status.model_calls, 0);
    assert_eq!(status.measured_consumption, "not-sampled");
    assert!(
        status.summary.contains("not measured consumption")
            || status.effective.detail.contains("not measured consumption"),
        "readback must not be presented as measured enforcement: {}",
        status.summary
    );
}

fn sleeper_template() -> &'static Path {
    static COMPILED: OnceLock<PathBuf> = OnceLock::new();
    COMPILED.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("cpu-status-sleeper-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("sleeper.rs");
        fs::write(
            &source,
            r#"fn main() {
    let mut args = std::env::args().skip(1);
    let ready = args.next().expect("ready");
    let stop = args.next().expect("stop");
    std::fs::write(ready, b"ready").expect("ready");
    for _ in 0..2400 {
        if std::path::Path::new(&stop).exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
"#,
        )
        .unwrap();
        let output = dir.join("sleeper.exe");
        let log = dir.join("sleeper.err");
        let status = Command::new("rustc")
            .arg(&source)
            .args(["--edition=2024", "-o"])
            .arg(&output)
            .stderr(fs::File::create(&log).unwrap())
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(
            status.success(),
            "sleeper compile failed: {}",
            fs::read_to_string(&log).unwrap_or_default()
        );
        output
    })
}

struct World {
    _root: tempfile::TempDir,
    account: PathBuf,
    bin: PathBuf,
}

impl World {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("cpu-status-Юникод-")
            .tempdir()
            .unwrap();
        let account = root.path().join("account");
        fs::create_dir_all(&account).unwrap();
        Self {
            bin: root.path().join("bin"),
            account,
            _root: root,
        }
    }

    fn copy(&self, name: &str) -> PathBuf {
        fs::create_dir_all(&self.bin).unwrap();
        let path = self.bin.join(name);
        fs::copy(sleeper_template(), &path).unwrap();
        path
    }

    fn write_policy(&self, ceiling: f64) -> Vec<u8> {
        let body = serde_json::to_vec_pretty(&serde_json::json!({
            "schema": 1,
            "ceiling_percent": ceiling,
            "escape_hatch": "CODEX_HARNESS_CPU_PERCENT",
            "note": "synthetic account ceiling"
        }))
        .unwrap();
        fs::write(self.account.join("shared-cpu-policy.json"), &body).unwrap();
        body
    }
}

struct Outside {
    child: Child,
    stop: PathBuf,
}

impl Drop for Outside {
    fn drop(&mut self) {
        let _ = fs::write(&self.stop, b"stop");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Admitted {
    _lifecycle: Job,
    _process: harness_core::process::OwnedProcess,
    stop: PathBuf,
    pid: u32,
}

impl Drop for Admitted {
    fn drop(&mut self) {
        let _ = fs::write(&self.stop, b"stop");
    }
}

fn wait_ready(path: &Path) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        if path.is_file() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("sleeper did not become ready: {}", path.display());
}

fn spawn_outside(exe: &Path, marker: bool) -> Outside {
    let ready = exe.with_extension("ready");
    let stop = exe.with_extension("stop");
    let _ = fs::remove_file(&ready);
    let _ = fs::remove_file(&stop);
    let mut command = Command::new(exe);
    command
        .arg(&ready)
        .arg(&stop)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if marker {
        command.arg("--harness-cpu-exception-launch");
    }
    let child = command.spawn().unwrap();
    wait_ready(&ready);
    Outside { child, stop }
}

fn admit(budget: &SharedCpuBudget, exe: &Path, marker: bool) -> Admitted {
    let ready = exe.with_extension("ready");
    let stop = exe.with_extension("stop");
    let _ = fs::remove_file(&ready);
    let _ = fs::remove_file(&stop);
    let mut spec = CommandSpec::new(exe);
    spec.args.push(ready.as_os_str().to_os_string());
    spec.args.push(stop.as_os_str().to_os_string());
    if marker {
        spec.args.push("--harness-cpu-exception-launch".into());
    }
    spec.current_dir = exe.parent().map(Path::to_path_buf);
    let lifecycle = Job::new(Limits::default()).unwrap();
    let process = budget.spawn(&lifecycle, &spec).unwrap();
    wait_ready(&ready);
    let pid = process.identity().pid;
    Admitted {
        _lifecycle: lifecycle,
        _process: process,
        stop,
        pid,
    }
}

fn route<'a>(
    status: &'a CpuBudgetStatus,
    name: &str,
) -> &'a harness_core::core_check::RouteCoverage {
    status
        .routes
        .iter()
        .find(|item| item.path.ends_with(name))
        .unwrap_or_else(|| panic!("missing route {name}: {status:?}"))
}

fn names(dir: &Path) -> Vec<String> {
    let mut found = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    found.sort();
    found
}

#[test]
fn inspection_does_not_mutate_policy_or_create_a_job() {
    let world = World::new();
    let policy = world.write_policy(75.0);
    let before = names(&world.account);
    let status = inspect_cpu_budget(&world.account, &[]);
    assert_inspection_is_idle(&status);
    assert_eq!(
        fs::read(world.account.join("shared-cpu-policy.json")).unwrap(),
        policy
    );
    assert_eq!(names(&world.account), before);
    assert!(!world.account.join("cpu-budget.json").exists());
    assert!(!status.effective.opened);
    assert!(status.effective.detail.contains("did not create a job"));
    assert_eq!(status.configured.source, "policy-record");
    assert_eq!(status.configured.ceiling_percent, Some(75.0));

    let missing = world._root.path().join("missing-account");
    let absent = inspect_cpu_budget(&missing, &[]);
    assert_inspection_is_idle(&absent);
    assert!(!missing.exists());
    assert!(!absent.created_job);

    let relative = inspect_cpu_budget(Path::new("relative-cpu-account"), &[]);
    assert_inspection_is_idle(&relative);
    assert!(!Path::new("relative-cpu-account").exists());
}

#[test]
fn configured_ceiling_is_distinct_from_effective_kernel_readback() {
    let world = World::new();
    let policy = world.write_policy(40.0);
    let budget = SharedCpuBudget::acquire(&world.account, 75.0).unwrap();
    let covered = world.copy("covered.exe");
    let admitted = admit(&budget, &covered, false);
    let snapshot = budget.snapshot().unwrap();
    let routes = [covered];
    let status = inspect_cpu_budget(&world.account, &routes);
    assert_inspection_is_idle(&status);
    assert_eq!(status.configured.source, "policy-record");
    assert_eq!(status.configured.ceiling_percent, Some(40.0));
    assert_eq!(status.effective.source, "kernel-readback");
    assert_eq!(status.effective.cpu_rate, Some(snapshot.cpu_rate));
    assert_eq!(status.effective.cpu_rate, Some(7500));
    assert_eq!(status.effective.cpu_hard_cap, Some(snapshot.cpu_hard_cap));
    assert_eq!(status.effective.cpu_hard_cap, Some(true));
    assert_eq!(
        status.effective.active_processes,
        Some(snapshot.active_processes)
    );
    let after = budget.snapshot().unwrap();
    assert_eq!(after.cpu_rate, snapshot.cpu_rate);
    assert_eq!(after.cpu_hard_cap, snapshot.cpu_hard_cap);
    assert_eq!(after.active_processes, snapshot.active_processes);
    assert!(status.effective.active_processes.unwrap_or(0) >= 1);
    assert!(status.effective.detail.contains("not measured consumption"));
    assert_ne!(status.enforcement, "capped");
    assert_eq!(
        fs::read(world.account.join("shared-cpu-policy.json")).unwrap(),
        policy
    );
    assert!(
        route(&status, "covered.exe")
            .covered_pids
            .contains(&admitted.pid)
    );
    drop(admitted);
    drop(budget);
}

#[test]
fn covered_and_uncovered_routes_are_identified() {
    let world = World::new();
    world.write_policy(75.0);
    let budget = SharedCpuBudget::acquire(&world.account, 75.0).unwrap();
    let covered = world.copy("covered.exe");
    let uncovered = world.copy("uncovered.exe");
    let admitted = admit(&budget, &covered, false);
    let outside = spawn_outside(&uncovered, false);
    let status = inspect_cpu_budget(&world.account, &[covered, uncovered]);
    assert_inspection_is_idle(&status);
    let covered_route = route(&status, "covered.exe");
    let uncovered_route = route(&status, "uncovered.exe");
    assert_eq!(covered_route.state, "covered");
    assert!(covered_route.covered_pids.contains(&admitted.pid));
    assert!(!covered_route.covered_pids.contains(&outside.child.id()));
    assert_eq!(uncovered_route.state, "uncovered");
    assert!(uncovered_route.outside_pids.contains(&outside.child.id()));
    assert!(!uncovered_route.covered_pids.contains(&outside.child.id()));
    drop(admitted);
    drop(budget);
}

#[test]
fn explicit_exception_is_visible_and_not_counted_covered() {
    let world = World::new();
    world.write_policy(75.0);
    let budget = SharedCpuBudget::acquire(&world.account, 75.0).unwrap();
    let exception = world.copy("exception.exe");
    let marked = world.copy("marked-member.exe");
    let outside = spawn_outside(&exception, true);
    let admitted = admit(&budget, &marked, true);
    let status = inspect_cpu_budget(&world.account, &[exception, marked]);
    assert_inspection_is_idle(&status);
    assert_eq!(status.command_line_observation, "available");
    assert!(
        status
            .exceptions
            .iter()
            .any(|item| item.pid == outside.child.id()
                && item.detail.contains("not counted as covered")),
        "{:?}",
        status.exceptions
    );
    assert!(
        !status
            .exceptions
            .iter()
            .any(|item| item.pid == admitted.pid)
    );
    assert!(
        status
            .rejected_exception_markers
            .iter()
            .any(|item| item.pid == admitted.pid
                && item.detail.contains("not an uncapped exception"))
    );
    assert_eq!(route(&status, "exception.exe").state, "exception");
    assert!(
        !route(&status, "exception.exe")
            .covered_pids
            .contains(&outside.child.id())
    );
    assert!(
        route(&status, "marked-member.exe")
            .covered_pids
            .contains(&admitted.pid)
    );
    assert!(
        !status
            .degraded_starts
            .iter()
            .any(|item| item.pid == outside.child.id())
    );
    drop(admitted);
    drop(budget);
}

#[test]
fn degraded_start_is_visible_and_distinct_from_an_explicit_exception() {
    let world = World::new();
    world.write_policy(75.0);
    let budget = SharedCpuBudget::acquire(&world.account, 75.0).unwrap();
    let degraded = world.copy("degraded.exe");
    let outside = spawn_outside(&degraded, false);
    let status = inspect_cpu_budget(&world.account, &[degraded]);
    assert_inspection_is_idle(&status);
    assert_eq!(status.command_line_observation, "available");
    let sighting = status
        .degraded_starts
        .iter()
        .find(|item| item.pid == outside.child.id())
        .unwrap_or_else(|| panic!("missing degraded start: {:?}", status.degraded_starts));
    assert!(
        sighting
            .detail
            .contains("not an explicit uncapped invocation")
    );
    assert!(
        !status
            .exceptions
            .iter()
            .any(|item| item.pid == outside.child.id())
    );
    assert!(
        !route(&status, "degraded.exe")
            .covered_pids
            .contains(&outside.child.id())
    );
    assert_eq!(route(&status, "degraded.exe").state, "uncovered");
    drop(budget);
}

#[test]
fn unknown_member_is_reported_and_not_counted_covered() {
    let world = World::new();
    world.write_policy(75.0);
    let budget = SharedCpuBudget::acquire(&world.account, 75.0).unwrap();
    let unknown = world.copy("unknown.exe");
    let idle = world.copy("idle-route.exe");
    let admitted = admit(&budget, &unknown, false);
    let status = inspect_cpu_budget(&world.account, &[idle]);
    assert_inspection_is_idle(&status);
    assert!(
        status
            .unknown_members
            .iter()
            .any(|item| item.pid == admitted.pid && item.detail.contains("not counted as covered")),
        "{:?}",
        status.unknown_members
    );
    assert!(
        !status
            .routes
            .iter()
            .any(|item| item.covered_pids.contains(&admitted.pid))
    );
    assert_eq!(route(&status, "idle-route.exe").state, "not_running");
    drop(admitted);
    drop(budget);
}

#[test]
fn restart_boundary_names_the_process_and_does_not_terminate_it() {
    let world = World::new();
    world.write_policy(75.0);
    let budget = SharedCpuBudget::acquire(&world.account, 75.0).unwrap();
    let uncovered = world.copy("restart.exe");
    let mut outside = spawn_outside(&uncovered, false);
    let status = inspect_cpu_budget(&world.account, &[uncovered]);
    assert_inspection_is_idle(&status);
    let pid = outside.child.id().to_string();
    assert!(
        status.restart_boundary.contains(&pid),
        "{}",
        status.restart_boundary
    );
    assert!(
        status
            .restart_boundary
            .contains("restart that process after its work can stop"),
        "{}",
        status.restart_boundary
    );
    assert!(
        status
            .restart_boundary
            .contains("inspection does not terminate"),
        "{}",
        status.restart_boundary
    );
    assert!(status.restart_boundary.contains("restart.exe"));
    assert!(outside.child.try_wait().unwrap().is_none());
    assert_eq!(status.activation, "incomplete");
    drop(budget);
}
