#![cfg(windows)]
use harness_core::process::{Cancellation, CommandSpec, Deadline, Job, Limits, StopReason};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command, time::Duration};

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::Builder::new()
        .prefix("outcome target-")
        .tempdir()
        .unwrap();
    let exe = root.path().join("case.exe");
    fs::copy(env!("CARGO_BIN_EXE_harness-launch-fixture"), &exe).unwrap();
    (root, exe)
}
fn audit(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn real_build_and_cli_distinguish_stale_artifacts_using_executable_location() {
    let (root, exe) = fixture();
    fs::write(root.path().join("source.json"), b"{\"version\":2}\n").unwrap();
    fs::write(root.path().join("built.json"), b"{\"version\":1}\n").unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    for (mode, expected) in [
        ("cli", b"1\n".as_slice()),
        ("build", b"".as_slice()),
        ("cli", b"2\n".as_slice()),
    ] {
        let output = Command::new(&exe)
            .args(["--outcome-case", mode])
            .current_dir(elsewhere.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, expected);
    }
    assert_eq!(
        audit(&root.path().join("execution-audit.jsonl")),
        vec![
            json!({"entrypoint":"cli","version":1}),
            json!({"entrypoint":"build","version":2}),
            json!({"entrypoint":"cli","version":2})
        ]
    );
    assert_eq!(
        fs::read(root.path().join("source.json")).unwrap(),
        fs::read(root.path().join("built.json")).unwrap()
    );
    assert!(!elsewhere.path().join("built.json").exists());
}

#[test]
fn real_process_targets_preserve_both_streams_failure_and_forced_cleanup_distinction() {
    let (root, exe) = fixture();
    for (mode, code) in [("flood", 0), ("fail", 7), ("no-ready", 0), ("hang", 0)] {
        let stdout = root.path().join(format!("{mode}.stdout"));
        let stderr = root.path().join(format!("{mode}.stderr"));
        let mut spec = CommandSpec::new(&exe);
        spec.args = vec!["--outcome-case".into(), mode.into()];
        spec.current_dir = Some(root.path().to_owned());
        spec.stdout = Some(fs::File::create(&stdout).unwrap());
        spec.stderr = Some(fs::File::create(&stderr).unwrap());
        let job = Job::new(Limits {
            memory_bytes: Some(128 * 1024 * 1024),
            cpu_percent: Some(50.0),
        })
        .unwrap();
        let suspended = job.spawn_suspended(&spec).unwrap();
        assert!(job.contains(suspended.process()).unwrap());
        let child = suspended.resume().unwrap();
        drop(spec);
        let slow = ["hang", "no-ready"].contains(&mode);
        let result = job
            .wait(
                &child,
                Deadline::after(Duration::from_secs(if slow { 2 } else { 15 })).unwrap(),
                &Cancellation::default(),
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(
            result.reason,
            if slow {
                StopReason::Timeout
            } else {
                StopReason::Exited
            }
        );
        if !slow {
            assert_eq!(result.exit_code, code);
        }
        if mode == "flood" {
            assert_eq!(fs::read(&stdout).unwrap(), vec![b'a'; 2097152]);
            assert_eq!(fs::read(&stderr).unwrap(), vec![b'b'; 2097152]);
        }
    }
    std::thread::sleep(Duration::from_secs(5));
    assert!(!root.path().join("descendant-survived.txt").exists());
    let events = audit(&root.path().join("process-audit.jsonl"));
    for mode in ["flood", "fail", "no-ready", "hang"] {
        assert!(
            events
                .iter()
                .any(|r| r["mode"] == mode && r["event"] == "start")
        );
    }
    assert!(
        events
            .iter()
            .any(|r| r["mode"] == "flood" && r["event"] == "natural-end")
    );
    assert!(!events.iter().any(|r| {
        ["fail", "no-ready", "hang"].contains(&r["mode"].as_str().unwrap())
            && r["event"] == "natural-end"
    }));
}
