## Why

The lead role is activated only by an explicit skill invocation or a user
request for executors, so a main session facing a well-decomposable parallel
task implements it alone unless the user remembers to ask. The confirmed
everyday behavior is different: the main session should decide orchestration
itself for such tasks, spawned executors must never widen orchestration, and
the lead's own tokens remain the scarcest resource.

## What Changes

- The main session may enter the lead role on its own judgement, without a
  user request, when the user's task decomposes into parallel, independently
  verifiable implementation slices whose orchestration cost the task repays;
  it states that activation and its basis before spawning anything.
- That autonomous decision belongs to the main session alone: executor
  sessions and ephemeral helper agents never activate the role or originate
  further agents or executors; they report the need to the lead.
- Lead economics become normative: minimize the lead's own token spend and
  the time to the accepted result; keep judgment, decomposition, integration,
  acceptance and capability-exceeding work with the lead; delegate the
  remaining parallelizable implementation work.
- Work whose delegation overhead exceeds the work itself - typically a
  one-line correction - stays direct, with no task, assignment or board note;
  no manufactured slices or delegation-count targets are introduced.

## Capabilities

### New Capabilities

### Modified Capabilities

- `lead-agent-orchestration`: lead activation may also happen autonomously by
  the main session for well-decomposable work; executor and helper widening
  stays prohibited; lead token economy and the trivial-correction exception
  become part of the activation contract.
- `global-working-principles`: the delegation-default requirement follows the
  new activation rule, keeps autonomous activation main-session-only, and
  exempts work cheaper than its own delegation overhead from solo reasons.

## Impact

- `openspec/specs/lead-agent-orchestration/spec.md` through the change delta
  and archive sync.
- `.agents/skills/team-lead/SKILL.md` description, activation section,
  operating objective and utilization exception.
- `global/principles-of-work.md` delegation paragraphs, delivered to sessions
  through the live global instructions link.
- `openspec/specs/global-working-principles/spec.md` through the change delta
  and archive sync.
- `docs/global-instructions.md`, `docs/agent-delegation.md` and
  `docs/project-decisions.md` consistency updates and the confirmed decision.
- `crates/harness-core/src/board_cli.rs` skill wording contract test.
