## Why

Installed CodeGraph startup can fail when its source-linked manager is stale.
Explicit rebuild restores startup, but installed consumer verification also
exposed Windows access-denied failures during automatic checkpoint refresh.
Users need connected projects to keep reflecting saved source changes.

## What Changes

- Restore the verified native manager and its owned global registration.
- Reproduce and correct transient Windows sharing failures during checkpoint
  publication while preserving committed data and bounded cancellation.
- Exercise the installed multi-project index, query, refresh and reopen path.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `bounded-tool-resources`: checkpoint publication must tolerate transient
  reader sharing conflicts within the existing operation deadline.

## Impact

Rust CodeGraph generation storage, its native regression tests, and the existing
installation lifecycle. No new dependency, package version, resource allowance,
or change to source/build integrity checks. Private runtime evidence stays local.
