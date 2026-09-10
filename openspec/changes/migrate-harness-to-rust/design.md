## Context

See [proposal.md](proposal.md) for the motivation and accepted scope. The exploration on 2026-09-08 found 95 Python files, 58 PowerShell scripts/modules, 10 JavaScript files, two C# files, one Rust source and one TypeScript fixture in the working checkout, including tests and uncommitted inputs. These counts describe a dated discovery snapshot, not a frozen migration checklist. Other changes are active and the working tree is extensively dirty.

The important execution paths are:

- `install.ps1` and `tools/kit.psm1` manage linked configuration, user PATH, ownership and recovery. PowerShell also implements launcher argument handling, source diagnostics, subscriptions and Task Scheduler operations.
- `tools/mcp.ps1` starts `tools/code-tools/launch.py`, which launches Python adapters for all four selected MCP tools. Dependency provisioning, brokers and process ownership also use Python.
- `tools/opencodex-process.cs` enforces Windows process containment; `tests/ConPty.cs` supports actual console acceptance. These are behavioral safeguards, not disposable language wrappers.
- `tools/opencodex-*.mjs` import internal TypeScript modules from the pinned OpenCodex package. `tools/code-tools/serena_entry.py` integrates with Python package internals. Replacing these seams requires proving an external contract that preserves their protections.
- `tools/rtk-adapter` already provides a small Rust executable and build identity. Its accepted behavior can be reused in a workspace.
- `launch.py` rejects `harness-lsp`, while `global/kit.psd1` still requires many LSP sources, including `tools/lsp/broker.py`. This manifest reference alone does not establish live use or safe retirement: current consumers must be checked per file.
- First-party skill helpers live under `.agents/skills/structured-codex-run/scripts/` and `.agents/skills/reproduce-regression/scripts/`; tests also contain generated executable fixtures and embedded C#.

Confirmed user decisions: this migration targets Rust for all maintained first-party executable code in `codex-harness`; third-party tools may retain their languages/runtimes. On 2026-09-09 the user additionally selected Rust as the programming default and PowerShell as the shell default across all projects and delegated work, as recorded in [the owning decision](../../../docs/project-decisions.md#language-and-shell-defaults). These global defaults permit explicit user or concrete integration/platform exceptions and do not require unrelated project rewrites. Earlier discussion of retaining harness PowerShell implementations is not an exception to the full migration target. The user values execution speed and the full cost/time of achieving a verified result. Repository guidance and migration documentation must remain consistent with the portable defaults while preserving the harness's stronger migration commitment.

## Goals / Non-Goals

**Goals:**

- Give the kit one first-party compiled implementation and verification toolchain, with cohesive reusable process, configuration and lifecycle code.
- Preserve accepted global behavior and existing safety boundaries while replacing language-specific entry points.
- Make source/build identity, external dependencies, migration completeness and runtime measurements inspectable.

**Non-Goals:**

- Rewriting upstream Codex, OpenSpec, OpenCodex, Serena, Graphify, Codebase Memory, CodeGraph, Nuphus, RTK or external language servers; continuing a CBM-specific port or indexer optimization after its replacement was selected.
- Changing provider/model assignments, billing, accepted hook selection, credentials, or the language of projects using the kit.
- Adding platform support beyond the currently accepted native Windows environment, reviving retired diagnostics, or replaying previously excluded `opencode-kit` evaluations.
- Implementing functionality from unrelated unfinished changes or treating their historical task counts as completion evidence for this migration.

## Decisions

### 1. Define ownership by behavior and maintenance, not extension

The completion boundary includes runtime code, installer/recovery/build helpers, tests and their executable doubles, skill helpers, service hosts and invocation adapters. Rust code that emits or embeds Python/PowerShell/JS/C# programs for execution is still a foreign-language first-party implementation and is excluded. Moving such code to another directory, a local package or an owned fork does not change ownership.

Declarative TOML/JSON/YAML/XML, Markdown, lockfiles and ordinary documented CLI invocations are data/documentation. Inert syntax samples used to inspect external language tooling can contain another language, but must be classified as test data and never serve as executable harness helpers. Third-party packages are separately identified by provenance/version and managed through the existing external dependency lifecycle. A small reviewed inventory/check must detect unexpected executable files, generated snippets and stale command registrations, rather than relying only on extension counts.

Alternative considered: keep small PS/JS/Python shims indefinitely. This would simplify some ports but would not meet the user's selected endpoint. Future proposals for a different language must explain their concrete benefit and reconcile this scope; an unported shim cannot be marked complete through an implementation-time exception.

### 2. Use one Cargo workspace and a small set of native entry points

Create a root workspace with a core library, a CLI/launcher package and the existing RTK adapter as a workspace member. Split additional crates only for a demonstrated build or dependency boundary; group modules by installation, process ownership, tool lifecycle, diagnostics, subscriptions and verification. Avoid a crate for every old script and avoid introducing a general plugin runtime.

The management entry point is `codex-harness.exe`, with `install`, `update`, `check`, `recover`, `disconnect`, explicit subscription operations and skill/verification subcommands. `check --diagnose --project <directory>` provides the focused source report. Component selectors retain the existing core/code-tools/subscriptions/token-workflow isolation and mutually exclusive validation. A Rust launcher keeps ordinary `codex` invocation; a native `codex-harness-check.exe` alias preserves convenient global discovery. RTK retains its accepted explicit executable contract.

Replace the PowerShell data manifest with a declarative native-readable manifest. Preserve profile precedence, command forwarding, the first-position task-effort selector, machine state boundaries and return statuses. Publish an old-command/new-command table for each supported script entry point; removal of `.ps1` names is an explicit breaking change. Resolve and persist the genuine upstream Codex command independently of the harness launcher to prevent recursion and PATH hijacking.

Alternative considered: a single mechanical Rust translation for each existing script. Shared primitives better preserve common ownership and argument rules and reduce duplicated lifecycle mechanisms.

### 3. Separate explicit builds from ordinary execution

Bootstrap from the checkout with a documented `cargo run --release --locked --manifest-path <checkout>/Cargo.toml -p codex-harness -- install --source <checkout>` path and explicit output-location options. Rust/Cargo and the Windows linker prerequisites are external build dependencies, checked before installation mutation. A compatible existing build can be reused. This change does not require publishing binaries or operating a release service.

Installed native artifacts live in versioned installation-owned build directories outside tracked sources. Record source identity, Cargo.lock, relevant build inputs/features, target/toolchain and binary hashes. Preserve the existing direct links/additional paths for instructions, skills, settings and other source-owned data; do not replace them with merged or copied configuration. A compiled executable is a derived artifact, not an alternative configuration source.

Build and validate a candidate before changing global registrations. Check detects a missing, altered or source-stale artifact. Ordinary launch performs the bounded relevant freshness/identity check, reports how to update when stale, and neither compiles nor downloads dependencies. Unchanged builds are reused; documentation-only changes do not invalidate unrelated binaries. Explicit install/update/recover handles rebuild and activation. Immutable build directories avoid overwriting executables still running in another session.

Freshness checks must not create a repair deadlock: the last accepted manager, after verifying its recorded binary integrity, can run Check and explicit update/recover/disconnect against compatible metadata even when its source is newer. It cannot use that exception to start an obsolete normal runtime. If the manager itself is missing, altered or metadata-incompatible, use the documented Cargo bootstrap with the explicit source/installation roots. Exercise both routes and failed candidate builds.

Alternative considered: build at every launch. This introduces unpredictable latency, network effects and cross-session contention. Reusing an unverified old executable would hide changes in this source-linked kit.

### 4. Port control and safety primitives before higher-level flows

Use native Windows interfaces from Rust for jobs, process handles, console/ConPTY support, environment/PATH changes, links, locks and Task Scheduler. Reuse narrowly scoped bindings where they reduce unsafe implementation; choose and pin dependencies through the normal Cargo lifecycle. Launch external commands with explicit argument vectors and intended cwd/environment, without shell-generated programs.

Preserve assignment to a bounded job before child execution, memory/CPU limits, cancellation/deadline semantics, kill-on-owner-close behavior, handle cleanup, PID identity checks and non-overlapping owned cleanup. Service restart policy, authentication flow and foreign service/process preservation remain explicit integration contracts. No migration test may stop the proxy carrying the current Codex session.

Port existing argument, containment and recovery oracles early. Test subprocesses and MCP doubles become small Rust programs from the same workspace. Compare representative positive and failure scenarios with a preserved baseline before redirecting callers; retain the old path only during migration, not in the delivered endpoint.

### 5. Keep foreign tools across explicit, verifiable interfaces

For out-of-process MCP adapters, preserve JSON-RPC IDs, framing, stdout purity, cancellation, EOF, initialization, schemas and tool results. Keep registry selection, resource bounds, lazy startup, shared-worker isolation, project identity and explicit dependency provisioning. Native runtime startup must not acquire packages. Existing tool language support belongs to the foreign tool/user project and is unaffected by the harness language policy.

The large-project acceptance now targets CodeGraph through [replace-cbm-with-codegraph](../replace-cbm-with-codegraph/proposal.md). A successful full index of the current `<large-acceptance-root>`, useful representative queries, bounded automatic/manual refresh, coverage, resource ownership and recovery remain required through the actual managed MCP. This supersedes the provider-specific requirement to make CBM index that project; it does not waive the underlying acceptance or authorize raising the 2 GiB limit. Small fixtures and the initial upstream trial do not establish complete managed/global acceptance. Preserve product sources and do not contact controllers.

The concrete consumer and representative source/symbol stay in local acceptance inputs outside Git. Existing `HARNESS_CBM_LARGE_PROJECT`, `HARNESS_CBM_LARGE_SOURCE` and `HARNESS_CBM_LARGE_SYMBOL` describe the historical CBM test entry point, not the new execution target. Replacement task 3.5 owns carrying the selected consumer and useful source oracles into CodeGraph acceptance with its verified command/input contract. Missing inputs keep that task open; they do not justify choosing a toy repository or resuming CBM development.

#### Graph-provider ownership and order

| Work | Single owner | Rust migration dependency |
| --- | --- | --- |
| Existing generic Jobs, framing, cancellation and broker primitives | This migration, tasks 2.4 and 5.3 | Reuse verified implementation; close only remaining generic integration gaps |
| Published CodeGraph dependency, native Rust adapter, concurrent-project observation, shared same-project resources, bounded scheduling/storage/results and large-project acceptance | Replacement tasks 2.1–3.5 | Task 5.4 adopts the same implementation and applicable evidence; no parallel CBM or second CodeGraph adapter |
| Provider activation, rollback, retirement of owned CBM registration and outside-checkout MCP verification | Replacement tasks 4.2–4.3 | Final native installation tasks 9.1–9.3 preserve and verify that selection |
| Complete native installer/launcher and retirement of remaining first-party script paths | This migration, tasks 4, 8 and 9 | Replacement may use the current lifecycle and does not wait for full Rust cutover |

Order: reuse the available native foundation, implement/verify CodeGraph in its owning replacement change, integrate that result into the remaining Rust migration, then complete the full native installation. This is not a circular dependency: replacement acceptance covers its changed adapter and current global MCP consumer, not completion of every Rust migration task. The published CodeGraph implementation and Node runtime remain third-party; only the harness adapter, supervision and executable tests must be Rust.

The replacement's current [concurrency contract](../replace-cbm-with-codegraph/specs/bounded-tool-resources/spec.md#requirement-deliberate-scope-and-honest-freshness) requires automatic service in all simultaneously open Codex CLI projects, shared same-project resources across clients and continued healthy operation beyond 600 seconds. Observation may stop after a project's last session closes; reopening catches up automatically. A single heavy indexing slot is an operation resource limit, not an exclusive active-project selection. Earlier single-project and finite-lifetime candidate checks do not establish these requirements; task 5.4 must adopt their completed replacement evidence before closing.

Existing CBM code and baseline evidence remain available for current operation, compatibility/rollback and consumer-backed retirement. Do not improve or re-port CBM to satisfy the inventory. Preserve generic transport/resource oracles, reuse applicable tests after checking identity, and retire CBM-only paths after the replacement's accepted cutover rather than blindly translating every old file.

Two focused feasibility checks precede their dependent ports:

1. **OpenCodex:** map `restoreNativeCodex`, candidate validation, browser-only login, bounded OAuth callbacks and relevant guard tests to the installed upstream CLI/HTTP/schema surface. Verify the current pinned package's exact exposed operations and error behavior. A Rust adapter can call a real upstream interface or implement the limited needed behavior in Rust with compatibility tests; do not duplicate the entire provider runtime or embed a JS host to keep owned snippets.
2. **Serena:** map guarded startup, shared worker/project isolation and runtime-provisioning suppression currently implemented through Python internals to native configuration/CLI/MCP behavior. Verify that the chosen boundary prevents implicit downloads and retains each accepted guard. Where needed, enforce the guard in the Rust process/protocol boundary and test the real foreign entry point.

The chosen strategy is external tool reuse with first-party Rust adapters; exact upstream flags/interfaces are evidence to establish, not assumed contracts. If a required behavior has no safe Rust/external-interface implementation, preserve the current installation, record the concrete blocker and keep the corresponding task open. Do not drop a tool, weaken its guard, silently keep owned foreign code, or label an owned patch as an upstream dependency. Acquiring a compatible released upstream capability is an allowed dependency resolution; maintaining new non-Rust first-party code is not.

### 6. Port accepted capabilities, retire only proven obsolete code

Build the migration map from the actual tracked and untracked source, manifests, global registrations, docs, skill entry points and applicable tests at implementation start. Each executable unit has a Rust replacement, a verified external dependency classification, or a retirement rationale with consumer evidence. Include embedded/generated helpers and resources reached dynamically through manifests.

Treat old `tools/lsp` code file by file: preserve or port shared brokers and explicit accepted functionality, remove disabled diagnostic implementations only after reference/configuration checks. Reconcile required-file inventories and inactive launch branches. Migrate skill invocation documentation together with its helper, preserving the globally linked skill directory. Historical Git/OpenSpec evidence stays historical; supported executable leftovers and disposable bytecode/temp helpers do not stay in the final delivered source selection.

There is a known planning collision: `autonomous-skill-evolution` modifies `linked-global-kit` / `Source updates and checkout availability` to add current-session skill awareness/revision recovery. This change's delta starts from the current main spec; before implementation integration and archive, compare the then-current main requirement and pending delta, preserving any landed skill clauses and scenarios while adding compiled freshness. Do not mark or implement the autonomous workflow's remaining tasks as part of the language migration. Likewise preserve the RTK exception from `optimize-agent-token-workflow`, bounded process requirements from `bound-code-tools-resources`, and added skill-registration/verification requirements from the other active changes; their accepted paths must migrate when present in the selected input revision.

### 7. Define acceptance around behavior and complete delivery

Keep a requirement-to-check migration map rooted in existing acceptance suites. Rust integration tests cover the actual CLI, not merely new internal functions. The matrix includes installation/upgrade/repair/relocation/disconnect, component isolation, rollback conflicts, launch/TUI forwarding, model-free private diagnostics, four MCP interfaces, subscription lifecycle/containment, RTK raw recovery/single execution, usage/outcome tooling and skill helpers. Distinguish deterministic doubles from actual native consumers and opt-in provider calls.

Expose ordinary Rust format, lint and test commands plus explicit native/global acceptance commands. Default checks perform no model calls and own their mutable resources. Run the required real global consumer checks outside the repository after migration without exercising destructive service controls against the active session. Every accepted current behavior needs a passing migrated check or retained applicable independent evidence; a missing provider/access prerequisite remains an unfinished acceptance task.

Measure matched cold/warm launch, model-free Check, MCP initialization/round trips and process-control/RTK paths with source/runtime identity, sample counts, dispersion and memory/resource observations. Establish the comparison method and noise tolerance from the baseline before reading candidate outcomes. Investigate and correct material regressions in the owned boundary; disclose externally dominated results and avoid claiming provider/network speedups or quota savings from the rewrite.

Alternative considered: close the change after Cargo tests or language-count cleanup. Neither proves that the globally connected kit, actual UI and external integrations still work.

## Risks / Trade-offs

- Foreign internal APIs are currently used -> prove replacement contracts early; preserve the installed path and unfinished status if a guard cannot be retained.
- Cross-session global cutover can interrupt the control channel -> stage an owned isolated installation, activate immutable builds transactionally and retire only demonstrably unused owned resources; destructive recovery tests use separate service targets.
- Windows process/console behavior can regress despite unit tests -> preserve actual subprocess, job-limit and ConPTY oracles, Unicode/quoting and cancellation checks.
- Compiled code weakens the old immediate-source-update expectation -> explicit build identity, scoped stale detection, source-linked data and a documented native update command.
- Porting tests while porting behavior can reproduce the same mistake -> retain baseline observations and failure oracles, compare entry-point results and independently review high-risk process/ownership changes.
- Broad dirty/concurrent work can invalidate a file inventory -> record exact inputs, inspect again before retirement/cutover, integrate changes without broad revert or unrelated cleanup.
- Rust reduces first-party language count but external runtimes remain -> report dependencies by owner and do not claim interpreter-free installation of the entire tool collection.

## Migration Plan

1. Capture current accepted behavior and source identities, reconcile concurrent affected work, and prove the OpenCodex/Serena replacement boundaries on owned targets before removing any dependency path.
2. Introduce the workspace, native build identity and process/verification foundations. Deliver a small native Check/build path first while leaving the current global entry points intact.
3. Port lifecycle/launcher/diagnostics, selected tool integrations, subscription controls, RTK integration, skill helpers and evaluation/usage tools in cohesive increments with their tests. Transitional non-Rust paths remain explicitly unfinished work.
4. Stage a complete native installation in owned roots, exercise upgrade from the old metadata/command layout, then activate globally after applicable checks. Journal expected source/target identities and retain the previous build/registration snapshot for rollback.
5. Finish real outside-checkout acceptance, remove stale owned script registrations and sources only after consumer checks, reconcile guidance/manifests/current affected specs, and verify the complete ownership inventory.
6. Archive only after all requirements and tasks pass. A rollback restores the previous owned registration/build layout and service readiness while preserving credentials and foreign edits; unexpected ownership changes stop automatic mutation and produce an actionable recovery report. Legacy executable rollback material is temporary machine-local recovery state, not maintained code in the final workspace, and is retired when safe.
