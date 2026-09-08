# Native structured Codex inspection helper

Verified 2026-09-08 in the dirty `codex-harness` checkout. This is the bounded
structured-run executable subset of
[migration task 7.2](../../openspec/changes/migrate-harness-to-rust/tasks.md),
not closure of that task, not reproduce-regression, and not global skill
registration or activation.

Owned files:

- [Helper library](../../crates/codex-harness/src/structured.rs)
- [Native CLI and compiled fixture](../../crates/codex-harness/src/bin/harness-inspect.rs)
- [Deterministic integration tests](../../crates/codex-harness/tests/structured.rs)
- Crate dependencies `sha2` and `tempfile` in
  [crates/codex-harness/Cargo.toml](../../crates/codex-harness/Cargo.toml)
- This evidence record

Unrelated dirty work was preserved. Existing Python
`scripts/run.py`, `stdin_bridge.py`, skill invocation docs, global configs and
parent-owned `process.rs`/`console.rs`/`main.rs` were not edited. Task
checkboxes were not closed.

## Behavior

`harness-inspect.exe` keeps the existing CLI flags: `--cwd`, `--prompt-file`,
`--command-json`, `--oracle-json`, `--model`, `--provider`, `--subscription`,
repeatable `--input`, optional `--codex-home`, `--timeout` and `--output-limit`.
It prints one JSON object with `status` and `evidence_root` and exits 0 only for
`success`.

The native prefix must be an actual `.exe`. There is no new shell wrapper. A
script launcher remains the caller's explicit provided prefix, which this helper
does not invent. Prompt text is written to a file and delivered through
`CommandSpec.stdin`; the model-facing argv still ends with `-` and does not
contain the prompt. Sandbox is fixed to `read-only`. Final JSON and event JSONL
are separate files under a unique evidence root outside the inspected checkout.

Process jobs use `harness_core::process::{Job, CommandSpec, Deadline, Cancellation}`
with a 512 MiB committed-memory bound. `Job::wait` owns timeout (exit 124) and
cancel (130). An output-cap watcher thread signals cancellation when stdout,
stderr or the final JSON file exceeds the limit and writes `output-limit.txt`.
The independent oracle is a second job. Receipts keep the existing `status` and
`native.ExitCode` contract and also store the parent `Serialize` `Outcome` under
`outcome`. Schema validation is the fixed inspection object only.

`--fixture` is compiled into the same binary so tests never spawn Python or
PowerShell helpers.

## Checks

Knowledge state: **confirmed** for the command below from the repository root.
Runtime: Rust/Cargo 1.97.1, `stable-x86_64-pc-windows-msvc`, debug artifacts.
Host memory required `--jobs 1` and `--test-threads 1`. No model calls, live
services or global mutations.

```text
cargo test -p codex-harness --test structured --offline --locked --jobs 1 -- --test-threads 1
```

Result after parent `Outcome`/`JobSnapshot`/`StopReason` `Serialize` derives and
lock reconciliation: 5 passed, 0 failed. `rustfmt --edition 2024 --check` on the
three new Rust files passed.

| Test | Coverage |
| --- | --- |
| `success_preserves_independent_inputs_and_distinct_evidence` | stdin vs argv, read-only sandbox, separate evidence, unchanged input hash |
| `negative_process_and_oracle_statuses_are_distinct` | 13 failure statuses including process vs oracle vs schema vs stale run_id |
| `timeout_and_output_limits_retain_partial_evidence` | timeout plus stdout and final-file output caps retain `final.json` |
| `run_ids_are_fresh_across_invocations` | two successes produce distinct 32-hex run identities |
| `timeout_receipt_is_distinct_from_oracle_and_keeps_partial_files` | process receipt `timeout` / exit 124, serialized `outcome.reason=Timeout` |

Source SHA-256 after the passing run:

- `structured.rs` `543E2045E87399F56AF54606AFFB53DA99145F76501CC50802F0264F6FDECF60`
- `harness-inspect.rs` `1E76DC3CA1AE64DC74869E5FC850ADA281925C97C48F61BACA8C2230CB6244BC`
- `tests/structured.rs` `5D9B7E2F8AB8AB08421406903EB8E377FCD5A0776E372B35E7E2D1A4689DD2AA`

The crate already listed `sha2` 0.10.9 and `tempfile` 3.27.0 in the workspace
lockfile. No new crate versions were downloaded.

## Missing parity

- Skill invocation docs still name `scripts/run.py`; parent owns global
  registration and extra binary build identity.
- Reproduce-regression is not ported.
- No outside-checkout owned-project consumer was run.
- No live Codex `--output-schema` / model route was used.
- Script-prefix compatibility was not reimplemented as a new helper.
- Combined workspace Clippy/format and parent native builder identity are
  unfinished adjacent work.
