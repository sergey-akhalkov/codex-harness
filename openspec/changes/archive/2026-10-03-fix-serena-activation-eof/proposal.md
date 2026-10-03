## Why

A Serena worker can exit during project activation. The client currently treats activation like an uncertain source edit and refuses recovery, leaving navigation unavailable. Worker failures also omit native exit and memory observations needed to distinguish resource exhaustion from other crashes.

## What Changes

- Recover an interrupted project activation once within its original deadline, preserving the selected project and the prohibition on replaying edits.
- Correct the broker's aggregate memory allowance to include its configured worker capacity while retaining the existing per-worker limits.
- Retain bounded native worker failure evidence and diagnose the observed startup failure before selecting its correction.
- Add deterministic regression coverage and exercise real semantic navigation through the delivered installation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-code-tools`: bounded activation recovery and actionable worker failure evidence.

## Impact

Native Serena session, broker, shared pool, existing Rust tests and global installation. Private project inputs and crash records remain in local storage. Language coverage and resource policy remain acceptance constraints.
