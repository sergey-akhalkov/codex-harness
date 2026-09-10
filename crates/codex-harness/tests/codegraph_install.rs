//! Isolated CodeGraph Install/Update/Recover/Disconnect in an owned Codex home.
//! Does not download packages, probe the published runtime, or write live global MCP.
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

fn native_prepare_check_missing(home: &Path, state: &Path) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args([
            "mcp",
            "prepare-codegraph",
            "--mode",
            "Check",
            "--codex-home",
            home.to_str().unwrap(),
            "--dependency-state",
            state.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", fail(&output));
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{}", fail(&output)))
}

fn apply_command(home: &Path, mode: &str, package: Option<&Path>, extra: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    command.args([
        "mcp",
        "apply-codegraph-registration",
        "--mode",
        mode,
        "--codex-home",
        home.to_str().unwrap(),
        "--command",
        env!("CARGO_BIN_EXE_codex-harness"),
    ]);
    if let Some(package) = package {
        command.args(["--package-root", package.to_str().unwrap()]);
    }
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
        "graphify": {
            "command": python,
            "args": ["-B", "-u", "launch.py", "graphify"],
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
    ["serena", "graphify", "nuphus"]
        .into_iter()
        .filter(|name| servers.get(*name).is_some())
        .map(str::to_owned)
        .collect()
}

fn run_apply(home: &Path, mode: &str, package: Option<&Path>, extra: &[&str]) -> Value {
    let output = apply_command(home, mode, package, extra).output().unwrap();
    assert!(output.status.success(), "{}", fail(&output));
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| panic!("{}", fail(&output)))
}

fn expect_error(home: &Path, mode: &str, package: Option<&Path>, extra: &[&str]) -> String {
    let output = apply_command(home, mode, package, extra).output().unwrap();
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
    let shared = root.join("shared-package");
    fs::create_dir_all(&shared).unwrap();
    fs::write(shared.join("keep.txt"), b"shared-bytes").unwrap();
    (home, cache)
}

fn same_path(left: &str, right: &Path) -> bool {
    let left = PathBuf::from(left);
    left == right || left.canonicalize().ok() == right.canonicalize().ok()
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
    assert_ne!(restamped, block);
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
    assert_eq!(prepared["retired"], json!([]));
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), before);
    assert!(!state.exists());
    assert!(!home.join("harness/code-tools-registration.json").exists());
}

#[test]
fn install_replaces_owned_cbm_and_preserves_indexes_packages_and_unrelated_settings() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = write_cbm_home(root.path());
    let package = root.path().join("shared-package");
    let before_index = fs::read(cache.join("indexes/project.db")).unwrap();
    let before_policy = fs::read(cache.join("policy.json")).unwrap();
    let before_shared = fs::read(package.join("keep.txt")).unwrap();
    let connected = run_apply(&home, "Install", Some(&package), &[]);
    assert_eq!(connected["status"], "connected");
    let after = String::from_utf8(fs::read(home.join("config.toml")).unwrap()).unwrap();
    assert!(after.contains("untouched.exe"));
    assert!(after.contains("# retain my comment"));
    let servers = servers(&home);
    assert!(servers.get("codebase-memory").is_none(), "{servers}");
    let graph = &servers["codegraph"];
    assert_eq!(graph["command"], env!("CARGO_BIN_EXE_codex-harness"));
    assert_eq!(graph["args"][0], "mcp");
    assert_eq!(graph["args"][1], "codegraph");
    assert_eq!(graph["tool_timeout_sec"], 660);
    assert!(
        same_path(graph["env"]["CODEX_HOME"].as_str().unwrap(), &home),
        "{graph}"
    );
    assert_eq!(servers["foreign"]["command"], "untouched.exe");
    assert_eq!(
        fs::read(cache.join("indexes/project.db")).unwrap(),
        before_index
    );
    assert_eq!(fs::read(cache.join("policy.json")).unwrap(), before_policy);
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), before_shared);
    let checked = run_apply(&home, "Check", Some(&package), &[]);
    assert_eq!(checked["status"], "connected");
    assert_eq!(fs::read(cache.join("policy.json")).unwrap(), CBM_POLICY);
}

#[test]
fn check_without_codegraph_is_degraded_and_read_only() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let package = root.path().join("shared-package");
    fs::create_dir_all(&package).unwrap();
    let before = fs::read(home.join("config.toml")).unwrap();
    let checked = run_apply(&home, "Check", Some(&package), &[]);
    assert_eq!(checked["status"], "degraded");
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), before);
    assert!(!home.join("harness/code-tools-registration.json").exists());
}

#[test]
fn deferred_install_recover_restores_cbm_manual_policy_and_index_bytes() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = write_cbm_home(root.path());
    let package = root.path().join("shared-package");
    let previous = fs::read(home.join("config.toml")).unwrap();
    let previous_state = fs::read(home.join("harness/code-tools-registration.json")).unwrap();
    let previous_index = fs::read(cache.join("indexes/project.db")).unwrap();
    let previous_policy = fs::read(cache.join("policy.json")).unwrap();
    assert!(servers(&home).get("codebase-memory").is_some());
    let interrupted = run_apply(&home, "Install", Some(&package), &["--defer-commit"]);
    assert_eq!(interrupted["status"], "connected");
    assert!(
        home.join("harness/code-tools-registration-pending.json")
            .is_file()
    );
    assert!(servers(&home).get("codegraph").is_some());
    assert!(servers(&home).get("codebase-memory").is_none());
    let recovered = run_apply(&home, "Recover", None, &[]);
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
    let package = root.path().join("shared-package");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("keep.txt"), b"shared-bytes").unwrap();
    run_apply(&home, "Install", Some(&package), &[]);
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    let edited = text.replacen("--package-root", "--package-root-user", 1);
    fs::write(home.join("config.toml"), edited.as_bytes()).unwrap();
    let preserved = fs::read(home.join("config.toml")).unwrap();
    let error = expect_error(&home, "Install", Some(&package), &[]);
    assert!(
        error.contains("ownership conflict") || error.contains("preserving"),
        "{error}"
    );
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), preserved);
    let disconnect_error = expect_error(&home, "Disconnect", None, &[]);
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
    let package = root.path().join("shared-package");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("keep.txt"), b"shared-bytes").unwrap();
    run_apply(&home, "Install", Some(&package), &[]);
    run_apply(&home, "Disconnect", None, &[]);
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), FOREIGN);
    let parsed: Value =
        toml::from_str(&fs::read_to_string(home.join("config.toml")).unwrap()).unwrap();
    assert!(parsed["mcp_servers"].get("codegraph").is_none());
    assert_eq!(parsed["mcp_servers"]["foreign"]["command"], "untouched.exe");
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), b"shared-bytes");
}

#[test]
fn update_rewrites_owned_entry_and_leaves_shared_package_in_place() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let first = root.path().join("shared-package-a");
    let second = root.path().join("shared-package-b");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::write(first.join("keep.txt"), b"first").unwrap();
    fs::write(second.join("keep.txt"), b"second").unwrap();
    run_apply(&home, "Install", Some(&first), &[]);
    let updated = run_apply(&home, "Update", Some(&second), &[]);
    assert_eq!(updated["status"], "connected");
    let graph = servers(&home)["codegraph"].clone();
    assert!(
        same_path(graph["args"][3].as_str().unwrap(), &second),
        "{graph}"
    );
    assert_eq!(fs::read(first.join("keep.txt")).unwrap(), b"first");
    assert_eq!(fs::read(second.join("keep.txt")).unwrap(), b"second");
    assert_eq!(servers(&home)["foreign"]["command"], "untouched.exe");
}

#[test]
fn recover_preserves_later_user_edits_after_interrupted_activation() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = write_cbm_home(root.path());
    let package = root.path().join("shared-package");
    run_apply(&home, "Install", Some(&package), &["--defer-commit"]);
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    fs::write(
        home.join("config.toml"),
        format!("{text}\n[later.keep]\nlater_setting = \"keep-me\"\n"),
    )
    .unwrap();
    let recovered = run_apply(&home, "Recover", None, &[]);
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
    let package = root.path().join("shared-package");
    let before_index = fs::read(cache.join("indexes/project.db")).unwrap();
    let before_policy = fs::read(cache.join("policy.json")).unwrap();
    let before_shared = fs::read(package.join("keep.txt")).unwrap();
    let stale_block = receipt(&home)["block"].as_str().unwrap().to_string();
    let connected = run_apply(&home, "Install", Some(&package), &[]);
    assert_eq!(connected["status"], "connected");
    let after_install = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(!after_install.contains(&stale_block));
    assert!(after_install.contains("# retain my comment"));
    assert!(after_install.contains("[projects.\"D:/foreign project\"]"));
    assert!(after_install.contains("Literal header, not a table:"));
    assert!(after_install.contains("[[features]]"));
    let installed = servers(&home);
    assert!(installed.get("codebase-memory").is_none(), "{installed}");
    assert!(installed.get("codegraph").is_some(), "{installed}");
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

    let second = root.path().join("shared-package-b");
    fs::create_dir_all(&second).unwrap();
    fs::write(second.join("keep.txt"), b"second").unwrap();
    fs::write(
        home.join("config.toml"),
        format!(
            "{}\n[later.keep]\nlater_setting = \"keep-me\"\n",
            fs::read_to_string(home.join("config.toml")).unwrap()
        ),
    )
    .unwrap();
    restamp_owned_block_as_toml(&home);
    let updated = run_apply(&home, "Update", Some(&second), &[]);
    assert_eq!(updated["status"], "connected");
    let after_update = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(after_update.contains("later_setting = \"keep-me\""));
    assert!(after_update.contains("[projects.\"D:/foreign project\"]"));
    assert_eq!(servers(&home)["foreign"]["command"], "untouched.exe");
    restamp_owned_block_as_toml(&home);
    run_apply(&home, "Disconnect", None, &[]);
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
    let package = root.path().join("shared-package");
    run_apply(&home, "Install", Some(&package), &["--defer-commit"]);
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    fs::write(
        home.join("config.toml"),
        format!("{text}\n[later.keep]\nlater_setting = \"keep-me\"\n"),
    )
    .unwrap();
    restamp_owned_block_as_toml(&home);
    let recovered = run_apply(&home, "Recover", None, &[]);
    assert_eq!(recovered["status"], "registration-recovered");
    let restored = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(restored.contains("later_setting = \"keep-me\""));
    assert!(restored.contains("[projects.\"D:/foreign project\"]"));
    assert!(restored.contains("Literal header, not a table:"));
    assert!(servers(&home).get("codebase-memory").is_some());
    assert!(servers(&home).get("codegraph").is_none());
    assert_eq!(servers(&home)["foreign"]["command"], "untouched.exe");
    assert_eq!(fs::read(cache.join("policy.json")).unwrap(), CBM_POLICY);

    run_apply(&home, "Install", Some(&package), &["--defer-commit"]);
    restamp_owned_block_as_toml(&home);
    let owned_edit = fs::read_to_string(home.join("config.toml"))
        .unwrap()
        .replacen("--package-root", "--package-root-user", 1);
    fs::write(home.join("config.toml"), owned_edit.as_bytes()).unwrap();
    let preserved = fs::read(home.join("config.toml")).unwrap();
    let error = expect_error(&home, "Recover", None, &[]);
    assert!(
        error.contains("preserving")
            || error.contains("ownership")
            || error.contains("changed after")
            || error.contains("block changed"),
        "{error}"
    );
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), preserved);
    assert!(servers(&home).get("codegraph").is_some());
}

#[test]
fn recover_refuses_managed_block_edits_after_interrupted_activation() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = write_cbm_home(root.path());
    let package = root.path().join("shared-package");
    run_apply(&home, "Install", Some(&package), &["--defer-commit"]);
    let text = fs::read_to_string(home.join("config.toml")).unwrap();
    let edited = text.replacen("--package-root", "--package-root-user", 1);
    fs::write(home.join("config.toml"), edited.as_bytes()).unwrap();
    let preserved = fs::read(home.join("config.toml")).unwrap();
    let error = expect_error(&home, "Recover", None, &[]);
    assert!(
        error.contains("preserving")
            || error.contains("changed after")
            || error.contains("block changed"),
        "{error}"
    );
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), preserved);
    assert!(servers(&home).get("codegraph").is_some());
    assert_eq!(fs::read(cache.join("policy.json")).unwrap(), CBM_POLICY);
}

#[test]
fn install_owns_readiness_zero_and_disconnect_restores_prior_absence_or_exact_zero() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let package = root.path().join("shared-package");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("keep.txt"), b"shared-bytes").unwrap();
    run_apply(&home, "Install", Some(&package), &[]);
    let policy = &receipt(&home)["connection_policy"];
    assert_eq!(policy["key"], READINESS);
    assert_eq!(policy["value"], 0);
    assert_eq!(policy["previous_present"], false);
    assert_eq!(parsed_config(&home)[READINESS], 0);
    run_apply(&home, "Disconnect", None, &[]);
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), FOREIGN);
    assert!(parsed_config(&home).get(READINESS).is_none());

    let preexisting = format!(
        "\"{READINESS}\" = 0 # user selection\n{}",
        std::str::from_utf8(FOREIGN).unwrap()
    );
    fs::write(home.join("config.toml"), preexisting.as_bytes()).unwrap();
    run_apply(&home, "Install", Some(&package), &[]);
    let kept = &receipt(&home)["connection_policy"];
    assert_eq!(kept["previous_present"], true);
    assert!(kept.get("prefix").is_none());
    let after_install = fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(after_install.contains(&format!("\"{READINESS}\" = 0 # user selection")));
    run_apply(&home, "Disconnect", None, &[]);
    assert_eq!(
        fs::read_to_string(home.join("config.toml")).unwrap(),
        preexisting
    );
}

#[test]
fn explicit_nonzero_or_edited_owned_readiness_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let package = root.path().join("shared-package");
    fs::create_dir_all(&package).unwrap();
    for value in ["1000", "false", "0.0"] {
        let conflicting = format!(
            "{READINESS} = {value}\n{}",
            std::str::from_utf8(FOREIGN).unwrap()
        );
        fs::write(home.join("config.toml"), conflicting.as_bytes()).unwrap();
        let error = expect_error(&home, "Install", Some(&package), &[]);
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
    run_apply(&home, "Install", Some(&package), &[]);
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
            let error = expect_error(&home, mode, Some(&package), &[]);
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
fn selected_codegraph_activation_is_native_and_does_not_invoke_python() {
    let root = tempfile::tempdir().unwrap();
    let (home, cache) = write_cbm_home(root.path());
    let package = root.path().join("shared-package");
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let output = apply_command(
        &home,
        "Install",
        Some(&package),
        &["--retained-registrations-json", &plan],
    )
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
    assert!(servers.get("codegraph").is_some());
    assert_eq!(
        retained_names(&servers),
        vec![
            "serena".to_string(),
            "graphify".to_string(),
            "nuphus".to_string()
        ]
    );
    assert_eq!(fs::read(cache.join("policy.json")).unwrap(), CBM_POLICY);
    assert_eq!(fs::read(package.join("keep.txt")).unwrap(), b"shared-bytes");
}

#[test]
fn fresh_install_registers_planned_retained_tools_and_omits_cbm_and_lsp() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let package = root.path().join("shared-package");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("keep.txt"), b"shared-bytes").unwrap();
    let plan = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let connected = run_apply(
        &home,
        "Install",
        Some(&package),
        &["--retained-registrations-json", &plan],
    );
    assert_eq!(connected["status"], "connected");
    let servers = servers(&home);
    assert!(servers.get("codebase-memory").is_none(), "{servers}");
    assert!(servers.get("harness-lsp").is_none(), "{servers}");
    assert!(servers.get("codegraph").is_some(), "{servers}");
    assert_eq!(servers["serena"]["command"], "C:\\python\\python.exe");
    assert_eq!(servers["graphify"]["args"][3], "graphify");
    assert_eq!(servers["nuphus"]["args"][5], "nuphus");
    assert_eq!(servers["foreign"]["command"], "untouched.exe");
}

#[test]
fn update_replaces_stale_owned_receipt_with_newly_planned_inventory() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let first = root.path().join("shared-package-a");
    let second = root.path().join("shared-package-b");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::write(first.join("keep.txt"), b"first").unwrap();
    fs::write(second.join("keep.txt"), b"second").unwrap();
    let stale = json!({
        "serena": {
            "command": "C:\\old-python\\python.exe",
            "args": ["-B", "-u", "launch.py", "serena"],
            "env": {"CODEX_HOME": home}
        }
    })
    .to_string();
    run_apply(
        &home,
        "Install",
        Some(&first),
        &["--retained-registrations-json", &stale],
    );
    assert!(servers(&home).get("graphify").is_none());
    let planned = retained_plan("C:\\python\\python.exe", "C:\\pwsh\\pwsh.exe", &home);
    let updated = run_apply(
        &home,
        "Update",
        Some(&second),
        &["--retained-registrations-json", &planned],
    );
    assert_eq!(updated["status"], "connected");
    let servers = servers(&home);
    assert!(
        same_path(servers["codegraph"]["args"][3].as_str().unwrap(), &second),
        "{servers}"
    );
    assert_eq!(servers["serena"]["command"], "C:\\python\\python.exe");
    assert_eq!(
        retained_names(&servers),
        vec![
            "serena".to_string(),
            "graphify".to_string(),
            "nuphus".to_string()
        ]
    );
    assert!(servers.get("codebase-memory").is_none());
    assert!(servers.get("harness-lsp").is_none());
    assert_eq!(fs::read(first.join("keep.txt")).unwrap(), b"first");
    assert_eq!(fs::read(second.join("keep.txt")).unwrap(), b"second");
    assert_eq!(servers["foreign"]["command"], "untouched.exe");
}

#[test]
fn retained_handoff_cannot_select_codegraph_or_enable_lsp() {
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let package = root.path().join("shared-package");
    fs::create_dir_all(&package).unwrap();
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
        Some(&package),
        &["--retained-registrations-json", &hijack],
    );
    let servers = servers(&home);
    assert_eq!(
        servers["codegraph"]["command"],
        env!("CARGO_BIN_EXE_codex-harness")
    );
    assert_eq!(servers["codegraph"]["args"][1], "codegraph");
    assert!(servers.get("harness-lsp").is_none(), "{servers}");
    assert_eq!(servers["serena"]["command"], "C:\\python\\python.exe");
}

#[test]
#[ignore = "requires explicit existing CODEGRAPH_LIFECYCLE_PYTHON; parent runs --ignored with the adopted Serena interpreter"]
fn planner_handoff_from_existing_inspect_registers_retained_tools_without_mutating_during_plan() {
    let python = std::env::var("CODEGRAPH_LIFECYCLE_PYTHON")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .expect("CODEGRAPH_LIFECYCLE_PYTHON must name an existing lifecycle interpreter");
    let powershell = std::env::var("CODEGRAPH_LIFECYCLE_POWERSHELL")
        .unwrap_or_else(|_| std::env::var("PWSH").unwrap_or_else(|_| "pwsh".into()));
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let home = write_home(root.path());
    let package = root.path().join("shared-package");
    fs::create_dir_all(&package).unwrap();
    let before_config = fs::read(home.join("config.toml")).unwrap();
    let planner = source.join("tools/code-tools/registration.py");
    let projection = json!({
        "registrations": {
            "codegraph": {
                "command": env!("CARGO_BIN_EXE_codex-harness"),
                "args": ["mcp", "codegraph", "--package-root", package],
                "env": {"CODEX_HOME": home}
            }
        },
        "retired": ["codebase-memory"]
    })
    .to_string();
    let planned = Command::new(&python)
        .args([
            "-B",
            planner.to_str().unwrap(),
            "--codex-home",
            home.to_str().unwrap(),
            "--source-root",
            source.to_str().unwrap(),
            "--native-codex",
            env!("CARGO_BIN_EXE_codex-harness"),
            "--powershell",
            &powershell,
            "--python",
            &python,
            "--mode",
            "Install",
            "--plan-only",
            "--native-providers-json",
            &projection,
        ])
        .output()
        .unwrap();
    assert!(planned.status.success(), "{}", fail(&planned));
    assert_eq!(fs::read(home.join("config.toml")).unwrap(), before_config);
    assert!(!home.join("harness/code-tools-registration.json").exists());
    let handoff: Value =
        serde_json::from_slice(&planned.stdout).unwrap_or_else(|_| panic!("{}", fail(&planned)));
    let registrations = handoff
        .get("registrations")
        .cloned()
        .unwrap_or(handoff.clone());
    assert!(registrations.get("serena").is_some(), "{handoff}");
    assert!(registrations.get("graphify").is_some(), "{handoff}");
    assert!(registrations.get("nuphus").is_some(), "{handoff}");
    assert!(registrations.get("harness-lsp").is_none(), "{handoff}");
    let connected = run_apply(
        &home,
        "Install",
        Some(&package),
        &["--retained-registrations-json", &handoff.to_string()],
    );
    assert_eq!(connected["status"], "connected");
    let servers = servers(&home);
    assert!(servers.get("codegraph").is_some(), "{servers}");
    assert_eq!(
        retained_names(&servers),
        vec![
            "serena".to_string(),
            "graphify".to_string(),
            "nuphus".to_string()
        ]
    );
    assert!(servers.get("codebase-memory").is_none(), "{servers}");
    assert!(servers.get("harness-lsp").is_none(), "{servers}");
    assert_eq!(servers["foreign"]["command"], "untouched.exe");
}
