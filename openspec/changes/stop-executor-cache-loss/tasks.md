## 1. Detection and interruption

- [x] 1.1 Implement exact-session incremental usage monitoring and the warmed-cache detector; verify cold starts, recovery, duplicates, malformed counters, foreign sessions and historical resume records with deterministic Rust checks.
- [x] 1.2 Integrate automatic stopping into control and observed launcher hosts; verify termination, retained work, non-success receipt/watch results and explicit coverage through owned fixtures.
- [x] 1.3 Implement fresh-session restart in the preserved slot with the recorded original assignment and bounded visible handoff; verify stale-identity refusal and preservation of partial work and commits.

## 2. Delivery

- [x] 2.1 Document the policy and its limits; run affected native tests, formatting, diagnostics, clippy and source hygiene checks. The affected suites passed 183 checks, and four opt-in checks against native Codex with a local canned provider passed. Clippy passed with only the existing `chunks_exact_to_as_chunks` lint excluded; unmodified core files fail that new toolchain lint. Compiler/clippy results supply fresh diagnostics; Serena diagnostics were unavailable.
- [x] 2.2 Deploy through the existing lifecycle and exercise the installed host outside the checkout with synthetic usage; retain the build identity and actual check results without paid calls. Build `99dea250b5c494da` deployed with `executor_probe_ok=true`; both installed-manager cache-loss checks passed outside the checkout with four local requests each and no paid provider calls.
