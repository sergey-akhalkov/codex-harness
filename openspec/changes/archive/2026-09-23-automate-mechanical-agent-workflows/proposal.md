## Why

Agents repeatedly reconstruct mechanical facts, format bookkeeping commands and filter oversized reports. Some rules already have Rust implementations but no installed entry point; others, including the existing instruction-size limit, are not mechanically checked. Move these operations into the existing owners while retaining engineering judgment and measuring only supported benefits.

## What Changes

- Enforce the existing 24 KiB principles limit and declared report-owner paths in native source checks; shorten principles without weakening their requirements.
- Add bounded token-audit summaries and durable full-report detail access while preserving existing complete JSON and baseline contracts.
- Add structured executor assignments with validated existing inputs, explicit new output paths, and generated checkout/base context before model launch; preserve free-text dispatch.
- Extend harness-observe to record explicit verification inputs, executable identity and before/after freshness together with actual process outcomes, without automatically accepting or caching a test pass.
- Expose existing feedback bookkeeping through an installed CLI over bd, preserving agent-selected semantic grouping, repeat protection and explicit partial failures.
- Update owning documentation and skills to use the commands, verify outside-checkout consumption, and deploy through the normal immutable-build lifecycle.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-working-principles`: executable enforcement of instruction and owner-route invariants.
- `token-audit`: bounded summaries with retained full detail and unchanged full machine output.
- `lead-agent-orchestration`: validated structured dispatch briefs.
- `project-verification`: native recording of execution identity and selected input freshness.
- `orchestration-feedback-loop`: installed command access to existing bookkeeping algorithms.

## Impact

Existing Rust workspace crates, their native CLI tests, portable principles and owning workflow documentation. No new dependency, scheduler, model call, mandatory hook, automatic acceptance or external publication. Local evidence remains outside shared source. Global installation remains the supported delivery boundary.
