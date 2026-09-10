//! Opt-in acceptance on explicit real roots. Private oracles and results stay
//! outside source control; all source mutations belong to an owned probe.
//! Sequential comparison remains the source-oracle accounting. A later concurrent
//! phase keeps the real roots plus one owned fixture active together.
use super::Client;
use harness_core::{
    broker_launch,
    broker_state::BrokerRoot,
    codegraph_generation::{ACTIVE_DIR_NAME, GenerationStore, StorageLimits},
    codegraph_store,
    codegraph_transport::Worker,
    process::{Cancellation, Deadline},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

fn deadline(seconds: u64) -> Deadline {
    Deadline::after(Duration::from_secs(seconds)).unwrap()
}
fn bytes(value: &Value) -> usize {
    serde_json::to_vec(value).unwrap().len()
}
fn save(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn retire(root: &BrokerRoot) {
    assert!(!matches!(
        broker_launch::retire(root, deadline(10), &Cancellation::default()).unwrap(),
        broker_launch::Retirement::Pending { .. }
    ));
}

fn inventory(project: &Path, package: &Path) -> BTreeSet<String> {
    // Read the pinned runtime's literal extension map, without executing a
    // generated foreign-language inventory helper or inferring from counts.
    let grammar = fs::read_to_string(package.join("lib/dist/extraction/grammars.js")).unwrap();
    let map = grammar
        .split("exports.EXTENSION_MAP = {")
        .nth(1)
        .unwrap()
        .split("};")
        .next()
        .unwrap();
    let extensions: BTreeSet<_> = map
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix('\'')
                .and_then(|line| line.split_once("':"))
                .map(|(ext, _)| ext.to_owned())
        })
        .collect();
    assert!(extensions.contains(".rs") && extensions.contains(".xml"));
    let result = Command::new("git.exe")
        .args(["ls-files", "-c", "-o", "--exclude-standard", "-z"])
        .current_dir(project)
        .output()
        .unwrap();
    assert!(result.status.success());
    String::from_utf8(result.stdout)
        .unwrap()
        .split('\0')
        .filter(|file| {
            let path = project.join(file);
            path.is_file()
                && Path::new(file).extension().is_some_and(|ext| {
                    extensions.contains(&format!(".{}", ext.to_string_lossy().to_lowercase()))
                })
        })
        .map(str::to_owned)
        .collect()
}

fn recover(client: &mut Client, answer: &Value) -> (String, usize, usize) {
    let Some(id) = answer["structuredContent"]["detail_id"].as_str() else {
        return (answer.to_string(), 0, 0);
    };
    let mut offset = 0;
    let mut recovered = String::new();
    let mut calls = 0;
    let mut total = 0;
    for _ in 0..128 {
        let page = client.call("codegraph_detail", json!({"id":id,"offset":offset}));
        assert!(bytes(&page) <= 4096);
        assert_ne!(
            page["structuredContent"]["capture_truncated"], true,
            "capture limit reached"
        );
        calls += 1;
        total += bytes(&page);
        recovered.push_str(page["content"][0]["text"].as_str().unwrap());
        let next = page["structuredContent"]["next_offset"].as_u64().unwrap();
        let retained = page["structuredContent"]["retained_bytes"]
            .as_u64()
            .unwrap();
        assert!(next > offset || next == retained, "detail did not advance");
        offset = next;
        if next == retained {
            return (recovered, calls, total);
        }
    }
    panic!("detail exceeded bounded pages");
}

fn query_until(
    client: &mut Client,
    query: &str,
    present: Option<&str>,
    absent: Option<&str>,
) -> Value {
    let until = deadline(30);
    loop {
        let answer = client.call("codegraph_search", json!({"query":query}));
        assert_ne!(answer["isError"], true, "{answer}");
        let rendered = answer.to_string();
        let fresh = answer["structuredContent"]["freshness"] == "complete";
        if fresh
            && present.is_none_or(|s| rendered.contains(s))
            && absent.is_none_or(|s| !rendered.contains(s))
        {
            return answer;
        }
        assert!(
            !until.expired(),
            "refresh did not satisfy the source oracle: {answer}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
#[ignore = "requires explicit published package, private oracle manifest and private output; full indexes real roots, then keeps them active with an owned fixture; mutates only owned probes"]
fn published_real_roots_compare_raw_and_managed_and_refresh() {
    let package = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_PACKAGE").expect("explicit package"),
    );
    let manifest_path = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_ORACLES").expect("private source oracles"),
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("CODEGRAPH_ACCEPTANCE_OUTPUT").expect("private result path"),
    );
    let manifest: Value = serde_json::from_slice(&fs::read(manifest_path).unwrap()).unwrap();
    let mut report = json!({"schema":1,"model_calls":0,"tokenizer_measurement":null,"roots":[]});
    for root_spec in manifest["roots"].as_array().unwrap() {
        let project = Path::new(root_spec["root"].as_str().unwrap());
        let eligible = inventory(project, &package);
        let config = harness_core::codegraph_stdio::configuration(
            &package.join("node.exe"),
            &package.join("lib/dist/bin/codegraph.js"),
            project,
            ACTIVE_DIR_NAME.into(),
        )
        .unwrap();
        let prepared = BrokerRoot::prepare().unwrap();
        let broker = prepared.root();
        let started = Instant::now();
        let mut client = Client::start_command(
            "",
            Some(config.clone()),
            Some(broker.path()),
            Some(&package),
        );
        let startup_ms = started.elapsed().as_millis();
        let catalogue = client.request("tools/list", json!({}));
        let indexed_at = Instant::now();
        let indexed = client.call("codegraph_index", json!({}));
        let index_ms = indexed_at.elapsed().as_millis();
        let mut root_report = json!({"id":root_spec["id"],"root":project,"eligible":eligible,
            "managed_startup_ms":startup_ms,"managed_catalogue_bytes":bytes(&catalogue),
            "index":indexed,"index_ms":index_ms,"cases":[]});
        report["roots"]
            .as_array_mut()
            .unwrap()
            .push(root_report.clone());
        save(&output, &report);
        assert_ne!(indexed["isError"], true, "index failed; see private output");
        let indexed_files: BTreeSet<_> = harness_core::codegraph_store::indexed_files(
            &project.join(ACTIVE_DIR_NAME).join("codegraph.db"),
            deadline(30),
            &Cancellation::default(),
        )
        .unwrap()
        .into_iter()
        .map(|path| {
            let path = Path::new(&path);
            let relative = if path.is_absolute() {
                path.strip_prefix(&config.project).unwrap()
            } else {
                path
            };
            relative.to_string_lossy().replace('\\', "/")
        })
        .collect();
        root_report["indexed_files"] = json!(indexed_files);
        root_report["missing_eligible"] =
            json!(eligible.difference(&indexed_files).collect::<Vec<_>>());
        root_report["extra_indexed"] =
            json!(indexed_files.difference(&eligible).collect::<Vec<_>>());
        *report["roots"].as_array_mut().unwrap().last_mut().unwrap() = root_report.clone();
        save(&output, &report);
        assert!(
            eligible == indexed_files,
            "working source inventory differs from extraction; see private output"
        );
        // Same source-oracle arguments for each lane, including deliberately
        // ambiguous and mismatched names. Retain the full answers locally.
        let mut cases = Vec::new();
        for case in root_spec["cases"].as_array().unwrap() {
            cases.push(case.clone());
            if case["followup"].is_object() {
                let mut followup = case["followup"].clone();
                followup["name"] = json!(format!("{}-followup", case["name"].as_str().unwrap()));
                cases.push(followup);
            }
        }
        for case in &cases {
            let started = Instant::now();
            let answer = client.call(case["tool"].as_str().unwrap(), case["arguments"].clone());
            let managed_ms = started.elapsed().as_millis();
            assert!(bytes(&answer) <= 4096);
            let (recovered, detail_calls, detail_bytes) = recover(&mut client, &answer);
            root_report["cases"]
                .as_array_mut()
                .unwrap()
                .push(json!({"name":case["name"],
                "tool":case["tool"],"arguments":case["arguments"],"required":case["required"],
                "managed_bytes":bytes(&answer),"managed_ms":managed_ms,"managed":answer,
                "recovered":recovered,"detail_calls":detail_calls,"detail_bytes":detail_bytes}));
        }
        client.finish();
        retire(broker);
        let cancel = Cancellation::default();
        let started = Instant::now();
        let mut raw =
            Worker::start(config.command().unwrap(), Duration::from_secs(600), &cancel).unwrap();
        let initialized = raw.initialize(deadline(60), &cancel).unwrap();
        root_report["raw_startup_ms"] = json!(started.elapsed().as_millis());
        root_report["raw_initialize_bytes"] = json!(bytes(&initialized));
        let raw_catalogue = raw
            .request("tools/list", json!({}), deadline(30), &cancel)
            .unwrap();
        root_report["raw_catalogue_bytes"] = json!(bytes(&raw_catalogue));
        root_report["raw_catalogue"] = raw_catalogue;
        for (case, observed) in cases
            .iter()
            .zip(root_report["cases"].as_array_mut().unwrap())
        {
            let started = Instant::now();
            let raw_reply = raw
                .request(
                    "tools/call",
                    json!({"name":case["tool"],"arguments":case["arguments"]}),
                    deadline(60),
                    &cancel,
                )
                .unwrap();
            observed["raw_ms"] = json!(started.elapsed().as_millis());
            observed["raw_bytes"] = json!(bytes(&raw_reply["result"]));
            let raw_text = raw_reply.to_string();
            let managed_text = observed["recovered"].as_str().unwrap();
            let required = case["required"].as_array().cloned().unwrap_or_default();
            observed["lost_source_oracles"] = json!(
                required
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|s| raw_text.contains(s) && !managed_text.contains(s))
                    .collect::<Vec<_>>()
            );
            observed["upstream_missing_oracles"] = json!(
                required
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|s| !raw_text.contains(s))
                    .collect::<Vec<_>>()
            );
            observed["raw"] = raw_reply;
        }
        root_report["raw_cleanup"] = json!(format!("{:?}", raw.close().unwrap()));
        // Published watcher/manual sync/catch-up run against the full real
        // graph. Product source files are never modified.
        let probe = tempfile::Builder::new()
            .prefix("harness_codegraph_acceptance_")
            .tempdir_in(project)
            .unwrap();
        let relative = probe.path().file_name().unwrap().to_string_lossy();
        let mut client = Client::start_command(
            "",
            Some(config.clone()),
            Some(broker.path()),
            Some(&package),
        );
        client.call("codegraph_status", json!({}));
        fs::write(
            probe.path().join("added.rs"),
            "pub fn harness_owned_refresh_old() -> u32 { 1 }\n",
        )
        .unwrap();
        let added = query_until(
            &mut client,
            "harness_owned_refresh_old",
            Some(&format!("{relative}/added.rs")),
            None,
        );
        for n in 2..5 {
            fs::write(
                probe.path().join("added.rs"),
                format!("pub fn harness_owned_refresh_changed() -> u32 {{ {n} }}\n"),
            )
            .unwrap();
        }
        let changed = query_until(
            &mut client,
            "harness_owned_refresh_changed",
            Some(&format!("{relative}/added.rs")),
            None,
        );
        let old = query_until(
            &mut client,
            "harness_owned_refresh_old",
            None,
            Some("added.rs"),
        );
        fs::rename(
            probe.path().join("added.rs"),
            probe.path().join("renamed.rs"),
        )
        .unwrap();
        let renamed = query_until(
            &mut client,
            "harness_owned_refresh_changed",
            Some(&format!("{relative}/renamed.rs")),
            Some("added.rs"),
        );
        let synced = client.call("codegraph_sync", json!({}));
        assert_ne!(synced["isError"], true, "{synced}");
        client.finish();
        retire(broker);
        fs::remove_file(probe.path().join("renamed.rs")).unwrap();
        let mut client =
            Client::start_command("", Some(config), Some(broker.path()), Some(&package));
        let deleted = query_until(
            &mut client,
            "harness_owned_refresh_changed",
            None,
            Some("renamed.rs"),
        );
        let final_sync = client.call("codegraph_sync", json!({}));
        assert_ne!(final_sync["isError"], true, "{final_sync}");
        client.finish();
        retire(broker);
        root_report["refresh"] = json!({"added":added,"changed":changed,"old_removed":old,"renamed":renamed,
            "manual":synced,"deleted_during_disconnect":deleted,"final_sync":final_sync});
        *report["roots"].as_array_mut().unwrap().last_mut().unwrap() = root_report;
        save(&output, &report);
    }
    let lost: usize = report["roots"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| r["cases"].as_array().unwrap())
        .map(|c| c["lost_source_oracles"].as_array().unwrap().len())
        .sum();
    assert_eq!(lost, 0, "source-oracle loss recorded in private output");
    concurrent_real_roots_with_owned_fixture(&package, &manifest, &output, &mut report);
}

fn committed_generation(project: &Path) -> u64 {
    GenerationStore::open(project, StorageLimits::default())
        .unwrap()
        .committed_handle()
        .unwrap()
        .expect("committed generation")
        .generation
        .unwrap()
}

fn symbol_rows(project: &Path, symbol: &str) -> Vec<String> {
    let handle = GenerationStore::open(project, StorageLimits::default())
        .unwrap()
        .committed_handle()
        .unwrap()
        .expect("committed generation");
    codegraph_store::symbol_files(
        &handle.database,
        symbol,
        deadline(5),
        &Cancellation::default(),
    )
    .unwrap()
}

fn assert_symbol(project: &Path, symbol: &str, file: Option<&str>, exists: bool) {
    let rows = symbol_rows(project, symbol);
    let found = rows.iter().any(|row| {
        let path = row.replace('\\', "/");
        file.is_none_or(|needle| {
            let needle = needle.replace('\\', "/");
            path == needle || path.ends_with(&needle) || path.ends_with(&format!("/{needle}"))
        })
    });
    assert_eq!(
        found, exists,
        "committed SQLite symbol {symbol} exists={found} expected={exists} file={file:?} rows={rows:?}"
    );
}

fn wait_symbol_generation(project: &Path, after: u64, symbol: &str, file: &str) -> u64 {
    let store = GenerationStore::open(project, StorageLimits::default()).unwrap();
    let until = deadline(90);
    loop {
        if let Some(handle) = store.committed_handle().unwrap()
            && handle.generation.unwrap() > after
        {
            let rows = codegraph_store::symbol_files(
                &handle.database,
                symbol,
                deadline(5),
                &Cancellation::default(),
            )
            .unwrap();
            if rows
                .iter()
                .any(|row| row.replace('\\', "/").ends_with(file))
            {
                return handle.generation.unwrap();
            }
        }
        assert!(
            !until.expired(),
            "automatic committed refresh did not contain {symbol} at {file}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn search_root(
    client: &mut Client,
    project: &Path,
    query: &str,
    present: Option<&str>,
    absent: Option<&str>,
) -> Value {
    let answer = client.call("codegraph_search", json!({"query": query}));
    assert_ne!(answer["isError"], true, "{answer}");
    assert_eq!(
        answer["structuredContent"]["root"],
        json!(project),
        "{answer}"
    );
    let rendered = answer.to_string();
    if let Some(text) = present {
        assert!(rendered.contains(text), "missing {text} in {answer}");
    }
    if let Some(text) = absent {
        assert!(!rendered.contains(text), "unexpected {text} in {answer}");
    }
    answer
}

fn lookup_query(root_spec: &Value) -> String {
    root_spec["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["tool"] == "codegraph_search")
        .and_then(|case| case["arguments"]["query"].as_str())
        .expect("real root needs a search oracle")
        .to_owned()
}

fn rust_fn(name: &str, n: u32) -> String {
    format!("pub fn {name}() -> u32 {{ {n} }}\n")
}

fn probe_name(id: &str, kind: &str) -> String {
    format!("harness_owned_concurrent_{}_{kind}", id.replace('-', "_"))
}

struct LiveRoot {
    id: String,
    config: harness_core::codegraph_stdio::Configuration,
    client: Client,
    generation: u64,
    probe: Option<tempfile::TempDir>,
}

fn probe_dir(live: &LiveRoot) -> PathBuf {
    match &live.probe {
        Some(dir) => dir.path().to_path_buf(),
        None => live.config.project.join("src"),
    }
}

fn probe_file(live: &LiveRoot, name: &str) -> String {
    match &live.probe {
        Some(dir) => format!(
            "{}/{}",
            dir.path().file_name().unwrap().to_string_lossy(),
            name
        ),
        None => format!("src/{name}"),
    }
}

fn connect_live(id: &str, project: &Path, package: &Path, broker: &Path, index: bool) -> LiveRoot {
    let config = harness_core::codegraph_stdio::configuration(
        &package.join("node.exe"),
        &package.join("lib/dist/bin/codegraph.js"),
        project,
        ACTIVE_DIR_NAME.into(),
    )
    .unwrap();
    if index {
        let mut client =
            Client::start_command("", Some(config.clone()), Some(broker), Some(package));
        let indexed = client.call("codegraph_index", json!({}));
        assert_ne!(indexed["isError"], true, "{indexed}");
        let generation = super::wait_checkpoint(&config.project, 0, Some("src/lib.rs"), None);
        LiveRoot {
            id: id.to_owned(),
            config,
            client,
            generation,
            probe: None,
        }
    } else {
        let previous = committed_generation(&config.project);
        let client = Client::start_command("", Some(config.clone()), Some(broker), Some(package));
        let generation = super::wait_checkpoint(&config.project, previous, None, None);
        LiveRoot {
            id: id.to_owned(),
            config,
            client,
            generation,
            probe: None,
        }
    }
}

fn concurrent_real_roots_with_owned_fixture(
    package: &Path,
    manifest: &Value,
    output: &Path,
    report: &mut Value,
) {
    let owned = tempfile::tempdir().unwrap();
    let fixture = owned.path().join("owned-fixture");
    fs::create_dir_all(fixture.join("src")).unwrap();
    assert!(
        Command::new("git.exe")
            .args(["init", "--quiet"])
            .arg(&fixture)
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        fixture.join("src/lib.rs"),
        "pub fn harness_owned_fixture_root() -> u32 { 1 }\n",
    )
    .unwrap();
    let prepared = BrokerRoot::prepare().unwrap();
    let broker = prepared.root();
    let fixture_live = connect_live("owned-fixture", &fixture, package, broker.path(), true);
    let mut lives = Vec::new();
    for root_spec in manifest["roots"].as_array().unwrap() {
        lives.push(connect_live(
            root_spec["id"].as_str().unwrap(),
            Path::new(root_spec["root"].as_str().unwrap()),
            package,
            broker.path(),
            false,
        ));
    }
    lives.push(fixture_live);
    assert_eq!(lives.len(), 3, "pack, large root and owned fixture");
    report["concurrent"] =
        json!({"projects": [], "same_root_extra_client": Value::Null, "refresh": {}});
    for live in &lives {
        report["concurrent"]["projects"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": live.id,
                "root": live.config.project,
                "committed_generation": live.generation
            }));
    }
    save(output, report);

    let first_query = lookup_query(&manifest["roots"][0]);
    let first_root = lives[0].config.project.clone();
    let first = search_root(&mut lives[0].client, &first_root, &first_query, None, None);
    let mut extra = Client::start_command(
        "",
        Some(lives[0].config.clone()),
        Some(broker.path()),
        Some(package),
    );
    let shared = extra.call("codegraph_search", json!({"query": first_query}));
    assert_ne!(shared["isError"], true, "{shared}");
    assert_eq!(
        first["structuredContent"]["worker"], shared["structuredContent"]["worker"],
        "{shared}"
    );
    assert_eq!(
        shared["structuredContent"]["root"],
        json!(lives[0].config.project),
        "{shared}"
    );
    report["concurrent"]["same_root_extra_client"] = json!({
        "root": lives[0].config.project,
        "shared_worker": true
    });
    save(output, report);

    let mut observed = BTreeSet::new();
    for (index, live) in lives.iter_mut().enumerate() {
        let query = if live.id == "owned-fixture" {
            "harness_owned_fixture_root".to_owned()
        } else {
            lookup_query(&manifest["roots"][index])
        };
        let present = (live.id == "owned-fixture").then_some("src/lib.rs");
        let answer = search_root(
            &mut live.client,
            &live.config.project,
            &query,
            present,
            None,
        );
        observed.insert(
            answer["structuredContent"]["root"]
                .as_str()
                .unwrap()
                .to_owned(),
        );
    }
    assert_eq!(
        observed.len(),
        lives.len(),
        "each active project answers with its own root"
    );

    for live in &mut lives {
        if live.id != "owned-fixture" {
            live.probe = Some(
                tempfile::Builder::new()
                    .prefix("harness_codegraph_concurrent_")
                    .tempdir_in(&live.config.project)
                    .unwrap(),
            );
        }
    }

    let mut refresh = json!({});
    for live in &lives {
        fs::write(
            probe_dir(live).join("added.rs"),
            rust_fn(&probe_name(&live.id, "old"), 1),
        )
        .unwrap();
    }
    for live in &mut lives {
        let relative = probe_file(live, "added.rs");
        live.generation =
            super::wait_checkpoint(&live.config.project, live.generation, Some(&relative), None);
        assert_symbol(
            &live.config.project,
            &probe_name(&live.id, "old"),
            Some(&relative),
            true,
        );
    }
    for live in &mut lives {
        let relative = probe_file(live, "added.rs");
        let added = search_root(
            &mut live.client,
            &live.config.project,
            &probe_name(&live.id, "old"),
            Some(&relative),
            None,
        );
        refresh[&live.id] = json!({"added": added, "committed_generation": live.generation});
    }
    let extra_added = extra.call(
        "codegraph_search",
        json!({"query": probe_name(&lives[0].id, "old")}),
    );
    assert_ne!(extra_added["isError"], true, "{extra_added}");
    assert_eq!(
        extra_added["structuredContent"]["root"],
        json!(lives[0].config.project),
        "{extra_added}"
    );
    assert!(
        extra_added
            .to_string()
            .contains(&probe_file(&lives[0], "added.rs")),
        "{extra_added}"
    );
    refresh["same_root_extra_client_added"] = extra_added;

    for live in &lives {
        for n in 2..5 {
            fs::write(
                probe_dir(live).join("added.rs"),
                rust_fn(&probe_name(&live.id, "changed"), n),
            )
            .unwrap();
        }
    }
    for live in &mut lives {
        let relative = probe_file(live, "added.rs");
        // A preceding no-change episode can commit after the add assertion.
        // Wait for the edited symbol itself, not merely a newer generation
        // which still contains the unchanged file path.
        live.generation = wait_symbol_generation(
            &live.config.project,
            live.generation,
            &probe_name(&live.id, "changed"),
            &relative,
        );
        assert_symbol(
            &live.config.project,
            &probe_name(&live.id, "changed"),
            Some(&relative),
            true,
        );
        assert_symbol(
            &live.config.project,
            &probe_name(&live.id, "old"),
            None,
            false,
        );
    }
    for live in &mut lives {
        let relative = probe_file(live, "added.rs");
        let changed = search_root(
            &mut live.client,
            &live.config.project,
            &probe_name(&live.id, "changed"),
            Some(&relative),
            None,
        );
        let old = search_root(
            &mut live.client,
            &live.config.project,
            &probe_name(&live.id, "old"),
            None,
            Some(&relative),
        );
        refresh[&live.id]["changed"] = changed;
        refresh[&live.id]["old_removed"] = old;
        refresh[&live.id]["committed_generation"] = json!(live.generation);
    }

    for live in &lives {
        fs::rename(
            probe_dir(live).join("added.rs"),
            probe_dir(live).join("renamed.rs"),
        )
        .unwrap();
    }
    for live in &mut lives {
        let renamed = probe_file(live, "renamed.rs");
        let added = probe_file(live, "added.rs");
        live.generation = super::wait_checkpoint(
            &live.config.project,
            live.generation,
            Some(&renamed),
            Some(&added),
        );
        assert_symbol(
            &live.config.project,
            &probe_name(&live.id, "changed"),
            Some(&renamed),
            true,
        );
    }
    for live in &mut lives {
        let renamed = probe_file(live, "renamed.rs");
        let added = probe_file(live, "added.rs");
        let answer = search_root(
            &mut live.client,
            &live.config.project,
            &probe_name(&live.id, "changed"),
            Some(&renamed),
            Some(&added),
        );
        refresh[&live.id]["renamed"] = answer;
        refresh[&live.id]["committed_generation"] = json!(live.generation);
    }

    extra.finish();
    for live in &lives {
        fs::remove_file(probe_dir(live).join("renamed.rs")).unwrap();
    }
    for live in &mut lives {
        let renamed = probe_file(live, "renamed.rs");
        live.generation =
            super::wait_checkpoint(&live.config.project, live.generation, None, Some(&renamed));
        assert_symbol(
            &live.config.project,
            &probe_name(&live.id, "changed"),
            None,
            false,
        );
    }
    for live in &mut lives {
        let renamed = probe_file(live, "renamed.rs");
        let deleted = search_root(
            &mut live.client,
            &live.config.project,
            &probe_name(&live.id, "changed"),
            None,
            Some(&renamed),
        );
        let synced = live.client.call("codegraph_sync", json!({}));
        assert_ne!(synced["isError"], true, "{synced}");
        refresh[&live.id]["deleted"] = deleted;
        refresh[&live.id]["manual"] = synced;
        refresh[&live.id]["committed_generation"] = json!(live.generation);
    }
    report["concurrent"]["refresh"] = refresh;
    save(output, report);
    for live in lives {
        live.client.finish();
    }
    retire(broker);
}
