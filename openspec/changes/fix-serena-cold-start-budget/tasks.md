## 1. Client and lifecycle budget

- [x] 1.1 Replace the three fixed 30-second caps in `crates/harness-core/src/serena_broker.rs` with the shared `CONTROL` budget (240 seconds), documented at the constant.
- [x] 1.2 Raise explicit `broker-retire` observation in `crates/codex-harness/src/mcp_cli.rs` from 8 to 60 seconds.

## 2. Checks

- [x] 2.1 `cargo fmt --all`; `cargo clippy --locked -p harness-core -p codex-harness --all-targets -- -D warnings`.
- [x] 2.2 `cargo test --locked -p harness-core --test serena --jobs 1 -- --test-threads=1`.
- [x] 2.3 `HARNESS_CODE_TOOLS_REGISTRY=<registry> cargo test --locked -p codex-harness --test serena_stdio grown_location_record_still_completes_handshake_after_deliveries -- --ignored --exact --test-threads=1` completes MCP initialize and tools/list against the adopted Serena package.
- [x] 2.4 End-to-end cold-start probe through `codex-harness mcp serena` stdio with no broker pre-running: initialize, tools/list and an `activate_project` call all complete within the request budget; a warm follow-up call completes in seconds.

## 3. Delivery

- [x] 3.1 `openspec validate fix-serena-cold-start-budget --strict --no-interactive` and `git diff --check`.
- [x] 3.2 Deliver the fixed launcher through the immutable deploy lifecycle from a source snapshot that does not include unrelated in-flight edits, and re-verify the cold-start probe outside this checkout.
