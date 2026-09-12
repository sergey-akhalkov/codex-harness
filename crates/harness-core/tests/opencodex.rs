//! Explicit installed-dependency acceptance; no models, network login or services.
#![cfg(windows)]
use harness_core::{
    opencodex::{
        ProbeStatus, ValidationStatus, login_xai_closed_stdin, restore_native, validate_candidate,
    },
    process::{Cancellation, Deadline},
};
use std::{path::PathBuf, time::Duration};

fn package() -> PathBuf {
    PathBuf::from(std::env::var_os("HARNESS_OPENCODEX_PACKAGE").expect("explicit package root"))
}

#[test]
#[ignore = "requires explicit HARNESS_OPENCODEX_PACKAGE for the adopted foreign package"]
fn installed_cli_validates_owned_candidates_and_preserves_source() {
    let package = package();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let valid = std::fs::read(repo.join("global/opencodex/config.json")).unwrap();
    let before =
        harness_core::build_identity::hash_file(&repo.join("global/opencodex/config.json"))
            .unwrap();
    let result = validate_candidate(
        &package,
        &valid,
        Deadline::after(Duration::from_secs(30)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    eprintln!("validation evidence: {}", result.evidence.display());
    assert_eq!(result.status, ValidationStatus::Valid);
    assert_eq!(
        before,
        harness_core::build_identity::hash_file(&repo.join("global/opencodex/config.json"))
            .unwrap()
    );
    let rejected = validate_candidate(
        &package,
        br#"{"port":"private-fixture-value"}"#,
        Deadline::after(Duration::from_secs(30)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    eprintln!("rejection evidence: {}", rejected.evidence.display());
    assert_eq!(rejected.status, ValidationStatus::Rejected);
    assert!(
        !serde_json::to_string(&rejected)
            .unwrap()
            .contains("private-fixture-value")
    );
    let cancelled = Cancellation::default();
    cancelled.cancel();
    let result = validate_candidate(
        &package,
        &valid,
        Deadline::after(Duration::from_secs(30)).unwrap(),
        &cancelled,
    )
    .unwrap();
    assert_eq!(result.status, ValidationStatus::Cancelled);
    let expired = validate_candidate(
        &package,
        &valid,
        Deadline::after(Duration::from_millis(1)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    assert_eq!(expired.status, ValidationStatus::TimedOut);
}

#[test]
#[ignore = "requires explicit HARNESS_OPENCODEX_PACKAGE for the adopted foreign package"]
fn public_restore_json_cleans_owned_injection_and_writes_desired_state() {
    let package = package();
    let injected = "# Auto-injected by opencodex\nopenai_base_url = \"http://127.0.0.1:10100/v1\"\nmodel = \"gpt-6-astra\"\n";
    let result = restore_native(
        &package,
        Some(injected),
        Deadline::after(Duration::from_secs(30)).unwrap(),
        &Cancellation::default(),
    )
    .unwrap();
    eprintln!("restore evidence: {}", result.evidence.display());
    assert_eq!(result.status, ProbeStatus::Ok);
    let stdout = std::fs::read_to_string(result.evidence.join("stdout.json")).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["success"], true);
    let config = std::fs::read_to_string(result.evidence.join("codex/config.toml")).unwrap();
    assert!(!config.contains("openai_base_url"));
    let ocx = std::fs::read_to_string(result.evidence.join("opencodex/config.json")).unwrap();
    assert!(
        ocx.contains("clientIntegrations") || ocx.contains("codex"),
        "public restore wrote durable desired-state unlike skipHistory"
    );
}

#[test]
#[ignore = "requires explicit HARNESS_OPENCODEX_PACKAGE for the adopted foreign package"]
fn public_login_with_closed_stdin_fails_without_echoing_secrets() {
    let package = package();
    let cancelled = Cancellation::default();
    let result = login_xai_closed_stdin(
        &package,
        Deadline::after(Duration::from_secs(8)).unwrap(),
        &cancelled,
    )
    .unwrap();
    eprintln!("login evidence: {}", result.evidence.display());
    assert_ne!(result.status, ProbeStatus::Ok);
    let stdout = std::fs::read_to_string(result.evidence.join("stdout.json")).unwrap_or_default();
    let stderr = std::fs::read_to_string(result.evidence.join("stderr.txt")).unwrap_or_default();
    assert!(!stdout.contains("access_token") && !stderr.contains("access_token"));
    assert!(!stdout.to_lowercase().contains("paste redirect") || result.status != ProbeStatus::Ok);
}
