## 1. Reuse and configuration

- [x] 1.1 Restore conditional C# discovery with matching server metadata and compatible installed runtime; preserve missing/ambiguous refusals.
- [x] 1.2 Pin the adopted Roslyn host, DLL and stdio arguments through existing Serena configuration and reconcile the operating guide.

## 2. Verification and delivery

- [ ] 2.1 Pass focused discovery/configuration tests and applicable Rust/OpenSpec checks.
- [ ] 2.2 Deliver through the existing lifecycle and verify real C# solution symbols, body, references and truthful explicit diagnostics.

Discovery: 12 passed, 1 pre-existing network check ignored. Serena configuration: 8 passed. Both ran through the shared heavy queue with locked Cargo dependencies. Strict all-spec validation: 46 passed. Explicit Rust diagnostics returned empty without independently established freshness and are not compilation credit. Affected-package Clippy remains blocked by the same 11 pre-existing diagnostics in unchanged `crates/harness-core/src/task_worktree.rs`, also recorded by `enable-serena-cpp`; no clean full-Clippy claim. Actual installed C# consumer validation remains open.