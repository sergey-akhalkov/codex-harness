# Harness Source Diagnostics Specification

## Purpose

Explain which harness and project sources a new Codex consumer uses, so configuration conflicts can be diagnosed without model requests or changes to user configuration.

## Requirements

### Requirement: Globally available focused Check

The kit SHALL provide `install.ps1 -Mode Check -Diagnose -ProjectPath <directory>` and an equivalent globally linked `codex-harness-check.ps1` command. Install and Update SHALL register its direct source link; Disconnect and recovery SHALL preserve the existing ownership and rollback guarantees. The existing Check without Diagnose SHALL retain its behavior. A report SHALL identify the project, selected profile, observation scope and overall status as healthy, attention or incomplete.

#### Scenario: Outside-repository consumer
- **WHEN** the installed global command runs in another project without a repository-relative command path
- **THEN** it inspects that project using the connected source and selected harness profile and returns a structured report

#### Scenario: Lifecycle and ownership
- **WHEN** a fixture installation is updated, disconnected or rolled back after an interrupted update
- **THEN** the diagnostic connection follows the same ownership rules as other managed links and unrelated files remain unchanged

### Requirement: Evidence-based sources and conflicts

The report SHALL list expected and observed managed link targets, reporting missing or retargeted links independently. It SHALL use native configuration origins and layers to identify effective settings and overrides for a documented bounded selection of settings, including model, reasoning, permissions and developer instructions. It SHALL list discovered skill identities and detect different source files declaring the same name. Disabled configuration layers and unavailable native observations SHALL be distinct from active overrides. Each finding SHALL identify a cause, affected source where known and a recovery action; diagnostics MUST NOT repair automatically.

#### Scenario: Multiple simultaneous conflicts
- **WHEN** a trusted consumer project overrides a profile setting, contains a conflicting skill name, and has a retargeted managed link
- **THEN** one report identifies all three conditions and relevant source paths with recovery advice

#### Scenario: Restore and untrusted project
- **WHEN** conflicts are removed and the project configuration is untrusted
- **THEN** the repeated report no longer reports the removed conflicts and does not call the ignored project values effective overrides

### Requirement: Private bounded inspection

Diagnostics SHALL make no model calls, run no project commands or hooks, and leave configuration, credentials, history, project files and existing services intact. Native consumer startup and requests SHALL have finite deadlines and cleanup SHALL terminate only the diagnostic-owned process tree. Reports and failure output SHALL exclude raw configuration, credentials, prompt bodies, skill descriptions and raw native error text. Only allowlisted preference values and source metadata SHALL be emitted; developer instructions SHALL expose presence and provenance only. Native runtime cache or logs created by read operations SHALL be documented separately from configuration mutations.

#### Scenario: Sensitive and invalid configuration
- **WHEN** configuration or native errors contain a sentinel secret, including invalid configuration
- **THEN** neither successful reports nor failure output contain that secret, and other independent file findings remain available

#### Scenario: Native failure or timeout
- **WHEN** the native consumer fails or exceeds its deadline
- **THEN** the report is incomplete with a bounded failure category and recovery action, and the owned process is cleaned up without stopping an existing consumer

### Requirement: Honest freshness boundary

The report SHALL distinguish fresh native observations from profile precedence reconstructed using native-parsed layers for the requested directory. It MUST NOT label reconstructed values as native effective-profile observations. Unsupported versions, managed constraints or profile changes affecting project discovery, trust or skill selection SHALL return incomplete rather than assert uncertain effective values. It SHALL state that already-running sessions and MCP/LSP server freshness are unknown unless separately observed; it MUST NOT infer loaded code freshness from file timestamps or link correctness. Documentation SHALL describe the diagnostic's limits and keep existing protocol checks available.

#### Scenario: Source changes during an existing session
- **WHEN** a source is edited and diagnostics run while another session remains open
- **THEN** the report describes the new consumer's observations and leaves the other session's loaded revision unknown with restart guidance
