# Native controlled outcome oracle

The controlled oracle passed native acceptance on 2026-09-09.
The command is `codex-harness.exe outcome-oracle --request <private-json>`.
It runs no models and writes a fresh private `oracle.json` using the existing
outcome check schema (`executed`, `exit_code`, `passed`, timestamps, evidence and
check details). A completed native turn is one required input to correctness.

The host request contains `case_root`, `setup`, `execution` and `arm`
(`baseline` or `candidate`). `setup` is the original object returned by
[controlled preparation](rust-outcome-cases.md), frozen by the host before the
candidate works. Keep this request outside the candidate's mutable case.
`execution` selects that attempt's private `native.json` from
[outcome-run](rust-outcome-run.md), rather than its low-level process `result.json`.
The oracle checks the recorded case and event paths, the supported fixture
revision, required immutable inputs and their executable hashes. It refuses an
altered supplied executable before independent execution.

[The consumer](../../crates/codex-harness/src/outcome_oracle.rs) checks the five
controlled cases. Its independent process execution reuses the same
[Rust observer](../../crates/codex-harness/src/regression.rs) as `harness-observe`,
with separate private streams, owned Jobs, deadlines and output bounds.
Generated artifact freshness and actual product entrypoint output are separate
checks. Candidate execution audit is read first. Independent CLI invocations
carry an audit origin, preventing a repeated oracle from supplying the missing
candidate execution. The narrow documentation case checks exact text, local
links, unrelated execution and observed delegation; the missing checker case
requires a blocked result and an actually absent prerequisite.

The [process oracle](../../crates/codex-harness/src/outcome_oracle_process.rs)
executes the candidate's `check_process.exe` again. It retains any prior report
in private evidence before that invocation, requires new real target audit
records without rewriting history, checks natural versus forced status and
every byte of both 2 MiB flood streams, and waits past the descendant marker's
deadline. A separate owned sentinel must remain alive throughout the check and
is then cleaned through its own Job. It never discovers a process by PID/name
for termination. The generated checker remains an explicitly executed candidate;
Jobs contain its process tree, not its filesystem access.

[Evidence parsing](../../crates/codex-harness/src/outcome_oracle_evidence.rs)
retains relevant completed command/MCP items and reads Markdown once for both
current and changed-document projections. Event records, totals and counts,
document files, totals, traversal entries and depth are bounded. Missing,
malformed or incomplete relevant evidence prevents success. Command success
requires native completed status and integer zero; MCP errors prevent a
successful reference. These fields follow the pinned CLI 0.153.4
[native exec event definitions](https://github.com/openai/codex/blob/3d2ee51ca2d5db578f328aa75e20aa22c0197c9a/codex-rs/exec/src/exec_events.rs).

The compatible `details.skill_use` fields mean only a successful tool-input
reference to the skill path. They do not prove that the skill body was read or
its instructions applied. Raw native events, documents and failures stay in
private local evidence; no opaque reasoning or compaction payload is decoded.

## Acceptance

Eleven evidence-parser tests and 37 integration tests passed. The latter comprise
nine oracle tests, five preparation tests, two controlled-target tests, twelve
executor tests and nine existing observer tests. They run actual compiled CLI
entrypoints outside the checkout, using an explicit Rust upstream double instead
of a model. Product targets, native observer calls and Windows processes are real.
The tests cover all five controlled cases, repeated independent CLI attribution,
stale artifacts, lost history, altered/redirected immutable inputs, incomplete
native evidence, extra work/delegation, absent prerequisites, stale process
reports, forced codes and truncated streams. Prior process reports survive in
private evidence. Sentinel survival and subsequent owned cleanup are checked on
both successful and failed process checks.

Clippy passed for all CLI targets after four style-only corrections. The eleven
parser tests and the four-case actual CLI smoke test passed again afterward.
Rustfmt passed. Explicit Serena diagnostics were empty for the oracle, process
helper and evidence module; an initial concurrent oracle diagnostic request was
busy and a subsequent sequential request succeeded.

```text
cargo test -p codex-harness --bin codex-harness --offline --locked --jobs 1 --target-dir <owned-check-target> outcome_oracle::evidence -- --test-threads=1
cargo test -p codex-harness --test outcome_oracle --test outcome_run --test regression --test outcome_prepare --test outcome_case_fixture --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo clippy -p codex-harness --all-targets --offline --locked --jobs 1 --target-dir <owned-check-target> -- -D warnings
```

Commands ran from this checkout on Windows x64 with Rust/Cargo 1.97.1, locked
dependencies and a separate parent-owned debug target. Logs, exact source/binary
hashes and private receipt locations are retained in
`%LOCALAPPDATA%/codex-harness-evidence/outcome-oracle-5f16350d8334402cafa9efa148712963/`:
`unit-final.*`, `integration-final.*`, `smoke-final.*`, `clippy-final.*` and
`acceptance.json`. `clippy-initial.*` preserves the initial style failures.

External consumer cases and full suite integration remain unfinished. This
increment does not close task 7.3, establish a quantitative improvement or perform
global native activation. No new opencode-kit evaluation is authorized here.
