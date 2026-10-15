# Tasks

## 1. Correct Pascal startup

- [x] 1.1 Add a native managed regression for ignored build copies and working Pascal navigation; demonstrate the eager-load failure on the baseline. `pascal_startup_skips_eager_build_copy_index_and_keeps_navigation` fails on the baseline's ignored-copy load and passes with the candidate through the real adopted backend.
- [x] 1.2 Configure the adopted backend through its existing initialization-options seam, document its scope and verify configuration plus real-backend acceptance, including symbol edits and readback. Configuration tests and both real-backend tests pass after integration with the current upstream tree.

## 2. Integrate and deliver

- [x] 2.1 Complete applicable native checks, source/ownership checks and explicit diagnostics; retain exact failures or coverage limits. Formatting, source/ownership checks, CLI library/binaries, ordinary Serena STDIO tests and real semantic acceptance pass. Explicit diagnostics returned no findings after recompilation, without claiming independent LSP completeness. The broad run passed all 228 harness-core integration tests but retained unrelated failures in two model-routing assertions and one token-audit cache assertion; warning-as-error Clippy retained 11 findings in unchanged task_worktree.rs under Rust 1.99.0. Detailed local logs preserve these limits; this change does not claim a fully green workspace.
- [x] 2.2 Deploy using the existing lifecycle and verify the installed backend on the affected local project without changing its source or compiler configuration. Native deployment reports `deployed`, the installed executable matches the semantic verifier's hash, and both lifecycle semantic checks pass. The affected project's installed MCP returns real classes/functions without OOM or loading build copies; source/configuration fingerprints are unchanged. A separate existing CodeTools syntax limitation on another unit remains explicitly unverified as semantic support, not confused with startup failure.

## Workflow follow-up

- Archive the completed change, commit the scoped fix and push it to Git.
