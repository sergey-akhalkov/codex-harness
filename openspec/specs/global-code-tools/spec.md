# Global Code Tools Specification
## Purpose
Make the accepted MCP and language capabilities usable from globally configured Codex sessions in other projects, with correct workspace context, explicit retained or retired selection, and evidence for each delivered operation.

## Requirements

### Requirement: Two globally available MCP integrations

Serena and Nuphus SHALL be the managed global capabilities, subject to
applicable user overrides. CodeGraph, Graphify and Codebase Memory SHALL be
fully retired: no managed selection, registration, first-party adapter or CLI
route survives in the kit, while an update removes any owned registration
recorded by an earlier version and shared packages, saved graphs and caches
remain inert host residue outside the kit's ownership. Retained integrations SHALL be
discoverable without per-project configuration, manual startup or a special
working directory in supported interactive, non-interactive, resumed/forked
and subagent consumers. Desktop/IDE coverage SHALL continue to require
actual configuration-consumer evidence rather than a CLI-only claim.
Existing processes needing registration reload SHALL be identified
explicitly.

#### Scenario: Codex starts in an unrelated project
- **WHEN** Codex starts outside the checkout after this change is active
- **THEN** Serena and Nuphus are available, and retired Graphify, Codebase Memory and CodeGraph registrations do not reappear

#### Scenario: Update runs over a pre-retirement installation
- **WHEN** an owned `codegraph` registration exists from an earlier version
- **THEN** Install/Update removes only that owned registration, preserves unrelated registrations and the shared package/state, and reports the retirement

#### Scenario: Several Codex CLI projects are open together
- **WHEN** separate Codex CLI sessions start in different projects through the installed global configuration
- **THEN** every session gets usable Serena operations for its own project context, without extra per-project setup or selecting one globally exclusive project; bounded resource contention is explicit

#### Scenario: A retired integration is still needed locally
- **WHEN** leftover shared packages, saved graphs, caches or account state from Graphify, Codebase Memory or CodeGraph exist on the host
- **THEN** the kit neither reads, restores nor re-registers them, and the managed selection reports only Serena and Nuphus

#### Scenario: A different session entry point is used
- **WHEN** non-interactive, resumed/forked, tool-capable child or supported local desktop/IDE consumers load the same host settings
- **THEN** they discover the accepted global selection with the applicable project context, preserve intentional native overrides, and do not restore retired registrations

#### Scenario: Activation fails
- **WHEN** a required resource, retrieval, coverage or lifecycle check fails
- **THEN** the previous managed configuration remains recoverable, the failure is not declared active, and the missing behavior remains open

### Requirement: MCP operations preserve useful source capabilities

Each retained integration SHALL preserve its accepted explicit source
operations through a bounded model-facing surface: suitable Serena
navigation, references, semantic edits and explicit diagnostics; Nuphus
authorized desktop/browser inspection and interaction. The Serena
model-facing tool list SHALL exclude memory, onboarding,
configuration-introspection tools and `search_for_pattern`; native Git
records remain the authoritative memory route, an explicit escape hatch
SHALL keep an unfiltered debugging view possible, and literal text, regex,
configuration and document search SHALL route to scoped native search
(`rg`). Diagnostic retention SHALL follow the
subscription-efficiency assessment and SHALL distinguish a returned empty
object from authoritative current completion. Tool filtering and
unavailable operations SHALL remain explicit. Representative actual calls,
rather than a tool list, SHALL establish delivered operation scope.
Acceptance mutations SHALL use owned targets. Required exact-reference and
refactoring claims SHALL be checked with current Serena or source evidence.

#### Scenario: Each MCP is exercised by its consumer
- **WHEN** acceptance performs a meaningful read and applicable bounded mutation through each retained MCP
- **THEN** the actual result or owned-target effect and server identity are recorded, with diagnostic uncertainty preserved

#### Scenario: Maintenance moves outside the model session
- **WHEN** any retired integration's host residue needs maintenance or deletion
- **THEN** it is handled outside the kit with explicit user action, and no managed MCP catalogue, registration or kit CLI route is restored

#### Scenario: Filtered Serena surface stays debuggable
- **WHEN** a debugging session needs the unfiltered Serena tool list
- **THEN** an explicit escape hatch restores it for that client without changing the default model-facing surface

#### Scenario: A required MCP operation fails
- **WHEN** discovery succeeds but an operation selected for delivery fails
- **THEN** that operation remains unverified and its original cause is retained; an unrelated passing call does not establish its support

#### Scenario: Index excludes a maintained input
- **WHEN** a needed source, configuration or specification file is unsupported, oversized, ignored or only partly extracted by any indexed or semantic route
- **THEN** coverage reports that limit and the appropriate current semantic or text tool remains usable; a successful exit code does not certify complete source coverage

#### Scenario: Literal text search stays native
- **WHEN** a session needs literal text, regex, configuration or document matches
- **THEN** the managed Serena catalogue offers no `search_for_pattern`, the call is rejected explicitly if attempted, and scoped `rg` remains the documented route

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

Project-sensitive calls, language-server state and diagnostics SHALL be bound
to a canonical project/worktree root. Concurrent sessions and subagents MUST
NOT select another project's mutable workspace or consume its diagnostic
results. Changes to project roots and user-approved additional roots SHALL be
resolved explicitly. Shared immutable installations are permitted; an
intentionally shared service SHALL retain its identity instead of being
represented as the current project's state.

#### Scenario: Two projects use the same symbol names
- **WHEN** two concurrent sessions navigate and edit their respective projects
- **THEN** each receives only the applicable project results and closing one session does not invalidate the other

#### Scenario: A worktree or nested project changes the applicable root
- **WHEN** a session operates in that root
- **THEN** the selected project, configuration, cache identity and reported file paths agree with that root
### Requirement: Complete global acceptance evidence

Acceptance SHALL exercise the selected configuration through actual installed Codex consumers outside this checkout. It SHALL cover retained operations, intentional absence of retired hooks/LSP, applicable entry points, concurrent-root isolation and selected install/update/recovery paths. Component versions, source identities, inputs, outputs and substitute-environment limits SHALL be recorded. Automatic diagnostic evidence SHALL be required only for capabilities passing the benefit gate, including every strict no-trigger scenario. Unrelated OpenCode configuration SHALL be preserved; additional runs of the original source-kit consumer SHALL NOT be required. Rejected capabilities SHALL NOT be misrepresented as verified support or force unbounded further evaluation.

#### Scenario: Global delivery is declared complete
- **WHEN** implementation is marked complete
- **THEN** all accepted operations and lifecycle requirements have evidence, rejected capabilities remain absent, and no required task is unchecked

### Requirement: Bounded visual desktop captures

Nuphus desktop and window screenshot operations SHALL deliver a local file path or a native image content block. They MUST NOT return image bytes, base64, data URLs or nested JSON text as model-visible text. A managed adapter MAY rewrite an upstream text-wrapped image into a native image block or a local file; it MUST NOT re-encode the same bytes as additional JSON text. Screenshot calls without an owned path SHALL still produce a bounded visual result or an explicit bounded refusal. Browser snapshots and element references remain the preferred web inspection path; desktop list, title and state operations remain the preferred window-identity path.

#### Scenario: Window capture without a caller path
- **WHEN** a consumer requests a desktop or window screenshot and omits a destination path
- **THEN** the delivered MCP result contains a native image block or a local file reference, not PNG or base64 inside `type: text`

#### Scenario: Capture is written to an owned path
- **WHEN** a consumer supplies an owned destination path
- **THEN** the model-visible result identifies that path and omits the image bytes

#### Scenario: Upstream wraps image bytes as JSON text
- **WHEN** the audited Nuphus executable returns a text block whose payload is image data
- **THEN** the managed adapter converts or stores that payload before it enters model context and does not emit a second nested JSON string of the same bytes

### Requirement: Serena and Nuphus MCP start with Codex CLI

Owned Serena and Nuphus SHALL become ready during Codex CLI startup through
the installed registrations. The registered command MUST remain able to
complete MCP initialize for those servers when its native binaries still
match their recorded hashes, even if the linked checkout later differs from
that build. A manager that is admitted only for integrity-checked
management MUST NOT be the Codex MCP command for Serena or Nuphus unless it
is also admitted for serving. Source-consuming MCP, including
`mcp codebase-memory`, MUST remain gated on a healthy source match.
Ordinary Codex startup MUST NOT download packages, rebuild native source,
alter agent instructions, install hooks or enable telemetry. Unrelated MCP
registrations, user edits and the adopted Serena and audited Nuphus
packages SHALL be preserved. An unavailable Serena or Nuphus process SHALL
remain explicit and MUST NOT disable the other retained MCP server.

#### Scenario: Codex CLI starts after later source edits
- **WHEN** a consumer starts Codex CLI through the installed launcher after the linked checkout has changed relative to the recorded native build, and the recorded binaries still match
- **THEN** Serena and Nuphus complete MCP initialize and are present in the session catalogue, without a rebuild or extra per-session setup

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
