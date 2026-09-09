#![cfg(windows)]

use harness_core::{
    config_create::ConfigCreation,
    config_file::ConfigSnapshot,
    feature_edit::{Feature, prepare_feature},
};
use std::{fs, path::PathBuf, time::Duration};

#[test]
#[ignore = "requires explicit HARNESS_NATIVE_CODEX; model-free owned native CLI acceptance"]
fn actual_native_feature_edit_preserves_unrelated_configuration() {
    let upstream =
        PathBuf::from(std::env::var_os("HARNESS_NATIVE_CODEX").expect("original native CLI path"));
    let before = br#"# retained comment
model = "gpt-6-astra"
model_reasoning_effort = "xhigh"
openai_base_url = "http://127.0.0.1:10100/v1"
[features]
hooks = true
goals = true
memories = false
[features.context_management]
experimental_mode = true
[projects.'C:\owned unrelated']
trust_level = "trusted"
"#;
    let publication = tempfile::tempdir().unwrap();
    let config = publication.path().join("config.toml");
    fs::write(&config, before).unwrap();
    let baseline = ConfigSnapshot::read(&config).unwrap();
    let disabled = prepare_feature(
        &upstream,
        before,
        Feature::Hooks,
        false,
        Duration::from_secs(30),
    )
    .unwrap();
    let expected = String::from_utf8(before.to_vec())
        .unwrap()
        .replace("hooks = true", "hooks = false");
    assert_eq!(disabled.proposed_config(), expected.as_bytes());
    let published_disabled = disabled.publish_to(&baseline).unwrap();
    assert_eq!(fs::read(&config).unwrap(), disabled.proposed_config());
    let repeated = prepare_feature(
        &upstream,
        disabled.proposed_config(),
        Feature::Hooks,
        false,
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(repeated.proposed_config(), disabled.proposed_config());
    let published_repeated = repeated.publish_to(&published_disabled).unwrap();
    let code_mode = prepare_feature(
        &upstream,
        disabled.proposed_config(),
        Feature::CodeMode,
        true,
        Duration::from_secs(30),
    )
    .unwrap();
    let text = std::str::from_utf8(code_mode.proposed_config()).unwrap();
    assert!(text.contains("code_mode = true"));
    assert_eq!(text.replace("code_mode = true\n", ""), expected);
    let published_code_mode = code_mode.publish_to(&published_repeated).unwrap();
    assert_eq!(fs::read(&config).unwrap(), code_mode.proposed_config());
    assert!(disabled.publish_to(&published_code_mode).is_err());
    assert_eq!(fs::read(&config).unwrap(), code_mode.proposed_config());
    published_code_mode.replace(before).unwrap();
    assert_eq!(fs::read(&config).unwrap(), before);
    let baseline = ConfigSnapshot::read(&config).unwrap();
    let registration =
        harness_core::registration::Registration::open(&publication.path().join("state")).unwrap();
    registration
        .apply_with_configs(&[], &[disabled.plan_for(&baseline).unwrap()])
        .unwrap();
    assert_eq!(fs::read(&config).unwrap(), disabled.proposed_config());
    registration.disconnect().unwrap();
    assert_eq!(fs::read(&config).unwrap(), before);
    let empty = prepare_feature(
        &upstream,
        b"",
        Feature::Hooks,
        false,
        Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(empty.proposed_config(), b"[features]\nhooks = false\n");
    let fresh_config = publication.path().join("fresh-home/config.toml");
    registration
        .apply_with_files(
            &[],
            &[],
            &[ConfigCreation::new(&fresh_config, empty.proposed_config()).unwrap()],
        )
        .unwrap();
    assert_eq!(fs::read(&fresh_config).unwrap(), empty.proposed_config());
    registration.disconnect().unwrap();
    assert!(!fresh_config.exists());
    let error = prepare_feature(
        &upstream,
        b"PRIVATE_FEATURE_SENTINEL = [",
        Feature::Hooks,
        false,
        Duration::from_secs(30),
    )
    .unwrap_err();
    assert!(!error.to_string().contains("PRIVATE_FEATURE_SENTINEL"));
    assert!(
        error
            .to_string()
            .contains("Native feature command did not succeed")
    );
    assert!(!format!("{disabled:?}").contains("openai_base_url"));
    for edit in [&disabled, &repeated, &code_mode, &empty] {
        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(edit.evidence.join("receipt.json")).unwrap()).unwrap();
        assert_eq!(receipt["passed"], true);
        assert_eq!(receipt["live_configuration_modified"], false);
        println!("Native feature evidence: {}", edit.evidence.display());
    }
    println!("Preserved malformed-input failure: {error}");
}
