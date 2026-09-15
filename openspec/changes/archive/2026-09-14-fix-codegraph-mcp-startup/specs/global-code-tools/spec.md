## ADDED Requirements

### Requirement: CodeGraph MCP starts with Codex CLI
Owned CodeGraph SHALL become ready during Codex CLI startup through the installed registration, together with the other retained MCP servers. The registered command MUST remain able to complete MCP initialize when its native binaries still match their recorded hashes, even if the linked checkout later differs from that build. A manager that is admitted only for integrity-checked management MUST NOT be the Codex MCP command unless it is also admitted for serving. Ordinary Codex startup MUST NOT download packages, rebuild native source, alter agent instructions, install hooks or enable telemetry. Unrelated MCP registrations, user edits, published CodeGraph/Node and existing indexes SHALL be preserved. An unavailable CodeGraph process SHALL remain explicit and MUST NOT disable Serena, Graphify or Nuphus.

#### Scenario: Codex CLI starts after later source edits
- **WHEN** a consumer starts Codex CLI through the installed launcher after the linked checkout has changed relative to the recorded native build, and the recorded CodeGraph binaries still match
- **THEN** CodeGraph completes MCP initialize and is present in the session catalogue with the other retained MCP servers, without a rebuild or extra per-session setup

#### Scenario: Interactive, non-interactive and child sessions share the same host settings
- **WHEN** interactive, non-interactive, resumed/forked or tool-capable child consumers load the same installed host configuration
- **THEN** each session gets a usable CodeGraph MCP connection for its current project root, or an explicit CodeGraph startup error that does not omit the other retained servers

#### Scenario: Native binaries no longer match the recorded build
- **WHEN** the registered CodeGraph command is missing, hash-mismatched or otherwise not admitted for serving
- **THEN** Codex CLI still starts, CodeGraph is omitted with an explicit startup failure, and Serena, Graphify and Nuphus remain available

### Requirement: Check reports CodeGraph serving admission
Check SHALL inspect owned CodeGraph without installing, rebuilding or mutating configuration. Check MUST NOT report the owned CodeGraph registration as connected when the registered command cannot complete MCP initialize. A source-stale checkout whose recorded binaries still match MAY be reported as degraded for rebuild awareness while remaining callable. Interrupted registration recovery, unrelated settings and user edits SHALL stay preserved.

#### Scenario: Check sees a command that cannot handshake
- **WHEN** Check inspects an owned CodeGraph registration whose command exits before MCP initialize
- **THEN** the result is degraded, configuration is unchanged, and the reason identifies that CodeGraph cannot serve

#### Scenario: Check sees a source-stale but serving-admitted command
- **WHEN** Check inspects an owned CodeGraph registration whose binaries match and that still completes MCP initialize after later source edits
- **THEN** it does not report the registration as connected-without-qualification if a rebuild is needed to pick up native adapter changes, and it does not disable serving
