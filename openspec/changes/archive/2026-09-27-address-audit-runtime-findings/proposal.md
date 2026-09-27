## Why

The external audit identifies three reproducible source-level gaps still present in the current checkout: normal Cargo verification bypasses RTK compression, independent Serena projects wait behind a pool-wide lock, and the feedback ledger can label a contradictory benefit record as adopted. Correct these bounded behaviors before investing in unmeasured instruction, verification-cache or agent-learning infrastructure.

## What Changes

- Extend the existing native RTK adapter to cover the Cargo test/build/check/clippy forms used by the kit, including resource-limited execution, with explicit compression decisions, preserved diagnostics, original exit status and bounded raw recovery. Keep exact diffs and machine output raw.
- Let independent Serena workers execute concurrently while retaining serialized access to each worker and client routing state, bounded admission, safe eviction, cancellation and existing process ownership.
- Expose bounded pool evidence through the existing local status surface so capacity decisions use observed queueing, cold starts and evictions. Preserve current worker and executor limits.
- Make the feedback ledger distinguish a recorded decision from a consistent, sufficiently populated comparison; contradictory or incomplete adoption records cannot appear as proven defaults.
- Preserve a recommendation-by-recommendation assessment in `design.md`, including counterexamples, evidence limitations and conditions for revisiting ideas excluded from implementation.

## Capabilities

### New Capabilities

None. Extend the existing owners.

### Modified Capabilities

- `token-efficient-agent-workflow`: compact real Cargo verification through the accepted native boundary with both output streams accounted for and observable bypass reasons.
- `global-code-tools`: concurrent requests across independent Serena workers, bounded safe pool admission and useful resource evidence.
- `agent-delegation`: truthful interpretation and presentation of benefit comparison records.

## Impact

Expected implementation owners are `tools/rtk-adapter/src/main.rs`, its integration tests, the Serena broker/shared-pool modules and tests, and the benefit-record reader plus feedback ledger. Update their existing guides and command recipes, and deliver through the current global lifecycle with acceptance outside the checkout.

No new runtime dependency, orchestrator, graph service, model route, automatic verification-pass cache or context-rewriting service is proposed. Paid model experiments, instruction-policy rewrites and larger resource limits are outside this change. The audit's priority labels are optimization ordering, not evidence of a critical production incident; no subscription-saving percentage is claimed.
