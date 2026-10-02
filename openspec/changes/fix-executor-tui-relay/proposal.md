## Why

Managed executor windows can fall behind the running conversation and reconnect for minutes. The frontend relay waits for input before forwarding each server event; an idle user therefore throttles a streaming conversation. Reports span Codex 0.157.1 and 0.159.3, with worse visible stalls after the upgrade; version causality is not yet established.

## What Changes

- Drain ready traffic in both directions without a per-event wait on the idle peer.
- Preserve authenticated attachment, byte order, warnings, bounded shutdown and exact-session reconnect without replaying work.
- Verify burst delivery and reconnect, qualify the installed native frontend, and deliver through the existing installation lifecycle.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `agent-delegation`: responsive managed executor presentation while the user is idle and after reconnect.

## Impact

The existing Rust frontend warning relay, its integration checks, and operational guidance. No model/provider change or new dependency.
