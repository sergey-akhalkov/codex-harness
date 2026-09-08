# Native Rust process boundary

Verified 2026-09-08 in the dirty `codex-harness` checkout based on
`dfc2d6af49f2da949b2b1b0ed549a33c62d1eb4b`. This is the bounded process portion of
[migration task 2.4](../../openspec/changes/migrate-harness-to-rust/tasks.md),
not closure of the 37-task migration or evidence of global activation.

Owned files:

- [Core API](../../crates/harness-core/src/process.rs).
- [Native integration tests](../../crates/harness-core/tests/process.rs).
- [Rust subprocess fixture](../../crates/codex-harness/src/bin/harness-process-fixture.rs).
- This evidence record. Manifests, exports, build consumers and task state belong
  to the integrating parent; unrelated dirty work was preserved.

## API and invariants

`Job::new(Limits { memory_bytes, cpu_percent })` creates a fresh anonymous,
non-inheritable kill-on-close job. CPU is a hard cap in hundredths of a percent,
rounded down; memory is aggregate committed bytes. There is no API for adopting
a named job, attaching the owner process, or assigning an arbitrary PID.
Termination also explicitly rejects a job containing the current process.

`CommandSpec::new(absolute_executable)` accepts public `args`, `current_dir`,
`stdout` and `stderr` fields. The latter two are `Option<File>`; clones of one
file provide a combined log. Stdin and unspecified outputs use NUL. Environment
is inherited, cwd may be specified, and UTF-16 argv is quoted without a shell.
Only duplicated standard file handles enter the inheritance allow-list.

`spawn_suspended(&CommandSpec)` returns `SuspendedProcess`, whose `process()`
provides retained identity and observation before `resume()` produces
`OwnedProcess`. `spawn` combines those steps. Dropping an unresumed process
terminates only its original native handle. The creation attribute
`PROC_THREAD_ATTRIBUTE_JOB_LIST` performs assignment inside `CreateProcessW`,
closing the owner-crash gap between separate create and assign operations.
This requires Windows 10+. There is no fallback that resumes without containment.
[Microsoft's atomic creation explanation](https://devblogs.microsoft.com/oldnewthing/20230209-00/?p=107812)
describes the avoided race.

`OwnedProcess` retains a native handle, exposes `identity`, `is_running`,
`exit_code`, `cpu_time`, `wait_for_exit`, and implements `AsHandle`. Identity is
PID plus the complete `GetProcessTimes` creation FILETIME. `Job::owns(identity)`
opens only query/synchronization rights, checks creation time, liveness and job
membership on that same handle, and never grants cleanup authority. Mismatch or
foreign membership returns false; ambiguous native errors remain errors.

`Job::wait(self, &process, Deadline, &Cancellation, cleanup_timeout)` consumes
the job, returning `Outcome { reason, exit_code, process_exit_code, job }`.
Cancellation, observed memory notification, root exit and deadline are checked
in that precedence. Codes are 130, 125, the unsigned native exit code, and 124.
CPU limits throttle execution; they do not invent a CPU-termination status.
On root exit, all remaining members are terminated. Cleanup waits for both zero
active members and a signalled root handle under one finite budget. The two
signals are not simultaneous on Windows. Notification draining is bounded so
descendant churn cannot indefinitely defer cancellation checks.

`Job::terminate(self, code, cleanup_timeout)` consumes the job and confirms zero
active members within the supplied budget. Closing the last job handle on any
return/error/panic/owner exit requests kernel cleanup; Drop never waits or
enumerates processes. Normal native calls remain synchronous; the budget bounds
the polling/wait policy, not arbitrary kernel or filesystem stalls.

`Deadline::after` rejects clock overflow. `Cancellation` is clonable and can be
signalled from another thread. `ExclusiveFileLock::try_acquire` and `acquire`
provide nonblocking or deadline/cancellation-bounded whole-file exclusive locks.
Opening does not truncate existing content; lock files persist. Windows handles
deny deletion/rename while participating, preventing a second lock object at the
same path. Closing the owning file unlocks, including after process termination.

## Reproducible checks

Knowledge state: **confirmed** for the following commands from the repository
root. Runtime: Rust/Cargo 1.97.1, `stable-x86_64-pc-windows-msvc`, debug artifacts,
`windows-sys` 0.61.2; workspace declares MSRV 1.89. The exact 1.89 toolchain was
not exercised. No dependency manifest or global registration was edited by this
bounded implementation.

```text
cargo build --locked -p codex-harness --bin harness-process-fixture
cargo test --locked -p harness-core --test process -- --test-threads=1 --nocapture
cargo clippy --locked -p harness-core --test process -- -D warnings
cargo clippy --locked -p codex-harness --bin harness-process-fixture -- -D warnings
rustfmt --edition 2024 --check crates/harness-core/src/process.rs crates/harness-core/tests/process.rs crates/codex-harness/src/bin/harness-process-fixture.rs
```

All passed. Final native suite: **13 passed, 0 failed**, 15.20 seconds. The test
uses `target/debug/harness-process-fixture.exe`; `HARNESS_PROCESS_FIXTURE` can
explicitly select another built executable. A missing fixture fails with the
build command rather than silently skipping native acceptance. Tests retain
owned temporary artifacts and print their paths. They make no model calls,
mutate no live services and never terminate processes by name or a recovered PID.

Final run output: `%TEMP%/harness-rust-process-final.log`. Relevant artifacts:

| Native check | Evidence directory under `%TEMP%` |
| --- | --- |
| Pre-resume containment, native identity, argv/Unicode, cwd/environment, stdin EOF, combined output | `harness-rust-process-suspended-cKPCED` |
| Memory denial and outcome | `harness-rust-process-memory-EmSeNB` |
| CPU control and uncapped comparison | `harness-rust-process-cpu-gfaJcv` |
| Timeout and cancellation, root/grandchild death, foreign survivor | `harness-rust-process-timeout-sQPeQV`, `harness-rust-process-cancel-oPPtk1` |
| Owner killed before/after resume, owner exit bypassing Rust destructors | `harness-rust-process-owner-suspended-4rhxRG`, `harness-rust-process-owner-running-FoLi85`, `harness-rust-process-owner-exit-wi4AhB` |
| Root already exited: job drop and explicit wait kill surviving grandchild | `harness-rust-process-root-exit-xESO0U`, `harness-rust-process-root-exit-UEXBTP` |
| Creation-time mismatch, foreign identity, retained exit identity | `harness-rust-process-identity-nDGo6c` |
| Cross-process lock contention, release and owner termination | `harness-rust-process-locks-6DoiQr` |
| Invalid argv/path, rejected atomic assignment, zero cleanup budget | `harness-rust-process-invalid-9yrrTl` |
| Native exit 259 and high-bit values | `harness-rust-process-exit-codes-fiphpF` |

Memory verification holds two real processes after allocation, sums their
`K32GetProcessMemoryInfo` `PrivateUsage`, and checks that granted aggregate private
commit remains within the 80 MiB limit. The first fixture committed 40 MiB; the
second could commit only 36 MiB before `VirtualAlloc` returned error 1455. An
independent job remained alive after memory-triggered termination. Measured
private commit was 81,117,184 bytes against an 83,886,080-byte limit. The peak job
counter was 87,736,320 bytes in this denial scenario; it is recorded separately
and is not used as the granted-memory oracle.
[The native memory-counter contract](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex)
defines `PrivateUsage` as private commit.

## Failure history and evidence limits

The first compiled native run passed 10/12. Memory and timeout paths exposed job
accounting reaching zero before the root handle signalled exit. Waiting for both
within the same budget corrected the owning mechanism; the full suite passed
afterward. Initial compilation also corrected binding locations for
`PROCESS_SYNCHRONIZE` and job-memory message 10. A later counterexample first
used a one-byte limit, which Windows rejects while configuring the job (error
87); the corrected one-page limit instead exercised failed atomic assignment.
No failed attempt was treated as acceptance.

Memory enforcement does not depend on a notification being delivered. Outcome
125 does: ordinary job-memory completion messages are best-effort, as in the
previous runner. Their absence cannot prove that no allocation was denied, and
some early startup failures may surface as creation/exit errors instead.
[Microsoft's notification contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_associate_completion_port)
explicitly distinguishes these messages from guaranteed notification limits.

Creation-time mismatch is a deterministic PID-reuse counterexample, not evidence
that the OS actually recycled a specific PID during this run. CPU evidence proves
throttling of this native workload on this host, not a general throughput claim.
Non-Windows job construction explicitly returns Unsupported; that branch was
not run on a non-Windows host. ConPTY/interactive streams, service/WMI/Scheduler
adoption, compiler-consumer integration, global activation, independent broader
review and full workspace acceptance remain with their owning tasks/parent.
No task checkbox was changed here.

Final input/artifact SHA-256 values (dirty source is material):

| Input/artifact | SHA-256 |
| --- | --- |
| `crates/harness-core/src/process.rs` | `87A96F8D482616AE99822E7EBC3D4F0576FC00BF311396ACCBABBEC546795D2F` |
| `crates/harness-core/tests/process.rs` | `8C266CDD0F01E16327FFA07D368762F942A8D27060F29B3B0C8B3821B88A65D9` |
| `crates/codex-harness/src/bin/harness-process-fixture.rs` | `19A0EC8E57F2C54DC49926BE6935A7004D4E9C7EDC84F1E4B5D9DA24DCCC3717` |
| Root `Cargo.lock` | `E12666F975672BB73A218DEA1753134810F990C524E5888B74B73D1B67B50F64` |
| `target/debug/harness-process-fixture.exe` | `41A2DB542FF7A9CABE881780955FFA26BF0BE1C99F22B838D6DC9C5F021FE179` |
| `target/debug/deps/process-a04f315cda14a974.exe` | `FF03FFDFD707CCFEBD53561685ACAEC0EB9D04ABE471F349ADE4FA03E42C6118` |
