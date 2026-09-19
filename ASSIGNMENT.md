# OFAP 6.1-6.3 + worktree lane reuse (executor profile ds)

You are the configured executor (`codex --profile ds`, exec mode: your tab
shows your work and closes when you finish). Do not spawn nested agents. Work
only in this worktree (branch `ofap/stage6`, base bd912aa).

## State

Stages 1-5 are accepted and integrated in this base: feedback intake/triage/
votes, observation routing, promotion/override/hygiene, scoped observations,
pacing, benefit gate, and instruction-refresh succession. Do not redo them.

## Outcome

1. pvr.5 Worktree lane reuse in code: extend
   `crates/harness-core/src/task_worktree.rs` so an accepted merge can
   reset-and-reuse the lane worktree (`git reset --hard <base>` +
   `git clean -fd`, keeping ignored build caches) instead of deleting it;
   keep retire/delete for lane retirement and unresettable state; preserve the
   existing audit/limit guard. Native tests for both paths.
2. 6.1 Lifecycle delivery: connect the loop through the kit installation
   lifecycle so a fresh external session discovers the workflow (skills,
   orchestration config, board), unrelated configuration is preserved, and
   rollback removes the loop without losing archived evidence. Use the
   existing install/check lifecycle tests and add what is missing.
3. 6.2 Owning records: update `docs/agent-delegation.md`,
   `docs/subscription-models.md` (efficiency), token-workflow record and
   `docs/project-decisions.md` links with the actual supported operation and
   limits as integrated (exec-mode tabs, watcher waiting, lane reuse,
   pacing bands, benefit gate, succession). Keep private evidence out of Git;
   one authoritative home per fact.
4. 6.3 End-to-end loop exercise on the real consuming task: assemble and
   verify the already-completed real run (OFAP stages 1-5 through this board:
   feedback -> dedup/votes -> promotion (pvr.5 lane reuse, benefit-gate
   adopt -11.8%) -> implementation -> succession path exercised by the
   stage-4 executor). Record the end-to-end evidence trail (board ids,
   commits, result files) and close only tasks supported by actual results -
   do not fabricate steps that did not happen; name any gap honestly.

## Constraints

Rust first-party; PowerShell 7; no model calls in deterministic mechanics;
public pack free of private data (paths, dashboards, logs stay local); do not
archive OpenSpec changes; do not close board issues. `executor_cli.rs` is
stable - read-only reference.

## Done when

- 6.1, 6.2, 6.3 and pvr.5 implemented/verified; tasks.md checkboxes marked
  only for fully completed work.
- Native checks ran (cargo fmt/test/clippy as applicable, lifecycle checks);
  private evidence in `%TEMP%`.
- `ofap-6-result.md` at this worktree root: what changed, how verified, the
  e2e evidence trail, remaining limits.
- `bd update codex-harness-pvr.1 --status lead_review` (do not close).
