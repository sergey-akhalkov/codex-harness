## 1. Reuse and configuration

- [x] 1.1 Restore conditional C# discovery with matching server metadata and compatible installed runtime; preserve missing/ambiguous refusals.
- [x] 1.2 Pin the adopted Roslyn host, DLL and stdio arguments through existing Serena configuration and reconcile the operating guide.

## 2. Verification and delivery

- [x] 2.1 Pass focused discovery/configuration tests and applicable Rust/OpenSpec checks.
- [ ] 2.2 Deliver through the existing lifecycle and verify real C# solution symbols, body, references and truthful explicit diagnostics.

Discovery: 12 passed, 1 pre-existing network check ignored. Serena configuration: 8 passed. Both ran through the shared heavy queue with locked Cargo dependencies. Strict all-spec validation: 46 passed. Explicit Rust diagnostics returned empty without independently established freshness and are not compilation credit. Affected-package Clippy remains blocked by the same 11 pre-existing diagnostics in unchanged `crates/harness-core/src/task_worktree.rs`, also recorded by `enable-serena-cpp`; no clean full-Clippy claim. The all-target package check also exposes unchanged `crates/codex-harness/tests/improvement_comparison_common.rs` being compiled as a standalone integration target (leading `super` paths; 134 errors). The production release build completed; broad harness-check closure is not claimed. Actual installed C# consumer validation remains open.

After integrating the client-lifetime repair and forwarding `APPDATA`, all 60 focused Serena core tests, 20 registration checks and four non-optional Serena STDIO checks pass. The five optional real-package STDIO checks were not rerun in that command; installed delivery and the C# consumer are verified separately. The isolated C# precheck resolves existing dependencies, returns method bodies and cross-file references, and reports only a style suggestion in the inspected file. Final combined managed delivery remains open.
