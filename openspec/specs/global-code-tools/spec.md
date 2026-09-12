# Global Code Tools Specification
## Purpose
Make the accepted MCP and language capabilities usable from globally configured Codex sessions in other projects, with correct workspace context, explicit retained or retired selection, and evidence for each delivered operation.
## Requirements

### Requirement: Four globally available MCP integrations

Serena, CodeGraph from `colbymchenry/codegraph`, Graphify using graphifyy, and Nuphus SHALL be the managed global capabilities after replacement acceptance passes, subject to applicable user overrides. Codebase Memory SHALL be retired from the managed selection without deleting unrelated installations or existing indexes. The accepted subscription-efficiency selection SHALL additionally be allowed to disable an LSP-bearing integration when its language functions fail the benefit gate, including Serena or harness-lsp; unrelated useful MCP functions and external installations SHALL be preserved. Retained integrations SHALL be discoverable without per-project configuration, manual startup or a special working directory in supported interactive, non-interactive, resumed/forked and subagent consumers. Desktop/IDE coverage SHALL require actual configuration-consumer evidence rather than a CLI-only claim. Existing processes needing registration reload SHALL be identified explicitly.

#### Scenario: Codex starts in an unrelated project
- **WHEN** Codex starts outside the checkout after replacement activation
- **THEN** the retained MCP capabilities are available, CodeGraph selects the intended root, and retired CBM and LSP registrations do not reappear

#### Scenario: Several Codex CLI projects are open together
- **WHEN** separate Codex CLI sessions start in different indexed projects through the installed global configuration
- **THEN** every session has usable CodeGraph operations and automatic refresh for its own root, without extra per-project setup, selecting one globally exclusive project or closing an earlier session; bounded resource contention is explicit and does not permanently disable later sessions

#### Scenario: LSP carrier is retired
- **WHEN** all useful operations of a managed LSP carrier fail the benefit gate
- **THEN** its Codex registration can be removed while unrelated tools, packages and other applications' settings are preserved

#### Scenario: A different session entry point is used
- **WHEN** non-interactive, resumed/forked, tool-capable child or supported local desktop/IDE consumers load the same host settings
- **THEN** they discover the accepted global selection with the applicable project context, preserve intentional native overrides, and do not restore retired registrations

#### Scenario: Replacement acceptance fails
- **WHEN** a required resource, retrieval, coverage or lifecycle check fails
- **THEN** the previous managed configuration remains recoverable, the failed replacement is not declared active, and the missing behavior remains open

### Requirement: MCP operations preserve useful source capabilities

Each retained integration SHALL preserve its accepted explicit source operations: suitable Serena navigation, references and semantic edits; CodeGraph deliberate full indexing and bounded automatic/manual incremental refresh, bounded symbol search, caller/callee and impact queries, and deliberate context exploration; Graphify selected-graph queries and supported repository operations; Nuphus authorized desktop/browser inspection and interaction. Diagnostic retention SHALL follow the subscription-efficiency assessment and SHALL distinguish a returned empty object from authoritative current completion. Tool filtering and unavailable operations SHALL remain explicit. Representative actual calls, rather than a tool list, SHALL establish delivered operation scope. Acceptance mutations SHALL use owned targets. CodeGraph's approximate edges SHALL be treated as candidates; required exact-reference and refactoring claims SHALL be checked with current Serena or source evidence.

#### Scenario: Each MCP is exercised by its consumer
- **WHEN** acceptance performs a meaningful read and applicable bounded mutation through each retained MCP
- **THEN** the actual result or owned-target effect and server identity are recorded, with diagnostic uncertainty preserved

#### Scenario: A required MCP operation fails
- **WHEN** discovery succeeds but an operation selected for delivery fails
- **THEN** that operation remains unverified and its original cause is retained; an unrelated passing call does not establish its support

#### Scenario: Index excludes a maintained input
- **WHEN** a needed source, configuration or specification file is unsupported, oversized, ignored or only partly extracted
- **THEN** coverage reports that limit and the appropriate current semantic or text tool remains usable; a successful exit code does not certify complete source coverage

### Requirement: Rust-owned CodeGraph integration

All new maintained first-party executable CodeGraph integration SHALL be implemented in Rust, including the MCP adapter, process supervision, refresh coordination, resource/storage limits, catalogue and response shaping, dependency/registration/recovery logic, tests and executable acceptance fixtures. Owned Python, JavaScript/TypeScript, PowerShell or C# bridges, including embedded or generated programs, MUST NOT implement this integration. Declarative configuration, documentation and inert language-analysis samples SHALL remain distinct from executable integration code.

The verified published CodeGraph implementation and its bundled Node runtime SHALL remain third-party dependencies with recorded provenance; this requirement SHALL NOT authorize an upstream rewrite or fork. Existing transitional lifecycle entry points MAY dispatch to native commands until the broader Rust migration replaces those entry points, but new CodeGraph-specific executable logic SHALL reside in Rust. This replacement SHALL NOT require completion of the entire native lifecycle migration before activation.

#### Scenario: Integration language is checked before activation
- **WHEN** the replacement is reviewed for activation
- **THEN** source ownership, generated/embedded programs, command registrations and the exercised process chain establish that all new first-party integration and executable acceptance code is Rust, while the published CodeGraph/Node process is recorded as external; a first-party foreign-language implementation leaves acceptance incomplete

#### Scenario: Transitional lifecycle invokes the provider
- **WHEN** an existing lifecycle entry point connects or recovers CodeGraph before full native cutover
- **THEN** it dispatches to the same Rust implementation without adding CodeGraph-specific executable logic in another language or creating a second provider implementation

### Requirement: Reproducible and reversible CodeGraph delivery

The kit SHALL adopt or stage a verified published CodeGraph package with recorded version and artifact identity through its existing dependency and registration lifecycle. Ordinary MCP startup SHALL NOT download packages, build upstream source, alter agent instructions, install hooks or enable telemetry. Check SHALL inspect the selected tool without installing it. Install, Update, Recover and Disconnect SHALL preserve unrelated registrations, user edits, shared packages and product sources. An interrupted replacement SHALL retain enough owned state to restore the last usable selection. The tool catalogue and initialization instructions SHALL accurately describe the delivered filtered tools and bounded automatic-refresh policy.

#### Scenario: Update is interrupted
- **WHEN** replacement activation stops between package selection and registration commit
- **THEN** Recover restores a coherent owned configuration, preserves later user changes and does not leave both providers indexing automatically

#### Scenario: Existing project changes between client requests
- **WHEN** a client switches canonical project root or the indexed source changes
- **THEN** subsequent requests cannot use another client's project, inherited context deduplication or an unverified stale index as current evidence

#### Scenario: Rust migration consumes the graph replacement
- **WHEN** the broader native migration integrates the graph provider
- **THEN** it adopts the same first-party Rust CodeGraph adapter and applicable acceptance evidence owned by this replacement, does not restart a CBM port or rewrite the third-party CodeGraph runtime, and does not make this replacement depend on completion of the entire native migration

#### Scenario: Upstream instructions promote unrestricted exploration
- **WHEN** the published server supplies broad retrieval or automatic-freshness guidance inconsistent with the managed entry point
- **THEN** the delivered initialization guidance describes the actual bounded selection, refresh limits and pending/stale state without concealing server identity or protocol errors

### Requirement: Selected languages

The support matrix SHALL record each candidate language and the accepted selection separately: retained explicit operations, retained automatic diagnostics, disabled, unavailable or retired. Rust, TypeScript, JavaScript, PowerShell, Python, Delphi, C++, C#, JSON, Markdown, TOML, XML, CMake, Bash and conditional YAML/QML/HTML/CSS are evaluation candidates rather than mandatory LSP installations. Any or all LSP support SHALL be allowed to remain disabled or be retired when its outcome benefit is absent or unproven.

Each retained operation SHALL have actual backend evidence and correct project inputs; an extension or installed package alone SHALL NOT count as working support. Ambiguous extensions such as Qt XML `.ts` SHALL use actual content/project identity. Supported definition, references, symbol discovery and other navigation operations SHALL be distinguished from unavailable ones. Automatic diagnostics SHALL obey the strict creation/content-modification-only contract and SHALL NOT become required merely because explicit navigation was retained. Equivalent alternatives SHALL NOT cause redundant installations. Unrelated pre-existing packages SHALL be preserved.

Serena MAY be the shared provider for accepted diagnostic operations as well as
navigation. The matrix SHALL distinguish the exposed interface from its underlying
language server and record the actual provider; using Serena SHALL NOT be reported
as eliminating LSP when its configured backend is LSP. A separate harness adapter
SHALL NOT remain mandatory when the accepted shared provider covers its useful
operations and passes the same freshness, isolation and delivery checks.

#### Scenario: Shared Serena replaces an overlapping harness provider
- **WHEN** retained operations pass their acceptance checks through the existing Serena service
- **THEN** duplicate managed diagnostic registrations/process ownership may be retired while useful Serena operations and unrelated consumers are preserved

#### Scenario: Explicit navigation is useful but automatic diagnostics are not
- **WHEN** only navigation passes the benefit gate
- **THEN** navigation remains explicitly callable and no automatic diagnostic handler is activated

#### Scenario: No language server is selected
- **WHEN** all LSP candidates are rejected or inconclusive
- **THEN** the matrix reports that outcome honestly and the kit completes with project-native verification rather than compulsory LSP provisioning

#### Scenario: A language is declared delivered
- **WHEN** a retained operation is marked available
- **THEN** its backend, version, inputs, result and limits have representative evidence

#### Scenario: A multi-language project has no Codex-specific setup
- **WHEN** a consumer opens a normally configured project containing retained languages and available dependencies
- **THEN** retained explicit operations use the correct roots and inputs without local kit configuration, while any accepted automatic diagnostics remain restricted to proven supported-file creation/content changes

#### Scenario: The source kit supports an additional language
- **WHEN** discovery encounters a backend outside the accepted language selection
- **THEN** it is not provisioned or updated merely because the source kit supports it, and an unrelated existing installation is preserved

### Requirement: Delphi support is verified on Delphi source

If Delphi LSP operations are retained, they SHALL use Delphi-appropriate syntax, project options, unit/include paths and SDK information. Free Pascal/Lazarus results alone SHALL NOT establish Delphi support, and source or compiler settings SHALL NOT be rewritten to obtain a passing check. Retired Delphi LSP SHALL be reported as intentionally absent rather than an incomplete compulsory installation.

#### Scenario: A Delphi project has multiple units and include paths
- **WHEN** a retained Delphi operation is exercised on representative Delphi units with their declared compiler inputs
- **THEN** evidence records the dialect and limitations, verifies promised cross-unit navigation, and verifies error/correction when diagnostics are retained

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

Acceptance SHALL exercise the selected configuration through actual installed Codex consumers outside this checkout. It SHALL cover retained operations, intentional absence of retired hooks/LSP, applicable entry points, concurrent-root isolation and selected install/update/recovery paths. Component versions, source identities, inputs, outputs and substitute-environment limits SHALL be recorded. Automatic diagnostic evidence SHALL be required only for capabilities passing the benefit gate, including every strict no-trigger scenario. Unrelated OpenCode configuration SHALL be preserved; additional opencode-kit runs SHALL NOT be required. Rejected capabilities SHALL NOT be misrepresented as verified support or force unbounded further evaluation.

#### Scenario: Global delivery is declared complete
- **WHEN** implementation is marked complete
- **THEN** all accepted operations and lifecycle requirements have evidence, rejected capabilities remain absent, and no required task is unchecked
