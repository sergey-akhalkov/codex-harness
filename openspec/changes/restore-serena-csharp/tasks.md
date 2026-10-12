## 1. Reuse and configuration

- [x] 1.1 Restore conditional C# discovery with matching server metadata and compatible installed runtime; preserve missing/ambiguous refusals.
- [x] 1.2 Pin the adopted Roslyn host, DLL and stdio arguments through existing Serena configuration and reconcile the operating guide.

## 2. Verification and delivery

- [x] 2.1 Pass focused discovery/configuration tests and applicable Rust/OpenSpec checks.
- [x] 2.2 Deliver through the existing lifecycle and verify real C# solution symbols, body, references and truthful explicit diagnostics.

Discovery: 12 passed, 1 pre-existing network check ignored. Serena configuration: 8 passed. Both ran through the shared heavy queue with locked Cargo dependencies. Strict all-spec validation: 46 passed. Explicit Rust diagnostics returned empty without independently established freshness and are not compilation credit. Affected-package Clippy remains blocked by the same 11 pre-existing diagnostics in unchanged `crates/harness-core/src/task_worktree.rs`, also recorded by `enable-serena-cpp`; no clean full-Clippy claim. The all-target package check also exposes unchanged `crates/codex-harness/tests/improvement_comparison_common.rs` being compiled as a standalone integration target (leading `super` paths; 134 errors). The production release build completed; broad harness-check closure is not claimed. Actual installed C# consumer validation remains open.

After integrating the client-lifetime repair and forwarding `APPDATA`, all 60 focused Serena core tests, 20 registration checks and four non-optional Serena STDIO checks pass. The five optional real-package STDIO checks were not rerun in that command; installed delivery and the C# consumer are verified separately. The combined immutable deployment completed with executor and managed semantic preflight checks. A fresh installed managed connection then opened the real C# solution, returned its symbol overview, method body and five cross-file references; explicit diagnostics for that file reported only IDE0305 information and no missing dependency/type errors. This verifies the inspected consumer, not a clean whole-solution analysis. Owned registrations now use the client-lifetime fix; existing client processes require reconnection. No package download or consumer binary rebuild was required.
