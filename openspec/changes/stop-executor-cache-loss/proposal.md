## Why

A long DeepSeek executor can repeatedly pay for its whole accumulated input when previously effective provider caching disappears. Recorded usage established repeated large misses, but does not establish their upstream cause; the user chose automatic interruption with preserved work instead of continued spending.

## What Changes

- Detect sustained large cache misses after observed cache warmup in observed DeepSeek executor runs, including exact-session continuation.
- Stop the owned run, retain its checkout and session, and report the usage evidence and explicit continuation remedy through the existing receipt and terminal.
- Let the lead restart a fresh conversation in the same preserved slot, carrying the original assignment and bounded visible progress so completed work survives without replaying the costly old context.
- Read appended usage after Windows/native events, with a one-second open-file size check covering delayed filesystem notifications and no model keepalive.
- Keep cold starts, isolated misses, duplicate observations and other providers from triggering this policy.
- Deliver the checked implementation through the existing global installation lifecycle without paid verification calls.

## Capabilities

### New Capabilities

### Modified Capabilities

- `agent-delegation`: observed DeepSeek executors stop on sustained cache loss while preserving partial work.

## Impact

Executor observation/control, their existing Rust fixtures and tests, and the delegation operating guide. No new dependencies, model routing changes, periodic model messages, or claim to repair the provider cache.
