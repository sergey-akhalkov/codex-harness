# Proposal

## Why

Lead sessions and executors currently receive a growing shared instruction surface even though their decision responsibilities differ. Separate, independently maintained role contexts should make execution guidance more focused and let improvements target their actual owner without duplicating common safeguards; speed and quality benefits remain hypotheses until exercised.

## What Changes

- Compose each managed session from a shared core, one role (`lead` or `executor`), applicable project guidance and its task. Move existing role-specific rules out of the common context instead of appending another prompt.
- Keep the lead responsible for requirements, consequential decisions, assignment, integration, acceptance and regression assessment. Keep executors autonomous in investigation, implementation, applicable checks and correction within their assignments, without originating agents or taking over the wider task.
- Maintain shared, lead and executor skill memberships with one canonical source per reusable skill. Apply the effective selection through native Codex configuration while preserving project and user-owned skills and explicit disables.
- Apply role context through existing managed launch and recovery paths, independently of provider/model/effort routing, and expose its effective identity in existing inspection and run state.
- Deliver through the global install/update/check/disconnect lifecycle, preserve concurrent session isolation and existing recovery, and verify actual instruction and skill consumption outside this checkout.
- Compare representative work using existing verification and feedback mechanisms. Attribute outcomes to lead guidance, executor guidance or shared tools before changing instructions; do not treat shorter prompts as proof of better work.

## Capabilities

### New Capabilities

- `agent-role-contexts`: Role-specific instruction composition, shared and role-specific skill availability, lifecycle binding and evidence for independent improvement.

### Modified Capabilities

- `global-working-principles`: Replace full undifferentiated policy loading with common-core loading plus the applicable role; retain all normative obligations in their responsible layer and align source checks with that composition.
- `agent-delegation`: Distinguish reusable responsibility roles from retired provider/effort presets while retaining configured routing, executor autonomy and acceptance ownership.

## Impact

- Instruction/configuration sources under `global/`, owned skill selection and the existing team-lead workflow; protected external skill and OpenSpec sources remain unchanged.
- Native Rust installation, launcher and orchestration configuration; executor assignment, control, continuation and recovery; existing skill catalogue, source checks and acceptance tests.
- Existing documentation owners: global instructions, agent delegation, installation, project decisions and skill evolution. Public source uses only portable configuration and synthetic acceptance inputs.
- Existing commands, routing, visible conversations, worktree ownership and task/feedback lifecycles remain compatible. This change creates no additional model route, recursive delegation tier, autonomous improvement loop or second skills library.
