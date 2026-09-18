## ADDED Requirements

### Requirement: Serena and Nuphus MCP start with Codex CLI

Owned Serena and Nuphus SHALL become ready during Codex CLI startup through
the installed registrations, together with CodeGraph. The registered command
MUST remain able to complete MCP initialize for those servers when its native
binaries still match their recorded hashes, even if the linked checkout later
differs from that build. A manager that is admitted only for integrity-checked
management MUST NOT be the Codex MCP command for Serena or Nuphus unless it is
also admitted for serving. Source-consuming MCP, including `mcp codebase-memory`,
MUST remain gated on a healthy source match. Ordinary Codex startup MUST NOT
download packages, rebuild native source, alter agent instructions, install
hooks or enable telemetry. Unrelated MCP registrations, user edits and the
adopted Serena and audited Nuphus packages SHALL be preserved. An unavailable
Serena or Nuphus process SHALL remain explicit and MUST NOT disable the other
retained MCP servers.

#### Scenario: Codex CLI starts after later source edits
- **WHEN** a consumer starts Codex CLI through the installed launcher after the linked checkout has changed relative to the recorded native build, and the recorded binaries still match
- **THEN** Serena and Nuphus complete MCP initialize and are present in the session catalogue with CodeGraph, without a rebuild or extra per-session setup

#### Scenario: Interactive, non-interactive and child sessions share the same host settings
- **WHEN** interactive, non-interactive, resumed/forked or tool-capable child consumers load the same installed host configuration
- **THEN** each session gets usable Serena and Nuphus MCP connections, or an explicit startup error for the failed server that does not omit the other retained servers

#### Scenario: Source-consuming MCP stays gated
- **WHEN** the same source-stale manager with matching hashes is asked to serve `mcp codebase-memory`
- **THEN** that command is refused, Serena and Nuphus remain callable, and ordinary Codex startup does not rebuild native source

### Requirement: Serena stdio answers each client request with its own id

The Serena stdio proxy SHALL return every response under the identity of the
request that opened it, including the cached shared-worker initialize result,
so an MCP client MUST NOT observe a conflicting initialize response id. Broker
or worker exchange identities SHALL remain internal to the proxy.

#### Scenario: Initialize against a shared worker

- **WHEN** an MCP client sends `initialize` with one request id and the shared
  worker answers from an earlier session
- **THEN** the proxy replies with the client's own id and the initialize result,
  and the client completes the handshake instead of reporting a conflicting
  response id
