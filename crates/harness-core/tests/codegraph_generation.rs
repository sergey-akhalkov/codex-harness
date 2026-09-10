//! Owned CodeGraph generation storage. Native SQLite fixtures only.
//! cargo test --locked -p harness-core --test codegraph_generation -- --test-threads=1 --nocapture
#![cfg(windows)]

use harness_core::codegraph_generation::{
    ACTIVE_DIR_NAME, DATABASE_FILE_NAME, GenerationRole, GenerationStore, OWNERSHIP_FILE_NAME,
    STAGE_DIR_NAME, StorageLimits,
};
use harness_core::codegraph_store;
use harness_core::process::{Cancellation, Deadline};
use std::{
    fs,
    os::windows::fs::symlink_dir,
    path::{Path, PathBuf},
    time::Duration,
};

fn fixture() -> PathBuf {
    let root = tempfile::Builder::new()
        .prefix("harness-codegraph-generation-")
        .tempdir()
        .unwrap()
        .keep();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    root
}

fn store(root: &Path) -> GenerationStore {
    GenerationStore::open(root, StorageLimits::for_tests(64 * 1024, 256 * 1024, 1)).unwrap()
}

fn graph_sql(files: u64, nodes: u64, edges: u64) -> String {
    let mut sql = String::from(
        "CREATE TABLE files(id INTEGER PRIMARY KEY, path TEXT); CREATE TABLE nodes(id INTEGER PRIMARY KEY, name TEXT); CREATE TABLE edges(id INTEGER PRIMARY KEY, src INTEGER, dst INTEGER);",
    );
    for i in 0..files {
        sql.push_str(&format!("INSERT INTO files(id,path) VALUES({i},'f{i}');"));
    }
    for i in 0..nodes {
        sql.push_str(&format!("INSERT INTO nodes(id,name) VALUES({i},'n{i}');"));
    }
    for i in 0..edges {
        sql.push_str(&format!("INSERT INTO edges(id,src,dst) VALUES({i},0,0);"));
    }
    sql
}

fn write_graph(database: &Path, files: u64, nodes: u64, edges: u64) {
    if database.exists() {
        fs::remove_file(database).unwrap();
    }
    codegraph_store::exec(database, &graph_sql(files, nodes, edges)).unwrap();
}

fn counts(database: &Path) -> (u64, u64, u64) {
    let value = codegraph_store::counts(database).unwrap();
    (
        value["files"].as_u64().unwrap(),
        value["nodes"].as_u64().unwrap(),
        value["edges"].as_u64().unwrap(),
    )
}

#[test]
fn committed_index_survives_failed_stage_and_partial_active_write() {
    let root = fixture();
    let store = store(&root);
    let active = store.startup_active().unwrap();
    assert_eq!(active.data_name, ACTIVE_DIR_NAME);
    write_graph(&active.database, 3, 5, 2);
    let committed = store.commit_quiescent(GenerationRole::Active).unwrap();
    assert_eq!(
        committed.status,
        harness_core::codegraph_generation::GenerationStatus::Active
    );
    assert_eq!(
        counts(&store.committed_handle().unwrap().unwrap().database),
        (3, 5, 2)
    );

    let stage = store.stage_full_rebuild().unwrap();
    assert_eq!(stage.data_name, STAGE_DIR_NAME);
    write_graph(&stage.database, 9, 1, 1);
    fs::write(stage.directory.join("partial.wal"), vec![0u8; 32]).unwrap();
    let after_fail = store.rollback_failed_stage().unwrap().unwrap();
    assert!(!store.layout().stage.exists());
    assert_eq!(
        counts(&store.committed_handle().unwrap().unwrap().database),
        (3, 5, 2)
    );
    assert_eq!(after_fail.data_name, ACTIVE_DIR_NAME);
    assert_eq!(counts(&after_fail.database), (3, 5, 2));

    write_graph(&after_fail.database, 1, 1, 0);
    let restored = store.startup_active().unwrap();
    assert_eq!(counts(&restored.database), (3, 5, 2));
    assert_eq!(
        counts(&store.committed_handle().unwrap().unwrap().database),
        (3, 5, 2)
    );
}

#[test]
fn interrupted_metadata_recovers_without_publishing_partial_generation() {
    let root = fixture();
    let store = store(&root);
    let active = store.startup_active().unwrap();
    write_graph(&active.database, 2, 2, 1);
    store.commit_quiescent(GenerationRole::Active).unwrap();
    let committed = counts(&store.committed_handle().unwrap().unwrap().database);

    let layout = store.layout();
    fs::write(layout.store.join("generation.json.tmp"), "{").unwrap();
    fs::write(layout.active.join("generation.json"), "not-json").unwrap();
    let recovered = store.recover().unwrap().unwrap();
    assert_eq!(
        recovered.status,
        harness_core::codegraph_generation::GenerationStatus::Failed
    );
    assert!(!layout.store.join("generation.json.tmp").exists());
    assert_eq!(
        counts(&store.committed_handle().unwrap().unwrap().database),
        committed
    );

    let restored = store.startup_active().unwrap();
    assert_eq!(counts(&restored.database), committed);
    assert_eq!(
        restored.status,
        harness_core::codegraph_generation::GenerationStatus::Active
    );
}

#[test]
fn interruption_between_checkpoint_directory_renames_restores_the_saved_pair() {
    let root = fixture();
    let store = store(&root);
    let active = store.startup_active().unwrap();
    write_graph(&active.database, 2, 7, 3);
    store.commit_quiescent(GenerationRole::Active).unwrap();
    let checkpoint = store.committed_handle().unwrap().unwrap();
    let layout = store.layout();
    // This is the actual publication boundary: the previous database and its
    // metadata have moved together, while the new directory is not published.
    fs::rename(&layout.committed, layout.store.join("previous-commit")).unwrap();
    let incoming = layout.store.join("pending-commit");
    fs::create_dir(&incoming).unwrap();
    fs::copy(
        layout.store.join(OWNERSHIP_FILE_NAME),
        incoming.join(OWNERSHIP_FILE_NAME),
    )
    .unwrap();
    write_graph(&incoming.join(DATABASE_FILE_NAME), 1, 1, 0);
    assert_eq!(
        store.committed_handle().unwrap().unwrap().generation,
        checkpoint.generation
    );
    let restored = store.startup_active().unwrap();
    assert_eq!(restored.generation, checkpoint.generation);
    assert_eq!(counts(&restored.database), (2, 7, 3));
    assert_eq!(
        counts(&store.committed_handle().unwrap().unwrap().database),
        (2, 7, 3)
    );
    assert!(!incoming.exists());
    assert!(!layout.store.join("previous-commit").exists());
}

#[test]
fn storage_pressure_leaves_committed_readable_and_queries_do_not_copy() {
    let root = fixture();
    let store = store(&root);
    let active = store.startup_active().unwrap();
    write_graph(&active.database, 4, 4, 1);
    store.commit_quiescent(GenerationRole::Active).unwrap();
    let committed = store.committed_handle().unwrap().unwrap();
    let before = counts(&committed.database);
    let usage = store.check_limits(None).unwrap();
    assert!(usage.project_bytes > 0);
    assert!(usage.free_bytes >= usage.reserve_bytes);

    let tight = GenerationStore::open(&root, StorageLimits::for_tests(1, 256 * 1024, 1)).unwrap();
    let pressure = tight.check_limits(None);
    assert!(pressure.is_err(), "{pressure:?}");
    assert_eq!(counts(&committed.database), before);
    assert!(!root.join("query-copy.db").exists());
    assert!(store.layout().active.join(DATABASE_FILE_NAME).is_file());
}

#[test]
fn link_safe_cleanup_preserves_external_sentinel_and_product_files() {
    let root = fixture();
    let store = store(&root);
    let active = store.startup_active().unwrap();
    write_graph(&active.database, 1, 1, 0);
    store.commit_quiescent(GenerationRole::Active).unwrap();
    let outside = tempfile::Builder::new()
        .prefix("harness-codegraph-sentinel-")
        .tempdir()
        .unwrap()
        .keep();
    let sentinel = outside.join("keep.txt");
    fs::write(&sentinel, "external-sentinel").unwrap();
    let layout = store.layout();
    let decoy = layout.store.join("outside-link");
    symlink_dir(&outside, &decoy).unwrap();
    fs::write(root.join("src/keep.rs"), "pub fn keep() {}\n").unwrap();
    store.cleanup_obsolete().unwrap();
    assert!(layout.store.join(OWNERSHIP_FILE_NAME).is_file());
    assert!(layout.committed.join(DATABASE_FILE_NAME).is_file());
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "external-sentinel");
    assert!(root.join("src/keep.rs").is_file());
    assert!(root.join("src/main.rs").is_file());
    assert!(decoy.is_dir() || decoy.exists());
}

#[test]
fn symlink_project_or_store_paths_are_rejected() {
    let root = fixture();
    let alias = root.parent().unwrap().join(format!(
        "{}-alias",
        root.file_name().unwrap().to_string_lossy()
    ));
    symlink_dir(&root, &alias).unwrap();
    assert!(
        GenerationStore::open(&alias, StorageLimits::for_tests(64 * 1024, 256 * 1024, 1)).is_err()
    );
    let store = store(&root);
    store.startup_active().unwrap();
    let linked_active = root.join("linked-active");
    symlink_dir(&store.layout().active, &linked_active).unwrap();
    assert!(store.check_limits(Some(&linked_active)).is_err());
}

fn lease() -> (Deadline, Cancellation) {
    (
        Deadline::after(Duration::from_secs(5)).unwrap(),
        Cancellation::default(),
    )
}

fn fill_until(path: &Path, target: u64) {
    let current = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if current >= target {
        return;
    }
    let mut bytes = fs::read(path).unwrap_or_default();
    bytes.resize(target as usize, 0);
    fs::write(path, bytes).unwrap();
}

#[test]
fn restore_from_committed_cannot_exceed_remaining_project_bytes() {
    let root = fixture();
    let store = store(&root);
    let active = store.startup_active().unwrap();
    write_graph(&active.database, 8, 8, 2);
    store.commit_quiescent(GenerationRole::Active).unwrap();
    let committed = store.committed_handle().unwrap().unwrap();
    let committed_size = fs::metadata(&committed.database).unwrap().len();
    let usage = store.usage().unwrap();
    fill_until(
        &store.layout().store.join("padding.bin"),
        usage.project_limit_bytes.saturating_sub(committed_size / 2),
    );
    let tight = GenerationStore::open(
        &root,
        StorageLimits::for_tests(64 * 1024, usage.project_limit_bytes, 1),
    )
    .unwrap();
    let (deadline, cancel) = lease();
    let restored = tight.startup_active_bounded(deadline, &cancel);
    assert!(restored.is_err(), "{restored:?}");
    let error = restored.unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("storage allowance") || message.contains("project"),
        "{message}"
    );
    assert_eq!(counts(&committed.database), (8, 8, 2));
    assert!(!root.join("query-copy.db").exists());
}

#[test]
fn usage_tolerates_transient_wal_disappearance_without_marking_mismatch() {
    let root = fixture();
    let store = store(&root);
    let active = store.startup_active().unwrap();
    write_graph(&active.database, 2, 2, 1);
    store.commit_quiescent(GenerationRole::Active).unwrap();
    let wal = store.layout().active.join("codegraph.db-wal");
    fs::write(&wal, vec![7u8; 4096]).unwrap();
    let before = store.check_limits(None).unwrap();
    fs::remove_file(&wal).unwrap();
    let after = store.check_limits(None).unwrap();
    assert!(after.project_bytes > 0);
    assert!(after.project_bytes <= before.project_bytes);
    assert_eq!(
        counts(&store.committed_handle().unwrap().unwrap().database),
        (2, 2, 1)
    );
}

#[test]
fn usage_retains_link_errors_with_relative_file_kind() {
    let root = fixture();
    let store = store(&root);
    store.startup_active().unwrap();
    let outside = tempfile::Builder::new()
        .prefix("harness-codegraph-usage-link-")
        .tempdir()
        .unwrap()
        .keep();
    let decoy = store.layout().store.join("outside-link");
    symlink_dir(&outside, &decoy).unwrap();
    let error = store.check_limits(None).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("outside-link"), "{message}");
    assert!(
        !message.contains(&outside.display().to_string()),
        "{message}"
    );
    fs::remove_dir(&decoy).unwrap();
}

#[test]
fn indexed_files_returns_stored_paths_and_honors_forced_interruption() {
    let root = fixture();
    let store = store(&root);
    let active = store.startup_active().unwrap();
    write_graph(&active.database, 3, 1, 0);
    store.commit_quiescent(GenerationRole::Active).unwrap();
    let committed = store.committed_handle().unwrap().unwrap();
    let (deadline, cancel) = lease();
    let paths = codegraph_store::indexed_files(&committed.database, deadline, &cancel).unwrap();
    assert_eq!(
        paths,
        vec!["f0".to_owned(), "f1".to_owned(), "f2".to_owned()]
    );
    let expired = Deadline::after(Duration::from_millis(1)).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    let stopped = codegraph_store::indexed_files(&committed.database, expired, &cancel);
    assert!(stopped.is_err(), "{stopped:?}");
    let kind = stopped.unwrap_err().kind();
    assert!(
        kind == std::io::ErrorKind::Interrupted || kind == std::io::ErrorKind::TimedOut,
        "{kind:?}"
    );
}

#[test]
fn exact_symbol_inventory_handles_quotes_and_cancellation_without_writes() {
    let root = fixture();
    let database = root.join("symbol-inventory.db");
    codegraph_store::exec(&database,
        "CREATE TABLE nodes(name TEXT, file_path TEXT); INSERT INTO nodes VALUES('entry','src/a.rs'),('entry','src/a.rs'),('entry','src/b.rs'),('quoted''name','src/c.rs')").unwrap();
    let before = fs::read(&database).unwrap();
    let (deadline, cancel) = lease();
    assert_eq!(
        codegraph_store::symbol_files(&database, "entry", deadline, &cancel).unwrap(),
        vec!["src/a.rs", "src/b.rs"]
    );
    assert_eq!(
        codegraph_store::symbol_files(&database, "quoted'name", deadline, &cancel).unwrap(),
        vec!["src/c.rs"]
    );
    assert!(
        codegraph_store::symbol_files(&database, "' OR 1=1 --", deadline, &cancel)
            .unwrap()
            .is_empty()
    );
    cancel.cancel();
    assert!(codegraph_store::symbol_files(&database, "entry", deadline, &cancel).is_err());
    assert_eq!(fs::read(&database).unwrap(), before);
}
