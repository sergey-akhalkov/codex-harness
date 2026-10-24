# Proposal

## Why

An installed DeepSeek executor thread can become permanently unusable when
Codex inserts an internal message or reasoning item between a tool call and its
output. DeepSeek's Responses endpoint accepts the matching output but not that
ordering, so every later turn repeats `No tool output found for tool call`.

## What Changes

- Extend the existing loopback Responses compatibility shim with a DeepSeek
  route that places a tool-call batch and its matching outputs before any
  items Codex inserted between them.
- Start that route for the configured `ds` profile and direct its provider
  `base_url` through the owned loopback endpoint.
- Preserve request content, authorization handling, the xAI route and the
  existing shim lifecycle. Do not patch or replace the installed Codex CLI.
- Allow a thread that already contains the rejected ordering to continue after
  restarting through the corrected transport; no rollout files are rewritten.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `agent-delegation`: DeepSeek executor transport preserves Responses
  tool-call pairing across inserted non-tool items.

## Impact

The implementation owners are the existing Responses shim and native launcher,
with focused Rust tests and the delegation/installation guides. No provider,
credential store, executor model or xAI request adaptation changes.
