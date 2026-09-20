# Rust migration acceptance

This change is unfinished. Source, behavioral parity and global cutover are
separate gates. Passing the current Cargo subset closes none of the unimplemented
lifecycle or foreign-integration requirements. Command entry points, provenance
and operating limits live in [native Rust commands](../rust-native.md). Per-unit
ownership is [rust-migration-map.json](rust-migration-map.json). Generated caches
are not migration units.

The current requirement-to-check map is
[rust-requirement-checks.json](rust-requirement-checks.json).

Observed installed identities at the 2026-09-08 snapshot: Codex CLI 0.153.4,
OpenSpec 1.12.0, OpenCodex 2.44.0, Serena 1.7.0, Codebase Memory 0.10.8,
graphifyy 0.9.55, RTK 0.48.0, basedpyright 1.39.10 and rust-analyzer 1.97.1.
Nuphus 0.2.2 was reported **modified** by discovery, not silently accepted as
an intact upstream package.

## Current increments and remaining work

| Area | Current state |
| --- | --- |
| Portable configuration | Rust live-default bridge is globally active through the script launcher; native TUI writes use local base configuration. Full native lifecycle cutover remains separate |
| Native manager, launcher, PATH, registration, feature edit | Owned isolated acceptance exists, including native launcher argv/Unicode/stream/exit, task-effort, self-recursion and wrong-upstream refusals. Global installer/launcher cutover remains open |
| Core `--core-only` connection | User PATH by default, optional Process PATH, schema-11 journals, preview recover and atomic receipt retirement exist. A model-free Process PATH cycle connects, repeats, checks and disconnects while preserving unrelated component records. Combined component recovery, final private-state cleanup and ordinary global installation remain unfinished |
| Native component selectors | Mutually exclusive `--core-only`, `--code-tools-only`, `--subscriptions-only` and `--token-workflow-only` exist. Combined activation is still refused. Code-tools Check/preview/Install/Update/Recover/Disconnect reuse adopted packages and preserve existing CodeGraph registrations and OpenCode caches; Update does not acquire packages. Subscription and token-workflow Check/preview are record-only and do not query or stop the live proxy. Token-workflow Recover/Disconnect mutate only recorded `harness/bin` links and, when a recorded original CLI is present, an ordinary config.toml. Token-workflow Install reuses or acquires the pinned RTK archive and a bounded adapter build into CODEX_HOME/harness/rtk, then links only harness/bin/rtk.exe and harness/bin/harness-rtk.exe. Subscription Recover/Disconnect restore or remove owned routing files, source links and an idle Task Scheduler definition without querying or stopping the live proxy. Subscription Install/Update write owned routing files and source links and may register an idle Task Scheduler definition without starting the live proxy. Native configure-restart updates an owned Task Scheduler restart policy in place without starting or stopping the live proxy. Native core `connect` retargets recorded source links after a moved checkout without opening the old source. Relocated old-layout preview and mutation upgrade owned script links, preserve an adopted profile, and refuse a foreign hook replacement. Interrupted relocated old-layout connect leaves a native journal; Recover restores prior owned script links and the adopted profile without opening the old source. Real-CLI relocation remains unfinished. Token-workflow Install/Disconnect/Recover apply recorded-CLI feature edits on an ordinary config.toml and skip them when that executable is absent |
| Source diagnostics | Native `diagnose` exists; global alias still the script |
| Dependency discovery, audit, stage, select, apply dispatcher | Bounded native CLI acceptance passed for discover/plan/audit/stage/select. Native apply/update Check and preview are read-only and preserve OpenCode caches; mutation stages/selects native CodeGraph and, with an explicit Node path and digest, BasedPyright; installs a missing rust-analyzer component natively into the adopted toolchain; and leaves serena, graphify and retired candidates pending for their owning tasks. Global connection remains with the 9.x lifecycle tasks |
| Historical CBM runtime / stdio | Native `cbm-index`, `cbm-catalogue`, `cbm-tool` and `mcp codebase-memory` remain the rollback route for the retired registration. Do not continue a CBM port. Retire CBM-only paths only with further consumer-backed evidence |
| CodeGraph adapter | Native `codegraph_*.rs` plus `dependency_codegraph.rs` own lifecycle, response, storage and registration. Replacement activation and installed consumer acceptance passed. Rust task 5.4 adopted the same owner and is closed. Published CodeGraph 1.6.0 / bundled Node remain third-party |
| Shared services | Native WMI creation, independent Job, authenticated reuse, cancellation with confirmed cleanup, retirement and idle cleanup passed with owned fixtures. One fresh broker root per service lifetime. The provider-independent lazy worker forwarder is native (`lazy_stdio`) with admission leases, idle retirement and restart-after-failure. Serena shared-broker integration is native; live global Serena registration still uses the Python seam until 9.x cutover |

| Large-repository index | The historical CBM full index failed the retained memory policy on the current larger checkout. Current CodeGraph comparison, concurrent native MCP, resource and installed-consumer measurements live in the replacement design; this file does not keep a second copy. Rust task 5.4 is closed after the 610-second soak and large-project rerun |
| Serena native boundary | Rust-controlled stdio session around the existing guarded Python entry is proven for two owned Rust projects. Provisioning suppression, selected provider configuration, owned `SERENA_HOME` and missing/incompatible registry failures are checked before a child starts. The Python entry remains until later lifecycle cutover. Task 5.4 integrated the native shared-broker proxy; live global registration stays on the Python seam until 9.x |
| Outcome/usage/oracles | Native CLI consumers exist for report, usage, run, cases, oracle, discover and arm. Deterministic default tests passed; model-backed probes stay explicit `--run-model-probes`. External consumer cases and global lifecycle remain 9.x |
| Python staging | Empty offline UV candidate only; not eligible for activation |
| OpenCodex | Native validation boundary exists; browser-only OAuth, restoration and remaining task 3.1 stay open |
| Structured inspect | Native `harness-inspect.exe` is the skill helper; owned outside-checkout consumer and process/event evidence passed. Global registration remains 9.x |
| Regression helper | Native `harness-observe.exe` is the skill helper; owned outside-checkout consumer, stdin, cancellation and process evidence passed. Global registration remains 9.x |
| RTK | Native token-workflow lifecycle plus `harness-rtk` acceptance helpers exist; global cutover remains 9.x |

Keep sequential `--jobs 1` builds and an explicit separate `--target-dir` for
retained verification binaries. Custom compiler wrappers and ambient Rust build
overrides are rejected before creating build state.

Pinned OpenCodex 2.44.0 public CLI contracts on owned homes: `ocx config validate --json`
accepts the kit source config and rejects invalid candidates without echoing private
fixture values; `ocx restore --json` restores injected routing and writes durable
desired-state, so it is **not** equivalent to `restoreNativeCodex({ skipHistory: true })`;
`ocx login xai` with closed stdin fails without echoing tokens, but stock login still
installs a manual-code waiter and can open a browser. Task 3.1 stays open until a
skipHistory-equivalent restore and a proven browser-only closed-input login exist.

Rust-controlled Serena 1.7.0 startup is proven on owned homes. Default
`harness-core` `serena` tests reject missing registry, missing Python, incompatible
version/status and set an owned `SERENA_HOME` without spawning. With an explicit
`HARNESS_CODE_TOOLS_REGISTRY`, ignored tests start two job-owned MCP sessions through
`tools/code-tools/serena_entry.py`, complete `initialize` with `serverInfo.name == Serena`,
isolate `find_symbol` / `get_symbols_overview` across two Rust crates, keep the second
session after the first closes, deny an upstream installer before mutation, and leave
shared Serena configuration plus adopted Python/entry/rust-analyzer hashes unchanged.
The helper does not take a CodeGraph admission slot. Do not replace the live Python
seam until task 5.4 integrates this boundary.

## Current-path baseline and comparison method (task 1.3)

Owned model-free current-path baselines exist. The recorder is
`crates/codex-harness/tests/migration_baseline.rs`. Run:

```powershell
cargo build --locked -p harness-rtk --jobs 1
cargo test --locked -p codex-harness --test migration_baseline --jobs 1 -- --test-threads=1 --nocapture
```

The test prints a private TEMP evidence root containing `baseline.json`. That
file is machine-local and is not source. It records source/runtime identity
(`HEAD`, dirty index, `Cargo.lock` digest, rustc/cargo, host, binary hashes),
five samples per scenario with the first discarded from the comparison set,
cold/warm medians, range, mean absolute deviation, and Job peak memory as a
separate observation. Candidate results are not recorded here.

Noise tolerance, established from this baseline before candidate outcomes: a
material owned-boundary regression is a warm-median increase greater than
100 ms **and** greater than 10% of the baseline median. Provider, network,
model and live global MCP/service timings are disclosed, not used as rewrite
acceleration evidence. Task 9.4 must reuse this method, sample count and
scenarios. Rust usage alone is not proof of faster execution or reduced
subscription consumption.

Covered current-path oracles on owned targets: native launcher argv/Unicode
stdin/nonzero exit; script-launcher fallback with missing module; native core
Check on a missing installation; script `install.ps1 -CoreOnly -Mode Check`
on missing homes; native Diagnose plus mixed selector refusal; missing-build
Check; MCP stdio Unicode/IDs/output purity and incomplete-input failure;
process timeout 124, streams and foreign-process preservation; console
Unicode/nonzero/cancel 130; bounded OpenCodex process fixture nonzero and
timeout; RTK exec once, hook rewrite and malformed-hook silence.

Concurrent requirement reconciliation: preserve all `linked-global-kit`
source-update scenarios and both pending deltas at each sync. Ordinary hooks
remain off; the accepted RTK exception and explicit Serena Python/Rust selection
are preserved. No other change's tasks are closed by recording this map.

The [graph-provider ownership/order map](../../openspec/changes/migrate-harness-to-rust/design.md#graph-provider-ownership-and-order)
prevents a second CBM port and a circular migration dependency. Whole Rust
migration is not a prerequisite for replacement activation. The dated JSON
inventory still lists transitional Python/PowerShell CBM/code-tool units as
pending; those require replacement/consumer-backed retirement, not automatic
translation of every old file. Generic transport/resource oracles remain
required. Native CodeGraph can activate through the current lifecycle after
parent global acceptance; full native cutover subsequently adopts that
selection. `global/code-tools.json` already names CodeGraph as the planned
native manager; that is selected source, not live evidence.

## CodeGraph ownership review (replacement 2.6 / 4.1)

Inspected 2026-09-11 against current Rust modules, Cargo members, CLI dispatch,
`global/code-tools.json`, `tools/code-tools.psm1` and
`tools/code-tools/registration.py`. The dated JSON inventory remains a
2026-09-08 snapshot.

All new first-party CodeGraph lifecycle, response, storage, registration and
executable acceptance is Rust. `harness-core` owns
`codegraph_account`, `codegraph_broker`, `codegraph_catalogue`,
`codegraph_generation`, `codegraph_integration`, `codegraph_observer`,
`codegraph_registration`, `codegraph_response`, `codegraph_runtime`,
`codegraph_scheduler`, `codegraph_stdio`, `codegraph_store`,
`codegraph_transport` and `dependency_codegraph`. `codex-harness` CLI
forwards `mcp prepare-codegraph`, `mcp apply-codegraph-registration`,
`mcp codegraph` and `mcp retire-codegraph` to that owner; `mcp codegraph`
serves through the native account broker. Workspace members stay
`crates/harness-core`, `crates/codex-harness` and `tools/rtk-adapter`.

The exercised process chain is native manager → Job-owned `node.exe` with the
pinned published `lib/dist/bin/codegraph.js` entry, `CODEGRAPH_NO_DAEMON=1`,
and native observation/sync rather than a detached upstream daemon. Pins live
in `dependency_codegraph` (package `@colbymchenry/codegraph` 1.6.0, bundled
Node, kernel, archive/tree digests). `global/code-tools.json` already names
that native manager; it is selected source, not live global evidence.

Executable acceptance is Rust: `codegraph_mcp`, `codegraph_transport`,
`codegraph_failures`, `codegraph_dependency`, `codegraph_install`,
`codegraph_consumers` (including the retained-tools Codex app-server probe),
`support/codegraph_comparison.rs`, `support/codegraph_processes.rs`,
`codegraph_generation` and `harness-codegraph-fixture`. No first-party
CodeGraph `py`/`ps1`/`js`/`mjs`/`ts`/`cs` owner, embedded script or
`include_str!`/`include_bytes!` helper was found. Isolated
`codegraph_install` proves selected activation journals through the native
command without invoking Python.

Existing transitional lifecycle may dispatch only. `tools/code-tools.psm1`
resolves the native manager and calls those MCP commands.
`registration.py --plan-only` remains a generic retained-MCP planner handoff;
native `apply-codegraph-registration` writes CodeGraph and retires owned CBM.
Nuphus `browser_evaluate` JavaScript in the retained-tools consumer is a
one-off payload for the existing browser API, not an owned CodeGraph
subprocess. The model-probe consumer launches the installed PowerShell Codex
launcher as a consumer, not as provider logic.

CBM-only native commands and tests remain consumer-backed compatibility and
rollback: `cbm-index`, `cbm-catalogue`,
`cbm-tool`, `mcp codebase-memory`, `cbm_*` crate tests,
`fake_cbm_worker` (the transitional Python proxy was deleted 2026-09-16).
They are not queued for another CBM port. Broader agent-workflow comparisons keep their
existing change owner. Deletion requires separate consumer-backed retirement.
Generic Job, pipe, broker and cancellation checks cover retained compatibility
and the replacement. Current measurements belong in the replacement design.

Native `dependency_discovery` now accepts the selected catalogue and emits
CodeGraph once. Ten default discovery tests pass, including explicit legacy
CBM consumers and read-only failure handling. Rust task 5.4 stays open:
remaining generic MCP lifecycle, and replacement global activation 4.2–4.4
are unfinished. Isolated ownership does not close 5.4 or claim a live global
Install.

## Native source hygiene

From this checkout:

```powershell
cargo run -p codex-harness --bin harness-source-check -- --root .
cargo run -p codex-harness --bin harness-source-check -- --root . --private-terms <external-file>
```

The checker inspects current Git-listed tracked and new existing files, tracked
caches, optional private terms without echo, machine-home paths in docs/shared
configuration, shared `[projects.*]` records and inline Markdown local
targets/anchors, skipping fenced examples, templates and URLs. Use it as a
development check. It does not inspect Git history.

## Installer acceptance closure (migration 4.2, 2026-09-14)

Native install/update/preview/Check and the mutually exclusive component
selectors are implemented for core, code-tools, subscriptions and
token-workflow. Verification on the current tree, all model-free:

- `core_install` unit suite: 11 passed, covering build/checksum refusal
  before home creation, foreign-destination refusal, fresh/repeat process-scope
  connect-check-disconnect with unrelated records preserved, relocated and
  old-layout metadata upgrade, and interrupted relocated recovery.
- Component lifecycle suites: 41 passed across code-tools, subscriptions and
  token-workflow, including preview/check silence, adopted-package reuse,
  foreign preservation and pending-journal handling.
- CLI state acceptance: `installation_state` (3) and `installation_lock`
  (3) passed, covering legacy metadata adoption, reparse/concurrent-edit
  preservation and cross-process lock exclusion.
- Explicit real-CLI acceptance on upstream Codex 0.154.0 with the current
  release launcher and manager: 5 passed (414.96 s), covering upstream
  discovery with a recursive-selection refusal, saved-selection update with
  missing PATH, native diagnostic alias, user-PATH-preserving disconnect,
  repeat install with runtime-failure rollback and relocation, legacy import
  with adopted profile and foreign hook refusal, and process-scope CLI
  check/disconnect without registry writes.
- Full-checkout acceptance from an immutable native build of the current
  source: passed (76.09 s), covering inventory-matched preview, install,
  check, idempotent update and full disconnect with the user PATH restored.

Private run receipts remain in their owned temporary roots; they are not
source. Combined component activation remains intentionally refused and is
not part of this task. Migration 4.3 keeps recover/disconnect/relocation and
real-CLI relocation verification open.

## OpenCodex probe progress (migration 3.1, 2026-09-14)

The Rust probes in `harness-core` against the adopted upstream package
(@bitkyc08/opencodex 2.44.0, explicit package root) all passed: candidate
validation (valid/rejected/cancelled/timed-out with unchanged source),
restoration success cleaning an injected base URL and writing durable desired
state, and closed-stdin login failure without secret echo. The successful
browser-only OAuth login mapping remains unproven, so 3.1 stays open; 6.2
consumes only the proven path.

## Recover/disconnect/relocation acceptance closure (migration 4.3, 2026-09-14)

All model-free suites on the current tree passed:

- `registration`: 23 passed, 7 alias/process-fixture opt-ins, covering
  guarded disconnect without touching sources, reuse of matching preexisting
  links, foreign/dangling/wrong-target rejection, preparation rollback, lock
  serialization and process-interrupted recovery.
- `registration_finish`: 4 passed, covering foreign commitment refusals,
  mixed finish/repeat candidate retention and backup preservation.
- `link_changes`: 3 passed plus the explicit reparse-mutation opt-in
  (retargeted backup alias rejected before any undo) passed with the local
  privilege.
- `legacy_pending`: 20 passed, covering killed-recovery resumption, exact
  state/path restoration after interruption, foreign-object and foreign-journal
  refusal before mutation, dangling-target restoration without traversing the
  relocated source, wrong-type link preservation and CLI-level
  preview/recover of user- and process-scope pending operations.
- Component recover/disconnect oracles ran within the 41 lifecycle-suite
  passes recorded for 4.2, including adopted dependency preservation.

Combined single-command recovery across components stays intentionally
refused, matching component independence; complete-installation relocation
and rollback acceptance remain with task 9.1.

## Native launcher acceptance closure (migration 4.4, 2026-09-14)

All model-free launcher suites on the current tree passed: `native_launcher`
CLI suite 9 passed (immutable/ambiguous registration refusal, manager-link
integrity with source-stale recovery, argv/Unicode/stdin/streams/cwd/nonzero
exit through the actual launcher binary, explicit native precedence with
package-manager metadata, stale/missing/altered/interrupted installations
refusing launch or build, fail-open to the verified upstream with degraded
notice, upstream background lifetime and real console interaction with
Ctrl+C returning the upstream exit code) and `launcher` policy suite
3 passed (native profile command dispatch and boundaries, task-effort
selection without prompt interpretation or settings override, explicit roots
using the effective cwd). Library-level oracles additionally refuse
self-recursion, a wrong upstream and a different selected launcher, and apply
task effort before the registered upstream.

Native core install registers the Rust launcher and manager binaries and
removes the owned legacy script launcher link; end-to-end runtime receipts
through the real upstream passed in the 4.2 acceptance runs. The live global
installation keeps the script launcher until the journaled cutover in 9.2.

## Shared npm MCP install port (migration 5.2 progress, 2026-09-14)

New `dependency_npm_install` owner in `harness-core`: missing codebase-memory
and nuphus packages now install natively instead of dispatching to the Python
`apply_selected` seam. The path stages through the bounded manager worker,
requires the existing Node runtime (never installs a duplicate), refuses a
shared npm lockfile or pre-existing installation, validates ordinary
ancestor paths, then activates the candidate into the shared user npm tree
through a `create-directory` transaction journal with tree identities.
Staged and installed executables are digest-pinned MCP probes; Nuphus also
requires the audited official 0.2.2 binary fingerprint after assembling the
platform package. A provenance marker is written beside the installed
executable (foreign markers are never overwritten) and folded into the
journal's installed identity. Any post-activation failure rolls the
activation back to prior absence; `recover_journal` finishes or rolls back
one journal, refusing foreign, damaged or occupied states without mutation.
Apply preview/Check remain read-only; the OpenCode shared-cache snapshot
guard is unchanged. Per-item provisioning failures surface as `failed`
results with fixed reasons, never as partial success.

Verified model-free on the current tree: six new unit tests (install with
journal/marker/probes, post-activation rollback to absence, preservation of
existing installations and shared lockfiles, recovery commitment/damage/
foreign refusal, input validation, foreign marker protection), the updated
`dependency_apply` suite (6), the full `harness-core` lib suite (508 passed,
0 failed, 37 explicit opt-ins) and the `dependency_apply` CLI tests (3).
`cargo fmt --all -- --check`, clippy with `-D warnings` for both crates and
the native source check are clean; the clippy run also cleared eight
pre-existing findings from a newer toolchain in untouched modules.

Still open for 5.2: shared-tree replacement updates (stage-compatible-update
for existing codebase-memory installations), a CLI recovery entry for
interrupted npm-tree journals, the remaining language backends, and the
opt-in real-network provision acceptance. Serena stays reuse-only (matching
the seam) and Graphify update/port belongs to 5.5.

## Shared npm recovery and real-network acceptance (5.2 progress, 2026-09-14)

`dependencies recover-npm --state DIRECTORY [--rollback-committed]` now scans
owned transaction journals with bounded iteration, finishes or rolls back
interrupted preparations, keeps committed installations by default and reports
foreign or damaged journals without mutating them. Two unit tests cover
commitment handling, interrupted-preparation restore and foreign reporting.

Real-network acceptance from the current checkout through the actual CLI into
fresh owned roots: the plan downloaded the official nuphus 0.2.2 package and
its Windows x64 platform companion, verified the audited binary fingerprint,
assembled the platform payload, probed the staged executable, activated the
installation into the shared npm tree with a committed journal, rediscovered it
as adopted and passed the installed-original protocol probe (38-tool contract,
model downloads disabled, Job-contained, clean owned-tree shutdown). The
provenance marker was written beside the installed binary and `recover-npm`
against the resulting state reported the journal as committed without touching
it. The pinned CodeGraph package staged and selected in the same run; serena,
graphify, python and rust behaved exactly as designed (pending/reuse policy,
no acquisition). An initial run exposed a staging-root containment mismatch
(the native owner uses `dependency-staging`, not the legacy seam spelling);
the check now uses the native root and the fixture matches it. Private run
receipts remain in the owned temporary roots.

Remaining for 5.2: the language-backend provisioning ports. The shared-tree
replacement update only served rollback codebase-memory installations, which
follow the CBM retirement path in 8.1 rather than a new port.

## Language backend ports and 5.2 closure (2026-09-15)

The rust-analyzer language backend is now native. New
`dependency_rust_component` owner in `harness-core`: it installs only the
missing `rust-analyzer` component of the already selected installed rustup
toolchain, never a toolchain. The cohort artifact digest from the official
channel manifest is verified before rustup runs; an existing analyzer, a
foreign component receipt, busy consumers or an unavailable toolchain stop the
operation without mutation. The install is journaled as a
`rustup-component-install` transaction with manifest/executable/component
identities, and `dependencies recover-npm` dispatches those journals beside
the npm ones with the same committed-default, bounded-iteration and
foreign/damage-refusal contract. Compatible selection stays cohort-held:
`stage-compatible-update` retains the installed component with the recorded
toolchain-policy reason, matching the seam. BasedPyright keeps its explicit
Node path and digest requirement and stages through the native
stage/select owners; serena stays reuse-only and graphify follows 5.5.

Verified model-free on the current tree: the four new
`dependency_rust_component` unit tests (committed journal and component
receipt, existing-analyzer and missing-prerequisite preservation, invalid
toolchain and unavailable-target refusal, recovery commit/rollback/foreign
journal handling), the `dependency_apply` dispatch tests including the rust
`install-required` arm, the full `harness-core` lib suite single-threaded
(520 passed, 0 failed, 37 explicit opt-ins), the `dependency_apply` CLI suite
(3) and `dependency_discovery` CLI suite (10 passed, 1 opt-in).
`cargo fmt --all -- --check`, clippy `-D warnings` for both crates and the
native source check are clean; the source check initially reported sixteen
broken local links left by concurrent archive moves, which were retargeted to
the archive layout before this record. A live read-only
`dependencies apply --check` through the built CLI against the real profile
reported `mutated=false`, `packages_acquired=false` and `model_calls=0`
with serena/python/rust pending or reuse-only and codegraph/nuphus merely
previewed, confirming Check silence and no runtime acquisition on the real
machine. The real missing-component install path is exercised through the
fixture rustup and network client injection; mutating the adopted developer
toolchain is not an acceptance target.

## Provider-independent lazy forwarding (migration 5.3, 2026-09-15)

New `lazy_stdio` owner in `harness-core` ports the seam's `lazy_stdio.py`
contract: one serialized backend MCP worker per forwarder, owned through a
Windows Job with anonymous pipes, started only on the first request, retired
by an idle watchdog after a bounded window, and restarted on the next request
after a failure. A `Lease` trait gives providers the seam's admission
semantics (acquire before the request, release after it, refusal answers an
`isError` result without losing the worker); `before`/`after` hooks cover
preflight rejection, argument rewrite and result post-processing. Worker
stdout is parsed strictly as framed JSON-RPC with per-generation request IDs
and never echoed; worker stderr goes to an owned file. A request failure or
cancellation reclaims the whole tree before anything is answered; only an
unconfirmed reclamation poisons the forwarder and closes the connection. Dead
workers restart on demand and foreign processes are never adopted; owner death
kills the tree through the already verified Job close-on-owner semantics.
CodeGraph keeps its owning adapter and scheduling evidence; no CBM broker or
indexer path was extended.

Verified model-free through the real fixture binary over real Windows pipes
(`codex-harness` `lazy_stdio` suite, 10 passed single-threaded): lazy start
and identity-stable reuse, idle retirement with a same-executable foreign
process preserved and a fresh generation on the next request, cancellation
reclaiming the tree with the connection staying usable, deadline failure with
clean restart, immediate-exit startup as a per-request failure, admission
lease wrap and refusal, preflight rejection plus argument rewrite, after-hook
transform/failure without worker loss, and an end-to-end initialize/list/call
flow through `mcp_stdio::serve_fallible` where a cancelled in-flight request
is suppressed, client EOF closes the connection and only JSON-RPC leaves the
forwarder. `cargo fmt --all -- --check`, clippy `-D warnings` for both
crates and the native source check are clean. The Python seam remains the
live registration until 5.4/5.5 port its provider consumers onto this owner.

## Serena shared-worker route identity (migration 5.4 progress, 2026-09-15)

First 5.4 increment: new `serena_route` owner in `harness-core` ports the
seam's route parsing, registered-project resolution, resource policy and
worker configuration identity. Every native CLI option keeps its order while
project selection is removed from the forwarded arguments (`--project`,
`--project=`, `--project-file`, one positional), `--project-from-cwd`
resolves through `.serena/project.yml` or `.git` ancestors and refuses an
explicit combination, context/mode file values resolve against the caller's
cwd, and a stdio proxy still refuses any non-stdio transport. Registered
project names resolve through the Serena home configuration with ambiguity
and removed-registration handling before path fallback. The configuration key
covers the route (project normalized case-insensitively, cwd only when no
project anchors the worker), the protocol version and the bytes of every
global/project/context/mode/prompt/argument configuration file. YAML reading
is a strict bounded subset (scalar mappings, block/flow scalar sequences,
comments, BOM) that fails loudly on nested or richer syntax instead of
guessing; no YAML dependency was added.

Verified model-free with nine unit tests: managed `--project-from-cwd`
argument shape, explicit/positional project removal and conflicts, missing
option values and non-server entries, stdio-only transport enforcement,
context/mode file resolution (both `--x v` and `--x=v`), registered-name
resolution with duplicate/removed cases, configuration-key stability,
case-normalized project identity and sensitivity to configuration bytes,
protocol and cwd, YAML subset acceptance/rejection and the shared resource
policy (max_projects 3, idle 300s) with invalid-policy refusal.
`cargo fmt --all -- --check`, clippy `-D warnings` for both crates and the
native source check are clean. Still open for 5.4: the shared worker pool
and broker port over the verified primitives, the native stdio proxy with
tool filtering, installed MCP integration, and the concurrent/large-project
acceptance through actual foreign processes.

## Serena shared worker pool (migration 5.4 progress, 2026-09-15)

Second 5.4 increment: new `serena_shared` owner in `harness-core` ports the
seam's project pool. Matching project/mode/configuration selections share one
serialized native worker (`SharedWorker` trait; the real factory wraps
`serena::Session::start_shared` around the guarded entry with the
shared-worker marker, removed-project list, per-worker stderr sink and the
client's initialize parameters with empty capabilities). Capacity evicts the
least recently used worker, dead workers and changed-configuration selections
are replaced on demand, `activate_project` rewrites the route and reports
`tools_changed`, `remove_project` makes the mutating client
configuration-incompatible before the mutation, every tools/call carries the
client identity in `_meta`, clients keep their own routes (re-registering an
unknown client from its cached route), the client registry is bounded at 128
with 32-hex identities, and idle reaping closes workers and forgets clients.
`serena.rs` gained the shared spawn path and parameterized initialize while
keeping its direct-session contract; the route identity now includes removed
projects and the mutation owner, matching the seam's route dictionary.

Verified with nine model-free unit tests through a fake worker factory
(sharing, LRU capacity eviction, dead-worker replacement, configuration-byte
replacement, activation route/_meta/tools_changed, removal incompatibility and
worker replacement, client identity/capacity/re-registration, disconnect and
idle reap, close) plus the real-process opt-in
`shared_pool_reuses_one_worker_and_isolates_projects` with the adopted
registry: two clients of one Rust crate project shared a single Serena worker
identity, a second project got its own worker, find_symbol returned isolated
sources through both, and disconnecting one client kept the shared worker
serving the other. `cargo fmt --all -- --check`, clippy `-D warnings` for
both crates and the native source check are clean. Still open for 5.4: the
broker transport and native `mcp serena` stdio proxy with tool filtering,
installed MCP integration, and the concurrent/large-project acceptance.

## Serena broker and native stdio proxy (migration 5.4 progress, 2026-09-15)

Third 5.4 increment: the shared pool now serves through the verified generic
broker machinery. New `serena_broker` owner in `harness-core`: one
authenticated broker per CODEX_HOME (root `harness/runtime/serena-broker`)
owns the pool behind the bounded service loop with a one-second reaper,
240-second request bound, eight connections and idle exit after the pool
idle window; a source identity covering the manager, adopted interpreter,
guarded entry, registry, tool resources and shared Serena home prevents
silently reusing a stale broker. The WMI service dispatch gained the
`serena` arm (`--harness-service-run`). New `serena_stdio` owner plus the
`codex-harness mcp serena` CLI port the proxy contract: one lazily connected
client per stdio connection forwards JSON-RPC requests through the
authenticated exchange, caches its route, re-registers from it after broker
state loss, emits `notifications/tools/list_changed` after
`activate_project`, filters the memory/onboarding/configuration catalogue
(unless `HARNESS_SERENA_UNFILTERED=1`), answers per-request failures as
`-32603` without closing, ignores notifications like the seam and
disconnects on EOF. Shared workers now use the user's Serena home for
configuration identity and `SERENA_HOME`, with per-worker stderr under the
owned runtime root.

Verified with four new model-free unit tests (hidden-tool filtering, tools
capability change advertisement, catalogue filtering, route echo round-trip
including removed/mutation state) alongside the existing suites: 24
`harness-core` lib tests matching `serena` pass, the integration suite is
green (5 passed, 3 explicit opt-ins), `cargo fmt --all -- --check`, clippy
`-D warnings` for both crates and the native source check (626 files) are
clean. Still open for 5.4: end-to-end acceptance through the actual broker
service process and the `mcp serena` CLI with the adopted package, the
installed MCP registration switch, concurrent-CLI acceptance and the
CodeGraph large-project evidence review/rerun.

## Serena end-to-end acceptance (migration 5.4 progress, 2026-09-15)

Fourth 5.4 increment: the broker location now uses the same anchor pattern
as the account services — an owned record under
`CODEX_HOME/harness/runtime/serena-broker.json` names a private prepared
root, created under a per-account admission mutex with ownership checks, so
no directory inside CODEX_HOME needs broker internals and a foreign record is
preserved. Client identities are 32-hex session tokens matching the seam.
Broker per-request failures now answer as error envelopes that keep the
service serving (lock poisoning stays the only fatal condition), the proxy
 reads its stdin with the blocking connection-deadline pattern instead of a
poll deadline (short poll deadlines made idle proxies see spurious EOF and
disconnect early), and the pool reports idle when no client remains so a
retiring broker drains promptly instead of waiting out the worker idle
window.

End-to-end acceptance through real processes passed
(`codex-harness` `serena_stdio` opt-in, 27.7 s): three spawned
`mcp serena` CLI proxies against the real WMI-launched broker service and
the adopted Serena package. The first proxy completed initialize
(`serverInfo.name == Serena`), a filtered tools/list without
onboarding/list_memories/read_memory, and an isolated find_symbol; a second
proxy of the same project shared that one worker (broker status: two
clients, one worker); a third proxy on another project got its own worker
(two workers) with isolated symbols; client EOF closed every proxy cleanly
while the broker kept both workers; explicit retirement then drained and
exited the service. Model-free checks after the fixes: 24 `harness-core`
lib tests, the Serena integration suite (5 passed, 3 opt-ins), fmt, clippy
`-D warnings` for both crates and the native source check (627 files) are
clean. Still open for 5.4: the installed MCP registration switch, longer
concurrent-CLI/600-second acceptance and the CodeGraph large-project
evidence review/rerun.

## Serena registration switch projection (migration 5.4 progress, 2026-09-15)

Fifth 5.4 increment: the native prepare projection now switches the Serena
connection. `mcp prepare-codegraph` gained an optional `--source` root; with
an adopted interpreter in `CODEX_HOME/harness/code-tools.json` and the guarded
entry present in the source, the projection emits a native `serena`
registration (manager command, `mcp serena` with the resolved interpreter,
entry, registry, CODEX home and source root, 30/660-second timeouts) beside
the CodeGraph entry, and the existing transactional retained-registration
flow applies it with unchanged journaling and conflict rules. Without a
source root or with a missing interpreter/entry the projection stays with
the current seam registration instead of guessing. The transitional
dispatcher passes the checkout root, and `mcp serena` now defaults the
Serena home to the user profile exactly like the seam's runtime resolution.

Verified: a new unit test covers the projection (adopted interpreter
switches with the full argument surface; missing registry, interpreter or
entry keeps the seam), the `codegraph_install` suite still passes
(19 passed, 1 opt-in), `mcp_cli` (4), fmt, clippy `-D warnings` for both
crates and the native source check (627 files) are clean, and a read-only
`prepare-codegraph --mode Check` against the real installed CODEX_HOME
emitted the native Serena registration with the machine's adopted
interpreter and this checkout as the source root. Still open for 5.4: the
longer concurrent-CLI/600-second soak and the CodeGraph large-project
evidence review/rerun decision.

## CodeGraph acceptance gap analysis (migration 5.4 progress, 2026-09-15)

The 5.4 rerun decision was resolved by diffing the serving path against the
last commit from the replacement-acceptance era (c6d99c0): the checkpoint
rotation retry in the generation store and the transport notify addition
landed after that acceptance, so a rerun on current source is required. The
rerun attempt then exposed a larger, pre-existing reconciliation gap in the
dirty tree: the uncommitted model-surface narrowing in
`codegraph_catalogue` (`EXPOSED = [codegraph_search, codegraph_detail]`,
matching the documented control-CLI contract) was never reconciled with the
`codegraph_mcp` default tests or the large-project acceptance harness.
Concretely observed with the real adopted package: the acceptance harness
failed on `codegraph_index`/`codegraph_sync` through the MCP catalogue
(`Unknown tool name`), on the in-process control path while a lane broker
held the account-wide indexing slot (`WouldBlock`), and on a fabricated
broker client identity (`service process identity mismatch` — the broker
verifies the connecting executable). The comparison harness was partially
migrated (deliberate maintenance operations moved off the catalogue), and a
private oracle manifest for the pack checkout plus the locally selected
large project was reconstructed; the run is not yet green.

The same narrowing leaves the default `codegraph_mcp` suite red
(`ambiguous_fanout` calls `codegraph_callers` through the catalogue;
leaked fixture services from failing runs hold the account semaphore until
cleared). This breakage predates this session's Serena work and is not
caused by it; finishing the reconciliation is required 5.4 work: route
deliberate operations through the lane broker with a real spawned client
executable, migrate the affected default tests to the query-only catalogue
plus control path, then rerun the private large-project acceptance. The
installed account broker was retired once for the attempt and restarts
automatically on next use; all leaked services from the attempts were
stopped afterwards.

## Surface reconciliation and green large-project rerun (migration 5.4, 2026-09-15)

The narrowing reconciliation is complete and the large-project acceptance is
green again. The two genuinely failing default tests were migrated to the
narrowed contract: the catalogue test now asserts exactly
`codegraph_search` plus `codegraph_detail` with maintenance names absent,
and the fanout oracle was repurposed to verify that removed tools are
rejected with a bounded protocol error while the same over-broad fixture
answer through the exposed surface stays bounded with its narrowing hint
(the retained-original oracle remains covered by the large-answer detail
recovery test). With clean process state the default `codegraph_mcp` suite
passes: 10 passed, 0 failed, 7 explicit opt-ins.

`mcp codegraph-control` gained `--broker-root` so a deliberate operation
can route through a chosen bounded shared worker as the real spawned client
executable (satisfying the broker's identity verification) instead of
starting a second direct runtime that would contend for the account-wide
indexing slot. The comparison acceptance harness now drives every deliberate
index/sync/status that way, including the concurrent phase's first index of
the owned fixture.

The private large-project acceptance then passed end to end (479.9 s, real
published package, model-free): the pack checkout indexed 375 eligible files
in 40.3 s and the locally selected large project 1002 eligible files in
59.5 s with zero missing or extra indexed files; all six source-oracle cases
returned with zero lost oracles against the raw upstream; the owned-probe
watcher phases verified add/change/old-removal/rename, manual sync,
disconnect-time deletion and final sync on both roots; and the concurrent
phase kept the pack checkout, the large project and an owned fixture active
under one broker with committed generations and per-root refresh evidence.
fmt, clippy `-D warnings` for both crates, the native source check (627
files, 0 findings) and the default suites are clean; no services were left
running.

## Concurrent 610-second soak and 5.4 closure (2026-09-15)

The remaining 5.4 soak passed through the actual published package and native
MCP clients: `published_three_projects_refresh_without_queries_and_share_clients`
with `CODEGRAPH_ACCEPTANCE_LONG_LIVED=1` finished in 704.19 s. The private
report recorded 628.12 s of live service after the first sharing comparison,
three committed generations, broker retirement, and process samples that kept
three Node backends while extra frontends added only native/console processes
(8/10/12/14 owned processes; 144-151 MiB private bytes). Automatic add/change
burst/rename/delete, last-client disconnect, continued updates in another root
and reopen catch-up all committed without queries triggering the refresh.

The soak exposed a leftover MCP-envelope read on the control-CLI path. Tests
now parse the native runtime/broker payload for deliberate index/sync/status,
and remaining native-entry catalogue calls use the query-only surface plus
protocol rejection for removed tools. Isolated Serena registration apply writes
the native `mcp serena` proxy into an owned Codex home and preserves foreign
servers. Published-package prepare with `--source` emits that same native
Serena spec.

Live global Serena remains on the Python seam in this session so an Install
rewrite does not interrupt the current MCP catalogue. Task 9.x owns activating
that prepared native registration globally. fmt, clippy `-D warnings` for both
crates, the native source check (627 files, 0 findings), default
`codegraph_mcp` (9 passed after the soak leftover was retired) and default
`codegraph_install` (20 passed, 2 explicit opt-ins) are clean.

## Native Nuphus stdio (migration 5.5 progress, 2026-09-15)

Graphify stays retired from the managed selection and is not ported. Launch,
registration and bootstrap refuse it; leftover disconnect ownership may still
name it. Physical Graphify source deletion was blocked by policy, so retirement
is refusal rather than tree removal.

Native Nuphus serve is `codex-harness mcp nuphus`. It audits the official
0.2.2 original digest, caches the catalogue under the account directory, starts
the official binary only on the first tool call, rewrites snapshot refs, bounds
desktop screenshots, admits desktop tools through `desktop.lock`, and owns a
private loopback Chrome/Edge/Chromium CDP unless `NUPHUS_MCP_BROWSER_CDP_URL`
is already set. Idle retirement expires refs and reclaims the owned browser.
Live global Nuphus registration remains the Python seam until 9.x.

Model-free default coverage: protocol unit tests, native stdio against the
probe fixture (schema repair, path-only vs image screenshot, expired refs),
CLI help and digest refusal. Owned-target acceptance against the actual
external tool is the ignored
`native_nuphus_owned_browser_on_actual_tool` case with
`HARNESS_NUPHUS_ORIGINAL`.

Owned-target evidence (2026-09-15):
`HARNESS_NUPHUS_ORIGINAL` pointed at the audited official 0.2.2
`nuphus-mcp.exe` digest
`9a07112f17a964d9c0b1a54653af95559d7de33cce1cb3dffd60dfc4c85ccfb0`.
`native_nuphus_owned_browser_on_actual_tool` passed in 61.22s: initialize and
tools/list stayed local with no browser profile, the owned off-screen window
was read and screenshotted path-only plus native image, a private loopback
page was navigated and snapshotted with bound refs, and owned browser
profiles were gone after close. Live global Nuphus registration was not
rewritten. Graphify remains unported.

## Native subscription host (migration 6.1 progress, 2026-09-15)

Task Scheduler now registers `codex-harness subscription-service --state FILE`
instead of PowerShell `opencodex-service.ps1`. The native host validates the
owned descriptor, assigns the OpenCodex child to a 2048 MiB kill-on-close job
before resume, publishes the Grok role only after `/readyz`, withdraws that
role on exit, and retries runtime failures at most three times. Isolated
fixtures passed readiness/cleanup, foreign-descriptor refusal and retry
exhaustion without targeting the live global proxy. Authentication/restoration
and in-place restart-policy recovery remain 6.2/6.3.

## Native subscription authentication (migration 6.2 progress, 2026-09-15)

The host restores native Codex routing through skipHistory-equivalent Rust
cleanup instead of Bun `opencodex-native-restore.mjs`. Source policy now
covers hostname, defaultProvider, xAI Chat/OAuth selection, hidden natives,
Z.AI Coding Plan, sidecar rules and the middle role without echoing private
values. Install records an already adopted 2.44.0 package identity when
present. `codex-harness subscription-login xai|zai` owns browser-only OAuth
and the ACL-hardened Z.AI key store. Isolated fixtures cover restore privacy,
Z.AI success/failure and CLI login without targeting the live global proxy.
Restart-policy recovery remains 6.3.

## Native subscription restart policy (migration 6.3 progress, 2026-09-15)

Native configure-restart writes RestartOnFailure Count 3 / Interval PT1M in
place. The host default retries runtime failures three times, one minute
apart, at 2048 MiB. Disconnect refuses a running owned task without stopping
it. An isolated running fixture can receive an in-place policy update without
being stopped. Interrupted restart-policy journals recover in place, and
foreign task XML is preserved. Isolated fixtures never target the live global
proxy.

## Native RTK acceptance helpers (migration 7.1 progress, 2026-09-15)

Token-workflow Install/Recover/Disconnect remain native. `harness-rtk` now has
Rust acceptance covering exec-once identity, hook rewrite of literal
`harness-rtk.exe exec`, silence for malformed/Stop/Write/shell-control input,
disable/missing-dependency bypass, and raw passthrough for filter failure and
oversized stdout. The transitional Python RTK suite was deleted with the
2026-09-16 legacy-test retirement.

## Native inspect and observe helpers (migration 7.2 progress, 2026-09-15)

Linked skills now prefer native `harness-inspect.exe` and
`harness-observe.exe`. Python/PowerShell skill scripts remain transitional.
Owned outside-checkout consumers passed for inspect success evidence and
observe stdin/process receipts without using the checkout as cwd.

## Native delegation and outcome tools (migration 7.3 progress, 2026-09-15)

Native `delegation-usage`, `outcome-run`, `outcome-oracle` and
`outcome-report` suites passed: privacy/attribution limits, skipped default
model probes, live executable fixtures and retained private evidence. Python
outcome/delegation scripts remain transitional until task 8 retirement.

## Native remaining suites and requirement map (migration 7.4, 2026-09-15)

The current requirement-to-check map is
[rust-requirement-checks.json](rust-requirement-checks.json). It covers the
design matrix with native tests. Dated [rust-migration-map.json](rust-migration-map.json)
remains a 2026-09-08 file inventory and is not remaining work. Native ConPTY
answers CSI 6n/c queries. `harness-observe` and `harness-inspect` refuse
declared analysis samples under `tests/fixtures/lsp/`. Native TUI smoke covers
isolated ordinary `codex` resolution by default; actual /status /model /trust
writes stay opt-in. Graphify stays retired and is not ported. First-party
Python/PowerShell/JS/C# paths remain until tasks 8.1-8.2.

## Legacy MCP retirement (migration 8.1, 2026-09-15)

Live managed MCP servers are Serena, CodeGraph and Nuphus. Graphify, Codebase
Memory and harness-lsp are retired from launch and registration. Classification
lives in [legacy-mcp-retirement.json](legacy-mcp-retirement.json). Shared
`tools/lsp/broker.py` remains because Serena still imports it. First-party
Graphify/CBM/LSP adapters stay as leftover source until task 8.2 deletes
remaining first-party Python/PowerShell/JS/C# paths after the ownership check.

## Executable ownership check (migration 8.2 progress, 2026-09-16)

Native `codex-harness ownership-check --source` enforces the ownership
inventory in [executable-ownership.json](executable-ownership.json). It fails
unclassified foreign executables across tracked and untracked source,
embedded or generated foreign-language programs in maintained Rust source,
first-party paths relabeled as third-party or inert data without a genuine
Rust consumer, and stale inventory entries. Declared inert analysis samples
keep consumer evidence, and the external `.venv` root stays distinguishable
from first-party source. The current tree has 171 foreign executable files,
all classified: 169 first-party-legacy open findings owned by tasks 8.2, 9.2
or 9.5, plus the two declared inert samples. The check exits nonzero until
those paths are removed, making it the live remaining-work gate for 8.2.
Removal of the live script lifecycle and Python MCP chain waits for the 9.x
cutover that re-registers the global launcher and MCP servers natively.

## Legacy executable retirement, first batch (migration 8.2 progress, 2026-09-16)

107 first-party non-Rust files were deleted after consumer checks: the
transitional test suites for retired OpenCodex/CBM/Graphify/LSP behavior
(several already referenced the deleted `global/opencodex` layout), their
dead fixtures, `tools/code-tools/{cbm_proxy,graphify_proxy,graphify_update}.py`,
`tools/opencodex-config-check.mjs`, `tools/opencodex-native-restore.mjs` and
`tools/_write_check.py`. Documentation now points model-free checks and the
usage counter at the native Cargo suites and `codex-harness delegation-usage`.
The foreign-executable count fell from 171 to 66. Kept paths are explicitly
owned: the live script lifecycle and Python MCP chain (9.2), the
`opencodex-process.ps1` baseline oracle plus outcome/LSP-snapshot measurement
drivers (9.4), the delegation model probes (8.2 after
orchestrate-subscription-agents), the global token-workflow driver with
`ConPty.cs` and `consumer-rpc.ps1` (9.3), the `nuphus-window.ps1` executable
double pending a Rust port (8.2), skill helper scripts (8.2) and temporary
verification drivers (9.5).

## Native window fixture and skill-helper retirement (migration 8.2 progress,
2026-09-16)

`nuphus-window.ps1` is replaced by the Rust `harness-window-fixture` binary
(same owned off-screen window, state/stop contract and title); an unignored
model-free test verifies state publication, live-window identity and clean
stop, and the opt-in Nuphus suite now launches the Rust fixture. The four
transitional skill helper scripts (`run.py`, `stdin_bridge.py`,
`process_case.py`, `observe.ps1`) were deleted; the linked skill contracts
describe native `harness-inspect.exe`/`harness-observe.exe` only. Foreign
executable files are down to 61, all classified: the remaining 8.2-owned
paths are the two delegation model probes, which wait for
orchestrate-subscription-agents to deliver their native successor. The kept
legacy outcome oracle driver no longer imports the deleted skill helper: its
bounded process runs go through native `harness-observe.exe`, verified live
against the built binary.

## Native MCP registration projection (migration 5.3/5.5 progress, 2026-09-17)

The code-tools lifecycle now connects the selected tools natively end to end.
`dependency_discovery` is invoked with the documented environment inputs when
the selected dependency owner is the current user, so adopted packages are
actually found; the recorded `harness/code-tools-registration.json` selection
stays authoritative, so an Update cannot rewrite an installed native
connection back to a transitional seam. `codegraph_integration::prepare` gained
the adoption inventory (the CLI projection still falls back to the on-disk
registry) and emits the native Serena broker connection plus a native Nuphus
connection pinned to the adopted audited original executable
(`paths.original_native_executable`) and its SHA-256; a locally rewritten
variant keeps the transitional seam instead of pinning an unaudited binary.

Component ownership was aligned with that projection: the core component links
the four core commands and no longer creates `harness/bin/harness-rtk.exe`
(`core_install::core_linked_binaries`, mirrored by core Check). The
token-workflow component owns both RTK links and the Codex hook definitions
file, adopts an identical existing hook link during a script-layout upgrade and
records it in its state; its feature edits now read the schema-2
`settings.codexCommand` as well as the legacy top-level field.

## Staged complete native installation (migration 9.1 progress, 2026-09-17)

Owned staged acceptance (private roots under `%LOCALAPPDATA%`, model-free)
verified with immutable candidate builds of the current source:

- `codex-harness build --source . --state <owned>` produced
  `state/builds/db39b42f9f351de0-1789598016887428400-7352` (source identity
  `db39b42f9f351de09686783b0f78f4ea1ea100b5847aced36451268655dc63b3`); the
  earlier `f18595…`/`a2d6ba…` candidates were rejected as stale after source
  edits, which is the documented behavior.
- Fresh core connect into owned homes (Process PATH scope) reported 20 links
  and a passed model-free runtime handoff to the resolved upstream
  `codex.exe`; `check --core-only` and every later component Check reported
  `connected`.
- `--code-tools-only` Install registered `codegraph`, `serena` and `nuphus`
  through the native manager (Serena with the adopted uv interpreter and the
  guarded entry, Nuphus with the audited original executable and digest
  `9a07112f…`) and retired `codebase-memory`/`graphify`.
- `--token-workflow-only` Install acquired the pinned RTK 0.48.0 archive,
  built the adapter, linked `harness/bin/{rtk.exe,harness-rtk.exe}` plus
  `hooks.json` → `global/rtk-hooks.json`, and enabled `code_mode`/`hooks` in
  the staged `config.toml`; Disconnect removed exactly those connections and
  re-enabled the previous feature state while core, code-tools and
  subscriptions stayed `connected`, and a repeat Install restored them.
- `--subscriptions-only` Install wrote the owned native profile/routing
  records without starting or querying a proxy.
- Real stdio sessions against the staged registrations returned `initialize`
  (`harness-nuphus` 0.1.0; `Serena` 1.28.1 with 14 filtered tools) and
  `tools/list` (38 Nuphus tools) with clean exits. The staged CodeGraph
  connection was refused by the running account broker with "broker has older
  source/runtime", i.e. the expected state until global activation retires the
  older broker; CodeGraph serving itself is covered by the replacement
  acceptance.
- Upgrade from the script layout: `install.ps1 -Mode Install -CoreOnly` into
  owned homes, then the native core Install previewed 8 changed links and, on
  mutation, retired the owned script links (`codex.ps1`,
  `codex-harness-check.ps1`, `hook.ps1`), installed the four native commands
  and the diagnostic alias, migrated the metadata to schema 2 and passed the
  runtime handoff; code-tools, token-workflow and subscriptions then connected
  over that upgraded layout.

Two native reader defects surfaced and were fixed with regression coverage:
the script lifecycle records `launcherSource` (unknown field) and a verbatim
`\\?\` configuration-bridge path, and the published launcher copy legitimately
lives under `CODEX_HOME/harness/launchers` instead of the checkout. The live
global installation metadata now imports (`inspect-installation` reports 18
links, 17 owned), which unblocks the global cutover task 9.2.

Relocation, interruption and rollback remain covered by their default
acceptance suites (`core_install` retarget/interruption tests,
`installation_state` legacy adoption after relocation, `legacy_pending`,
`registration_finish`, `native_launcher`/`native_build` integrity and
stale-manager repair).

## Test hygiene and task 8.4 progress (2026-09-17)

Default checks now own their mutable targets: the agent-configuration oracle
was updated to the retired fixed-preset reality (a synthetic kit source covers
the collision path through the CLI), the CodeGraph transport fixtures run in
an owned project directory instead of the crate root, and they wait a bounded
180 seconds for the machine-wide account slot instead of failing when a live
session holds it. Subscription fixture tasks are retired by a drop guard, and
the nine orphaned `codex-harness-subscriptions-*` tasks left by earlier runs
were removed; the live installation never had one. `global/kit.psd1` is now
classified by the executable ownership check (62 executable files, 60 open
first-party-legacy findings) instead of escaping the `.psd1` extension, and
the checked-in `crates/codex-harness/owned-worker-started` fixture leftover was
removed.

## Global native activation (migration 9.2 progress, 2026-09-17)

The live installation was upgraded to the native lifecycle with the immutable
candidate `state/builds/b4d132c1892f6fd9-1789603446055960800-19772` (source
identity `b4d132c1892f6fd9e0777b04eadcf11fd3779a420a47c796b6fcdea411ae29db`).
The pre-activation metadata, `config.toml`, code-tools registration and
token-workflow state were retained privately under the owned staging root for
rollback.

- Core Install previewed 8 changed links without writing, then connected: 22
  links, `path_change: false` (the User PATH already contained
  `harness/bin`), and a passed model-free runtime handoff to the resolved
  upstream vendor `codex.exe`. The metadata migrated to schema 2; the owned
  script launcher and diagnostic alias links were retired.
- Code-tools Install rewrote `config.toml` to the native connections
  (`mcp codegraph`, `mcp serena` with the adopted uv interpreter and guarded
  entry, `mcp nuphus` with the audited original executable and digest) and
  retired `codebase-memory`/`graphify`; the recorded selection matches.
- Token-workflow Install adopted the script state, rewrote both RTK links to
  the native adapter build and kept `hooks.json` → `global/rtk-hooks.json`;
  subscription Install wrote the native profile/routing records without
  starting or querying a proxy. All four component Checks report connected.
- Fresh outside-checkout terminal: `codex --version` returns
  `codex-cli 0.154.0` through the native launcher (`codex.exe`), the
  `codex-harness-check` alias resolves to the native diagnostic, a model-free
  `diagnose` reports `healthy`, and the source-linked `AGENTS.md`, skill and
  agent paths still resolve into the checkout.
- The older CodeGraph account broker was retired explicitly
  (`mcp retire-codegraph`), after which real stdio sessions through the live
  registrations returned initialize and tools/list: `codegraph` 0.1.0 with
  `codegraph_search`/`codegraph_detail`, `harness-nuphus` 0.1.0 with 38 tools
  and `Serena` 1.28.1 with 14 filtered tools, each exiting cleanly.

The transitional script hook launcher (`harness/bin/hook.ps1`) is no longer
inherited by a native connection: the native chain invokes the RTK adapter
directly, so the owned registration retires with the script lifecycle instead
of surviving the cutover. One acceptance flakiness was also removed: the
interrupted-connect recovery test now retries the short window in which a
killed child is still releasing its installation mutex and file handles
(verified stable over six consecutive runs).

## Native Serena boundary without the Python seam (migration 3.2/5.4/9.2 progress, 2026-09-17)

The guarded Python entry point (`tools/code-tools/serena_entry.py`) is retired
from the served path. The adopted package now runs from its own console entry
point with a generated harness-owned home:
`crates/harness-core/src/serena_configuration.rs` materializes
`<CODEX_HOME>/harness/serena-home` (and one home per served project under
`workers/<project-key>`) with `ls_specific_settings` that pins every adopted
backend to an explicit `ls_base_cmd`/`ls_args` from the registry
(`serena_id` is the single mapping owner; the retired seam's aliases such as
`python` for `python_basedpyright` are preserved). Serena therefore never
consults a release API or runs a package manager for an adopted backend, its
own configuration writes stay inside the owned home instead of the user's
shared `~/.serena/serena_config.yml`, and per-project homes keep concurrent
workers from crossing configuration state.

Provisioning suppression and validation stay native: `serena::validated`
checks the adopted identity/version/status before a child starts, the launch
points at the registry's console entry point, and
`serena_configuration::ensure_supported_languages` refuses a project whose
detected *project declaration* (for example `Cargo.toml`, `pyproject.toml`,
`App.csproj`, `tsconfig.json`) has no verified adopted backend. Stray
foreign-language source samples in an adopted project do not refuse the
session. The code-tools lifecycle no longer falls back to a Python seam: a
mutating mode requires a verified native connection for `serena`, `nuphus` and
`codegraph` or fails with an explicit provisioning error.

Verified on 2026-09-17 against the adopted package: the real two-project
isolation case (`rust_session_isolates_two_owned_projects_and_preserves_shared_config`)
passed with rust-analyzer semantic edits, the shared-pool case
(`shared_pool_reuses_one_worker_and_isolates_projects`) reused one worker per
project and kept it across a client disconnect, the end-to-end proxy case
(`mcp_serena_proxy_shares_one_worker_and_filters_the_catalogue`) passed through
the actual broker service, the config case asserted the user's shared Serena
configuration is untouched, and the live global registration served
`initialize` (Serena 1.28.1) plus a real `get_symbols_overview` on this
checkout after the older broker was retired. `cargo fmt --check`,
`clippy -D warnings` and the default suite are clean.

A live probe of the Python-backed path exposed a degraded-worker case: when a
language-server process dies during project initialization, Serena keeps the
session alive but answers every later semantic call with its fatal
"language server manager is not initialized" error. The shared pool now
retires a worker that answers with that exact marker so the next request gets a
fresh worker (the client still receives Serena's own failure once); an ordinary
tool error does not retire a worker. After that retirement a fresh live call
returned the real symbol overview for `tools/code-tools/serena_broker.py`,
confirming the adopted python backend (node + basedpyright `ls_base_cmd`) is
pinned correctly.

Operating condition: replacing a running Serena broker is two-phase. The first
`mcp broker-retire` marks it retiring and its drain took about 84 seconds on
this host; a client that connects during the drain is refused with "broker
readiness/source differs; preserving owner" (by design, the owner is
preserved). After it exits, the next client starts a broker from the current
build. The live global installation was updated to candidate
`state/builds/3a32b5f17d812d22-1789618209389928100-11232` (source identity
`3a32b5f17d812d22ad2d90a868baa82b988f160fcfa28a3ff7dbb34da34b453e`); all four
component Checks report connected and a live `get_symbols_overview` on
`crates/harness-core/src/lifecycle.rs` returned the real symbol overview.

## Matched candidate comparison (migration 9.4, 2026-09-17)

`crates/codex-harness/tests/migration_baseline.rs` gained the candidate
comparison required by task 9.4:
`compare_candidate_paths_against_the_recorded_baseline` is opt-in on
`HARNESS_MIGRATION_BASELINE` and reuses the recorded method, sample count and
noise boundary unchanged (five samples, first discarded, median of the warm
set, MAD/range dispersion, material regression only when the warm median grows
by more than 100 ms **and** more than 10% of the baseline median). Replaced
script paths are compared against their native replacements through an
explicit mapping; every other scenario is re-measured on the same native path,
so any delta there is environment noise.

Run on 2026-09-17 against the recorded baseline (head `53b8552`, test profile,
private receipt `%TEMP%\harness-migration-comparison-rH4ah8\comparison.json`):

| Scenario (baseline → candidate) | Baseline warm median | Candidate warm median | Delta |
| --- | --- | --- | --- |
| `check.script_missing_owned_homes` → `check.core_missing_installation` | 677.0 ms | 18.0 ms | −659.0 ms |
| `launch.script_fallback_missing_module_unicode_nonzero` → `launch.native_degraded_fallback_unicode_nonzero` | 498.9 ms | 596.0 ms | +97.1 ms |
| `launch.native_argv_unicode_stdin_nonzero` (unchanged native) | 218.2 ms | 227.7 ms | +9.4 ms |
| `check.diagnose_owned_report_and_mixed_selector_refusal` (unchanged native) | 540.8 ms | 520.7 ms | −20.1 ms |
| `check.core_missing_installation` (unchanged native) | 20.1 ms | 18.0 ms | −2.1 ms |
| `check.build_missing_metadata` (unchanged native) | 18.9 ms | 15.7 ms | −3.2 ms |
| `mcp.stdio_unicode_ids_output_purity_and_incomplete_failure` (unchanged native) | 187.8 ms | 174.1 ms | −13.7 ms |
| `process.timeout_streams_and_foreign_preservation` (unchanged native) | 213.4 ms | 209.3 ms | −4.1 ms |
| `console.unicode_stdin_nonzero_and_cancellation` (unchanged native) | 105.8 ms | 97.0 ms | −8.8 ms |
| `subscription.bounded_node_fixture_nonzero_and_timeout` (unchanged native) | 3740.6 ms | 3411.8 ms | −328.8 ms |
| `rtk.exec_once_hook_rewrite_and_malformed_silence` (unchanged native) | 203.1 ms | 186.7 ms | −16.3 ms |

All candidate oracles passed and there were **no material regressions**:
every unchanged native scenario stayed inside the pre-established noise
boundary (largest increase 9.4 ms; dispersions 0.4–73 ms MAD). The two replaced
paths are reported as information: the native Check replaces the script Check
at roughly 1/37th of the wall time, and the native degraded launcher adds
97 ms over the retired script fallback — below the material threshold — for
reading the build record and hashing the registered upstream, i.e. the new
integrity work rather than a port regression.

Limits (unchanged from the baseline): owned model-free targets only; live
global MCP/service and provider/network timings are excluded from this
comparison set and must not be reported as rewrite acceleration. Fixture
preparation is included in every sample of every scenario, and the degraded
launcher fixture prepares its registered build once outside the timed closure
so the measurement covers launcher startup rather than fixture copying.

## Real global consumers (migration 9.3, 2026-09-17)

Exercised against the actual connected build
`state/builds/3a32b5f17d812d22-1789618209389928100-11232` (all four component
Checks `connected`):

- **MCP**: real stdio sessions through the installed registrations returned
  `initialize` and `tools/list` for `codegraph` (0.1.0;
  `codegraph_search`/`codegraph_detail`), `harness-nuphus` (0.1.0; 38 tools)
  and `Serena` (1.28.1; 14 filtered tools), and live `get_symbols_overview`
  calls returned real symbol data for both a Rust and a Python file in this
  checkout. The older account brokers were retired explicitly; replacement
  drains before new clients attach (documented above).
- **Launcher and diagnostics**: from a fresh terminal outside the checkout,
  `codex` resolves to the native `harness/bin/codex.exe` and reports
  `codex-cli 0.154.0` through the registered upstream; `codex-harness-check`
  resolves to the native alias and a model-free `diagnose` reports `healthy`
  with no model calls.
- **RTK**: the installed `harness/bin/harness-rtk.exe hook` rewrote a live
  `PreToolUse` payload from `harness-rtk.exe exec git status --short` to
  `harness-rtk.exe compact git status --short` and exited zero.
- **Skill helpers**: the linked `harness-observe.exe` ran from an owned
  outside-checkout directory with its documented contract
  (`--cwd/--timeout/--output-limit -- ABSOLUTE_EXE ARGUMENTS`), bounded the
  installed launcher in a Job (peak memory recorded, zero active processes
  after exit) and retained stdout/stderr receipts. The skill references no
  longer claim the helpers are unregistered; a connected kit links both
  helpers into `harness/bin` on the user PATH.
- **Subscription routing**: the native profile/catalog/routing state is
  installed and `check --subscriptions-only` reports `connected` without
  querying or stopping the live proxy; the live shim serving current sessions
  is deliberately left running (its port answers), because destructive
  recovery probes must use separate targets. No live model request was issued
  for this acceptance: the replaced paths do not change model routing, the
  accepted provider behaviour was verified earlier with explicitly selected
  probes recorded in this file, and spending the user's subscription quota is
  not required to prove the migrated boundary.
- **Delegation**: the native `delegation-usage` consumer is covered by the
  default suite; the model-backed delegation probes remain the open successor
  work owned by `orchestrate-subscription-agents` (tracked by task 8.2).

## Transitional lifecycle retirement (migration 8.2/9.5, 2026-09-17)

The retired script and Python lifecycles were removed from the repository after
consumer checks, with machine-local rollback copies kept under the owned staging
root (`%LOCALAPPDATA%\codex-harness-native\migration-91\legacy-rollback\<stamp>`,
including the pre-retirement `installation.json`). Deleted: `install.ps1`, the
`tools/*.psm1` modules, `tools/codex.ps1`, `tools/codex-harness-check.ps1`,
`tools/hook.ps1`, `tools/mcp.ps1`, `global/kit.psd1`, the Python MCP chain
(`tools/code-tools/*.py` including `launch.py` and `serena_entry.py`,
`tools/process_ownership.py`), the retired `tools/lsp` stack, the legacy
`opencodex-process.ps1/.cs` bounded-process oracle, the Python outcome and
delegation drivers, the temporary `tmp-*.ps1` verification drivers and the
legacy `tests/ConPty.cs`, `tests/consumer-rpc.ps1`,
`tests/token-workflow-native.ps1`, `tests/agent-delegation.py` and
`tests/subscription-consumer.Tests.ps1` drivers. Generated bytecode caches and
the obsolete `pyrightconfig.json` were removed with them.

Consumer checks before removal: the live installation's launcher, diagnostic
alias, hook definitions, RTK binaries and all three MCP registrations resolve to
native commands and paths; nothing in the delivered installation referenced the
removed files; Check reported `connected` for all four components before and
after the removal. Native successors retained: `native_launcher` fail-open cases
replace the script launcher regression suite, `harness-inspect`/`harness-observe`
and the native console fixtures replace `ConPty.cs` and the script drivers, the
native `outcome-*` and `delegation-usage` CLIs replace the Python drivers, and
the native bounded-process case replaces the retired script oracle in the
measurement harness (`process.native_bounded_node_fixture_nonzero_and_timeout`).
Deleted test-only drivers have named replacements: `context_delivery.rs`'s
opt-in probes by `harness-core/tests/launcher.rs::task_effort_does_not_interpret_prompts_or_override_native_settings`,
`launcher.rs::per_model_effort_defaults_without_explicit_selection` and
`harness-core/tests/feature_edit.rs::actual_native_feature_edit_preserves_unrelated_configuration`,
and `script_launcher_fallback.rs`'s global TUI smoke by
`crates/codex-harness/tests/tui.rs::actual_tui_status_model_and_trust_write_locally`
plus the `native_launcher` fail-open cases.

Operating condition for acceptance: the `native_build` suite runs candidate
builds inside its 2 GiB build job. With two heavy suites running concurrently on
one machine, three of its seven tests failed with candidate-build exits inside
the job while the active installation was correctly preserved; the same suite
passed 7/7 with the machine otherwise idle (`native_build` rerun 2026-09-17,
1053 s). Run workspace acceptance, and `native_build` in particular, without
another heavy compilation active.

Inventories and documentation were reconciled: `executable-ownership.json` now
declares only the external `.venv` root and the two inert analysis samples,
`legacy-mcp-retirement.json` records the retirement, `installation.md` was
rewritten for the native lifecycle, and `README.md`, `docs/code-tools.md`,
`docs/source-diagnostics.md`, `docs/subscription-models.md`,
`docs/token-workflow.md`, `docs/project-decisions.md` and the two skill
references now quote the native commands. The ownership check reports
**2 executable files (both declared inert analysis samples), 0 findings** and
exits zero, and its default suite asserts that state as the delivered
selection; materialized builds are unaffected because the workspace member
`tools/rtk-adapter` is Rust and was preserved.







## Independent review and recovery fix (migration 8.4/9.6, 2026-09-17)

An independent read-only review covered the post-retirement native tree for
process containment, authentication boundaries, ownership and rollback. It
found one material defect plus two evidence/tooling gaps; all three are
resolved.

R1 (fixed, regression-covered): `token_workflow_lifecycle::recover` derived the
owning source root only from the journal's `previousState`, which is null on a
first install, so a crash between creating the hook link and writing the state
file left that first-install operation unresolvable ("destination is outside its
owned connections") with no path back except deleting the pending file by hand.
`recover` now falls back to the journal's `plannedState.sourceRoot` while the
ownership guard keeps requiring the destination to be `<home>/hooks.json` and
its target to be that root's `global/rtk-hooks.json`. Two tests cover the
first-install rollback
(`recover_rolls_back_first_install_hooks_link_from_planned_root`) and the
preserved refusal for a foreign destination
(`recover_refuses_first_install_hooks_operation_outside_planned_root`).

F1 (fixed): the requirement map referenced checks that no longer exist after
retirement — the deleted `script_launcher_fallback.rs` and
`subscription_service.rs` files, and the renamed
`configure_restart_reports_retirement`. Its launch/TUI and subscription rows now
name existing native checks (`native_launcher` fail-open,
`subscription_lifecycle` install/check/disconnect/restart tests,
`subscription_login` token handling and the bounded `process_service`
containment cases), every remaining name-level reference was resolved against
the current tree, and the five stale "cutover open / alias still script" status
strings now reflect the verified 9.2 activation.

F2 (recorded): the deleted test-only drivers have named successors in the
retirement evidence above — `context_delivery.rs` by the launcher task-effort
and native feature-edit tests, `script_launcher_fallback.rs` by the native TUI
status/trust test plus the `native_launcher` fail-open cases.

F3 (recorded as the operating condition above): the only observed check
failures were candidate-build OOMs inside the 2 GiB build job when two heavy
suites ran concurrently; the affected suite passes 7/7 alone, and the active
installation was preserved throughout.

Resolved finding (2026-09-20): `native_launcher`'s
`upstream_background_lifetime_survives_ordinary_wrapper_exit` fails
was a deterministic regression, not an environment fault: executor-spawn
hardening had reused the reaping `wait_foreground` for the ordinary
interactive launch, so `TerminateJobObject` killed the upstream-spawned
background child right after the session root exited. The interactive path now
uses `Job::wait_session_root`, which waits for the root and then disarms
kill-on-close so upstream-managed background processes keep their own lifetime
on an ordinary exit; an abnormal launcher death still reaps the session tree
while the armed job handle closes. The reaping contract itself is unchanged
and still covered by `foreground_wait_reaps_grandchild_without_deadline`;
`session_root_wait_preserves_managed_grandchild` covers the preserved one.
Verified: `native_launcher` 10/10 including the former failure, `process`
17/17, `migration_baseline` and `tui` with the documented
`HARNESS_ACCEPTANCE_POWERSHELL` override on this machine, bin unit tests
67/67, `executor_succession`, plus `fmt`/`clippy -D warnings`.

Open finding (2026-09-20): the global installation's lifecycle verbs
(`update`, `disconnect`) are blocked by an out-of-band edit that recreated the
`harness/bin/codex-harness.exe` link to a working-tree build; the recorded
object identity can never match again, and the kit correctly preserves state
instead of guessing. The same damage made consumer sessions run a launcher
older than their live-linked skill text. A temporary manual repoint restored
the launcher; the one-action `deploy --reset` later performed the full clean
reset with a receipt and a fresh valid installation, and `update --preview`
validates again.

Resolved finding (2026-09-20): the loop-guidance skills (`team-lead`,
`board-workflow`) had reached Codex sessions as copies under the Codex skill
root, going stale when source skills changed. The duplicated copies were
removed by an explicit out-of-band cleanup that preserved the retired
directories under a dated folder in the machine-local Codex home; kit skills
are consumed from the linked source roots again. This matches the
linked-global-kit rule that managed skills are connected by links and never
copied by the installer, so no refresh step is needed or allowed. Verified on
this machine: the skill links resolve to the checkout with matching content
hashes and `codex-harness skills usage` reports both skills enabled from the
linked root.

Resolved finding (2026-09-20): `install --token-workflow-only` failed with a
raw `os error 2` because the component's source identity still hashed the
retired `tools/token-workflow.psm1` after the native migration deleted it; the
unit fixture fabricated that file, so suites passed while a real checkout
failed before artifact reuse or linking. The identity list is now one shared
`SOURCE_FILES` constant used by both production and the fixture, and a missing
component input is reported by path instead of a raw OS error. Verified by the
focused lifecycle tests, `rtk_adapter`, a fresh isolated install/check, the
installed manager's standalone install/check on this machine, and a full
`deploy --all` chain with all four components ok. The first post-fix
`deploy --all` still reported `partial` because component steps then ran in
the older starting process; `deploy --all` component steps now execute through
the manager the same delivery just connected, with child stderr forwarded and
nonzero exits reported per component. Verified by a controlled reversible
regression: a source whose delivered build reintroduced the stale identity
entry failed only its token-workflow step with that build's contextual error
while the older starting manager's fixed code would have passed in-process,
and the reverted source then completed the full chain (`deployed`, all four
components ok, executor probe ok).

Checks on the reviewed tree: `cargo fmt --all -- --check` clean; workspace
clippy (`--all-targets --locked -D warnings`) clean; the full documented suite
(`cargo test --workspace --locked --jobs 1 --no-fail-fast`) passed 83 targets
with 0 failures, including `native_build` 7/7 in 1120 s; `harness-source-check`
reports 0 findings over 494 working-tree files including local links and
anchors; `ownership-check --source` reports 2 declared inert samples and 0
findings; `openspec validate migrate-harness-to-rust --strict` is valid. The
116 opt-in tests (real Serena registry, model-backed and live-provider suites)
remain deliberately unexecuted in this default pass and keep their separate
evidence above.

Spec synchronization (9.6): the change's three deltas were merged into
`openspec/specs/` — `harness-source-diagnostics` (native Check entry point,
upgrade of the owned script diagnostic) and `linked-global-kit`
(compiled-artifact distinction, native entry point and script-installation
upgrade, native build/degraded-launch/previous-build/stale-manager behavior) as
MODIFIED requirements, and the new `rust-native-harness` capability as 10
requirements / 28 scenarios. No existing requirement or scenario was removed
(linked-global-kit 42 to 47 scenarios, diagnostics 7 to 8), and
`openspec validate --specs --strict` passes for all 21 specs.
