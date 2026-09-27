## ADDED Requirements

### Requirement: Cold Serena broker startup stays within the request budget

The Serena stdio client SHALL bound broker startup, relocation and recovery by
the same budget class as a forwarded request rather than by a shorter fixed
control cap, so a cold broker start followed by the first worker's
language-server activation completes the waiting request instead of surfacing a
client-side broker HTTP deadline. Explicit broker retirement SHALL observe a
cold or busy owned broker for at least sixty seconds before reporting failure.

#### Scenario: First call after the shared broker exits
- **WHEN** no Serena broker is running and a session's first Serena request must start the broker and activate a worker
- **THEN** the request completes within the forwarded request budget with the worker's answer, instead of failing with `broker HTTP deadline expired`

#### Scenario: Explicit retirement during cold start
- **WHEN** `codex-harness mcp broker-retire` targets a broker that is still publishing its endpoint or activating its first worker
- **THEN** the command observes the owned service for up to sixty seconds instead of expiring while it starts
