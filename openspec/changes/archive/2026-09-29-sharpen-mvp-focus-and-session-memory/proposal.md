## Why

The main session spends its own context on transferable implementation and loses
MVP priority unless the `team-lead` role happens to be active, which forces the
user to restate focus and delegation expectations mid-work. Confirmed decisions,
priorities and deferred work that matter in future sessions are recorded too
passively, so a new session restarts without them and the user must repeat
agreements such as "do this after the MVP".

## What Changes

- Add an MVP-priority effort gate to the portable principles: before
  substantial effort (research, extra checks, refactoring, infrastructure), the
  agent classifies it as accelerating the earliest verified MVP, required by
  acceptance, or deferrable; deferrable work is recorded in its existing owner
  instead of being done now or lost.
- State the main-session posture in the portable principles: coordination,
  judgment, ideation, deep analysis, decomposition, integration and acceptance
  stay in the main session; its context and tokens are a scarce resource, and
  substantial transferable execution is delegated under the existing activation
  rules. Work cheaper than its handoff stays direct, and small tasks stay solo.
- Strengthen session-memory persistence: confirmed decisions, priorities,
  agreements and deferred work that affect future sessions are recorded in the
  existing owning record when they are decided; an explicit request to remember
  or roadmap an item causes an immediate write to the right owner. Session
  transcripts, progress logs and noise remain excluded.
- Align the `team-lead` skill operating objective with the MVP gate and lead
  context economy, and the `project-memory` skill with the decision-time
  trigger.
- Record one generalized user decision in the project decision record.
- Keep the canonical principles within a raised, need-based 25,088-byte
  limit: compensating compression first, then one explicit user-approved
  512-byte limit raise carried by the native check, its test and the spec.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `global-working-principles`: add the MVP-priority effort gate and the
  main-session posture with scarce lead context, within the existing size
  limit.
- `git-project-memory`: replace the passive recording wording with a
  decision-time persistence trigger for confirmed decisions, agreements,
  priorities and deferred work that affect future sessions, while keeping the
  existing publication and noise boundaries.

## Impact

- `global/principles-of-work.md` (delivered through the live global `AGENTS.md`
  link; already running sessions must be restarted to load new text).
- `.agents/skills/team-lead/SKILL.md` and `.agents/skills/project-memory/SKILL.md`.
- `docs/project-decisions.md` (one generalized decision; no private consumer
  identities or private roadmap items enter the shared kit).
- No executable code or dependency changes. Verification: source and link
  diagnostics, the principles size check, strict OpenSpec validation, and a
  model-free prompt-input loading check from an outside consumer.