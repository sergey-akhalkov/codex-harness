# Rust migration acceptance

This change is unfinished. Source, behavioral parity and global cutover are
separate gates. Passing the current Cargo subset closes none of the unimplemented
lifecycle or foreign-integration requirements. Command entry points, provenance
and operating limits live in [native Rust commands](../rust-native.md). Per-unit
ownership is [rust-migration-map.json](rust-migration-map.json). Generated caches
are not migration units.

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
| Outcome/usage/oracles | Local CLI consumers exist for report, usage, run, cases, oracle, discover and arm. External consumer cases, full suite migration and global lifecycle remain open |
| Python staging | Empty offline UV candidate only; not eligible for activation |
| OpenCodex | Native validation boundary exists; browser-only OAuth, restoration and remaining task 3.1 stay open |
| Structured inspect | Bounded native helper and actual outside-checkout consumer exist; linked invocation and global lifecycle remain open |
| Regression helper | Scoped native observer exists; global skill invocation remains open |
| RTK | Transitional workspace member `harness-rtk`; native RTK lifecycle still needed |

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
`fake_cbm_worker` and transitional `tools/code-tools/cbm_proxy.py`. They are
not queued for another CBM port. Broader agent-workflow comparisons keep their
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






