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
        CommandSpec, Job, Limits, OwnedProcess, ProcessIdentity, SHARED_CPU_PERCENT,
        SharedCpuBudget, SharedCpuSnapshot, StopReason,
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
    fn inherit_console_stays_in_job_and_is_not_nul() {
        let root = root("inherit-console");
        let redirected = spec("stdio-kind", &root.join("nul.json"));
        let job = Job::new(Limits::default()).unwrap();
        let child = job.spawn(&redirected).unwrap();
        assert_eq!(run(job, &child).exit_code, 17);
        let nul = receipt(&root.join("nul.json"));
        assert_eq!(nul["in_job"], true);
        assert_eq!(nul["stdout_type"], 2);
        assert_eq!(nul["console"], false);

        let mut inherited = spec("stdio-kind", &root.join("inherit.json"));
        inherited.inherit_console = true;
        inherited.stdout = Some(std::fs::File::create(root.join("nope.log")).unwrap());
        let job = Job::new(Limits::default()).unwrap();
        let error = job.spawn(&inherited).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot be combined with redirected streams"),
            "{error}"
        );

        let mut inherited = spec("stdio-kind", &root.join("inherit.json"));
        inherited.inherit_console = true;
        let job = Job::new(Limits::default()).unwrap();
        let child = job.spawn(&inherited).unwrap();
        assert!(job.contains(&child).unwrap());
        assert_eq!(run(job, &child).exit_code, 17);
        let inherited = receipt(&root.join("inherit.json"));
        assert_eq!(inherited["in_job"], true);
        assert!(
            inherited["stdout_type"] != 2 || inherited["console"] == true,
            "inherited stdout should not be NUL: {inherited}"
        );
    }

    #[test]
    fn foreground_wait_reaps_grandchild_without_deadline() {
        let root = root("foreground");
        let job = Job::new(Limits::default()).unwrap();
        let mut command = spec("tree-exit", &root.join("tree.json"));
        command.inherit_console = true;
        let child = job.spawn(&command).unwrap();
        let data = receipt(&root.join("tree.json"));
        let grandchild = Observer::open(data["grandchild"].as_u64().unwrap() as u32);
        let start = Instant::now();
        let code = job.wait_foreground(&child, CLEANUP).unwrap();
        assert_eq!(code, 17);
        assert!(start.elapsed() < Duration::from_secs(6));
        grandchild.assert_dead();
    }

    #[test]
    fn session_root_wait_preserves_managed_grandchild() {
        let root = root("session-root");
        let tree = root.join("tree.json");
        let exit_signal = tree.with_extension("grandchild.exit");
        let job = Job::new(Limits::default()).unwrap();
        let mut command = spec("tree-managed", &tree);
        command.inherit_console = true;
        let child = job.spawn(&command).unwrap();
        let data = receipt(&tree);
        let grandchild = Observer::open(data["grandchild"].as_u64().unwrap() as u32);
        let code = job.wait_session_root(&child).unwrap();
        assert_eq!(code, 17);
        // The job handle is gone; an ordinary wrapper exit must not reap the
        // upstream-managed background grandchild.
        assert!(grandchild.alive());
        std::fs::write(&exit_signal, "exit").unwrap();
        grandchild.assert_dead();
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

    /// One shared-budget participant: its own budget handle, its own lifecycle
    /// Job, and the member process created through the ordered pair.
    fn shared_member(
        account: &Path,
        root: &Path,
        index: usize,
    ) -> (SharedCpuBudget, Job, OwnedProcess, PathBuf) {
        let budget = SharedCpuBudget::acquire(account, SHARED_CPU_PERCENT).unwrap();
        let job = Job::new(Limits::default()).unwrap();
        let marker = root.join(format!("member-{index}.json"));
        let member = budget.spawn(&job, &spec("hold", &marker)).unwrap();
        (budget, job, member, marker)
    }

    fn wait_for_empty(budget: &SharedCpuBudget) -> SharedCpuSnapshot {
        let deadline = Instant::now() + CLEANUP;
        loop {
            let snapshot = budget.snapshot().unwrap();
            if snapshot.active_processes == 0 {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "shared budget still reports {} members",
                snapshot.active_processes
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn shared_cpu_budget_first_joins_converge_on_one_group() {
        let root = root("shared-converge");
        let account = root.join("account");
        let racers = 4;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(racers));
        let threads: Vec<_> = (0..racers)
            .map(|index| {
                let barrier = std::sync::Arc::clone(&barrier);
                let account = account.clone();
                let root = root.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    shared_member(&account, &root, index)
                })
            })
            .collect();
        let members: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        let name = members[0].0.name().to_owned();
        assert!(name.starts_with("CodingAgentsHarness.SharedCpu."), "{name}");
        assert!(name.is_ascii() && name.len() <= 128, "{name}");
        for (budget, _, _, _) in &members {
            assert_eq!(
                budget.name(),
                name,
                "simultaneous first joins must converge on one object"
            );
        }
        // Every independently created handle observes every independently
        // created process in the one shared object.
        for (index, (_, _, member, marker)) in members.iter().enumerate() {
            let recorded = receipt(marker);
            assert_eq!(recorded["in_job"], true);
            assert_eq!(
                recorded["pid"].as_u64().unwrap() as u32,
                member.identity().pid
            );
            for (other, (budget, _, _, _)) in members.iter().enumerate() {
                assert!(
                    budget.contains(member).unwrap(),
                    "handle {other} does not see member {index}"
                );
            }
        }
        let snapshot = members[0].0.snapshot().unwrap();
        assert_eq!(snapshot.cpu_rate, 7500);
        assert!(snapshot.cpu_hard_cap);
        assert!(
            !snapshot.kill_on_close,
            "the accounting job owns no cleanup"
        );
        assert_eq!(snapshot.job_memory_limit_bytes, 0);
        assert!(!snapshot.breakaway_ok && !snapshot.silent_breakaway_ok);
        // Creation with a job list counts each member more than once in the
        // kernel's active-process accounting, so only a lower bound is claimed;
        // `contains` above is the exact membership proof.
        assert!(snapshot.active_processes as usize >= racers);
        // Lifecycle jobs stay exclusive: each one holds only its own member.
        for (index, (_, job, member, _)) in members.iter().enumerate() {
            assert!(job.contains(member).unwrap());
            for (other, (_, _, peer, _)) in members.iter().enumerate() {
                if other != index {
                    assert!(
                        !job.contains(peer).unwrap(),
                        "lifecycle jobs must stay exclusive"
                    );
                }
            }
        }
        // The exclusive named-job contract still refuses the shared name.
        let refused = Job::new_named(Limits::default(), &name).unwrap_err();
        assert!(refused.to_string().contains("already in use"), "{refused}");
        let mut retained = Vec::new();
        for (budget, job, member, _) in members {
            job.terminate(0, CLEANUP).unwrap();
            assert!(member.wait_for_exit(CLEANUP).unwrap());
            retained.push((budget, member));
        }
        let empty = wait_for_empty(&retained[0].0);
        assert_eq!(empty.cpu_rate, 7500);
        record(
            &root.join("verified.json"),
            json!({"name": name, "members": retained.iter().map(|(_, member)| member.identity().pid).collect::<Vec<_>>(), "cpu_rate": snapshot.cpu_rate, "kill_on_close": snapshot.kill_on_close, "breakaway_ok": snapshot.breakaway_ok}),
        );
    }

    #[test]
    fn shared_cpu_budget_rejoins_existing_group_and_keeps_peer_allowance() {
        let root = root("shared-rejoin");
        let account = root.join("account");
        let first = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        let name = first.name().to_owned();
        let first_job = Job::new(Limits::default()).unwrap();
        let first_marker = root.join("first.json");
        let first_member = first
            .spawn(&first_job, &spec("hold", &first_marker))
            .unwrap();
        assert_eq!(receipt(&first_marker)["in_job"], true);
        let first_observer = Observer::open(first_member.identity().pid);
        // A later manager joins the same object while a member runs, rather
        // than creating a second allowance for the account.
        let second = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        assert_eq!(
            second.name(),
            name,
            "a later caller must rejoin the same object, not create another"
        );
        assert!(second.contains(&first_member).unwrap());
        let second_job = Job::new(Limits::default()).unwrap();
        let second_marker = root.join("second.json");
        let second_member = second
            .spawn(&second_job, &spec("hold", &second_marker))
            .unwrap();
        assert_eq!(receipt(&second_marker)["in_job"], true);
        let second_observer = Observer::open(second_member.identity().pid);
        assert!(first.contains(&second_member).unwrap());
        assert!(!first_job.contains(&second_member).unwrap());
        // Dropping one participant handle kills no peer and lifts no allowance.
        drop(first);
        assert!(first_observer.alive() && second_observer.alive());
        assert!(second.contains(&first_member).unwrap());
        assert_eq!(second.snapshot().unwrap().cpu_rate, 7500);
        // While any participant holds a handle the name stays reserved, so a
        // further manager joins the same object rather than creating a group.
        let rejoined = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        assert_eq!(rejoined.name(), name);
        assert!(rejoined.contains(&first_member).unwrap());
        assert!(rejoined.contains(&second_member).unwrap());
        let snapshot = rejoined.snapshot().unwrap();
        assert_eq!(snapshot.cpu_rate, 7500);
        assert!(
            snapshot.active_processes >= 2,
            "one object, two participants"
        );
        // A closed participant handle neither kills peers nor lifts the budget.
        drop(second);
        assert!(second_observer.alive() && first_observer.alive());
        assert!(rejoined.contains(&second_member).unwrap());
        assert_eq!(rejoined.snapshot().unwrap().cpu_rate, 7500);
        // A replacement manager still joins the surviving object.
        let latest = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        assert_eq!(latest.name(), name);
        assert!(latest.contains(&first_member).unwrap());
        assert!(latest.contains(&second_member).unwrap());
        drop(rejoined);
        // One participant's lifecycle cleanup reaps its own member only.
        first_job.terminate(0, CLEANUP).unwrap();
        assert!(first_member.wait_for_exit(CLEANUP).unwrap());
        first_observer.assert_dead();
        assert!(second_observer.alive(), "peer work must survive its peer");
        assert!(latest.contains(&second_member).unwrap());
        assert_eq!(latest.snapshot().unwrap().cpu_rate, 7500);
        second_job.terminate(0, CLEANUP).unwrap();
        assert!(second_member.wait_for_exit(CLEANUP).unwrap());
        second_observer.assert_dead();
        wait_for_empty(&latest);
        record(
            &root.join("verified.json"),
            json!({"name": name, "members": [first_observer.identity.pid, second_observer.identity.pid], "rejoin_rate": snapshot.cpu_rate, "peer_alive_after_peer_cleanup": true}),
        );
    }

    #[test]
    fn shared_cpu_budget_rate_reaches_admitted_payload() {
        let root = root("shared-rate");
        let account = root.join("account");
        // 0.1% is far below the desktop ceiling yet still large enough for
        // process initialization on a loaded host, so the measured gap from the
        // uncapped control stays large without hitting the wait deadline.
        let budget = SharedCpuBudget::acquire(&account, 0.1).unwrap();
        assert_eq!(budget.snapshot().unwrap().cpu_rate, 10);
        let capped_job = Job::new(Limits::default()).unwrap();
        let capped = budget
            .spawn(&capped_job, &spec("cpu", &root.join("capped.json")))
            .unwrap();
        let free_job = Job::new(Limits::default()).unwrap();
        let free = free_job
            .spawn(&spec("cpu", &root.join("free.json")))
            .unwrap();
        assert!(budget.contains(&capped).unwrap());
        assert!(capped_job.contains(&capped).unwrap());
        let capped_result = run(capped_job, &capped);
        let free_result = run(free_job, &free);
        assert_eq!((capped_result.exit_code, free_result.exit_code), (0, 0));
        assert_eq!(
            capped_result.job.cpu_rate, 0,
            "the lifecycle job adds no second cap"
        );
        let cap_cpu = capped.cpu_time().unwrap().as_secs_f64();
        let free_cpu = free.cpu_time().unwrap().as_secs_f64();
        assert!(
            free_cpu > 0.3,
            "host too busy for an uncapped control: {free_cpu}"
        );
        assert!(
            cap_cpu < free_cpu * 0.6 + 0.05,
            "shared allowance not observed: capped={cap_cpu}, control={free_cpu}"
        );
        assert_eq!(budget.snapshot().unwrap().cpu_rate, 10);
        record(
            &root.join("verified.json"),
            json!({"capped_cpu_seconds": cap_cpu, "free_cpu_seconds": free_cpu, "cpu_rate": 10, "capped_in_lifecycle": true}),
        );
    }

    #[test]
    fn shared_cpu_budget_refuses_conflicts_without_creating_a_second_group() {
        let root = root("shared-conflict");
        let account = root.join("account");
        for percent in [0.0, -1.0, 100.01, f64::INFINITY, f64::NAN] {
            assert!(
                SharedCpuBudget::acquire(&account, percent).is_err(),
                "{percent}"
            );
        }
        assert!(
            !account.exists(),
            "an invalid request must not create account state"
        );
        let established = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        let name = established.name().to_owned();
        let other_rate = SharedCpuBudget::acquire(&account, 50.0).unwrap_err();
        assert!(
            other_rate.to_string().contains("already established"),
            "{other_rate}"
        );
        assert_eq!(established.snapshot().unwrap().cpu_rate, 7500);
        drop(established);
        // A same-named object with foreign limits is preserved, never adopted.
        let foreign = Job::new_named(
            Limits {
                memory_bytes: Some(64 * 1024 * 1024),
                cpu_percent: Some(10.0),
            },
            &name,
        )
        .unwrap();
        let refused = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap_err();
        assert!(
            refused.to_string().contains("CPU-only settings"),
            "{refused}"
        );
        let foreign_snapshot = foreign.snapshot().unwrap();
        assert!(foreign_snapshot.kill_on_close && foreign_snapshot.cpu_rate == 1000);
        drop(foreign);
        // With the conflicting object gone the same account re-establishes.
        let recovered = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        assert_eq!(recovered.snapshot().unwrap().cpu_rate, 7500);
        // A record copied into another directory authorizes nothing there.
        let copied = root.join("copied-account");
        std::fs::create_dir(&copied).unwrap();
        std::fs::copy(
            account.join("cpu-budget.json"),
            copied.join("cpu-budget.json"),
        )
        .unwrap();
        let copied_refusal = SharedCpuBudget::acquire(&copied, SHARED_CPU_PERCENT).unwrap_err();
        assert!(
            copied_refusal
                .to_string()
                .contains("does not describe this account directory"),
            "{copied_refusal}"
        );
        std::fs::remove_file(copied.join("cpu-budget.json")).unwrap();
        assert_eq!(
            SharedCpuBudget::acquire(&copied, SHARED_CPU_PERCENT)
                .unwrap()
                .snapshot()
                .unwrap()
                .cpu_rate,
            7500
        );
        record(
            &root.join("verified.json"),
            json!({"name": name, "foreign_rate": foreign_snapshot.cpu_rate, "refusals": ["different rate", "foreign settings", "copied record"]}),
        );
    }

    #[test]
    fn shared_cpu_budget_admits_ordered_pair_and_keeps_cleanup_independent() {
        let root = root("shared-ordered");
        let account = root.join("account");
        let budget = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        let tree_job = Job::new(Limits::default()).unwrap();
        let tree = root.join("tree.json");
        let tree_member = budget.spawn(&tree_job, &spec("tree-hold", &tree)).unwrap();
        let data = receipt(&tree);
        let grandchild = Observer::open(data["grandchild"].as_u64().unwrap() as u32);
        // Creation-time assignment covers the complete ordered pair, and an
        // ordinary grandchild inherits the shared accounting job too.
        assert!(budget.contains(&tree_member).unwrap());
        assert!(tree_job.contains(&tree_member).unwrap());
        assert!(tree_job.owns(grandchild.identity).unwrap());
        let snapshot = budget.snapshot().unwrap();
        assert_eq!(snapshot.cpu_rate, 7500);
        assert!(snapshot.active_processes >= 2, "{snapshot:?}");
        // The lifecycle job keeps exclusive cleanup authority over its own tree.
        assert_eq!(tree_job.terminate(0, CLEANUP).unwrap().active_processes, 0);
        grandchild.assert_dead();
        assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
        // Dropping one participant's lifecycle Job reaps only that member.
        let kept_job = Job::new(Limits::default()).unwrap();
        let kept_marker = root.join("kept.json");
        let kept = budget
            .spawn(&kept_job, &spec("hold", &kept_marker))
            .unwrap();
        assert_eq!(receipt(&kept_marker)["in_job"], true);
        let kept_observer = Observer::open(kept.identity().pid);
        let dropped_job = Job::new(Limits::default()).unwrap();
        let dropped_marker = root.join("dropped.json");
        let dropped = budget
            .spawn(&dropped_job, &spec("hold", &dropped_marker))
            .unwrap();
        assert_eq!(receipt(&dropped_marker)["in_job"], true);
        let dropped_observer = Observer::open(dropped.identity().pid);
        drop(dropped_job);
        dropped_observer.assert_dead();
        assert!(
            kept_observer.alive(),
            "a peer must survive another member's cleanup"
        );
        assert!(budget.contains(&kept).unwrap());
        assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
        kept_job.terminate(0, CLEANUP).unwrap();
        assert!(kept.wait_for_exit(CLEANUP).unwrap());
        wait_for_empty(&budget);
        record(
            &root.join("verified.json"),
            json!({"tree": tree_member.identity().pid, "grandchild": grandchild.identity.pid, "kept": kept_observer.identity.pid, "dropped": dropped_observer.identity.pid, "peer_survived": true}),
        );
    }

    #[test]
    fn shared_cpu_budget_enforcement_survives_losing_every_handle() {
        let root = root("shared-detached");
        let account = root.join("account");
        // 0.1% leaves a large, independently measured gap from the control.
        let budget = SharedCpuBudget::acquire(&account, 0.1).unwrap();
        let hold_job = Job::new(Limits::default()).unwrap();
        let hold_marker = root.join("hold.json");
        let hold = budget
            .spawn(&hold_job, &spec("hold", &hold_marker))
            .unwrap();
        assert_eq!(receipt(&hold_marker)["in_job"], true);
        let hold_observer = Observer::open(hold.identity().pid);
        let capped_job = Job::new(Limits::default()).unwrap();
        let capped = budget
            .spawn(&capped_job, &spec("cpu", &root.join("capped.json")))
            .unwrap();
        // Losing every participant handle must not lift the allowance: the
        // object lives while members do, so its rate still governs them.
        drop(budget);
        assert!(hold_observer.alive());
        let free_job = Job::new(Limits::default()).unwrap();
        let free = free_job
            .spawn(&spec("cpu", &root.join("free.json")))
            .unwrap();
        assert_eq!(run(capped_job, &capped).exit_code, 0);
        assert_eq!(run(free_job, &free).exit_code, 0);
        let cap_cpu = capped.cpu_time().unwrap().as_secs_f64();
        let free_cpu = free.cpu_time().unwrap().as_secs_f64();
        assert!(
            free_cpu > 0.3,
            "host too busy for an uncapped control: {free_cpu}"
        );
        assert!(
            cap_cpu < free_cpu * 0.6 + 0.05,
            "allowance was lifted after the last handle closed: capped={cap_cpu}, control={free_cpu}"
        );
        // Documented boundary: the name is released with the last handle while
        // members remain, so a manager starting now establishes a fresh object
        // instead of rejoining the surviving one. A participant that keeps its
        // handle for the session lifetime (or a coordinator) is what reserves
        // one group for the account; the old members keep their old allowance.
        let later = SharedCpuBudget::acquire(&account, 0.1).unwrap();
        assert_eq!(later.snapshot().unwrap().cpu_rate, 10);
        assert!(!later.contains(&hold).unwrap());
        // Independent lifecycle cleanup still works with no shared handle.
        hold_job.terminate(0, CLEANUP).unwrap();
        assert!(hold.wait_for_exit(CLEANUP).unwrap());
        hold_observer.assert_dead();
        record(
            &root.join("verified.json"),
            json!({"capped_cpu_seconds": cap_cpu, "free_cpu_seconds": free_cpu, "hold": hold_observer.identity.pid, "rejoin_after_last_handle": false}),
        );
    }

    /// Child entry for the lifecycle tests. A normal suite run has no controller
    /// environment and returns immediately. A spawned copy holds one account
    /// handle until released or killed, and never runs session cleanup itself.
    #[test]
    fn shared_cpu_budget_controller() {
        let Ok(account) = std::env::var("HARNESS_SHARED_CPU_CONTROLLER") else {
            return;
        };
        let ready = PathBuf::from(std::env::var("HARNESS_SHARED_CPU_READY").unwrap());
        let release = PathBuf::from(std::env::var("HARNESS_SHARED_CPU_RELEASE").unwrap());
        let budget = SharedCpuBudget::acquire(Path::new(&account), SHARED_CPU_PERCENT).unwrap();
        let rate = budget.snapshot().unwrap().cpu_rate;
        std::fs::write(
            &ready,
            format!(
                "{{\"pid\":{},\"name\":{},\"rate\":{rate}}}",
                std::process::id(),
                serde_json::to_string(budget.name()).unwrap()
            ),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !release.exists() {
            assert!(
                Instant::now() < deadline,
                "controller was not released or killed"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(budget.snapshot().unwrap().cpu_rate, rate);
    }

    fn spawn_controller(account: &Path, ready: &Path, release: &Path, stderr: &Path) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .arg("native::shared_cpu_budget_controller")
            .arg("--exact")
            .arg("--test-threads=1")
            .env("HARNESS_SHARED_CPU_CONTROLLER", account)
            .env("HARNESS_SHARED_CPU_READY", ready)
            .env("HARNESS_SHARED_CPU_RELEASE", release)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(stderr).unwrap())
            .spawn()
            .unwrap()
    }

    fn wait_controller(path: &Path) -> Value {
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if let Ok(bytes) = std::fs::read(path)
                && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "controller did not report its handle: {}",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_child_exit(child: &mut Child) {
        let deadline = Instant::now() + CLEANUP;
        loop {
            if child.try_wait().unwrap().is_some() {
                return;
            }
            assert!(Instant::now() < deadline, "controller did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    struct RestoredAccountEnv(Option<OsString>);
    impl RestoredAccountEnv {
        fn set(account: &Path) -> Self {
            let previous = std::env::var_os(harness_core::process::CPU_BUDGET_ACCOUNT_ENV);
            unsafe {
                std::env::set_var(harness_core::process::CPU_BUDGET_ACCOUNT_ENV, account);
            }
            Self(previous)
        }
    }
    impl Drop for RestoredAccountEnv {
        fn drop(&mut self) {
            unsafe {
                match self.0.take() {
                    Some(value) => {
                        std::env::set_var(harness_core::process::CPU_BUDGET_ACCOUNT_ENV, value);
                    }
                    None => std::env::remove_var(harness_core::process::CPU_BUDGET_ACCOUNT_ENV),
                }
            }
        }
    }

    #[test]
    fn shared_cpu_budget_normal_exit_keeps_peer_allowance() {
        let root = root("shared-normal-exit");
        let account = root.join("account");
        let budget = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        let name = budget.name().to_owned();
        let peer_job = Job::new(Limits::default()).unwrap();
        let peer_marker = root.join("peer.json");
        let peer = budget
            .spawn(&peer_job, &spec("hold", &peer_marker))
            .unwrap();
        assert_eq!(receipt(&peer_marker)["in_job"], true);
        let peer_observer = Observer::open(peer.identity().pid);
        let ready = root.join("controller-ready.json");
        let release = root.join("controller-release");
        let mut controller =
            spawn_controller(&account, &ready, &release, &root.join("controller.stderr"));
        let reported = wait_controller(&ready);
        assert_eq!(reported["name"], name);
        assert_eq!(reported["rate"], 7500);
        std::fs::write(&release, []).unwrap();
        wait_child_exit(&mut controller);
        assert_eq!(controller.wait().unwrap().code(), Some(0));
        assert!(
            peer_observer.alive(),
            "normal controller exit killed the peer"
        );
        assert!(budget.contains(&peer).unwrap());
        assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
        let rejoined = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        assert_eq!(rejoined.name(), name, "normal exit created a second group");
        assert!(rejoined.contains(&peer).unwrap());
        assert!(!rejoined.snapshot().unwrap().kill_on_close);
        peer_job.terminate(0, CLEANUP).unwrap();
        assert!(peer.wait_for_exit(CLEANUP).unwrap());
        record(
            &root.join("verified.json"),
            json!({"name": name, "peer": peer_observer.identity.pid, "controller": reported["pid"], "rate": 7500, "normal_exit": true}),
        );
    }

    #[test]
    fn shared_cpu_budget_abrupt_owner_death_rejoins_with_ownership_validation() {
        let root = root("shared-abrupt-death");
        let account = root.join("account");
        let budget = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        let name = budget.name().to_owned();
        let peer_job = Job::new(Limits::default()).unwrap();
        let peer_marker = root.join("peer.json");
        let peer = budget
            .spawn(&peer_job, &spec("hold", &peer_marker))
            .unwrap();
        assert_eq!(receipt(&peer_marker)["in_job"], true);
        let peer_observer = Observer::open(peer.identity().pid);
        let ready = root.join("controller-ready.json");
        let release = root.join("controller-release");
        let mut controller =
            spawn_controller(&account, &ready, &release, &root.join("controller.stderr"));
        let reported = wait_controller(&ready);
        assert_eq!(reported["name"], name);
        assert_eq!(reported["rate"], 7500);
        // TerminateProcess: the controller Drop and any cleanup do not run.
        controller.kill().unwrap();
        wait_child_exit(&mut controller);
        assert!(
            !release.exists(),
            "abrupt death must not take the release path"
        );
        assert!(
            peer_observer.alive(),
            "killing the control owner killed the peer"
        );
        assert!(budget.contains(&peer).unwrap());
        let survived = budget.snapshot().unwrap();
        assert_eq!(survived.cpu_rate, 7500);
        assert!(survived.cpu_hard_cap && !survived.kill_on_close);
        let rejoined = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        assert_eq!(rejoined.name(), name);
        assert!(rejoined.contains(&peer).unwrap());
        assert_eq!(rejoined.snapshot().unwrap().cpu_rate, 7500);
        let wrong_rate = SharedCpuBudget::acquire(&account, 50.0).unwrap_err();
        assert!(
            wrong_rate.to_string().contains("already established"),
            "{wrong_rate}"
        );
        assert_eq!(rejoined.snapshot().unwrap().cpu_rate, 7500);
        let copied = root.join("copied-account");
        std::fs::create_dir_all(&copied).unwrap();
        std::fs::copy(
            account.join("cpu-budget.json"),
            copied.join("cpu-budget.json"),
        )
        .unwrap();
        let copied_refusal = SharedCpuBudget::acquire(&copied, SHARED_CPU_PERCENT).unwrap_err();
        assert!(
            copied_refusal
                .to_string()
                .contains("does not describe this account"),
            "{copied_refusal}"
        );
        assert!(
            Job::new_named(Limits::default(), &name)
                .unwrap_err()
                .to_string()
                .contains("already in use"),
            "restart must not publish a second group name"
        );
        assert!(peer_observer.alive());
        assert!(rejoined.contains(&peer).unwrap());
        peer_job.terminate(0, CLEANUP).unwrap();
        assert!(peer.wait_for_exit(CLEANUP).unwrap());
        record(
            &root.join("verified.json"),
            json!({"name": name, "peer": peer_observer.identity.pid, "killed_controller": reported["pid"], "rate": 7500, "rejoined": true}),
        );
    }

    #[test]
    fn shared_cpu_budget_incomplete_activation_names_outside_route_and_keeps_peer() {
        let root = root("shared-incomplete");
        let account = root.join("account");
        let budget = SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT).unwrap();
        let name = budget.name().to_owned();
        let policy =
            br#"{"schema":1,"ceiling_percent":75.0,"escape_hatch":"CODEX_HARNESS_CPU_PERCENT"}"#;
        std::fs::write(account.join("shared-cpu-policy.json"), policy).unwrap();
        let _account_env = RestoredAccountEnv::set(&account);
        let peer_job = Job::new(Limits::default()).unwrap();
        let peer_marker = root.join("peer.json");
        let peer = budget
            .spawn(&peer_job, &spec("hold", &peer_marker))
            .unwrap();
        assert_eq!(receipt(&peer_marker)["in_job"], true);
        let peer_observer = Observer::open(peer.identity().pid);
        let before = harness_core::core_install::inspect_cpu_policy(&[fixture()]);
        assert_eq!(before.action, "inspected");
        assert!(!before.wrote_policy);
        assert!(
            !before
                .restart_boundary
                .contains(&format!("pid {}", peer.identity().pid)),
            "a covered peer must not be reported as needing restart: {}",
            before.restart_boundary
        );
        let outside_marker = root.join("outside.json");
        let mut outside = Foreign::spawn("hold", &outside_marker);
        let outside_receipt = receipt(&outside_marker);
        let outside_pid = outside_receipt["pid"].as_u64().unwrap();
        let report = harness_core::core_install::inspect_cpu_policy(&[fixture()]);
        assert_eq!(report.activation, "incomplete", "{report:?}");
        assert_eq!(report.action, "inspected");
        assert!(!report.wrote_policy);
        assert_eq!(report.model_calls, 0);
        assert_eq!(report.measured_consumption, "not-sampled");
        assert!(
            report
                .restart_boundary
                .contains(&format!("pid {outside_pid}")),
            "{}",
            report.restart_boundary
        );
        assert!(
            report.restart_boundary.contains("does not terminate"),
            "{}",
            report.restart_boundary
        );
        assert!(
            !report
                .restart_boundary
                .contains(&format!("pid {}", peer.identity().pid)),
            "covered peer was listed as uncovered: {}",
            report.restart_boundary
        );
        assert!(
            report.kernel_configuration.contains(&name)
                && report.kernel_configuration.contains("cpu_rate 7500"),
            "{}",
            report.kernel_configuration
        );
        assert_eq!(
            std::fs::read(account.join("shared-cpu-policy.json")).unwrap(),
            policy
        );
        assert!(peer_observer.alive() && budget.contains(&peer).unwrap());
        assert_eq!(budget.snapshot().unwrap().cpu_rate, 7500);
        assert!(
            outside.0.try_wait().unwrap().is_none(),
            "inspection stopped the outside route"
        );
        assert_eq!(
            SharedCpuBudget::acquire(&account, SHARED_CPU_PERCENT)
                .unwrap()
                .name(),
            name
        );
        outside.kill_and_wait();
        peer_job.terminate(0, CLEANUP).unwrap();
        assert!(peer.wait_for_exit(CLEANUP).unwrap());
        record(
            &root.join("verified.json"),
            json!({"name": name, "peer": peer.identity().pid, "outside": outside_pid, "activation": report.activation, "rate": 7500}),
        );
    }
}
