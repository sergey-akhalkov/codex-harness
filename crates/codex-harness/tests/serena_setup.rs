//! Explicit clean-host preparation through the committed serena_setup boundary.
#![cfg(windows)]
use harness_core::{
    dependency_discovery::{self, Request as DiscoveryRequest},
    serena_setup::{self, Request},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static ENV: Mutex<()> = Mutex::new(());

fn catalogue() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global/code-tools.json")
}

fn set_env(path: &Path, assets: Option<&Path>) {
    // Integration tests are single-threaded by this local lock; Rust 2024
    // requires an unsafe acknowledgement for process-wide environment writes.
    unsafe {
        std::env::set_var("HARNESS_SERENA_SETUP_PATH", path);
        match assets {
            Some(assets) => std::env::set_var("HARNESS_SERENA_SETUP_ASSETS", assets),
            None => std::env::remove_var("HARNESS_SERENA_SETUP_ASSETS"),
        }
    }
}

fn clear_env() {
    unsafe {
        std::env::remove_var("HARNESS_SERENA_SETUP_PATH");
        std::env::remove_var("HARNESS_SERENA_SETUP_ASSETS");
    }
}

fn node(bin: &Path) {
    fs::write(bin.join("node.exe"), b"inert test runtime, not executable").unwrap();
}

fn serena(home: &Path) {
    let root = home.join("AppData/Roaming/uv/tools/serena-agent");
    let site = root.join("Lib/site-packages");
    let dist = site.join("serena_agent-1.7.0.dist-info");
    fs::create_dir_all(&dist).unwrap();
    fs::create_dir_all(site.join("serena")).unwrap();
    fs::create_dir_all(root.join("Scripts")).unwrap();
    fs::write(
        dist.join("METADATA"),
        "Metadata-Version: 2.4\nName: Serena_Agent\nVersion: 1.7.0\n\nsynthetic",
    )
    .unwrap();
    fs::write(
        dist.join("entry_points.txt"),
        "[console_scripts]\nserena = serena.cli:top_level\n",
    )
    .unwrap();
    fs::write(site.join("serena/cli.py"), b"abc").unwrap();
    fs::write(root.join("Scripts/python.exe"), b"inert").unwrap();
    fs::write(root.join("Scripts/serena.exe"), b"inert").unwrap();
    fs::write(
        root.join("uv-receipt.toml"),
        "[tool]\nrequirements = [{name = 'serena-agent'}]\n",
    )
    .unwrap();
    fs::write(
        dist.join("RECORD"),
        "serena/cli.py,sha256=ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0,3\n",
    )
    .unwrap();
}

fn basedpyright(home: &Path) {
    let root = home.join("AppData/Roaming/npm/node_modules/basedpyright");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("package.json"),
        serde_json::to_vec(&json!({
            "name": "basedpyright",
            "version": "1.40.2",
            "bin": {"basedpyright-langserver": "langserver.index.js"}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(root.join("langserver.index.js"), b"inert entrypoint").unwrap();
}

fn request(root: &Path) -> (Request, PathBuf, PathBuf) {
    let home = root.join("home");
    let state = root.join("state");
    (
        Request {
            catalogue: catalogue(),
            user_home: home.clone(),
            state: state.clone(),
        },
        home,
        state,
    )
}

fn record<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["mcp"]
        .as_array()
        .unwrap()
        .iter()
        .chain(report["languages"].as_array().unwrap())
        .find(|row| row["id"] == id)
        .unwrap()
}

fn assert_discovered(request: &Request, bin: &Path) {
    let inventory = dependency_discovery::discover(&DiscoveryRequest {
        catalogue: request.catalogue.clone(),
        user_home: request.user_home.clone(),
        npm_prefixes: vec![request.user_home.join("AppData/Roaming/npm")],
        uv_tools_dir: Some(request.user_home.join("AppData/Roaming/uv/tools")),
        path: Some(bin.as_os_str().to_owned()),
        full_records: true,
        ..DiscoveryRequest::default()
    })
    .unwrap();
    assert_eq!(record(&inventory, "serena")["status"], "adopted");
    assert_eq!(record(&inventory, "serena")["version"], "1.7.0");
    assert_eq!(record(&inventory, "python")["status"], "adopted");
    assert_eq!(record(&inventory, "python")["version"], "1.40.2");
}

#[test]
fn inspect_on_empty_state_reports_preparation_required_without_creating_it() {
    let _guard = ENV.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (request, _home, state) = request(root.path());
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    set_env(&bin, None);
    let report = serena_setup::inspect(&request).unwrap();
    clear_env();
    assert_eq!(report["status"], "preparation-required");
    assert_eq!(report["ready"], false);
    assert_eq!(report["mutated"], false);
    assert_eq!(report["model_calls"], 0);
    assert_eq!(report["missing"], json!(["serena", "basedpyright"]));
    assert!(!state.exists());
}

#[test]
fn compatible_shared_installations_are_adopted_without_acquisition_or_repetition() {
    let _guard = ENV.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (request, home, state) = request(root.path());
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    serena(&home);
    basedpyright(&home);
    node(&bin);
    set_env(&bin, None);
    let report = serena_setup::prepare(&request).unwrap();
    assert_eq!(report["status"], "ready");
    assert_eq!(report["ready"], true);
    assert_eq!(report["packages_acquired"], false);
    assert_eq!(report["mutated"], true);
    assert_eq!(report["identities"]["serena"], "1.7.0");
    assert_eq!(report["identities"]["python"], "1.40.2");
    assert!(!state.join("runtime").exists());
    assert!(!state.join("downloads").exists());
    assert_discovered(&request, &bin);
    let first = fs::read(serena_setup::registry_path(&state)).unwrap();
    let repeated = serena_setup::prepare(&request).unwrap();
    clear_env();
    assert_eq!(repeated["status"], "ready");
    assert_eq!(repeated["packages_acquired"], false);
    assert_eq!(repeated["mutated"], false);
    assert_eq!(
        fs::read(serena_setup::registry_path(&state)).unwrap(),
        first
    );
    let inspected = serena_setup::inspect(&request).unwrap();
    assert_eq!(inspected["status"], "ready");
    assert_eq!(inspected["mutated"], false);
}

#[test]
fn failed_acquisition_preserves_the_previous_registry_and_reports_the_cause() {
    let _guard = ENV.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (request, home, state) = request(root.path());
    let bin = root.path().join("bin");
    let assets = root.path().join("assets");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&assets).unwrap();
    serena(&home);
    basedpyright(&home);
    node(&bin);
    set_env(&bin, None);
    serena_setup::prepare(&request).unwrap();
    let before = fs::read(serena_setup::registry_path(&state)).unwrap();
    fs::remove_dir_all(home.join("AppData/Roaming/npm/node_modules/basedpyright")).unwrap();
    let interrupted = state.join("runtime/staging-interrupted");
    fs::create_dir_all(&interrupted).unwrap();
    fs::write(interrupted.join("partial"), b"incomplete").unwrap();
    fs::write(
        assets.join("basedpyright-1.40.2.tgz"),
        b"not the pinned official artifact",
    )
    .unwrap();
    set_env(&bin, Some(&assets));
    let error = serena_setup::prepare(&request).unwrap_err();
    clear_env();
    assert!(
        error.to_string().contains("artifact-integrity-mismatch"),
        "{error}"
    );
    assert_eq!(
        fs::read(serena_setup::registry_path(&state)).unwrap(),
        before
    );
    assert!(
        home.join("AppData/Roaming/uv/tools/serena-agent/Scripts/serena.exe")
            .is_file()
    );
    assert!(!state.join("downloads/basedpyright-1.40.2.tgz").exists());
    assert!(!interrupted.exists());
}

#[test]
#[ignore = "downloads the pinned official uv, Python, Node, Serena and BasedPyright artifacts"]
fn clean_prepare_uses_pinned_artifacts_and_repeats_without_downloads() {
    let _guard = ENV.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (request, _home, state) = request(root.path());
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    set_env(&bin, None);
    let first = serena_setup::prepare(&request).unwrap();
    assert_eq!(first["status"], "ready");
    assert_eq!(first["packages_acquired"], true);
    assert_eq!(first["identities"]["serena"], "1.7.0");
    assert_eq!(first["identities"]["python"], "1.40.2");
    let acquired = first["acquired"].as_array().unwrap();
    for id in ["uv", "python", "node", "serena-agent", "basedpyright"] {
        assert!(acquired.iter().any(|value| value == id), "{id}: {first}");
    }
    assert!(state.join("runtime").is_dir());
    let prepared = &first["prepared"];
    let serena_tools = PathBuf::from(prepared["serena"].as_str().unwrap());
    let basedpyright = PathBuf::from(prepared["basedpyright"].as_str().unwrap());
    let npm_prefix = basedpyright.parent().unwrap().parent().unwrap();
    let node = PathBuf::from(prepared["node"].as_str().unwrap());
    let inventory = dependency_discovery::discover(&DiscoveryRequest {
        catalogue: request.catalogue.clone(),
        user_home: request.user_home.clone(),
        npm_prefixes: vec![npm_prefix.to_owned()],
        uv_tools_dir: Some(serena_tools),
        path: Some(node.parent().unwrap().as_os_str().to_owned()),
        full_records: true,
        ..DiscoveryRequest::default()
    })
    .unwrap();
    assert_eq!(record(&inventory, "serena")["status"], "adopted");
    assert_eq!(record(&inventory, "serena")["version"], "1.7.0");
    assert_eq!(record(&inventory, "python")["status"], "adopted");
    assert_eq!(record(&inventory, "python")["version"], "1.40.2");
    let downloads = |state: &Path| {
        let mut files = fs::read_dir(state.join("downloads"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), entry.metadata().unwrap().len())
            })
            .collect::<Vec<_>>();
        files.sort();
        files
    };
    let before_downloads = downloads(&state);
    let repeated = serena_setup::prepare(&request).unwrap();
    assert_eq!(repeated["packages_acquired"], false);
    assert_eq!(repeated["mutated"], false);
    assert_eq!(downloads(&state), before_downloads);
    let inspected = serena_setup::inspect(&request).unwrap();
    clear_env();
    assert_eq!(inspected["status"], "ready");
    assert_eq!(inspected["identities"]["serena"], "1.7.0");
    assert_eq!(inspected["identities"]["python"], "1.40.2");
}
