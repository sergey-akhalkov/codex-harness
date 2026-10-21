//! Opt-in native Dolt branch transition; the server and board are test-owned.

use super::*;
use std::{
    net::{TcpListener, TcpStream},
    process::{Child, Stdio},
    time::{Duration, Instant},
};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires HARNESS_DOLT_EXE pointing to an installed native Dolt binary"]
fn native_branch_change_pauses_and_restoration_preserves_pending_acceptance() {
    let dolt = std::env::var_os("HARNESS_DOLT_EXE").expect("set HARNESS_DOLT_EXE explicitly");
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("server-data");
    fs::create_dir(&data).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let port = address.port().to_string();
    drop(listener);
    let stdout = temp.path().join("server.stdout");
    let stderr = temp.path().join("server.stderr");
    let mut command = Command::new(dolt);
    command
        .args([
            "sql-server",
            "--host",
            "127.0.0.1",
            "--port",
            &port,
            "--data-dir",
        ])
        .arg(&data)
        .stdin(Stdio::null())
        .stdout(fs::File::create(&stdout).unwrap())
        .stderr(fs::File::create(&stderr).unwrap());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut server = Server(command.spawn().expect("native owned Dolt server starts"));
    let deadline = Instant::now() + Duration::from_secs(15);
    while TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err() {
        assert!(
            server.0.try_wait().unwrap().is_none() && Instant::now() < deadline,
            "native Dolt startup failed: {} {}",
            fs::read_to_string(&stdout).unwrap(),
            fs::read_to_string(&stderr).unwrap()
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let board = Board::new_with_init(
        &temp.path().join("consumer"),
        &[
            "--server",
            "--external",
            "--server-host",
            "127.0.0.1",
            "--server-port",
            &port,
            "--server-user",
            "root",
        ],
    );
    board.json(&["config", "set", "events-journal", "true", "--json"]);
    let controller = board.controller();
    activate(&controller, JournalPolicy::RequireExisting);
    let task = board.create_task(
        "Accepted before native branch change",
        Some(Purpose::Ordinary),
        None,
    );
    board.set_binding(&task, &binding(&task, Purpose::Ordinary, "native-branch"));
    board.close(&task);
    let first = controller.poll().unwrap();
    assert_eq!(first.accepted, 1, "{first:?}");
    let before = controller.stored_state().unwrap().unwrap();
    assert_eq!(board.json(&["vc", "status", "--json"])["branch"], "main");
    board.json(&["branch", "reflection-branch", "--json"]);
    board.json(&[
        "sql",
        "SET GLOBAL bdct_default_branch = 'reflection-branch'",
        "--json",
    ]);
    assert_eq!(
        board.json(&["vc", "status", "--json"])["branch"],
        "reflection-branch"
    );

    assert_eq!(
        controller.check_active_binding(LEAD, None).unwrap().status,
        BindingStatus::StaleSource
    );
    let changed = controller.poll().unwrap();
    assert_eq!(
        changed.recovery.as_ref().unwrap().kind,
        RecoveryKind::BranchChanged,
        "{changed:?}"
    );
    assert_eq!(changed.accepted, 0);
    assert_eq!(changed.checkpoint, before.checkpoint);
    assert_eq!(
        controller.stored_state().unwrap().unwrap().records,
        before.records
    );
    assert_eq!(
        controller.check_active_binding(LEAD, None).unwrap().status,
        BindingStatus::RecoveryPending
    );

    board.json(&["sql", "SET GLOBAL bdct_default_branch = 'main'", "--json"]);
    assert_eq!(board.json(&["vc", "status", "--json"])["branch"], "main");
    controller
        .rebaseline_current("native branch restored and retained acceptance reconciled")
        .unwrap();
    let restored = controller.poll().unwrap();
    assert!(restored.recovery.is_none(), "{restored:?}");
    assert_eq!(restored.accepted, 0);
    assert_eq!(restored.jobs.len(), 1);
    assert_eq!(
        restored.jobs[0].acceptance_id,
        acceptance_for_task(&first, &task)
    );
    assert_eq!(
        controller.stored_state().unwrap().unwrap().records,
        before.records
    );
    eprintln!(
        "Native main -> reflection-branch -> main: paused at checkpoint {}, retained one exact pending acceptance, restored without duplicate trigger",
        before.checkpoint
    );
}
