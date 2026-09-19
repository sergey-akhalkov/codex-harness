# OFAP 2.1-2.3 finish and verify (executor profile ds)

You are the configured executor (`codex --profile ds`). Do not spawn nested
agents. Do not write to the shared checkout
`D:\home\sergey-akhalkov\codex-harness`. Work only in this worktree.

## State left by the previous executor (xai, stopped by account limit)

- Tasks 2.1-2.3 checkboxes are already marked done in this worktree's
  `openspec/changes/orchestrate-feedback-and-pacing/tasks.md`.
- Implemented here: `crates/harness-core/src/board_feedback.rs` (new, unit
  tests exercise the real `bd`), `board_cli.rs` (actor-aware helpers,
  incubator label, WindowsApps PATH filter), `lib.rs` wiring, updates to
  `.agents/skills/board-workflow/SKILL.md`, `.agents/skills/team-lead/SKILL.md`,
  and test fixtures `control_responses.rs`, `task_control_launch.rs`.
- Missing: verified checks, result file, board status update.
- Stray junk: `openspec/changes/orchestrate-feedback-and-pacing/orchestrate-feedback-and-pacing/`
  is an accidental nested copy of the change directory.

## Outcome

1. Review the existing implementation against OFAP tasks 2.1-2.3 and
   `specs/orchestration-feedback-loop/spec.md` decisions 1-2 (feedback is a
   queue; one vote per distinct episode/reporter with provenance). Fix defects.
   Do not redo from scratch. Do not roll back 1.1-1.3. Do not start 2.4 or
   stage 3+.
2. Confirm the stray nested duplicate directory is not referenced, then delete
   it.
3. Run native checks for the changed code: at minimum
   `cargo test -p harness-core board` plus formatting/static checks applicable
   to changed files. Keep a private evidence summary (commands and results) in
   `%TEMP%`, not in Git.
4. Write `ofap-213-result.md` at this worktree root: what changed relative to
   the partial state above, how it was verified (exact commands and outcomes),
   and remaining limits.
5. Set the board issue to review:
   `bd update codex-harness-dl6.1 --status lead_review`. Do not close it.

## Constraints

- Rust for first-party code. PowerShell 7 for shell. Strip WindowsApps from
  PATH for child processes where the code already does.
- Public pack: no private consumer paths, names or data in tracked files.
- Routine board mechanics (list, merge, vote) make no model calls.
- Do not archive OpenSpec changes. Do not close board issues.

Stop after the 2.1-2.3 outcome is complete.
