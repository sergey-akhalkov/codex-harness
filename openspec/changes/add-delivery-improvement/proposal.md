# Proposal

## Why

During sustained development, leads and executors concentrate on their assignments, so recurring corrections and avoidable work are captured late and improvement tasks can remain unassigned. A separately focused agent should turn observed friction into evidenced, scheduled and verified improvements while the user's work continues.

## What Changes

- Add an explicitly invoked `delivery-improvement` skill that starts one visible improvement session for the active lead and its work. It observes both lead and executor activity without becoming another lead or spawning workers.
- Analyze bounded batches of new messages and meaningful work events, including `executor message`, clarification exchanges, verification failures and repeated preparation. Reuse native observation, delivery and recovery owners; idle observation makes no model calls.
- Diagnose the cause before prescribing a remedy. Prefer removing unnecessary work or correcting the owning interface/check over accumulating instructions. Lead, executor, shared and improvement-agent guidance are eligible targets.
- Keep beads as the task and decision owner. Deduplicate by underlying evidence, admit confirmed bounded improvements without waiting for three votes while this mode is active, and retain the ordinary incubator for unverified ideas.
- Require the lead to decide at safe boundaries and dispatch accepted, ready improvements at the next suitable allocation opportunity, or record a concrete deferral and return condition. Permit one ordinary improvement implementation at a time within existing executor capacity.
- Cover improvements in the consuming project and the canonical shared kit, with public-safe kit tasks, existing specification/skill-evaluation requirements, verified installation and explicit evidence of consumption. Activation does not authorize unrelated work or silently change active sessions.
- Complete the loop by checking the relevant user path and recording supported, adverse or inconclusive effects. Preserve the separate experiment controller for hypotheses requiring comparisons rather than imposing its entire workflow on every correction.

## Capabilities

### New Capabilities

- `delivery-improvement`: optional independent observation, bounded diagnosis, lead decision and scheduling, result verification, lifecycle recovery and globally usable delivery.

### Modified Capabilities

- `orchestration-feedback-loop`: allow the designated observer to prepare bounded feedback, prevent derived observations from inflating votes, and add explicit evidence-based admission under the activated mode while preserving ordinary routing and evaluation boundaries.

## Impact

Extend the existing Rust session/control/observation and feedback owners, role resolution, orchestration configuration and installation lifecycle. Add the owned skill and improvement-role instructions; update `team-lead`, `board-workflow` and their existing operational guides. Reuse native tests and real installed consumer acceptance.

Integrate with the role resolver planned in `separate-lead-executor-contexts`; coordinate checkout allocation with `unify-reusable-worktree-pool`. Those changes remain separate owners with unfinished acceptance. No new task tracker, provider, generic agent framework, transcript warehouse or edits to protected OpenSpec workflows are proposed. This change contains planning artifacts only.
