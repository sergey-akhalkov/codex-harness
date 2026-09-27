## Why

The Serena stdio client capped broker startup, relocation and recovery at 30
seconds while a forwarded request may wait 240 seconds. On supported Windows
hosts a cold broker takes roughly 20 seconds from process start to endpoint
publication and the first worker's language-server activation takes another
20 to 110 seconds. After the shared broker exits - for example after a kit
delivery or an idle shutdown - the first Serena call of a session therefore
failed with `broker HTTP deadline expired` while the worker was still starting,
and the user-visible symptom was that Serena stopped answering or answered
only after minutes. The explicit `codex-harness mcp broker-retire` command had
the same defect with an 8-second budget and expired against a broker that was
still publishing its endpoint.

## What Changes

Bound broker startup, relocation and recovery in
`crates/harness-core/src/serena_broker.rs` by the same 240-second control
budget as a forwarded request instead of a fixed 30-second cap.
Let explicit broker retirement observe a cold or busy owned broker for up to
60 seconds instead of 8 seconds in `crates/codex-harness/src/mcp_cli.rs`.
Keep the broker's own request timeout, worker idle policy, resource bounds and
the Serena worker command line unchanged.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-code-tools`: cold Serena broker startup and recovery is admitted
  within the forwarded request budget instead of a shorter fixed control cap.

## Impact

Primary owners are the Serena broker client and the explicit broker lifecycle
command, with their Rust checks. Slow cold starts remain slow; this change
keeps the waiting call alive to receive the worker's answer instead of
converting a slow start into a failed request. Repository-local cargo
fingerprint repair (one `cargo check --workspace --all-targets --locked` run)
removes the pathological rust-analyzer recheck that made cold starts minutes
long on this checkout; that is host state, not delivered code.
