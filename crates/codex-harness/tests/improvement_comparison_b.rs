//! Native dispatch, short real-operation and nuisance-control checks split
//! from `improvement_comparison`.
#![cfg(windows)]

#[path = "improvement_comparison_common.rs"]
mod improvement_comparison_common;
use improvement_comparison_common::*;

use harness_core::build_identity;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

/// One successful model-free measured pair through the ordinary control-backed
/// dispatch route: real visible dispatch, native frontend attachment, the
/// host's own lifecycle receipt, settlement from that receipt, independent
/// acceptance and a published Beads decision - no seeded success receipt.
#[test]
fn one_real_control_backed_pair_settles_through_native_observation() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("native-pair");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_real_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    // First resume: preparation plus the real baseline dispatch.
    let resume = fixture.resume_dispatched(&[]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let cursor = fixture.cursor();
    let baseline = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["role"] == "baseline")
        .cloned()
        .unwrap_or_else(|| {
            panic!("the visible owner accepted the baseline attempt: {cursor}\n{output}")
        });
    let generation = baseline["binding"]["generation"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("the accepted dispatch recorded its generation: {baseline}\n{output}")
        });
    assert_eq!(baseline["state"], "started", "{cursor}");
    let baseline_receipt = attempt_receipt(&fixture, "base-1");
    let record = wait_for_terminal_receipt(&baseline_receipt);
    assert_eq!(
        record["observation"]["state"], "completed",
        "the controlled conversation completed through the real host: {record}"
    );
    assert!(
        !record["observation"]["session"].is_null(),
        "the host recorded the native session: {record}"
    );

    // A stale generation is refused through the real receipt, then restored.
    let real_bytes = fs::read(&baseline_receipt).unwrap();
    let mut stale: Value = serde_json::from_slice(&real_bytes).unwrap();
    stale["originatingLead"]["runGeneration"] = json!("stale-generation");
    fs::write(
        &baseline_receipt,
        serde_json::to_vec_pretty(&stale).unwrap(),
    )
    .unwrap();
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let cursor = fixture.cursor();
    let baseline_attempt = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["id"] == "base-1")
        .unwrap();
    assert!(
        baseline_attempt["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("generation")),
        "a stale generation is refused from the real receipt: {cursor}"
    );
    assert_eq!(
        cursor["comparison"]["baseline"]["accepted"],
        Value::Null,
        "no comparison result is derived from the stale generation"
    );
    fs::write(&baseline_receipt, &real_bytes).unwrap();

    // The harness's own trusted-project addition to the workspace this dispatch
    // allocated is accepted, but every other change - including an unrelated
    // trusted workspace - still blocks the post-attempt consumption instead of
    // entering the comparison.
    let arm_config = fixture.arm_dir("baseline").join("home").join("config.toml");
    let served = fs::read_to_string(&arm_config).unwrap();
    assert!(
        served.contains("trust_level = \"trusted\""),
        "the real dispatch trusted its bound workspace: {served}"
    );
    // The trusted entry names exactly the workspace the accepted dispatch
    // recorded on its own attempt, so the narrowed rule has real authority to
    // check against.
    let recorded_checkout = fixture.cursor()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["role"] == "baseline")
        .and_then(|attempt| attempt["checkout"].as_str())
        .map(str::to_owned)
        .unwrap_or_default();
    assert!(
        served
            .to_ascii_lowercase()
            .contains(&recorded_checkout.to_ascii_lowercase()),
        "the arm configuration carries the workspace the accepted dispatch recorded ({recorded_checkout}): {served}"
    );
    let unrelated_trust =
        format!("{served}\n[projects.'c:\\unrelated-workspace']\ntrust_level = \"trusted\"\n");
    let changed_level = served.replace("trust_level = \"trusted\"", "trust_level = \"untrusted\"");
    let extra_setting = format!("{served}\n[fixture-drift]\nvalue = 1\n");
    let model_drift = served.replace("model = \"fixture-glyph-1\"", "model = \"fixture-glyph-2\"");
    for (name, mutated) in [
        ("an unrelated trusted-project workspace", unrelated_trust),
        ("a changed trust level", changed_level),
        ("an extra configuration key", extra_setting),
        ("model drift in the arm configuration", model_drift),
    ] {
        fs::write(&arm_config, &mutated).unwrap();
        let resume = fixture.resume();
        let output = text(&resume);
        assert!(resume.status.success(), "{name}: {output}");
        assert!(
            output.contains("the arm configuration changed since preparation"),
            "{name} still refuses consumption: {output}"
        );
        assert_eq!(
            fixture.cursor()["comparison"]["baseline"]["accepted"],
            Value::Null,
            "{name} cannot enter the comparison"
        );
    }
    fs::write(&arm_config, &served).unwrap();

    // Restoring the accepted generation settles the baseline from its own
    // receipt, runs the independent oracle, and dispatches the candidate.
    let resume = fixture.resume_dispatched(&[]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "the real baseline solution passed the frozen oracle: {status}\n{output}"
    );
    let candidate_receipt = attempt_receipt(&fixture, "cand-1");
    let record = wait_for_terminal_receipt(&candidate_receipt);
    assert_eq!(record["observation"]["state"], "completed", "{record}");

    // The candidate settles, is independently checked and the frozen policy
    // publishes its decision.
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    let comments =
        harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, &fixture.card)
            .unwrap();
    let records = harness_core::benefit_gate::parse_gate_comments(&comments);
    let assessment = harness_core::benefit_gate::assess(&records, &fixture.card)
        .expect("a decision is published");
    assert!(
        assessment
            .latest
            .revisions
            .as_deref()
            .is_some_and(|revisions| revisions == format!("{base}..{}", checkout.revision)),
        "the published decision carries the exact evaluated revisions: {comments:?}"
    );
    // Both arms settled from real host receipts, and each arm's committed
    // solution was verified independently; nothing was seeded.
    let cursor = fixture.cursor();
    for (id, _role) in [("base-1", "baseline"), ("cand-1", "candidate")] {
        let attempt = cursor["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|attempt| attempt["id"] == id)
            .unwrap();
        assert_eq!(attempt["state"], "completed", "{id}: {cursor}");
        assert!(
            attempt["retained"]["receipt_sha256"].is_string(),
            "the attempt settled from its own retained receipt: {cursor}"
        );
    }
    for arm in ["baseline", "candidate"] {
        let oracle: Value =
            serde_json::from_slice(&fs::read(fixture.arm_dir(arm).join("oracle.json")).unwrap())
                .unwrap();
        assert_eq!(oracle["executed"], true, "{arm}: {oracle}");
        assert_eq!(oracle["checker_executed"], true, "{arm}: {oracle}");
        assert_eq!(oracle["passed"], true, "{arm}: {oracle}");
        assert!(
            status["comparison"][arm]["revision"].is_string(),
            "{arm}: the verified revision is retained: {status}"
        );
    }
    assert!(!generation.is_empty());
}

/// An existing kit workload already carries its own executor declaration; the
/// native dispatch must select that configured profile, bind the qualified
/// local route under it and leave the frozen task tree unchanged.
#[test]
fn native_dispatch_uses_the_existing_executor_declaration() {
    let _serial = INSTALL.lock().unwrap();
    let declaration = "schema = 1\nlead_profile = \"default\"\nsuccessor_lead_profile = \"default\"\nexecutor_profiles = [\"workload-executor\"]\nmax_concurrent_executors = 1\nvote_threshold = 2\nincubator_size_cap = 32\nfeedback_batch_limit = 8\n";
    let fixture = Fixture::with_workload_declaration("declared", Some(declaration));
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_real_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    let resume = fixture.resume_dispatched(&[]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    let cursor = fixture.cursor();
    let baseline = cursor["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["role"] == "baseline")
        .cloned()
        .unwrap_or_else(|| {
            panic!("the task tree's own executor profile dispatched: {cursor}\n{output}")
        });
    assert_eq!(
        baseline["profile"], "workload-executor",
        "the dispatch records the task tree's own executor profile: {baseline}\n{output}"
    );
    assert_eq!(baseline["state"], "started", "{cursor}\n{output}");

    // The frozen declaration is consumed, never rewritten, and the prepared
    // arm home mirrors the qualified local route under that exact profile.
    let dispatch_checkout = fixture.arm_dir("baseline").join("checkout");
    let status = git_output(&dispatch_checkout, &["status", "--porcelain"]);
    assert_eq!(
        status.trim(),
        "",
        "the frozen task tree stays unchanged: {status}"
    );
    assert_eq!(
        fs::read_to_string(dispatch_checkout.join("global/orchestration.toml"))
            .unwrap()
            .replace("\r\n", "\n"),
        declaration,
        "the frozen executor declaration stays unchanged"
    );
    let config =
        fs::read_to_string(fixture.arm_dir("baseline").join("home").join("config.toml")).unwrap();
    let profile = config
        .split("[profiles.workload-executor]")
        .nth(1)
        .unwrap_or_else(|| panic!("the arm home binds the declared profile: {config}"));
    assert!(
        profile.contains("model = \"fixture-glyph-1\""),
        "the profile binds the qualified model: {config}"
    );
    assert!(
        profile.contains("model_provider = \"local\""),
        "the profile binds the qualified provider: {config}"
    );
    assert!(
        profile.contains("model_reasoning_effort = \"low\""),
        "the profile binds the qualified effort: {config}"
    );

    // The real conversation still completes through the host's own receipt.
    let receipt = attempt_receipt(&fixture, "base-1");
    let record = wait_for_terminal_receipt(&receipt);
    assert_eq!(record["observation"]["state"], "completed", "{record}");
}

/// Ordinary managed commands in each comparison arm consume that arm's
/// installation on the initial dispatch and on a later controller process,
/// even when both inherit a conflicting PATH. Unqualified names and explicit
/// `.exe` names are both executed. No model request is made.
#[test]
fn selected_arm_tools_resolve_on_dispatch_and_later_resume() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("arm-tools");
    let baseline_tool = compile_identity(&fixture.root, "baseline-id", "baseline");
    let candidate_tool = compile_identity(&fixture.root, "candidate-id", "candidate");
    let foreign_initial = compile_identity(&fixture.root, "foreign-initial-id", "foreign-initial");
    let foreign_resume = compile_identity(&fixture.root, "foreign-resume-id", "foreign-resume");
    let foreign_rtk = compile_identity(&fixture.root, "foreign-rtk-id", "foreign-rtk");
    let unrelated = compile_relative(
        &fixture.root,
        "unrelated-id",
        "unrelated",
        "marker-data.txt",
    );
    let foreign1 = fixture.root.join("foreign-initial");
    let foreign2 = fixture.root.join("foreign-resume");
    let unrelated_dir = fixture.root.join("unrelated-tools");
    for directory in [&foreign1, &foreign2, &unrelated_dir] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::copy(&foreign_initial, foreign1.join("codex-harness.exe")).unwrap();
    fs::copy(&foreign_rtk, foreign1.join("harness-rtk.exe")).unwrap();
    fs::copy(&foreign_resume, foreign2.join("codex-harness.exe")).unwrap();
    fs::copy(&foreign_rtk, foreign2.join("harness-rtk.exe")).unwrap();
    fs::copy(&unrelated, unrelated_dir.join("marker-tool.exe")).unwrap();
    fs::write(
        unrelated_dir.join("marker-data.txt"),
        "unrelated-sibling-sentinel",
    )
    .unwrap();

    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_identity_builds(
        &checkout,
        &fs::read(&baseline_tool).unwrap(),
        &fs::read(&candidate_tool).unwrap(),
    );
    fixture.start_with_ready_candidate(&checkout, false);

    let allow = fixture
        .root
        .to_str()
        .expect("fixture root is Unicode")
        .to_owned();
    let baseline_receipt = fixture.root.join("baseline-tools.json");
    let candidate_receipt = fixture.root.join("candidate-tools.json");
    write_tool_probe(&fixture.run, "baseline", &baseline_receipt, &allow);
    write_tool_probe(&fixture.run, "candidate", &candidate_receipt, &allow);
    let initial_path = prefixed_path(&[&foreign1, &unrelated_dir]);
    let resume = fixture.resume_in_process(&[("PATH", initial_path.as_str())]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("PowerShell 7."),
        "the dispatch prepared the owner shell: {output}"
    );
    assert!(
        baseline_receipt.is_file(),
        "the initial dispatch did not host the baseline tool probe: {}\n{output}",
        fs::read_to_string(fixture.run.join("tool-probes").join("baseline-trace.txt"))
            .unwrap_or_else(|error| format!("no trace: {error}"))
    );
    let baseline = tool_receipt(&baseline_receipt);
    assert_tool(
        &baseline,
        "codex-harness",
        "arm-tool-identity:baseline",
        "foreign-initial",
    );
    assert_tool(
        &baseline,
        "codex-harness.exe",
        "arm-tool-identity:baseline",
        "foreign-initial",
    );
    assert_tool(
        &baseline,
        "marker-tool",
        "arm-tool-identity:unrelated",
        "foreign-initial",
    );
    assert_relative_tool(&baseline, "marker-tool", "unrelated-sibling-sentinel");
    assert_shell(&baseline);

    let resume_path = prefixed_path(&[&foreign2, &unrelated_dir]);
    let resume = fixture.resume_in_process(&[("PATH", resume_path.as_str())]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        candidate_receipt.is_file(),
        "the later controller did not host the candidate tool probe: {output}"
    );
    let candidate = tool_receipt(&candidate_receipt);
    assert_tool(
        &candidate,
        "codex-harness",
        "arm-tool-identity:candidate",
        "foreign-resume",
    );
    assert_tool(
        &candidate,
        "codex-harness.exe",
        "arm-tool-identity:candidate",
        "foreign-resume",
    );
    assert_tool(
        &candidate,
        "marker-tool",
        "arm-tool-identity:unrelated",
        "foreign-resume",
    );
    assert_relative_tool(&candidate, "marker-tool", "unrelated-sibling-sentinel");
    assert_shell(&candidate);
    assert_ne!(
        baseline["pid"], candidate["pid"],
        "resume must be a later controller process: baseline {baseline} candidate {candidate}"
    );

    // A selected command removed after the hosted turn is drift. The next
    // resume must preserve that attempt and refuse a benefit decision.
    fs::remove_file(
        fixture
            .arm_dir("candidate")
            .join("home/harness/bin/codex-harness.exe"),
    )
    .unwrap();
    let drifted = fixture.resume_in_process(&[]);
    let output = text(&drifted);
    assert!(drifted.status.success(), "{output}");
    assert!(
        output.contains("no longer consumes its prepared runtime")
            || output.contains("changed since preparation")
            || output.contains("does not point at its prepared source"),
        "post-attempt command drift was accepted: {output}"
    );
    let status = fixture.status_json();
    assert_ne!(status["phase"], "decision-recorded", "{status}");
    assert_eq!(
        status["comparison"]["candidate"]["accepted"],
        Value::Null,
        "drift must not authorize a benefit decision: {status}"
    );
}

/// An advertised token-workflow component that the lifecycle owner cannot
/// prepare refuses readiness before a hosted turn. A foreign copy on PATH is
/// not consumed as a substitute.
#[test]
fn advertised_token_workflow_refuses_readiness_before_dispatch() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("arm-tools-unprepared");
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fs::write(
        fixture.proj.join("global/rtk.json"),
        br#"{"version":"0.48.0","executableSha256":"abc"}"#,
    )
    .unwrap();
    let foreign = fixture.root.join("foreign-rtk");
    fs::create_dir_all(&foreign).unwrap();
    let foreign_tool = compile_identity(&fixture.root, "foreign-rtk-tool", "foreign-rtk");
    fs::copy(&foreign_tool, foreign.join("harness-rtk.exe")).unwrap();
    fixture.start_with_ready_candidate(&checkout, false);
    let receipt = fixture.root.join("should-not-run.json");
    write_tool_probe(
        &fixture.run,
        "baseline",
        &receipt,
        fixture.root.to_str().unwrap(),
    );
    let path = prefixed_path(&[&foreign]);
    let resume = fixture.resume_in_process(&[("PATH", path.as_str())]);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("token-workflow") && output.contains("readiness is refused"),
        "an unprepared advertised component was treated as ready: {output}"
    );
    assert!(
        !receipt.is_file(),
        "readiness refusal still hosted a turn: {}",
        fs::read_to_string(&receipt).unwrap_or_default()
    );
    let status = fixture.status_json();
    assert_ne!(status["phase"], "decision-recorded", "{status}");
    assert!(
        status["comparison"]["baseline"]["accepted"] != true,
        "an unprepared arm authorized a benefit: {status}"
    );
}

/// Different prepared component inventories follow one recorded preparation
/// method, so the controller does not report a preparation-policy mismatch.
/// The component owner's installed-configuration edit remains a config-identity
/// mismatch. A genuinely different recorded method remains incomparable, and
/// independent acceptance still runs.
#[test]
fn different_component_inventories_share_one_preparation_method() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("component-method");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let mut checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    let vendor = b"staged-rtk-for-comparison-arm";
    let adapter = b"staged-adapter-for-comparison-arm";
    for (relative, bytes) in [
        (
            "crates/harness-rtk/Cargo.toml",
            &b"[package]\nname = \"harness-rtk\"\nversion = \"0.0.0\"\n"[..],
        ),
        ("crates/harness-rtk/src/main.rs", &b"fn main() {}\n"[..]),
    ] {
        let path = checkout.path.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
    }
    fs::write(
        checkout.path.join("global/rtk.json"),
        serde_json::to_vec(&json!({
            "version": "0.48.0",
            "executableSha256": build_identity::hash_bytes(vendor),
        }))
        .unwrap(),
    )
    .unwrap();
    git(&checkout.path, &["add", "."]);
    git(
        &checkout.path,
        &[
            "-c",
            "user.email=fixture@example.test",
            "-c",
            "user.name=Fixture",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "advertise optional token workflow",
        ],
    );
    checkout.revision = git_output(&checkout.path, &["rev-parse", "HEAD"]);
    let identity =
        harness_core::token_workflow_lifecycle::component_source_identity(&checkout.path)
            .expect("the candidate source advertises a preparable token-workflow component");
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_flag = stop.clone();
    let home = fixture.arm_dir("candidate").join("home");
    let staged_identity = identity.clone();
    let watcher = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        while !stop_flag.load(std::sync::atomic::Ordering::Relaxed)
            && std::time::Instant::now() < deadline
        {
            if home.is_dir() {
                stage_token_workflow_package(&home, &staged_identity, vendor, adapter);
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    });
    let installed = fixture.resume();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    watcher
        .join()
        .expect("the component staging watcher finishes");
    let output = text(&installed);
    assert!(installed.status.success(), "{output}");
    assert!(
        fixture.arm_dir("candidate").join("runtime.json").is_file()
            && fixture.arm_dir("baseline").join("runtime.json").is_file(),
        "both arms were not prepared: {output}"
    );

    let baseline_runtime: harness_core::improvement_runtime::ArmRuntime = serde_json::from_slice(
        &fs::read(fixture.arm_dir("baseline").join("runtime.json")).unwrap(),
    )
    .unwrap();
    let candidate_runtime: harness_core::improvement_runtime::ArmRuntime = serde_json::from_slice(
        &fs::read(fixture.arm_dir("candidate").join("runtime.json")).unwrap(),
    )
    .unwrap();
    assert!(
        baseline_runtime.components.is_empty(),
        "the baseline source advertises no optional component: {baseline_runtime:?}"
    );
    let component = candidate_runtime
        .components
        .iter()
        .find(|component| component.name == "token-workflow")
        .expect("the candidate treatment's optional component was prepared");
    assert_eq!(component.status, "Token workflow connected");
    assert!(!component.state_sha256.is_empty());
    for name in ["rtk.exe", "harness-rtk.exe"] {
        let link = component
            .links
            .iter()
            .find(|link| link.name == name)
            .unwrap_or_else(|| panic!("{name} was not retained"));
        assert_eq!(
            build_identity::hash_file(&link.source).unwrap(),
            link.sha256
        );
        assert!(!link.sha256.is_empty());
    }
    for arm in ["baseline", "candidate"] {
        let receipt = load_json(&fixture.arm_dir(arm).join("preparation-method.json"));
        assert_eq!(receipt["schema"], 1, "{arm} method receipt: {receipt}");
        assert_eq!(
            receipt["method"], "owner-install/v1",
            "{arm} method receipt: {receipt}"
        );
        let config = fs::read_to_string(fixture.arm_dir(arm).join("home/config.toml")).unwrap();
        assert!(
            config.contains("fixture-glyph-1"),
            "{arm} lost the declared model: {config}"
        );
    }

    settle_dispatched_pair(&fixture);
    let report = load_json(&fixture.run.join("comparison/report.json"));
    let reasons = comparison_reasons(&report);
    assert!(
        !reasons
            .iter()
            .any(|reason| reason.contains("preparation_policy")),
        "different component inventories were treated as a preparation-policy mismatch: {reasons:?}\n{report}"
    );
    // The component owner enables its features in the installed configuration.
    // That digest remains a matched field; this test must not bypass it.
    assert_eq!(
        reasons,
        vec!["mismatch:config_identity".to_owned()],
        "the shared method removed a gate other than preparation policy: {reasons:?}"
    );
    assert_eq!(report["comparisons"][0]["comparable"], false, "{reasons:?}");
    assert_ne!(
        baseline_runtime
            .configuration
            .as_ref()
            .map(|item| &item.sha256),
        candidate_runtime
            .configuration
            .as_ref()
            .map(|item| &item.sha256),
        "the config gate did not observe the component owner's feature edit"
    );
    let baseline_row = load_json(&fixture.arm_dir("baseline").join("row.json"));
    let candidate_row = load_json(&fixture.arm_dir("candidate").join("row.json"));
    assert_eq!(
        baseline_row["matched"]["preparation_policy"],
        "method:owner-install/v1"
    );
    assert_eq!(
        baseline_row["matched"]["preparation_policy"],
        candidate_row["matched"]["preparation_policy"]
    );
    assert_eq!(
        baseline_row["treatment"]["components"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        candidate_row["treatment"]["components"][0]["name"],
        "token-workflow"
    );
    let retained_links = candidate_row["treatment"]["components"][0]["links"]
        .as_array()
        .expect("the accounting row retains the prepared links");
    assert_eq!(retained_links.len(), component.links.len());
    for link in &component.links {
        assert!(
            retained_links.iter().any(|retained| {
                retained["name"] == link.name && retained["sha256"] == link.sha256
            }),
            "the accounting row dropped prepared link {}: {retained_links:?}",
            link.name
        );
    }
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}"
    );
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    let decision = status["comparison"]["decision"].as_str().unwrap_or("");
    assert!(
        !decision.contains("outcome=adopt"),
        "a config mismatch still authorized adoption: {status}"
    );
    assert_eq!(git_output(&fixture.proj, &["rev-parse", "HEAD"]), base);

    let mut changed = candidate_row.clone();
    changed["matched"]["preparation_policy"] = json!("method:owner-install/v0");
    let changed_report = harness_core::outcome_report::summarize_attempts(&[baseline_row, changed])
        .expect("the accounting owner still compares a changed method");
    let changed_reasons = comparison_reasons(&changed_report);
    assert!(
        changed_reasons
            .iter()
            .any(|reason| reason == "mismatch:preparation_policy"),
        "a changed preparation method stayed comparable: {changed_reasons:?}"
    );
    assert_eq!(changed_report["comparisons"][0]["comparable"], false);
}

/// An arm whose retained preparation method differs is incomparable even when
/// both inventories were prepared by the same controller and independently
/// accepted. The method receipt is not rewritten on resume.
#[test]
fn changed_preparation_method_remains_incomparable() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("changed-method");
    let base = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let installed = fixture.resume();
    assert!(installed.status.success(), "{}", text(&installed));
    let receipt = fixture.arm_dir("candidate").join("preparation-method.json");
    let mut method = load_json(&receipt);
    assert_eq!(method["method"], "owner-install/v1", "{method}");
    method["method"] = json!("owner-install/v0");
    fs::write(&receipt, serde_json::to_vec_pretty(&method).unwrap()).unwrap();
    settle_dispatched_pair(&fixture);
    let retained = load_json(&receipt);
    assert_eq!(
        retained["method"], "owner-install/v0",
        "resume rewrote the recorded preparation method: {retained}"
    );
    let report = load_json(&fixture.run.join("comparison/report.json"));
    let reasons = comparison_reasons(&report);
    assert!(
        reasons
            .iter()
            .any(|reason| reason == "mismatch:preparation_policy"),
        "a changed actual preparation method was comparable: {reasons:?}"
    );
    assert_eq!(report["comparisons"][0]["comparable"], false, "{reasons:?}");
    assert_eq!(
        load_json(&fixture.arm_dir("baseline").join("row.json"))["matched"]["preparation_policy"],
        "method:owner-install/v1"
    );
    assert_eq!(
        load_json(&fixture.arm_dir("candidate").join("row.json"))["matched"]["preparation_policy"],
        "method:owner-install/v0"
    );
    let status = fixture.status_json();
    assert_eq!(
        status["comparison"]["baseline"]["accepted"], true,
        "{status}"
    );
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], true,
        "{status}"
    );
    let decision = status["comparison"]["decision"].as_str().unwrap_or("");
    assert!(
        !decision.contains("outcome=adopt"),
        "an incomparable method still authorized adoption: {status}"
    );
    assert_eq!(git_output(&fixture.proj, &["rev-parse", "HEAD"]), base);
}

/// The full short path: the directed-measurement gate admits the run, both
/// arms execute the declared real operation (a genuine build/check cycle
/// with unchanged-input reuse, changed-input invalidation and a real built
/// output), the frozen checker independently accepts the baseline and fails
/// the candidate's wrong workload result, the decision is published and the
/// rejected candidate leaves the accepted baseline unchanged with every
/// artifact retained. No model conversation is opened anywhere.
#[test]
fn a_short_real_operation_pair_reaches_a_supported_rejection_and_restores_the_baseline() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, checkout) = short_operation_fixture(
        "short-operation",
        "solved",
        "broken",
        &["{workspace}", "{runtime}", "{target}"],
    );
    let head_before = git_output(&fixture.proj, &["rev-parse", "HEAD"]);
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let resumed = resume_short_operation(&fixture);
    let output = text(&resumed);
    assert!(resumed.status.success(), "{output}");
    assert!(
        output.contains("direct operation completed through the real heavy-command route"),
        "{output}"
    );
    // The directed initial-measurement gate admitted the run before any arm
    // ran, and its receipt binds the declared operation the arms executed.
    let measurement = load_json(&fixture.run.join("measurement-receipt.json"));
    assert_eq!(
        measurement["scope"]["workload"]["operation"],
        "cargo build --offline --manifest-path build/Cargo.toml",
        "{measurement}"
    );
    // The declared model qualification record is absent, yet both measured
    // arms ran: the direct-operation unit performs no model dispatch and
    // requires no unrelated model qualification.
    assert!(!fixture.root.join("absent-qualification.json").exists());

    // Both measured arms are real executions with genuine retained outputs.
    for (arm, expected_exit) in [("baseline", 0), ("candidate", 0)] {
        let receipt = load_json(&fixture.arm_dir(arm).join("operation-receipt.json"));
        assert_eq!(receipt["arm"], json!(arm), "{receipt}");
        assert_eq!(receipt["method"], "real-operation", "{receipt}");
        assert_eq!(
            receipt["workspace"].as_str().unwrap_or_default(),
            fixture
                .arm_dir(arm)
                .join("checkout")
                .to_string_lossy()
                .as_ref(),
            "{receipt}"
        );
        assert_eq!(receipt["status"], "exited", "{receipt}");
        assert_eq!(receipt["exit_code"], json!(expected_exit), "{receipt}");
        assert_eq!(receipt["model_calls"], 0, "{receipt}");
        assert_eq!(receipt["model_metrics"], "inapplicable", "{receipt}");
        assert!(
            receipt["elapsed_seconds"].as_f64().unwrap_or(0.0) > 0.0,
            "the execution time is the heavy owner's measured wall clock: {receipt}"
        );
        assert_eq!(
            receipt["program_sha256"].as_str().unwrap_or_default().len(),
            64,
            "{receipt}"
        );
        let stdout = receipt["admitted"]["streams"]["stdout"]["path"]
            .as_str()
            .expect("retained stdout");
        assert!(Path::new(stdout).is_file(), "{receipt}");
        let report = fs::read_to_string(
            fixture
                .arm_dir(arm)
                .join("operation-target")
                .join("report.json"),
        )
        .expect("the operation's own report is retained");
        assert!(report.contains("\"second_fresh\":1"), "{arm}: {report}");
        assert!(
            report.contains("\"first_compiled\":true"),
            "{arm}: {report}"
        );
        assert!(
            report.contains("\"third_compiled\":true"),
            "the changed input genuinely recompiled: {arm}: {report}"
        );
    }

    // Independent acceptance through the frozen oracle: the baseline's real
    // build/check result passes; the candidate's wrong workload result fails.
    let baseline_oracle = load_json(&fixture.arm_dir("baseline").join("oracle.json"));
    let candidate_oracle = load_json(&fixture.arm_dir("candidate").join("oracle.json"));
    assert_eq!(
        baseline_oracle["checker_executed"], true,
        "{baseline_oracle}"
    );
    assert_eq!(baseline_oracle["passed"], true, "{baseline_oracle}");
    assert_eq!(
        candidate_oracle["checker_executed"], true,
        "{candidate_oracle}"
    );
    assert_eq!(candidate_oracle["passed"], false, "{candidate_oracle}");
    // No duplicate feature implementation: the two arms executed the same
    // existing operation, no workload implementation was carried, and no
    // fabricated implementation record was written to either hypothesis card.
    for card in [&fixture.card, &fixture.workload_card] {
        let comments =
            harness_core::board_feedback::list_comments(&fixture.bd, &fixture.proj, card).unwrap();
        assert!(
            comments
                .iter()
                .all(|comment| !comment.starts_with("hypothesis-implementation")),
            "{card}: {comments:?}"
        );
    }

    // The comparison decided and the rejection was consumed: the accepted
    // baseline is unchanged, nothing was integrated or activated, and the
    // candidate, its failed acceptance and the measured scope stay retained.
    let status = fixture.status_json();
    assert_eq!(status["phase"], "idle", "{status}\n{output}");
    assert!(
        status["condition"]
            .as_str()
            .unwrap_or_default()
            .contains("leaves the baseline unchanged"),
        "{status}"
    );
    assert_eq!(
        status["comparison"]["candidate"]["accepted"], false,
        "{status}"
    );
    assert!(!fixture.run.join("integration.json").is_file());
    assert!(!fixture.run.join("activation.json").is_file());
    assert_eq!(
        git_output(&fixture.proj, &["rev-parse", "HEAD"]),
        head_before
    );
    assert!(
        checkout.path.is_dir(),
        "the candidate checkout is preserved"
    );
    let lineage = load_json(&fixture.run.join("lineage.json"));
    assert_eq!(lineage["decision"], "reject", "{lineage}");
    let reconcile = load_json(&fixture.run.join("reconcile.json"));
    assert_eq!(reconcile["outcome"], "reject", "{reconcile}");
    assert!(
        fixture.proj.join("openspec/changes/add-synthetic").is_dir(),
        "an unadopted change is never synchronized into the main specs"
    );
    assert!(fixture.run.join("comparison/decision.json").is_file());
    assert!(fixture.run.join("comparison/report.json").is_file());
    assert!(fixture.run.join("comparison/evaluation.json").is_file());

    // No conversation was opened and no model work ran for any phase.
    let cursor = fixture.cursor();
    assert!(
        cursor["attempts"].as_array().unwrap().is_empty(),
        "the short path requires no model conversation: {cursor}"
    );
    assert!(
        cursor["effects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|effect| effect["kind"].as_str().unwrap_or_default() != "dispatch-accepted"),
        "no conversation is ever dispatched on the direct-operation route: {cursor}"
    );
    assert!(
        cursor["effects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|effect| effect["kind"].as_str().unwrap_or_default() != "comparison-arm-refused"),
        "{cursor}"
    );
    let baseline_receipt =
        fs::read(fixture.arm_dir("baseline").join("operation-receipt.json")).unwrap();
    // A repeated resume reuses the retained executions and publishes nothing
    // new: unchanged input never replays a completed operation.
    let again = resume_short_operation(&fixture);
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(
        fs::read(fixture.arm_dir("baseline").join("operation-receipt.json")).unwrap(),
        baseline_receipt,
        "the retained baseline execution is reused, never replayed"
    );
    let effects = fixture.cursor()["effects"].as_array().unwrap().clone();
    let count = |kind: &str| {
        effects
            .iter()
            .filter(|effect| effect["kind"] == kind)
            .count()
    };
    assert_eq!(count("comparison-arm-accepted"), 2, "{effects:?}");
    assert_eq!(count("comparison-decision-recorded"), 1, "{effects:?}");
}

/// A settled arm is reused while all declared inputs match, and a rewritten
/// operation declaration refuses reuse with the exact changed field: the
/// retained execution is preserved and remeasurement is required rather than
/// silently reusing evidence that no longer binds the run's declaration.
#[test]
fn a_settled_short_operation_is_reused_only_while_its_declared_inputs_match() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, _checkout) = short_operation_fixture(
        "short-operation-reuse",
        "solved",
        "solved",
        &["{workspace}", "{runtime}", "{target}"],
    );
    // The reuse case is about retained executions, not about a margin: an
    // unreachable declared effect makes the final verdict independent of the
    // two operations' wall-clock noise while the accounting stays intact.
    let mut policy = load_json(&fixture.policy);
    policy["meaningfulEffectPercent"] = json!(1000.0);
    fs::write(&fixture.policy, serde_json::to_vec_pretty(&policy).unwrap()).unwrap();
    let first = resume_short_operation(&fixture);
    assert!(first.status.success(), "{}", text(&first));
    let baseline_path = fixture.arm_dir("baseline").join("operation-receipt.json");
    let baseline_receipt = fs::read(&baseline_path).unwrap();
    assert!(
        !fixture
            .arm_dir("candidate")
            .join("operation-receipt.json")
            .is_file(),
        "one arm is executed per advance"
    );

    // Unchanged inputs: the retained baseline execution is reused and only
    // the candidate's own operation runs.
    let second = resume_short_operation(&fixture);
    assert!(second.status.success(), "{}", text(&second));
    assert_eq!(
        fs::read(&baseline_path).unwrap(),
        baseline_receipt,
        "unchanged inputs reuse the retained execution"
    );
    assert!(
        fixture
            .arm_dir("candidate")
            .join("operation-receipt.json")
            .is_file()
    );

    // A rewritten declaration cannot reuse the completed candidate: the
    // exact changed field is named and the retained result is untouched.
    let declaration_path = fixture.run.join("operation.json");
    let original = fs::read(&declaration_path).unwrap();
    let mut declaration: Value = serde_json::from_slice(&original).unwrap();
    declaration["arguments"] = json!(operation_arguments(&["--redeclared"]));
    fs::write(
        &declaration_path,
        serde_json::to_vec_pretty(&declaration).unwrap(),
    )
    .unwrap();
    let candidate_path = fixture.arm_dir("candidate").join("operation-receipt.json");
    let candidate_receipt = fs::read(&candidate_path).unwrap();
    let third = resume_short_operation(&fixture);
    let output = text(&third);
    assert!(third.status.success(), "{output}");
    assert!(output.contains("cannot be reused"), "{output}");
    assert!(output.contains("arguments"), "{output}");
    assert_eq!(
        fs::read(&candidate_path).unwrap(),
        candidate_receipt,
        "the retained execution is preserved, never replayed"
    );
    assert!(
        !fixture
            .run
            .join("comparison")
            .join("decision.json")
            .is_file(),
        "changed conditions publish no decision"
    );

    // Restoring the exact declaration revalidates the retained executions
    // and the pair settles without remeasurement.
    fs::write(&declaration_path, &original).unwrap();
    let fourth = resume_short_operation(&fixture);
    assert!(fourth.status.success(), "{}", text(&fourth));
    assert!(fixture.run.join("comparison/decision.json").is_file());
    assert_eq!(fs::read(&baseline_path).unwrap(), baseline_receipt);
    assert_eq!(fs::read(&candidate_path).unwrap(), candidate_receipt);
    // Both arms passed their independent checks, so the model-free pair forms
    // a comparable unit on the operation's own measured work: the model
    // dimensions stay explicitly inapplicable (never a measured zero). Real
    // durations vary run to run, so which supported verdict the measured
    // difference reaches (adopt or reject against the declared threshold) is
    // not fixed here; the assertions below bind the record instead of a
    // timing value.
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    assert_eq!(
        decision["decision"], evaluation["decision"],
        "the published decision is the evaluated verdict: {decision}\n{evaluation}"
    );
    let verdict = evaluation["decision"].as_str().unwrap_or_default();
    assert!(
        verdict == "adopt" || verdict == "reject",
        "both real operations were independently accepted, so the verdict is supported, never inconclusive: {decision}\n{evaluation}"
    );
    assert_eq!(
        evaluation["acceptedTasks"], 2,
        "both real operations were independently accepted: {evaluation}"
    );
    // The model-free pair is a first-class measured unit: its comparability
    // rests on the declared identities and the operation's own recorded work,
    // and its model metrics stay inapplicable rather than a measured zero.
    assert_eq!(evaluation["matched"], 1, "{evaluation}");
    assert_eq!(
        evaluation["coverage"].as_str().unwrap_or_default(),
        "time+method:real-operation model-metrics:inapplicable operation-work:duration+exit+inputs; complete-pairs:1; variation:unmeasured",
        "the unmeasured model dimensions are visible limits, never measured zero: {evaluation}"
    );
    assert_eq!(
        evaluation["quality"], "unchanged",
        "independent acceptance held on both arms: {evaluation}"
    );
    // The model-free distinction is recorded whichever way the timing falls:
    // the operation's own work covers the non-time dimensions and the model
    // metrics stay inapplicable rather than becoming a measured zero.
    assert_eq!(
        evaluation["coverage"].as_str().unwrap_or_default(),
        "time+method:real-operation model-metrics:inapplicable operation-work:duration+exit+inputs; complete-pairs:1; variation:unmeasured",
        "the coverage names the real-operation method with inapplicable model metrics: {evaluation}"
    );
    assert!(
        evaluation["variation"]["basis"]
            .as_str()
            .unwrap_or_default()
            .contains(
                "the declared method executes no model call, so its model metrics are inapplicable rather than a measured zero"
            ),
        "the recorded variation basis carries the model-free distinction: {evaluation}"
    );
    // The verdict must follow the record's own measured work: an adoption
    // needs a positive reduction clearing the declared meaningful threshold;
    // a non-adoption either names the un-met threshold or a material time
    // regression beyond the declared tolerance.
    let reasons = evaluation["reasons"]
        .as_array()
        .expect("the evaluation records its reasons")
        .iter()
        .map(|reason| reason.as_str().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("; ");
    let baseline_seconds = evaluation["baselineSeconds"]
        .as_f64()
        .expect("the accepted pair records the baseline duration");
    let candidate_seconds = evaluation["candidateSeconds"]
        .as_f64()
        .expect("the accepted pair records the candidate duration");
    assert!(
        baseline_seconds > 0.0 && candidate_seconds > 0.0,
        "the pair records real measured durations: {evaluation}"
    );
    let policy = load_json(&fixture.policy);
    let meaningful = policy["meaningfulEffectPercent"]
        .as_f64()
        .expect("the fixture policy declares its meaningful effect");
    let tolerance = policy["tolerancePercent"]
        .as_f64()
        .expect("the fixture policy declares its tolerance");
    let effect_percent = (baseline_seconds - candidate_seconds) / baseline_seconds * 100.0;
    if verdict == "adopt" {
        assert!(
            effect_percent >= meaningful,
            "the adoption clears the declared meaningful effect on the recorded work: {evaluation}"
        );
        assert!(
            reasons.contains("model metrics are inapplicable"),
            "the adoption names the model-free distinction: {evaluation}"
        );
    } else if reasons.contains("materially regressed the primary time metric") {
        assert!(
            effect_percent < -tolerance,
            "the recorded material regression is beyond the declared tolerance: {evaluation}"
        );
    } else {
        assert!(
            reasons.contains("does not meet the predeclared meaningful threshold"),
            "the non-adoption states the measured limit precisely: {evaluation}"
        );
        assert!(
            effect_percent >= -tolerance && effect_percent < meaningful,
            "the non-adoption is grounded in the recorded work below the declared threshold: {evaluation}"
        );
    }
    let effects = fixture.cursor()["effects"].as_array().unwrap().clone();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| effect["kind"] == "comparison-arm-accepted")
            .count(),
        2,
        "exactly one executed settlement per arm: {:?}",
        fixture.cursor()
    );
}

/// A failed real operation is retained with its own exit status and streams,
/// the comparison stops without a decision, and a resumed pass never replays
/// the failed execution.
#[test]
fn a_failed_short_operation_is_retained_and_never_replayed() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, _checkout) = short_operation_fixture(
        "short-operation-failure",
        "solved",
        "solved",
        &["{workspace}", "{runtime}", "{target}", "--fail-build"],
    );
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let first = resume_short_operation(&fixture);
    let output = text(&first);
    assert!(first.status.success(), "{output}");
    assert!(output.contains("did not enter the comparison"), "{output}");

    let status = fixture.status_json();
    assert_eq!(status["phase"], "blocked", "{status}\n{output}");
    let condition = status["condition"].as_str().unwrap_or_default();
    assert!(condition.contains("exit_code"), "{condition}");
    assert!(
        status["comparison"]["baseline"]["condition"].is_string(),
        "{status}"
    );
    // The genuine build failure is retained with its own exit status and the
    // real Cargo failure output; no counter is simulated.
    let receipt_path = fixture.arm_dir("baseline").join("operation-receipt.json");
    let receipt = load_json(&receipt_path);
    assert_eq!(receipt["status"], "exited", "{receipt}");
    assert_ne!(receipt["exit_code"], 0, "{receipt}");
    assert_eq!(receipt["model_calls"], 0, "{receipt}");
    let stderr_path = receipt["admitted"]["streams"]["stderr"]["path"]
        .as_str()
        .expect("retained stderr");
    let stderr = fs::read_to_string(stderr_path).unwrap();
    assert!(
        stderr.contains("cargo build failed"),
        "the failing build's own output is retained: {stderr}"
    );
    assert!(!fixture.arm_dir("baseline").join("oracle.json").is_file());
    assert!(!fixture.run.join("comparison/decision.json").is_file());
    assert!(
        fixture.cursor()["attempts"].as_array().unwrap().is_empty(),
        "{}",
        fixture.cursor()
    );

    // A resumed pass re-reads the retained failure instead of replaying it.
    let receipt_bytes = fs::read(&receipt_path).unwrap();
    let second = resume_short_operation(&fixture);
    let output = text(&second);
    assert!(second.status.success(), "{output}");
    assert_eq!(status_json_phase(&fixture), "blocked");
    assert_eq!(
        fs::read(&receipt_path).unwrap(),
        receipt_bytes,
        "the failed execution is never replayed"
    );
    assert_eq!(
        fixture.cursor()["effects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|effect| effect["kind"] == "comparison-arm-accepted")
            .count(),
        0
    );
}

#[test]
fn a_warmer_second_arm_blocks_the_pair_until_the_owned_state_is_corrected() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, _checkout) = short_operation_fixture(
        "nuisance-warm-second-arm",
        "solved",
        "wrong",
        &["{workspace}", "{runtime}", "{target}"],
    );
    write_nuisance_policy(&fixture.policy, &fixture_nuisance_plan());
    // `continuous` drives the pair to its outcome or blocking condition in
    // one invocation, as the declared comparison expects.
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();

    let scratch = fixture
        .run
        .join("comparison")
        .join("candidate")
        .join("operation-target");
    // While the baseline arm is being measured, the second arm's owned
    // scratch receives inherited output - exactly the warmer-second-arm
    // condition the frozen plan must not absorb silently. The write happens
    // after the comparison is prepared (so it is not a partial preparation)
    // and before the candidate arm starts.
    let first = std::thread::scope(|scope| {
        let running = scope.spawn(|| resume_short_operation(&fixture));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(180);
        // The frozen bindings appear at the end of the model-free preparation,
        // while both arms are installed and long before the candidate arm
        // starts. Writing afterwards cannot be mistaken for a partial
        // preparation, and the candidate's own start check still runs later.
        let bindings = fixture.run.join("comparison").join("bindings.json");
        while !bindings.is_file() {
            assert!(
                std::time::Instant::now() < until,
                "the comparison was never prepared"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        fs::create_dir_all(&scratch).unwrap();
        fs::write(scratch.join("warm-leftover.bin"), b"inherited\n").unwrap();
        running.join().unwrap()
    });
    let output = text(&first);
    assert!(first.status.success(), "{output}");
    assert!(
        output.contains("would start from non-empty-owned-state"),
        "the warmed second arm is refused: {output}"
    );
    assert!(
        output.contains("no shared or unrelated state was cleared"),
        "{output}"
    );
    assert_eq!(
        fs::read(scratch.join("warm-leftover.bin")).unwrap(),
        b"inherited\n",
        "the inherited owned state is preserved, never cleared"
    );
    assert_eq!(status_json_phase(&fixture), "blocked");
    assert!(
        !fixture.run.join("comparison/decision.json").is_file(),
        "no decision is published from violated conditions"
    );
    let start = load_json(&fixture.run.join("comparison/candidate/nuisance-start.json"));
    assert_eq!(start["declared"], "owned-cold", "{start}");
    assert_eq!(start["violated"], true, "{start}");
    assert!(
        start["observed"]
            .as_str()
            .is_some_and(|observed| observed.contains("non-empty")),
        "{start}"
    );
    // The frozen plan was already bound before the arms: the declared plan,
    // its realized order and the unobserved shared state were recorded first.
    let receipt_path = fixture.run.join("comparison/nuisance-preflight.json");
    let receipt = load_json(&receipt_path);
    assert_eq!(receipt["realized_first"], "baseline-first", "{receipt}");
    assert_eq!(receipt["controller_order"], "baseline-first", "{receipt}");
    assert_eq!(receipt["shared_state"], "unobserved", "{receipt}");
    assert_eq!(receipt["plan"]["faults"], "observed-effect", "{receipt}");
    assert_eq!(receipt["plan"]["retries"], "policy-stopping", "{receipt}");
    assert!(
        receipt["resets"]
            .as_str()
            .is_some_and(|resets| resets.starts_with("none:")),
        "the receipt proves no state beyond owned observation is touched: {receipt}"
    );

    // Correcting the owned condition and resuming exercises the pair. The
    // recorded block is retried by the single-step controller, which
    // re-observes the corrected owned state before the arm starts.
    fs::remove_dir_all(&scratch).unwrap();
    fs::remove_file(fixture.run.join("supervision.json")).unwrap();
    for _ in 0..4 {
        if fixture.run.join("comparison/decision.json").is_file() {
            break;
        }
        let step = resume_short_operation(&fixture);
        assert!(step.status.success(), "{}", text(&step));
    }
    assert_eq!(status_json_phase(&fixture), "decision-recorded");
    let candidate_start = load_json(&fixture.run.join("comparison/candidate/nuisance-start.json"));
    assert_eq!(candidate_start["violated"], false, "{candidate_start}");

    // Both measured arms carry the frozen declaration, the observed initial
    // condition and the load disclosure into the report; the unobserved
    // shared state and the retained order exposure stay explicit in the
    // published evaluation.
    for arm in ["baseline", "candidate"] {
        let row = load_json(&fixture.arm_dir(arm).join("row.json"));
        assert_eq!(
            row["nuisance"]["order"]["realized"], "baseline-first",
            "{row}"
        );
        assert_eq!(
            row["nuisance"]["initial"]["observed"], "empty-or-absent-owned-state",
            "{row}"
        );
        assert_eq!(
            row["nuisance"]["load"]["observed"]["admission_evidence"], "unobserved",
            "{row}"
        );
        assert_eq!(row["nuisance"]["load"]["rule"], "recorded", "{row}");
    }
    let evaluation = load_json(&fixture.run.join("comparison/evaluation.json"));
    let coverage = evaluation["coverage"].as_str().unwrap_or_default();
    assert!(
        coverage.contains("nuisance-shared-unobserved"),
        "{evaluation}"
    );
    assert!(
        coverage.contains("nuisance-order-exposure-retained"),
        "{evaluation}"
    );

    // Recovery reuses the frozen receipt unchanged.
    let frozen = fs::read(&receipt_path).unwrap();
    let third = resume_short_operation(&fixture);
    assert!(third.status.success(), "{}", text(&third));
    assert_eq!(
        fs::read(&receipt_path).unwrap(),
        frozen,
        "the nuisance-control receipt is reused unchanged on resume"
    );
}

#[test]
fn an_arm_order_this_controller_cannot_realize_is_refused_before_results() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, _checkout) = short_operation_fixture(
        "nuisance-unrealizable-order",
        "solved",
        "solved",
        &["{workspace}", "{runtime}", "{target}"],
    );
    // The predeclared seed realizes the candidate first. This sequential
    // controller runs the baseline first and refuses to silently substitute.
    let mut plan = fixture_nuisance_plan();
    plan.order = harness_core::improvement_policy::OrderRule::Randomized;
    plan.seed = Some(1);
    write_nuisance_policy(&fixture.policy, &plan);
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();

    let resume = resume_short_operation(&fixture);
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(
        output.contains("realizes candidate-first")
            && output.contains("will not silently substitute a different order"),
        "{output}"
    );
    assert_eq!(status_json_phase(&fixture), "blocked");
    assert!(
        !fixture
            .run
            .join("comparison/nuisance-preflight.json")
            .is_file(),
        "an unrealizable order is not frozen into a receipt"
    );
    assert!(
        !fixture.arm_dir("baseline").join("row.json").is_file()
            && !fixture.arm_dir("candidate").join("row.json").is_file(),
        "no arm is measured under an unrealized order"
    );
}

#[test]
fn a_failed_measured_execution_is_classified_and_follows_the_frozen_retry_rule() {
    let _serial = INSTALL.lock().unwrap();
    let (fixture, _checkout) = short_operation_fixture(
        "nuisance-execution-failure",
        "solved",
        "solved",
        &["{workspace}", "{runtime}", "{target}", "--fail-build"],
    );
    write_nuisance_policy(&fixture.policy, &fixture_nuisance_plan());
    fs::write(
        fixture.run.join("supervision.json"),
        r#"{"schema":1,"mode":"continuous"}"#,
    )
    .unwrap();
    let first = resume_short_operation(&fixture);
    let output = text(&first);
    assert!(first.status.success(), "{output}");
    assert!(output.contains("did not enter the comparison"), "{output}");

    // The fault is classified by its observed effect: the operation started
    // and failed, so it is not an eligible transport-only idle wait, and the
    // frozen retry rule records that it is not replayed.
    let fault_path = fixture.arm_dir("baseline").join("fault.json");
    let fault = load_json(&fault_path);
    assert_eq!(fault["class"], "execution-failure", "{fault}");
    assert_ne!(fault["class"], "transport-only-refusal", "{fault}");
    assert_eq!(fault["retry"]["rule"], "policy-stopping", "{fault}");
    assert_eq!(fault["retry"]["max_attempts_per_arm"], 1, "{fault}");
    assert_eq!(fault["retry"]["replayed"], false, "{fault}");

    // The genuine failure is retained with its own exit status and never
    // replayed; no row and no comparison decision are fabricated from it.
    let receipt_path = fixture.arm_dir("baseline").join("operation-receipt.json");
    let receipt_bytes = fs::read(&receipt_path).unwrap();
    let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
    assert_eq!(receipt["status"], "exited", "{receipt}");
    assert_ne!(receipt["exit_code"], 0, "{receipt}");
    let start = load_json(&fixture.run.join("comparison/baseline/nuisance-start.json"));
    assert_eq!(start["violated"], false, "{start}");
    assert!(!fixture.arm_dir("baseline").join("row.json").is_file());
    assert!(!fixture.run.join("comparison/decision.json").is_file());

    let second = resume_short_operation(&fixture);
    assert!(second.status.success(), "{}", text(&second));
    assert_eq!(
        fs::read(&receipt_path).unwrap(),
        receipt_bytes,
        "the failed execution is never replayed"
    );
    assert_eq!(
        load_json(&fault_path)["class"],
        "execution-failure",
        "the classification stays bound to the frozen plan"
    );
}

/// The background-contention counterexample: an unrelated owned heavy command
/// occupies the isolated account slot while the baseline arm runs. The
/// recorded contention stays visible on that arm, no utilization-based time
/// correction is invented for it, the retained attempts keep their raw and
/// admitted evidence, and the unsupported causal claim stays inconclusive
/// instead of publishing a false gain.
#[test]
fn recorded_background_contention_withholds_the_unsupported_verdict() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("nuisance-contention");
    write_nuisance_policy(&fixture.policy, &fixture_nuisance_plan());
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_real_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);

    // One unrelated owned holder occupies the only slot of the isolated
    // account the measured arms use.
    let account = fixture.root.join("isolated-heavy-account");
    harness_core::heavy_command::prepare(&account).unwrap();
    let mut budget = harness_core::heavy_command::Budget::read(&account).unwrap();
    budget.max_concurrent_trees = 1;
    harness_core::heavy_command::Budget::write(&account, &budget).unwrap();
    let started = fixture.root.join("holder-started.txt");
    let launch = env!("CARGO_BIN_EXE_harness-launch-fixture");
    let mut holder = Command::new(env!("CARGO_BIN_EXE_codex-harness"));
    holder
        .arg("heavy")
        .arg("--account")
        .arg(&account)
        .arg("--attempt")
        .arg("holder-other")
        .arg("--")
        .arg(launch)
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "180000")
        .env("HARNESS_HEAVY_FIXTURE_STARTED", &started);
    let mut holder = holder.spawn().expect("isolated holder starts");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !started.exists() {
        assert!(
            std::time::Instant::now() < until,
            "holder did not acquire the isolated slot"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // The baseline arm is dispatched while the unrelated holder blocks the
    // shared slot; the holder is released after the arm reached its heavy
    // command, exactly as the existing owned-contention check does.
    let account_text = account.display().to_string();
    let marker = fixture.root.join("heavy-started.txt");
    let marker_text = marker.display().to_string();
    let release = std::thread::spawn({
        let marker = marker.clone();
        move || {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(240);
            while !marker.exists() && std::time::Instant::now() < until {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            if marker.exists() {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            let _ = holder.kill();
            let _ = holder.wait();
        }
    });
    let mut command = Command::new(manager());
    command
        .arg("improve")
        .args(["resume", "--run", fixture.run.to_str().unwrap()])
        .env_remove("HARNESS_EXECUTOR_SESSION")
        .env_remove("HARNESS_EXECUTOR_FIXTURE_MODE")
        .env_remove("HARNESS_EXECUTOR_CHILD_FIXTURE_MODE")
        .env_remove("HARNESS_EXECUTOR_RUN")
        .env_remove("HARNESS_ORIGINATING_LEAD")
        .env_remove("HARNESS_LEAD_THREAD")
        .env_remove("HARNESS_LEAD_RECIPIENT")
        .env_remove("WT_SESSION")
        .env(CONTROL_CHILD_MODE.0, CONTROL_CHILD_MODE.1)
        .env("HARNESS_IMPROVEMENT_FIXTURE_HEAVY", "1")
        .env("CODEX_HARNESS_HEAVY_ACCOUNT", &account_text)
        .env("HARNESS_IMPROVEMENT_FIXTURE_HEAVY_MARKER", &marker_text)
        .env("HARNESS_IMPROVEMENT_FIXTURE_HEAVY_PROGRAM", launch)
        .env(
            "HARNESS_IMPROVEMENT_FIXTURE_HEAVY_CLI",
            env!("CARGO_BIN_EXE_codex-harness"),
        )
        .env("HARNESS_LAUNCH_FIXTURE_MODE", "heavy-hold")
        .env("HARNESS_HEAVY_FIXTURE_MS", "1000");
    let resume = command.output().expect("baseline resume");
    let output = text(&resume);
    assert!(
        marker.exists(),
        "the fixture did not reach the owned heavy command: {output}"
    );
    assert!(resume.status.success(), "{output}");
    let _ = release.join();
    let receipt = attempt_receipt(&fixture, "base-1");
    let record = wait_for_terminal_receipt(&receipt);
    assert_eq!(
        record["observation"]["state"], "completed",
        "the visible fixture conversation did not complete: {record}"
    );

    // The baseline settles, then the candidate arm is dispatched without the
    // holder (the account slot is free again) and settles too.
    let settle = fixture.resume_in_process(&[]);
    assert!(settle.status.success(), "settlement: {}", text(&settle));
    let candidate_receipt = attempt_receipt(&fixture, "cand-1");
    let candidate_record = wait_for_terminal_receipt(&candidate_receipt);
    assert_eq!(
        candidate_record["observation"]["state"], "completed",
        "{candidate_record}"
    );
    let settle = fixture.resume_in_process(&[]);
    assert!(settle.status.success(), "decision: {}", text(&settle));
    let status = fixture.status_json();
    assert_eq!(status["phase"], "decision-recorded", "{status}");

    // The baseline arm's own row records the observed unrelated contention;
    // the candidate arm's row records none, and neither is repaired.
    let baseline = load_json(&fixture.arm_dir("baseline").join("row.json"));
    let candidate = load_json(&fixture.arm_dir("candidate").join("row.json"));
    assert_eq!(
        baseline["nuisance"]["load"]["observed"]["unrelated_wait"], true,
        "{baseline}"
    );
    assert_eq!(
        baseline["nuisance"]["load"]["observed"]["admission_evidence"], "recorded",
        "{baseline}"
    );
    assert_eq!(
        candidate["nuisance"]["load"]["observed"]["unrelated_wait"], false,
        "{candidate}"
    );
    // The raw observed time and the admitted evidence stay retained.
    let report = load_json(&fixture.run.join("comparison/report.json"));
    assert!(
        report["attempts"][0]["infrastructure"]["observed_seconds"]
            .as_f64()
            .is_some_and(|seconds| seconds > 0.0),
        "{report}"
    );

    // The asymmetric contention could explain the observed difference without
    // any declared work-efficiency binding: the causal claim is withheld and
    // named, rather than corrected by an invented utilization factor.
    let evaluation: Value = load_json(&fixture.run.join("comparison/evaluation.json"));
    assert_eq!(evaluation["decision"], "inconclusive", "{evaluation}");
    assert!(
        evaluation["reasons"]
            .as_array()
            .is_some_and(|reasons| reasons.iter().any(|reason| reason
                .as_str()
                .is_some_and(|reason| reason.contains(
                    "unrelated external heavy command was measured over exactly one arm"
                )))),
        "{evaluation}"
    );
    let decision = load_json(&fixture.run.join("comparison/decision.json"));
    assert_eq!(decision["decision"], "inconclusive", "{decision}");
}

/// A refusal that happened after the measured work started is classified by
/// its observed effect: the changed or unverifiable response cannot be
/// repaired by subtracting its wall duration, the original attempt stays
/// retained, and the frozen retry rule records that it is never replayed.
#[test]
fn an_unverifiable_measured_response_is_classified_and_not_repaired() {
    let _serial = INSTALL.lock().unwrap();
    let fixture = Fixture::new("nuisance-unverified-response");
    write_nuisance_policy(&fixture.policy, &fixture_nuisance_plan());
    let checkout = fixture.prepare_candidate(Some("// candidate implementation\n"));
    fixture.prepare_builds(&checkout);
    fixture.start_with_ready_candidate(&checkout, false);
    let resume = fixture.resume();
    assert!(resume.status.success(), "{}", text(&resume));
    // The conversation ran and produced a response, but the observed model
    // facts do not match the declared client: the trajectory is unverifiable,
    // so the arm is refused instead of entering the comparison.
    let session = session_id("nuisance-unverified-session");
    fixture.simulate_arm("baseline", "baseline", "solved", 50, 5.0, 2, 3, &session);
    rewrite_rollout(&fixture, "baseline", &session, "another-model", "low");
    let resume = fixture.resume();
    let output = text(&resume);
    assert!(resume.status.success(), "{output}");
    assert!(output.contains("recorded model another-model"), "{output}");

    let fault_path = fixture.arm_dir("baseline").join("fault.json");
    let fault = load_json(&fault_path);
    assert_eq!(fault["class"], "work-started-unverified", "{fault}");
    assert_ne!(fault["class"], "transport-only-refusal", "{fault}");
    assert_eq!(fault["retry"]["rule"], "policy-stopping", "{fault}");
    assert_eq!(fault["retry"]["replayed"], false, "{fault}");
    // The attempt and its refusal stay retained; no comparison row, oracle
    // result or decision is fabricated from the unverifiable trajectory.
    assert_eq!(
        fixture.cursor()["comparison"]["baseline"]["accepted"],
        Value::Null
    );
    assert!(!fixture.arm_dir("baseline").join("row.json").is_file());
    assert!(!fixture.run.join("comparison/decision.json").is_file());

    // A resumed pass re-reads the retained refusal instead of replaying the
    // arm or repairing its trajectory.
    let second = fixture.resume();
    assert!(second.status.success(), "{}", text(&second));
    assert_eq!(
        load_json(&fault_path)["class"],
        "work-started-unverified",
        "the classification stays bound to the frozen plan"
    );
}
