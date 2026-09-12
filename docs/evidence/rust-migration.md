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
| Dependency discovery, audit, stage, select, apply dispatcher | Bounded native CLI acceptance passed for discover/plan/audit/stage/select. Native apply/update Check and preview are read-only and preserve OpenCode caches; mutation stages/selects native CodeGraph and, with an explicit Node path and digest, BasedPyright; holds rust-analyzer to the installed compiler cohort; and leaves remaining backends pending for Python apply_selected. Complete provisioning and global connection remain open |
| Historical CBM runtime / stdio | Native `cbm-index`, `cbm-catalogue`, `cbm-tool` and `mcp codebase-memory` remain the rollback route for the retired registration. Do not continue a CBM port. Retire CBM-only paths only with further consumer-backed evidence |
| CodeGraph adapter | Native `codegraph_*.rs` plus `dependency_codegraph.rs` own lifecycle, response, storage and registration. Replacement activation and installed consumer acceptance passed. Rust task 5.4 adopts the same owner and stays open. Published CodeGraph 1.6.0 / bundled Node remain third-party |
| Shared services | Native WMI creation, independent Job, authenticated reuse, cancellation with confirmed cleanup, retirement and idle cleanup passed with owned fixtures. One fresh broker root per service lifetime. Serena integration and installed MCP connections remain unfinished |
| Large-repository index | The historical CBM full index failed the retained memory policy on the current larger checkout. Current CodeGraph comparison, concurrent native MCP, resource and installed-consumer measurements live in the replacement design; this file does not keep a second copy. Rust task 5.4 remains open |
| Serena native boundary | Rust-controlled stdio session around the existing guarded Python entry is proven for two owned Rust projects. Provisioning suppression, selected provider configuration, owned `SERENA_HOME` and missing/incompatible registry failures are checked before a child starts. The Python entry remains until later lifecycle cutover. Task 5.4 still owns installed MCP integration |
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





