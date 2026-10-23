## Context

The normal user action is concurrent independent dispatch with distinct
checkout, assignment and conversation identities. The historical failure window
is now guarded by startup reservations and atomic admission. Reuse the current
`task_worktree` and `executor_cli::acquire_pool_slot` owners and existing tests;
establish any remaining defect through a controlled native reproduction.

The shared lifecycle in `unify-reusable-worktree-pool` owns allocation and base
selection. An explicit available local base needs no fetch, and continuation of
a held task preserves HEAD and partial work. This change adds the continuous
ownership invariant without replacing that pool contract or reviving owner-id
reset semantics. Integrate the deltas in either order without losing either
contract, and keep the prerequisite's unfinished acceptance visible.

## Goals / Non-Goals

- Keep slot ownership exclusive during allocation, synchronization, receipt
  publication and handoff to the process lease.
- Preserve exact owner/run identity, useful work, bounded startup and recovery.
- Allow configured executor concurrency once startup has completed.
- Do not introduce a second allocator, recurring scheduler, hidden model process,
  fixed service lifetime or a new general transaction framework.

## Decisions

Use the existing pool and native process/locking facilities to protect the
allocation-to-handoff interval. A narrowly scoped per-pool startup guard or an
equivalent process-owned reservation can supply the missing exclusion. Select
the smallest mechanism after reproducing the actual overlap and checking its
failure behavior; a slot-record create alone is insufficient while another
caller can reclaim it before the lease exists. Do not hold exclusion for a
model conversation's lifetime. Report a bounded wait/refusal instead of treating
startup contention as permission to overwrite or allocate beyond the pool.

All paths that can reclaim or replace the relevant claim must respect the same
ownership boundary. Startup failure must retain the original cause and release
only its own unused reservation. Loss of the creating process may permit
recovery after its exact liveness is resolved; uncertain ownership preserves
the claim. Once a host is live, its existing process lease remains authoritative.
Reusing an owner identifier must not allow two live generations to share a slot.

The realistic counterexample is a slow remote fetch or host attach: exclusion
held unnecessarily broadly can delay otherwise independent startup. Keep the
critical interval explicit, preserve existing bounds, measure that added delay
and avoid claiming a throughput benefit from a correctness fix alone. Parallel
model work after handoff must remain available at the configured capacity.

## Acceptance

Before comparison, freeze the exact historical source exhibiting the gap, the
current candidate, checker, startup bounds and synchronization gates. Locate and
qualify the historical control from the reservation change's history; a commit
label alone is not a failing result. An already-correct current HEAD is a valid
candidate, not an assumed negative control. If neither applicable historical
evidence nor a reproducible gap is available, retain the acceptance limitation
and make no new corrective implementation on the strength of the old narrative.

Measure lost/overwritten assignments, false accepted receipts, duplicate live
bindings and out-of-capacity allocations. The historical control must exhibit
the selected violation and the candidate must produce zero violations across
the declared schedules below. Retain startup wait times and enforce the frozen
existing bounds, without inferring speed from a correctness improvement.
Reject additional candidate changes that fail retained behavior; restore the
accepted source/runtime through the existing owners and preserve the evidence.

Reproduce the overlap with controlled native processes against an owned source,
home and pool, without a model request or live installation mutation. Gate the
first dispatch after its claim and before host publication, start a competing
dispatch, then release the first gate. Verify unique bindings and unchanged
assignment/run identities through both observed terminal outcomes. With one
slot, the contender must wait boundedly or refuse; with available capacity,
successful conversations must have distinct slots. Neither receipt may report
acceptance for an assignment that never reaches its corresponding host.

Cover same-owner overlap, failure before handoff, a creator exiting during
startup, a live host, uncertain process identity and preserved unreviewed work.
Use synchronization gates rather than timing-only sleeps to make the original
race reproducible. Keep original errors inspectable and prove recovery through
the native path, not a manufactured success receipt. Run affected pool and
observation checks, scoped clippy, formatting and a native build.

The parent experiment must freeze independent acceptance outside both solution
write scopes before measuring this task, including a failing unchanged control
and a passing corrected control. Record preparation, checks, waiting, failures
and recovery once in the parent accounting. If the candidate watch change is
not actually consumed on this workload, its broader usefulness remains
inconclusive. Each task's two arms are compared with each other, not with the
duration of a different task.

## Delivery and Recovery

Retain candidate commits and evidence under their existing owners. Update the
existing pool guidance with observed startup contention/recovery behavior.
Only supported benefit and applicable integration/publication authority may
advance an experimental baseline or live installation. Rejection retains useful
artifacts without synchronizing the unadopted requirement delta into main specs.

Reconciled 2026-10-23: the existing pool admission guard and process startup
reservation held until the host lease already implement this design through
`unify-reusable-worktree-pool` task 2.2. Independent acceptance now reproduces
claim replacement on pre-reservation `c39b2aa` and passes on product `bcfc3c2`,
using one frozen checker with a post-claim reset gate. The gate observes the
contender before release and accepts historical records without a startup field;
absence alone is not a violation. See tasks.md for the checks and their limits.
The parent experiment's comparative benefit and adoption gates remain pending.
This workload's completed acceptance does not adopt the requirement delta or
authorize publication of an experimental baseline.
