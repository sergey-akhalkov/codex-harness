## 1. Finalized completion through the native watch path

- [x] 1.1 Reconcile the existing `e2395f5` implementation and regression against the finalized-outcome contract; freeze an applicable historical failing control and current executable, verify the controlled transition in text/JSON, and change code only for a demonstrated remaining gap. Record premature-success counts and retained result identities for both controls.
- [x] 1.2 Preserve bounded timeout, original failures, finalized output defects, unresolved reply holds and exact-run correlation; run the affected native observation regressions and verify an incomplete finalization never returns success or stops/replays the executor.

## 2. Independent acceptance and delivery preparation

- [x] 2.1 Update the existing watch guidance to describe pending finalization and its result/timeout behavior; verify source hygiene and the affected local links without adding another lifecycle guide.
- [x] 2.2 Build the native CLI, pass formatting and scoped clippy, and run the frozen independent checker against the actual executable; retain the historical failing control and corrected, skipped and forged-success results. Do not require already-correct current source to fail; identify the exact historical baseline. Retain valid prior checks and leave unexecuted acceptance open.
- [x] 2.3 Inspect the candidate's delivery plan through `deploy --preview` using owned isolated installation inputs, and retain the actual preparation/check cost and preview outcome; preserve experimental artifacts for the parent benefit/integration/publication gates, without treating a preview as applied global delivery or synchronizing an unadopted delta.

## Acceptance evidence (2026-10-23)

No source change was needed: `e2395f5` already gates a successful watch return on
the recorded host outcome (`completion_awaiting_host`, `settled_watch_outcome`)
and keeps observing the same run while finalization is pending. The frozen
`watch-oracle` ran through the real `executor watch` entry point against
controlled receipts.

- Candidate (`b71f84d` build, executable sha256 `dae8e3a7…`): 11/11 cases passed
  in text and JSON, including the finalization-during-watch transition, the
  declared timeout with an unchanged receipt and live host, and the
  finalized-without-result defect. Premature successes: 0. Retained result
  identity: `WATCH_ORACLE_FINALIZED / remaining: none / checks: controlled
  native finalization`.
- Historical control (`c39b2aa`, the parent of `e2395f5`; executable sha256
  `8919c77d…`): 6/11 cases failed, with two immediate premature successes
  (`Some(0)` at 0.0 s) in the pending-finalization text/JSON cases; the
  during-watch cases returned without the finalized result and the
  finalized-without-result cases returned success instead of exit 1.
- Native observation regressions: `executor_observation` 49 passed, 0 failed,
  4 opt-in ignored. The checker's own skipped-build and forged-success cases
  pass; its corrected-specimen case is covered by the candidate run above.
- Build and formatting passed. Scoped `cargo clippy -p codex-harness
  --all-targets` completes with exit 0 (29 warnings, including two pre-existing
  `clippy::question_mark` hits in `crates/harness-core/src/improvement_runtime.rs`);
  the stricter `-D warnings` invocation exits 101 on those base lints. That
  baseline gap is outside this change and retained as a limitation, not
  repaired here.
- Delivery preview: isolated codex/user/state inputs, `delivery-fast` profile,
  `status=preview`, prepared build, healthy build check, 34 planned links,
  130.5 s preparation. Preview is not applied delivery.
- `harness-source-check` reports five pre-existing missing local links in an
  unrelated archived change; the watch guidance's local links all resolve.
  `openspec validate --strict` passes.

Private evidence and command logs are retained under
`.beads/verification/settle-executor-watch-finalization/`.
