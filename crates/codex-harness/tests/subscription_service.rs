//! Isolated native subscription host: readiness, 2048 MiB job assignment,
//! shutdown cleanup and runtime retry. Never targets the live global proxy.
#![cfg(windows)]

use harness_core::{subscription_lifecycle, subscription_service};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

fn write_source(root: &Path) {
    let source = root.join("source");
    fs::create_dir_all(source.join("global/opencodex/agents")).unwrap();
    fs::write(
        source.join("global/opencodex/config.json"),
        br#"{"hostname":"127.0.0.1","port":10100,"codexAutoStart":false,"codexShimAutoRestore":false,"providers":{"zai":{"adapter":"openai-chat","baseUrl":"https://api.z.ai/api/coding/paas/v4","authMode":"key","apiKey":"${ZAI_API_KEY}","selectedModels":["glm-5.3"]}}}"#,
    )
    .unwrap();
}

fn setup(root: &Path, port: u16, fail: bool) -> PathBuf {
    write_source(root);
    let source = std::path::absolute(root.join("source")).unwrap();
    let home = std::path::absolute(root.join("codex")).unwrap();
    let user = std::path::absolute(root.join("user")).unwrap();
    fs::create_dir_all(user.join(".opencodex")).unwrap();
    std::os::windows::fs::symlink_file(
        source.join("global/opencodex/config.json"),
        user.join(".opencodex/config.json"),
    )
    .unwrap();
    let paths = subscription_lifecycle::service_paths(&source, &user, &home).unwrap();
    fs::create_dir_all(&paths.runtime).unwrap();
    fs::create_dir_all(paths.role_link.parent().unwrap()).unwrap();
    let bun = PathBuf::from(env!("CARGO_BIN_EXE_harness-subscription-fixture"));
    let descriptor = json!({
        "schema_version": 1,
        "owner": "codex-harness-subscriptions",
        "source": source,
        "user": user,
        "codex": home,
        "task": paths.task,
        "port": port,
        "links": {"configLink": paths.config_source, "roleLink": paths.role_source},
        "dependency": {
            "bun": bun,
            "cli": bun,
            "root": bun.parent().unwrap(),
            "fixture": true,
            "fail": fail
        }
    });
    fs::write(
        &paths.service,
        serde_json::to_vec_pretty(&descriptor).unwrap(),
    )
    .unwrap();
    fs::write(
        &paths.state,
        serde_json::to_vec_pretty(&descriptor).unwrap(),
    )
    .unwrap();
    paths.service
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
fn native_host_assigns_2048_mib_job_publishes_ready_and_cleans_owned_runtime() {
    let root = tempfile::tempdir().unwrap();
    let port = free_port();
    let service = setup(root.path(), port, false);
    let options = subscription_service::HostOptions {
        ready: Duration::from_secs(8),
        retry_delay: Duration::from_millis(20),
        memory_bytes: 2048 * 1024 * 1024,
        restore: false,
    };
    subscription_service::serve_with(&service, options).unwrap();
    let home = std::path::absolute(root.path().join("codex")).unwrap();
    let user = std::path::absolute(root.path().join("user")).unwrap();
    let source = std::path::absolute(root.path().join("source")).unwrap();
    let paths = subscription_lifecycle::service_paths(&source, &user, &home).unwrap();
    let role = subscription_lifecycle::current_link_target(&paths.role_link).unwrap();
    assert!(
        role.is_none(),
        "owned role must be withdrawn after shutdown"
    );
    let logs: Vec<_> = fs::read_dir(&paths.runtime)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("jsonl"))
        .collect();
    assert!(!logs.is_empty());
    let text = fs::read_to_string(logs[0].path()).unwrap();
    assert!(text.contains("\"stage\":\"ready\""), "{text}");
    assert!(text.contains("\"stage\":\"completed\""), "{text}");
}

#[test]
fn native_host_retries_runtime_failure_then_exhausts() {
    let root = tempfile::tempdir().unwrap();
    let port = free_port();
    let service = setup(root.path(), port, true);
    let options = subscription_service::HostOptions {
        ready: Duration::from_millis(200),
        retry_delay: Duration::from_millis(20),
        memory_bytes: 2048 * 1024 * 1024,
        restore: false,
    };
    let error = subscription_service::serve_with(&service, options).unwrap_err();
    assert!(
        error.to_string().contains("Subscription runtime failed"),
        "{error}"
    );
    let home = std::path::absolute(root.path().join("codex")).unwrap();
    let logs: Vec<_> = fs::read_dir(home.join("harness/subscriptions/runs"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("jsonl"))
        .collect();
    let text = fs::read_to_string(logs[0].path()).unwrap();
    assert!(text.contains("service-retry"), "{text}");
    assert!(
        text.contains("service-exhausted") || text.matches("service-attempt").count() >= 2,
        "{text}"
    );
}

#[test]
fn native_host_refuses_foreign_descriptor_without_starting_runtime() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("harness/subscriptions")).unwrap();
    let path = root.path().join("harness/subscriptions/service.json");
    fs::write(&path, br#"{"schema_version":1,"owner":"foreign"}"#).unwrap();
    let error = subscription_service::serve(&path).unwrap_err();
    assert!(
        error.to_string().contains("descriptor path mismatch")
            || error.to_string().contains("ownership"),
        "{error}"
    );
}
