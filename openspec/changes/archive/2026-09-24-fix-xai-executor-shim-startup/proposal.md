## Why

Observed xAI executors pass their provider configuration to `codex app-server` as config overrides. The launcher starts the compatibility shim only for `--profile xai`, leaving executors dependent on a separately opened interactive session. In addition, a shim started by the executor's temporary shell preflight inherits its cleanup Job and is killed when that preflight ends. A missing shim produces network retries before an HTTP response exists; opening an interactive xAI session starts the missing transport.

## What Changes

- Prepare the xAI compatibility transport for the resolved executor provider before its owned app-server process starts.
- Start the shared shim through the existing independent service bootstrap so preflight or tool cleanup cannot terminate it.
- Reuse the existing shim identity, readiness and replacement lifecycle, preserving OAuth helper behavior and other providers.
- Add a deterministic regression and isolated process checks, then deliver through the installed lifecycle.

## Capabilities

### New Capabilities

### Modified Capabilities

- `subscription-model-routing`: xAI executor startup prepares its required local transport without an interactive warmup.

## Impact

The native launcher shim preparation and observed executor startup, their native tests and the subscription guide. No new dependency, credential format or provider billing route.
