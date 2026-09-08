# Native OpenCodex boundary

2026-09-08. Migration task 3.1 is unfinished. The first verified replacement
is [candidate validation](../../crates/harness-core/src/opencodex.rs), using the
installed upstream CLI's `config validate <path> --json`. It does not import
internal TypeScript through generated code. Package metadata must identify
`@bitkyc08/opencodex` 2.44.0; this check alone does not certify package integrity.
The native dependency lifecycle still owns provenance verification.

Every call creates fresh owned Codex/OpenCodex homes, closes stdin, disables
the audited `OPENCODEX_CODEX_SHIM_AUTO_RESTORE` preflight and passes candidate
bytes through an owned file. The 512 MiB/25% CPU Job and caller deadline bound
the foreign process. Output beyond 1 MiB causes a distinct failure; raw output
stays in private temporary evidence. Public receipts contain status and process
evidence, not provider responses or candidate values. Full routing-policy
validation remains separate from the upstream schema result.

The [native acceptance test](../../crates/harness-core/tests/opencodex.rs) was
explicitly run against the installed package selected by the live installation
receipt, Rust/Cargo 1.97.1, Windows x64. From the repository root:

```text
# HARNESS_OPENCODEX_PACKAGE must identify the explicitly selected adopted package.
cargo test --locked --jobs 1 -p harness-core --test opencodex -- --ignored --test-threads=1 --nocapture
cargo clippy --locked --jobs 1 -p harness-core --lib --test opencodex -- -D warnings
```

One test covering valid input, schema rejection, cancellation and an expired
deadline passed in 0.60 s. Clippy passed. Valid/rejected evidence is under
`%TEMP%/harness-ocx-validate-sccI3k` and `harness-ocx-validate-qvyUFW`.
The earlier rejected attempt, `harness-ocx-validate-SHa718`, records the actual
upstream behavior: process exit 0 with JSON `ok:false`. The Rust boundary now
treats that as rejection. Exit 0 alone cannot certify valid configuration.
The test also checks source preservation and value-free public failure output;
it does not claim exhaustive observation of every foreign filesystem path.

The installed public OAuth CLI still supplies its manual-code callback and
opens a browser from the child. It cannot yet replace the browser-only helper's
closed-input and browser-lifetime protections. Public `restore` also has wider
desired-state/history effects than the current `restoreNativeCodex(skipHistory)`
boundary. Native equivalents and successful owned login/restoration acceptance
remain open. No live subscription service, credentials or command registration
was changed, and no model request was made by this validation test.
