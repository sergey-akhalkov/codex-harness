## Context

Today one heavy-command account admits exactly one tree: `resource_admission::Lease` takes an exclusive byte lock on `heavy-command.lock`, and the admitted tree runs in one containment Job with the per-tree policy (default 8 GiB commit memory, 30-minute deadline, bounded queue wait). `heavy_command.rs` documents the lock order "account slot first, shared CPU budget second" and nested-heavy inheritance. After `add-shared-agent-cpu-budget`, CPU is enforced by one account-wide shared Job regardless of how many trees run, so serialization now only costs latency. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**
- Bounded concurrent admission per account with mechanical queue semantics and holder diagnostics.
- One kernel-enforced aggregate commit-memory envelope per account covering every admitted tree, without a background owner process.
- Policy/reporting surface for slot count and aggregate memory; legacy policies keep working unchanged; slot count 1 restores today's behavior exactly.
- Preserve per-tree containment, deadline, cancellation, cleanup, nested inheritance, `--account` isolation, exit codes and stdout protocols.

**Non-Goals:**
- No change to the shared CPU budget, its policy or its reporting.
- No per-checkout or per-repository scheduling: the account is the only admission domain.
- No strict FIFO ordering guarantee, priority classes or preemption; the queue stays a bounded-wait polling admission like today.
- No distributed or cross-machine coordination.

## Decisions

1. **Slot-set admission instead of one exclusive lease.** Replace the single `HeavyCommand` lease with `max_concurrent_trees` per-slot lock files (`heavy-command.slot-0.lock` ... ) acquired by try-lock-any under the same 20 ms poll/queue-wait deadline loop. The legacy `heavy-command.lock` name is not reused for admission, so a mixed-version machine cannot silently interleave old and new admission; the documented holder-description records move to per-slot files. Alternative rejected: a counting state file with a coordinator - needs a second lock and stale-holder repair, more states for no benefit at this bound.
2. **Slot count from policy, default 2.** `budget.json` gains `max_concurrent_trees` (positive integer; 1 = legacy serialization) and `aggregate_memory_limit_bytes`. Defaults: 2 slots; aggregate memory equal to the existing per-tree default (8 GiB), so the account's total envelope is unchanged from a serialized account. Missing fields mean defaults; the file is never rewritten by reads. Alternative rejected: per-checkout slots - checkout identity is not a kernel resource, path/worktree aliasing makes it unreliable, and it does not bound whole-machine impact; account slots do.
3. **Named account aggregate Job as the memory envelope.** Each caller creates-or-opens a named Job (`CodingAgentsHarness.HeavyAggregate.<account-hash>`) with `JOB_OBJECT_LIMIT_PROCESS_MEMORY` = aggregate limit and no CPU rate, joins its payload tree to it, and holds the handle for the command lifetime; the object disappears with the last handle, mirroring the existing shared-CPU owner pattern in `process.rs`. The per-tree containment Job stays inner (nested jobs), preserving memory, deadline, kill-on-close and snapshot reporting. Alternative rejected: splitting the aggregate into per-tree reservations at admission - either shrinks a lone tree's envelope or needs dynamic re-limits across live jobs; the shared named Job gives the kernel one honest aggregate and a lone tree the full envelope.
4. **Admission order preserved.** Order stays: slot admission -> aggregate Job create/join -> shared CPU budget -> payload creation (outermost Job first), keeping every existing deadlock argument intact; a caller waiting for a slot holds no Job handles.
5. **Diagnostics and reporting.** The waiting caller prints one bounded line ("waiting for a free heavy-command slot; k/N busy; holders ...") from per-slot holder descriptions. `heavy budget` text and JSON add slot count, aggregate memory, per-tree memory and field sources; `check`-surface heavy reporting is extended only if it already prints heavy budget fields.

## Risks / Trade-offs

- [Two concurrent trees can contend for the same memory envelope and fail allocation where serialization previously waited] -> Documented consequence of concurrency; the failure stays inside the Job, diagnostics name the aggregate limit, and slot count 1 restores waiting behavior.
- [Slot polling is not strictly FIFO; a later caller can grab a freed slot first] -> Same ordering property as today's queue; recorded as a limitation, acceptable at the default bound of 2.
- [Mixed old/new binaries on one machine could admit through different mechanisms] -> New slot files are separate names; an old binary still serializes on the legacy lock while a new binary ignores it. Release note documents that all heavy callers should move to one build during rollout.
- [Aggregate Job lifetime depends on caller handles] -> Same lifetime model as the shared CPU budget: last handle out closes it; orphan windows only affect reporting, never enforcement, and 4.3-style lifecycle tests cover owner death.

## Migration Plan

Ship behind policy defaults in one release: new fields optional, default slot count 2, aggregate memory equal to the previous per-tree default. Rollback is setting `max_concurrent_trees` to 1 (or deploying the previous build); no state migration is required because admission state is transient locks and kernel objects.