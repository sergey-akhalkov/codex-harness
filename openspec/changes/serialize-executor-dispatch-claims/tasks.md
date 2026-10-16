## 1. Exclusive startup through the existing pool

> **Reconciliation (2026-10-14, candidate `2a62908`)**: the exclusive startup ownership this section specifies is delivered by the common pool owner - `task_worktree`'s admission guard plus this process's startup reservation, held until the host lease, consumed by `executor_cli` acquisition - and is integrated under `unify-reusable-worktree-pool` task 2.2 together with its deterministic gate checks and the retained failing pre-correction control (`RED_CONTROL_OK`). This change therefore adds no separate implementation; its remaining scope is section 2's independent acceptance, whose frozen checker, unchanged/corrected/interrupted controls and parent accounting have not run. No task here is closed by this reconciliation, and the requirement delta stays out of the main specs until that experiment is accepted.

- [ ] 1.1 Reproduce the competing claim-to-host window with controlled model-free native processes; retain exact assignment/receipt identities and a failing unchanged result.
- [ ] 1.2 Preserve exclusive ownership through host handoff using existing pool/process facilities; verify simultaneous different-owner and same-owner dispatches cannot overwrite a pending claim or assignment and cannot exceed pool capacity.
- [ ] 1.3 Preserve bounded contention, failure cause and exact-owner recovery before/after handoff; exercise a creator exit, live/uncertain host identity and unreviewed work without deleting foreign state or serializing complete model conversations.

## 2. Independent acceptance and retained delivery

- [ ] 2.1 Update the existing pool guidance, pass formatting, scoped clippy, native build and the affected pool/observation regressions; verify source hygiene and affected local links.
- [ ] 2.2 Pass the frozen independent native checker against the actual solution, including unchanged, corrected and interrupted-startup controls; retain all preparation, verification, waiting and failed-work costs for the parent experiment.
- [ ] 2.3 Preserve exact candidate commits, useful task artifacts and recovery inputs for the parent benefit/integration/publication gates; keep an unadopted delta separate from accepted main specs.
