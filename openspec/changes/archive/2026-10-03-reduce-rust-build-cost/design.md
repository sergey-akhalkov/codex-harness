## Context

See [proposal.md](proposal.md). The stable workspace uses edition 2024 and Rust 1.98. Ordinary profiles currently inherit full debug information; the release profile already has a recorded comparison supporting its fat-LTO configuration. Native publication selects all binaries of three packages but publishes seven declared programs. The workspace has many separate integration executables. Existing heavy-command admission and immutable publication identity are required operating boundaries.

The ordinary scenario is repeated local edits, focused tests, a complete verification boundary and occasional installation. Related worktrees coexist, so a fresh target for every command wastes reuse, while one shared target for concurrent roots creates contention. The counterexample to keeping caches is a retired worktree that will not be built again; the counterexample to aggressive cleanup is tomorrow's edit in an active checkout.

## Goals / Non-Goals

Reduce work generated and retained before introducing additional tools. Keep the default fast path simple, make full debugging explicit, and transfer the procedure through `cargo-fast`. This change does not promise a universal minimum, change CPU/memory policy, weaken tests, switch release optimization, install a new compiler/cache/runner, or add a background cleanup service. CI caching, sccache and nextest remain documented conditional alternatives unless local native improvements reveal a measured remaining need.

## Decisions

### Profiles and comparison

Set `profile.dev.debug = "line-tables-only"` and dependency debug information to false. Tests inherit dev. Keep incremental development and all existing assertion/overflow behavior. Add `profile.debugging` inheriting dev with full workspace and dependency debug information. The owner explicitly selected this debugging trade-off.

Use the existing heavy wrapper and stable toolchain. Capture a matched baseline/candidate on the same representative real package/target and source bytes: fresh compilation with timings, unchanged warm invocation, and a restored small source edit. Keep raw evidence outside published source. Record wall time, compiler time when available, target logical bytes and PDB/incremental breakdown. Use one cold pair and three warm observations per arm; report this as a scoped measurement, not a statistical population claim. A smaller tree with passing behavior checks supports the storage improvement; an apparent time regression above 10% requires one cause-driven confirmation before adoption. Missing measurements stay unknown.

### Publication selection

Derive explicit Cargo binary selection from the existing declared delivery set, preserving the package selection and finalization owner. Do not maintain a second independent list of names. A fixture-only binary that fails if compiled is a direct counterexample proving selection through the real publication entry point. Existing changed-byte/restored-mtime, tampered artifact, compiler override and recovery checks retain their role. A persistent publication compiler target is not introduced: verified immutable artifact reuse already handles unchanged identities, and changed identities retain fresh compiler inputs.

The selected delivery set belongs to the requested source revision. Preserve the existing transition from an installed producer to a consumer with additional delivery binaries and changed input rules, including a producer using the optimized selection. Rewriting only the test producer to use the former `--bins` behavior does not establish that property for the new implementation. Prefer the existing freshly compiled source-side handoff to a second maintained list or heuristic Rust-source parsing.

### Integration targets and concurrency

Evaluate a bounded `harness-core` integration-target consolidation first, with a recorded mapping from each former executable/test to the candidate module/test. Group only when shared resources and process assumptions remain valid. Preserve ignored cases and platform gates, update operational selectors, and compare compile/link cost and execution against the same selected cases. Adopt only if cost improves without an unexplained regression; otherwise retain the current layout and its measured reason. Do not broaden to every integration target without evidence that the pilot repays it.

Compare Cargo jobs 1 and 2 within unchanged admission, CPU and memory limits. Keep test execution serialized wherever ownership requires it. A package's monolithic compiler work may make additional jobs ineffective; retain the conservative default in that case. No silent global environment override is allowed because native publication deliberately rejects ambient compiler/profile overrides.

### Cache ownership

Inventory known target roots, distinguish file length from allocated space, and identify the active development target. Reclaim only exact inactive regenerable roots after resolving paths, checking process use and retained evidence. Preserve unfinished executor trees and active processes. Use Cargo's native scoped/dry-run cleanup where applicable; avoid deleting individual fingerprint or incremental internals. Establish cleanup at retirement or demonstrated disk pressure, not before every build. Document that Cargo global-cache GC does not collect target outputs.

### Portable skill and delivery

Update the owned `cargo-fast` identity instead of adding a duplicate skill. Keep the entry point concise and place detailed diagnosis/comparison commands and platform conditions in its reference. Draft outside discovery, then perform the existing skill-evaluation acceptance and Registration publication. The capability claim is better diagnosis and decision coverage, not subscription savings. Use one fixed baseline/candidate batch with the same configured model/effort: intended large-target task, a non-Rust negative case, a constrained-publication boundary, and an independently held transfer case. Each arm receives raw inputs and its assigned skill revision without author conclusions; the lead owns acceptance. No unchanged failing episode is repeated.

Reusable source stays in this repository; raw measurements, absolute machine paths and evaluation state stay in local owning storage. After checks pass, use `codex-harness deploy`, verify the new immutable runtime and read/invoke the accepted skill outside this checkout. Installation rollback remains with the existing lifecycle.

## Risks / Trade-offs

- Reduced debug information requires a separate rebuild for variable inspection; the debugging profile is the supported route.
- Consolidating tests can expose shared-state interactions; compare test inventories and exercise the real cases before adoption.
- More Cargo jobs can exceed useful memory parallelism; keep the existing admission envelope and measure before changing defaults.
- Cleanup can discard valuable warm state or interrupt another task; explicit path and runtime ownership checks precede reclamation.
- New profile artifacts temporarily coexist with old ones; account for experiment space and clean only owned retired outputs after evidence is retained.

## Migration Plan

Capture the baseline, implement independent native-selection and profile changes, integrate accepted target/concurrency experiments, complete applicable native checks and skill evaluation, then deploy. Update the existing native guide and project decisions with the accepted operating path and concise evidence. Revert the specific manifest/selector change to roll back development behavior; use the installation lifecycle to restore a prior immutable build. No unrelated worktree reset or global config replacement is part of recovery.
