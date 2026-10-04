## 1. Split and verify

- [ ] 1.1 Extract the shared fixtures/helpers of `improvement_comparison.rs` into a module both new targets include without duplication.
- [ ] 1.2 Split the tests at the measured boundary, keeping every test name and body unchanged; both targets compile and each serialized run finishes under 1500 s on this machine.
- [ ] 1.3 Update `.github/workflows/windows-installed-integration.yml` and the `ci_workflow` equality contract to the new target set; run both targets end-to-end through `codex-harness heavy` and record actual durations.
