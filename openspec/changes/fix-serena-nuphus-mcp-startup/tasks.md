## 1. Serving admission

- [x] 1.1 Admit `mcp serena` and `mcp nuphus` through serving admission while leaving `mcp codebase-memory` on source-runtime admission. Verify a recorded source-stale manager with matching hashes serves `mcp serena --help` and `mcp nuphus --help` and still refuses `mcp codebase-memory`.
- [x] 1.2 Extend the native source-stale MCP test so Serena and Nuphus `--help` succeed on a source-stale manager while `mcp codebase-memory` stays refused. Verify `cargo test --locked -p codex-harness --test mcp_cli --jobs 1 -- --test-threads=1`.

## 2. Notes and live command

- [x] 2.1 Update the CodeGraph serving-admission restart notes so Serena and Nuphus share that boundary: later source edits do not stop hash-matching adapters, and native adapter changes still need explicit Install/Update plus session restart. Verify the owning docs name all three retained servers without private paths.
- [x] 2.2 Rebuild and re-register the installed native command from current source, then verify a framed initialize against the registered Serena and Nuphus commands no longer exits with source-consuming runtime disabled.

## 3. Serena response identity

- [x] 3.1 Answer every Serena stdio response, including the cached shared-worker initialize result, with the id of the client request that opened it. Verify with a framed probe against the real entry point that `initialize` with id 0 returns id 0 and the next request returns its own id.
- [x] 3.2 Cover the id rewrite with a focused proxy test and keep the client-facing error envelope on the same id.
