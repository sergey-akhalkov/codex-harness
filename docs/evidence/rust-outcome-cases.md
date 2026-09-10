# Native controlled outcome preparation

On 2026-09-09, the Rust `outcome-prepare` command prepared the five controlled
cases `freshness`, `entrypoint`, `process`, `missing` and `negative` through the
actual manager executable. Each invocation creates a new owned temporary case
outside this checkout, records immutable input and initial Markdown hashes, and
returns its prompt and private preparation receipt. Existing workspaces are never
adopted. Preparation runs no models, Cargo, network acquisition or global changes.

```powershell
codex-harness.exe outcome-prepare --case freshness
codex-harness.exe outcome-prepare --case process --observer C:/absolute/path/harness-observe.exe
```

The [preparer](../../crates/codex-harness/src/outcome_prepare.rs) copies the current
manager into `case.exe`; the existing manager and test fixture share the
[controlled target implementation](../../crates/codex-harness/src/outcome_case_fixture.rs).
This adds no sixth deployment artifact. The process case also copies an explicitly
selected observer. Copies require ordinary bounded executable files, exclude a
concurrent writer, preserve existing destinations, and verify copied bytes against
the held source. Their hashes identify inputs; they are not an authenticity claim.

Build/CLI cases start with source version 2 and generated version 1. Actual target
invocations append an execution audit and resolve files relative to the copied
executable, including when the caller uses a different working directory. Process
targets retain full concurrent streams, natural exit 7, absent readiness and a
delayed descendant. Instructions distinguish natural process status from a forced
termination code. The missing-tool and narrow documentation cases preserve their
original scope. Preparation labels this revision `controlled-v3-rust`; comparisons
must not silently mix it with historical `controlled-v2` fixtures.

Seven integration tests passed: two real controlled-target tests and five preparer
tests covering all cases, fresh independent inputs, immutable hashes, actual copied
manager build/CLI execution, actual copied observer execution, invalid/external
case refusal, and bounded source input without altering prior generated output.
Two copy-boundary unit tests passed, including active writers, links, oversized
input and existing destinations. Clippy passed for all CLI targets; explicit Serena
diagnostics for the preparer were empty. Logs are retained under
`%LOCALAPPDATA%/codex-harness-evidence/outcome-cases-1ca2b9b6d02e4e5c95937965508ac048/`
in `integration-final.*`, `unit.*` and `clippy.*`. Earlier guarded integration logs
remain separate. [Target process acceptance](rust-outcome-run.md#controlled-outcome-case-targets)
records the additional native observer scenarios.

```text
cargo test -p codex-harness --test outcome_case_fixture --test outcome_prepare --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo clippy -p codex-harness --all-targets --offline --locked --jobs 1 --target-dir <owned-check-target> -- -D warnings
```

This is preparation acceptance, not a completed evaluation. The later
[controlled oracle acceptance](rust-outcome-oracle.md) covers independent result
checks; frozen external consumer inputs and suite integration remain unfinished.
External cases `focused`, `second` and `reduction` are explicitly refused here.
No new opencode-kit evaluation ran. Task 7.3 and global native migration remain open.
