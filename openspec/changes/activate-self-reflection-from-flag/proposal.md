# Proposal

## Why

`self_reflection_enabled = true` currently leaves reflection inactive until a separate project activation command. The flag must be the only ordinary activation control; the harness should prepare its existing project and lead binding itself.

## What Changes

- Automatically prepare the native journal and reflection binding in the managed lead host when the flag is true, including when it becomes true during a session.
- Preserve retained results, checkpoints, recovery, exact process ownership and the single-consumer lease across resume and verified lead succession.
- Prepare activation before ordinary acceptance closes its first eligible task so startup timing cannot drop that acceptance.
- Keep false as the stop control for new model stages, and make status and compatibility commands explain the same control.
- Deliver the change through the global installation lifecycle and exercise the installed entry point outside the kit.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `delivery-improvement`: Flag-driven automatic activation and ownership preparation, without a second ordinary enable step.

## Impact

Managed lead host, reflection lifecycle CLI, ordinary acceptance, existing native lifecycle tests, configuration comments and operating guidance. No new provider, dependency or background model.
