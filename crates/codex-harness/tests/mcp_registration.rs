//! Isolated MCP registration lifecycle in an owned Codex home after the
//! managed CodeGraph retirement. Does not download packages, probe the
//! published runtime, or write live global MCP.
#![cfg(windows)]
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const FOREIGN: &[u8] =
    b"# retain my comment\nmodel = \"gpt-6-astra\"\n[mcp_servers.foreign]\ncommand = \"untouched.exe\"\nargs = []\n";

const CBM_BLOCK: &str = "\n# BEGIN codex-harness MCP registrations\n[mcp_servers.codebase-memory]\ncommand = \"cbm.exe\"\nargs = [\"mcp\"]\ntool_timeout_sec = 660\n# END codex-harness MCP registrations\n";

const RETIRED_GRAPH_BLOCK: &str = "\n# BEGIN codex-harness MCP registrations\n[mcp_servers.codegraph]\ncommand = \"old-manager.exe\"\nargs = [\"mcp\", \"codegraph\", \"--package-root\", \"C:/old-package\"]\nstartup_timeout_sec = 30\ntool_timeout_sec = 660\n# END codex-harness MCP registrations\n";

const CBM_POLICY: &[u8] = br#"{"auto_index":false,"auto_watch":true,"ui_enabled":false}"#;

const READINESS: &str = "mcp_optional_startup_grace_ms";

fn fail(output: &Output) -> String {
    format!(
        "status={:?} stdout={} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn native_prepare_check_missing(home: &Path, _state: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args([
            "mcp",
            "prepare-mcp",
            "--mode",
            "Check",
            "--codex-home",
            home.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", fail(&output));
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{}", fail(&output)))
}

fn apply_command(home: &Path, mode: &str, extra: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    command.args([
        "mcp",
        "apply-registration",
        "--mode",
        mode,
        "--codex-home",
        home.to_str().unwrap(),
    ]);
    command.args(extra);
    command
}

fn retained_plan(python: &str, powershell: &str, home: &Path) -> String {
    json!({
        "serena": {
            "command": python,
            "args": ["-B", "-u", "launch.py", "serena"],
            "env": {"CODEX_HOME": home}
        },
        "nuphus": {
            "command": powershell,
            "args": ["-NoLogo", "-NoProfile", "-File", "mcp.ps1", "-Server", "nuphus"],
            "env": {"CODEX_HOME": home}
        }
    })
    .to_string()
}

fn retained_names(servers: &Value) -> Vec<String> {
    ["serena", "nuphus"]
        .into_iter()
        .filter(|name| servers.get(*name).is_some())
        .map(str::to_owned)
        .collect()
}

fn run_apply(home: &Path, mode: &str, extra: &[&str]) -> Value {
    let output = apply_command(home, mode, extra).output().unwrap();
    assert!(output.status.success(), "{}", fail(&output));
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{}", fail(&output)))
}

fn expect_error(home: &Path, mode: &str, extra: &[&str]) -> String {
    let output = apply_command(home, mode, extra).output().unwrap();
    assert!(!output.status.success(), "{}", fail(&output));
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn servers(home: &Path) -> Value {
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    toml::from_str::<Value>(&text).unwrap()["mcp_servers"].clone()
}

fn write_home(root: &Path) -> PathBuf {
    let home = root.join("owned-codex-home");
    fs::create_dir_all(home.join("harness")).unwrap();
    fs::write(home.join("config.toml"), FOREIGN).unwrap();
    home
}

fn shared_package(root: &Path, name: &str) -> PathBuf {
    let package = root.join(name);
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("keep.txt"), b"shared-bytes").unwrap();
    package
}

fn write_cbm_home(root: &Path) -> (PathBuf, PathBuf) {
    let home = write_home(root);
    let mut config = FOREIGN.to_vec();
    config.extend_from_slice(CBM_BLOCK.as_bytes());
    fs::write(home.join("config.toml"), &config).unwrap();
    let state = json!({
        "schema_version": 1,
        "registrations": {
            "codebase-memory": {
                "command": "cbm.exe",
                "args": ["mcp"],
                "tool_timeout_sec": 660
            }
        },
        "block": CBM_BLOCK,
        "connection_policy": null
    });
    fs::write(
        home.join("harness/code-tools-registration.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
    let cache = root.join("cbm-cache");
    fs::create_dir_all(cache.join("indexes")).unwrap();
    fs::write(cache.join("indexes/project.db"), b"owned-cbm-index").unwrap();
    fs::write(cache.join("policy.json"), CBM_POLICY).unwrap();
    (home, cache)
}

/// A home recorded by a pre-retirement version with an owned CodeGraph
/// registration; Update must remove exactly that owned entry.
fn write_pre_retirement_graph_home(root: &Path) -> PathBuf {
    let home = write_home(root);
    let mut config = FOREIGN.to_vec();
    config.extend_from_slice(RETIRED_GRAPH_BLOCK.as_bytes());
    fs::write(home.join("config.toml"), &config).unwrap();
    let state = json!({
        "schema_version": 1,
        "registrations": {
            "codegraph": {
                "command": "old-manager.exe",
                "args": ["mcp", "codegraph", "--package-root", "C:/old-package"],
                "startup_timeout_sec": 30,
                "tool_timeout_sec": 660
            }
        },
        "block": RETIRED_GRAPH_BLOCK,
        "connection_policy": null
    });
    fs::write(
        home.join("harness/code-tools-registration.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
    home
}

fn parsed_config(home: &Path) -> Value {
    toml::from_str(&fs::read_to_string(home.join("config.toml")).unwrap()).unwrap()
}

fn receipt(home: &Path) -> Value {
    serde_json::from_slice(&fs::read(home.join("harness/code-tools-registration.json")).unwrap())
        .unwrap()
}

fn restamp_owned_block_as_toml(home: &Path) {
    let config = home.join("config.toml");
    let text = fs::read_to_string(&config).unwrap();
    let block = receipt(home)["block"].as_str().unwrap().to_string();
    let matches = text.matches(&block).count();
    assert_eq!(matches, 1, "owned block must appear once before restamping");
    let parsed: toml::Table = toml::from_str(&block).unwrap();
    let restamped = toml::to_string(&parsed).unwrap();
    assert_ne!(&restamped, &block);
    fs::write(&config, text.replacen(&block, &restamped, 1)).unwrap();
}

fn seed_normalized_cbm_home(root: &Path) -> (PathBuf, PathBuf) {
    let (home, cache) = write_cbm_home(root);
    let old_block = receipt(&home)["block"].as_str().unwrap().to_string();
    let normalized = concat!(
        "model = \"gpt-6-astra\"\n",
        "[mcp_servers.foreign]\n",
        "command = \"untouched.exe\"\n",
        "args = []\n",
        "# retain my comment\n",
        "notes = \"\"\"Literal header, not a table:\n",
        "[mcp_servers.codebase-memory]\n",
        "# BEGIN codex-harness MCP registrations\n",
        "command = \"foreign prose\"\n",
        "\"\"\"\n",
        "[mcp_servers.codebase-memory]\n",
        "args = [\"mcp\"]\n",
        "command = \"cbm.exe\"\n",
        "tool_timeout_sec = 660\n",
        "# native TUI project trust\n",
        "[projects.\"D:/foreign project\"]\n",
        "trust_level = \"trusted\"\n",
        "[[features]]\n",
        "name = \"keep-array\"\n",
    );
    assert!(
        !normalized.contains(&old_block),
        "normalized config must not contain the exact receipt block"
    );
    fs::write(home.join("config.toml"), normalized).unwrap();
    (home, cache)
}

#[test]
fn native_prepare_check_missing_is_empty_and_does_not_write() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let state = root.path().join("missing-state");
    let before = fs::read(home.join("config.toml")).unwrap();
    let prepared = native_prepare_check_missing(&home, &state);
    assert_eq!(prepared["status"], "missing");
    assert_eq!(prepared["mcp"], json!([]));
    assert_eq!(prepared["registrations"], json!({}));
    assert_eq!(
        prepared["retired"],
        json!(["codebase-memory", "graphify", "codegraph"])
    );
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), before);
    assert!(!state.exists());
    assert!(!home.join("harness/code-tools-registration.json").exists());
}

#[test]
fn install_replaces_owned_cbm_and_preserves_indexes_packages_and_unrelated_settings() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = write_cbm_home(root.path());
    let package = shared_package(root.path(), "shared-package");
    let before_index = fs::read(cache.join("indexes/project.db")).unwrap();
    let before_policy = fs::read(cache.join("policy.json")).unwrap();
    let before_shared = fs::read(package.join("keep.txt")).unwrap();
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let connected = run_apply(&home, "Install", &["--retained-registrations-json", &plan]);
    assert_eq!(connected["status"], "connected");
    let after = String::from_utf8(fs::read(home.join("config.toml")).unwrap()).unwrap();
    assert!(after.contains("untouched.exe"));
    assert!(after.contains("# retain my comment"));
    let servers = servers(&home);
    assert!(servers.get("codebase-memory").is_none(), "{servers}");
    assert!(servers.get("codegraph").is_none(), "{servers}");
    assert_eq!(
        retained_names(&servers),
        vec!["serena".to_string(), "nuphus".to_string()]
    );
    assert_eq!(
        fs::read(cache.join("indexes/project.db")).unwrap(),
        before_index
    );
    assert_eq!(fs::read(cache.join("policy.json")).unwrap(), before_policy);
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), before_shared);
}

#[test]
fn install_removes_pre_retirement_owned_codegraph_registration() {
    let root = tempfile::tempdir().unwrap();
    let home = write_pre_retirement_graph_home(root.path());
    let package = shared_package(root.path(), "shared-package");
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let connected = run_apply(&home, "Install", &["--retained-registrations-json", &plan]);
    assert_eq!(connected["status"], "connected");
    assert_eq!(
        connected["retired"],
        json!(["codebase-memory", "graphify", "codegraph"])
    );
    let servers = servers(&home);
    assert!(servers.get("codegraph").is_none(), "{servers}");
    assert!(servers.get("codebase-memory").is_none(), "{servers}");
    assert_eq!(retained_names(&servers).len(), 2);
    assert_eq!(servers["foreign"]["command"], "untouched.exe");
    assert!(receipt(&home)["registrations"].get("codegraph").is_none());
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), b"shared-bytes");
}

#[test]
fn update_removes_pre_retirement_owned_codegraph_registration() {
    let root = tempfile::tempdir().unwrap();
    let home = write_pre_retirement_graph_home(root.path());
    let package = shared_package(root.path(), "old-package-copy");
    let before = fs::read(home.join("config.toml")).unwrap();
    let updated = run_apply(&home, "Update", &[]);
    assert_eq!(updated["status"], "connected");
    let servers = servers(&home);
    assert!(servers.get("codegraph").is_none(), "{servers}");
    assert_eq!(servers["foreign"]["command"], "untouched.exe");
    let after = fs::read(home.join("config.toml")).unwrap();
    assert_ne!(after, before);
    assert!(String::from_utf8_lossy(&after).contains("# retain my comment"));
    assert!(receipt(&home)["registrations"].get("codegraph").is_none());
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), b"shared-bytes");
}

#[test]
fn check_reports_retirement_without_mutating() {
    let root = tempfile::tempdir().unwrap();
    let home = write_pre_retirement_graph_home(root.path());
    let before = fs::read(home.join("config.toml")).unwrap();
    let before_state = fs::read(home.join("harness/code-tools-registration.json")).unwrap();
    let checked = run_apply(&home, "Check", &[]);
    assert_eq!(checked["status"], "retired");
    assert_eq!(checked["serving_allowed"], false);
    assert_eq!(checked["callable"], false);
    assert!(checked["note"].as_str().unwrap().contains("retired"));
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), before);
    assert_eq!(
        fs::read(home.join("harness/code-tools-registration.json")).unwrap(),
        before_state
    );
}

#[test]
fn deferred_install_recover_restores_cbm_manual_policy_and_index_bytes() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = write_cbm_home(root.path());
    let package = shared_package(root.path(), "shared-package");
    let previous = fs::read(home.join("config.toml")).unwrap();
    let previous_state = fs::read(home.join("harness/code-tools-registration.json")).unwrap();
    let previous_index = fs::read(cache.join("indexes/project.db")).unwrap();
    let previous_policy = fs::read(cache.join("policy.json")).unwrap();
    assert!(servers(&home).get("codebase-memory").is_some());
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let interrupted = run_apply(
        &home,
        "Install",
        &["--retained-registrations-json", &plan, "--defer-commit"],
    );
    assert_eq!(interrupted["status"], "connected");
    assert!(
        home.join("harness/code-tools-registration-pending.json")
            .is_file()
    );
    assert!(servers(&home).get("codebase-memory").is_none());
    let recovered = run_apply(&home, "Recover", &[]);
    assert_eq!(recovered["status"], "registration-recovered");
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), previous);
    assert_eq!(
        fs::read(home.join("harness/code-tools-registration.json")).unwrap(),
        previous_state
    );
    assert!(
        !home
            .join("harness/code-tools-registration-pending.json")
            .exists()
    );
    assert!(servers(&home).get("codebase-memory").is_some());
    assert!(servers(&home).get("codegraph").is_none());
    assert_eq!(
        fs::read(cache.join("indexes/project.db")).unwrap(),
        previous_index
    );
    assert_eq!(
        fs::read(cache.join("policy.json")).unwrap(),
        previous_policy
    );
    assert_eq!(previous_policy, CBM_POLICY);
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), b"shared-bytes");
}

#[test]
fn user_edited_owned_config_is_refused_and_preserved() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let package = shared_package(root.path(), "shared-package");
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    run_apply(&home, "Install", &["--retained-registrations-json", &plan]);
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    let edited = text.replacen("launch.py", "launch-user.py", 1);
    fs::write(home.join("config.toml"), edited.as_bytes()).unwrap();
    let preserved = fs::read(home.join("config.toml")).unwrap();
    let error = expect_error(&home, "Install", &["--retained-registrations-json", &plan]);
    assert!(
        error.contains("ownership conflict") || error.contains("preserving"),
        "{error}"
    );
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), preserved);
    let disconnect_error = expect_error(&home, "Disconnect", &[]);
    assert!(
        disconnect_error.contains("ownership conflict") || disconnect_error.contains("preserving"),
        "{disconnect_error}"
    );
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), preserved);
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), b"shared-bytes");
}

#[test]
fn disconnect_removes_owned_native_entry_and_keeps_foreign_server() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let package = shared_package(root.path(), "shared-package");
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    run_apply(&home, "Install", &["--retained-registrations-json", &plan]);
    run_apply(&home, "Disconnect", &[]);
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), FOREIGN);
    let parsed = parsed_config(&home);
    assert!(parsed["mcp_servers"].get("codegraph").is_none());
    assert!(parsed["mcp_servers"].get("serena").is_none());
    assert_eq!(parsed["mcp_servers"]["foreign"]["command"], "untouched.exe");
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), b"shared-bytes");
}

#[test]
fn update_replaces_retained_entry_and_leaves_shared_package_in_place() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let first = shared_package(root.path(), "shared-package-a");
    let second = shared_package(root.path(), "shared-package-b");
    let stale = retained_plan("C:\\old-python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    run_apply(&home, "Install", &["--retained-registrations-json", &stale]);
    let planned = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let updated = run_apply(
        &home,
        "Update",
        &["--retained-registrations-json", &planned],
    );
    assert_eq!(updated["status"], "connected");
    let servers = servers(&home);
    assert_eq!(servers["serena"]["command"], "C:\\python\\python.exe");
    assert_eq!(
        retained_names(&servers),
        vec!["serena".to_string(), "nuphus".to_string()]
    );
    assert!(servers.get("codegraph").is_none(), "{servers}");
    assert_eq!(fs::read(first.join("keep.txt")).unwrap(), b"shared-bytes");
    assert_eq!(fs::read(second.join("keep.txt")).unwrap(), b"shared-bytes");
    assert_eq!(servers["foreign"]["command"], "untouched.exe");
}

#[test]
fn recover_preserves_later_user_edits_after_interrupted_activation() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = write_cbm_home(root.path());
    let _package = shared_package(root.path(), "shared-package");
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    run_apply(
        &home,
        "Install",
        &["--retained-registrations-json", &plan, "--defer-commit"],
    );
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    fs::write(
        home.join("config.toml"),
        format!("{text}\n[later.keep]\nlater_setting = \"keep-me\"\n"),
    )
    .unwrap();
    let recovered = run_apply(&home, "Recover", &[]);
    assert_eq!(recovered["status"], "registration-recovered");
    let restored = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(restored.contains("later_setting = \"keep-me\""));
    assert!(servers(&home).get("codebase-memory").is_some());
    assert!(servers(&home).get("codegraph").is_none());
    assert_eq!(servers(&home)["foreign"]["command"], "untouched.exe");
    assert_eq!(fs::read(cache.join("policy.json")).unwrap(), CBM_POLICY);
    assert_eq!(
        fs::read(cache.join("indexes/project.db")).unwrap(),
        b"owned-cbm-index"
    );
}

#[test]
fn install_update_and_disconnect_preserve_tui_normalized_foreign_bytes() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = seed_normalized_cbm_home(root.path());
    let package = shared_package(root.path(), "shared-package");
    let before_index = fs::read(cache.join("indexes/project.db")).unwrap();
    let before_policy = fs::read(cache.join("policy.json")).unwrap();
    let before_shared = fs::read(package.join("keep.txt")).unwrap();
    let stale_block = receipt(&home)["block"].as_str().unwrap().to_string();
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let connected = run_apply(&home, "Install", &["--retained-registrations-json", &plan]);
    assert_eq!(connected["status"], "connected");
    let after_install = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(!after_install.contains(&stale_block));
    assert!(after_install.contains("# retain my comment"));
    assert!(after_install.contains("[projects.\"D:/foreign project\"]"));
    assert!(after_install.contains("Literal header, not a table:"));
    assert!(after_install.contains("[[features]]"));
    let installed = servers(&home);
    assert!(installed.get("codebase-memory").is_none(), "{installed}");
    assert!(installed.get("codegraph").is_none(), "{installed}");
    assert_eq!(installed["foreign"]["command"], "untouched.exe");
    assert_eq!(
        parsed_config(&home)["projects"]["D:/foreign project"]["trust_level"],
        "trusted"
    );
    assert_eq!(
        fs::read(cache.join("indexes/project.db")).unwrap(),
        before_index
    );
    assert_eq!(fs::read(cache.join("policy.json")).unwrap(), before_policy);
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), before_shared);

    fs::write(
        home.join("config.toml"),
        format!(
            "{}\n[later.keep]\nlater_setting = \"keep-me\"\n",
            fs::read_to_string(home.join("config.toml")).unwrap()
        ),
    )
    .unwrap();
    restamp_owned_block_as_toml(&home);
    let revised = retained_plan("C:\\python-two\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let updated = run_apply(
        &home,
        "Update",
        &["--retained-registrations-json", &revised],
    );
    assert_eq!(updated["status"], "connected");
    let after_update = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(after_update.contains("later_setting = \"keep-me\""));
    assert!(after_update.contains("[projects.\"D:/foreign project\"]"));
    assert_eq!(servers(&home)["foreign"]["command"], "untouched.exe");
    restamp_owned_block_as_toml(&home);
    run_apply(&home, "Disconnect", &[]);
    let after_disconnect = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(after_disconnect.contains("later_setting = \"keep-me\""));
    assert!(after_disconnect.contains("# retain my comment"));
    assert!(after_disconnect.contains("[[features]]"));
    assert!(servers(&home).get("codegraph").is_none());
    assert_eq!(servers(&home)["foreign"]["command"], "untouched.exe");
}

#[test]
fn recover_preserves_normalized_later_edits_and_refuses_owned_semantic_changes() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = seed_normalized_cbm_home(root.path());
    let _package = shared_package(root.path(), "shared-package");
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    run_apply(
        &home,
        "Install",
        &["--retained-registrations-json", &plan, "--defer-commit"],
    );
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    fs::write(
        home.join("config.toml"),
        format!("{text}\n[later.keep]\nlater_setting = \"keep-me\"\n"),
    )
    .unwrap();
    restamp_owned_block_as_toml(&home);
    let recovered = run_apply(&home, "Recover", &[]);
    assert_eq!(recovered["status"], "registration-recovered");
    let restored = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(restored.contains("later_setting = \"keep-me\""));
    assert!(restored.contains("[projects.\"D:/foreign project\"]"));
    assert!(restored.contains("Literal header, not a table:"));
    assert!(servers(&home).get("codebase-memory").is_some());
    assert!(servers(&home).get("codegraph").is_none());
    assert_eq!(servers(&home)["foreign"]["command"], "untouched.exe");
    assert_eq!(fs::read(cache.join("policy.json")).unwrap(), CBM_POLICY);

    run_apply(
        &home,
        "Install",
        &["--retained-registrations-json", &plan, "--defer-commit"],
    );
    restamp_owned_block_as_toml(&home);
    let owned_edit = fs::read_to_string(home.join("config.toml"))
        .unwrap()
        .replacen("launch.py", "launch-edited.py", 1);
    fs::write(home.join("config.toml"), owned_edit.as_bytes()).unwrap();
    let error = expect_error(&home, "Recover", &[]);
    assert!(
        error.contains("ownership conflict") || error.contains("preserving"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(home.join("config.toml")).unwrap(),
        owned_edit
    );
    assert!(servers(&home).get("codegraph").is_none());
}

#[test]
fn install_owns_readiness_zero_and_disconnect_restores_prior_absence_or_exact_zero() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let _package = shared_package(root.path(), "shared-package");
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    run_apply(&home, "Install", &["--retained-registrations-json", &plan]);
    let installed = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(installed.contains(&format!("{READINESS} = 0\n")));
    run_apply(&home, "Disconnect", &[]);
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), FOREIGN);

    let preexisting = format!("{READINESS} = 0\n{}", String::from_utf8_lossy(FOREIGN));
    fs::write(home.join("config.toml"), preexisting.as_bytes()).unwrap();
    run_apply(&home, "Install", &["--retained-registrations-json", &plan]);
    run_apply(&home, "Disconnect", &[]);
    assert_eq!(
        fs::read_to_string(home.join("config.toml")).unwrap(),
        preexisting
    );
}

#[test]
fn explicit_nonzero_or_edited_owned_readiness_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let _package = shared_package(root.path(), "shared-package");
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    for value in ["1000", "false", "0.0"] {
        let conflicting = format!(
            "{READINESS} = {value}\n{}",
            std::str::from_utf8(FOREIGN).unwrap()
        );
        fs::write(home.join("config.toml"), conflicting.as_bytes()).unwrap();
        let error = expect_error(&home, "Install", &["--retained-registrations-json", &plan]);
        assert!(
            error.contains("readiness") || error.contains("preserving"),
            "{error}"
        );
        assert_eq!(
            fs::read_to_string(home.join("config.toml")).unwrap(),
            conflicting
        );
    }

    fs::write(home.join("config.toml"), FOREIGN).unwrap();
    run_apply(&home, "Install", &["--retained-registrations-json", &plan]);
    let installed = fs::read_to_string(home.join("config.toml")).unwrap();
    let state = fs::read(home.join("harness/code-tools-registration.json")).unwrap();
    for edited in [
        installed.replacen(&format!("{READINESS} = 0\n"), "", 1),
        installed.replacen(
            &format!("{READINESS} = 0\n"),
            &format!("{READINESS} = 25\n"),
            1,
        ),
    ] {
        fs::write(home.join("config.toml"), edited.as_bytes()).unwrap();
        for mode in ["Check", "Install", "Disconnect"] {
            let extra: &[&str] = if mode == "Install" {
                &["--retained-registrations-json", &plan]
            } else {
                &[]
            };
            let error = expect_error(&home, mode, extra);
            assert!(
                error.contains("readiness") || error.contains("preserving"),
                "{error}"
            );
            assert_eq!(
                fs::read_to_string(home.join("config.toml")).unwrap(),
                edited
            );
            assert_eq!(
                fs::read(home.join("harness/code-tools-registration.json")).unwrap(),
                state
            );
        }
    }
}

#[test]
fn retained_activation_is_native_and_does_not_invoke_python() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = write_cbm_home(root.path());
    let package = shared_package(root.path(), "shared-package");
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let output = apply_command(&home, "Install", &["--retained-registrations-json", &plan])
        .env("PATH", "")
        .env_remove("HARNESS_ACCEPTANCE_PYTHON")
        .env_remove("PYTHONHOME")
        .env_remove("VIRTUAL_ENV")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", fail(&output));
    let connected: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(connected["status"], "connected");
    let servers = servers(&home);
    assert!(servers.get("codebase-memory").is_none());
    assert!(servers.get("harness-lsp").is_none());
    assert!(servers.get("codegraph").is_none());
    assert_eq!(
        retained_names(&servers),
        vec!["serena".to_string(), "nuphus".to_string()]
    );
    assert_eq!(fs::read(cache.join("policy.json")).unwrap(), CBM_POLICY);
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), b"shared-bytes");
}

#[test]
fn fresh_install_registers_planned_retained_tools_and_omits_retired_names() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let _package = shared_package(root.path(), "shared-package");
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let connected = run_apply(&home, "Install", &["--retained-registrations-json", &plan]);
    assert_eq!(connected["status"], "connected");
    let servers = servers(&home);
    assert!(servers.get("codebase-memory").is_none(), "{servers}");
    assert!(servers.get("harness-lsp").is_none(), "{servers}");
    assert!(servers.get("codegraph").is_none(), "{servers}");
    assert!(servers.get("graphify").is_none(), "{servers}");
    assert_eq!(servers["serena"]["command"], "C:\\python\\python.exe");
    assert_eq!(servers["nuphus"]["args"][5], "nuphus");
    assert_eq!(servers["foreign"]["command"], "untouched.exe");
}

#[test]
fn update_replaces_stale_owned_receipt_with_newly_planned_inventory() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let first = shared_package(root.path(), "shared-package-a");
    let second = shared_package(root.path(), "shared-package-b");
    let stale = json!({
        "serena": {
            "command": "C:\\old-python\\python.exe",
            "args": ["-B", "-u", "launch.py", "serena"],
            "env": {"CODEX_HOME": home}
        }
    })
    .to_string();
    run_apply(&home, "Install", &["--retained-registrations-json", &stale]);
    assert!(servers(&home).get("graphify").is_none());
    let planned = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let updated = run_apply(
        &home,
        "Update",
        &["--retained-registrations-json", &planned],
    );
    assert_eq!(updated["status"], "connected");
    let servers = servers(&home);
    assert_eq!(servers["serena"]["command"], "C:\\python\\python.exe");
    assert_eq!(
        retained_names(&servers),
        vec!["serena".to_string(), "nuphus".to_string()]
    );
    assert!(servers.get("codebase-memory").is_none());
    assert!(servers.get("harness-lsp").is_none());
    assert!(servers.get("codegraph").is_none());
    assert_eq!(fs::read(first.join("keep.txt")).unwrap(), b"shared-bytes");
    assert_eq!(fs::read(second.join("keep.txt")).unwrap(), b"shared-bytes");
    assert_eq!(servers["foreign"]["command"], "untouched.exe");
}

#[test]
fn retained_handoff_cannot_select_codegraph_or_enable_lsp() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let _package = shared_package(root.path(), "shared-package");
    let hijack = json!({
        "codegraph": {
            "command": "python.exe",
            "args": ["-B", "-u", "launch.py", "codegraph"],
            "env": {"CODEX_HOME": home}
        },
        "harness-lsp": {
            "command": "python.exe",
            "args": ["-B", "-u", "launch.py", "harness-lsp"],
            "env": {"CODEX_HOME": home}
        },
        "serena": {
            "command": "C:\\python\\python.exe",
            "args": ["-B", "-u", "launch.py", "serena"],
            "env": {"CODEX_HOME": home}
        }
    })
    .to_string();
    run_apply(
        &home,
        "Install",
        &["--retained-registrations-json", &hijack],
    );
    let servers = servers(&home);
    assert!(servers.get("codegraph").is_none(), "{servers}");
    assert!(servers.get("harness-lsp").is_none(), "{servers}");
    assert_eq!(servers["serena"]["command"], "C:\\python\\python.exe");
}

#[test]
fn isolated_apply_accepts_native_serena_proxy_and_keeps_foreign_servers() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let _package = shared_package(root.path(), "shared-package");
    let console = root.path().join("serena.exe");
    fs::write(&console, b"fixture console").unwrap();
    let serena = json!({
        "command": env!("CARGO_BIN_EXE_codex-harness"),
        "args": [
            "mcp",
            "serena",
            "--serena",
            console,
            "--registry",
            home.join("harness/code-tools.json"),
            "--codex-home",
            home,
            "--source-root",
            root.path(),
            "--connection-seconds",
            "86400"
        ],
        "env": {"CODEX_HOME": home},
        "startup_timeout_sec": 30,
        "tool_timeout_sec": 660
    });
    let nuphus = json!({
        "command": "C:\\pwsh\\pwsh.exe",
        "args": ["-NoLogo", "-NoProfile", "-File", "mcp.ps1", "-Server", "nuphus"],
        "env": {"CODEX_HOME": home}
    });
    let connected = run_apply(
        &home,
        "Install",
        &[
            "--retained-registrations-json",
            &json!({
                "serena": serena,
                "nuphus": nuphus
            })
            .to_string(),
        ],
    );
    assert_eq!(connected["status"], "connected", "{connected}");
    let servers = servers(&home);
    assert!(
        servers["serena"]["command"] == env!("CARGO_BIN_EXE_codex-harness"),
        "{servers}"
    );
    assert_eq!(servers["serena"]["args"][1], "serena");
    assert!(servers.get("graphify").is_none(), "{servers}");
    assert!(servers.get("codegraph").is_none(), "{servers}");
    assert_eq!(servers["nuphus"]["args"][5], "nuphus");
    assert_eq!(servers["foreign"]["command"], "untouched.exe");
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(text.contains("retain my comment"), "{text}");
    assert!(text.contains("untouched.exe"), "{text}");
}
