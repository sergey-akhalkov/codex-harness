# Retained native dependency candidates

This is an increment of Rust migration task 5.2. It connects the
[official archive staging](rust-dependency-archives.md) to runtime validation
and journaled local selection. Global registration, complete provisioning and
the migration task remain unfinished.

The implementation is in
[dependency_candidate.rs](../../crates/harness-core/src/dependency_candidate.rs),
[dependency_selection.rs](../../crates/harness-core/src/dependency_selection.rs)
and the [CLI routes](../../crates/codex-harness/src/dependency_selection_cli.rs).
The explicitly supplied manifest SHA-256 must come from trusted preparation.
A caller-created manifest is not authenticated merely by matching its own hash.

`dependencies validate` inspects the complete package file listing and then
runs the selected runtime check. Inspection rejects missing/extra files and
directories, path aliases, reparses, hardlinks, changed hashes, mismatched
package identity and inconsistent sizes. Limits include an 8 MiB manifest,
512 MiB package payload and 16,384 traversed entries. BasedPyright additionally
requires an explicit Node executable and SHA-256. Its `--version` process has
a private environment/cwd, bounded pipes, a 20-second deadline, and a 512 MiB /
25% CPU job. Wrong version, nonzero exit and output flooding are rejected.
Codebase Memory and the Nuphus native companion use the existing bounded MCP
probes. The standalone Nuphus wrapper remains incompatible with this consumer;
validation never installs its companion.

Opaque inspection/validation results retain file handles through their caller's
operation. These are share-exclusion observations, not a transactional whole-tree
snapshot or an OS sandbox. They do not exclude every possible preexisting mapped
writer. Runtime validation checks the stated CLI/MCP compatibility, not arbitrary
future tool actions. The existing Nuphus schema gaps remain reported by its probe.

`dependencies select` accepts only a candidate under the same native-owned
state's `dependency-staging` directory. The supported slots are
`codebase-memory`, `nuphus` and `basedpyright`. It validates compatibility and
changes a small selection record; package directories and adopted/shared
installations stay in place. `selected` inspects integrity without package
execution or acquisition. `recover-selection` handles an interrupted journal;
`rollback-selection --receipt-sha256` uses an exact retained operation receipt.
These records do not yet connect the selected package to global MCP/LSP launchers.

The selection journal records before/after bytes and file IDs before mutation.
Native transaction/handle guards protect creation, replacement and deletion.
The journal stays pinned while publishing; history and a separate durable
ownership receipt are retained before journal removal. This prevents a later
copying editor's same-byte replacement from being adopted as an owned pointer.
Unexpected bytes/objects are preserved for explicit recovery.

Recovering a committed deletion reserves a new file, amends the journal with
its new ID, then publishes it. Recovery also saves a usable rollback receipt for
that restored object. An idle recovery retry can rediscover this receipt if the
previous process finished the write but lost its response. No package tree is
removed by these operations. This is process-interruption recovery; no physical
power-loss guarantee is asserted.

Independent source review identified the foreign-record adoption and restored-ID
rollback gaps before acceptance. The corrected
[selection tests](../../crates/harness-core/src/dependency_selection_tests.rs)
cover the public foreign replacement entry point, before/after interruption,
recovery followed by rollback, lost-response retry and conflicting journals.
The initial parent run passed four tests and failed four public entries with
Windows error 32. Replacing the owner marker's transactional guard with a retained
read-only guard allowed nested ownership verification; all eight then passed.

The [CLI acceptance](../../crates/codex-harness/tests/dependency_selection.rs)
has deterministic option/privacy checks and an ignored actual-artifact case.
The latter requires `HARNESS_DEPENDENCY_STAGE`,
`HARNESS_DEPENDENCY_MANIFEST_SHA256` and `HARNESS_DEPENDENCY_SLOT`, plus explicit
Node path/digest variables for BasedPyright. It requires an initially unselected
owned slot, executes package validation, exercises selection/reuse, a same-byte
foreign replacement and rollback, and leaves the slot unselected. Package
directories and operation evidence remain. Every CLI invocation runs outside
the checkout. It makes no global registration changes.

Acceptance on 2026-09-10 used Windows x64 and Rust/Cargo 1.97.1 at HEAD
`1ba176c9bf8926795e734bb76e8a6b4d38459dc3` plus the recorded dirty source.
Evidence is in
`%LOCALAPPDATA%/codex-harness-evidence/core-lifecycle-29d511c364fb499286ae57b389e34c6a/`.
`dependency-selection-checkpoint.json` records focused source and actual CLI
hashes, exact commands and retained evidence paths. The earlier archive and
build-selection checkpoints remain separate identities.

- `dependency-selection-combined.stdout` / `.stderr`: 65 dependency core tests
  passed, five explicit-only cases ignored. This includes the seven candidate
  and eight selection tests; those counts are not additional runs.
- `dependency-selection-cli-final.stdout` / `.stderr`: two deterministic CLI
  tests passed, the actual-artifact case ignored in that default run.
- `dependency-selection-actual-codebase`, `-actual-nuphus` and
  `-actual-basedpyright` logs: one explicit actual-artifact lifecycle test passed
  for each package, in 44.76 s, 3.26 s and 84.48 s respectively. These are
  acceptance durations, not matched benchmark comparisons.
- Core/CLI all-target Clippy with `-D warnings`, workspace formatting and the
  scoped documentation/link checks passed.

The retained CLI operation directories are
`%TEMP%/harness-dependency-selection-native-D1eBJ0` (Codebase Memory 0.10.8),
`...-x2smEp` (Nuphus Windows x64 0.2.2), and `...-cbcsv3` (BasedPyright 1.39.10).
Each contains selection, read-only inspection, idempotent reuse, refused foreign
replacement, rollback, absent-state inspection and idle-recovery output.
Codebase Memory completed all seven probe operations with one private project;
Nuphus exposed 38 tools and its six known branch-type gaps. Both stopped their
owned trees and removed private state without cleanup retry. BasedPyright's
actual Node CLI returned the exact selected version and stopped its owned tree.
Node 24.18.1 was explicitly selected with SHA-256
`ac51903c4c111815d52280b1fdcc8da067cbb37e2fe1a765097b85c3292c8582`.
All three slots ended unselected; their package trees and operation history
remain in native owned state. Complete task 5.2 remains open.
