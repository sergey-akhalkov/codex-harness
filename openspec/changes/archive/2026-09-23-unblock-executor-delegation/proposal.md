## Why

A lead session treated the wording conflict between "explicit per-assignment
model/effort selection" and profile-fixed executor dispatch as a dispatch
blocker, continued solo, and reported executors as effectively blocked even
though the launcher, the configured profile and the pool were healthy. The
kit's executor is deliberately the single configured `ds` profile (DeepSeek
V4.1-Flash at `max`); that restriction must read as normal routing, never as
a reason to withhold delegation.

## What Changes

- Executor dispatch instructions state that the configured profile already is
  the complete explicit model/effort selection: per-assignment model/effort
  arguments belong to ordinary in-session agents only, and their absence from
  `executor spawn` is not a rule conflict.
- The single configured executor profile (`ds`, DeepSeek V4.1-Flash, `max`) is
  documented as full delegation capacity; no substitute model, effort or
  profile is dispatched, and routing preferences never override it.
- Portable principles make executor delegation the default for
  executor-suitable slices of assigned work whenever executors are configured,
  requested or active; solo or sequential execution needs a recorded concrete
  reason, and instruction wording alone never withholds delegation.
- A user request to use executors for the current work (including asking why
  they are unused) activates the lead role for that work without naming the
  `team-lead` skill.
- Only a launcher- or installation-check-reported dispatch failure blocks
  dispatch; it is reported with its exact cause and remedy instead of silent
  solo continuation.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `agent-delegation`: add the profile-fixed executor routing rule - the
  configured profile is the explicit selection, a single executor profile is
  full capacity, routing wording never justifies withholding delegation, and
  only verified dispatch failures block.
- `lead-agent-orchestration`: a user request to use executors for the current
  work activates the lead role for that work without the skill name.
- `global-working-principles`: assigned work delegates executor-suitable
  slices by default, and no instruction wording withholds delegation.

## Impact

- `global/principles-of-work.md` (live global instructions through the host
  `AGENTS.md` link), `docs/agent-delegation.md`, `docs/global-instructions.md`
  and `.agents/skills/team-lead/SKILL.md` (live-linked skill).
- No launcher code or configuration change: `global/orchestration.toml`
  already configures `executor_profiles = ["ds"]`, and the profile already
  binds DeepSeek V4.1-Flash with `max` reasoning effort.
- The principles document is already above its 24 KiB spec floor before this
  change; this change records that pre-existing violation instead of hiding
  it, and keeps its own additions bounded (see design).
