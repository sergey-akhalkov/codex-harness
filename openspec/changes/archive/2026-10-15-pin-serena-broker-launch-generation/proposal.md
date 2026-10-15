# Proposal

## Why

A long-lived Serena client caches its broker source digest but launches replacement brokers through the mutable installed manager link. Updating that link can make a later cold request launch a different build, which exits before endpoint readiness because its source digest differs.

## What Changes

- Retain the immutable manager path used to compute a client's broker identity and use it for every broker launch in that session.
- Exercise an installed-link update and broker restart inside one real MCP conversation.
- Deliver the correction through the existing immutable build lifecycle and verify the affected consumer operation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `global-code-tools`: open Serena sessions preserve their manager generation when the installed link changes.

## Impact

The shared Serena client, its real stdio integration tests and operational documentation. No new dependency, language selection change or consumer-source change.
