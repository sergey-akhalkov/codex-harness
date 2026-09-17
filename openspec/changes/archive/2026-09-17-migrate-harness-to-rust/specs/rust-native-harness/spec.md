## Purpose

Provide the globally connected harness through maintainable first-party Rust code while preserving accepted behavior, safe migration and independently verifiable native operation.

## ADDED Requirements

### Requirement: Harness Rust ownership policy

The kit SHALL use Rust for all maintained first-party executable code in the delivered repository, including runtime, installation/recovery/build helpers, tests, executable fixtures and skill helpers. The repository's owning guidance and decision record SHALL distinguish this full migration commitment from the separately confirmed portable Rust programming and PowerShell shell defaults, which apply across projects and delegated work. Those defaults MUST NOT be interpreted as authorization for unrelated rewrites of existing projects. Third-party tools and their required runtimes SHALL remain allowed under documented external ownership and provenance. Declarative configuration, documentation and inert language-analysis samples SHALL be distinguished from executable harness code.

#### Scenario: New first-party helper
- **WHEN** a runtime, test, build or skill helper is added to the maintained kit
- **THEN** its executable implementation is Rust and its invoking documentation uses the supported native interface

#### Scenario: Foreign tool and project language
- **WHEN** the installed harness uses a third-party Python/TypeScript tool or operates in a non-Rust project
- **THEN** the external language/runtime remains supported, the portable language/shell defaults and their explicit exceptions guide new work, and the harness does not require that project's existing implementation to undergo this full Rust migration

### Requirement: Complete executable ownership accounting

Completion SHALL include a current inventory and executable ownership check covering tracked and untracked source, generated/embedded programs, manifests, supported command registrations and globally linked skill resources. Every first-party executable unit SHALL have a verified Rust replacement or a consumer-backed retirement. Hiding non-Rust implementation in strings, fixtures, an owned fork or a nominal dependency MUST NOT satisfy the ownership check. Transient caches, retired helpers and unexplained exclusions MUST NOT mask a supported execution path.

#### Scenario: Generated foreign-language program
- **WHEN** Rust code invokes an owned Python, PowerShell, JavaScript or C# program embedded in a string or generated at runtime
- **THEN** the ownership check fails and the corresponding migration work remains incomplete

#### Scenario: Inert analysis fixture
- **WHEN** a declared language sample is inspected by an external analyzer without being executed as a harness helper
- **THEN** the inventory records it as test data with its consumer and does not misclassify it as a foreign runtime implementation

#### Scenario: Source changes during migration
- **WHEN** concurrent work adds an executable helper after the baseline inventory
- **THEN** final acceptance includes that helper and does not rely on the older file count as proof of completeness

### Requirement: Native management and command compatibility

The kit SHALL expose native executable management, launcher, diagnostic, service, tool and skill-helper entry points. Ordinary `codex` usage, argument boundaries, Unicode, cwd/environment, streams, exit codes, native option precedence and accepted task-effort selection SHALL retain their supported behavior. Native lifecycle commands SHALL retain component isolation, preview, Check, explicit recovery and ownership rules. Replaced script command names SHALL have a documented native equivalent and existing managed registrations SHALL migrate without requiring the user to recreate every connection. Launcher discovery MUST distinguish the upstream command from its own executable.

Ordinary upstream CLI availability SHALL be independent of optional harness health. If harness preparation or shared build/source/configuration validation fails, the launcher SHALL use the available original CLI with native arguments and local settings, identify the degraded enhancement, and preserve command/stream/exit behavior without automatic provisioning. It SHALL NOT weaken integrity checks for executing harness extensions or retry by launching another upstream session after the first starts.

#### Scenario: Existing user's ordinary launch
- **WHEN** an upgraded user opens a fresh terminal outside the checkout and invokes `codex` with explicit native options and quoted Unicode arguments
- **THEN** the connected configuration is selected, each argument reaches the intended upstream command unchanged apart from documented harness options, and no recursive launcher invocation occurs

#### Scenario: Shared configuration cannot be prepared
- **WHEN** harness shared configuration is stale, missing or malformed and the registered original CLI remains usable
- **THEN** the original CLI starts without harness-injected defaults, receives native arguments and stdin unchanged, and its output and exit status are preserved without a second invocation

#### Scenario: Component-only operation
- **WHEN** a native lifecycle command selects only one accepted component or requests preview
- **THEN** unrelated connected components remain intact, mutually exclusive selectors are rejected, and preview does not install, compile, connect or disconnect resources

### Requirement: Explicit reproducible native build

The kit SHALL document and verify a Rust-native bootstrap from the checkout using declared build prerequisites and locked dependencies. Installed artifacts SHALL carry the relevant source/build identity and binary integrity evidence. A successful build SHALL be verified before replacing an active installation. Ordinary runtime startup and read-only Check MUST NOT build code or acquire dependencies. Stale, missing or altered required builds SHALL produce an actionable status instead of silently executing an unverified candidate. Source-linked configuration and resources SHALL retain their authoritative checkout paths.

#### Scenario: Missing build prerequisite
- **WHEN** explicit bootstrap cannot find a compatible Rust toolchain or required native linker
- **THEN** it reports the missing prerequisite before global connection changes and preserves any existing working installation

#### Scenario: Source-stale artifact
- **WHEN** relevant executable source changes after a successful build
- **THEN** Check and the affected entry point identify the stale build and its explicit update action without compiling or downloading during the invocation

#### Scenario: Unchanged artifact and data update
- **WHEN** executable inputs are unchanged and a source-linked instruction or configuration file changes
- **THEN** the compatible verified binary is reused and the next consumer observes the updated authoritative data through the existing source connection

#### Scenario: Repair after source changes
- **WHEN** relevant source is newer than the installed manager or its dependent executable
- **THEN** an integrity-verified last accepted manager can perform Check and explicit update/recover/disconnect with compatible metadata without starting an obsolete ordinary runtime, or the documented Rust bootstrap provides the recovery path when that manager cannot safely be used

### Requirement: Preserved process and protocol guarantees

Rust replacements SHALL preserve accepted process memory/CPU containment, timeouts, cancellation, stdio forwarding, console behavior, PID/handle ownership and bounded cleanup. A bounded child SHALL enter its required containment before execution. Cleanup MUST target only the owned process tree or resources whose current identity matches the recorded owner. MCP replacements SHALL preserve initialization, framing, request IDs, schemas, cancellation, EOF and stdout purity, including representative error results. Migration MUST NOT weaken these guarantees to simplify a language port.

#### Scenario: Limit or cancellation
- **WHEN** an owned fixture process exceeds its resource/deadline limit or its caller cancels
- **THEN** the applicable process tree is stopped within the accepted bounds, the reason and exit outcome remain distinguishable, and unrelated processes and services remain running

#### Scenario: MCP failure and shutdown
- **WHEN** a foreign MCP tool emits a protocol error, closes its stream or receives cancellation
- **THEN** the native adapter forwards the defined outcome without corrupting JSON-RPC output, leaking owned processes or treating unavailable evidence as success

### Requirement: Foreign integrations retain accepted protections

The four selected MCP tools, subscription routing, source diagnostics, RTK and accepted skill workflows SHALL remain functional through verified external interfaces and first-party Rust adapters. External packages SHALL retain documented provenance/version and lifecycle management. Replacement of current OpenCodex and Serena internal-language integrations SHALL establish equivalent authentication, validation, runtime-provisioning suppression, worker/project isolation and recovery protections before retiring the old path. Runtime startup MUST NOT install or update foreign dependencies. An unavailable safe replacement SHALL leave the task unfinished and preserve the current working installation; it MUST NOT authorize dropping a capability or accepting an owned foreign-language shim.

#### Scenario: OpenCodex integration replacement
- **WHEN** the native implementation replaces browser login, candidate validation or native restoration currently reached through internal TypeScript modules
- **THEN** the equivalent success, closed-input/timeout/cancellation and failure paths are verified against the intended upstream version, credentials remain private, and routing/containment behavior is preserved

#### Scenario: Serena integration replacement
- **WHEN** the native implementation starts the adopted Serena tool for two independently owned project contexts
- **THEN** implicit package provisioning stays disabled, project/worker ownership remains isolated as required, and explicit semantic operations work through the intended actual foreign process

#### Scenario: Large real CodeGraph index
- **WHEN** acceptance builds a full CodeGraph index for the current `<large-acceptance-root>` project through the actual managed MCP entry point
- **THEN** indexing completes and publishes a usable current graph under the retained resource controls, representative maintained source can be queried, bounded automatic/manual refresh and response budgets work, and material coverage gaps remain visible; a small fixture does not substitute for this acceptance, product-source files remain unchanged and no controller operation occurs

#### Scenario: Graph provider replacement is shared with the Rust migration
- **WHEN** the native migration reaches the graph adapter or classifies a legacy CBM implementation
- **THEN** it integrates the first-party Rust CodeGraph adapter and applicable acceptance evidence owned by `replace-cbm-with-codegraph`, does not resume a separate CBM port or rewrite upstream CodeGraph, and leaves dependent tasks open until the replacement and integrated behavior pass

#### Scenario: Concurrent CLI projects use the native graph provider
- **WHEN** several Codex CLI sessions keep distinct indexed projects open and another client shares one of those projects
- **THEN** each project receives automatic refresh and correctly scoped queries throughout the healthy session lifetime, same-project clients share suitable backend/index/watcher resources, aggregate limits remain enforced, and closing one client does not interrupt the others; a single-active-project or periodic-manual-renewal restriction leaves acceptance incomplete

#### Scenario: Provider replacement precedes full native cutover
- **WHEN** CodeGraph is accepted through the existing global installation lifecycle while other Rust migration tasks remain open
- **THEN** replacement acceptance does not depend on completing the entire Rust migration, and subsequent native installation preserves the accepted CodeGraph selection instead of restoring CBM

### Requirement: Rust verification and skill delivery

The maintained harness SHALL provide Rust-based verification and executable test fixtures for its accepted behavior, including native console/process tests and global lifecycle tests. Supported skill helper interfaces SHALL migrate with their globally linked consumers. Default verification MUST make no model calls and SHALL use owned mutable targets. Checks requiring real external/native or opt-in provider access SHALL be identified separately; their absence MUST remain incomplete evidence. Existing meaningful failure oracles SHALL be retained or replaced with equivalent observed-behavior checks, rather than deleted merely because their implementation language changed.

#### Scenario: Skill used from another repository
- **WHEN** a fresh consumer discovers a migrated harness-owned skill outside this checkout and invokes its helper
- **THEN** the supported Rust entry point provides the documented output/error contract without an owned non-Rust bridge

#### Scenario: Default and native acceptance
- **WHEN** the ordinary Rust verification suite passes but a required native/global scenario is unexecuted or fails
- **THEN** the report identifies that boundary and does not mark the overall migration complete

### Requirement: Consumer-backed retirement and current selection

Retirement SHALL verify current callers, dynamic manifest references, installed registrations, skill consumers and accepted checks before removing a first-party implementation. The delivered manifest and supported command documentation SHALL match the native selection. Ordinary diagnostic/Stop hooks and the retired standalone harness LSP SHALL remain disabled; accepted RTK behavior and shared resource services SHALL remain available. Directory membership or a historical disabled feature MUST NOT alone establish that shared code is unused.

#### Scenario: Shared code in a retired subsystem
- **WHEN** a broker in the legacy LSP directory still serves an accepted resource or tool lifecycle
- **THEN** its behavior is ported and verified before the old source is removed

#### Scenario: Final installed selection
- **WHEN** final ownership and registration acceptance runs
- **THEN** no supported first-party Python/PowerShell/JS/C# execution path remains, obsolete owned script registrations are removed, and foreign registrations are preserved

### Requirement: Safe upgrade and recoverable cutover

The migrated kit SHALL upgrade from the existing script-based installation through an owned, journaled and identity-checked native lifecycle. Candidate build and isolated acceptance SHALL precede global cutover. Repair, relocation, interruption, concurrent sessions, rollback and disconnect SHALL preserve credentials, foreign edits, native Codex availability and accepted component independence. Recovery MUST NOT overwrite an unexpected target or stop the service carrying the active control session. Temporary legacy rollback material SHALL remain machine-local recovery state and SHALL NOT count as maintained executable code in the final source selection.

#### Scenario: Interrupted native activation
- **WHEN** activation fails after some owned registrations have changed
- **THEN** recovery restores a known working owned state or reports the exact ownership conflict with recovery evidence, while preserving credentials and unrelated state

#### Scenario: Active session during migration
- **WHEN** candidate acceptance and global activation occur while another session uses the prior build or subscription service
- **THEN** destructive probes use separate owned targets, running owned artifacts are not overwritten, and stale resources are retired only when safe

### Requirement: Complete global evidence and measured runtime effects

Completion SHALL include passing applicable acceptance from the real connected entry points in a fresh terminal/session outside the checkout, a complete ownership inventory, and resolved required migration tasks. Evidence SHALL identify source/build/dependency versions, inputs, tested behaviors and limits. Representative model-free runtime paths SHALL be compared against the baseline with the same scenarios, sample method and a pre-established noise tolerance. Material regressions in the owned boundary SHALL be investigated and corrected or remain explicitly unresolved; Rust usage alone MUST NOT be reported as proof of faster execution or reduced subscription consumption.

#### Scenario: Actual global consumer
- **WHEN** final acceptance exercises native launch, diagnostics, the selected MCP tools, subscription routing, RTK and migrated skill helpers outside the repository
- **THEN** the report identifies the actual connected build and observed behaviors, distinguishes isolated recovery tests, and confirms complete lifecycle/ownership acceptance

#### Scenario: Performance evidence
- **WHEN** migration results are reported
- **THEN** matched local runtime measurements include their dispersion and resource observations, external provider/network effects are separated, and unsupported acceleration or quota claims are absent
