## 1. Admission semantics

- [ ] 1.1 Change the health-to-admission mapping so `SourceStale` and `SourceUnavailable` keep `runtime_allowed` true after recorded binaries and metadata verify, with action text that names explicit build/update and continued launches on the delivered build; verify the `build_identity` unit tests pin the new flags and text
- [ ] 1.2 Adjust launcher/entrypoint classification so source staleness alone no longer degrades an ordinary launch while missing, altered or metadata-incompatible inputs keep the existing degraded fallback; verify the native launcher tests cover both branches

## 2. Test contract

- [ ] 2.1 Update the source-consuming runtime gate test: a stale and an unavailable source launch the gated command successfully with unchanged stdout and empty stderr, while `check --build` still reports `source-stale`/`source-unavailable` with `runtime_allowed: true` and its existing nonzero exit
- [ ] 2.2 Confirm integrity refusals remain: altered or missing recorded binaries still refuse runtime with the corrective action, and the CodeGraph check-reporting and build-reuse tests match the new stale semantics

## 3. Documentation

- [ ] 3.1 Update the owning installation/native guides where they describe the stale-source runtime disable, stating delivered-build launch authority, the Check/diagnose report and the deploy switch for new processes; verify local links and factual consistency

## 4. Verification and delivery

- [ ] 4.1 Run the workspace checks (`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked --jobs 1 -- -D warnings`, `cargo test --workspace --locked --jobs 1 -- --test-threads=1`, `harness-source-check`) and fix failures
- [ ] 4.2 Deliver with the standard one-action deploy from the kit checkout, then with a deliberate source edit make the checkout source-ahead and verify through the installed manager that a gated runtime command still launches while Check reports the stale relationship; revert the edit afterwards
