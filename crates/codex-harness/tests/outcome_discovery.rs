//! Fixed app-server protocol and actual native discovery; no model calls.
#![cfg(windows)]
use serde_json::{Value, json};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    root: tempfile::TempDir,
    case: PathBuf,
    home: PathBuf,
    request: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("discovery проверка-")
            .tempdir()
            .unwrap();
        let case = root.path().join("case");
        let home = root.path().join("home");
        fs::create_dir(&case).unwrap();
        fs::create_dir(&home).unwrap();
        fs::write(home.join("config.toml"),"model = \"gpt-6-astra\"\ncheck_for_update_on_startup = false\n[features]\nhooks = false\nmulti_agent = false\n").unwrap();
        let request = root.path().join("request.json");
        fs::write(&request,serde_json::to_vec(&json!({"case_root":case,"codex_home":home,
            "upstream":env!("CARGO_BIN_EXE_harness-launch-fixture"),"timeout":5,"extra_config":{"skills.config":[]}})).unwrap()).unwrap();
        Self {
            root,
            case,
            home,
            request,
        }
    }
    fn edit(&self, key: &str, value: Value) {
        let mut request = read(&self.request);
        request[key] = value;
        fs::write(&self.request, serde_json::to_vec(&request).unwrap()).unwrap();
    }
    fn command(&self, mode: &str) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
        cmd.args(["outcome-discover", "--request"])
            .arg(&self.request)
            .current_dir(self.root.path())
            .env("HARNESS_LAUNCH_FIXTURE_MODE", "discovery")
            .env("HARNESS_DISCOVERY_FIXTURE", mode);
        cmd
    }
    fn run(&self, mode: &str, expected: i32) -> Value {
        self.check(self.command(mode).output().unwrap(), expected)
    }
    fn check(&self, output: Output, expected: i32) -> Value {
        assert_eq!(
            output.status.code(),
            Some(expected),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let root = evidence(&value);
        println!("discovery evidence: {}", root.display());
        assert_eq!(value, read(&root.join("discovery.json")));
        assert!(!root.starts_with(self.root.path()));
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("private discovery fixture sentinel")
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("private discovery fixture sentinel")
        );
        assert_eq!(value["model_calls"], 0);
        value
    }
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn evidence(value: &Value) -> PathBuf {
    PathBuf::from(value["evidence_root"].as_str().unwrap())
}

#[test]
fn actual_protocol_orders_requests_verifies_scope_and_leaves_config_unchanged() {
    let f = Fixture::new();
    let before = fs::read(f.home.join("config.toml")).unwrap();
    let value = f.run("success", 0);
    assert_eq!(value["status"], "passed");
    assert_eq!(value["skills"].as_array().unwrap().len(), 2);
    assert_eq!(value["config"]["config"]["model"], "gpt-6-astra");
    assert_eq!(value["base_configuration_unchanged"], true);
    let requests = fs::read_to_string(f.case.join("fixture-requests.jsonl")).unwrap();
    let rows: Vec<Value> = requests
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(
        rows.iter()
            .map(|r| r["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["initialize", "initialized", "skills/list", "config/read"]
    );
    assert_eq!(rows[0]["id"], 1);
    assert_eq!(rows[2]["id"], 2);
    assert_eq!(rows[3]["id"], 3);
    assert_eq!(rows[2]["params"]["forceReload"], true);
    assert_eq!(fs::read(f.home.join("config.toml")).unwrap(), before);
    assert_eq!(value["process"]["outcome"]["job"]["active_processes"], 0);
    assert_eq!(value["process"]["assigned_before_resume"], true);
    let request = read(&evidence(&value).join("request.json"));
    assert_eq!(
        request["arguments"],
        json!(["app-server", "--stdio", "-c", "skills.config=[]"])
    );
    assert_eq!(
        value["process"]["outcome"]["job"]["memory_limit_bytes"],
        512 * 1024 * 1024
    );
}

#[test]
fn malformed_wrong_id_error_incomplete_scope_and_nonzero_exit_cannot_pass() {
    for mode in [
        "malformed",
        "truncated",
        "unknown-id",
        "init-error",
        "skills-error",
        "cwd-mismatch",
        "nonzero",
    ] {
        let f = Fixture::new();
        f.edit("timeout", json!(1));
        if mode == "cwd-mismatch" {
            fs::create_dir(f.case.join("other")).unwrap();
        }
        let row = f.run(mode, 1);
        assert_eq!(row["status"], "failed", "{mode}");
        assert!(evidence(&row).join("rpc.jsonl").is_file());
        assert!(evidence(&row).join("failure.json").is_file());
        if ["malformed", "init-error", "skills-error"].contains(&mode) {
            assert!(
                fs::read_to_string(evidence(&row).join("stderr.txt"))
                    .unwrap()
                    .contains("private discovery fixture sentinel")
            );
        }
        assert_eq!(row["process"]["outcome"]["job"]["active_processes"], 0);
        if mode == "nonzero" {
            assert_eq!(row["process"]["outcome"]["exit_code"], 19);
        }
    }
}

#[test]
fn initialization_must_confirm_the_selected_native_home_before_any_discovery() {
    for mode in ["wrong-home", "missing-home"] {
        let f = Fixture::new();
        let row = f.run(mode, 1);
        assert_eq!(row["status"], "failed");
        let requests = fs::read_to_string(f.case.join("fixture-requests.jsonl")).unwrap();
        assert_eq!(requests.lines().count(), 1);
        assert_eq!(
            serde_json::from_str::<Value>(requests.lines().next().unwrap()).unwrap()["method"],
            "initialize"
        );
    }
}

#[test]
fn trailing_errors_or_requests_cannot_hide_behind_a_valid_last_response() {
    for mode in [
        "duplicate-final",
        "trailing-malformed",
        "late-server-request",
    ] {
        let f = Fixture::new();
        let row = f.run(mode, 1);
        assert_eq!(row["status"], "failed");
        assert_eq!(row["process"]["outcome"]["exit_code"], 0);
        assert!(evidence(&row).join("response.json").is_file());
    }
}

#[test]
fn startup_eof_and_output_limits_bound_real_process_lifetime() {
    for mode in ["no-read", "hang-after-eof", "output-limit"] {
        let f = Fixture::new();
        f.edit("timeout", json!(1));
        f.edit("output_limit", json!(64 * 1024));
        let started = Instant::now();
        let value = f.run(mode, 1);
        assert!(started.elapsed() < Duration::from_secs(6));
        assert_eq!(value["process"]["outcome"]["job"]["active_processes"], 0);
        if mode == "output-limit" {
            assert_eq!(value["process"]["output_limit_reached"], true);
        }
    }
}

#[test]
fn successful_rpc_still_cleans_a_descendant_after_root_exit() {
    let f = Fixture::new();
    let row = f.run("root-exit-child", 0);
    assert!(f.case.join("descendant-pid.txt").is_file());
    assert_eq!(row["process"]["outcome"]["job"]["active_processes"], 0);
    std::thread::sleep(Duration::from_secs(3));
    assert!(!f.case.join("descendant-survived.txt").exists());
}

#[test]
fn profile_discovery_overrides_and_invalid_routes_fail_before_launch() {
    let f = Fixture::new();
    for text in [
        "[skills]\nconfig=[]\n".to_owned(),
        "project_root_markers=[]\n".into(),
        "credential_broker={}\n".into(),
        format!(
            "[projects.{}]\ntrust_level=\"trusted\"\n",
            json!(f.case.to_str().unwrap())
        ),
    ] {
        fs::write(f.home.join("harness.config.toml"), text).unwrap();
        let row = f.run("success", 1);
        assert!(!evidence(&row).join("process-started.json").exists());
        assert!(!f.case.join("fixture-requests.jsonl").exists());
    }
    fs::remove_file(f.home.join("harness.config.toml")).unwrap();
    f.edit(
        "extra_config",
        json!({"openai_base_url":"http://127.0.0.1:9"}),
    );
    assert_eq!(f.run("success", 1)["status"], "failed");
    assert!(!f.case.join("fixture-requests.jsonl").exists());
}

#[test]
fn unrelated_profile_settings_and_declared_links_remain_read_only() {
    let f = Fixture::new();
    let target = f.root.path().join("profile-source.toml");
    let other = f.root.path().join("unrelated");
    fs::create_dir(&other).unwrap();
    let text = format!(
        "model=\"gpt-6-astra\"\n[projects.{}]\ntrust_level=\"trusted\"\n",
        json!(other.to_str().unwrap())
    );
    fs::write(&target, &text).unwrap();
    std::os::windows::fs::symlink_file(&target, f.home.join("harness.config.toml")).unwrap();
    assert_eq!(f.run("success", 0)["status"], "passed");
    assert_eq!(fs::read_to_string(&target).unwrap(), text);
}

#[test]
fn absent_base_config_is_not_created_by_the_discovery_client() {
    let f = Fixture::new();
    fs::remove_file(f.home.join("config.toml")).unwrap();
    assert_eq!(f.run("success", 0)["status"], "passed");
    assert!(!f.home.join("config.toml").exists());
}

struct OwnedChild(Option<Child>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn changed_inputs_cannot_supply_an_accepted_discovery() {
    for mode in [
        "changed-bytes",
        "same-bytes-new-object",
        "base-appeared",
        "profile-retarget",
        "upstream-changed",
    ] {
        let f = Fixture::new();
        f.edit("timeout", json!(10));
        let config = f.home.join("config.toml");
        let before = fs::read(&config).unwrap();
        let profile = f.home.join("harness.config.toml");
        let first = f.root.path().join("profile-one.toml");
        let second = f.root.path().join("profile-two.toml");
        let upstream = f.root.path().join("upstream.exe");
        if mode == "base-appeared" {
            fs::remove_file(&config).unwrap();
        }
        if mode == "profile-retarget" {
            fs::write(&first, "model=\"gpt-6-astra\"\n").unwrap();
            fs::write(&second, "model=\"gpt-6-astra\"\n").unwrap();
            std::os::windows::fs::symlink_file(&first, &profile).unwrap();
        }
        if mode == "upstream-changed" {
            fs::copy(env!("CARGO_BIN_EXE_harness-launch-fixture"), &upstream).unwrap();
            f.edit("upstream", json!(upstream));
        }
        let mut child = OwnedChild(Some(
            f.command("pause-before-config")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ));
        let until = Instant::now() + Duration::from_secs(8);
        while !f.case.join("config-read-ready").exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            f.case.join("config-read-ready").is_file(),
            "{mode}: double never reached config/read"
        );
        match mode {
            "changed-bytes" => {
                fs::write(&config, "# changed by an independent owned actor\n").unwrap()
            }
            "same-bytes-new-object" => {
                fs::rename(&config, f.home.join("retained-config.toml")).unwrap();
                fs::write(&config, &before).unwrap();
            }
            "base-appeared" => fs::write(&config, &before).unwrap(),
            "profile-retarget" => {
                fs::remove_file(&profile).unwrap();
                std::os::windows::fs::symlink_file(&second, &profile).unwrap();
            }
            "upstream-changed" => {
                fs::rename(&upstream, f.root.path().join("retained-upstream.exe")).unwrap();
                let mut candidate = fs::read(env!("CARGO_BIN_EXE_harness-launch-fixture")).unwrap();
                candidate.extend_from_slice(b"owned changed executable bytes");
                fs::write(&upstream, candidate).unwrap();
            }
            _ => unreachable!(),
        }
        fs::write(f.case.join("config-read-continue"), "").unwrap();
        let row = f.check(child.0.take().unwrap().wait_with_output().unwrap(), 1);
        assert_eq!(row["status"], "failed");
        assert_eq!(row["process"]["outcome"]["exit_code"], 0);
        assert_eq!(
            read(&evidence(&row).join("failure.json"))["phase"],
            "input-verification"
        );
        match mode {
            "changed-bytes" => assert_eq!(
                fs::read_to_string(&config).unwrap(),
                "# changed by an independent owned actor\n"
            ),
            "same-bytes-new-object" | "base-appeared" => {
                assert_eq!(fs::read(&config).unwrap(), before)
            }
            "profile-retarget" => assert_eq!(
                profile.canonicalize().unwrap(),
                second.canonicalize().unwrap()
            ),
            "upstream-changed" => assert!(
                fs::read(&upstream)
                    .unwrap()
                    .ends_with(b"owned changed executable bytes")
            ),
            _ => unreachable!(),
        }
    }
}

#[test]
#[ignore = "explicit installed native CLI, no models or authentication copy"]
fn installed_native_skills_and_configuration_are_observed_outside_checkout() -> io::Result<()> {
    let upstream = std::env::var_os("HARNESS_NATIVE_CODEX")
        .ok_or_else(|| io::Error::other("HARNESS_NATIVE_CODEX required"))?;
    let f = Fixture::new();
    f.edit("upstream", json!(PathBuf::from(upstream)));
    f.edit("timeout", json!(60));
    f.edit("extra_config", json!({}));
    let skill = f.home.join("skills/outcome-owned-discovery");
    fs::create_dir_all(&skill)?;
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: outcome-owned-discovery\ndescription: Owned model-free discovery fixture; never execute.\n---\nRead-only fixture.\n",
    )?;
    let before = fs::read(f.home.join("config.toml"))?;
    let value = f.run("success", 0);
    let selected: Vec<_> = value["skills"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["name"] == "outcome-owned-discovery")
        .collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0]["enabled"], true);
    assert_eq!(
        Path::new(selected[0]["path"].as_str().unwrap()).canonicalize()?,
        skill.join("SKILL.md").canonicalize()?
    );
    assert_eq!(fs::read(f.home.join("config.toml"))?, before);
    assert_eq!(value["config"]["config"]["model"], "gpt-6-astra");
    Ok(())
}
