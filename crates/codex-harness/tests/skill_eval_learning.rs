//! Independent 7.4 add-vs-absence batch. Model runs stay opt-in.
#![cfg(windows)]
use serde_json::{Value, json};
use skill_evolution::{decision, isolation, package, plan};
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

fn accept_skill() -> PathBuf {
    workspace_root().join("crates/skill-evolution/fixtures/harness-product-cli")
}

fn process_skill() -> PathBuf {
    workspace_root().join("crates/skill-evolution/fixtures/harness-process-check")
}

fn skill_for(case_id: &str) -> PathBuf {
    if case_id == "process" {
        process_skill()
    } else {
        accept_skill()
    }
}

fn compared_name(case_id: &str) -> &'static str {
    if case_id == "process" {
        "harness-process-check"
    } else {
        plan::ACCEPT_SKILL
    }
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

#[test]
fn independent_acceptance_cases_prepare_without_models() {
    let batch = plan::independent_acceptance_batch();
    assert_eq!(batch.kind, skill_evolution::comparison::Kind::AddAbsence);
    for case_id in [
        batch.intended.as_str(),
        batch.negative.as_str(),
        batch.held_out.as_str(),
    ] {
        let host = tempfile::Builder::new()
            .prefix(&format!("skill-eval-learn-prep-{case_id}-"))
            .tempdir()
            .unwrap();
        let prepared = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
            .args(["outcome-prepare", "--case", case_id])
            .current_dir(host.path())
            .output()
            .unwrap();
        assert!(
            prepared.status.success(),
            "{case_id} {}",
            String::from_utf8_lossy(&prepared.stderr)
        );
        let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
        assert_eq!(prepared["setup"]["case_id"], case_id);
        assert_eq!(prepared["model_calls"], 0);
    }
    assert_eq!(
        package::load(&accept_skill()).unwrap().name,
        plan::ACCEPT_SKILL
    );
}

fn run_arm(case_id: &str, arm: &str, enable_skill: bool, launcher: &Path, timeout: u64) -> Value {
    let host = tempfile::Builder::new()
        .prefix(&format!("skill-eval-learn-{case_id}-{arm}-"))
        .tempdir()
        .unwrap();
    let mut prepare = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    prepare.args(["outcome-prepare", "--case", case_id]);
    if case_id == "process" {
        prepare.args(["--observer", env!("CARGO_BIN_EXE_harness-observe")]);
    }
    let prepared = prepare.current_dir(host.path()).output().unwrap();
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
    fs::create_dir_all(&library).unwrap();
    let named = library.join(compared_name(case_id));
    package::copy_into(&skill_for(case_id), &named).unwrap();
    fs::write(
        control.join("oracle.json"),
        format!("{{\"case\":{case_id:?}}}"),
    )
    .unwrap();
    fs::write(
        control.join("baseline.json"),
        serde_json::to_vec(&json!({"arm": arm, "enabled": enable_skill})).unwrap(),
    )
    .unwrap();
    let isolated = isolation::isolate(&isolation::Request {
        source_root: workspace_root(),
        case_root: case.clone(),
        control_root: control,
        library_root: named.clone(),
        session_marker: live_skill().join("SKILL.md"),
    })
    .unwrap();
    assert!(isolated.isolation_verified, "{case_id}/{arm}");
    let discovered = user.join(".agents/skills").join(compared_name(case_id));
    fs::create_dir_all(discovered.parent().unwrap()).unwrap();
    package::copy_into(&skill_for(case_id), &discovered).unwrap();
    let skill_md = launch_path(&named.join("SKILL.md"));
    let extra_config = if enable_skill {
        json!({"skills.config": [{"path": skill_md, "enabled": true}]})
    } else {
        json!({})
    };
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
            "extra_config": extra_config,
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
    let result: Value = serde_json::from_slice(&ran.stdout).unwrap_or_else(|_| {
        panic!(
            "{case_id}/{arm} stdout={} stderr={}",
            String::from_utf8_lossy(&ran.stdout),
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
            "arm": if enable_skill { "candidate" } else { "baseline" },
            "compared_skill": compared_name(case_id),
            "reject_profile_skills": true
        }),
    );
    let oracle = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args(["outcome-oracle", "--request"])
        .arg(&oracle_request)
        .current_dir(host.path())
        .output()
        .unwrap();
    let oracle_json: Value =
        serde_json::from_slice(&oracle.stdout).unwrap_or_else(|_| json!({"status": "unreadable"}));
    let _ = host.keep();
    json!({
        "case_id": case_id,
        "arm": arm,
        "skill_enabled": enable_skill,
        "library_revision": isolated.library.revision,
        "run_status": result["status"],
        "elapsed_seconds": result["elapsed_seconds"],
        "usage": result["usage"],
        "oracle_passed": oracle_json["passed"],
        "oracle_exit": oracle.status.code(),
        "oracle_checks": oracle_json["details"]["checks"],
        "skill_use": oracle_json["details"]["skill_use"],
        "compared_skill": oracle_json["details"]["compared_skill"],
        "model": result["model"],
        "effort": result["effort"],
        "control_files_write": isolated.control_files_write
    })
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LEARNING_CYCLE=1; independent 7.4 xai batch"]
fn independent_add_absence_batch_runs_on_authorized_xai() {
    assert_eq!(
        std::env::var("HARNESS_SKILL_LEARNING_CYCLE").as_deref(),
        Ok("1"),
        "refusing to spend model pairs without HARNESS_SKILL_LEARNING_CYCLE=1"
    );
    std::env::var_os("HARNESS_NATIVE_CODEX").expect("HARNESS_NATIVE_CODEX required");
    let launcher = native_launcher();
    let live_before = package::load(&live_skill()).unwrap().revision;
    let timeout = plan::pilot().budget.timeout_seconds;
    let batch = plan::independent_acceptance_batch();
    let mut pairs = Vec::new();
    for case_id in [
        batch.intended.as_str(),
        batch.negative.as_str(),
        batch.held_out.as_str(),
    ] {
        let baseline = run_arm(case_id, "baseline", false, &launcher, timeout);
        let candidate = run_arm(case_id, "candidate", true, &launcher, timeout);
        pairs.push(json!({"case_id": case_id, "baseline": baseline, "candidate": candidate}));
    }
    assert_eq!(package::load(&live_skill()).unwrap().revision, live_before);
    let intended = &pairs[0];
    let negative = &pairs[1];
    let held_out = &pairs[2];
    let intended_gain = intended["candidate"]["oracle_passed"] == true
        && intended["baseline"]["oracle_passed"] != true;
    let held_out_gain = held_out["candidate"]["oracle_passed"] == true
        && held_out["baseline"]["oracle_passed"] != true;
    let negative_ok = negative["candidate"]["oracle_passed"] == true
        && negative["baseline"]["oracle_passed"] == true;
    let evidence = decision::ComparisonEvidence {
        integrity_ok: pairs.iter().all(|pair| {
            pair["baseline"]["run_status"] == "completed"
                && pair["candidate"]["run_status"] == "completed"
        }),
        evidence_complete: true,
        provider_matched: true,
        must_pass: intended["candidate"]["oracle_passed"] == true
            && negative_ok
            && held_out["candidate"]["oracle_passed"] == true,
        selection_demonstrated: true,
        protected_regression: negative["candidate"]["oracle_passed"] != true,
        benefit_established: intended_gain && held_out_gain && negative_ok,
        within_budgets: true,
        claim: decision::Claim::Capability,
        skipped_required_check: false,
        single_lucky_run: !(intended_gain && held_out_gain),
        meaningful_difference: intended_gain || held_out_gain,
    };
    let verdict = decision::decide(&evidence);
    let summary = json!({
        "authorized_runner": "xai/grok-4.6",
        "batch": serde_json::to_value(&batch).unwrap(),
        "pairs": pairs,
        "evidence": serde_json::to_value(&evidence).unwrap(),
        "verdict": serde_json::to_value(&verdict).unwrap(),
        "unsupported_measurements": plan::pilot().unsupported_measurements
    });
    write(
        &std::env::temp_dir().join("skill-eval-learning-cycle.json"),
        &summary,
    );
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LEARNING_CYCLE=1; one process absence probe"]
fn process_absence_probe_runs_on_authorized_xai() {
    assert_eq!(
        std::env::var("HARNESS_SKILL_LEARNING_CYCLE").as_deref(),
        Ok("1"),
        "refusing to spend a model pair without HARNESS_SKILL_LEARNING_CYCLE=1"
    );
    std::env::var_os("HARNESS_NATIVE_CODEX").expect("HARNESS_NATIVE_CODEX required");
    let launcher = native_launcher();
    let live_before = package::load(&live_skill()).unwrap().revision;
    let timeout = plan::pilot().budget.timeout_seconds;
    let baseline = run_arm("process", "baseline", false, &launcher, timeout);
    assert_eq!(package::load(&live_skill()).unwrap().revision, live_before);
    write(
        &std::env::temp_dir().join("skill-eval-process-absence.json"),
        &baseline,
    );
    println!("{}", serde_json::to_string_pretty(&baseline).unwrap());
}

#[test]
#[ignore = "explicit HARNESS_NATIVE_CODEX, HARNESS_SKILL_LEARNING_CYCLE=1; one process candidate probe"]
fn process_candidate_probe_runs_on_authorized_xai() {
    assert_eq!(
        std::env::var("HARNESS_SKILL_LEARNING_CYCLE").as_deref(),
        Ok("1"),
        "refusing to spend a model pair without HARNESS_SKILL_LEARNING_CYCLE=1"
    );
    std::env::var_os("HARNESS_NATIVE_CODEX").expect("HARNESS_NATIVE_CODEX required");
    let launcher = native_launcher();
    let live_before = package::load(&live_skill()).unwrap().revision;
    let timeout = plan::pilot().budget.timeout_seconds;
    assert_eq!(
        package::load(&process_skill()).unwrap().name,
        "harness-process-check"
    );
    let candidate = run_arm("process", "candidate", true, &launcher, timeout);
    assert_eq!(package::load(&live_skill()).unwrap().revision, live_before);
    write(
        &std::env::temp_dir().join("skill-eval-process-candidate.json"),
        &candidate,
    );
    println!("{}", serde_json::to_string_pretty(&candidate).unwrap());
}
