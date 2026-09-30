//! Independent 7.4 add-vs-absence batch. Model runs stay opt-in.
#![cfg(windows)]
use serde_json::{Value, json};
use skill_evolution::{comparison, decision, isolation, package, plan};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[path = "fixtures/skill_consumption.rs"]
mod skill_consumption;
use skill_consumption::{
    ArmSummary, ArmVerdict, BatchFacts, CasePair, EvidenceVector, Role, frozen_batch_evidence,
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

fn role_for(case_id: &str) -> Role {
    match case_id {
        "process" => Role::ProtectedOverlapping,
        "negative" | "typo-fix" => Role::SimilarUnsuitable,
        "missing" => Role::BoundaryFailure,
        "freshness" | "cli-prior" => Role::IndependentHeldOut,
        _ => Role::Intended,
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
    if let Ok(bytes) = fs::read(&registration)
        && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        && let Some(path) = value["upstream"]["executable"].as_str()
    {
        let upstream = PathBuf::from(path);
        if upstream.is_file() {
            return upstream;
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

#[test]
fn protected_process_workflow_prepares_without_models() {
    let host = tempfile::Builder::new()
        .prefix("skill-eval-protected-prep-")
        .tempdir()
        .unwrap();
    let prepared = Command::new(env!("CARGO_BIN_EXE_codex-harness"))
        .args([
            "outcome-prepare",
            "--case",
            "process",
            "--observer",
            env!("CARGO_BIN_EXE_harness-observe"),
        ])
        .current_dir(host.path())
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let prepared: Value = serde_json::from_slice(&prepared.stdout).unwrap();
    assert_eq!(prepared["setup"]["case_id"], "process");
    assert_eq!(prepared["model_calls"], 0);
    assert!(
        prepared["setup"]["prompt"]
            .as_str()
            .unwrap()
            .contains("bounded capture and cleanup")
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
    let evidence_root = PathBuf::from(result["evidence_root"].as_str().unwrap());
    let frozen = skill_consumption::Frozen::new(
        isolated.library.clone(),
        vec![named.clone(), discovered.clone()],
        skill_consumption::live_skill_roots(),
    );
    let consumption = skill_consumption::collect_arm(
        comparison::Kind::AddAbsence,
        role_for(case_id),
        enable_skill,
        &frozen,
        &evidence_root,
        &oracle_json,
    );
    let _ = host.keep();
    json!({
        "case_id": case_id,
        "arm": arm,
        "skill_enabled": enable_skill,
        "library_revision": isolated.library.revision,
        "catalogue": {
            "name": isolated.library.name,
            "description": isolated.library.description,
            "revision": isolated.library.revision
        },
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
        "control_files_write": isolated.control_files_write,
        "consumption": consumption
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
    let evidence = frozen_batch_evidence(&batch_facts(&batch, &pairs, timeout));
    let verdict = decision::decide(&evidence);
    let summary = json!({
        "authorized_runner": "xai/grok-4.6",
        "batch": serde_json::to_value(&batch).unwrap(),
        "pairs": pairs,
        "evidence": serde_json::to_value(&evidence).unwrap(),
        "verdict": serde_json::to_value(verdict).unwrap(),
        "unsupported_measurements": plan::pilot().unsupported_measurements
    });
    write(
        &std::env::temp_dir().join("skill-eval-learning-cycle.json"),
        &summary,
    );
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
}

fn batch_facts(batch: &comparison::Plan, pairs: &[Value], timeout: u64) -> BatchFacts {
    let declared = skill_consumption::declared_workflows()
        .into_iter()
        .filter(|(name, _, _)| *name == "add-absence")
        .map(|(_, case_id, role)| (case_id.to_owned(), role))
        .collect();
    BatchFacts {
        kind: comparison::Kind::AddAbsence,
        model: batch.model.clone(),
        effort: batch.effort.clone(),
        timeout_seconds: timeout,
        declared,
        pairs: pairs
            .iter()
            .map(|pair| {
                let case_id = pair["case_id"].as_str().unwrap().to_owned();
                CasePair {
                    role: role_for(&case_id),
                    case_id,
                    baseline: summary(&pair["baseline"]),
                    candidate: summary(&pair["candidate"]),
                }
            })
            .collect(),
    }
}

fn summary(arm: &Value) -> ArmSummary {
    let model = arm["model"].as_str().unwrap_or_default().to_owned();
    let effort = arm["effort"].as_str().unwrap_or_default().to_owned();
    let checks = arm["oracle_checks"].clone();
    let total = arm["usage"]["totals"]["total_tokens"].as_u64();
    let cached = arm["usage"]["totals"]["cached_input_tokens"].as_u64();
    ArmSummary {
        run_status: arm["run_status"].as_str().unwrap_or_default().to_owned(),
        oracle_passed: arm["oracle_passed"].as_bool(),
        oracle_readable: !arm["oracle_exit"].is_null(),
        oracle_agreement: arm["consumption"]["oracle_cross_check"]
            .as_bool()
            .unwrap_or(false),
        verdict: arm["consumption"]["verdict"].as_str().map(verdict_from),
        live_unchanged: true,
        elapsed_seconds: arm["elapsed_seconds"]
            .as_f64()
            .map(|value| value.max(0.0) as u64),
        evidence: EvidenceVector {
            accepted_task_cost: total.map(|tokens| format!("reported {tokens} tokens this arm")),
            required_discovery: ["positive_activation", "negative_activation"]
                .iter()
                .any(|key| checks.get(key).and_then(Value::as_bool) == Some(true)),
            errors_detail_recovery: checks.is_object(),
            cache_basis: cached.map(|tokens| format!("within-arm cached_input_tokens={tokens}")),
            uncertainty: Some(format!(
                "single pair on {model}/{effort}; quota attribution unknown"
            )),
        },
        model,
        effort,
    }
}

fn verdict_from(value: &str) -> ArmVerdict {
    match value {
        "attributed" => ArmVerdict::Attributed,
        "missing_treatment" => ArmVerdict::MissingTreatment,
        "contaminated" => ArmVerdict::Contaminated,
        "drifted" => ArmVerdict::Drifted,
        "incomplete" => ArmVerdict::Incomplete,
        "absent" => ArmVerdict::Absent,
        "activated" => ArmVerdict::Activated,
        other => panic!("unexpected arm verdict {other}"),
    }
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
