# OFAP 4.1-4.2 result: instruction-refresh succession

Base commit `3f5f14a`; worktree `<kit-checkout>-ofap-succession`.

## What changed

Instruction-refresh succession for orchestrated workers is implemented and verified against
owned synthetic sessions. `openspec/changes/orchestrate-feedback-and-pacing/tasks.md` 4.1 and 4.2
are checked.

- `crates/harness-core/src/task_succession.rs` (new): validated succession request and
  read-only consumption of the compact skill revision identity published by skill-evolution
  (`name`, canonical `path`, `revision`, `operation`); safe-boundary decision over the
  predecessor's controller artifacts plus native background-terminal/turn facts; bounded
  durable handover record; verified successor argv (`exec resume <SESSION_ID>`, never a picker,
  never `--last`, recorded sandbox/approval kept); rollout-based reload verification that reports
  `succession not established`; exact-session pointer lookup.
- `crates/harness-core/src/task_runtime.rs`: a managed session records its private controller root
  at `CODEX_HOME/harness/task-sessions/<session>.json`, so succession resolves the exact session
  non-interactively.
- `crates/harness-core/src/process_service.rs`: identity-revalidated, bounded
  `ServiceProcess::terminate` used only as the stop fallback.
- `crates/codex-harness/src/executor_cli.rs`: `codex-harness executor succeed --request FILE`.
  The command makes no model calls of its own: it waits for a safe boundary after in-flight tool
  effects, writes the handover record into the owning records (task-store record and private
  session state), stops the predecessor, spawns the successor, verifies the reload from the
  successor's own rollout, writes a per-attempt receipt and exits 0 (established), 1
  (not established) or 2 (blocked/deferred/refused).
- `crates/codex-harness/src/main.rs`: CLI help lists the new verb.
- `crates/codex-harness/tests/executor_succession.rs` plus
  `crates/codex-harness/tests/fixtures/succession_responses.rs` (new, ignored environment-gated
  entry-point checks with a model-free Responses fixture).
- `docs/agent-delegation.md` and `docs/rust-native.md`: supported operation and check coverage.

Ownership boundary respected: `board_feedback.rs`/`board_cli.rs`, the 2.x-3.x slice and
`.agents/skills/board-workflow/SKILL.md` are untouched; OpenSpec changes were not archived and no
board issue was closed. No catalogue injection or compact recovery was implemented (owned by
`autonomous-skill-evolution`).

## How verified

All succession runs used owned synthetic sessions in the harness's private state area under
`%LOCALAPPDATA%` (`hmp-*` roots) with a model-free Responses fixture; no live lead/executor
session was stopped, resumed or otherwise touched. The private evidence summary is under
`%TEMP%`.

1. Release entry-point acceptance

   ```powershell
   $env:HARNESS_CONTROL_CODEX_EXE = '<native codex-cli 0.155.1 executable>'
   cargo test --release -p codex-harness --test executor_succession -- --ignored --nocapture
   ```

   `2 passed` twice consecutively. Case `instruction_refresh_succession_preserves_work_and_reports_not_established`:

   - a stale published revision was refused (`notEstablished`, exit 1) with the predecessor left
     running and no successor request;
   - the valid succession was `established` (exit 0): predecessor stop confirmed, successor
     resumed the exact session id through `codex exec resume` with the recorded
     `danger-full-access`/`approval_policy=never` binding, partial work (`proof.txt` = `one`) was
     observed but not replayed, and the successor's own rollout shows the reloaded AGENTS
     instructions and skill revision;
   - a decoy instruction source made reload verification fail and the command reported
     `succession not established` (exit 1) instead of relying on the refresh.

   Case `succession_defers_while_the_predecessor_turn_is_in_flight`: with the seed turn held open
   the command reported `deferred` (exit 2) with no `stop.json`, no handover and no successor
   request, and the predecessor kept running until its turn settled.

2. Regression and hygiene checks

   - `cargo test -p harness-core --lib task_succession`: 8 passed.
   - `cargo test -p codex-harness --bin codex-harness`: 58 passed.
   - `cargo test -p harness-core --lib -- --test-threads=1`: 651 passed, 5 failed. Those five are
     pre-existing at the base commit and unrelated: committed `global/orchestration.toml` names
     `zai`/`ds` while `orchestration_config`/`orchestration_lifecycle` tests still expect `xai`
     (confirmed with `git show HEAD:...`). With default parallelism the same suite shows more
     transient process/timeout flakes under concurrent build/test load; they pass serially.
   - `cargo test --release -p codex-harness --test task_control_launch ordinary_launcher_starts_control_and_delivers_tool_result -- --ignored --nocapture`: passed (managed-session
     regression, covers the session-pointer addition).
   - `cargo build --release --workspace`; `codex-harness ownership-check --source .` (0 findings);
     `rustfmt --check` on changed files; `cargo clippy -p harness-core -p codex-harness
     --all-targets` (no new warnings).

Private evidence (receipts, requests, successor stdout, rollout extract, command list):
`%TEMP%\ofap-41-42-evidence-20260920\README.md`; owned run roots
`%LOCALAPPDATA%\hmp-88X8BE` (acceptance) and `%LOCALAPPDATA%\hmp-0DNkl0` (deferral).

## Remaining limits

- The successor is a bounded `codex exec resume` continuation turn, not an interactive visible
  view; interactive successor views and production resume/fork ingress remain unimplemented, so
  successors do not yet satisfy the Stage 1 "every active conversation is visible" rule by
  themselves.
- Reload verification depends on the instruction source and the live skill package staying
  readable until the successor turn completes; both are checked again after the turn.
- An assignment-scoped succession requires the owning task record (task and assignment ids) to
  agree with the exact session; otherwise it refuses instead of risking conflicting writers.
- Succession replaces a process for an already-authorized task; it confers no new authority and
  does not close the same-session compact/in-process requirements owned by
  `autonomous-skill-evolution`.
