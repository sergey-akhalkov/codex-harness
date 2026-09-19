## ADDED Requirements

### Requirement: MCP registrations resolve the delivered manager

The managed Codex MCP registrations for the retained servers SHALL name the
stable manager link under the installation home rather than a single frozen
build path, so a new Codex CLI session resolves the currently delivered
manager without rewriting the registration. The recorded command MUST resolve
into an integrity-verified build when the registration is written, and a
scoped code-tools operation MUST NOT pin an older manager for later sessions.
Unrelated registrations, adopted packages and running sessions SHALL remain
unchanged.

#### Scenario: New session after a manager delivery

- **WHEN** a newer verified manager build has been delivered and a new Codex
  CLI session loads the existing MCP registrations
- **THEN** its retained MCP servers start from the fresh manager through the
  stable link, with no registration rewrite required

#### Scenario: Scoped update from the previous manager

- **WHEN** a code-tools Install/Update runs from the previous manager
- **THEN** the recorded command remains the stable link, and the next session
  still resolves the delivered build instead of the older one
