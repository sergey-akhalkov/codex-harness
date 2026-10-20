## 1. Managed C++ support

- [x] 1.1 Restore reuse-only clangd discovery and canonical C++ project admission; verify native discovery, missing/ambiguous installation, planning and admission tests.
- [x] 1.2 Extend existing managed semantic acceptance with C++ symbols, cross-file references and disposable semantic edits; pass actual backend acceptance.
- [x] 1.3 Deliver the immutable manager and scoped code-tools configuration; verify the installed registry and actual consuming C++ project with its compilation database, then reconcile the guide and strict OpenSpec checks.

## Verification scope

The focused native checks passed: 11 discovery integration cases, 94 dependency
cases, 7 Serena configuration cases and 8 code-tools lifecycle cases. The
installed `mcp serena-check` passed for Rust, Python, Pascal and C++, including
the C++ cross-file reference and disposable semantic edit. Deployment reported
`deployed` and `executor_probe_ok: true`; scoped installation adopted clangd.

A real consuming C++ project additionally passed managed declaration/body and
cross-file reference queries with explicit system include paths. Explicit
diagnostics were empty after semantic translation-unit queries; the same two
translation units compiled with the actual MSVC toolchain. An immediate empty
diagnostic result alone was not accepted as readiness. Project inputs and
machine-local receipts remain with that consumer.

Strict all-spec validation passed (45 items) and changed Rust files passed
format checking. Full affected-package Clippy remains blocked by 11 diagnostics
in unchanged `crates/harness-core/src/task_worktree.rs` (needless borrows and
collapsible conditionals); no clean full-Clippy claim is made.
