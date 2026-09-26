//! Synthetic, per-response native rollout counters; never calls a provider.
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// One exact-session rollout that proves warmup and then records three
/// consecutive large misses.
pub fn write_loss(home: &Path, session: &str) -> PathBuf {
    write(
        home,
        session,
        &[
            (400_000, 399_000),
            (400_000, 6_000),
            (400_000, 6_000),
            (400_000, 6_000),
        ],
    )
}

/// One exact-session rollout of valid counters that never meet the warmup
/// thresholds: the run is observed with usable numbers, its runtime support
/// stays unproven, and no stop may follow.
pub fn write_never_warmed(home: &Path, session: &str) -> PathBuf {
    write(home, session, &[(200_000, 0); 4])
}

/// Writes the given per-response `(input, cached)` counters for one exact
/// session, timestamped now so none of them counts as historical warmup.
pub fn write(home: &Path, session: &str, counters: &[(u64, u64)]) -> PathBuf {
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
    let mut total = 0;
    for (index, (input, cached)) in counters.iter().enumerate() {
        total += input;
        writeln!(
            file,
            "{}",
            json!({"type":"token_usage_record","timestamp":timestamp,
            "payload":{"thread_id":session,"response_id":format!("response-{index}"),
            "usage":{"input_tokens":input,"cached_input_tokens":cached},
            "thread_token_usage":{"input_tokens":total}}})
        )
        .unwrap();
    }
    file.flush().unwrap();
    path
}
