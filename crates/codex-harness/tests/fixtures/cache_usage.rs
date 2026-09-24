//! Synthetic, per-response native rollout counters; never calls a provider.
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn write_loss(home: &Path, session: &str) -> PathBuf {
    let directory = home.join("sessions/2026/01/01");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("rollout-fixture-{session}.jsonl"));
    let mut file = fs::File::create(&path).unwrap();
    writeln!(
        file,
        "{}",
        json!({"type":"session_meta","payload":{"id":session}})
    )
    .unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let timestamp = chrono::DateTime::from_timestamp_millis(now)
        .unwrap()
        .to_rfc3339();
    for (index, cached) in [399_000, 6_000, 6_000, 6_000].into_iter().enumerate() {
        writeln!(
            file,
            "{}",
            json!({"type":"token_usage_record","timestamp":timestamp,
            "payload":{"thread_id":session,"response_id":format!("response-{index}"),
            "usage":{"input_tokens":400_000,"cached_input_tokens":cached},
            "thread_token_usage":{"input_tokens":400_000 * (index + 1)}}})
        )
        .unwrap();
    }
    file.flush().unwrap();
    path
}
