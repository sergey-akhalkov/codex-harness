//! Native ConPTY acceptance. Build the workspace fixture first:
//! cargo build --locked -p codex-harness --bin harness-console-fixture
//! cargo test --locked -p codex-harness --test console -- --test-threads=1 --nocapture
//! HARNESS_CONSOLE_FIXTURE may select an explicitly built fixture executable.
//! Artifacts are retained under the printed owned temporary directories.

use std::time::Duration;

#[cfg(not(windows))]
#[test]
fn console_explicitly_rejects_non_windows() {
    use harness_core::console::{ConsoleSession, ConsoleSpec};
    use harness_core::process::CommandSpec;
    let spec = ConsoleSpec::new(CommandSpec::new(std::env::current_exe().unwrap()));
    assert_eq!(
        ConsoleSession::spawn(spec).unwrap_err().kind(),
        std::io::ErrorKind::Unsupported
    );
}

#[cfg(windows)]
mod native {
    use super::*;
    use harness_core::console::{ConsoleSession, ConsoleSize, ConsoleSpec};
    use harness_core::process::{Cancellation, CommandSpec, Deadline, ProcessIdentity, StopReason};
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

    fn fixture() -> PathBuf {
        let path = std::env::var_os("HARNESS_CONSOLE_FIXTURE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("harness-console-fixture.exe")
            });
        assert!(
            path.is_file(),
            "build the Rust fixture first: cargo build --locked -p codex-harness --bin harness-console-fixture; missing {}",
            path.display()
        );
        path.canonicalize().unwrap()
    }

    fn root(name: &str) -> PathBuf {
        let root = tempfile::Builder::new()
            .prefix(&format!("harness-rust-console-{name}-"))
            .tempdir()
            .unwrap()
            .keep();
        println!("console evidence: {}", root.display());
        root
    }

    fn spec(role: &str) -> CommandSpec {
        let mut spec = CommandSpec::new(fixture());
        spec.args = vec![role.into()];
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

    fn wait_transcript(session: &ConsoleSession, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let text = session.transcript();
            if text.contains(needle) {
                return;
            }
            if Instant::now() >= deadline {
                panic!("missing {needle:?} in {text}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

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
            assert!(result == WAIT_OBJECT_0 || result == WAIT_TIMEOUT);
            result == WAIT_TIMEOUT
        }
        fn assert_dead(&self) {
            let deadline = Instant::now() + CLEANUP;
            while self.alive() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(
                !self.alive(),
                "owned console process survived cleanup: {:?}",
                self.identity
            );
        }
    }

    struct Foreign(Child);
    impl Foreign {
        fn spawn(artifact: &Path) -> Self {
            Self(
                Command::new(fixture())
                    .arg("hold")
                    .arg(artifact)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            )
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
    fn unicode_quoting_cwd_env_stdin_and_streams() {
        let root = root("streams");
        let marker = root.join("result.json");
        let args = [
            "",
            "space here",
            "кириллица λ",
            "quote\"middle",
            "backslash\\",
        ];
        let mut command = spec("report");
        command.args.push(marker.clone().into());
        command.args.extend(args.iter().map(OsString::from));
        command.current_dir = Some(root.clone());
        command.env.insert(
            OsString::from("HARNESS_CONSOLE_MARKER"),
            Some(OsString::from("маркер")),
        );
        let session = ConsoleSession::spawn(ConsoleSpec::new(command)).unwrap();
        let observer = Observer::open(session.identity().pid);
        assert_eq!(observer.identity, session.identity());
        session.send("first line\rвторая строка\r\x1a\r").unwrap();
        let result = session
            .wait(
                Deadline::after(Duration::from_secs(15)).unwrap(),
                &Cancellation::default(),
                CLEANUP,
            )
            .unwrap();
        assert_eq!(
            (
                result.outcome.reason,
                result.outcome.exit_code,
                result.outcome.process_exit_code
            ),
            (StopReason::Exited, 0, 0)
        );
        let data = receipt(&marker);
        assert_eq!(data["pid"], observer.identity.pid);
        assert_eq!(data["console"], true);
        assert_eq!(data["marker"], "маркер");
        assert_eq!(data["system_root"], json!(std::env::var("SystemRoot").ok()));
        assert_eq!(data["args"], json!(args));
        assert_eq!(
            PathBuf::from(data["cwd"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            root.canonicalize().unwrap()
        );
        assert_eq!(data["stdin"], "first line\r\nвторая строка\r\n");
        assert!(!result.output_truncated);
        assert!(result.transcript.contains("fixture stdout"));
        assert!(result.transcript.contains("fixture stderr"));
        observer.assert_dead();
        std::fs::write(
            root.join("verified.json"),
            serde_json::to_vec_pretty(&json!({
                "pid": observer.identity.pid,
                "creation_time": observer.identity.creation_time,
                "exit": result.outcome.exit_code,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn terminal_ctrl_c_reaches_the_child() {
        let root = root("ctrl-c");
        let marker = root.join("ready.json");
        let mut command = spec("hold");
        command.args.push(marker.clone().into());
        let session = ConsoleSession::spawn(ConsoleSpec::new(command)).unwrap();
        let _ = receipt(&marker);
        session.send("\x03").unwrap();
        let result = session
            .wait(
                Deadline::after(Duration::from_secs(10)).unwrap(),
                &Cancellation::default(),
                CLEANUP,
            )
            .unwrap();
        assert_eq!(result.outcome.reason, StopReason::Exited);
        assert_eq!(result.outcome.exit_code, 0xc000013a);
    }

    #[test]
    fn transcript_limit_drains_output_without_unbounded_retention() {
        let mut config = ConsoleSpec::new(spec("flood"));
        config.max_output_bytes = 512;
        let session = ConsoleSession::spawn(config).unwrap();
        let result = session
            .wait(
                Deadline::after(Duration::from_secs(20)).unwrap(),
                &Cancellation::default(),
                CLEANUP,
            )
            .unwrap();
        assert_eq!(result.outcome.reason, StopReason::Exited);
        assert_eq!(result.outcome.exit_code, 0);
        assert_eq!(result.transcript.len(), 512);
        assert!(result.output_truncated);
    }

    #[test]
    fn nonzero_exit_is_preserved() {
        let root = root("nonzero");
        let session = ConsoleSession::spawn(ConsoleSpec::new(spec("nonzero"))).unwrap();
        let observer = Observer::open(session.identity().pid);
        let result = session
            .wait(
                Deadline::after(Duration::from_secs(15)).unwrap(),
                &Cancellation::default(),
                CLEANUP,
            )
            .unwrap();
        assert_eq!(
            (
                result.outcome.reason,
                result.outcome.exit_code,
                result.outcome.process_exit_code
            ),
            (StopReason::Exited, 19, 19)
        );
        assert!(result.transcript.contains("fixture failure"));
        observer.assert_dead();
        std::fs::write(root.join("verified.json"), b"{\"exit\":19}").unwrap();
    }

    #[test]
    fn cancellation_stops_console_child_and_preserves_foreign() {
        let root = root("cancel");
        let foreign = Foreign::spawn(&root.join("foreign.json"));
        let mut command = spec("hold");
        command.args.push(root.join("owned.json").into());
        let session = ConsoleSession::spawn(ConsoleSpec::new(command)).unwrap();
        let observer = Observer::open(session.identity().pid);
        let _owned = receipt(&root.join("owned.json"));
        let foreign_id = receipt(&root.join("foreign.json"))["pid"].as_u64().unwrap() as u32;
        let foreign_observer = Observer::open(foreign_id);
        let cancel = Cancellation::default();
        cancel.cancel();
        let result = session
            .wait(
                Deadline::after(Duration::from_secs(15)).unwrap(),
                &cancel,
                CLEANUP,
            )
            .unwrap();
        assert_eq!(result.outcome.reason, StopReason::Cancelled);
        assert_eq!(result.outcome.exit_code, 130);
        observer.assert_dead();
        assert!(foreign_observer.alive(), "unrelated fixture was terminated");
        drop(foreign);
        std::fs::write(
            root.join("verified.json"),
            serde_json::to_vec_pretty(
                &json!({"owned": observer.identity.pid, "foreign": foreign_id}),
            )
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn interactive_prompt_echo_and_resize() {
        let root = root("interactive");
        let mut command = spec("interactive");
        command.args.push(root.join("ready.json").into());
        let session = ConsoleSession::spawn(ConsoleSpec::new(command)).unwrap();
        let observer = Observer::open(session.identity().pid);
        let _ = receipt(&root.join("ready.json"));
        wait_transcript(&session, "prompt>");
        session.send("кириллица\r").unwrap();
        wait_transcript(&session, "echo:");
        session
            .resize(ConsoleSize {
                columns: 80,
                rows: 24,
            })
            .unwrap();
        wait_transcript(&session, "resized:80x24");
        let result = session
            .wait(
                Deadline::after(Duration::from_secs(15)).unwrap(),
                &Cancellation::default(),
                CLEANUP,
            )
            .unwrap();
        assert_eq!(result.outcome.reason, StopReason::Exited);
        assert!(result.transcript.contains("echo:"));
        observer.assert_dead();
        std::fs::write(root.join("verified.json"), result.transcript.as_bytes()).unwrap();
    }

    #[test]
    fn drop_cancels_and_closes_console() {
        let root = root("drop");
        let mut command = spec("hold");
        command.args.push(root.join("owned.json").into());
        let session = ConsoleSession::spawn(ConsoleSpec::new(command)).unwrap();
        let observer = Observer::open(session.identity().pid);
        let _ = receipt(&root.join("owned.json"));
        drop(session);
        observer.assert_dead();
        std::fs::write(root.join("verified.json"), b"{\"dropped\":true}").unwrap();
    }
}
