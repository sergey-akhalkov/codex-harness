# Native Rust implementation

The [migration specification](../openspec/changes/migrate-harness-to-rust/proposal.md)
requires Rust for all maintained harness-owned executable code, including tests
and skill helpers. This is a rule for pack development; it does not change the
languages of projects using the kit. External Codex, OpenCodex, Serena,
Codebase Memory, published CodeGraph/Node, Graphify, Nuphus and language
servers retain their own implementations and supported runtimes. The
first-party CodeGraph adapter is Rust and is the live global graph after
replacement acceptance.

Global installation still uses the existing script lifecycle. Native commands
prepare, check and recover isolated candidates; they do not yet replace the
global launcher or installer. The Rust configuration bridge is already consumed
by the script launcher; the full native lifecycle cutover remains open. Remaining work lives in
[Rust migration](evidence/rust-migration.md).

## Prerequisites and checks

Windows x64, Rust MSVC toolchain (minimum Rust 1.89), Cargo, the MSVC C++
linker and Windows SDK. Workspace dependencies are locked in the root
`Cargo.lock`. The RTK adapter is a workspace member with no independent
lockfile. Use the existing toolchain; Check does not install packages.

```powershell
cargo build --workspace --locked --jobs 1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked --jobs 1 -- -D warnings
cargo test --workspace --locked --jobs 1 -- --test-threads=1
cargo run -p codex-harness --bin harness-source-check -- --root .
```

Optional `--private-terms <external-file>` supplies local audit terms without
persisting or echoing them. The checker inspects current Git-listed tracked and
new existing files, tracked caches, machine-home paths in docs/shared
configuration, shared `[projects.*]` records and inline Markdown local
targets/anchors, skipping fenced examples, templates and URLs. It does not
inspect Git history.

Current-path baselines for the Rust migration are captured by
`cargo test --locked -p codex-harness --test migration_baseline --jobs 1 -- --test-threads=1 --nocapture`
after `cargo build --locked -p harness-rtk --jobs 1`. The comparison method
and noise tolerance live in [Rust migration](evidence/rust-migration.md).
Detailed receipts stay in the printed private TEMP root.

Keep builds and process-resource acceptance sequential. `--jobs 1` also bounds
the native manager's compiler concurrency without increasing its 2 GiB Job
limit. A failed compilation retains its log and does not alter the active
installation. Windows can keep a running test executable locked after Cargo
releases its build lock; use `--target-dir <owned-verification-directory>`
when a retained test binary could occupy the default target. That is a Cargo
output choice, not an ambient `CARGO_TARGET_DIR` override for native build
identity. Custom compiler wrappers and ambient Rust build overrides are
rejected before creating build state.

## Native task-control contract

The [subscription orchestration change](../openspec/changes/orchestrate-subscription-agents/tasks.md)
is under development. Its bounded Rust WebSocket client has an opt-in native
contract check; global activation and quota handoff remain unfinished.
Supply the intended native CLI executable explicitly, then run from this checkout:

```powershell
# Set HARNESS_CONTROL_CODEX_EXE to the native Codex executable being qualified.
cargo test --locked -p codex-harness --test task_control_contract native_two_clients_reconnect_tool_result_and_tui --jobs 1 -- --ignored --nocapture --test-threads=1
```

The ordinary native-entry acceptance is a separate opt-in test:

```powershell
cargo build --release --locked --workspace --bins --jobs 1
cargo test --release --locked -p codex-harness --test task_control_launch ordinary_launcher_starts_control_and_delivers_tool_result --jobs 1 -- --ignored --exact --nocapture
```

It uses actual Cargo binaries in an owned fixture registration and the same
explicit native CLI input. The fixture enables `task_control`; existing
registrations default to disabled and the installer does not activate this
unfinished controller yet. The test requires automatic service startup, exact
model binding, a single tool effect, visible final output, a controller checkpoint
containing that result and verified service exit after the native TUI quits.
Fixture registration is not evidence for the unfinished global install lifecycle.
The native-entry scenario passed on Codex 0.154.0 on 2026-09-12, including the
checkpoint and owned-process exit assertions. Its printed private root retains
binary hashes, provider requests and terminal evidence. A local synthetic provider
does not establish subscribed model availability or quota-handoff behavior.

This check uses private owned state, Windows Jobs, a token-protected loopback
app-server, a local canned Responses provider and ConPTY. It makes no live model
requests. It captures the CLI version/hash, generated experimental protocol
schema, request/event evidence and terminal output in the printed private root.
It exercises a native child assignment, one actual tool mutation, two-client
event delivery, reconnect while the child remains active, final-result retrieval
and visibility in the native TUI. The mutation must occur exactly once.

Verified on Codex 0.154.0: the app-server exposes child identities through
`subAgentActivity.agentThreadId`, with `parentThreadId` and `sessionId` available
on resume. A child can be observed/resumed but reports `canAcceptDirectInput=false`.
The canned native collaboration call uses namespace `collaboration`, an underscore
task name and `encrypted_function_args: []` to identify its known plaintext brief.
This is a fixture protocol contract, not permission to reinterpret opaque history.
The test pauses its synthetic native goal and bounds provider requests: an active
native goal can generate further turns after a canned final response. The future
controller must coordinate with that scheduler rather than dispatch twice.

The ordinary TUI also creates ephemeral system threads for generated titles and
uses paginated history. These threads are not user task owners and reject full
history reads. Subscribe to a user thread after its first active turn establishes
a resumable rollout; retain completed-item events rather than relying only on an
early history snapshot for the final result. The owned provider supplies a title
response separately from the tool-using assignment.

The separate `native_visible_chats_before_model_dispatch` test in that target
requires `HARNESS_CONTROL_VISUAL_ACCEPTANCE=1` and opens three owned native windows.
Its private `views-ready.json` identifies the exact threads; scoped Nuphus
observation supplies `views-observed.json` with those `threads` and
`simultaneouslyVisible=true`, then `visible-result-observed.json` with
`finalVisible=true` after the native final marker is seen. Observation waits are
bounded, and owned Jobs clean the windows even after assertion failure. These
receipts record desktop evidence; the probe uses only canned local responses.

The native path can open empty conversations with zero provider requests, name
them through `thread/name/set`, then receive controller-initiated tool work and
final output in their existing windows. Naming before dispatch avoided automatic
title requests in the owned scenario. The `native_named_empty_thread_can_be_attached`
test separately verifies controller-created, named empty threads can be resumed
before a model request, then exercises native TUI attachment and a synthetic
tool result. The controller must first resume the named empty thread by ID:
attaching the TUI directly by its rollout path otherwise fails with missing
source history on the tested CLI. This preparation makes no model request.
Remote resume rejects CLI permission overrides even when they match
the thread. For a new managed thread those options remain on the backend and
are omitted from the attaching TUI; prompt, model, effort and presentation
options remain. An explicit resume that requests changed permissions keeps the
ordinary native path, with a diagnostic, until equivalent managed behavior is
verified. Observers of TUI-created threads must subscribe after
the first active turn; global thread-start events alone do not subscribe them
to final turn events. The new visible-console option retains Job ownership
and gives the new window its own standard devices.

The native-entry test now also opens an owned conversation window. Its
`native-entry-result-ready.json` identifies the runtime/window for scoped Nuphus
inspection. After observing the final output, supply
`native-entry-result-observed.json` with `finalVisible=true` and issue `/quit`
in that exact native window. The fixture then checks wrapper exit, final checkpoint
and service cleanup. Simultaneous active executor work, automatic runtime view
loss/reconnection and global delivery remain unfinished.

A frontend that exits early records its native exit code in private
`client-closed.json`; the entry-point test reports that exit immediately instead
of waiting for a model result from a closed conversation.

The separate `ordinary_launcher_suspends_and_recovers_visible_conversation`
entry-point test pauses a command after its first file effect. At its private
`native-entry-partial-ready.json`, minimize the identified owned window with
scoped Nuphus input. Wait for `native-entry-suspended-ready.json`, which requires
native interruption, preserved file content and no second provider request.
Restore that same window, verify its actual state and supply
`native-entry-view-restored.json` while the original tool is still running. The
test checks that restoration alone does not issue another provider request,
then releases the tool and awaits its native completion before continuation.
Then use the ordinary final-observation and quit steps above. The restoration
and final desktop gates each allow 90 seconds;
run consecutive interaction steps together rather than leaving a gate pending
while investigating other work. This scenario passed with synthetic Responses:
one file effect, visible continuation and final result, exactly three requests,
native exit zero and owned controller cleanup. The ordinary entry scenario also
passed on the same build. These checks cover one conversation; simultaneous
active workers, frontend death/recreation and quota handoff remain unfinished.

The controller watches the retained frontend process identity and actual window,
including minimization; an old `view-state.json` is not readiness evidence.
Native turn interruption does not prove a running tool has stopped. The observer
retains active tool items until their completion events and waits for them before
resuming the interrupted conversation, including after its window is restored.
Private checkpoint reads/writes retain transactional guards and retry only
Windows sharing/lock conflicts for at most 500 ms. Other errors remain failures,
and this retry never repeats a model or tool request. The focused checkpoint
tests cover a short reader and a persistent lock preserving the previous state.

The separate `ordinary_launcher_records_quota_refusal_without_another_gpt_request`
fixture returns one structured synthetic usage-limit refusal before the lead can
issue a fallback instruction. The controller records the correlated terminal
error in private `failures.json`, retaining the native error and observation time.
Its classifier distinguishes a usage limit, throttling, a session budget,
connection failure, interrupted output and exhausted retries; unrecognized or
ambiguous evidence stays unknown. A generic HTTP 429 or quota wording does not
establish exhausted subscription capacity or its reset window. Observe the
failure in the native window and use the ordinary final receipt and `/quit`
steps. A native `systemError` can settle after its latest failed turn is observed
and active operations have finished; an old failure cannot settle a newer turn.
The native refusal and ordinary tool-result cases passed on the corrected build,
including visible output and owned controller exit. The following separate check
covers the first automatic successor dispatch.

`ordinary_launcher_hands_quota_refusal_to_visible_zai_lead` exercises the integrated
successor path with synthetic Responses and an explicit local
`HARNESS_CONTROL_MODEL_CATALOG` input. The fixture copies model metadata only;
it neither imports credentials nor contacts subscriptions. It requires a fresh
Z.AI thread, unchanged initial permission binding, preserved visible task context
without opaque reasoning, and both native views at the first successor request.
The wrapper waits for the native TUI's named-thread caption before reporting the
new view ready. At `native-entry-result-ready.json`, inspect `view` for the Z.AI
result and `previousView` for the original failure/history, then supply the final
observation receipt and `/quit` in both windows. This native check passed: one GPT
refusal, two `zai/glm-5.3` requests at advertised `high` effort, one tool effect,
visible final result, retained previous chat and natural controller exit. It used
owned synthetic Responses and the explicit local model metadata input, not live
subscription capacity. Global activation, concurrent executors, restart and
leadership return remain open.

Original failures are retained privately: daemon startup refused an elevated
terminal; the Unix listener rejected its directory's privacy; a new thread had
no resumable rollout before its first turn; a closed provider port entered native
offline recovery instead of finishing; and an early fixture inherited Windows
socket nonblocking mode. The WebSocket alternative preserves the daemon's guard.
These results do not establish subscribed model binding, global lifecycle,
leadership transfer, recovery after controller restart or measured quota benefit.

## Manager, inventory and core connection

```powershell
cargo run --release --locked -p codex-harness -- build --source . --state "$env:LOCALAPPDATA/codex-harness-native"
codex-harness.exe check --build <directory> [--source <checkout>]
codex-harness.exe inventory --source <checkout> --codex-home <directory> --user-home <directory>
codex-harness.exe inspect-installation --codex-home <directory> --user-home <directory> [--dependency-user-home <directory>]
codex-harness.exe activate-build --state <state-directory> --build <build-directory>
codex-harness.exe recover-build --state <state-directory>
```

Check is model-free and executes no Cargo, project command or hooks. Optional
`--source` checks relocation without modifying registration. Inventory reports
core data links and names without mutation and does not yet cover the full
installer or dependency checks. Inspect-installation reads and validates legacy
metadata without changing it, distinguishing owned/adopted links.

Activate/recover manage a local candidate pointer only. They do not register
global commands or change services. Selection is serialized with builds. A
journal retains the exact previous pointer; recovery checks expected contents
and build identity. Repair that started from a damaged build finishes the
verified replacement. Unexpected edits preserve both target and journal. Old
binaries are never overwritten or removed. Ordinary consumers must reject an
unfinished selection journal or stale build. An integrity-verified manager may
select a repaired build even if the old binary was altered; recovery refuses to
restore an altered previous build as usable.

Every compilation uses a fresh owned temporary target with a short path for
MSVC; unchanged candidates reuse verified immutable binaries. Explicit release
compilation has a 30-minute deadline, a 2 GiB Job and a 50% CPU cap. This is
separate from CodeGraph's 600-second indexing deadline and 25% CPU cap.
Reusing Cargo's
mtime cache across changed source hashes is unsafe even if the manifest uses
content hashes. Changing native source/lock inputs makes Check stale;
documentation-only changes do not require compilation. Missing or altered
manager binaries require explicit Cargo bootstrap. Executable text resources
belong under crate `src`; source-owned skill/configuration data remains live
filesystem input and must not be embedded in a deployed binary. Source
subdirectories named `tests` or `examples` are included. Root documentation
and noncompiled Markdown outside `src` do not require recompilation.

Native core `install` / `update` / `check` / `recover` / `disconnect --core-only`
exist. A model-free Process PATH cycle connects, repeats, checks and disconnects
while preserving unrelated token-workflow and subscription records. Default PATH scope is User; optional `--path-scope User|Process` selects
a new connection's scope. An existing installation keeps its recorded scope; an
explicit differing scope requires disconnection first. Process PATH belongs to
the current manager command and descendants and cannot change the calling
PowerShell environment. Owner identity is PID plus creation time. Mutation uses
the Process PATH module mutex. Journal schema 11 remains read-compatible with
schemas 8/9/10. Current PATH receipts retire atomically with their terminating
recovery record. Historical files without a live intent/commit are preserved.
`recover --core-only --preview` reports rollback, finish or no pending operation
without writing. Native `connect` retargets recorded source links after a moved
checkout without opening the old source. Relocated old-layout preview and mutation
upgrade owned script links, preserve an adopted profile, and refuse a foreign
hook replacement. Interrupted relocated old-layout connect leaves a native
journal; Recover restores prior owned script links and the adopted profile
without opening the old source. Combined component recovery, real-CLI
relocation, final private-state cleanup and ordinary global installation remain
unfinished.
Registration currently requires local NTFS with TxF.

Mutually exclusive `--core-only`, `--code-tools-only`, `--subscriptions-only`
and `--token-workflow-only` are accepted. Combined activation is refused.
`--code-tools-only` Check/preview/Install reuse adopted packages and preserve an
existing CodeGraph registration. Update rewrites owned MCP registrations from
adopted inventory without acquiring packages or stopping shared OpenCode
consumers. Subscription and token-workflow Check/preview inspect owned records
only and do not query or stop the live routing proxy. Code-tools Recover/Disconnect
reuse the native MCP
journal. Token-workflow Recover/Disconnect restore or remove only recorded
`harness/bin/rtk.exe` and `harness/bin/harness-rtk.exe` links, preserve foreign
files, and do not follow link targets. When a recorded original CLI is present,
they also edit an ordinary `config.toml` as described below. Subscription
Recover/Disconnect restore or remove owned routing files, source links and an idle Task Scheduler
definition without querying or stopping the live proxy. Subscription
Install/Update write those owned files and source links
and may register an idle Task Scheduler definition without starting the live proxy;
Recover can restore an interrupted restart-policy journal in place without
stopping the live proxy. `configure-restart --subscriptions-only` updates the
owned Task Scheduler restart count/interval in place without starting or
stopping the live proxy.
Token-workflow Install reuses or acquires the pinned RTK archive into
`harness/rtk/packages/<version>/rtk.exe` and a bounded adapter build under
`harness/rtk/build/<identity>/harness-rtk.exe`, then links only those two
`harness/bin` destinations. Preview creates no links. When
`harness/installation.json` records an original native CLI, first-connect
Install enables `code_mode` then `hooks` on an ordinary `config.toml`,
Disconnect disables `hooks` and restores recorded `previousCodeMode`, and
Recover of a not-enabled previous state disables `hooks`. Feature edits are
skipped when that executable is absent and refuse a reparse `config.toml`
without following it.
`cargo test --locked -p harness-core --test serena --jobs 1 -- --test-threads=1`
rejects missing registry, missing Python and incompatible version/status before a
child starts. Adopted-package probes need `HARNESS_CODE_TOOLS_REGISTRY` and
`--ignored`. The helper sets an owned `SERENA_HOME` under the launch home, does
not take a CodeGraph admission slot, and does not replace the live Python seam.

## Diagnostics, dependencies and CBM

`codex-harness.exe diagnose` and core alias `codex-harness-check.exe` provide
read-only source reports. Global cutover has not replaced the script diagnostic
alias.

`dependencies discover --source CHECKOUT` and `dependencies plan --source CHECKOUT`
are read-only. `dependencies audit --package-root DIRECTORY` compares selected
npm package files with their exact official archive in a bounded worker.
`dependencies stage --package NAME --version VERSION --state DIRECTORY` prepares
an owned candidate without activation. `dependencies probe --executable FILE
--kind codebase-memory|nuphus --sha256 DIGEST` checks a digest-pinned native
MCP: Codebase Memory uses an owned inert source sample; Nuphus lists its
protocol/tool contract without desktop or browser actions. The caller must
establish executable and companion DLL provenance first.

`dependencies validate`, `select`, `selected`, `recover-selection` and
`rollback-selection` connect retained candidates to explicit runtime validation
and journaled local selection. BasedPyright requires an explicit Node path and
digest. `dependencies apply|update` dispatches explicit plan actions. Check
and preview stay read-only and do not stage, select, acquire packages or
stop shared OpenCode consumers. Apply stages and selects native CodeGraph and, with an explicit Node path and
digest, BasedPyright. nuphus, codebase-memory, Serena, Graphify and remaining
backends stay pending for Python apply_selected. Complete provisioning and global
connection remain open.

`dependencies resource-check --cache DIRECTORY` reads persisted CBM policy.
`dependencies cbm-index`, `cbm-catalogue` and `cbm-tool` are explicit audited
operations. A cache held by another daemon is refused before launch. Missing
CBM UI JSON is treated as enabled for the audited embedded-UI binary,
regardless of SQLite settings. `mcp codebase-memory` connects explicit CBM
paths and a saved catalogue report to a bounded native stdio connection. The
installed MCP entry requires the build receipt's `runtime_allowed` decision;
source-stale runtimes are rejected. The optional `--broker-root` route passed
an actual two-client index/query/EOF scenario outside the checkout and requires
a fresh private broker root for each service lifetime. Keep these CBM commands
for rollback of the retired registration. Do not continue a CBM port; retire
CBM-only paths only with further consumer-backed evidence. The historical CBM
full index of the locally selected large repository failed the retained
2 GiB / 25% CPU / 600-second memory policy.

Native CodeGraph commands:

```powershell
codex-harness.exe mcp prepare-codegraph --mode Check --codex-home <directory> --dependency-state <directory>
codex-harness.exe mcp prepare-codegraph --mode Install --codex-home <directory> --dependency-state <directory> [--package-root <directory>]
codex-harness.exe mcp apply-codegraph-registration --mode Check --codex-home <directory> [--package-root <directory>]
codex-harness.exe mcp codegraph --package-root <directory> [--project <directory>] [--broker-root <directory>]
codex-harness.exe mcp retire-codegraph
```

`prepare-codegraph` Check is read-only. Install/Update may explicitly stage and
probe the pinned published Windows x64 1.6.0 tree (940 files, archive SHA-256
`cd76c3c3391f2d40abef12b142151950b6d77abc2d8429e648f89eaa90f5b68a`). Ordinary
startup does not download, build or enable telemetry. The command emits the
desired registration (`startup_timeout_sec=30`, `tool_timeout_sec=660`) and
does not write MCP registrations. `apply-codegraph-registration` owns the native
Install/Update/Check/Recover/Disconnect journal; the transitional installer
coordinates it with retained components and passes their planned registrations.
`mcp codegraph` serves the verified package through one account-wide
broker. The exact current directory is the default project. Initial index is
deliberate; every connected indexed root receives native observation and queued
finite catch-up. Indexing episodes share one account slot, a 2 GiB Job, 25% CPU
and a 600-second deadline. Backend idle retirement is 60 seconds; healthy
replacement preserves observation and queued changes. Automatic and manual
refresh commit generations; queries do not copy the database. Failed episodes
preserve the committed checkpoint and require deliberate recovery.
`retire-codegraph` retires the owned account broker. Concurrent installed MCP
acceptance and global CodeGraph activation are complete; see the
[provider contract](code-tools.md#native-codegraph-provider). After a native
rebuild, retire an account broker from the older build before new consumers
connect.

`dependencies stage-python --help` creates an empty offline UV environment from
explicit trusted executable hashes. That unactivated candidate is not eligible
for activation.

## Outcome, usage and helpers

`codex-harness.exe outcome-report --input <private-json> [--markdown]` formats
local attempt accounting without executing anything.
`delegation-usage` reads explicit local telemetry without model calls.
`outcome-run --request PATH --run-model-probes` executes one explicit native
launcher in an isolated temporary case/home and defaults to a model-free skip.
`outcome-prepare`, `outcome-oracle`, `outcome-discover` and `outcome-arm`
cover the five local controlled cases, independent oracles, model-free
discovery and both skill comparison modes. External consumer cases, full suite
migration and global lifecycle remain open. Detailed JSON is private.

The native launcher and argument policy preserve upstream-managed background
process lifetime, package-manager metadata, argv, Unicode, streams, exit codes,
task-effort options and Ctrl+C. Isolated tests refuse self-recursion and a
wrong upstream executable. The actual global launcher still uses its existing
script. Native profile/config
precedence follows the
[official configuration contract](https://learn.chatgpt.com/docs/config-file/config-basic#configuration-precedence).

## Compatibility during concurrent changes

Preserve all `linked-global-kit` source-update scenarios and both pending
deltas at each sync. Executable helpers follow build integrity; source-owned
skill descriptors remain live; accepted skill revisions must be delivered in an
existing session without reloading the entire initial prompt. Ordinary hooks
remain off. The accepted RTK exception and explicit Serena Python/Rust
selection are preserved. The current resource selection must survive every
native port: CodeGraph uses one account-wide worker, the 2048 MiB/25%
CPU/600-second boundary, shared project-isolated Serena workers and
ownership-aware cleanup; restored CBM remains explicit indexing only. Rust
task 5.4 adopts this same CodeGraph owner and evidence; the whole Rust
migration is not a prerequisite for replacement activation.

The original RTK script lifecycle now reads the root workspace manifest/lock
and builds only `harness-rtk`. Native RTK lifecycle and Rust acceptance-helper
migration are still needed.
