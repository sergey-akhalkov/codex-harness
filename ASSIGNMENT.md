# OFAP 2.4 + 3.1-3.4 complete outcome (executor profile ds)

You are the configured executor (`codex --profile ds`). Do not spawn nested
agents. Work only in this worktree (branch `ofap/feedback-triage`, base a266f59).

## State

OFAP 2.1-2.3 are accepted and integrated in this base: bounded feedback intake,
batch triage and vote provenance live in `crates/harness-core/src/board_feedback.rs`
(+ `board_cli.rs`), skills describe the format and command table. Build on that
API; do not rewrite it. Tasks 1.1-1.3 and 2.1-2.3 are done - do not redo them.

## Outcome (tasks 2.4, 3.1-3.4; design decisions 3-5)

1. 2.4 Observation routing: a verified reusable procedure in owned skill scope
   is handed to `autonomous-skill-evolution` as a reference only (no package
   writes, `SKILL.md` untouched); process/orchestration/requirement/tool/unclear
   or material observations stay in the incubator; kit skill or instruction
   demand promotes to the kit backlog without private consuming-project data.
   Verify one observation is never both voted as incubator demand and
   auto-authored as a skill, and that promotion never writes skill packages.
2. 3.1 Promotion threshold from `global/orchestration.toml`
   (`vote_threshold = 3` = promote on more than two counted votes); promotion
   moves the incubator item to the backlog with history preserved.
3. 3.2 Consequence routing: small improvement -> backlog task; behavior or
   requirement change -> OpenSpec change entry; kit-concern item -> kit
   backlog, no private data.
4. 3.3 Lead consequence override: promote without votes for material
   correctness/integrity/safety evidence, with a recorded reason in history.
5. 3.4 Lead-owned incubator hygiene, deterministic triggers only: sweep when
   the lead closes a stage or epic during acceptance, and when a triage batch
   finds incubator size above `incubator_size_cap`; safe deferral when no lead
   session is active; archive with visible reasons, inspectable history,
   restoration on fresh evidence; size checks make no model calls.

## Constraints

- A parallel executor owns succession files (`orchestration_lifecycle.rs`,
  `task_orchestrate.rs`, `task_handoff.rs`, `executor_cli.rs`). Do not modify
  those; record needed changes as comments on board issue `codex-harness-dl6.2`.
- Rust first-party; PowerShell 7 for shell; no model calls in routine board
  mechanics; no private consumer data in tracked files; do not archive OpenSpec
  changes; do not close board issues; do not start stage 4+.

## Done when

- 2.4 and 3.1-3.4 implemented and verified; tasks.md checkboxes marked only
  for fully completed work.
- Native checks ran (cargo test/clippy/fmt as applicable); private evidence
  summary in `%TEMP%`.
- `ofap-24-3-result.md` at this worktree root: what changed, how verified,
  remaining limits.
- `bd update codex-harness-dl6.2 --status lead_review` and
  `bd update codex-harness-51w.1 --status lead_review` (do not close).
