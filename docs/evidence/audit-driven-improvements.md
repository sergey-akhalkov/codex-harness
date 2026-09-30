# Audit-driven harness improvements acceptance

Implementation evidence for the `verify-audit-driven-harness-improvements`
OpenSpec change. This record is unfinished until that change is archived; each
entry names the actual command, revision, scope and limits. Private raw logs,
model-backed inputs and machine-local state stay outside tracked source.

## Implementation base

- Base revision: `4b2f958b88d6667b9eff71c25254cf42834ecdb7` ("Retarget archived
  RTK design link to the harness-rtk crate", 2026-09-29 16:36:52 +0300).
- Dirty inputs at implementation start: untracked planning artifacts in
  `openspec/changes/verify-audit-driven-harness-improvements/`; no tracked-file
  modifications. The planning design rechecked its source observations at this
  same revision, so no audit row was fixed by an intervening change and no item
  needs preservation as "already fixed".
- Toolchain observed 2026-09-29: rustc/cargo 1.98.1, clippy 0.1.98, Windows
  x64. Serena project `codex-harness` activated with `rust` and `python`
  language servers; `get_symbols_overview` on `crates/harness-rtk/src/main.rs`
  succeeded. The planning-time language-server failure did not reproduce in
  this invocation; its unresolved cause remains group 11 work, and this single
  success is not delivery evidence for semantic readiness.
- Small native check: `cargo fmt --all -- --check` passed at the base revision.

## Audit predicate spot-verification

The working tree equals the audited revision, so the design's recheck holds by
identity. Representative predicates were re-read in source on 2026-09-29 at
that revision:

| Row | Current source evidence |
| --- | --- |
| A01 | `pack_store` (`crates/harness-rtk/src/main.rs:610`) writes the pack file before reading the shared index; `retain` (`:586`) deletes `.log` files absent from the in-memory index; `write_pack_index` (`:542`) uses a common `index.json.tmp`. The two-writer loss interleaving is present. |
| N01 | `read_pack_index` (`crates/harness-rtk/src/main.rs:524`) does not validate schema or records and silently drops malformed entries via `filter_map`. |
| A05/A09/N02 | `baseline::save` (`crates/token-audit/src/baseline.rs:120`) derives the snapshot name only from the report timestamp and renames over an existing name; same-second saves collide and overwrite. |
| A19 | `sweep_stale_scratch` (`crates/harness-core/src/native_build.rs:268`) removes prefix-matching directories by age in the process temp root without verified ownership or a live lease. |

## Entry-point and test map

| Change group | Native owners |
| --- | --- |
| 1 RTK | `crates/harness-rtk/src/main.rs`, `crates/codex-harness/tests/rtk_adapter.rs` |
| 2 model/effort | `crates/harness-core/src/portable_config.rs`, `crates/harness-core/src/launcher.rs`, `crates/harness-core/src/native_launcher.rs` and their existing tests |
| 3 baselines | `crates/token-audit/src/baseline.rs`, `crates/token-audit/src/report.rs`, `crates/token-audit/src/main.rs`, `crates/token-audit/tests/` |
| 4 interval/incremental | `crates/harness-core/src/rollout_reader.rs`, `crates/token-audit/src/report.rs`, `crates/codex-harness/tests/delegation_usage.rs` |
| 5 launch integrity | `crates/harness-core/src/build_identity.rs`, `crates/harness-core/src/build_selection.rs`, `crates/harness-core/src/native_build.rs`, `crates/harness-core/src/native_launcher.rs` |
| 6 scratch | `crates/harness-core/src/native_build.rs` and its tests |
| 7 xAI transport | `crates/harness-core/src/native_launcher.rs`, `crates/harness-core/src/xai_responses_shim.rs`, `crates/codex-harness/tests/xai_transport.rs` |
| 8 feedback/publication | existing dev/package/integration routes over `crates/harness-core/src/native_build.rs` and `build_identity.rs` |
| 9 benefit evidence | `crates/harness-core/src/benefit_gate.rs`, `crates/harness-core/src/outcome_report.rs`, `crates/codex-harness/tests/outcome_*.rs` |
| 10 skills/context | `crates/codex-harness/tests/skills_isolate.rs`, `crates/codex-harness/tests/skill_eval_*.rs` and the skill owners they exercise |
| 11 semantic readiness | `crates/harness-core/src/serena.rs`, `crates/harness-core/src/serena_shared.rs`, `crates/codex-harness/tests/serena_stdio.rs` |
| 12 CI | new `.github/workflows` plus the native check commands in `docs/rust-native.md` |
| 13 integration | installation lifecycle, outside-checkout consumers and this record |

## Synthetic input and oracle ownership

- Concurrent storage (RTK packs, baseline snapshots): every new reproduction
  runs under an isolated `tempfile::TempDir` home. RTK tests set `CODEX_HOME`
  to the fixture home through the `rtk_adapter.rs` invoke helpers, and
  token-audit tests pass explicit temporary directories; fixtures never touch
  the real user home. New two-process reproductions must keep this boundary
  and use deterministic synchronization rather than sleeps.
- Transport (xAI/loopback): owned `127.0.0.1` listeners on ephemeral ports
  with synthetic secrets only; no production endpoint or credential is
  contacted by tests.
- Usage (rollout sessions): synthetic session files under temporary roots;
  the real rollout directory is never a test input.
- Oracles: authoritative expected outcomes are compiled assertions in tracked
  test source; candidate binaries cannot rewrite them. Any file a candidate
  writes is compared against the oracle, never trusted as its own expectation.
- Evidence locations: public synthetic commands and outcomes live in this
  tracked record; raw logs, model-backed inputs and machine-local detail stay
  in local temporary or private roots outside the repository.

## Group 2: installed native configuration contract (task 2.1)

The installed consumer is Codex CLI 0.157.1
(`@openai/codex` npm package, registered upstream
`.../@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe`,
sha256 `8cb0e69e99ff2a158c54815db82d0f2e524d8f301bc30184722cfd1ae5973574`).
It was qualified on 2026-09-29 with owned synthetic homes and project
directories; every case ran `codex doctor --json` through the upstream
executable directly, which is model-free. Observed contract:

- Layer order: trusted-project file over user configuration; CLI `-c`/`-m`
  overrides win over both. A trusted-project file is the nearest
  `.codex/config.toml` from the working directory up to a directory whose
  exact `[projects."<path>"]` trust entry is `trusted`; outside a git
  worktree only the working directory itself can be that directory, inside
  one any ancestor up to the worktree root can, and the nearest trust entry
  wins (verified with trusted cwd, trusted intermediate, trusted root,
  untrusted, outside-worktree, non-git-ancestor, no-trust and both-trusted
  tie cases).
- Untrusted project files never apply. Model or effort keys inside the
  `[projects]` table itself are not applied; the table carries trust state.
- `profile`/`--profile` now selects a sandbox policy (`CONFIG_PROFILE_V2`).
  A `[profiles.p]` table with model or effort keys makes the whole
  configuration fail to load with `invalid data`; configuration-load failure
  is therefore a required launcher state, not a theoretical one.
- `codex doctor --json` reports the effective `model` but not
  `model_reasoning_effort`; no supported model-free API exposes the
  effective effort. The app-server `config/read` route used by
  `codex-harness diagnose` reports per-setting values with layer
  declarations, including project declarations.

The launcher fix therefore resolves the two applicable configuration files
for the two decision keys (model, `model_reasoning_effort`) with the trust
rule above, treats any read or parse failure as an unknown native choice that
blocks fallback injection, and keeps `codex-harness diagnose` as the
provenance owner: its settings/declarations come from the real native RPC and
its `launcherPreferences` view reports the launcher-side resolution with
sources.

## Group 2: implementation and verification (tasks 2.2-2.4)

Changed owners: `crates/harness-core/src/portable_config.rs` (layer
resolution, shared-default shield), `crates/harness-core/src/launcher.rs`
(`NativePreferences` input to `per_model_effort`),
`crates/harness-core/src/native_launcher.rs` (effective-preference
resolution with the working directory),
`crates/codex-harness/src/source_diagnostics.rs` (`launcherPreferences`
view), `crates/codex-harness/src/main.rs` (project-aware
`config-overrides`), `docs/token-workflow.md` (fallback-mapping wording),
and the launcher/diagnostic test owners.

Verified 2026-09-30 on this tree (heavy-command route, `--jobs 1`):

- `cargo test --locked -p harness-core --lib --test launcher -- --test-threads=1`:
  726 lib tests and 11 launcher tests passed, including the new
  `per_model_effort_yields_to_applicable_native_configuration`,
  layer-resolution, worktree-boundary, error and shield unit tests.
- `cargo test --locked -p codex-harness --test native_launcher --test source_diagnostics --test mcp_cli -- --test-threads=1`:
  27 native-launcher tests (including the saved model/low-effort
  counterexample, trusted-project preference, fallback-only injection and
  unresolved-configuration cases), 4 source-diagnostics tests and 4 mcp_cli
  tests passed.
- Explicit real-CLI acceptance
  (`HARNESS_SOURCE_REAL_CLI` = registered Codex 0.157.1 upstream,
  `HARNESS_SOURCE_REAL_GIT` = installed Git):
  `actual_native_layers_conflicts_privacy_and_restoration` passed with the
  corrected expectation — a trusted project's `model_reasoning_effort = low`
  is the effective native value (previously the shared `xhigh` default
  overrode it), the lower declarations stay visible, private sentinels never
  appear, and `launcherPreferences` reports the model/effort sources
  (`shared-default`, `trusted-project`).

The TUI launch variant, CLI/profile/remote/compatibility-selector bypass,
missing effort, unmapped models and malformed configuration states are
covered by the same launcher tests; the profile-shaped `[profiles.p]`
configuration failure is exercised through the unresolved-configuration
case, matching the qualified 0.157.1 behavior above.

## Group 6: owned scratch reclamation (tasks 6.1-6.3)

Implemented in worktree slot 4 (`exec-ds-scratch`, base `dd5b151`) and
merged as `f5d835f` after lead review. Changed owners:
`crates/harness-core/src/native_build.rs` (dedicated `%TEMP%\chx` root with
an ownership record, per-entry `owner` identity plus a live `lease` object
held by an exclusive file lock, sweep that reclaims only verified owned
entries with a released lease, reparse children skipped, record removed last),
`crates/codex-harness/tests/native_build.rs` (real interrupted-build
acceptance) and `docs/rust-native.md` (bounded recovery text). The legacy
`hcb-`/`hcc-`/`hca-` and `harness-build-prerequisite-` sweeps are removed:
unproven legacy scratch is preserved (BREAKING per the change spec) with
documented manual recovery.

Executor evidence (all through `codex-harness heavy --`): 12
`native_build` lib tests, the 8-test `native_build` integration suite
including a real killed build with lease release, blocked-cleanup and
foreign/junction/neighbor survival (1212 s), `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --locked -- -D warnings`,
`harness-source-check --root .` and `codex-harness ownership-check` clean.
The full workspace suite cannot finish inside the shared 30-minute heavy
deadline, so the executor ran the identical scope as three partitioned runs
(`--workspace --exclude codex-harness`, the codex-harness lib/bins and 63
test targets excluding `native_build`, and `--test native_build`), all
passing; the literal single-command workspace gate remains part of the
integrated-candidate acceptance below.

Lead review and independent verification (2026-09-30): diff inspected
(ownership verification, lease acquisition on an existing lease file only,
record-last removal, reparse refusal, foreign-root preservation); the 12
`native_build` lib tests were rerun independently on the executor worktree
and pass. Lead decisions: the three-partition workspace evidence is accepted
for this slice; the short `chx` root name and the bounded empty-directory
crash window (identity not yet published) are accepted as designed.

## Group 11: semantic readiness evidence so far (task 11.1, unfinished)

Observed incidents, preserved in local logs (not tracked; private paths):

- `serena-home\workers\99fc5a20a9ad7890\logs\2026-09-29\mcp_20260929-145914_6244.txt`
  (planning session) and `...\mcp_20260929-220952_3712.txt` (dispatch burst).
  Both show the same sequence: the Node-based BasedPyright language server
  dies with `JavaScript heap out of memory`, its GC trace reporting only
  ~180 MB heap at the failed allocation; in the 22:10 incident rust-analyzer's
  flycheck `cargo check --workspace --all-targets` failed simultaneously
  (rustc exit `0xC0000409`) inside the same worker tree; Serena then reports
  `The language server manager is not initialized`, and the pool's single
  bounded retry also fails under the same pressure.

Verified backend inputs and coverage: Serena agent 1.7.0 (uv tool) with
rust-analyzer (rustup stable) and BasedPyright under Node 24 (Winget); the
managed configuration selects Python and Rust, and the checkout contains
Python files (2, the migration-owned classified paths), so the Python layer
is a deliberate selection, not a discovery accident. Each worker tree runs
under one Windows Job with a 4 GiB memory limit and 25% CPU
(`serena::WORKER_JOB_MEMORY_BYTES`; a 2 GiB limit previously caused a
`MemoryError` and the same uninitialized-manager failure, after which the
limit was raised). Workers are shared per route/configuration; warm steady
state is small (~200 MB active: serena agent, one rust-analyzer, no Node).
The pool shares one cold start among concurrent callers of the *same*
configuration and reports the configured limit (not a measurement) in its
status.

Resource evidence that constrains the cause: Node's own heap limit was not
reached (180 MB at refusal), so the allocation was refused externally. Two
candidate external causes remain, both consistent with all evidence: (a) the
per-worker 4 GiB Job exhausted by the combined startup transient
(rust-analyzer full-workspace analysis plus its flycheck cargo/rustc children
plus BasedPyright indexing in one tree), or (b) machine-wide exhaustion: the
host has 15.8 GB RAM, and the 22:10 burst started five fresh worker trees on
five workspace copies (four executor worktrees plus the main checkout) while
other lead sessions' executors were also active. Both incidents coincided
with concurrent heavy activity; four later staggered fresh workers
(2026-09-30 00:30, 02:02, 03:04 and one at 22:55) initialized cleanly with no
OOM, uninitialized-manager or startup failure in their logs.

Still open for 11.1: a measured fresh-startup peak (one cold worker with job
memory sampling) to separate cause (a) from (b). It is deliberately deferred
while four executor suites occupy the machine; reproducing the memory burst
now would sabotage them. No correction has been selected yet — the existing
single bounded retry is not duplicated, and no language or resource policy
has been weakened.

Measured cold start (2026-09-30 04:55, task 11.1): with 7 GB free and no live
language servers, a fresh managed worker for this checkout was cold-started
through the ordinary semantic route (the same configuration as both failing
incidents) while a detached sampler recorded every worker-tree process. The
operation succeeded in 27.5 s. Tree commit peak: **~1 112 MB** (rust-analyzer
756 MB, BasedPyright-under-Node 320 MB, Serena agent 166 MB, proc-macro
server 3 MB), settling to ~343 MB after warm-up. The sampler did not include
flycheck cargo/rustc children; with the warm target directory they were not
observed. Conclusion: a single cold start stays far below the 4 GiB per-worker
job cap, so cause (a) alone cannot explain the incidents unless a cold
flycheck adds multi-GB rustc children. Cause (b) — machine-wide commit
exhaustion (five concurrent cold trees ≈ 5.5 GB of language servers plus four
executor cargo suites with 8 GiB caps on a 15.8 GB machine, matching the
22:10 burst) — is the strongest supported cause, with a cold-flycheck
transient as a possible per-tree amplifier. The smallest cause-supported
correction is therefore a machine-wide bound on concurrent cold starts in the
shared pool, not another retry, a larger cap or a weaker language policy.

Group 11.2 correction (2026-09-30): the shared pool now bounds cold starts
across configurations. `serena_route::Policy` gains
`max_concurrent_startups` (read from `global/tool-resources.json`, default 1,
validated 1..=4; this checkout sets 1), and the pool's reservation step waits
while another configuration's startup is in flight, instead of starting
beside it. Same-configuration callers keep sharing one startup, the existing
single bounded retry is untouched, no language or worker limit changed, and
the pool status now reports `concurrent_startup_limit` and a
`serialized_startups` counter so the bound is observable. Verified:
`different_projects_cold_start_serially_under_the_startup_bound` (held first
startup, deterministic counter-based wait, second factory provably not
invoked until release, both complete, `cold_starts == 2`), plus the
same-config sharing, independent-overlap and policy tests; 40 sibling
`serena_shared`/`serena_route` tests passed with the production change in the
same session; `cargo clippy -p harness-core --all-targets --locked --
-D warnings` clean. Task 11.3 remains open: the installed-consumer exercise
outside the checkout waits for the corrected build to reach the global
lifecycle (group 13 delivery).

## Group 1: RTK publication, validation and presentation (tasks 1.1-1.6)

Implemented in worktree slot 1 (`exec-ds-rtk`, base `dd5b151`) and merged as
`975a026` after lead review and independent verification. Changed owners:
`crates/harness-rtk/src/main.rs` and
`crates/codex-harness/tests/rtk_adapter.rs` only.

Behavior: one short store transaction (`store.lock` with a bounded 2 s wait,
staging under `pack/staging/` written inside the lock, rename into place,
atomic index replacement through a unique temporary name, oldest-first
eviction only after the index commit); recovery under the lock removes only
staging leftovers and validated-index-unreferenced `*.log` files and never
follows reparse points; strict index validation (schema, per-record types,
handle grammar, 64-hex digest, duplicates, checked size accounting) aborts
publication and retention before any deletion; recall serves verified
committed content with explicit expiry; complete-presentation accounting
decides `applied` only when the delivered bytes (both streams, locators,
handles, exit notice, adapter notices) are smaller than the captured raw
bytes, with fallbacks recomputed; Cargo status recognition requires the exact
`{:>12} ` layout and keeps every nonmatching line byte-verbatim. A
deterministic barrier seam (`HARNESS_RTK_TEST_BARrier_DIR`) is inert without
the environment variable.

Verification: executor ran fmt/clippy clean, the 38-test `rtk_adapter` suite,
decomposed package coverage (a single workspace invocation cannot fit the
1800 s heavy deadline), pre-fix failure demonstrations on an instrumented
base revision (two-process publication loss, unbounded contention, N01
parent-relative deletion of an outside sentinel) and an outside-checkout
staged run with an isolated home. Lead review inspected the transaction
design and the full 38-test suite was rerun independently on the worktree
(38/38 pass). The global installed acceptance of task 1.6 folds into group
13 delivery; the load-sensitive `executor_message` flake the executor
observed is unrelated to RTK and is to be confirmed on an idle machine.

## Task 13.1: small real user path outside the checkout

Exercised 2026-09-30 with the merged tree (`857eb8a` binaries) and an
isolated `CODEX_HOME`, from working directories outside the checkout:

- **Analyzer**: `token-audit report --format text` (exact coverage and
  warnings, no fabricated totals), `baseline save` (uniquely named immutable
  snapshot with nanosecond publication identity), `baseline diff --format
  text` (valid snapshot, per-side coverage, `usage_basis=unavailable` and
  `partial=true` labels — a reduced subtotal is never a saving), retention of
  the comparison, and `detail --diff <retained> --sessions` paging. A lookup
  of an evicted comparison produced the designed explicit error naming the
  remedy.
- **RTK**: `harness-rtk compact cargo test … -- --list` and
  `compact git log …` ran the real child exactly once with correct exit
  status and byte-identical output from an outside working directory;
  both outputs honestly stayed raw (test-name and log content is not
  filter-compressible), matching the complete-presentation decision. The
  packed recall path is covered by the executor's staged outside-checkout
  compact/recall run (digest-verified window, retention expiry) and the
  lead's independent 38/38 `rtk_adapter` rerun.
- **Native launch**: `codex --version` through the installed harness launcher
  from an outside working directory returned the real upstream
  `codex-cli 0.157.1` in 845 ms; the merged-tree built manager also ran from
  outside the checkout. Global delivery of the merged build itself remains
  task 13.2.

## Groups 9 and 3: accepted executor slices (2026-09-30)

**Group 9 (tasks 9.1-9.6), merged as `7d32067`.** Executor `exec-ds-benefit`
delivered the stricter benefit gate (a consistent record without a positive
effect — improved quality or a faster candidate — can no longer authorize a
default; tolerated regressions stay non-adoptions) and complete
experiment/pair/block accounting with unit-level independence (Cartesian
edges from one unit are not independent samples), retry identity matching,
single attribution of worker usage and attempt time, undefined per-task ratio
at zero accepted tasks, predeclared claim classes
(deterministic/stochastic/unmeasured), and the no-migration/no-blind-pruning
decisions with a middle-diagnostic counterexample. Executor verification:
fmt, clippy `-D warnings`, harness-core (731 lib + integration) and every
codex-harness target in bounded batches (a single workspace invocation cannot
fit the 1800 s heavy deadline; the policy kill was not a test failure). Lead
review: diff inspected; 24 benefit_gate/outcome_report lib tests rerun
independently on the worktree and pass. The group-9 analogues of the owning
boundaries each have a rejecting regression; the cross-group half of 9.6
(RTK/baseline/runtime slices) is re-reviewed at integration (13.3).

**Group 3 (tasks 3.1-3.4 complete), merged as `22ba5c9`.** Executor
`exec-ds-baseline` delivered uniquely named immutable snapshots
(timestamp + nanosecond publication identity, create-new reservation, private
staging, pointer replaced only after the complete snapshot commits), full
snapshot validation with `snapshot_status`/`version_changed`/`compatible`/
`comparable` kept distinct, usage-basis/partial/warnings/coverage projection
through snapshot and diff, bounded diff text and stable detail/paging, and
the resolved-name/Codex-home fixes with escaping-name and pointer rejection.
Executor verification: fmt, clippy, all 45 token-audit tests, every
workspace target in chunks (two unrelated concurrency flakes pass in
isolation), and a local installed save/diff/detail path outside the checkout
with an isolated home. Lead review: diff inspected; all 45 token-audit tests
rerun independently on the worktree and pass. Lead ratifies the recorded
semantics: coverage regression and legacy snapshots yield `comparable=false`;
`latest` is the last completed publication; an unusable snapshot yields empty
movements and null baseline totals, never fabricated zeros.

Group 3 documentation reconciliation (2026-09-30): the bounded token-report
section of `docs/rust-native.md`, the baseline entry in
`docs/memory/token-audit.md` and the `tokenomics` skill's decide-with-a-
baseline step now describe the published surface (immutable snapshots,
publication-order `latest`, validation-before-compatibility, explicit weaker
status, bounded diff with exact degradation labels, `detail --diff` paging).
The documented command forms were verified against the merged CLI usage text
and `harness-source-check --root .` passed with 0 findings over 894 files.

## Group 12: deterministic and installed CI routes (tasks 12.1-12.3)

Two workflows were added and are enforced by a native contract test
(`crates/codex-harness/tests/ci_workflow.rs`, `yaml-rust2` dev-dependency):

- `.github/workflows/windows-native-checks.yml` — the deterministic push/
  pull-request signal: pinned Rust 1.98.1, `cargo fmt --check`, clippy
  `-D warnings`, the locked single-threaded workspace suite, the executable
  ownership check and `harness-source-check`. Only `actions/checkout` is
  used, pinned to its immutable commit revision
  `3d3c42e5aac5ba805825da76410c181273ba90b1` (v7.0.1, verified against the
  GitHub API); permissions are `contents: read`; no cache, no secrets and no
  model-backed route. The contract test fails when any required check
  disappears, an action floats off a full revision, a credential appears, or
  the model-backed selector leaks in — omitted checks cannot look green.
  Command failure propagation is observed directly (`cargo fmt --check`
  exited 1 on real formatting drift during this work).
- `.github/workflows/windows-installed-integration.yml` — the separate
  installed-lifecycle route, manual `workflow_dispatch` only: build, then
  launcher/recovery (`native_launcher`), delivery (`manager_delivery`),
  isolated outside-checkout install (`orchestration_isolated_install` with
  the pinned `@openai/codex@0.157.1` upstream via
  `HARNESS_CONTROL_CODEX_EXE` and `--ignored`, failing when the executable is
  absent), transport, MCP stdio and the Windows TUI. The contract test
  enforces the separation: no push/pull triggers, and the deterministic
  workflow contains none of the installed-only steps, so a green default CI
  run never stands in for installed acceptance.

Local exercise (2026-09-30): all four `ci_workflow` contract tests pass;
`orchestration_isolated_install` was exercised with the registered real
upstream Codex 0.157.1 and passed after its stale board-skill assertion
(`feedback-route v1`, removed from the skill in `bc77f79`) was corrected to
the current `pacing-observation v1`/`route records` contract — an example of
exactly the drift this route exists to catch. The ownership check passes
clean on the current tree (0 findings; its stale "exits nonzero" doc note was
corrected).

Task 12.4 remains open: an authorized actual CI run tied to the candidate
revision requires remote publication and Actions enablement, neither of
which has been authorized in this session.
