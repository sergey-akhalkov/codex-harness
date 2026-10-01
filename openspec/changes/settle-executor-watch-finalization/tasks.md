## 1. Finalized completion through the native watch path

- [ ] 1.1 Make `executor watch` wait for the current run's finalized successful outcome and retained result; verify a controlled completion-before-finalization transition through the real CLI in text and JSON modes.
- [ ] 1.2 Preserve bounded timeout, original failures, finalized output defects, unresolved reply holds and exact-run correlation; run the affected native observation regressions and verify an incomplete finalization never returns success or stops/replays the executor.

## 2. Independent acceptance and delivery preparation

- [ ] 2.1 Update the existing watch guidance to describe pending finalization and its result/timeout behavior; verify source hygiene and the affected local links without adding another lifecycle guide.
- [ ] 2.2 Build the native CLI, pass formatting and scoped clippy, and run the supplied frozen independent checker against the actual executable; retain successful and failing counterexample outputs so an unchanged, skipped or forged-success solution cannot pass.
- [ ] 2.3 Inspect the candidate's delivery plan through `deploy --preview` using owned isolated installation inputs, and retain the actual preparation/check cost and preview outcome; preserve experimental artifacts for the parent benefit/integration/publication gates, without treating a preview as applied global delivery or synchronizing an unadopted delta.
