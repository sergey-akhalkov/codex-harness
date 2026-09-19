# OFAP 2.4 + 3.1-3.4 result (executor `ds`, lane A)

Using change: `orchestrate-feedback-and-pacing` (schema `spec-driven`), tasks
2.4 and 3.1-3.4, per `ASSIGNMENT.md`. Branch `ofap/feedback-triage` at base
`a266f59`; all changes are uncommitted working-tree edits in this lane, which
the lead owns integrating. Work ran in this worktree only; no agents were
spawned and no board issue was closed.

## What changed

| File | Change |
| --- | --- |
| `crates/harness-core/src/board_feedback.rs` | Observation routing, promotion, consequence override and lead-owned hygiene; ledger now parses route and promotion history |
| `.agents/skills/board-workflow/SKILL.md` | Command tables and rules for routing, promotion, override, hygiene triggers and restoration |
| `.agents/skills/team-lead/SKILL.md` | Lead duties: route by kind, promote by consequence, override, sweep on the two triggers |
| `crates/harness-core/src/orchestration_config.rs` | Four stale tests still expected the old `xai` profile names; updated to the committed `ds`/`zai` config (tests only, no behavior change) |
| `openspec/changes/orchestrate-feedback-and-pacing/tasks.md` | 2.4 and 3.1-3.4 checked |
| `openspec/changes/orchestrate-feedback-and-pacing/design.md` | Kit-backlog open question resolved |
| `docs/project-decisions.md` | Durable kit-backlog decision recorded |

Mechanics (all through the non-interactive `bd` CLI; no model calls):

- **Routing (2.4):** each triage action carries an `ObservationKind`. A
  verified reusable procedure is handed to `autonomous-skill-evolution` as a
  reference (route comment plus `skill-evolution` label; `feedback` label
  removed) and records no vote; process, orchestration, requirement, tool,
  unclear, material and kit-concern observations incubate with a visible
  `feedback-route v1` classification. Both directions of the exclusivity rule
  are enforced, so one observation can never be both incubator demand and a
  skill candidate. Routing and promotion write no skill package.
- **Promotion (3.1/3.2):** candidates are counted from `feedback-vote v1`
  history against the configured `vote_threshold` (3 in
  `global/orchestration.toml` = promotion after more than two counted votes).
  Promotion removes `incubator`, adds a route label (`backlog`, `openspec` or
  `kit-forwarded`) and appends `feedback-promote v1` history; every vote,
  merge and route comment stays inspectable. Consequence routing: small
  improvements stay backlog tasks, requirement changes create an
  `openspec/changes/feedback-<item>/proposal.md` entry, kit instruction/skill
  or tool concerns create a sanitized `kit-feedback` task on the kit
  checkout's own board (explicit kit path) containing only kit-level summary
  and scope.
- **Override (3.3):** `ConsequenceOverride` promotes without votes and records
  the concrete consequence and reason verbatim in the promotion comment;
  empty or oversized text is refused.
- **Hygiene (3.4):** `incubator_size`/`incubator_over_cap` are board queries
  only. `sweep_incubator` is lead-owned: a non-lead caller defers with no
  mutation, triggers are `stage-or-epic-closed` and
  `incubator-above-cap size=<n> cap=<n>` (the second is checked against the
  live incubator), archives close with a visible reason and keep labels,
  votes and merge history, and `restore_archived` reopens the item on fresh
  evidence. No background scheduler exists; with no lead session the sweep
  waits.

## How verified

| Check (final revision) | Result |
| --- | --- |
| `cargo test -p harness-core --lib -- --test-threads=1` | 668 passed, 1 failed (pre-existing, see limits), 39 ignored |
| `HARNESS_BD_EXE=... cargo test -p harness-core windows_noninteractive -- --ignored` | 2 passed (real beads v1.3.0 contract on isolated temp projects) |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p harness-core --all-targets` | no warnings in changed files (5 pre-existing warnings in unmodified files) |
| `openspec validate "orchestrate-feedback-and-pacing" --strict` | valid |
| `harness-source-check --root .` | 32 findings, byte-identical to the base-commit clone; none introduced by this task |

Scenario evidence: the eight new tests in `board_feedback` cover the procedure
handoff with a sentinel `SKILL.md` proven untouched, both exclusivity
directions, the threshold promotion with preserved history, the
requirement-to-OpenSpec and kit-concern-to-kit-board routes (including the
no-private-data assertion), override recording and validation, the two
hygiene triggers with deferral/archive/restore, and the skill command-table
consistency check. Private evidence summary and raw logs are in `%TEMP%`
(`ofap-24-3-evidence-*.md`, `ofap-lib-serial.txt`).

## Remaining limits

- `orchestration_lifecycle::tests::preview_check_does_not_write_and_disconnect_keeps_stopped_checkpoints`
  fails at base and after this change: the test still installs the old `xai`
  profile against the committed `ds`/`zai` configuration (same staleness the
  four `orchestration_config` tests had). That file is owned by the parallel
  succession executor, so it was not edited; the needed change is recorded as
  a comment on `codex-harness-dl6.2`.
- The "lead closes a stage or epic" hygiene trigger is procedural by design
  (no controller board parsing and no scheduler): the skill instructs the
  lead to run the sweep with that trigger, and the mechanic plus its history
  record are tested. Automation of that call belongs to a later lead-flow
  task.
- Kit-backlog promotion needs the kit checkout path passed explicitly; the
  kit board's runtime state stays host-local and out of Git.
- The override records lead-supplied evidence; the mechanic enforces
  presence and bounds, not the truth of the evidence (lead judgment by
  design).
- `harness-source-check` still reports 32 pre-existing broken links in older
  archives and records; none are in this task's changed lines.
