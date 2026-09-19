## Why

Stage 1 orchestration makes parallel execution possible; without a
disciplined loop, feedback either burns tokens in chatter or evaporates.
Improvements need durable, deduplicated demand signals and an honest budget
check: nothing becomes a default just because it was suggested, and no
improvement may multiply token burn without measured quality or speed gains.

## What Changes

- Route lead/executor feedback as board triage tasks with bounded context, batch-triaged by the lead at safe boundaries instead of real-time chat between agents.
- Route each observation by kind before it competes as an incubator vote: a verified reusable procedure in owned skill scope is handed to `autonomous-skill-evolution` rather than waiting for votes; process, orchestration, requirement, tool and unclear or material changes stay in the incubator; kit-wide skill or instruction demand from consuming projects promotes to the kit backlog without private data, after which skill-evolution executes any library mutation. Promotion confers eligibility only; this change does not write skill packages.
- Add an incubator of unique improvement items: similar incoming feedback is merged into an existing item and counts as one vote from a distinct episode; a configurable vote threshold (default: more than two votes) promotes the item to the backlog.
- Route promoted items by size: small improvements become backlog tasks for the lead or an executor; material behavior changes require an OpenSpec change; items concerning kit instructions, skills or tools promote to the kit's own backlog without private consuming-project data.
- Permit an immediate lead override for material correctness, integrity or safety consequences, with a recorded reason, because votes measure frequency rather than value.
- Keep the incubator healthy with lead-owned sweeps fired by deterministic triggers - stage or epic closure during acceptance, or a triage batch finding the incubator above its configured size cap - archiving stale items with reasons instead of growing an unbounded backlog graveyard.
- Add instruction-refresh succession for orchestrated workers: a session whose instructions or skills changed spawns a successor through a deterministic `codex resume` profile session at a safe boundary after in-flight tool effects, hands over durable context, then stops its own CLI process; resumed sessions must be verified to reload current instructions and skills. Succession consumes the compact revision identity published by skill-evolution. It does not own catalogue delivery, in-process activation or same-session compact recovery, and a replaced process does not satisfy those requirements.
- Apply quota/token pacing from honest observations - native GPT limit reads where available, actual refusals and bounded dashboard snapshots elsewhere, unknown stays unknown - adjusting concurrency, effort and feedback cadence without preempting healthy workers.
- Gate adopted orchestration defaults on matched comparisons of quality and speed per token burn, including feedback-triage, coordination and rework cost. This gate is not the skill-evaluation contract for library mutations.

## Capabilities

### New Capabilities

- `orchestration-feedback-loop`: Asynchronous feedback triage with incubator deduplication, voting and promotion, consequence override, incubator hygiene, instruction-refresh succession and budget-honest pacing.

### Modified Capabilities

- `agent-delegation`: The efficiency-and-quality evidence requirement is extended into a benefit gate for feedback-driven improvements before they become defaults.
- `subscription-efficiency`: Quota-aware pacing across account windows, previously deferred from the orchestration change.

## Impact

Depends on the Stage 1 orchestration change (`orchestrate-subscription-agents`)
delivering lead/executor dispatch, board coordination and durable task state;
this change extends that loop rather than replacing it. Affected owners
include the board workflow (`beads`/`bd` non-interactive operations for
merges, votes and promotion), the task controller's succession path, the
`codex resume` contract verification, usage/limit observation owners, and the
owning delegation, efficiency and decision records. `autonomous-skill-evolution`
continues to own skill and instruction evolution, in-process catalogue
delivery and same-session compact/resume recovery; this change owns the
intake queue, observation routing, promotion, account-window pacing and
process succession that consume those updates. Private consuming-project
data stays outside kit sources.
