## Why

The managed OpenCodex process exited unexpectedly on 2026-09-07 and its cleanup removed the conditional Grok role. The user requested automatic background recovery so an isolated crash does not require manual reconnection.

## What Changes

- Configure the existing hidden Windows scheduled service host for three restart attempts, one minute apart, after failure.
- Handle runtime exits in that foreground host; scheduler restart settings cover startup failures, which native acceptance distinguishes from an action exiting nonzero.
- Apply that policy to an installed task without stopping its running proxy, with ownership checks and recoverable state updates.
- Preserve job memory limits, private failure evidence, readiness-gated role publication, and explicit disconnect behavior.
- Verify actual scheduler recovery in isolated state and restored Grok delegation through the global launcher.

## Capabilities

### New Capabilities

- `subscription-runtime-recovery`: bounded automatic recovery and non-disruptive policy activation for the subscription runtime. Complements the existing unarchived subscription-routing change.

### Modified Capabilities

None.

## Impact

`tools/subscription-routing.psm1`, `install.ps1`, subscription lifecycle checks and operational documentation. Reuses Windows Task Scheduler and the installed OpenCodex dependency; no new daemon, credentials or paid provider.
