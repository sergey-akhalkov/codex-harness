//! Native acceptance. Build the workspace fixture first:
//! cargo build --locked -p codex-harness --bin harness-process-fixture
//! cargo test --locked -p harness-core --test process -- --test-threads=1 --nocapture
//! HARNESS_PROCESS_FIXTURE may select an explicitly built fixture executable.
//! Artifacts are retained under the printed owned temporary directories.

use harness_core::process::{Cancellation, Deadline, ExclusiveFileLock};
use std::time::Duration;

#[test]
fn lock_deadline_and_cancellation_are_bounded() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("lock");
    std::fs::write(&path, "preserve").unwrap();
    let lock = ExclusiveFileLock::try_acquire(&path).unwrap().unwrap();
    let start = std::time::Instant::now();
    let error = ExclusiveFileLock::acquire(
        &path,
        Deadline::after(Duration::from_millis(50)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(2));
    let cancelled = Cancellation::default();
    cancelled.cancel();
    assert_eq!(
        ExclusiveFileLock::acquire(
            &path,
            Deadline::after(Duration::from_secs(30)).unwrap(),
            &cancelled
        )
        .unwrap_err()
        .kind(),
        std::io::ErrorKind::Interrupted
    );
    drop(lock);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "preserve");
    assert!(ExclusiveFileLock::try_acquire(&path).unwrap().is_some());
    assert!(Deadline::after(Duration::MAX).is_err());
}

#[cfg(not(windows))]
#[test]
fn jobs_explicitly_reject_non_windows() {
    assert_eq!(
        harness_core::process::Job::new(Default::default())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::Unsupported
    );
}

#[cfg(windows)]
mod native {
    use super::*;
    use harness_core::process::{
        CommandSpec, Job, Limits, OwnedProcess, ProcessIdentity, StopReason,
    };
    use serde_json::{Value, json};
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::Instant;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::Threading::*;

    const CLEANUP: Duration = Duration::from_secs(5);

    // Independent psapi.h layout; only PrivateUsage is used as committed
    // private memory, never working set or the job's attempted-charge peak.
    #[repr(C)]
    #[derive(Default)]
    struct MemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_non_paged: usize,
        quota_non_paged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private_usage: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn K32GetProcessMemoryInfo(
            process: HANDLE,
            counters: *mut MemoryCounters,
            bytes: u32,
        ) -> i32;
    }

    fn fixture() -> PathBuf {
        let path = std::env::var_os("HARNESS_PROCESS_FIXTURE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("harness-process-fixture.exe")
            });
        assert!(
            path.is_file(),
            "build the Rust fixture first: cargo build --locked -p codex-harness --bin harness-process-fixture; missing {}",
            path.display()
        );
        path.canonicalize().unwrap()
    }

    fn root(name: &str) -> PathBuf {
        let root = tempfile::Builder::new()
            .prefix(&format!("harness-rust-process-{name}-"))
            .tempdir()
            .unwrap()
            .keep();
        println!("process evidence: {}", root.display());
        root
    }

    fn spec(role: &str, artifact: &Path) -> CommandSpec {
        let mut spec = CommandSpec::new(fixture());
        spec.args = vec![role.into(), artifact.into()];
        spec
    }

    fn receipt(path: &Path) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match std::fs::read(path) {
                Ok(bytes) => {
                    return serde_json::from_slice(&bytes).expect("atomic fixture receipt");
                }
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("fixture receipt {}: {error}", path.display()),
            }
        }
    }

    fn record(path: &Path, value: Value) {
        std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }

    fn run(job: Job, child: &OwnedProcess) -> harness_core::process::Outcome {
        job.wait(
            child,
            Deadline::after(Duration::from_secs(15)).unwrap(),
            &Cancellation::default(),
            CLEANUP,
        )
        .unwrap()
    }

    /// Independent retained handle oracle, never an image-name/PID kill.
    struct Observer {
        handle: OwnedHandle,
        identity: ProcessIdentity,
    }
    impl Observer {
        fn open(pid: u32) -> Self {
            let handle = unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    pid,
                )
            };
            assert!(
                !handle.is_null(),
                "OpenProcess({pid}): {}",
                io::Error::last_os_error()
            );
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            let (mut creation, mut exit, mut kernel, mut user) = unsafe {
                (
                    std::mem::zeroed(),
                    std::mem::zeroed(),
                    std::mem::zeroed(),
                    std::mem::zeroed(),
                )
            };
            assert_ne!(
                unsafe {
                    GetProcessTimes(
                        handle.as_raw_handle(),
                        &mut creation,
                        &mut exit,
                        &mut kernel,
                        &mut user,
                    )
                },
                0
            );
            let creation_time =
                ((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64;
            Self {
                handle,
                identity: ProcessIdentity { pid, creation_time },
            }
        }
        fn alive(&self) -> bool {
            let result = unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) };
            assert!(
                result == WAIT_OBJECT_0 || result == WAIT_TIMEOUT,
                "native wait failed"
            );
            result == WAIT_TIMEOUT
        }
        fn private_bytes(&self) -> usize {
            let mut counters = MemoryCounters {
                cb: std::mem::size_of::<MemoryCounters>() as u32,
                ..Default::default()
            };
            assert_ne!(
                unsafe {
                    K32GetProcessMemoryInfo(
                        self.handle.as_raw_handle(),
                        &mut counters,
                        std::mem::size_of::<MemoryCounters>() as u32,
                    )
                },
                0,
                "native private commit query: {}",
                io::Error::last_os_error()
            );
            counters.private_usage
        }
        fn assert_dead(&self) {
            let deadline = Instant::now() + CLEANUP;
            while self.alive() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                !self.alive(),
                "owned process survived cleanup: {:?}",
                self.identity
            );
        }
    }

    struct Foreign(Child);
    impl Foreign {
        fn spawn(role: &str, artifact: &Path) -> Self {
            Self(
                Command::new(fixture())
                    .arg(role)
                    .arg(artifact)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        }
        fn kill_and_wait(&mut self) {
            self.0.kill().unwrap();
            let deadline = Instant::now() + CLEANUP;
            while self.0.try_wait().unwrap().is_none() {
                assert!(
                    Instant::now() < deadline,
                    "owned external fixture failed to exit"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    impl Drop for Foreign {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let deadline = Instant::now() + CLEANUP;
            while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    #[test]
    fn suspended_containment_quoting_cwd_and_combined_output() {
        let root = root("suspended");
        let marker = root.join("result.json");
        let job = Job::new(Limits {
            memory_bytes: Some(128 * 1024 * 1024),
            cpu_percent: Some(12.345),
        })
        .unwrap();
        let log = std::fs::File::create(root.join("combined.log")).unwrap();
        let args = [
            "",
            "space here",
            "кириллица λ",
            "quote\"middle",
            "backslash\\",
            "two\\\\\"quotes",
            "\\\\trailing\\",
        ];
        let mut command = spec("report", &marker);
        command.args.extend(args.iter().map(OsString::from));
        command.current_dir = Some(root.clone());
        command.stdout = Some(log.try_clone().unwrap());
        command.stderr = Some(log);
        let pending = job.spawn_suspended(&command).unwrap();
        let observer = Observer::open(pending.process().identity().pid);
        assert_eq!(observer.identity, pending.process().identity());
        assert!(job.contains(pending.process()).unwrap());
        assert!(job.owns(observer.identity).unwrap());
        let snapshot = job.snapshot().unwrap();
        assert_eq!(snapshot.active_processes, 1);
        assert_eq!(snapshot.memory_limit_bytes, 128 * 1024 * 1024);
        assert_eq!(snapshot.cpu_rate, 1234);
        assert!(snapshot.cpu_hard_cap && snapshot.kill_on_close && !snapshot.handle_inheritable);
        std::thread::sleep(Duration::from_millis(100));
        assert!(!marker.exists(), "child executed before resume");
        let child = pending.resume().unwrap();
        let outcome = run(job, &child);
        assert_eq!(
            (outcome.reason, outcome.exit_code, outcome.process_exit_code),
            (StopReason::Exited, 17, 17)
        );
        let data = receipt(&marker);
        assert_eq!(data["pid"], observer.identity.pid);
        assert_eq!(data["in_job"], true);
        assert_eq!(data["system_root"], json!(std::env::var("SystemRoot").ok()));
        assert_eq!(data["args"], json!(args));
        assert_eq!(
            PathBuf::from(data["cwd"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            root.canonicalize().unwrap()
        );
        let log = std::fs::read_to_string(root.join("combined.log")).unwrap();
        assert!(log.contains("fixture stdout") && log.contains("fixture stderr"));
        observer.assert_dead();
        record(
            &root.join("verified.json"),
            json!({"pid": observer.identity.pid, "creation_time": observer.identity.creation_time, "cpu_rate": snapshot.cpu_rate, "suspended_no_artifact": true, "exit": outcome.exit_code}),
        );
    }

    #[test]
    fn explicit_stdin_eof_streams_and_child_environment_are_preserved() {
        let root = root("streams");
        let marker = root.join("result.json");
        let input = "binary \0 input\nкириллица\n".as_bytes();
        std::fs::write(root.join("stdin"), input).unwrap();
        let mut command = spec("streams", &marker);
        command.stdin = Some(std::fs::File::open(root.join("stdin")).unwrap());
        command.stdout = Some(std::fs::File::create(root.join("stdout")).unwrap());
        command.stderr = Some(std::fs::File::create(root.join("stderr")).unwrap());
        command.env.insert(
            "HARNESS_PROCESS_STREAM_TEST".into(),
            Some("дочернее значение".into()),
        );
        let original = std::env::var_os("HARNESS_PROCESS_STREAM_TEST");
        let job = Job::new(Limits::default()).unwrap();
        let child = job.spawn(&command).unwrap();
        let result = job
            .wait(
                &child,
                Deadline::after(Duration::from_secs(10)).unwrap(),
                &Cancellation::default(),
                CLEANUP,
            )
            .unwrap();
        assert_eq!(result.exit_code, 23);
        assert_eq!(result.reason, StopReason::Exited);
        assert_eq!(std::fs::read(root.join("stdout")).unwrap(), input);
        assert_eq!(
            std::fs::read(root.join("stderr")).unwrap(),
            b"separate stderr\n"
        );
        let receipt = receipt(&marker);
        assert_eq!(receipt["custom"], "дочернее значение");
        assert_eq!(
            receipt["system_root"].as_str(),
            std::env::var("SystemRoot").ok().as_deref()
        );
        assert_eq!(std::env::var_os("HARNESS_PROCESS_STREAM_TEST"), original);
    }

    #[test]
    fn dropping_pending_process_stops_only_that_handle() {
        let root = root("pending-drop");
        let job = Job::new(Limits::default()).unwrap();
        let pending = job
            .spawn_suspended(&spec("hold", &root.join("never.json")))
            .unwrap();
        let observer = Observer::open(pending.process().identity().pid);
        drop(pending);
        observer.assert_dead();
        assert!(!root.join("never.json").exists());
        let other = job
            .spawn(&spec("report", &root.join("other.json")))
            .unwrap();
        assert_eq!(run(job, &other).exit_code, 17);
    }

    #[test]
    fn timeout_and_cancellation_stop_grandchildren_preserve_foreign() {
        for cancel in [false, true] {
            let root = root(if cancel { "cancel" } else { "timeout" });
            let foreign = Foreign::spawn("hold", &root.join("foreign.json"));
            receipt(&root.join("foreign.json"));
            let foreign_handle = Observer::open(foreign.0.id());
            let job = Job::new(Limits::default()).unwrap();
            let marker = root.join("tree.json");
            let child = job.spawn(&spec("tree-hold", &marker)).unwrap();
            let child_handle = Observer::open(child.identity().pid);
            let data = receipt(&marker);
            let grandchild = Observer::open(data["grandchild"].as_u64().unwrap() as u32);
            assert!(job.owns(grandchild.identity).unwrap());
            let cancellation = Cancellation::default();
            let signal = cancellation.clone();
            let thread = cancel.then(|| {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(60));
                    signal.cancel();
                })
            });
            let start = Instant::now();
            let outcome = job
                .wait(
                    &child,
                    Deadline::after(if cancel {
                        Duration::from_secs(30)
                    } else {
                        Duration::from_millis(100)
                    })
                    .unwrap(),
                    &cancellation,
                    CLEANUP,
                )
                .unwrap();
            if let Some(thread) = thread {
                thread.join().unwrap();
            }
            assert!(start.elapsed() < Duration::from_secs(6));
            assert_eq!(
                outcome.reason,
                if cancel {
                    StopReason::Cancelled
                } else {
                    StopReason::Timeout
                }
            );
            assert_eq!(outcome.exit_code, if cancel { 130 } else { 124 });
            assert_eq!(outcome.process_exit_code, outcome.exit_code);
            assert_eq!(outcome.job.active_processes, 0);
            child_handle.assert_dead();
            grandchild.assert_dead();
            assert!(foreign_handle.alive());
            record(
                &root.join("verified.json"),
                json!({"root": child.identity().pid, "grandchild": grandchild.identity.pid, "foreign": foreign_handle.identity.pid, "foreign_alive": true, "exit": outcome.exit_code, "elapsed_ms": start.elapsed().as_millis()}),
            );
        }
    }

    #[test]
    fn root_exit_and_job_drop_both_clean_surviving_grandchild() {
        for explicit_wait in [false, true] {
            let root = root("root-exit");
            let job = Job::new(Limits::default()).unwrap();
            let marker = root.join("tree.json");
            let child = job.spawn(&spec("tree-exit", &marker)).unwrap();
            let data = receipt(&marker);
            let grandchild = Observer::open(data["grandchild"].as_u64().unwrap() as u32);
            assert!(child.wait_for_exit(CLEANUP).unwrap());
            assert!(grandchild.alive());
            assert!(job.owns(grandchild.identity).unwrap());
            if explicit_wait {
                assert_eq!(run(job, &child).exit_code, 17);
            } else {
                drop(job);
            }
            grandchild.assert_dead();
            record(
                &root.join("verified.json"),
                json!({"root": child.identity().pid, "grandchild": grandchild.identity.pid, "explicit_wait": explicit_wait, "grandchild_dead": true}),
            );
        }
    }

    #[test]
    fn owner_death_before_resume_and_after_resume_closes_child_job() {
        for role in ["owner-suspended", "owner-running", "owner-exit"] {
            let root = root(role);
            let marker = root.join("owner.json");
            let mut owner = Foreign::spawn(role, &marker);
            let data = receipt(&marker);
            let identity = ProcessIdentity {
                pid: data["pid"].as_u64().unwrap() as u32,
                creation_time: data["creation_time"].as_u64().unwrap(),
            };
            let child = Observer::open(identity.pid);
            assert_eq!(child.identity, identity);
            assert!(child.alive());
            if role == "owner-suspended" {
                assert!(!marker.with_extension("child.json").exists());
            }
            if role == "owner-exit" {
                std::fs::write(marker.with_extension("exit"), "exit now").unwrap();
                let deadline = Instant::now() + CLEANUP;
                while owner.0.try_wait().unwrap().is_none() {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(10));
                }
            } else {
                owner.kill_and_wait();
            }
            child.assert_dead();
            if role == "owner-suspended" {
                assert!(!marker.with_extension("child.json").exists());
            }
            record(
                &root.join("verified.json"),
                json!({"role": role, "child": identity.pid, "creation_time": identity.creation_time, "dead": true}),
            );
        }
    }

    #[test]
    fn stale_and_foreign_identities_never_authorize_cleanup() {
        let root = root("identity");
        let job = Job::new(Limits::default()).unwrap();
        let child = job.spawn(&spec("hold", &root.join("owned.json"))).unwrap();
        receipt(&root.join("owned.json"));
        let foreign = Foreign::spawn("hold", &root.join("foreign.json"));
        receipt(&root.join("foreign.json"));
        let other = Observer::open(foreign.0.id());
        let mut mismatch = child.identity();
        mismatch.creation_time ^= 1;
        assert!(!job.owns(mismatch).unwrap());
        assert!(!job.owns(other.identity).unwrap());
        // PID zero has no provable identity; it must never become authority.
        assert!(
            !job.owns(ProcessIdentity {
                pid: 0,
                creation_time: 0
            })
            .unwrap_or(false)
        );
        assert!(child.is_running().unwrap() && other.alive());
        let snapshot = job.terminate(42, CLEANUP).unwrap();
        assert_eq!(snapshot.active_processes, 0);
        assert!(child.wait_for_exit(CLEANUP).unwrap());
        assert!(other.alive());
        let unrelated = Job::new(Limits::default()).unwrap();
        assert!(!unrelated.owns(child.identity()).unwrap());
        record(
            &root.join("verified.json"),
            json!({"owned": child.identity().pid, "mismatched_creation": mismatch.creation_time, "foreign": other.identity.pid, "foreign_alive": true}),
        );
    }

    #[test]
    fn waiting_on_another_jobs_process_is_rejected() {
        let root = root("foreign-wait");
        let first = Job::new(Limits::default()).unwrap();
        let second = Job::new(Limits::default()).unwrap();
        let child = second
            .spawn(&spec("hold", &root.join("child.json")))
            .unwrap();
        receipt(&root.join("child.json"));
        assert_eq!(
            first
                .wait(
                    &child,
                    Deadline::after(Duration::from_secs(1)).unwrap(),
                    &Cancellation::default(),
                    CLEANUP
                )
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(child.is_running().unwrap());
        second.terminate(0, CLEANUP).unwrap();
    }

    #[test]
    fn aggregate_memory_denial_and_observed_125_preserve_other_job() {
        let root = root("memory");
        let limit = 80 * 1024 * 1024;
        let job = Job::new(Limits {
            memory_bytes: Some(limit),
            cpu_percent: None,
        })
        .unwrap();
        let other_job = Job::new(Limits::default()).unwrap();
        let other = other_job
            .spawn(&spec("hold", &root.join("other.json")))
            .unwrap();
        receipt(&root.join("other.json"));
        let mut first_spec = spec("allocate-hold", &root.join("first.json"));
        first_spec.args.push("40".into());
        let first = job.spawn(&first_spec).unwrap();
        let granted = receipt(&root.join("first.json"));
        assert_eq!(granted["allocated_mib"], 40);
        assert!(granted["error"].is_null());
        let mut second_spec = spec("allocate", &root.join("second.json"));
        second_spec.args.push("64".into());
        let second = job.spawn(&second_spec).unwrap();
        let denied = receipt(&root.join("second.json"));
        assert_eq!(denied["error"], 1455); // ERROR_COMMITMENT_LIMIT, independent VirtualAlloc oracle.
        assert!(denied["allocated_mib"].as_u64().unwrap() < 40);
        assert!(denied["allocated_mib"].as_u64().unwrap() >= 16);
        assert!(job.snapshot().unwrap().active_processes >= 2);
        let snapshot = job.snapshot().unwrap();
        let committed = Observer::open(first.identity().pid).private_bytes()
            + Observer::open(second.identity().pid).private_bytes();
        assert!(
            committed <= limit,
            "aggregate private commit exceeds job limit: {committed}"
        );
        assert!(committed > 56 * 1024 * 1024);
        let outcome = run(job, &second);
        assert_eq!(
            (outcome.reason, outcome.exit_code),
            (StopReason::MemoryLimit, 125)
        );
        assert!(first.wait_for_exit(CLEANUP).unwrap());
        assert!(second.wait_for_exit(CLEANUP).unwrap());
        assert!(other.is_running().unwrap());
        other_job.terminate(0, CLEANUP).unwrap();
        record(
            &root.join("verified.json"),
            json!({"first": granted, "second": denied, "limit_bytes": limit, "committed_private_bytes": committed, "peak_job_memory_bytes": snapshot.peak_job_memory_bytes, "exit": outcome.exit_code, "other_survived": true}),
        );
    }

    #[test]
    fn cpu_hard_cap_reduces_actual_process_cpu_time() {
        let root = root("cpu");
        let capped_job = Job::new(Limits {
            memory_bytes: None,
            // 0.01% can starve Windows process initialization before this
            // fixture starts its timed workload on a loaded host. 0.1% still
            // leaves a large, independently measured gap from the control.
            cpu_percent: Some(0.1),
        })
        .unwrap();
        let free_job = Job::new(Limits::default()).unwrap();
        let capped = capped_job
            .spawn(&spec("cpu", &root.join("capped.json")))
            .unwrap();
        let free = free_job
            .spawn(&spec("cpu", &root.join("free.json")))
            .unwrap();
        let capped_result = run(capped_job, &capped);
        let free_result = run(free_job, &free);
        assert_eq!((capped_result.exit_code, free_result.exit_code), (0, 0));
        let cap_cpu = capped.cpu_time().unwrap().as_secs_f64();
        let free_cpu = free.cpu_time().unwrap().as_secs_f64();
        assert_eq!(capped_result.job.cpu_rate, 10);
        assert!(capped_result.job.cpu_hard_cap);
        record(
            &root.join("verified.json"),
            json!({"capped": receipt(&root.join("capped.json")), "free": receipt(&root.join("free.json")), "capped_cpu_seconds": cap_cpu, "free_cpu_seconds": free_cpu, "logical_cpus": std::thread::available_parallelism().unwrap().get()}),
        );
        assert!(
            free_cpu > 0.3,
            "host too busy to establish an uncapped control: {free_cpu}"
        );
        assert!(
            cap_cpu < free_cpu * 0.6 + 0.05,
            "CPU cap not observed: capped={cap_cpu}, control={free_cpu}"
        );
    }

    #[test]
    fn file_locks_serialize_processes_and_release_on_owner_death() {
        let root = root("locks");
        let path = root.join("stable.lock");
        std::fs::write(&path, "preserved content").unwrap();
        let lock = ExclusiveFileLock::try_acquire(&path).unwrap().unwrap();
        // Deletion must not split the lock domain while an owner exists.
        assert!(std::fs::remove_file(&path).is_err());
        let job = Job::new(Limits::default()).unwrap();
        let mut command = spec("lock-try", &root.join("blocked.json"));
        command.args.push(path.clone().into_os_string());
        let child = job.spawn(&command).unwrap();
        assert_eq!(run(job, &child).exit_code, 0);
        assert_eq!(receipt(&root.join("blocked.json"))["acquired"], false);
        let job = Job::new(Limits::default()).unwrap();
        let mut command = spec("lock-wait", &root.join("acquired.json"));
        command.args.push(path.clone().into_os_string());
        let child = job.spawn(&command).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!root.join("acquired.json").exists());
        drop(lock);
        assert_eq!(run(job, &child).exit_code, 0);
        assert_eq!(receipt(&root.join("acquired.json"))["acquired"], true);
        let job = Job::new(Limits::default()).unwrap();
        let mut command = spec("lock-hold", &root.join("held.json"));
        command.args.push(path.clone().into_os_string());
        let child = job.spawn(&command).unwrap();
        receipt(&root.join("held.json"));
        assert!(ExclusiveFileLock::try_acquire(&path).unwrap().is_none());
        job.terminate(0, CLEANUP).unwrap();
        assert!(child.wait_for_exit(CLEANUP).unwrap());
        assert!(ExclusiveFileLock::try_acquire(&path).unwrap().is_some());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "preserved content");
    }

    #[test]
    fn native_exit_codes_preserve_259_and_high_bits() {
        let root = root("exit-codes");
        for code in [259u32, 0xc0000005, u32::MAX] {
            let job = Job::new(Limits::default()).unwrap();
            let marker = root.join(format!("{code}.json"));
            let mut command = spec("exit-code", &marker);
            command.args.push(code.to_string().into());
            let child = job.spawn(&command).unwrap();
            let outcome = run(job, &child);
            assert_eq!(
                (outcome.reason, outcome.exit_code, outcome.process_exit_code),
                (StopReason::Exited, code, code)
            );
            assert_eq!(receipt(&marker)["exit"], code);
        }
    }

    #[test]
    fn invalid_launches_and_limits_fail_without_child_execution() {
        for cpu in [0.0, -1.0, 100.01, f64::INFINITY, f64::NAN] {
            assert!(
                Job::new(Limits {
                    memory_bytes: None,
                    cpu_percent: Some(cpu)
                })
                .is_err()
            );
        }
        assert!(
            Job::new(Limits {
                memory_bytes: Some(0),
                cpu_percent: None
            })
            .is_err()
        );
        let root = root("invalid");
        let job = Job::new(Limits::default()).unwrap();
        let mut command = spec("report", &root.join("never.json"));
        command.args.push("bad\0arg".into());
        assert_eq!(
            job.spawn(&command).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        command.args.pop();
        command.args.push("a".repeat(32767).into());
        assert_eq!(
            job.spawn(&command).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        command.args.pop();
        command.program = root.join("missing.exe");
        assert!(job.spawn(&command).is_err());
        assert_eq!(job.snapshot().unwrap().active_processes, 0);
        assert!(!root.join("never.json").exists());
        // A one-page job cannot admit the executable's initial committed
        // memory. Atomic JOB_LIST assignment must fail before any fixture code.
        let denied_job = Job::new(Limits {
            memory_bytes: Some(4096),
            cpu_percent: None,
        })
        .unwrap();
        assert!(
            denied_job
                .spawn_suspended(&spec("report", &root.join("denied.json")))
                .is_err()
        );
        // Failed native creation can return before job accounting observes the
        // kernel's aborted process teardown. Confirm bounded cleanup as well as
        // absence of fixture execution instead of requiring simultaneous signals.
        let denied_deadline = Instant::now() + CLEANUP;
        while denied_job.snapshot().unwrap().active_processes != 0 {
            assert!(
                Instant::now() < denied_deadline,
                "failed creation leaked a job member"
            );
            assert!(!root.join("denied.json").exists());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!root.join("denied.json").exists());
        let start = Instant::now();
        let child = job.spawn(&spec("hold", &root.join("child.json"))).unwrap();
        let _ = job.terminate(1, Duration::ZERO); // May report pending kernel teardown.
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(child.wait_for_exit(CLEANUP).unwrap());
    }
}
