# Tasks

Design artifact deliberately skipped: its own instruction requires it only for cross-cutting architecture, new dependencies, data-model or migration complexity, or unresolved ambiguity; this instruction-and-spec change has none.

## 1. Lead workflow instructions

- [x] 1.1 Add the executor-utilization duty to `.agents/skills/team-lead/SKILL.md` near the operating objective: while orchestration is active, worthwhile capability-sized work is dispatched to configured executors, and every idle executor or free slot carries a recorded concrete reason (no worthwhile slice, unresolved dependency, quota pacing, preserved or blocked slot, dispatch unavailable); verify by re-reading that every idle cause in the `agent-delegation` delta is covered and no delegation-count target or manufactured filler work is introduced
- [x] 1.2 Add utilization checkpoints to the lead cycle in the same skill - session start, stage or epic planning, after each dispatch decision, and after each acceptance or slot release with backfill - and require lead progress reports to state busy slots against configured concurrency plus each idle reason from board and pool records; verify the wording preserves quota pacing and never preempts a healthy executor
- [x] 1.3 Keep the ordinary-session boundary intact in the edited skill: no executor spawn, board record or utilization reporting without lead activation; verify by checking the activation section still precedes and governs the new text

## 2. Owning documentation

- [x] 2.1 Add the utilization-discipline paragraph to `docs/agent-delegation.md` as the single detailed owner (dispatch-or-reason duty, checkpoint cadence, occupancy reporting, no filler work), linking to the specifications instead of duplicating requirement text; verify changed local links resolve
- [x] 2.2 Extend the decomposition paragraph in `global/principles-of-work.md` with one sentence: delegated workers are not left idle without a recorded reason; verify the sole-developer default for ordinary sessions is unchanged

## 3. Validation and delivery

- [x] 3.1 Run `openspec validate lead-executor-utilization --strict` and fix every reported finding until validation passes
- [x] 3.2 Run applicable documentation hygiene for changed files (local-link and factual checks) and `codex-harness ownership-check`, and fix findings until clean
- [x] 3.3 Deliver the updated skill and instructions through the kit installation lifecycle and verify from a fresh external consumer session that the installed `team-lead` skill text contains the utilization duty and checkpoints
