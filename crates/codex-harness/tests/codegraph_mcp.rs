#![cfg(windows)]
use harness_core::{
    cancellable_pipe::{CancellablePipe, anonymous_pipe},
    codegraph_stdio::Configuration,
    mcp_protocol::Decoder,
    process::{Cancellation, CommandSpec, Deadline, Job, Limits, OwnedProcess},
};
use serde_json::{Value, json};
use std::{fs, time::Duration};

#[path = "support/codegraph_comparison.rs"]
mod comparison;
#[path = "support/codegraph_processes.rs"]
mod processes;

struct Client {
    job: Option<Job>,
    child: OwnedProcess,
    input: Option<CancellablePipe>,
    output: Option<CancellablePipe>,
    decoder: Decoder,
    cancel: Cancellation,
    _root: tempfile::TempDir,
    id: u64,
}
impl Client {
    fn start(mode: &str, real: Option<Configuration>) -> Self {
        Self::start_shared(mode, real, None)
    }
    fn start_shared(
        mode: &str,
        real: Option<Configuration>,
        shared: Option<&std::path::Path>,
    ) -> Self {
        Self::start_command(mode, real, shared, None)
    }

    fn start_command(
        mode: &str,
        real: Option<Configuration>,
        shared: Option<&std::path::Path>,
        package: Option<&std::path::Path>,
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let cfg = real.unwrap_or_else(|| {
            let project = root.path().join("project");
            fs::create_dir_all(project.join(".codegraph-fixture")).unwrap();
            fs::write(
                project.join(".codegraph-fixture/codegraph.db"),
                b"inert database placeholder; this test uses a Rust protocol peer",
            )
            .unwrap();
            let entry = root.path().join(mode);
            fs::write(&entry, b"inert entry marker").unwrap();
            harness_core::codegraph_stdio::configuration(
                std::path::Path::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture")),
                &entry,
                &project,
                ".codegraph-fixture".into(),
            )
            .unwrap()
        });
        let (stdin, write) = anonymous_pipe(4096).unwrap();
        let (read, stdout) = anonymous_pipe(4096).unwrap();
        let mut command = CommandSpec::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture"));
        command.args = vec![
            "--managed".into(),
            serde_json::to_string(&cfg).unwrap().into(),
        ];
        if let Some(path) = shared {
            command.args[0] = "--managed-shared".into();
            command.args.push(path.into());
        }
        if let Some(package) = package {
            command.program = env!("CARGO_BIN_EXE_codex-harness").into();
            command.args = vec![
                "mcp".into(),
                "codegraph".into(),
                "--package-root".into(),
                package.into(),
            ];
            if let Some(shared) = shared {
                command.args.extend([
                    "--project".into(),
                    cfg.project.clone().into_os_string(),
                    "--broker-root".into(),
                    shared.into(),
                ]);
            }
        }
        command.env.insert(
            "CODEX_HOME".into(),
            Some(root.path().join("independent-codex-home").into_os_string()),
        );
        if mode == "long-session" {
            command.env.insert(
                "HARNESS_FIXTURE_CONNECTION_SECONDS".into(),
                Some("900".into()),
            );
        }
        command.current_dir = Some(if package.is_some() && shared.is_none() {
            cfg.project.clone()
        } else {
            root.path().into()
        });
        command.stdin = Some(stdin);
        command.stdout = Some(stdout);
        command.stderr = Some(fs::File::create(root.path().join("stderr")).unwrap());
        let job = Job::new(Limits {
            memory_bytes: Some(2 * 1024 * 1024 * 1024),
            cpu_percent: Some(25.0),
        })
        .unwrap();
        let child = job.spawn(&command).unwrap();
        drop(command);
        let cancel = Cancellation::default();
        let mut client = Self {
            job: Some(job),
            child,
            input: Some(CancellablePipe::writer(write, cancel.clone()).unwrap()),
            output: Some(CancellablePipe::reader(read, cancel.clone()).unwrap()),
            decoder: Decoder::default(),
            cancel,
            _root: root,
            id: 0,
        };
        let initialized=client.request("initialize",json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"owned-codegraph-acceptance","version":"1"}}));
        assert!(
            initialized["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("Prefer Serena")
        );
        client.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        client
    }
    fn send(&mut self, value: Value) {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        self.input
            .as_mut()
            .unwrap()
            .write_all(
                &bytes,
                Deadline::after(Duration::from_secs(5)).unwrap(),
                &self.cancel,
            )
            .unwrap();
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        let seconds = if matches!(
            params["name"].as_str(),
            Some("codegraph_index" | "codegraph_sync")
        ) {
            660
        } else {
            60
        };
        let deadline = Deadline::after(Duration::from_secs(seconds)).unwrap();
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0","id":self.id,"method":method,"params":params}));
        loop {
            if let Some(message) = self.decoder.next_message().unwrap() {
                assert_eq!(message.id(), Some(&json!(self.id)));
                return message.into_value();
            }
            let bytes = self
                .output
                .as_mut()
                .unwrap()
                .read(4096, deadline, &self.cancel)
                .unwrap();
            self.decoder.push(&bytes).unwrap();
        }
    }
    fn call(&mut self, name: &str, args: Value) -> Value {
        let reply = self.request("tools/call", json!({"name":name,"arguments":args}));
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }
    fn finish(mut self) {
        self.input
            .take()
            .unwrap()
            .close(Deadline::after(Duration::from_secs(5)).unwrap())
            .unwrap();
        let outcome = self
            .job
            .take()
            .unwrap()
            .wait(
                &self.child,
                Deadline::after(Duration::from_secs(8)).unwrap(),
                &Cancellation::default(),
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(
            outcome.exit_code,
            0,
            "stderr={}",
            fs::read_to_string(self._root.path().join("stderr")).unwrap()
        );
        assert_eq!(outcome.job.active_processes, 0);
    }
}

fn wait_checkpoint(
    root: &std::path::Path,
    generation: u64,
    present: Option<&str>,
    absent: Option<&str>,
) -> u64 {
    use harness_core::{
        codegraph_generation::{GenerationStore, StorageLimits},
        codegraph_store,
    };
    let store = GenerationStore::open(root, StorageLimits::default()).unwrap();
    let deadline = Deadline::after(Duration::from_secs(90)).unwrap();
    loop {
        if let Ok(Some(handle)) = store.committed_handle()
            && handle.generation.unwrap() > generation
            && let Ok(paths) = codegraph_store::indexed_files(
                &handle.database,
                Deadline::after(Duration::from_secs(2)).unwrap(),
                &Cancellation::default(),
            )
        {
            let contains = |needle: &str| {
                paths
                    .iter()
                    .any(|path| path.replace('\\', "/").ends_with(needle))
            };
            if present.is_none_or(contains) && absent.is_none_or(|name| !contains(name)) {
                return handle.generation.unwrap();
            }
        }
        assert!(
            !deadline.expired(),
            "automatic committed refresh did not complete for owned project"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "requires explicitly selected published package; creates only its own disposable project"]
fn published_native_entry_indexes_watches_syncs_and_reconnects() {
    use harness_core::{
        broker_launch,
        broker_state::BrokerRoot,
        codegraph_generation::{ACTIVE_DIR_NAME, GenerationStore, StorageLimits},
    };
    let package = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_PACKAGE").expect("explicit published package"),
    );
    let owned = tempfile::tempdir().unwrap();
    let project = owned.path().join("project");
    fs::create_dir_all(project.join("src")).unwrap();
    assert!(
        std::process::Command::new("git.exe")
            .args(["init", "--quiet"])
            .arg(&project)
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname=\"graph-fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    fs::write(project.join("src/lib.rs"), "pub fn checkpoint_target() -> u32 { 3 }\npub fn checkpoint_caller() -> u32 { checkpoint_target() }\n").unwrap();
    let cfg = harness_core::codegraph_stdio::configuration(
        &package.join("node.exe"),
        &package.join("lib/dist/bin/codegraph.js"),
        &project,
        ACTIVE_DIR_NAME.into(),
    )
    .unwrap();
    let prepared = BrokerRoot::prepare().unwrap();
    let root = prepared.root();
    let mut client =
        Client::start_command("", Some(cfg.clone()), Some(root.path()), Some(&package));
    let unindexed = client.call("codegraph_status", json!({}));
    assert_eq!(
        unindexed["structuredContent"]["freshness"], "unindexed",
        "{unindexed}"
    );
    assert!(!project.join(ACTIVE_DIR_NAME).exists());
    let indexed = client.call("codegraph_index", json!({}));
    assert_ne!(indexed["isError"], true, "{indexed}");
    let untracked = std::process::Command::new("git.exe")
        .args([
            "ls-files",
            "--others",
            "--exclude-standard",
            "--",
            ".codegraph-harness-active",
            ".codegraph-harness-stage",
            ".codegraph-harness-store",
        ])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        untracked.status.success() && untracked.stdout.is_empty(),
        "private index data was visible to git add"
    );
    let first_generation = indexed["structuredContent"]["generation"].clone();
    assert!(first_generation.as_u64().is_some(), "{indexed}");
    let queried = client.call("codegraph_search", json!({"query":"checkpoint_target"}));
    assert_ne!(queried["isError"], true, "{queried}");
    assert!(queried.to_string().contains("checkpoint_target"));
    let store = GenerationStore::open(
        &harness_core::dependency_discovery::local_path(&project).unwrap(),
        StorageLimits::default(),
    )
    .unwrap();
    let active_database = store.layout().active.join("codegraph.db");
    let committed = store.committed_handle().unwrap().unwrap();
    let active_modified = fs::metadata(&active_database).unwrap().modified().unwrap();
    let committed_modified = fs::metadata(&committed.database)
        .unwrap()
        .modified()
        .unwrap();
    for _ in 0..3 {
        let warm = client.call("codegraph_search", json!({"query":"checkpoint_target"}));
        assert_ne!(warm["isError"], true, "{warm}");
        assert_eq!(
            warm["structuredContent"]["worker"],
            queried["structuredContent"]["worker"]
        );
        assert_eq!(
            fs::metadata(&active_database).unwrap().modified().unwrap(),
            active_modified,
            "unchanged warm queries must not restore a full active database"
        );
        assert_eq!(
            fs::metadata(&committed.database)
                .unwrap()
                .modified()
                .unwrap(),
            committed_modified,
            "unchanged warm queries must not publish another full checkpoint"
        );
        assert!(
            !store.layout().stage.exists(),
            "a query must not stage a rebuild"
        );
    }
    for (name, arguments, oracle) in [
        (
            "codegraph_callers",
            json!({"symbol":"checkpoint_target"}),
            "checkpoint_caller",
        ),
        (
            "codegraph_callees",
            json!({"symbol":"checkpoint_caller"}),
            "checkpoint_target",
        ),
        (
            "codegraph_impact",
            json!({"symbol":"checkpoint_target"}),
            "checkpoint_caller",
        ),
        (
            "codegraph_node",
            json!({"symbol":"checkpoint_target","includeCode":true}),
            "u32",
        ),
        (
            "codegraph_node",
            json!({"file":"src/lib.rs","symbolsOnly":true}),
            "checkpoint_caller",
        ),
        (
            "codegraph_explore",
            json!({"query":"checkpoint_target checkpoint_caller","maxFiles":1}),
            "checkpoint_target",
        ),
    ] {
        let answer = client.call(name, arguments);
        assert_ne!(answer["isError"], true, "{name}: {answer}");
        assert!(serde_json::to_vec(&answer).unwrap().len() <= 4096);
        assert!(answer.to_string().contains(oracle), "{name}: {answer}");
    }
    let mismatch = client.call(
        "codegraph_callers",
        json!({"symbol":"checkpoint_target","file":"src/absent.rs"}),
    );
    assert_eq!(mismatch["isError"], true, "{mismatch}");
    let probe = project.join("src/watched.rs");
    fs::write(&probe, "pub fn watched_addition() -> u32 { 7 }\n").unwrap();
    let until = Deadline::after(Duration::from_secs(12)).unwrap();
    loop {
        let found = client.call("codegraph_search", json!({"query":"watched_addition"}));
        assert_ne!(found["isError"], true, "{found}");
        if found.to_string().contains("src/watched.rs") {
            break;
        }
        assert!(
            !until.expired(),
            "watcher did not index owned source: {found}"
        );
        std::thread::sleep(Duration::from_millis(150));
    }
    fs::rename(&probe, project.join("src/renamed.rs")).unwrap();
    let synced = client.call("codegraph_sync", json!({}));
    assert_ne!(synced["isError"], true, "{synced}");
    assert_ne!(synced["structuredContent"]["generation"], first_generation);
    let renamed = client.call("codegraph_search", json!({"query":"watched_addition"}));
    assert!(renamed.to_string().contains("src/renamed.rs"), "{renamed}");
    assert!(!renamed.to_string().contains("src/watched.rs"), "{renamed}");
    client.finish();
    assert!(!matches!(
        broker_launch::retire(
            root,
            Deadline::after(Duration::from_secs(10)).unwrap(),
            &Cancellation::default()
        )
        .unwrap(),
        broker_launch::Retirement::Pending { .. }
    ));
    fs::remove_file(project.join("src/renamed.rs")).unwrap();
    let mut again = Client::start_command("", Some(cfg), Some(root.path()), Some(&package));
    let caught_up = again.call("codegraph_search", json!({"query":"watched_addition"}));
    assert_ne!(caught_up["isError"], true, "{caught_up}");
    assert!(
        !caught_up.to_string().contains("src/renamed.rs"),
        "{caught_up}"
    );
    let store = GenerationStore::open(
        &harness_core::dependency_discovery::local_path(&project).unwrap(),
        StorageLimits::default(),
    )
    .unwrap();
    assert!(store.committed_handle().unwrap().is_some());
    again.finish();
    assert!(!matches!(
        broker_launch::retire(
            root,
            Deadline::after(Duration::from_secs(10)).unwrap(),
            &Cancellation::default()
        )
        .unwrap(),
        broker_launch::Retirement::Pending { .. }
    ));
}

#[test]
#[ignore = "explicit published package and existing owned index; exercises cwd selection and the stable account broker"]
fn published_native_entry_adopts_existing_owned_index_from_cwd() {
    let package = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_PACKAGE").expect("explicit package"),
    );
    let project = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_PROJECT").expect("explicit owned index root"),
    );
    let config = harness_core::codegraph_stdio::configuration(
        &package.join("node.exe"),
        &package.join("lib/dist/bin/codegraph.js"),
        &project,
        harness_core::codegraph_generation::ACTIVE_DIR_NAME.into(),
    )
    .unwrap();
    let store = harness_core::codegraph_generation::GenerationStore::open(
        &config.project,
        harness_core::codegraph_generation::StorageLimits::default(),
    )
    .unwrap();
    assert!(
        store.committed_handle().unwrap().is_some(),
        "only an existing owned checkpoint may be selected by this probe"
    );
    let mut client = Client::start_command("", Some(config.clone()), None, Some(&package));
    let answer = client.call("codegraph_status", json!({}));
    assert_ne!(answer["isError"], true, "{answer}");
    assert_eq!(
        answer["structuredContent"]["root"],
        config.project.to_string_lossy().as_ref()
    );
    client.finish();
    let retired = std::process::Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["mcp", "retire-codegraph"])
        .output()
        .unwrap();
    assert!(
        retired.status.success(),
        "{}",
        String::from_utf8_lossy(&retired.stderr)
    );
    for directory in [
        harness_core::codegraph_generation::ACTIVE_DIR_NAME,
        harness_core::codegraph_generation::STORE_DIR_NAME,
    ] {
        assert_eq!(
            fs::read(project.join(directory).join(".gitignore")).unwrap(),
            b"*\n"
        );
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.job.take();
        if let Some(pipe) = self.input.take() {
            let _ = pipe.close(Deadline::after(Duration::from_secs(5)).unwrap());
        }
        if let Some(pipe) = self.output.take() {
            let _ = pipe.close(Deadline::after(Duration::from_secs(5)).unwrap());
        }
    }
}

#[test]
fn managed_catalogue_defaults_errors_and_private_detail_recovery() {
    let mut client = Client::start("large", None);
    let catalogue = client.request("tools/list", json!({}));
    assert_eq!(catalogue["result"]["tools"].as_array().unwrap().len(), 10);
    let bad = client.call("codegraph_search", json!({"query":"entry","limit":0}));
    assert_eq!(bad["isError"], true);
    let answer = client.call("codegraph_search", json!({"query":"entry"}));
    assert!(serde_json::to_vec(&answer).unwrap().len() <= 4096);
    assert_eq!(answer["structuredContent"]["truncated"], true);
    let id = answer["structuredContent"]["detail_id"]
        .as_str()
        .expect("bounded answer must retain access");
    let query_counter = client._root.path().join("project/owned-query-count");
    let upstream_queries = fs::read(&query_counter).unwrap();
    let mut offset = 0;
    let mut recovered = String::new();
    for _ in 0..128 {
        let page = client.call("codegraph_detail", json!({"id":id,"offset":offset}));
        assert!(serde_json::to_vec(&page).unwrap().len() <= 4096);
        recovered.push_str(page["content"][0]["text"].as_str().unwrap());
        let next = page["structuredContent"]["next_offset"].as_u64().unwrap();
        if next == offset {
            break;
        }
        offset = next;
        if recovered.contains("TAIL_ORACLE") {
            break;
        }
    }
    assert!(recovered.contains("TAIL_ORACLE"));
    assert!(recovered.contains("approximate edges"));
    assert_eq!(
        fs::read(&query_counter).unwrap(),
        upstream_queries,
        "detail pagination must never rerun the backend query"
    );
    let mut other = Client::start("normal", None);
    let stolen = other.call("codegraph_detail", json!({"id":id}));
    assert_eq!(stolen["isError"], true);
    other.finish();
    client.finish();
}

#[test]
fn managed_protocol_failures_stay_bounded_explicit_and_cleanup_confirmed() {
    for mode in ["malformed", "oversized", "error", "empty"] {
        let mut client = Client::start(mode, None);
        let response = client.call("codegraph_search", json!({"query":"entry"}));
        assert_eq!(response["isError"], true, "{response}");
        assert!(serde_json::to_vec(&response).unwrap().len() <= 4096);
        client.finish();
    }
}

#[test]
fn ambiguous_fanout_requires_narrowing_and_retains_the_original() {
    let mut client = Client::start("fanout", None);
    let answer = client.call("codegraph_callers", json!({"symbol":"entry"}));
    assert_eq!(answer["isError"], true, "{answer}");
    assert!(serde_json::to_vec(&answer).unwrap().len() <= 4096);
    let id = answer["structuredContent"]["detail_id"].as_str().unwrap();
    let page = client.call("codegraph_detail", json!({"id":id}));
    assert!(page.to_string().contains("caller_c"), "{page}");
    client.finish();
}

#[test]
#[ignore = "requires explicitly selected published package and owned indexed project"]
fn published_managed_mcp_answers_with_caps_and_filtered_handlers() {
    let package = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_PACKAGE").expect("explicit package"),
    );
    let project = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_PROJECT").expect("explicit owned project"),
    );
    let data = std::env::var("CODEGRAPH_ACCEPTANCE_DATA_NAME").expect("explicit data");
    let config = harness_core::codegraph_stdio::configuration(
        &package.join("node.exe"),
        &package.join("lib/dist/bin/codegraph.js"),
        &project,
        data,
    )
    .unwrap();
    let mut client = Client::start("", Some(config));
    for (name, args) in [
        ("codegraph_search", json!({"query":"evaluation_target"})),
        ("codegraph_callers", json!({"symbol":"evaluation_target"})),
        (
            "codegraph_node",
            json!({"symbol":"evaluation_target","includeCode":true}),
        ),
        (
            "codegraph_explore",
            json!({"query":"evaluation_target evaluation_caller","maxFiles":1}),
        ),
    ] {
        let answer = client.call(name, args);
        assert_ne!(answer["isError"], true, "{answer}");
        assert!(serde_json::to_vec(&answer).unwrap().len() <= 4096);
        assert!(answer.to_string().contains("evaluation_"), "{answer}");
    }
    let mismatched = client.call(
        "codegraph_callers",
        json!({"symbol":"evaluation_target","file":"src/not_present.rs"}),
    );
    assert_eq!(mismatched["isError"], true, "{mismatched}");
    client.finish();
}

#[test]
fn shared_worker_reuses_root_across_homes_preserves_other_client_and_retires() {
    let source = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join(".codegraph-fixture")).unwrap();
    fs::write(
        source.path().join(".codegraph-fixture/codegraph.db"),
        b"inert protocol peer database marker",
    )
    .unwrap();
    let entry = source.path().join("normal");
    fs::write(&entry, b"inert entry marker").unwrap();
    let config = harness_core::codegraph_stdio::configuration(
        std::path::Path::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture")),
        &entry,
        source.path(),
        ".codegraph-fixture".into(),
    )
    .unwrap();
    shared_scenario(config);
}

fn shared_scenario(config: Configuration) {
    use harness_core::{broker_endpoint, broker_launch, broker_state::BrokerRoot};
    let prepared = BrokerRoot::prepare().unwrap();
    let root = prepared.root();
    let mut first = Client::start_shared("", Some(config.clone()), Some(root.path()));
    let a = first.call("codegraph_search", json!({"query":"evaluation_target"}));
    assert_ne!(a["isError"], true, "{a}");
    let worker = a["structuredContent"]["worker"].clone();
    assert!(worker["pid"].as_u64().is_some(), "{a}");
    let mut second = Client::start_shared("", Some(config.clone()), Some(root.path()));
    let b = second.call("codegraph_search", json!({"query":"evaluation_target"}));
    assert_eq!(b["structuredContent"]["worker"], worker, "{b}");
    first.finish();
    let c = second.call("codegraph_search", json!({"query":"evaluation_target"}));
    assert_eq!(c["structuredContent"]["worker"], worker, "{c}");
    let different = tempfile::tempdir().unwrap();
    let mut other_config = config.clone();
    other_config.project =
        harness_core::dependency_discovery::local_path(different.path()).unwrap();
    let mut other = Client::start_shared("", Some(other_config), Some(root.path()));
    let busy = other.call("codegraph_search", json!({"query":"evaluation_target"}));
    assert_eq!(busy["isError"], true, "{busy}");
    other.finish();
    let d = second.call("codegraph_search", json!({"query":"evaluation_target"}));
    assert_ne!(d["isError"], true, "{d}");
    // Selecting a different root drains the previous backend, while its client
    // and observation remain usable. Returning may create a fresh owned worker.
    assert!(
        d["structuredContent"]["worker"]["pid"].as_u64().is_some(),
        "{d}"
    );
    second.finish();
    let result = broker_launch::retire(
        root,
        Deadline::after(Duration::from_secs(10)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    assert!(!matches!(result, broker_launch::Retirement::Pending { .. }));
    assert!(!matches!(
        broker_endpoint::observe(root).unwrap(),
        broker_endpoint::Observation::Ready { .. }
    ));
    // A later session reuses the protected location after an owned retirement;
    // it does not need a newly prepared root or an accumulating log directory.
    let mut later = Client::start_shared("", Some(config), Some(root.path()));
    let again = later.call("codegraph_search", json!({"query":"evaluation_target"}));
    assert_ne!(again["isError"], true, "{again}");
    assert_ne!(again["structuredContent"]["worker"], worker);
    later.finish();
    assert!(!matches!(
        broker_launch::retire(
            root,
            Deadline::after(Duration::from_secs(10)).unwrap(),
            &Cancellation::default()
        )
        .unwrap(),
        broker_launch::Retirement::Pending { .. }
    ));
}

#[test]
#[ignore = "requires explicitly selected published package and owned indexed project"]
fn published_shared_worker_reuses_the_real_direct_runtime() {
    let package = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_PACKAGE").expect("explicit package"),
    );
    let project = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_PROJECT").expect("explicit owned project"),
    );
    let data = std::env::var("CODEGRAPH_ACCEPTANCE_DATA_NAME").expect("explicit data");
    shared_scenario(
        harness_core::codegraph_stdio::configuration(
            &package.join("node.exe"),
            &package.join("lib/dist/bin/codegraph.js"),
            &project,
            data,
        )
        .unwrap(),
    );
}

#[test]
fn shared_worker_retires_after_sixty_seconds_without_work() {
    use harness_core::{broker_endpoint, broker_launch, broker_state::BrokerRoot};
    let source = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join(".codegraph-fixture")).unwrap();
    fs::write(
        source.path().join(".codegraph-fixture/codegraph.db"),
        b"inert protocol peer",
    )
    .unwrap();
    let entry = source.path().join("normal");
    fs::write(&entry, b"inert entry marker").unwrap();
    let config = harness_core::codegraph_stdio::configuration(
        std::path::Path::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture")),
        &entry,
        source.path(),
        ".codegraph-fixture".into(),
    )
    .unwrap();
    let prepared = BrokerRoot::prepare().unwrap();
    let root = prepared.root();
    let mut client = Client::start_shared("", Some(config), Some(root.path()));
    let answer = client.call("codegraph_search", json!({"query":"entry"}));
    assert_ne!(answer["isError"], true, "{answer}");
    let broker_endpoint::Observation::Ready { owner, .. } = broker_endpoint::observe(root).unwrap()
    else {
        panic!("expected live owned service")
    };
    client.finish();
    let began = std::time::Instant::now();
    let deadline = Deadline::after(Duration::from_secs(70)).unwrap();
    while owner.is_running().unwrap() && !deadline.expired() {
        std::thread::sleep(Duration::from_millis(200));
    }
    let retired = !owner.is_running().unwrap();
    if !retired {
        let _ = broker_launch::retire(
            root,
            Deadline::after(Duration::from_secs(10)).unwrap(),
            &Cancellation::default(),
        );
    }
    assert!(retired, "owned broker did not retire after its idle limit");
    assert!(began.elapsed() >= Duration::from_secs(50));
    assert!(!matches!(
        broker_endpoint::observe(root).unwrap(),
        broker_endpoint::Observation::Ready { .. }
    ));
}

#[test]
#[ignore = "explicit published package; three owned real indexed projects; optional 610-second live-session acceptance"]
fn published_three_projects_refresh_without_queries_and_share_clients() {
    use harness_core::{
        broker_launch, broker_state::BrokerRoot, codegraph_generation::ACTIVE_DIR_NAME,
    };
    use std::{path::PathBuf, time::Instant};
    struct Retire(PathBuf);
    impl Drop for Retire {
        fn drop(&mut self) {
            if let Ok(root) = BrokerRoot::open(&self.0) {
                let _ = broker_launch::retire(
                    &root,
                    Deadline::after(Duration::from_secs(10)).unwrap(),
                    &Cancellation::default(),
                );
            }
        }
    }
    let package =
        PathBuf::from(std::env::var_os("CODEGRAPH_ACCEPTANCE_PACKAGE").expect("explicit package"));
    let owned = tempfile::tempdir().unwrap();
    let prepared = BrokerRoot::prepare().unwrap();
    let root = prepared.root();
    let _retire = Retire(root.path().into());
    let mut configs = Vec::new();
    let mut clients = Vec::new();
    let mut generations = Vec::new();
    let mut samples = Vec::new();
    for number in 0..3 {
        let project = owned.path().join(format!("project-{number}"));
        fs::create_dir_all(project.join("src")).unwrap();
        assert!(
            std::process::Command::new("git.exe")
                .args(["init", "--quiet"])
                .arg(&project)
                .status()
                .unwrap()
                .success()
        );
        fs::write(
            project.join("Cargo.toml"),
            format!("[package]\nname=\"project-{number}\"\nversion=\"0.1.0\"\nedition=\"2024\"\n"),
        )
        .unwrap();
        fs::write(
            project.join("src/lib.rs"),
            format!("pub fn root_{number}() -> u32 {{ {number} }}\n"),
        )
        .unwrap();
        let configuration = harness_core::codegraph_stdio::configuration(
            &package.join("node.exe"),
            &package.join("lib/dist/bin/codegraph.js"),
            &project,
            ACTIVE_DIR_NAME.into(),
        )
        .unwrap();
        let mut client = Client::start_command(
            "",
            Some(configuration.clone()),
            Some(root.path()),
            Some(&package),
        );
        let indexed = client.call("codegraph_index", json!({}));
        assert_ne!(indexed["isError"], true, "{indexed}");
        let generation = indexed["structuredContent"]["generation"].as_u64().unwrap();
        generations.push(wait_checkpoint(
            &configuration.project,
            generation,
            Some("src/lib.rs"),
            None,
        ));
        configs.push(configuration);
        clients.push(client);
        let answer = clients[number].call(
            "codegraph_search",
            json!({"query":format!("root_{number}")}),
        );
        assert_ne!(answer["isError"], true, "{answer}");
        let harness_core::broker_endpoint::Observation::Ready { owner, .. } =
            harness_core::broker_endpoint::observe(root).unwrap()
        else {
            panic!("live broker required");
        };
        let mut owners = vec![owner.identity()];
        owners.extend(clients.iter().map(|client| client.child.identity()));
        samples.push(processes::sample(&owners).unwrap());
    }
    let began = Instant::now();
    let first = clients[0].call("codegraph_search", json!({"query":"root_0"}));
    let mut duplicate = Client::start_command(
        "",
        Some(configs[0].clone()),
        Some(root.path()),
        Some(&package),
    );
    let shared = duplicate.call("codegraph_search", json!({"query":"root_0"}));
    assert_eq!(
        first["structuredContent"]["worker"], shared["structuredContent"]["worker"],
        "{shared}"
    );
    let harness_core::broker_endpoint::Observation::Ready { owner, .. } =
        harness_core::broker_endpoint::observe(root).unwrap()
    else {
        panic!("live broker required");
    };
    let mut owners = vec![owner.identity(), duplicate.child.identity()];
    owners.extend(clients.iter().map(|client| client.child.identity()));
    samples.push(processes::sample(&owners).unwrap());
    for (extra, sample) in samples.iter().enumerate() {
        assert_eq!(
            sample["images"]["node.exe"], samples[0]["images"]["node.exe"],
            "extra clients must not duplicate the heavy backend: {samples:?}"
        );
        assert_eq!(
            sample["images"]["codex-harness.exe"],
            samples[0]["images"]["codex-harness.exe"].as_u64().unwrap() + extra as u64
        );
        assert!(sample["private_bytes"].as_u64().unwrap() < 2 * 1024 * 1024 * 1024);
    }

    for (number, configuration) in configs.iter().enumerate() {
        for revision in 0..16 {
            fs::write(
                configuration.project.join("src/probe.rs"),
                format!("pub fn probe_{number}_{revision}() {{}}\n"),
            )
            .unwrap();
        }
    }
    for (number, configuration) in configs.iter().enumerate() {
        generations[number] = wait_checkpoint(
            &configuration.project,
            generations[number],
            Some("src/probe.rs"),
            None,
        );
    }
    for configuration in &configs {
        fs::rename(
            configuration.project.join("src/probe.rs"),
            configuration.project.join("src/renamed.rs"),
        )
        .unwrap();
    }
    for (number, configuration) in configs.iter().enumerate() {
        generations[number] = wait_checkpoint(
            &configuration.project,
            generations[number],
            Some("src/renamed.rs"),
            Some("src/probe.rs"),
        );
        let answer = clients[number].call(
            "codegraph_search",
            json!({"query":format!("probe_{number}_15")}),
        );
        assert_ne!(answer["isError"], true, "{answer}");
        assert!(answer.to_string().contains("src/renamed.rs"), "{answer}");
        assert_eq!(
            answer["structuredContent"]["root"],
            json!(configuration.project)
        );
    }
    // Closing the original same-root client leaves its replacement connected.
    clients.remove(0).finish();
    clients.insert(0, duplicate);
    for configuration in &configs {
        fs::remove_file(configuration.project.join("src/renamed.rs")).unwrap();
    }
    for (number, configuration) in configs.iter().enumerate() {
        generations[number] = wait_checkpoint(
            &configuration.project,
            generations[number],
            None,
            Some("src/renamed.rs"),
        );
    }
    if std::env::var_os("CODEGRAPH_ACCEPTANCE_LONG_LIVED").is_some() {
        while began.elapsed() <= Duration::from_secs(610) {
            std::thread::sleep(Duration::from_secs(1));
        }
        for configuration in &configs {
            fs::write(
                configuration.project.join("src/after_lease.rs"),
                "pub fn after_lease() {}\n",
            )
            .unwrap();
        }
        for (number, configuration) in configs.iter().enumerate() {
            generations[number] = wait_checkpoint(
                &configuration.project,
                generations[number],
                Some("src/after_lease.rs"),
                None,
            );
        }
    }
    // Last-client disconnect stops only one root; reopen must catch up on its own.
    clients.pop().unwrap().finish();
    std::thread::sleep(Duration::from_millis(400));
    fs::write(
        configs[2].project.join("src/offline.rs"),
        "pub fn offline_change() {}\n",
    )
    .unwrap();
    fs::write(
        configs[1].project.join("src/still_live.rs"),
        "pub fn still_live() {}\n",
    )
    .unwrap();
    wait_checkpoint(
        &configs[1].project,
        generations[1],
        Some("src/still_live.rs"),
        None,
    );
    clients.push(Client::start_command(
        "",
        Some(configs[2].clone()),
        Some(root.path()),
        Some(&package),
    ));
    wait_checkpoint(
        &configs[2].project,
        generations[2],
        Some("src/offline.rs"),
        None,
    );
    for client in clients {
        client.finish();
    }
    assert!(!matches!(
        broker_launch::retire(
            root,
            Deadline::after(Duration::from_secs(10)).unwrap(),
            &Cancellation::default()
        )
        .unwrap(),
        broker_launch::Retirement::Pending { .. }
    ));
    if let Some(path) = std::env::var_os("CODEGRAPH_CONCURRENT_REPORT") {
        fs::write(path, serde_json::to_vec_pretty(&json!({"samples":samples,"long_lived":std::env::var_os("CODEGRAPH_ACCEPTANCE_LONG_LIVED").is_some(),"elapsed_seconds":began.elapsed().as_secs_f64(),"generations":generations,"retired":true})).unwrap()).unwrap();
    }
}

#[test]
fn busy_project_allows_other_connections_and_eof_reclaims_its_work() {
    use harness_core::{broker_launch, broker_state::BrokerRoot};
    let owned = tempfile::tempdir().unwrap();
    let entry = owned.path().join("per-project");
    fs::write(&entry, b"owned inert fixture selection").unwrap();
    let mut configs = Vec::new();
    for (number, mode) in ["hang", "normal"].into_iter().enumerate() {
        let project = owned.path().join(format!("project-{number}"));
        fs::create_dir_all(project.join(".codegraph-fixture")).unwrap();
        fs::write(
            project.join(".codegraph-fixture/codegraph.db"),
            b"inert database",
        )
        .unwrap();
        fs::write(project.join(".fixture-mode"), mode).unwrap();
        configs.push(
            harness_core::codegraph_stdio::configuration(
                std::path::Path::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture")),
                &entry,
                &project,
                ".codegraph-fixture".into(),
            )
            .unwrap(),
        );
    }
    let prepared = BrokerRoot::prepare().unwrap();
    let root = prepared.root();
    let mut first = Client::start_shared("", Some(configs[0].clone()), Some(root.path()));
    first.send(json!({"jsonrpc":"2.0","id":900,"method":"tools/call","params":{"name":"codegraph_search","arguments":{"query":"blocked"}}}));
    let deadline = Deadline::after(Duration::from_secs(8)).unwrap();
    while !configs[0].project.join("owned-worker-started").exists() {
        assert!(
            !deadline.expired(),
            "fixture must enter its blocking operation"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let began = std::time::Instant::now();
    let mut other = Client::start_shared("", Some(configs[1].clone()), Some(root.path()));
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "connection control must not wait for an indexing episode"
    );
    first.finish();
    let answer = other.call("codegraph_search", json!({"query":"other-root"}));
    assert_ne!(answer["isError"], true, "{answer}");
    fs::write(configs[0].project.join(".fixture-mode"), "normal").unwrap();
    let mut reopened = Client::start_shared("", Some(configs[0].clone()), Some(root.path()));
    let caught_up = reopened.call("codegraph_search", json!({"query":"reopened"}));
    assert_ne!(caught_up["isError"], true, "{caught_up}");
    reopened.finish();
    other.finish();
    assert!(!matches!(
        broker_launch::retire(
            root,
            Deadline::after(Duration::from_secs(10)).unwrap(),
            &Cancellation::default()
        )
        .unwrap(),
        broker_launch::Retirement::Pending { .. }
    ));
}

#[test]
fn failed_project_does_not_restart_or_disable_another_root() {
    use harness_core::{broker_launch, broker_state::BrokerRoot};
    let owned = tempfile::tempdir().unwrap();
    let entry = owned.path().join("per-project");
    fs::write(&entry, b"owned inert fixture selection").unwrap();
    let mut configs = Vec::new();
    for (number, mode) in ["wrong-id", "normal"].into_iter().enumerate() {
        let project = owned.path().join(format!("project-{number}"));
        fs::create_dir_all(project.join(".codegraph-fixture")).unwrap();
        fs::write(
            project.join(".codegraph-fixture/codegraph.db"),
            b"inert database",
        )
        .unwrap();
        fs::write(project.join(".fixture-mode"), mode).unwrap();
        configs.push(
            harness_core::codegraph_stdio::configuration(
                std::path::Path::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture")),
                &entry,
                &project,
                ".codegraph-fixture".into(),
            )
            .unwrap(),
        );
    }
    let prepared = BrokerRoot::prepare().unwrap();
    let root = prepared.root();
    let mut first = Client::start_shared("", Some(configs[0].clone()), Some(root.path()));
    let failed = first.call("codegraph_search", json!({"query":"failed"}));
    assert_eq!(
        failed["structuredContent"]["freshness"], "failed",
        "{failed}"
    );
    let mut same = Client::start_shared("", Some(configs[0].clone()), Some(root.path()));
    let still_failed = same.call("codegraph_search", json!({"query":"still-failed"}));
    assert_eq!(
        still_failed["structuredContent"]["freshness"], "failed",
        "{still_failed}"
    );
    assert_eq!(
        fs::read_to_string(configs[0].project.join("owned-worker-launches"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let mut other = Client::start_shared("", Some(configs[1].clone()), Some(root.path()));
    let answer = other.call("codegraph_search", json!({"query":"unaffected"}));
    assert_ne!(answer["isError"], true, "{answer}");
    first.finish();
    same.finish();
    other.finish();
    assert!(!matches!(
        broker_launch::retire(
            root,
            Deadline::after(Duration::from_secs(10)).unwrap(),
            &Cancellation::default()
        )
        .unwrap(),
        broker_launch::Retirement::Pending { .. }
    ));
}

fn automatic_failure(mode: &str, disconnect: bool) {
    use harness_core::{
        broker_endpoint, broker_http, broker_launch,
        broker_state::BrokerRoot,
        codegraph_generation::{ACTIVE_DIR_NAME, GenerationRole, GenerationStore, StorageLimits},
        codegraph_store,
    };
    let owned = tempfile::tempdir().unwrap();
    let entry = owned.path().join("per-project-cli");
    fs::write(&entry, b"owned inert CLI failure selection").unwrap();
    let mut configs = Vec::new();
    let mut stores = Vec::new();
    for (number, mode) in [mode, "cli-normal"].into_iter().enumerate() {
        let project = owned.path().join(format!("project-{number}"));
        fs::create_dir(&project).unwrap();
        fs::write(project.join(".fixture-mode"), mode).unwrap();
        let configuration = harness_core::codegraph_stdio::configuration(
            std::path::Path::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture")),
            &entry,
            &project,
            ACTIVE_DIR_NAME.into(),
        )
        .unwrap();
        let store =
            GenerationStore::open(&configuration.project, StorageLimits::default()).unwrap();
        let staged = store.stage_full_rebuild().unwrap();
        fs::remove_file(&staged.database).unwrap();
        codegraph_store::exec(&staged.database, "CREATE TABLE files(path TEXT); INSERT INTO files VALUES('committed.rs'); CREATE TABLE nodes(id INTEGER); CREATE TABLE edges(id INTEGER); CREATE TABLE unresolved_refs(status TEXT);").unwrap();
        store.commit_quiescent(GenerationRole::Stage).unwrap();
        configs.push(configuration);
        stores.push(store);
    }
    let committed = stores[0].committed_handle().unwrap().unwrap();
    let saved = fs::read(&committed.database).unwrap();
    let generation = committed.generation.unwrap();
    let prepared = BrokerRoot::prepare().unwrap();
    let root = prepared.root();
    let mut failed_client = Some(Client::start_shared(
        "long-session",
        Some(configs[0].clone()),
        Some(root.path()),
    ));
    let marker = owned.path().join("project-0.wrote");
    let deadline = Deadline::after(Duration::from_secs(8)).unwrap();
    while !marker.exists() {
        assert!(
            !deadline.expired(),
            "connect-time automatic work must write before failure"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let written = fs::metadata(&marker).unwrap().modified().unwrap();
    let mut healthy =
        Client::start_shared("long-session", Some(configs[1].clone()), Some(root.path()));
    if disconnect {
        failed_client.take().unwrap().finish();
    }
    let deadline = Deadline::after(Duration::from_secs(if mode == "cli-hang" && !disconnect {
        615
    } else {
        12
    }))
    .unwrap();
    let cause = loop {
        let broker_endpoint::Observation::Ready { endpoint, .. } =
            broker_endpoint::observe(root).unwrap()
        else {
            panic!("owned broker must remain live");
        };
        let status = broker_http::exchange(
            endpoint.port,
            endpoint.token(),
            "status",
            &json!({}),
            Deadline::after(Duration::from_secs(2)).unwrap(),
            &Cancellation::default(),
        )
        .unwrap();
        assert!(
            status.get("error").is_none(),
            "broker status failed: {status}"
        );
        if status["backend"]["busy"] == true {
            assert!(
                !deadline.expired(),
                "broker remained busy past the episode deadline: {status}"
            );
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let project = status["backend"]["projects"]
            .as_array()
            .unwrap_or_else(|| {
                panic!("broker omitted projects without an explicit busy result: {status}")
            })
            .iter()
            .find(|project| project["root"] == json!(configs[0].project))
            .unwrap_or_else(|| panic!("connected project disappeared: {status}"));
        if let Some(failure) = project["runtime"]["failed"].as_str() {
            break failure.to_owned();
        }
        assert!(
            !deadline.expired(),
            "automatic failed work must complete within its finite boundary: {status}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(cause.contains("did not complete"), "{cause}");
    if mode == "cli-memory" {
        assert!(
            cause.contains("Windows denied committed allocation"),
            "{cause}"
        );
    }
    if mode == "cli-hang" && !disconnect {
        assert!(cause.contains("Timeout"), "{cause}");
    }
    if disconnect {
        assert!(cause.contains("Cancelled"), "{cause}");
    }
    assert_eq!(
        stores[0].committed_handle().unwrap().unwrap().generation,
        Some(generation)
    );
    assert_eq!(fs::read(&committed.database).unwrap(), saved);
    assert_eq!(
        codegraph_store::counts(&committed.database).unwrap()["files"],
        1
    );
    let deadline = Deadline::after(Duration::from_secs(12)).unwrap();
    while stores[1]
        .committed_handle()
        .unwrap()
        .unwrap()
        .generation
        .unwrap()
        <= 1
    {
        assert!(
            !deadline.expired(),
            "failed root must release admission to pending healthy root"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let answer = healthy.call("codegraph_search", json!({"query":"healthy"}));
    assert_ne!(answer["isError"], true, "{answer}");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        fs::metadata(&marker).unwrap().modified().unwrap(),
        written,
        "automatic failure must not restart"
    );
    if let Some(client) = failed_client.take() {
        client.finish();
    }
    fs::write(configs[0].project.join(".fixture-mode"), "cli-normal").unwrap();
    let recovered =
        Client::start_shared("long-session", Some(configs[0].clone()), Some(root.path()));
    let deadline = Deadline::after(Duration::from_secs(12)).unwrap();
    while stores[0]
        .committed_handle()
        .unwrap()
        .unwrap()
        .generation
        .unwrap()
        <= generation
    {
        assert!(
            !deadline.expired(),
            "reopening must catch up from saved checkpoint"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        codegraph_store::counts(&stores[0].committed_handle().unwrap().unwrap().database).unwrap()
            ["files"],
        1
    );
    recovered.finish();
    healthy.finish();
    assert!(!matches!(
        broker_launch::retire(
            root,
            Deadline::after(Duration::from_secs(10)).unwrap(),
            &Cancellation::default()
        )
        .unwrap(),
        broker_launch::Retirement::Pending { .. }
    ));
}

#[test]
fn automatic_memory_failure_preserves_checkpoint_and_other_project_service() {
    automatic_failure("cli-memory", false);
}

#[test]
fn change_during_query_reports_pending_with_current_source_fallback() {
    use harness_core::{
        broker_launch,
        broker_state::BrokerRoot,
        codegraph_generation::{ACTIVE_DIR_NAME, GenerationRole, GenerationStore, StorageLimits},
        codegraph_store,
    };
    let owned = tempfile::tempdir().unwrap();
    let entry = owned.path().join("per-project-cli");
    fs::write(&entry, b"owned inert CLI fixture selection").unwrap();
    let project = owned.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join(".fixture-mode"), "cli-pending").unwrap();
    let configuration = harness_core::codegraph_stdio::configuration(
        std::path::Path::new(env!("CARGO_BIN_EXE_harness-codegraph-fixture")),
        &entry,
        &project,
        ACTIVE_DIR_NAME.into(),
    )
    .unwrap();
    let store = GenerationStore::open(&configuration.project, StorageLimits::default()).unwrap();
    let staged = store.stage_full_rebuild().unwrap();
    fs::remove_file(&staged.database).unwrap();
    codegraph_store::exec(&staged.database, "CREATE TABLE files(path TEXT); INSERT INTO files VALUES('committed.rs'); CREATE TABLE nodes(id INTEGER); CREATE TABLE edges(id INTEGER); CREATE TABLE unresolved_refs(status TEXT);").unwrap();
    store.commit_quiescent(GenerationRole::Stage).unwrap();
    let prepared = BrokerRoot::prepare().unwrap();
    let root = prepared.root();
    let mut client = Client::start_shared("", Some(configuration), Some(root.path()));
    let deadline = Deadline::after(Duration::from_secs(8)).unwrap();
    while store
        .committed_handle()
        .unwrap()
        .unwrap()
        .generation
        .unwrap()
        <= 1
    {
        assert!(!deadline.expired(), "initial automatic catch-up required");
        std::thread::sleep(Duration::from_millis(100));
    }
    let result = client.call("codegraph_search", json!({"query":"saved"}));
    assert_ne!(result["isError"], true, "{result}");
    assert_eq!(
        result["structuredContent"]["freshness"], "pending",
        "{result}"
    );
    assert!(
        result["structuredContent"]["source_fallback"]
            .as_str()
            .is_some_and(|text| text.contains("Serena/source")),
        "{result}"
    );
    assert!(project.join("owned-pending.rs").exists());
    client.finish();
    assert!(!matches!(
        broker_launch::retire(
            root,
            Deadline::after(Duration::from_secs(10)).unwrap(),
            &Cancellation::default()
        )
        .unwrap(),
        broker_launch::Retirement::Pending { .. }
    ));
}

#[test]
fn last_client_disconnect_cancels_automatic_work_and_reopening_recovers() {
    automatic_failure("cli-hang", true);
}

#[test]
#[ignore = "holds an owned hung automatic sync until the actual 600-second deadline"]
fn automatic_episode_deadline_preserves_checkpoint_and_other_project_service() {
    automatic_failure("cli-hang", false);
}
