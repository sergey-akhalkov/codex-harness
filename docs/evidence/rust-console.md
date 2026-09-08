# Native Rust ConPTY boundary

Verified 2026-09-08 on Windows x64, Rust/Cargo 1.97.1 and windows-sys 0.61.2,
with dirty source based on `dfc2d6af49f2da949b2b1b0ed549a33c62d1eb4b`.
This covers [migration task 2.5](../../openspec/changes/migrate-harness-to-rust/tasks.md);
the native global launcher and final combined migration acceptance remain open.

The [session](../../crates/harness-core/src/console.rs) owns ConPTY, pipes,
transcript and cleanup. [Process creation](../../crates/harness-core/src/process.rs)
assigns the Job atomically with `JOB_LIST`, omits `HANDLE_LIST` for ConPTY, and
uses `STARTF_USESTDHANDLES` with null standard handles and no handle inheritance.
This follows the retained [C# acceptance oracle](../../tests/ConPty.cs), including
its handling of a parent with redirected stdio. Creation-side pipe handles stay
open until child creation. [Microsoft's contract](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session)
describes pipe lifetime and draining output during pseudoconsole closure.

The [Rust fixture](../../crates/codex-harness/src/bin/harness-console-fixture.rs)
uses ordinary Rust stdin/stdout/stderr, without reopening console devices or
ignoring control events. Cooked console EOF is the real Ctrl+Z sequence;
the fixture reads to EOF and the test checks the exact Unicode input. Closing
the host transport is a different operation, not a promised graceful stdin EOF.
The console transcript combines the rendered stdout/stderr screen; separate
redirected streams are covered by the process suite. Separate redirects in a
ConPTY command are rejected instead of silently ignored.

The transcript retains at most 4 MiB by default. The pump drains all later output
and returns an explicit `output_truncated` flag. Byte retention avoids repeatedly
copying the entire transcript as each fragment arrives. A real 8 MiB fixture
with a 512-byte capture limit completes naturally and reports truncation.

Verified commands from the repository root:

```text
cargo build --locked --jobs 1 -p codex-harness --bin harness-console-fixture --bin harness-process-fixture
cargo test --locked --jobs 1 -p codex-harness --test console -- --test-threads=1
cargo clippy --locked --jobs 1 -p harness-core -p codex-harness --lib --bin codex-harness --bin harness-process-fixture --bin harness-console-fixture --test process --test console --test native_build --test launcher -- -D warnings
```

The seven console cases pass: Unicode/argv/cwd/environment/exact EOF input,
actual stdout and stderr, nonzero exit 19, interactive prompt/echo/resize,
terminal Ctrl+C (`0xc000013a`), cancellation/Drop with foreign survival, and
bounded output capture. Strict Clippy also passed. Console cases completed in
1.38 s in the combined invocation. Its unrelated CPU-resource test later timed
out; the corrected full fourteen-case process suite and its independent CPU-ratio
oracle subsequently passed.
That failed combined invocation remains recorded, not rewritten as a success.

Evidence under `%LOCALAPPDATA%/codex-harness-evidence/`
`native-candidate-cf47006dc75b44b6b4636b98026fb1c9/` includes the reviewed input
hashes in `console-reviewed-inputs.json` and the initial sidecar report in
`console-sidecar-original.md`. Console tests retain and print their owned temp
artifacts. Initial sidecar tests required `CONIN$`/`CONOUT$` reopening and an
`EOF` sentinel; those were insufficient for normal-process acceptance and were
replaced. A subsequent counterexample at `%TEMP%/harness-rust-console-streams-gQ8isK`
proved that merely omitting handle inheritance still leaked parent redirects.
Explicit null handles fixed it; `streams-t0l4So` and `streams-Lewl2E` passed.

Non-Windows Unsupported and the declared Rust 1.89 minimum were not exercised.
No console-related global registration, subscription service or model call was
changed by these checks. The retained launcher suite is still required when
the native launcher replaces the script entry point.
