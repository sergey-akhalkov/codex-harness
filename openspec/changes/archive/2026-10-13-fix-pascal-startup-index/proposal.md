# Proposal

## Why

The adopted Pascal server eagerly loads workspace copies of source files during
initialization, independently of Serena's exclusions. Accumulated build trees can
exhaust its memory before the first semantic request completes.

## What Changes

- Avoid the redundant eager Pascal workspace symbol index while retaining Serena
  symbol discovery, bodies, edits and cross-unit navigation.
- Exercise ignored build copies and real Pascal navigation through the managed
  entry point; verify recovery on the affected local consumer.
- Deliver through the existing global installation lifecycle.

## Capabilities

### Modified Capabilities

- `global-code-tools`: Pascal startup must not eagerly load excluded build copies
  merely to serve a request for working source.

## Impact

Managed Serena backend settings, native acceptance tests and the code tools guide.
No dependency installation, backend fork or Delphi compiler setting changes.
