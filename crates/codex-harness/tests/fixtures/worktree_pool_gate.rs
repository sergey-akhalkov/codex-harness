//! Model-free checkout gate for the worktree pool checks.
//!
//! The kit checkout's `post-checkout` adapter invokes this compiled fixture
//! when `WORKTREE_POOL_TEST_GATE_EXE` is configured, so one dispatch can be
//! held inside its checkout preparation while a check measures admission,
//! serialization or recovery against it. The fixture owns the whole wait: it
//! records that the gated preparation started and then polls the gate file
//! until the releasing check writes it or its own bound elapses. The Git hook
//! adapter only forwards to this binary, and nothing here is a product entry
//! point.
//!
//! Compiled with rustc by the worktree pool checks; no dependencies.
use std::{
    env, fs,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

/// Bounded like the hook boundary it replaces: long enough for a slow check,
/// short enough to release an orphaned gate without external help.
const WAIT_BOUND: Duration = Duration::from_secs(90);

/// Poll interval of one gated preparation.
const WAIT_STEP: Duration = Duration::from_millis(200);

fn main() {
    let Ok(gate) = env::var("WORKTREE_POOL_TEST_GATE") else {
        return;
    };
    if gate.trim().is_empty() {
        return;
    }
    let marker = env::var("WORKTREE_POOL_TEST_STARTED").expect("started marker path");
    fs::write(&marker, "started\n").expect("started marker is writable");
    let gate = PathBuf::from(gate);
    let deadline = Instant::now() + WAIT_BOUND;
    while Instant::now() < deadline {
        if gate.is_file() {
            return;
        }
        thread::sleep(WAIT_STEP);
    }
}
