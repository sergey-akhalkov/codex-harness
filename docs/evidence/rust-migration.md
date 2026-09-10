# Rust migration acceptance

This change is unfinished. Source, behavioral parity and global cutover are
separate acceptance gates; passing the current Cargo subset closes none of the
unimplemented lifecycle or foreign-integration requirements.

## Selected inputs and ownership

The [focused native source report and diagnostic alias](rust-source-diagnostics.md)
complete task 4.5's implementation and owned-consumer acceptance, including the
legacy diagnostic registration upgrade and original CLI observations. The full
installation and global activation in section 9 remain unfinished.

[Native dependency discovery and release planning](rust-dependency-discovery.md)
complete task 5.1's port and actual CLI acceptance for the six selected
dependencies. Plans preserve uncertain/modified shared installations and retain
the project Rust toolchain. [Audited staging](rust-dependency-archives.md) and
[retained candidate validation/selection/rollback](rust-dependency-selection.md)
now pass their bounded native acceptance. Complete provisioning, registration,
the remaining task 5.2 lifecycle and global activation remain unfinished.

The [native CBM runtime increment](rust-cbm-runtime.md) has focused fixture
evidence for configuration inspection, account admission and bounded indexing.
Actual owned indexing and isolated catalogue retrieval pass. The [native stdio
increment](rust-mcp-stdio.md) exercises protocol, queue/cancellation and actual
Windows pipes; query integration and the complete installed runtime remain open.

[Empty Python environment preparation](rust-python-staging.md) now has an
explicit native CLI consumer. Complete runtime/wheel provenance and provisioning
remain unfinished; the candidate is not eligible for activation.

The [Serena external-interface investigation](rust-serena-boundary.md) establishes
a configuration-isolation path and reproduces the current HTTP listener/session
limitations. Native per-client prompt sessions share the active project and the
shipped listener accepts unauthenticated initialization. Task 3.2 stays open
pending a verified private session boundary and actual semantic/no-acquisition
acceptance; the installed Python adapter is preserved.

The [native PATH primitive and atomic receipts](rust-environment-path.md) have
owned-key publication, rollback/conflict and killed-process evidence, including
one KTM transaction for the registry write and confirming file. The
[native core connection](rust-core-installation.md) now integrates PATH with
schema-11 registration intent, Process/User scope, metadata retirement and actual CLI acceptance. Full lifecycle/component
orchestration and global activation remain open.

[Installation ownership](rust-installation-metadata.md#missing-owned-destination-repair)
now supports guarded repair of absent owned links with new identities, while
preserving foreign replacements and omitted ownership records. The actual native
inspection consumer also [serializes both connection and dependency owners](rust-installation-lock.md#separate-connection-and-dependency-owners).
These are accepted lifecycle prerequisites; the complete installer remains unfinished.

The [native outcome report](rust-outcome-report.md) now preserves attempts,
verification/retry timing and comparison exclusions through an explicit local
CLI consumer. The [native usage parser](rust-delegation-usage.md) now passes
ported CLI cases and a real closed-log comparison. The [native single-attempt
executor](rust-outcome-run.md) uses owned jobs and separate event/process
evidence. [Native discovery](rust-outcome-discovery.md) now verifies the selected
home, skill/config responses and unchanged inputs through installed CLI 0.153.4.
[Arm preparation](rust-outcome-arm.md) now consumes schema 1/2 installation link
checks and exact configuration publication, verified against both native modes.
[Controlled case preparation](rust-outcome-cases.md) now creates fresh Rust
targets and instructions for the five local cases without running models.
[Controlled oracles](rust-outcome-oracle.md) now verify those five cases through
real native targets and separate process evidence. Task 7.3 still requires the
external consumer cases, full suite migration and remaining integrations.

The input receipt at
`%LOCALAPPDATA%/codex-harness-evidence/rust-inputs-62dc5d72165947b88d7e400181e180fa/inputs.json`
records HEAD `dfc2d6af49f2da949b2b1b0ed549a33c62d1eb4b`, the exact capture time,
457 Git-listed tracked/nonignored dirty source hashes, and hashes of the five
installed component/connection registries. It contains no credential bodies.
[Per-unit ownership](rust-migration-map.json) accounts for the 211 executable
or derived-cache units in that snapshot, including skill resources and the
source owners of generated/embedded test programs. Every executable entry in
the then-current `global/kit.psd1` was present in the map. The map records planned
replacement/retirement ownership, not proven retirement. New files added during
implementation must enter the final inventory; snapshot counts cannot close it.

Observed installed identities on 2026-09-08: Codex CLI 0.153.4, OpenSpec 1.12.0,
OpenCodex 2.44.0, Serena 1.7.0, Codebase Memory 0.10.8, graphifyy 0.9.55,
RTK 0.48.0, basedpyright 1.39.10 and rust-analyzer 1.97.1. Nuphus 0.2.2 is
reported **modified** by discovery, not silently accepted as an intact upstream
package. Its provenance/behavior needs the existing native dependency acceptance
before final migration delivery. The installed script launch/check/bootstrap
links point into the mapped `tools/` sources. The four MCP registrations use the
same managed source and foreign dependency inventory. Existing instructions,
skills, configuration and registries remain source-owned data.

| Required behavior | Current owning checks / contract | Native replacement responsibility |
| --- | --- | --- |
| Source links, ownership, install/update/recover/disconnect, relocation and component isolation | `tests/installer.Tests.ps1`, `tests/activation.Tests.ps1`, `tests/code-tools-scoped.Tests.ps1`; `linked-global-kit` | Native installation/component lifecycle; scripts remain until migrated acceptance passes |
| Argument/Unicode/cwd/stream/exit forwarding and real console | `tests/launcher.Tests.ps1`, `tests/ConPty.cs`; native-launch compatibility | Native launcher and process/console fixtures |
| Private model-free bounded source diagnostics | `tests/source-diagnostics.Tests.ps1`; `harness-source-diagnostics` | Native diagnostic command and global alias |
| Job admission, memory/CPU, timeout/cancel, PID ownership and cleanup | `tests/process-ownership.py`, `tests/subscription-process.Tests.ps1`, `tests/tool-resources.py`; bounded-tool requirements | Shared Rust process primitives and real Rust fixture processes |
| Dependency ownership and four MCP protocols/project scopes | `tests/mcp-codebase.py`, `tests/serena-shared.py`, `tests/graphify_native.py`, `tests/nuphus-resources.py`, dependency/lifecycle suites | Native dependency and MCP lifecycle; retain upstream runtime provenance |
| Subscription auth, native restoration, exact role routing and service recovery | `tests/subscription-config.Tests.ps1`, `tests/subscription-consumer.Tests.ps1`, `tests/subscription-service-recovery.Tests.ps1`; subscription specs | Native subscription service; destructive acceptance only in owned isolated services |
| RTK explicit hook/command, one execution, raw recovery and feature ownership | `tests/rtk-adapter.py`, `tests/token-workflow.Tests.ps1`; token-workflow spec | Existing Rust adapter plus native lifecycle/tests |
| Skill discovery, bounded structured runs and regression helpers | `tests/structured-codex-run.py`, `tests/isolated-worktree-example.ps1`; active native-workflow specs | Native skill helpers and owned outside-project consumers |
| Outcome/usage schemas, privacy, attribution and preserved failure oracles | `tests/delegation-usage.py`, `tests/outcome-report.py`, `tests/outcome-oracles.py` and native counterparts | Native evaluation/usage commands; no new opencode-kit evaluations |
| Inactive diagnostic handlers, legacy brokers, caches and temporary scripts | Current consumer/manifest checks plus `tests/subscription-efficiency.py` | Port shared accepted behavior; retire only with per-unit consumer evidence |

Concurrent requirement reconciliation is recorded in
[native compatibility](../rust-native.md#compatibility-during-concurrent-changes).
The three existing source-update scenarios and both pending deltas must survive
sync. Ordinary hooks remain off; the accepted RTK exception and explicit Serena
Python/Rust selection are preserved. No autonomous-skill task was closed by this
reconciliation.

## Initial native build evidence

From the source root on Rust/Cargo 1.97.1 and the native MSVC linker:

- `cargo check --workspace --locked` passed for the initial workspace.
- `cargo test -p codex-harness --test native_build --locked` passed two real
  CLI/Cargo cases in temporary Unicode/space paths. Cases cover first build,
  unchanged reuse, documentation edits, stale source with management available,
  binary tampering, failed candidate preservation, missing tool and foreign state.
- `cargo test -p harness-core --lib --locked` passed the three initial source
  identity, manager-integrity and file-lock/foreign-state checks.
- An initial native publish failed with Windows access denied while the build
  receipt was still open. Closing/syncing it before directory rename corrected
  the actual failing integration case; no test oracle was weakened.

These commands describe the initial increment. Process primitives and ongoing
source changes require combined checks before workspace task closure. Explicit
Check was exercised with PATH pointing to a nonexistent tool directory and still
returned the expected structured result, proving it does not need Cargo/model
execution for the exercised healthy-build case.

## Transitional RTK workspace delivery

The duplicate member lockfile was retired after checking consumers. The legacy
manifest/installer now select root `Cargo.toml`/`Cargo.lock` and `-p harness-rtk`.
The script compatibility path is temporary; the final migration still requires
Rust lifecycle and migrated test helpers.

`cargo build -p harness-rtk --release --locked` passed. Existing native adapter
acceptance exercised the resulting binary: 15 passed, one optional pytest case
was skipped because no pytest interpreter was selected. This is not a claim that
the skipped case passed. The isolated token lifecycle passed all 15 assertions
at `%TEMP%/harness-token-lifecycle-665e8a05ae2e47bba86186e3a765c38c`.

After those checks, `./install.ps1 -Mode Install -TokenWorkflowOnly` rebuilt and
connected the adapter successfully. From `%TEMP%`, the absolute installer path
with `-Mode Check -TokenWorkflowOnly` returned `Token workflow connected` and RTK
0.48.0. This preserved the current global feature while the remaining native
migration continues; it is not global cutover of the whole harness.

## Reviewed native foundation — 2026-09-08

The native manager now exposes `activate-build` and `recover-build` for an owned
local candidate selection. Global command/service registrations remain separate
unfinished work. Selection journals preserve the exact old pointer; healthy
starting states roll back after interruption, while repairs that started from
damaged artifacts finish the verified replacement. Completed journals retain
the damaged starting evidence. Foreign pointer edits leave the journal intact.

Independent source review identified two P1 cache defects. Both were reproduced
through the actual manager/Cargo CLI before correction:

| Counterexample | Failing evidence | Corrected behavior |
| --- | --- | --- |
| Change source bytes and restore mtime | `%TEMP%/harness-native-mtime-UnOrrl`: accepted executable printed `candidate-a` instead of `candidate-b` | A fresh compiler target for each candidate produced `candidate-b`; unchanged builds reuse verified immutable artifacts |
| Pre-existing cache reparse point | `%TEMP%/harness-native-cache-link-6a0sht`: Cargo created a directory in the owned foreign-target fixture before rejection | Compilation uses a newly allocated target; the sentinel and foreign directory contents stay unchanged |

The baseline builder source SHA-256 is
`B863C555524C83495AB4C6B1989497F78C40F94F2610605501A9271931BFECB9`, retained as
`review-baseline-native_build.rs` in the receipt directory below. These cases
demonstrate actual defects and their correction; no production target was used.

Additional corrections cover canonical Windows ancestry checks, missing old
metadata and already damaged repair states, source modules named `examples` or
`tests`, compiled text resources, ancestor/user Cargo configuration hashes and
ambient compiler overrides. Cargo dep-info must agree with the input inventory;
an omitted compiled resource fails candidate acceptance. The tested native
resource convention is documented in [native commands](../rust-native.md).

Current accepted checks comprise six real manager/Cargo CLI cases, nine
library cases, three launcher-policy cases, seven [ConPTY cases](rust-console.md)
and fourteen process cases. Strict Clippy passed the affected library, binaries
and integration targets. The legacy script launcher also passed 157 assertions;
the Rust argument policy is not yet the global launcher. One combined process
run failed because its 0.01% CPU fixture starved during initialization. At 0.1%,
the unchanged CPU-ratio oracle measured 0.015625 s against an uncapped 3.0 s;
the corrected full fourteen-case process binary passed in 6.14 s.

Real clean release preparation and manager repair evidence remains at
`%LOCALAPPDATA%/codex-harness-evidence/native-candidate-cf47006dc75b44b6b4636b98026fb1c9/`.
The reviewed build initially hit MSVC LNK1181 under a long nested target path;
a short owned temporary compiler directory corrected that failure. Concurrent
console publication also produced E0583, with previous artifacts preserved.
Later compiler runs exposed memory-allocation failure and misleading stdlib
metadata errors on the loaded 16 GiB host. Explicit Cargo compilation now uses
one compiler at the same 2 GiB Job limit. The first owned-snapshot release still
ran out of memory while resource tests overlapped; its sequential repeat passed.
Logs for every failed preparation remain under the corresponding state/staging
directory. Existing installations were not replaced by failed candidates.

`bounded-compile.json` records the successful bounded release
`fcee13ca3cd503956fbf7e8aa5705a785b74db7343df7be448978fb7f3d95316`.
After a meaningful source-comment update, that verified manager reported
source-stale (management allowed/runtime refused), then built
`8c714c6445f212e0f034a2d3105392e0297d9a1e14cae194c9ca74a765b988b7`
in `bounded-stale-repair.json`.

To decouple ongoing worker edits, an owned frozen Rust workspace was copied to
`manager-repair-source`. It built and selected a healthy candidate with source
`77b91fa324f6bfaba52e70236ae3d74cd9f70123bf07499ade791abe11d8f2b4`.
The manager file was backed up, its resolved path checked inside that owned
state, and only that file removed. Native Check rejected it; explicit Cargo
bootstrap built a fresh replacement (`cargo-bootstrap-repair.json`).
Restoring the old executable with an appended test marker made its actual
mutating command refuse execution with the explicit bootstrap message.
A first root-workspace bootstrap attempt saw a concurrent dependency/lockfile
mismatch and failed honestly. Bootstrap from the frozen workspace then reused
the healthy replacement (`altered-manager-bootstrap-2.json`).

The repaired executable selected its immutable candidate from outside the
checkout. `bootstrap-final-check.json` is healthy with both permissions true;
`bootstrap-recovery-idempotent.json` reports changed=false. Interrupted
selection, already damaged starting states and foreign conflicts are also
covered by the deterministic library tests. This is process-boundary recovery
evidence, not a simulated power-loss guarantee or global cutover.

Task 2.3 was reopened after an actual old-producer/new-consumer counterexample:
`%LOCALAPPDATA%/codex-harness-evidence/update-producer-4c889ee82b54442191819ce9d3eabb3c/`.
A copied old manager was healthy, became source-stale after current Rust inputs
were overlaid, then built and selected a candidate with exit 0. Its old validator
reported healthy; the actual new manager reported incompatible with both runtime
and management disabled. The old producer omitted the new `harness-observe.exe`
and used its own source-identity rules. Global registrations were untouched.
The [native version handoff](rust-self-update.md) now passes the real transition,
cross-version recovery and independent review. A second reproduced P1 in
indeterminate predecessor classification was corrected and checked before task
2.3 closed again; previous same-contract repair evidence remains valid.

Tasks 2.1 through 2.5 are closed for the native foundation. The subsequent
[native launcher acceptance](rust-launcher.md) supplies task 2.2's ordinary
launch, complete input/build identity and actual outside-checkout CLI evidence.
Global lifecycle, foreign integration parity and final combined acceptance
remain unfinished.

The next increment verifies the [native OpenCodex validation boundary](rust-opencodex.md).
It leaves browser-only OAuth, restoration and the rest of task 3.1 open.
The [structured Rust helper](rust-structured.md) also passed an actual Astra
schema-constrained consumer; native skill registration remains unfinished.
The [native core data inventory](rust-inventory.md) reads current links and
descriptors through a declarative manifest; installation mutation remains open.
The [installation mutex](rust-installation-lock.md) now serializes that native
consumer with the old installer. [Legacy metadata inspection](rust-installation-state.md)
retains host, dependency-owner and adopted-link distinctions without mutation.
The [native registration foundation](rust-registration.md) now verifies owned
link staging/publication/recovery, including real process interruption and
same-target competing actors. Full configuration and installer integration,
relocation and global activation remain open in tasks 4.1 through 4.3.
The [native feature editor](rust-feature-edit.md) prepares and verifies Hooks and
Code Mode candidates through the actual upstream CLI, preserving unrelated
configuration. Its [existing-file publication](rust-config-file.md) now verifies
atomic writes, guarded rollback and abrupt process termination. The registration
journal now includes those configuration changes and passes interrupted combined
recovery. [Absent configuration creation](rust-config-creation.md) now shares
the journal and passes mixed interrupted recovery, foreign conflict, alias and
immediate reconnection checks. [Existing-link replacement/removal](rust-link-changes.md)
then adds original-object rollback, including interrupted publication and an
unavailable old checkout. [Finish](rust-registration-finish.md) adds
irreversible commitment and restartable cleanup with an explicit old-reader
version boundary. The [metadata builder introduced in schema 8](rust-registration-metadata.md)
also persists reused IDs and feeds the [stable ownership document](rust-installation-metadata.md),
including a verified isolated legacy conversion. The [native core consumer](rust-core-installation.md)
now integrates PATH and metadata retirement in schema-10 intent and checks actual CLI startup; complete
installer/component orchestration and global activation remain open.

The combined checkpoint after the launcher/inventory increments passed
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --offline
--locked --jobs 1 -- -D warnings`, and `cargo test --workspace --offline --locked
--jobs 1 -- --test-threads=1`: 55 tests passed. The explicit installed OpenCodex
test is ignored by the default suite; its separate accepted run is linked above.
This checkpoint precedes the concurrent regression-helper integration and does
not replace the still-required final global migration acceptance.
The [Rust regression observer](rust-regression.md) subsequently passed its nine
scoped tests, including parent-requested preservation corrections. Global skill
invocation and final combined acceptance remain open.
