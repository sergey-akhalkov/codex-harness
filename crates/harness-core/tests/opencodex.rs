//! Explicit installed-dependency acceptance; no models, network login or services.
#![cfg(windows)]
use harness_core::{
    opencodex::{ValidationStatus, validate_candidate},
    process::{Cancellation, Deadline},
};
use std::{path::PathBuf, time::Duration};

#[test]
#[ignore = "requires explicit HARNESS_OPENCODEX_PACKAGE for the adopted foreign package"]
fn installed_cli_validates_owned_candidates_and_preserves_source() {
    let package = PathBuf::from(
        std::env::var_os("HARNESS_OPENCODEX_PACKAGE").expect("explicit package root"),
    );
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
