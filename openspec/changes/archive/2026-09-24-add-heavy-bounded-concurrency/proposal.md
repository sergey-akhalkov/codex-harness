## Why

The account-wide heavy-command queue admits exactly one batch tree at a time. With the shared agent CPU budget now enforcing one aggregate CPU ceiling for all local agent work, strict serialization no longer protects the machine: it only makes independent checkouts and executors wait for each other while CPU headroom goes unused.

## What Changes

- Allow a bounded number of concurrent heavy-command trees per heavy-command account instead of one, with a configurable slot count (`1` restores today's serialized behavior).
- Keep one aggregate memory envelope per account: all admitted trees are members of one account aggregate Job whose commit-memory limit is `JOB_OBJECT_LIMIT_JOB_MEMORY` (`JobMemoryLimit`), not a per-process cap. Each tree keeps its own containment Job with per-tree memory, deadline, cancellation and cleanup.
- Keep the legacy `heavy-command.lock` as the shared admission gate. New callers hold a shared lock on that same file for the slot lifetime, so an older exclusive lock and a current admission cannot run together and cannot bypass the bound through different lock-file names.
- Preserve queue semantics: bounded wait, observable waiting/execution/exit/failure diagnostics, holder reporting, nested-heavy admission without double slot use, and release on normal exit, failure or interruption.
- Extend the machine-local heavy-command policy and `heavy budget` reporting with the concurrency and aggregate-memory settings; keep per-account isolation through `--account`.
- CPU behavior is unchanged: admitted trees remain members of the existing shared account CPU budget; concurrency never multiplies the CPU allowance.
- Update the native/installation documentation and the project decision log; no new external dependency.

## Capabilities

### New Capabilities

- `heavy-command-concurrency`: bounded concurrent admission for heavy-command trees, the account aggregate memory envelope, slot policy/reporting, and installed acceptance of concurrent batch work.

### Modified Capabilities

- `lead-agent-orchestration`: the shared heavy-command admission requirement changes from strict serialization under one slot to bounded concurrency under one aggregate memory envelope and one aggregate CPU ceiling.

## Impact

- `crates/harness-core/src/resource_admission.rs`: account admission grows from one exclusive lease to a bounded slot set that still shares `heavy-command.lock`, with queue diagnostics.
- `crates/harness-core/src/heavy_command.rs` and `crates/harness-core/src/process.rs`: account aggregate Job creation/membership and snapshot reporting, using the existing job-memory flag and the documented admission/lock order.
- `crates/codex-harness/src/heavy_command_cli.rs` and its tests: policy fields, `heavy budget` output, queue diagnostics and concurrency fixtures.
- `docs/rust-native.md`, `docs/installation.md`, `docs/project-decisions.md`: heavy-command budget section, policy defaults and the retained aggregate guarantees.
- No breaking CLI change: existing arguments, policy files without the new fields, exit codes and stdout protocols keep working; behavior changes only through the new bounded default.
