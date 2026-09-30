## Why

A managed Serena symbol query can fail with Windows connection reset 10054.
The client does not recover transport failures, and the real Serena acceptance
tests are ignored by the existing integration command. Users need semantic
navigation to survive a transient broker failure in an existing session and
delivery checks that actually exercise the adopted server.

## What Changes

- Recover a transient broker transport failure within the original deadline
  with one bounded retry of explicitly safe reads, preserving client routing.
- Preserve uncertain edit outcomes without replay and keep later requests usable.
- Exercise real Serena over MCP, including injected transport failure, broker
  replacement, independent projects, and retained semantic edits/diagnostics.
- Make real semantic acceptance part of the delivery path and an automatic
  credential-free CI route, with missing prerequisites reported as failures.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-code-tools`: bounded session recovery and mandatory real Serena
  acceptance for delivery.

## Impact

Serena broker client, native MCP acceptance, delivery verification, Rust
integration tests, CI, and the code-tools guide. Preserve existing resource
limits, package ownership, supported languages, and unrelated sessions.
