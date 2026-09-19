# OFAP 5.1-5.3 complete outcome (executor profile ds)

You are the configured executor (`codex --profile ds`). Do not spawn nested
agents. Work only in this worktree (branch `ofap/feedback-pacing`, base ff3284f).

## State

Stages 1-3 are accepted and integrated in this base: feedback intake/triage,
vote provenance, observation routing, promotion threshold, consequence
routing+override and incubator hygiene live in
`crates/harness-core/src/board_feedback.rs` (+ `board_cli.rs`,
`orchestration_config.rs`); skills describe the command tables. Do not redo
them.

## Outcome (tasks 5.1, 5.2, 5.3; design decisions 7-8)

1. 5.1 Scoped observations: native lead-account limit reads where the
   installed contract exposes them (GPT/Codex native limits only), actual
   provider refusals observed by executors, and bounded user-supplied
   dashboard snapshots accepted as opaque inputs. Unknown stays unknown: no
   probe calls, no invented percentages, no telemetry scraping.
2. 5.2 Pacing: apply the scoped observations to new assignments,
   concurrency, effort and feedback cadence without preempting healthy
   executors; account for reset-time burst avoidance across tasks sharing an
   account. Pacing decisions must be inspectable (visible reason) and
   reversible; no background scheduler.
3. 5.3 Benefit-gate: run a matched comparison on at least one promoted
   improvement before it becomes a default. Candidate with real data: the
   lane-reuse worktree policy (reset-after-merge vs fresh worktree) - compare
   warm vs cold build-cache delivery time and unchanged quality on matched
   tasks; include coordination and rework in the accounting. Produce an honest
   record: matched scenario, metrics, limits; an inconclusive result is
   acceptable if measured, but then the improvement must not be marked as a
   proven default.

## Ownership boundary

A parallel executor owns succession files (`orchestration_lifecycle.rs`,
`task_orchestrate.rs`, `task_handoff.rs`, `executor_cli.rs`). Do not modify
those; record needed changes as a comment on board issue `codex-harness-qr6.1`.
You own `board_feedback.rs`, `board_cli.rs`, `orchestration_config.rs` and may
add new modules.

## Constraints

Rust first-party; PowerShell 7 for shell; no model calls in deterministic
mechanics; no probe API calls for telemetry; public pack free of private
consumer data (dashboard snapshots are opaque bounded inputs, never raw
transcripts); do not archive OpenSpec changes; do not close board issues; do
not start stage 6.

## Done when

- 5.1-5.3 implemented/verified; tasks.md checkboxes marked only for fully
  completed work.
- Native checks ran (cargo test/clippy/fmt as applicable); private evidence
  in `%TEMP%`.
- `ofap-5-result.md` at this worktree root: what changed, how verified, the
  benefit-gate comparison record, remaining limits.
- `bd update codex-harness-qr6.1 --status lead_review` (do not close).
