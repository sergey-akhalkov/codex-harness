## Why

When the `team-lead` role is active, lead sessions intermittently forget to dispatch available work to configured executors, or leave accepted executor capacity idle while doing executor-suitable routine work themselves. Current instructions frame delegation economically - delegate when worthwhile - but define no utilization duty, no utilization checkpoint in the lead cycle, and no observable idle-state reporting, so under-delegation stays invisible until a human notices.

## What Changes

- Add an executor-utilization duty to the lead role: while orchestration is active, the lead keeps configured executor capacity supplied with worthwhile, capability-sized work, and every idle executor or free slot has an explicitly recorded reason (no suitable slice, dependency bottleneck, quota window, blocked or preserved slot, unavailable launcher).
- Keep the lead's reserved scope explicit: specification, consequential decisions, integration, acceptance, merge and over-capability slices stay with the lead; executor-suitable routine work MUST NOT be retained by the lead while configured capacity idles without a recorded reason.
- Add utilization checkpoints to the lead cycle: session start, stage or epic planning, after each dispatch decision, and after each acceptance or slot release (backfill the freed capacity with the next worthwhile slice before moving on).
- Make utilization observable: lead progress reporting states executor occupancy against the configured concurrency limit and the recorded reason for every idle or free slot, derived from board and executor-pool records rather than window polling.
- Preserve the honest escape hatch: no manufactured work, no delegation-count target, and correctness, completion and configured quota pacing retain precedence. Ordinary non-lead sessions and the portable sole-developer default are unchanged.
- Strengthen the delivering surfaces: `team-lead` skill instructions, the agent-delegation guide, and one sentence in the portable principles stating that delegated workers are not left idle without a recorded reason.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `agent-delegation`: the economical-delegation requirement gains an executor-utilization duty - worthwhile executor-suitable work is dispatched while configured capacity is available, idle capacity requires a recorded reason, the lead does not retain executor-suitable routine work while capacity idles without that reason, and no manufactured work or delegation-count target is introduced.
- `lead-agent-orchestration`: the lead workflow gains utilization checkpoints at session start, planning, after dispatch decisions and after acceptance or slot release, plus observable idle-state reporting in lead status through board and pool records.

## Impact

- Skills: `.agents/skills/team-lead/SKILL.md` (lead duties and cycle).
- Documentation: `docs/agent-delegation.md` (utilization discipline owner), `global/principles-of-work.md` (one added principle sentence).
- Specifications: delta specs for `agent-delegation` and `lead-agent-orchestration` in this change.
- No runtime code change is expected: occupancy evidence comes from existing board records and `codex-harness executor pool`.
- Delivery follows the kit installation lifecycle, with skill activation verified outside this checkout.
