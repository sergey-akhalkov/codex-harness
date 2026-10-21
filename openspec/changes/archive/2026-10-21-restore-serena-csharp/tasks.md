## 1. Reuse and configuration

- [x] 1.1 Restore conditional C# discovery with matching server metadata and compatible installed runtime; preserve missing/ambiguous refusals.
- [x] 1.2 Pin the adopted Roslyn host, DLL and stdio arguments through existing Serena configuration and reconcile the operating guide.

## 2. Verification and delivery

- [x] 2.1 Pass focused discovery/configuration tests and applicable Rust/OpenSpec checks.
- [x] 2.2 Deliver through the existing lifecycle and verify real C# solution symbols, body, references and truthful explicit diagnostics. Closed by explicit owner acceptance of another installed environment; see acceptance scope below.

Historical implementation checks: discovery passed 12 tests with one pre-existing network check ignored; Serena configuration passed eight tests. Both ran through the shared heavy queue with locked Cargo dependencies. Strict all-spec validation passed 46 items. Explicit Rust diagnostics returned empty without independently established freshness and are not compilation credit. Affected-package Clippy encountered 11 pre-existing diagnostics in unchanged `crates/harness-core/src/task_worktree.rs`, also recorded by `enable-serena-cpp`; no clean full-Clippy claim. The all-target package check also exposed unchanged `crates/codex-harness/tests/improvement_comparison_common.rs` being compiled as a standalone integration target (leading `super` paths; 134 errors). The production release build completed; these historical results do not establish current broad harness-check closure.

After integrating the client-lifetime repair and forwarding `APPDATA`, all 60 focused Serena core tests, 20 registration checks and four non-optional Serena STDIO checks passed. The five optional real-package STDIO checks were not rerun in that command. The isolated C# precheck resolved existing dependencies, returned method bodies and cross-file references, and reported only a style suggestion in the inspected file.

## Acceptance scope

On 2026-10-21 the owner confirmed that C# works in another project on another machine with the harness installed, and explicitly approved closing and archiving this change. Task 2.2 is closed on that owner acceptance. This session did not independently collect that environment's per-operation receipts or establish diagnostic freshness or compilation success there.

On the current machine, the scoped code-tools installation lifecycle completed and refreshed the C# discovery row to `missing`. No reusable Roslyn server or compatible .NET 10 runtime was found in the checked installations and caches. Managed C# activation refused with `csharp has no verified reusable dependency; installation and updates are disabled during MCP sessions`. This verifies the missing-dependency refusal, not local C# semantic readiness. No new runtime or backend was installed; the reuse-only constraint remains in force.

Closure checks: both delta requirements match the main specification, `git diff --check` passed, and `openspec validate --specs --json` passed all 40 specifications. Strict bulk validation passed all active changes but rejected existing main-spec requirement descriptions longer than 500 characters. The affected main specification has the same 15 overlength requirements as its committed baseline; this change introduced none. No current all-spec strict pass is claimed.
