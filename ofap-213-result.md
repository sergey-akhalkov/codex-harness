# OFAP 2.1-2.3 finish and verify — executor result

- Board issue: `codex-harness-dl6.1` (moved to `lead_review` after this file was written; not closed)
- Worktree: this checkout, branch `ofap/feedback-intake`, base commit `99aa5d4`
- Date: 2026-09-19
- Private evidence summary (commands, outputs, environment): `%TEMP%\ofap-213-evidence.md`

## Outcome

Tasks 2.1-2.3 in `openspec/changes/orchestrate-feedback-and-pacing/tasks.md` were
already implemented in the partial state left by the previous executor. This
pass reviewed that implementation against the three tasks and the
`orchestration-feedback-loop` spec decisions 1-2 (feedback is a queue, not chat;
one vote per distinct episode/reporter with inspectable provenance), found no
defect that required a code change, and verified the behavior with native
checks. The implementation was not redone, 1.1-1.3 were not touched, and 2.4 /
stage 3+ were not started. The only working-tree delta relative to the partial
state is this report file (plus the private evidence file under `%TEMP%`).

Reviewed and confirmed:

- `crates/harness-core/src/board_feedback.rs` (new): bounded feedback fields
  (observation/scope/reporter/episode/kind with hard caps, no transcripts),
  board-backed recording with the reporter as actor, batch triage with an
  explicit lead merge decision (`bd duplicate` plus a visible merge comment),
  one counted vote per distinct `(episode, reporter)`, same-reporter repeats
  recorded visibly as `counted=false reason=repeat`, automated diagnostics
  always `counted=false reason=automated-diagnostic`, and a ledger parser for
  `bd comments` provenance inspection. Only `bd` commands are invoked; the
  model-backed `bd find-duplicates` path is explicitly not used, and similarity
  stays lead judgment supplied as explicit actions.
- `crates/harness-core/src/board_cli.rs` (new): non-interactive `bd` helpers,
  actor-aware JSON calls, `feedback`/`incubator` labels, and the WindowsApps
  PATH filter for child processes.
- `crates/harness-core/src/lib.rs`: module wiring.
- `.agents/skills/board-workflow/SKILL.md` and `.agents/skills/team-lead/SKILL.md`:
  bounded feedback description format, batch-triage command table, safe-boundary
  triage, and the no-model-calls rule for routine triage mechanics.
- `crates/codex-harness/tests/fixtures/control_responses.rs` and
  `tests/task_control_launch.rs`: formatting-only normalization; both files
  fail `rustfmt --check` at the base commit, so these edits are required for the
  workspace formatting gate.

Stray nested duplicate directory
`openspec/changes/orchestrate-feedback-and-pacing/orchestrate-feedback-and-pacing/`
is absent: a recursive directory scan finds only the real change directory
(7 files), `git grep` finds no tracked reference (the only mention of that path
is the untracked `ASSIGNMENT.md` note), so there was nothing to delete.
`openspec validate orchestrate-feedback-and-pacing --type change --strict` is
valid.

## How it was verified

All commands run from the worktree root:

1. `cargo fmt --all -- --check` — exit 0.
2. `cargo test -p harness-core board --locked --jobs 1 -- --test-threads=1` —
   16 passed, 0 failed, 2 ignored; includes four tests that drive the real
   `bd` v1.3.0 in isolated temp git projects (record/list without chat,
   merge + one vote per distinct episode/reporter, repeat/diagnostic vote
   integrity, batch deferral).
3. `HARNESS_BD_EXE=<installed bd>` then
   `cargo test -p harness-core board --locked --jobs 1 -- --ignored --test-threads=1` —
   2 passed (epic/feature/feedback/status contract; merge/vote/promote/archive
   incubator contract).
4. `cargo clippy -p harness-core --all-targets --locked --jobs 1` — zero
   warnings in the changed files; the only two warnings are pre-existing lints
   in unmodified files (`task_observer.rs:489`, `task_runtime.rs:320`), which
   are identical at the base commit. The codex-harness edits are rustfmt-only
   and are covered by the formatting check above.
5. Retry probe: `bd duplicate <id> --of <canonical>` is idempotent (second
   invocation exits 0), so re-running the same triage actions after a transient
   `bd` failure restores the vote without double counting.
6. Manual walkthrough of the documented lead workflow in an owned temp project
   using exactly the skill's command table: list open feedback, admit, merge,
   record merge + vote, attempt a repeat (`counted=false`), inspect provenance
   via `bd comments`, confirm the queue drains. No chat channel involved.

Execution identity: cargo 1.97.1 / rustfmt 1.9.0 / clippy 0.1.97; `bd` 1.3.0
(`f45b249ce`) from `%CODEX_HOME%\harness\bin` and PATH; real `git` for fixtures.
This host has no desktop PowerShell 7 (only Windows PowerShell 5.1 and the
forbidden WindowsApps `pwsh` alias), so checks ran as direct tool executions
through the provided command shell; no installs were made.

## Merge inventory for acceptance

- Modified tracked: `.agents/skills/board-workflow/SKILL.md`,
  `.agents/skills/team-lead/SKILL.md`,
  `crates/codex-harness/tests/fixtures/control_responses.rs`,
  `crates/codex-harness/tests/task_control_launch.rs`,
  `crates/harness-core/src/lib.rs`, and the
  `openspec/changes/orchestrate-feedback-and-pacing/` artifacts
  (`design.md`, `proposal.md`, `tasks.md`, three spec files).
- New untracked deliverables: `crates/harness-core/src/board_cli.rs`,
  `crates/harness-core/src/board_feedback.rs`.
- `global/orchestration.toml` is untracked here but already tracked and
  byte-identical in the lead checkout (`vote_threshold = 3`,
  `incubator_size_cap = 32`, `feedback_batch_limit = 8`).
- `.openspec.yaml` shows as modified only by line endings; content equals HEAD.
- Local-only artifacts, not deliverables: `ASSIGNMENT.md`,
  `executor-spawn.json`, this report.

## Remaining limits

- 2.4 and stage 3+ are intentionally not implemented (threshold promotion,
  consequence override, hygiene, observation routing).
- The batch limit is a code default kept consistent with the kit TOML by test;
  config read and check-time validation remain with the Stage 1
  orchestration-configuration owner (task 1.3), not this module.
- Crate-wide `cargo clippy ... -D warnings` is blocked by the two pre-existing
  lints above (unmodified files); reported, not fixed as out of scope.
- A retry after a mid-batch failure re-adds the merge comment (duplicate
  provenance, no vote double count); the release-archive digest pin in
  `board_cli.rs` was not re-derived (1.x/3.x contract scope).
- Execution policy on this host blocks recursive deletion, so four disposable
  synthetic probe projects remain under `%TEMP%\ofap-*` (no private data);
  the test suites clean their own temp projects automatically.
