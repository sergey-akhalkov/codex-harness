## 1. Remove the transport size caps

- [x] 1.1 Remove the one-mebibyte message/frame/write-buffer caps and the send-side record refusal from `crates/harness-core/src/task_control.rs`; keep all timeout bounds. Verify with `cargo check -p harness-core -p codex-harness --locked`.
- [x] 1.2 Remove the same caps from the relay's WebSocket configuration in `crates/codex-harness/src/executor_warning.rs`.
- [x] 1.3 Remove the oversized-final-message-read fallback, its defect wording and the `transport_limit_exceeded` classifier from `crates/codex-harness/src/executor_cli.rs`, and update the executor help text. Verify with `cargo clippy -p harness-core -p codex-harness --all-targets --locked -- -D warnings`.

## 2. Regressions and fixtures

- [x] 2.1 Add an exact-session resume regression whose thread state exceeds one mebibyte to `crates/codex-harness/tests/executor_control.rs`, asserting the assignment is submitted on the resumed thread and the run completes without `Message too long`.
- [x] 2.2 Rework the two oversized thread-read tests for the uncapped transport: a above-one-mebibyte read records its own final message and completes; a read without a message is the missing-message output defect, with no transport-limit wording.
- [x] 2.3 Forward an above-one-mebibyte record through the relay byte-preserved in both directions and deliver an above-one-mebibyte degraded diagnostic natively in `crates/codex-harness/tests/executor_warning.rs`; raise the canned double's own sanity bound to 16 MiB. Known limit recorded here: the relay's genuine socket-write-failure branch no longer has a deterministic model-free trigger and is covered by review only.
- [x] 2.4 Run the affected suites: `cargo test -p codex-harness --locked --test executor_control`, `--test executor_observation`, `--test executor_warning`, and `cargo fmt --all`.

## 3. Document and deliver

- [x] 3.1 Record the user-confirmed decision in `docs/project-decisions.md`, superseding the one-mebibyte transport bound.
- [x] 3.2 Validate with `openspec validate --strict --no-interactive` and `git diff --check`.
- [ ] 3.3 Exercise the installed launcher outside this checkout through the immutable lifecycle: one fresh managed spawn and one real exact-session resume whose state exceeds one mebibyte, with rollback to the prior build retained.
