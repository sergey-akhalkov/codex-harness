## Purpose

Make useful MCP capabilities the normal choice for matching tasks across globally configured projects and child agents, while preserving accurate results and bounded context use.

## ADDED Requirements

### Requirement: Global task-specific preference

Global agent instructions SHALL prefer available MCP capabilities for structural code discovery, symbol operations, selected knowledge graphs, diagnostics and authorized browser or desktop work. They SHALL retain native tools for literal text, small line edits and execution. They MUST NOT require unrelated MCP calls on every turn.

#### Scenario: Structural code task
- **WHEN** a parent or child agent needs code definitions, references or call relationships
- **THEN** it uses an applicable semantic or graph MCP after verifying project context, or states the concrete coverage or availability reason for a scoped fallback

#### Scenario: Literal documentation change
- **WHEN** the task needs a known document or literal string
- **THEN** native text tools remain appropriate without mandatory graph setup

### Requirement: Verified prerequisites and honest fallback

Instructions SHALL require correct project/index/graph identity, relevant freshness and coverage checks, and bounded outputs. Missing indices SHALL be initialized when useful within the authorized repository scope. Unavailable operations, absent document sessions and unsupported languages MUST remain explicit; graph absence MUST NOT establish source absence.

#### Scenario: Selected graph belongs elsewhere
- **WHEN** a saved graph is available but its relationship to the task has not been established
- **THEN** the agent selects a relevant graph explicitly or uses another applicable tool without attributing that graph to the current project

### Requirement: Practical parent and child evidence

Acceptance SHALL exercise every MCP integration available in the implementation session with representative safe operations and record observable effects, usefulness and limitations. It SHALL include actual calls from a named tool-capable subagent. Destructive or external mutations SHALL NOT be required; mutating code and browser checks SHALL use owned fixtures.

#### Scenario: Connector lacks an active document
- **WHEN** session discovery succeeds but no connected document exists
- **THEN** the report records discovery as verified and document operations as unexercised

### Requirement: Portable activation and preservation

The policy SHALL be maintained in the repository's linked global instruction source and verified in native prompt input from two outside directories, including a repository with local instructions. Existing instruction hierarchy, unrelated edits, credentials and services SHALL be preserved. Evidence SHALL distinguish policy guidance from enforced scheduling and measured savings from qualitative judgments.

#### Scenario: New external project session
- **WHEN** ordinary Codex starts outside the kit after the source update
- **THEN** its initial instructions contain the MCP preference without a project-local MCP registration or manual file read
