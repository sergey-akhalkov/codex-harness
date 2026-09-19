# OFAP 4.1-4.2 complete outcome (executor profile ds)

You are the configured executor (`codex --profile ds`). Do not spawn nested
agents. Do not write to other checkouts. Work only in this worktree
(`D:\home\sergey-akhalkov\codex-harness-ofap-succession`, branch `ofap/succession`,
base commit 3f5f14a).

## Outcome

Implement and verify OFAP tasks 4.1 and 4.2 from
`openspec/changes/orchestrate-feedback-and-pacing/` (tasks.md, design.md
decision 6, specs), reusing the Stage 1 machinery already in this tree:

1. 4.1 Successor spawning through the verified `codex resume` path at a safe
   boundary after in-flight tool effects: deterministic non-interactive session
   selection, durable context handover, predecessor process stop. Consume the
   compact revision identity published by skill-evolution (name, canonical
   path, revision, operation) as read-only input. Do NOT implement catalogue
   injection, in-process activation or same-session compact recovery (owned by
   `autonomous-skill-evolution`).
2. 4.2 Verify succession preserves partial work and authorization, does not
   replay uncertain external operations, and reports "succession not
   established" when reload verification fails.

Relevant existing code: `crates/harness-core/src/orchestration_lifecycle.rs`,
`orchestration_config.rs` (`SuccessorChoice`), `task_orchestrate.rs`,
`task_handoff.rs`; `crates/codex-harness/src/executor_cli.rs` shows the current
dispatch patterns (trust handling, session-env isolation).

## Ownership boundary

A parallel executor owns `board_feedback.rs`/`board_cli.rs` and the 2.x-3.x
slice. Do not modify those files or `.agents/skills/board-workflow/SKILL.md`.
If a change there is required, record it as a comment on board issue
`codex-harness-kon.1` instead.

## Safety constraints

- Verify resume/succession only against synthetic sessions in `%TEMP%`
  projects. Never stop or resume the live lead or executor sessions.
- Rust for first-party code. PowerShell 7 for shell. No private consumer data
  in tracked files. No model calls for deterministic mechanics.
- Do not archive OpenSpec changes. Do not close board issues. Do not start
  stage 5+.

## Done when

- 4.1 and 4.2 are implemented and verified in this worktree; tasks.md
  checkboxes marked only for fully completed work.
- Native checks for changed code ran (cargo test/clippy/fmt as applicable);
  keep a private evidence summary in `%TEMP%`.
- `ofap-41-42-result.md` at this worktree root: what changed, how verified
  (exact commands and outcomes), remaining limits.
- `bd update codex-harness-kon.1 --status lead_review` (do not close).
