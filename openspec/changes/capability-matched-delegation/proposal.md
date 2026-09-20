## Why

Delegation guidance tells the lead to decompose work before dispatch and to
weigh "reasoning needs", but it never states who performs the pre-dispatch
analysis or how slice difficulty is matched to the configured executor
profiles. With cheaper executor routes, a slice whose reasoning demands exceed
the executor's capability produces churn - weak first output, steering loops
and lead rework - so the strongest model pays twice: once for briefing and
again for repair. Conversely, hoarding all analysis on the lead would
serialize work and burn scarce lead turns on routine investigation that
executors already own.

## What Changes

- Extend the `agent-delegation` economical-delegation requirement: before
  dispatch, the lead performs the analysis each slice needs to become
  sufficiently specified - requirement interpretation, risk and consequence
  decisions, approach direction and acceptance - and sizes every slice against
  the configured executor profile's reasoning capability.
- A slice beyond the available executor capability stays with the lead, is
  decomposed further into capability-sized slices, or uses the existing bounded
  principal-consultation path; it is never delegated as-is.
- Executors keep investigation, implementation, checks and correction within
  their assignments and capabilities - the rule targets over-difficult slices,
  not executor investigation in general.
- The `team-lead` skill's assignment guidance and the agent-selection document
  implement the same rule; orchestration configuration and executor dispatch
  mechanics stay unchanged.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `agent-delegation`: the economical-delegation requirement gains lead-owned
  pre-dispatch analysis and capability-matched slice sizing, with scenarios for
  an over-difficult slice, a sufficiently specified delegated slice and
  executor-owned bounded investigation.

## Impact

- `openspec/specs/agent-delegation/spec.md` (requirement text and scenarios,
  applied on archive).
- `.agents/skills/team-lead/SKILL.md` (`Assign and brief` guidance).
- `docs/agent-delegation.md` (selection guidance).
- No executable code, API, dependency or configuration changes; native checks
  and the installed launcher are unaffected. Documentation-only implementation
  needs source hygiene, local-link and factual checks plus OpenSpec validation.
