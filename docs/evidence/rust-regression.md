# Native reproduce-regression process observer

Verified 2026-09-08 in the dirty `codex-harness` checkout. This is the bounded
reproduce-regression executable subset of
[migration task 7.2](../../openspec/changes/migrate-harness-to-rust/tasks.md),
not closure of that task, not structured-codex-run skill wiring, and not global
skill registration or activation.

Owned files:

- [Helper library](../../crates/codex-harness/src/regression.rs)
- [Native CLI and compiled fixture](../../crates/codex-harness/src/bin/harness-observe.rs)
- [Deterministic integration tests](../../crates/codex-harness/tests/regression.rs)
- This evidence record

Unrelated dirty work was preserved. Existing Python/PowerShell helpers, skill
invocation docs, `BINARIES`, crate manifests and OpenSpec task checkboxes were
not edited. Parent owns binary staging and skill lifecycle integration.

## Behavior

`harness-observe.exe` keeps the process_case CLI: `--cwd`, `--timeout`, optional
`--ready-timeout`, optional `--output-limit`, optional `--stdin`, then `--` and an
absolute child executable with separately tokenized arguments. There is no PATH
or shell lookup. Invalid requests, including a preexisting `--root`, exit 2 with
a private diagnostic and no JSON receipt. Launch/wait failures print JSON with
`status=infrastructure-failure` and exit 1. CLI exit 0 is only
`status == "exited"` and `native.ExitCode == 0`.

The child receives `PROCESS_CASE_ROOT` pointing at a newly allocated temporary
directory, or an explicit absolute `--root` created with `create_dir` so an
existing leaf (including a link) is rejected. Ordinary parents are required
through `harness_core::inventory::ordinary_parents`. Readiness is the exact
trimmed text `READY` in `ready.txt`; marker reads are capped at 64 bytes.
A polled output-byte threshold, readiness deadline and optional `cancel.txt`
containing `CANCEL` signal job cancellation. Natural exit, readiness-failure,
readiness-timeout, timeout, cancelled, memory-limit, output-limit and
infrastructure-failure remain distinct. Native receipts keep `ExitCode`,
`ProcessExitCode` and `AssignedBeforeResume`. `request.json`, `observed.json`
and `report.json` are created with `create_new`; a child-created symlink is
left in place and the helper fails instead of following it. Case files are
retained; Job close is the cleanup authority.

Process jobs use `harness_core::process::{Job, CommandSpec, Deadline, Cancellation}`
with a 512 MiB committed-memory bound. Assignment happens through
`spawn_suspended` before resume. `Job::wait` owns timeout (exit 124), cancel
(130) and memory (125). Stdin is an optional file; unspecified stdin is NUL.
Stdout and stderr are distinct new files under the case root.

`--fixture` is compiled into the same binary so tests never spawn Python or
PowerShell helpers.

## Checks

Knowledge state: **confirmed** for the commands below from the repository root.
Runtime: Rust/Cargo 1.97.1, `stable-x86_64-pc-windows-msvc`, debug artifacts.
Host memory required `--jobs 1` and `--test-threads 1`. No model calls, live
services or global mutations.

```text
cargo test -p codex-harness --test regression --offline --locked --jobs 1 -- --test-threads 1 --nocapture
cargo clippy -p codex-harness --offline --locked --jobs 1 --bin harness-observe --test regression -- -D warnings
rustfmt --edition 2024 --check crates/codex-harness/src/regression.rs crates/codex-harness/src/bin/harness-observe.rs crates/codex-harness/tests/regression.rs
```

Final native suite: **9 passed, 0 failed**, 11.90 seconds. Clippy and rustfmt
passed on the owned targets.

| Test | Coverage |
| --- | --- |
| `success_preserves_args_cwd_env_and_stdin` | Unicode argv, cwd, PROCESS_CASE_ROOT, stdin, AssignedBeforeResume |
| `natural_nonzero_is_distinct_from_readiness_and_timeout` | exit 7, readiness-failure, readiness-timeout / cancel 130 |
| `output_limit_and_execution_timeout_reap_owned_descendants` | output-limit plus timeout 124 and descendant Job close |
| `concurrent_streams_and_file_cancellation_are_observed` | 2 MiB concurrent streams and cancel.txt |
| `missing_executable_is_infrastructure_failure` | JSON infrastructure-failure, native null |
| `rejects_relative_and_unbounded_requests` | relative exe, bad timeout/ready/output-limit |
| `preexisting_root_preserves_private_files` | existing --root rejected; private file untouched |
| `report_symlink_preserves_foreign_target` | child report.json link not followed |
| `oversized_ready_marker_is_not_readiness` | >64-byte READY is not readiness |

Evidence directories under `%TEMP%`:

- `harness-rust-regression-success-mBNyj8`
- `harness-rust-regression-statuses-o4onbO`
- `harness-rust-regression-limits-gyWYbi`
- `harness-rust-regression-streams-jg4Yck`
- `harness-rust-regression-infra-8ltLdI`
- `harness-rust-regression-preexisting-root-kj0K49`
- `harness-rust-regression-report-link-H3qb5a`
- `harness-rust-regression-oversize-ready-yyf62V`

Source SHA-256 after the passing run:

- `regression.rs` `2AF308508BC3AEB560BB5C456C46596ABB68774CBE786EBF3525B33C0BE66778`
- `harness-observe.rs` `0F9988D7A439D97DAA15CAE5335EE6AD19A3EE8731E357ED23A8C7383270CA18`
- `tests/regression.rs` `B62AE148D585AB3DE6E66B41A0C8B03180B76BD4F39A64BF95DB698C12219F8D`

## Missing parity

- Skill invocation docs still name `scripts/process_case.py`; parent owns global
  registration and extra binary build identity.
- The parent subsequently ran the actual observer outside the checkout with
  the compiled fixture and owned stdin. `%TEMP%/harness-process-case-wEQxi4`
  records natural exit zero, READY, assignment before resume, separate 52/15-byte
  streams and zero remaining owned processes. No model call or service mutation.
- Python API `run_case(..., output_limit=)` remains the transitional helper
  until parent retires it.
- Combined workspace Clippy/format and parent native builder identity are
  unfinished adjacent work.
- Memory-limit outcome 125 is produced by Job wait and recorded in receipts,
  but this suite does not allocate against the 512 MiB cap.
