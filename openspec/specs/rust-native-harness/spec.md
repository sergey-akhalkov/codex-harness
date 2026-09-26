# Rust Native Harness Specification

## Purpose

Provide the globally connected harness through maintainable first-party Rust code while preserving accepted behavior, safe migration and independently verifiable native operation.

## Requirements

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

### Requirement: Fresh manager delivery to new sessions

Installing or updating the native manager SHALL deliver a fresher published
build by adding its immutable files and moving the stable links that new
consumers resolve, and MUST NOT replace, delete or rewrite a build file that a
running session may hold. The delivered build SHALL be an integrity-verified
published build: the explicit `--build` when the operator supplies one,
otherwise the freshest verified build published from the selected source in
the owned state of the running manager. When the running manager does not
belong to an owned state, or no verified build matches the selected source,
the explicit build remains required and the failure is reported before any
registration changes. A Codex CLI session started before delivery SHALL keep
its already-resolved manager and MUST NOT be interrupted; a session started
after delivery SHALL resolve the freshly delivered manager through the stable
links. Selection MUST stay explicit: ordinary launch performs no build,
download or registration mutation, and a build that merely exists without an
explicit Install/Update MUST NOT become the consumer's manager.

#### Scenario: Delivery while an earlier manager still runs

- **WHEN** Install/Update delivers a newer verified build while a session still
  runs the previous manager
- **THEN** the operation succeeds without touching the previous build file, the
  running session continues on its manager, and a new consumer resolves the
  delivered build through the stable manager link

#### Scenario: Scoped update from an older manager

- **WHEN** a scoped update is run from, or refers to, an older manager while the
  owned state already holds a fresher verified build
- **THEN** new sessions resolve the freshest delivered build, and the update
  does not pin the older manager for later consumers

#### Scenario: Unverified or foreign candidate

- **WHEN** the newest directory under an owned state has a missing, altered or
  unverifiable record or binary
- **THEN** it is not delivered, the previous delivered manager stays in place
  and the failure is reported explicitly

### Requirement: Broker generations coexist across a delivery

The shared Serena and CodeGraph brokers SHALL resolve one private location per
delivered build generation. A consumer of a newer generation MUST NOT retire a
live broker that consumers of an older generation still use; it starts or joins
its own generation's broker instead, and MUST keep the existing behavior of
joining a broker that already matches its own generation. A broker whose
consumers are all gone SHALL retire on its own idle timeout, and an explicit
maintenance retirement SHALL remain available. Records whose location no longer
exists SHALL be dropped, and locations whose generation no longer has a live
broker SHALL be pruned from the record and reusable by a newer generation, so
the record and location count stay bounded across repeated deliveries. The
location records SHALL remain readable after historical growth: a bounded
record grown by prior deliveries MUST NOT make the Serena or CodeGraph MCP
entrypoint fail before MCP initialization, and a first successful startup after
such growth SHALL rewrite the record without its dead entries.

#### Scenario: New generation starts while an older session is live

- **WHEN** a new Codex CLI session starts after a delivery while a session of
  the previous build still uses its broker
- **THEN** both sessions complete MCP initialize against their own broker, and
  neither session interrupts the other

#### Scenario: Older generation drains

- **WHEN** every consumer of a generation has finished
- **THEN** that generation's broker retires on its idle timeout while the
  delivered generation keeps serving

#### Scenario: Location record grown by many deliveries

- **WHEN** a Serena or CodeGraph MCP entrypoint starts and its location record
  contains many historical generations with no live broker, larger than the
  previous fixed read bound
- **THEN** the entrypoint completes MCP initialize and tools/list, reuses a
  free location instead of preparing an unbounded new one, and rewrites the
  record without the dead generation entries

### Requirement: Ordinary launch survives an upstream Codex update

When the registered upstream Codex executable or its managed `@openai/codex`
package metadata changes in place, ordinary `codex` invocation SHALL still
start that current CLI. The launcher MUST NOT require an explicit harness
update, rewrite launch registration, compile, download or start a second
session. A digest mismatch alone is not incompatibility and MUST NOT produce
a launch warning when harness enhancements still apply. A warning is allowed
only when harness enhancements cannot be applied to the current CLI, and
Codex MUST still start. Integrity checks for harness extensions, refusal of
launcher recursion and failure when the upstream executable itself is missing
SHALL remain. Check MAY report the stale registration until explicit update
refreshes it.

#### Scenario: Codex CLI updates in place
- **WHEN** a user updates the installed Codex CLI so the registered upstream executable hash or package.json digest no longer matches launch registration, and that path still names a usable Codex executable whose harness session can be prepared
- **THEN** the next `codex` invocation starts that current CLI with the supplied arguments, without an explicit-update error and without an extra compatibility warning

#### Scenario: Recursion and a missing CLI stay explicit failures
- **WHEN** launch registration points at the harness launcher itself, or the registered upstream executable is missing
- **THEN** the launcher fails explicitly without invoking an arbitrary replacement or a second session

### Requirement: Interactive CLI session process tree

Ordinary interactive Codex CLI sessions started through the harness launcher SHALL run the registered upstream executable inside a Windows Job that kills remaining members when the last job handle closes. Containment SHALL be established before the upstream image executes. The session job SHALL NOT apply the helper memory or CPU caps used for MCP, indexer and probe workers. Independently started kit services, including the xAI shim and shared MCP brokers, SHALL remain outside that session job. Interactive standard streams and the calling console SHALL be inherited so argument, Unicode, cwd, stdin, stdout, stderr and exit-code contracts stay unchanged. The launcher SHALL wait for the session root to exit with no execution deadline, then reap surviving session descendants. Cleanup MUST target only that owned tree. Closing the wrapper or the console SHALL also reclaim the tree. The kit SHALL NOT hunt processes by name or PID alone, SHALL NOT scan the computer for closed sessions, and SHALL NOT leave a detached scavenger running after the launcher returns.

#### Scenario: Hidden helper outlives the CLI today
- **WHEN** an ordinary `codex` session started through the harness launcher spawns a detached helper and then the CLI root exits
- **THEN** the helper is reclaimed with the session job, a separately started kit service keeps running, and the launcher returns the CLI exit code

#### Scenario: Wrapper or console dies first
- **WHEN** the harness launcher process or its console is terminated while session descendants are still running
- **THEN** those owned descendants terminate and unrelated processes remain

#### Scenario: Interactive streams stay attached
- **WHEN** a user starts `codex` from an existing terminal through the harness launcher
- **THEN** the upstream CLI inherits that console and standard streams and is not attached to NUL

### Requirement: Native build scratch reclamation

The explicit native build lifecycle SHALL reclaim its own abandoned
process-temp scratch. Before compiling a candidate, the manager SHALL remove
direct children of the process temp root whose names use the harness scratch
prefixes (`hcb-`, `hcc-`, `hca-`) when they are ordinary directories whose
last write is older than the documented sweep age. The sweep SHALL skip
reparse points and never follow them, SHALL NOT touch entries outside those
prefixes, and removal failures SHALL NOT fail the build.

#### Scenario: Abandoned scratch is reclaimed
- **WHEN** a previous interrupted explicit management operation left
  `hcb-`/`hcc-`/`hca-` prefixed scratch directories older than the sweep age
  in the process temp root
- **THEN** the next explicit native build removes them before compiling its
  candidate

#### Scenario: Fresh and foreign entries survive
- **WHEN** the process temp root also contains a fresh prefixed scratch
  directory, a reparse point or unrelated directories
- **THEN** only stale prefixed ordinary directories are removed and every
  other entry remains

### Requirement: Native substitution preserves accepted outcomes

A replacement by a native capability SHALL preserve the affected accepted outcomes, business rules, authorization, concurrent use, recovery and installed entrypoint behavior. Help text, a successful API acknowledgment or a unit-test-only consumer SHALL NOT establish equivalence. An unsupported version or missing native guarantee SHALL retain the smallest working adapter and report the concrete limitation rather than silently drop a feature, add routine manual steps, change a provider or weaken acceptance. Required unfinished work SHALL remain explicit after removal of an unused implementation; removal SHALL NOT count as completing that work.

#### Scenario: Native operation lacks an ownership guarantee
- **WHEN** a native command can start or stop a session but cannot enforce the kit's exact-run ownership and neighboring-run isolation
- **THEN** the retained adapter enforces those rules through the supported entrypoint, and the replacement is not described as a complete native substitution

#### Scenario: A helper has no established runtime consumer
- **WHEN** a candidate is referenced only by tests or inactive scaffolding
- **THEN** its removal requires checked callers, supported external entrypoints and a disposition for every accepted rule it represents, without deleting those rules or declaring their unfinished acceptance complete

#### Scenario: Substitution reaches ordinary global use
- **WHEN** a native-backed replacement is delivered
- **THEN** the installed outside-checkout workflow preserves its arguments, results, failure meanings, live source ownership, active work and recovery without a new endpoint, configuration or manual-check ritual

### Requirement: Simplification reduces maintained work rather than relocating it

The completed simplification SHALL demonstrate a net reduction of first-party production code and affected owned instruction content against an identified source baseline, with tests, fixtures, planning artifacts and operational documentation reported separately. It SHALL preserve required checks and readable implementation; minification, moving code to another maintained component, deleting tests, hiding instructions in mandatory references or substituting repeated agent reasoning for deterministic checks SHALL NOT establish reduction. Every assessed replacement SHALL have a recorded removal or an evidence-based retention decision in its existing owner. No percentage, token, speed or subscription benefit SHALL be claimed beyond the actual measurement.

#### Scenario: Code is moved into another package
- **WHEN** a refactor removes a module but introduces an equivalent maintained helper or dependency adapter elsewhere
- **THEN** acceptance includes the added code and operating burden rather than counting only the deleted module

#### Scenario: Tests dominate a reported line reduction
- **WHEN** obsolete implementation-specific tests are removed after equivalent behavior coverage is retained
- **THEN** production and test changes are reported separately and the production reduction claim does not include test deletion
