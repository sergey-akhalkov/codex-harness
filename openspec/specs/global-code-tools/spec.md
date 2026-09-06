# Global Code Tools Specification

## Purpose

Make the four source-kit MCP integrations and Rust, TypeScript, JavaScript, PowerShell, Python, Delphi, C++, C#, JSON, Markdown, TOML, XML, CMake and Bash usable from globally configured Codex sessions in other projects, with correct workspace context and evidence for each delivered capability.

## Requirements

### Requirement: Four globally available MCP integrations

The kit SHALL make Serena, Codebase Memory, Graphify using the graphifyy distribution, and Nuphus discoverable and callable by new ordinary local Codex sessions across projects after global activation. Registration SHALL not require per-project MCP configuration, a special working directory, manual server startup or a per-session profile selection. Availability SHALL cover interactive, non-interactive, resumed/forked sessions and tool-capable subagents under the supported native Codex configuration contract. Applicable explicit user overrides and permissions SHALL retain their native precedence. A local desktop/IDE consumer of the same Codex host MUST NOT be claimed covered solely from a CLI launcher test; its configuration loading SHALL be verified and connected as part of global delivery. Already running processes can require a documented restart to load new registrations.

#### Scenario: Codex starts in an unrelated project
- **WHEN** the ordinary globally configured Codex entry point starts a new session outside this checkout
- **THEN** all four MCP integrations initialize and their required operations are available without project-local registration

#### Scenario: A different session entry point is used
- **WHEN** a non-interactive, resumed/forked, tool-capable subagent or installed local desktop/IDE session loads the same host configuration
- **THEN** it receives the global integrations and the applicable project context, or an explicit native user override explains their intentional exclusion

### Requirement: MCP operations preserve useful source capabilities

Each integration SHALL expose its applicable source capabilities: Serena symbol navigation, references, semantic edits and explicit diagnostics; Codebase Memory project indexing and structured code queries; Graphify queries over the selected existing graph and supported repository operations; Nuphus desktop/browser inspection and interaction. Tool filtering SHALL be explicit and MUST NOT silently discard a required operation. Validation SHALL use actual MCP calls and representative results, not only an advertised tool list. Desktop/browser mutations in acceptance SHALL use a disposable test target.

#### Scenario: Each MCP is exercised by its consumer
- **WHEN** acceptance performs a meaningful read operation through each MCP and a bounded representative mutation where its capability includes writes
- **THEN** the expected source result or disposable-target effect is observed and its server/tool identity is recorded

#### Scenario: A required MCP operation fails
- **WHEN** discovery succeeds but the representative operation fails
- **THEN** that integration remains unverified and the report retains the tool's original failure

### Requirement: Selected languages

The kit SHALL provide language navigation and diagnostics for exactly the following delivery scope: Rust, TypeScript, JavaScript, PowerShell, Python, Delphi, C++, C#, JSON, Markdown, TOML, XML, CMake and Bash. The versioned support matrix SHALL map each to a selected backend, required runtime/project inputs, exposed operations, automatic diagnostic behavior and actual verification evidence. Equivalent backend alternatives do not require duplicate installations. YAML, QML, HTML and CSS SHALL be connected and verified when a ready compatible backend is available; otherwise each unavailable status SHALL be reported without making their provisioning a completion prerequisite. Other languages from the source-kit catalogue, including F#, are outside this change; existing unrelated installations SHALL be preserved. Language detection SHALL distinguish ambiguous extensions such as Qt XML translation `.ts` from TypeScript using project context or source identification. A matrix row, file extension match or installed binary alone MUST NOT count as working support.

| Language | Backend selection constraints |
| --- | --- |
| Rust | Reuse the existing rustup Rust Analyzer where compatible; preserve the project toolchain. |
| TypeScript | Reuse a compatible TypeScript language server and honor project-local TypeScript configuration. |
| JavaScript | May share the TypeScript server installation; verify JavaScript project behavior separately. |
| PowerShell | Reuse PowerShell Editor Services and its applicable analyzer. |
| Python | Select and verify one compatible Python backend; reuse existing installations and project environments. |
| Delphi | Reuse existing Pascal tooling only if it passes the Delphi-specific requirement below. |
| C++ | Reuse the existing C++ server and honor the project build configuration and include paths. |
| C# | Discover and reuse the existing installation and project SDK. |
| JSON | Select a ready server and verify schema-aware diagnostics and applicable navigation. |
| Markdown | Select a ready server and verify its actual navigation and diagnostic capabilities. |
| TOML | Verify project manifests and schema/config-aware diagnostics. |
| XML | Verify XML inputs and schemas; route Qt XML formats according to actual content/project identity. |
| CMake | Verify CMakeLists.txt and CMake module operations using project context. |
| Bash | Verify applicable shell scripts without executing them as an analysis side effect. |
| YAML / QML / HTML / CSS (conditional) | Reuse ready compatible support when available; explicitly report each absence. |

Support SHALL include symbol discovery, definition and references where the selected source backend provides them, plus explicit and automatic diagnostics. Hover/type information, implementation navigation, workspace symbols, call hierarchy, rename and other source-advertised operations SHALL be represented and tested rather than implicitly promised for servers that lack them. Source-specific limitations SHALL be explicit. Missing prerequisites or backend functionality for these required languages SHALL remain visible unmet requirements.

#### Scenario: A multi-language project has no Codex-specific setup
- **WHEN** a session opens a project using the selected languages with its normal language/SDK configuration and available dependencies
- **THEN** language selection uses the appropriate roots, servers and project inputs automatically and exposes the corresponding navigation and diagnostics

#### Scenario: The source kit supports an additional language
- **WHEN** discovery encounters a backend outside the required languages and conditional YAML/QML/HTML/CSS
- **THEN** this change neither installs nor updates it as a required dependency, and preserves any existing installation

#### Scenario: A language is declared delivered
- **WHEN** its matrix row is marked verified
- **THEN** evidence contains a representative actual server interaction and correct project result for each promised operation, including diagnostic introduction and clearance

### Requirement: Delphi support is verified on Delphi source

Delphi support SHALL resolve representative units, definitions and references and report diagnostics using Delphi-appropriate syntax, project options, unit/include paths and installed SDK information. Existing Pascal tooling SHALL be reused where it meets this contract. Successful Free Pascal/Lazarus tests alone MUST NOT establish Delphi support, and project compiler settings or source files MUST NOT be rewritten to make the language server pass.

#### Scenario: A Delphi project has multiple units and include paths
- **WHEN** the integrated backend opens a representative project with its declared compiler options
- **THEN** navigation crosses unit boundaries correctly, a known diagnostic is reported and clears after a correction, and the tested Delphi version/dialect and limitations are recorded

### Requirement: Workspace and session isolation

Project-sensitive calls, language-server state, diagnostics and indexes SHALL be bound to a canonical project/worktree root. Concurrent sessions and subagents MUST NOT select another project's mutable workspace or consume its diagnostic results. Changes to project roots and user-approved additional roots SHALL be resolved explicitly. Shared immutable installations and intentional knowledge bases are permitted; an intentionally shared graph SHALL retain its identity instead of being represented as the current project's graph.

#### Scenario: Two projects use the same symbol names
- **WHEN** two concurrent sessions navigate, index and edit their respective projects
- **THEN** each receives only the applicable project results and closing one session does not invalidate the other

#### Scenario: A worktree or nested project changes the applicable root
- **WHEN** a session operates in that root
- **THEN** the selected project, configuration, cache identity and reported file paths agree with that root

### Requirement: Graphify lifecycle and repository selection

Graphify SHALL reuse the intended graphifyy installation and preserve the existing graph data. A healthy compatible shared endpoint SHALL be reused when its authenticated connection and ownership are available; otherwise the kit SHALL provide a verified independent connection using the same installation and selected graph without depending on launching OpenCode. Secrets SHALL stay outside tracked source and ordinary logs. Repository-specific Graphify operations SHALL require an explicit resolved repository and MUST NOT infer it from the shared service's working directory.

#### Scenario: The existing shared service is stopped
- **WHEN** Codex starts in an ordinary terminal
- **THEN** Graphify becomes callable through the managed connection lifecycle without requiring OpenCode to be launched and without replacing the saved graph

#### Scenario: A repository-specific call lacks repository identity
- **WHEN** a shared Graphify repository operation has no explicit valid repository
- **THEN** it fails with a precise input error before querying or acting on an unintended repository

### Requirement: Complete global acceptance evidence

Acceptance SHALL exercise the real installed Codex consumer outside this checkout, in a neutral directory and at least two separate representative projects. It SHALL verify all four MCPs, all delivered language-matrix operations, automatic diagnostics, concurrent isolation, existing OpenCode compatibility after shared updates and a clean-host installation path. The report SHALL record component versions, entry points, inputs, outputs and environment limits. A disposable substitute for a second physical PC SHALL be identified as such. Unverified selected-language rows and failing required scenarios MUST NOT be closed or represented as completed delivery.

#### Scenario: Global delivery is declared complete
- **WHEN** the change is accepted as fully implemented
- **THEN** every required scenario and selected-language row has appropriate evidence, no required task is unchecked, and actual global activation has passed outside the harness
