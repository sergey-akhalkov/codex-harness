//! First comparison pair for project-verification shortening. Model runs stay opt-in.
#![cfg(windows)]
use serde_json::{Value, json};
use skill_evolution::{isolation, package, plan};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn live_skill() -> PathBuf {
    workspace_root().join(".agents/skills/project-verification")
}

fn short_skill() -> PathBuf {
    workspace_root().join("crates/skill-evolution/fixtures/project-verification-short")
}

fn live_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("USERPROFILE").unwrap()).join(".codex"))
}

fn native_launcher() -> PathBuf {
    let requested = PathBuf::from(std::env::var_os("HARNESS_NATIVE_CODEX").unwrap());
    let registration = live_codex_home().join("harness/native-launch.json");
    if let Ok(bytes) = fs::read(&registration) {
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            if let Some(path) = value["upstream"]["executable"].as_str() {
                let upstream = PathBuf::from(path);
                if upstream.is_file() {
                    return upstream;
                }
            }
        }
    }
    requested
}

fn launch_path(path: &Path) -> String {
    let text = path.canonicalize().unwrap().to_string_lossy().into_owned();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

fn copy_auth(isolated_home: &Path) {
    let auth = live_codex_home().join("auth.json");
    if auth.is_file() {
        fs::copy(&auth, isolated_home.join("auth.json")).unwrap();
    }
}

fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

struct Pair {
    _root: tempfile::TempDir,
    source: PathBuf,
    control: PathBuf,
    baseline_library: PathBuf,
    candidate_library: PathBuf,
    marker: PathBuf,
    live_revision: String,
}

impl Pair {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("skill-eval-pilot-")
            .tempdir()
            .unwrap();
        let source = workspace_root().canonicalize().unwrap();
        let case = root.path().join("case");
        let control = root.path().join("control");
        let baseline_library = root.path().join("L");
        let candidate_library = root.path().join("Lp");
        fs::create_dir(&case).unwrap();
        fs::create_dir(&control).unwrap();
        let live = package::load(&live_skill()).unwrap();
        package::copy_into(&live_skill(), &baseline_library).unwrap();
        package::copy_into(&short_skill(), &candidate_library).unwrap();
        fs::write(control.join("oracle.json"), "{\"case\":\"entrypoint\"}").unwrap();
        fs::write(
            control.join("baseline.json"),
            serde_json::to_vec(&json!({"revision": live.revision})).unwrap(),
        )
        .unwrap();
        let marker = live_skill().join("SKILL.md");
        Self {
            _root: root,
            source,
            control,
            baseline_library,
            candidate_library,
            marker,
            live_revision: live.revision,
        }
    }

    fn isolate(&self, library: &Path) -> isolation::Report {
        isolation::isolate(&isolation::Request {
            source_root: self.source.clone(),
            case_root: self._root.path().join("case"),
            control_root: self.control.clone(),
            library_root: library.to_path_buf(),
            session_marker: self.marker.clone(),
        })
        .unwrap()
    }
}

#[test]
fn first_pair_isolation_uses_live_skill_and_does_not_touch_it() {
    let pair = Pair::new();
    let before = fs::read(&pair.marker).unwrap();
    let baseline = pair.isolate(&pair.baseline_library);
    let candidate = pair.isolate(&pair.candidate_library);
    assert!(baseline.isolation_verified);
    assert!(candidate.isolation_verified);
    assert_eq!(baseline.model_calls, 0);
    assert_eq!(candidate.model_calls, 0);
    assert_eq!(baseline.library.name, plan::OWNED_SKILL);
    assert_eq!(candidate.library.name, plan::OWNED_SKILL);
    assert_eq!(baseline.library.description, candidate.library.description);
    assert_eq!(baseline.library.revision, pair.live_revision);
    assert_ne!(candidate.library.revision, baseline.library.revision);
    assert_eq!(baseline.control_files_write, "denied");
    assert_eq!(fs::read(&pair.marker).unwrap(), before);
    assert_eq!(
        package::load(&live_skill()).unwrap().revision,
        pair.live_revision
    );
}

#[test]
fn shortening_batch_prepares_declared_cases_without_models() {
    let batch = plan::shortening_batch();
    assert_ne!(batch.intended, batch.held_out);
    let live_before = package::load(&live_skill()).unwrap().revision;
    let mut prepared = Vec::new();
    for case_id in batch.cases() {
        let host = tempfile::Builder::new()
            .prefix(&format!("skill-eval-batch-{case_id}-"))
            .tempdir()
            .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["outcome-prepare", "--case", case_id])
            .current_dir(host.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{case_id} {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["case_id"], case_id);
        prepared.push(case_id.to_string());
        let _ = host.keep();
    }
    assert_eq!(prepared, ["entrypoint", "negative", "missing", "freshness"]);
    assert_eq!(package::load(&live_skill()).unwrap().revision, live_before);
    assert_eq!(
        skill_evolution::decision::decide(&plan::provider_limited_batch_evidence()),
        skill_evolution::decision::Verdict::Inconclusive
    );
}

#[test]
#[ignore = "explicit installed native CLI and --run-model-probes"]
fn first_pair_runs_through_outcome_prepare_and_outcome_run() {
    std::env::var_os("HARNESS_NATIVE_CODEX").expect("HARNESS_NATIVE_CODEX required");
    let launcher = native_launcher();
    assert!(launcher.is_file());
    let plan = plan::pilot();
    let live_before = package::load(&live_skill()).unwrap();
    let mut arms = Vec::new();
    for (name, skill) in [("baseline", live_skill()), ("candidate", short_skill())] {
        let host = tempfile::Builder::new()
            .prefix(&format!("skill-eval-{name}-"))
            .tempdir()
            .unwrap();
        let prepared = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["outcome-prepare", "--case", &plan.case_id])
            .current_dir(host.path())
            .output()
            .unwrap();
        assert!(
            prepared.status.success(),
            "{}",
            String::from_utf8_lossy(&prepared.stderr)
        );
        let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
        let case = PathBuf::from(prepared["case_root"].as_str().unwrap());
        let home = host.path().join("home");
        let user = host.path().join("user");
        let library = host.path().join("library");
        let control = host.path().join("control");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&user).unwrap();
        fs::create_dir(&control).unwrap();
        copy_auth(&home);
        package::copy_into(&skill, &library).unwrap();
        fs::write(control.join("oracle.json"), "{\"case\":\"entrypoint\"}").unwrap();
        fs::write(
            control.join("baseline.json"),
            serde_json::to_vec(
                &json!({"arm": name, "revision": package::load(&library).unwrap().revision}),
            )
            .unwrap(),
        )
        .unwrap();
        let isolated = isolation::isolate(&isolation::Request {
            source_root: workspace_root(),
            case_root: case.clone(),
            control_root: control,
            library_root: library.clone(),
            session_marker: live_skill().join("SKILL.md"),
        })
        .unwrap();
        assert!(isolated.isolation_verified, "{name}");
        let skill_md = launch_path(&library.join("SKILL.md"));
        let request = host.path().join("run.json");
        write(
            &request,
            &json!({
                "case_root": case,
                "codex_home": home,
                "user_home": user,
                "launcher": launcher,
                "prompt": prepared["setup"]["prompt"],
                "timeout": plan.budget.timeout_seconds,
                "extra_config": {
                    "skills.config": [{"path": skill_md, "enabled": true}]
                }
            }),
        );
        let ran = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["outcome-run", "--request"])
            .arg(&request)
            .arg("--run-model-probes")
            .current_dir(host.path())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&ran.stdout);
        let result: Value = serde_json::from_slice(&ran.stdout).unwrap_or_else(|_| {
            panic!(
                "{name} stdout={stdout} stderr={}",
                String::from_utf8_lossy(&ran.stderr)
            )
        });
        let execution =
            PathBuf::from(result["evidence_root"].as_str().unwrap()).join("native.json");
        let oracle_request = host.path().join("oracle.json");
        write(
            &oracle_request,
            &json!({
                "case_root": case,
                "setup": prepared["setup"],
                "execution": execution,
                "arm": if name == "candidate" { "candidate" } else { "baseline" }
            }),
        );
        let oracle = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["outcome-oracle", "--request"])
            .arg(&oracle_request)
            .current_dir(host.path())
            .output()
            .unwrap();
        let oracle_json: Value = serde_json::from_slice(&oracle.stdout)
            .unwrap_or_else(|_| json!({"status":"unreadable"}));
        arms.push(json!({
            "arm": name,
            "library_revision": isolated.library.revision,
            "run_status": result["status"],
            "elapsed_seconds": result["elapsed_seconds"],
            "usage": result["usage"],
            "oracle_passed": oracle_json["passed"],
            "oracle_exit": oracle.status.code(),
            "model": result["model"],
            "effort": result["effort"],
            "control_files_write": isolated.control_files_write,
            "control_create_child": isolated.control_create_child
        }));
        let _ = host.keep();
    }
    let live_after = package::load(&live_skill()).unwrap();
    assert_eq!(live_after.revision, live_before.revision);
    assert_eq!(arms.len(), 2);
    assert_ne!(arms[0]["library_revision"], arms[1]["library_revision"]);
    let summary = json!({
        "plan": plan,
        "cli_version_source": "HARNESS_NATIVE_CODEX",
        "live_skill_unchanged": true,
        "arms": arms,
        "unsupported_measurements": plan.unsupported_measurements
    });
    let report = std::env::temp_dir().join("skill-eval-pilot-first-pair.json");
    write(&report, &summary);
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_PILOT_NEXT=1; remaining 0.2 cases on user-authorized xai"]
fn shortening_remaining_cases_run_on_authorized_xai() {
    assert_eq!(
        std::env::var("HARNESS_SKILL_PILOT_NEXT").as_deref(),
        Ok("1"),
        "refusing to spend model pairs without HARNESS_SKILL_PILOT_NEXT=1"
    );
    std::env::var_os("HARNESS_NATIVE_CODEX").expect("HARNESS_NATIVE_CODEX required");
    let launcher = native_launcher();
    let live_before = package::load(&live_skill()).unwrap().revision;
    let timeout = plan::pilot().budget.timeout_seconds;
    let mut arms = Vec::new();
    for case_id in [
        plan::INTENDED_CASE,
        plan::BOUNDARY_CASE,
        plan::HELD_OUT_CASE,
    ] {
        let baseline = run_isolated_arm(case_id, "baseline", &live_skill(), &launcher, timeout);
        let candidate = run_isolated_arm(case_id, "candidate", &short_skill(), &launcher, timeout);
        assert_ne!(baseline["library_revision"], candidate["library_revision"]);
        arms.push(json!({"case_id": case_id, "baseline": baseline, "candidate": candidate}));
    }
    assert_eq!(package::load(&live_skill()).unwrap().revision, live_before);
    let summary = json!({
        "authorized_runner": "xai/grok-4.6",
        "batch": plan::shortening_batch(),
        "pairs": arms,
        "unsupported_measurements": plan::pilot().unsupported_measurements
    });
    write(
        &std::env::temp_dir().join("skill-eval-pilot-remaining-batch.json"),
        &summary,
    );
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
}

fn run_isolated_arm(
    case_id: &str,
    arm: &str,
    skill: &Path,
    launcher: &Path,
    timeout: u64,
) -> Value {
    let host = tempfile::Builder::new()
        .prefix(&format!("skill-eval-{case_id}-{arm}-"))
        .tempdir()
        .unwrap();
    let prepared = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-prepare", "--case", case_id])
        .current_dir(host.path())
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{case_id}/{arm} {}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    let case = PathBuf::from(prepared["case_root"].as_str().unwrap());
    let home = host.path().join("home");
    let user = host.path().join("user");
    let library = host.path().join("library");
    let control = host.path().join("control");
    fs::create_dir(&home).unwrap();
    fs::create_dir(&user).unwrap();
    fs::create_dir(&control).unwrap();
    copy_auth(&home);
    package::copy_into(skill, &library).unwrap();
    fs::write(
        control.join("oracle.json"),
        format!("{{\"case\":{case_id:?}}}"),
    )
    .unwrap();
    fs::write(
        control.join("baseline.json"),
        serde_json::to_vec(
            &json!({"arm": arm, "revision": package::load(&library).unwrap().revision}),
        )
        .unwrap(),
    )
    .unwrap();
    let isolated = isolation::isolate(&isolation::Request {
        source_root: workspace_root(),
        case_root: case.clone(),
        control_root: control,
        library_root: library.clone(),
        session_marker: live_skill().join("SKILL.md"),
    })
    .unwrap();
    assert!(isolated.isolation_verified, "{case_id}/{arm}");
    let skill_md = launch_path(&library.join("SKILL.md"));
    let request = host.path().join("run.json");
    write(
        &request,
        &json!({
            "case_root": case,
            "codex_home": home,
            "user_home": user,
            "launcher": launcher,
            "prompt": prepared["setup"]["prompt"],
            "timeout": timeout,
            "extra_config": {
                "skills.config": [{"path": skill_md, "enabled": true}]
            }
            ,
            "profile": "xai",
            "xai_auth": {
                "command": env!("CARGO_BIN_EXE_codex-harness"),
                "home": live_codex_home()
            }
        }),
    );
    let ran = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-run", "--request"])
        .arg(&request)
        .arg("--run-model-probes")
        .current_dir(host.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let result: Value = serde_json::from_slice(&ran.stdout).unwrap_or_else(|_| {
        panic!(
            "{case_id}/{arm} stdout={stdout} stderr={}",
            String::from_utf8_lossy(&ran.stderr)
        )
    });
    let execution = PathBuf::from(result["evidence_root"].as_str().unwrap()).join("native.json");
    let oracle_request = host.path().join("oracle.json");
    write(
        &oracle_request,
        &json!({
            "case_root": case,
            "setup": prepared["setup"],
            "execution": execution,
            "arm": if arm == "candidate" { "candidate" } else { "baseline" }
        }),
    );
    let oracle = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-oracle", "--request"])
        .arg(&oracle_request)
        .current_dir(host.path())
        .output()
        .unwrap();
    let oracle_json: Value =
        serde_json::from_slice(&oracle.stdout).unwrap_or_else(|_| json!({"status":"unreadable"}));
    let _ = host.keep();
    json!({
        "case_id": case_id,
        "arm": arm,
        "library_revision": isolated.library.revision,
        "run_status": result["status"],
        "elapsed_seconds": result["elapsed_seconds"],
        "usage": result["usage"],
        "oracle_passed": oracle_json["passed"],
        "oracle_exit": oracle.status.code(),
        "model": result["model"],
        "effort": result["effort"],
        "control_files_write": isolated.control_files_write
    })
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_PILOT_NEXT=1, and --run-model-probes; one pair only"]
fn shortening_negative_case_runs_as_the_next_episode_pair() {
    assert_eq!(
        std::env::var("HARNESS_SKILL_PILOT_NEXT").as_deref(),
        Ok("1"),
        "refusing to spend a model pair without HARNESS_SKILL_PILOT_NEXT=1"
    );
    std::env::var_os("HARNESS_NATIVE_CODEX").expect("HARNESS_NATIVE_CODEX required");
    let launcher = native_launcher();
    let live_before = package::load(&live_skill()).unwrap().revision;
    let timeout = plan::pilot().budget.timeout_seconds;
    let baseline = run_isolated_arm(
        plan::NEGATIVE_CASE,
        "baseline",
        &live_skill(),
        &launcher,
        timeout,
    );
    let candidate = run_isolated_arm(
        plan::NEGATIVE_CASE,
        "candidate",
        &short_skill(),
        &launcher,
        timeout,
    );
    assert_eq!(package::load(&live_skill()).unwrap().revision, live_before);
    assert_ne!(baseline["library_revision"], candidate["library_revision"]);
    let summary = json!({
        "case_id": plan::NEGATIVE_CASE,
        "batch": plan::shortening_batch(),
        "arms": [baseline, candidate],
        "verdict": skill_evolution::decision::decide(&plan::provider_limited_batch_evidence()),
        "unsupported_measurements": plan::pilot().unsupported_measurements
    });
    write(
        &std::env::temp_dir().join("skill-eval-pilot-negative-pair.json"),
        &summary,
    );
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
}
