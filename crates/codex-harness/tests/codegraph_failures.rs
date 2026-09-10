#![cfg(windows)]
//! Force real writes followed by OS memory denial, deadline and cancellation
//! through the same Runtime/Job boundary used by the published provider.
use harness_core::{
    codegraph_generation::{ACTIVE_DIR_NAME, GenerationRole, GenerationStore, StorageLimits},
    codegraph_runtime::Runtime,
    codegraph_store,
    process::{Cancellation, Deadline},
};
use serde_json::json;
use std::{fs, path::Path, time::Duration};

fn failure(mode: &str, cancel_after_write: bool) {
    let owned = tempfile::tempdir().unwrap();
    let project = owned.path().join("project");
    fs::create_dir(&project).unwrap();
    let project = harness_core::dependency_discovery::local_path(&project).unwrap();
    let store = GenerationStore::open(&project, StorageLimits::default()).unwrap();
    let staged = store.stage_full_rebuild().unwrap();
    fs::remove_file(&staged.database).unwrap();
    codegraph_store::exec(&staged.database,
        "CREATE TABLE files(path TEXT); INSERT INTO files VALUES('committed.rs'); CREATE TABLE nodes(id INTEGER); CREATE TABLE edges(id INTEGER);").unwrap();
    store.commit_quiescent(GenerationRole::Stage).unwrap();
    let committed = store.committed_handle().unwrap().unwrap();
    let saved = fs::read(&committed.database).unwrap();
    let entry = owned.path().join(mode);
    fs::write(&entry, b"owned inert protocol mode").unwrap();
    let configuration = harness_core::codegraph_stdio::configuration(
        Path::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture")),
        &entry,
        &project,
        ACTIVE_DIR_NAME.into(),
    )
    .unwrap();
    let mut runtime = Runtime::new(configuration).unwrap();
    let cancel = Cancellation::default();
    let cancellation = cancel_after_write.then(|| {
        let marker = project.join("owned-worker-wrote");
        let stop = cancel.clone();
        std::thread::spawn(move || {
            let until = std::time::Instant::now() + Duration::from_secs(4);
            while !marker.exists() && std::time::Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                marker.exists(),
                "cancellation must follow a real partial write"
            );
            stop.cancel();
        })
    });
    let result = runtime
        .call(
            "codegraph_search",
            json!({"query":"committed"}),
            Deadline::after(Duration::from_secs(if cancel_after_write { 6 } else { 2 })).unwrap(),
            &cancel,
        )
        .unwrap();
    if let Some(thread) = cancellation {
        thread.join().unwrap();
    }
    assert!(project.join("owned-worker-wrote").exists());
    assert_eq!(result["isError"], true, "{result}");
    assert_eq!(result["freshness"], "failed", "{result}");
    assert_eq!(result["cleanup"]["job"]["active_processes"], 0, "{result}");
    assert_eq!(
        result["cleanup"]["job"]["memory_limit_bytes"],
        2u64 * 1024 * 1024 * 1024
    );
    assert_eq!(result["cleanup"]["job"]["cpu_rate"], 2500);
    if mode == "write-memory-denial" {
        assert!(
            result
                .to_string()
                .contains("Windows denied committed allocation"),
            "{result}"
        );
    }
    assert!(
        fs::read(&committed.database).unwrap() == saved,
        "saved checkpoint bytes changed"
    );
    assert_eq!(
        codegraph_store::counts(&committed.database).unwrap()["files"],
        1
    );
    let marker_time = fs::metadata(project.join("owned-worker-wrote"))
        .unwrap()
        .modified()
        .unwrap();
    let again = runtime
        .call(
            "codegraph_search",
            json!({"query":"committed"}),
            Deadline::after(Duration::from_secs(2)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(again["freshness"], "failed");
    assert_eq!(
        fs::metadata(project.join("owned-worker-wrote"))
            .unwrap()
            .modified()
            .unwrap(),
        marker_time,
        "failure must not automatically restart the worker"
    );
    runtime.close().unwrap();
    let restored = store.startup_active().unwrap();
    assert_eq!(
        codegraph_store::counts(&restored.database).unwrap()["files"],
        1
    );
}

#[test]
fn memory_denial_after_partial_refresh_preserves_checkpoint() {
    failure("write-memory-denial", false);
}
#[test]
fn deadline_after_partial_refresh_preserves_checkpoint() {
    failure("write-hang", false);
}
#[test]
fn cancellation_after_partial_refresh_preserves_checkpoint() {
    failure("write-hang", true);
}
