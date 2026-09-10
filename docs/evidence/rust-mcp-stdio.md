# Native MCP stdio migration

Migration tasks 5.3 and 5.4 remain open. The native protocol, serial session and
stdio transport are an implementation increment; the installed Python adapters,
shared brokers, owner reconciliation and complete foreign-tool integration have
not been replaced. No global registration has changed in this increment.

[mcp_protocol.rs](../../crates/harness-core/src/mcp_protocol.rs) checks bounded
UTF-8 JSON-RPC frames, preserves string/numeric identities and rejects duplicate
JSON keys. It is also consumed by the isolated native dependency catalogue/probe.
Frames are limited to 16 MiB, with reads of at most 64 KiB. Tool-session requests
are limited to 16 KiB, including their identity and envelope.

[mcp_session.rs](../../crates/harness-core/src/mcp_session.rs) negotiates
`2024-11-05`, advertises tools only, and waits for the initialized notification.
It handles ping/catalogue locally and serializes tool work behind a 64-request
queue. Indexing has a 600-second request deadline; other operations have 60
seconds, starting at session admission. Cancellation or timeout does not free
the active slot until the caller reports completed cleanup. Tool errors and
structured results remain intact. Unsupported methods are explicit errors.

The lifecycle and cancellation contracts are taken from the selected protocol's
[lifecycle](https://modelcontextprotocol.io/specification/2024-11-05/basic/lifecycle)
and [cancellation](https://modelcontextprotocol.io/specification/2024-11-05/basic/utilities/cancellation)
specifications. Negotiating this version does not claim support for later task
extensions or additional server capabilities.

[mcp_stdio.rs](../../crates/harness-core/src/mcp_stdio.rs) consumes the session
through owned Windows pipe endpoints. It requires an explicit connection deadline
and a handler that respects each operation's cancellation/deadline and reclaims
its owned children before returning. Input buffering has two envelope slots;
output writes have a five-second deadline. Cleanup errors are explicit and
prohibit reusing unfinished worker state. This is not a sandbox for arbitrary
in-process callbacks.

[cancellable_pipe.rs](../../crates/harness-core/src/cancellable_pipe.rs) admits
synchronous byte-pipe handles. It checks native `FileModeInformation` before
calling `ReadFile`/`WriteFile` with a null OVERLAPPED pointer; overlapped and
message-mode handles are rejected. This follows Microsoft's
[ReadFile contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-readfile)
and [mode query contract](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntqueryinformationfile).
An unexpected pending mode query is an explicit failure that retains its small
heap buffer rather than freeing memory potentially referenced by the kernel.
No worker or pipe read/write starts on that path.

Cancellation sets an internal stop flag and repeats native cancellation while
waiting for the actual thread to exit. Error/deadline cleanup has a separate
two-second budget. The worker keeps its endpoint alive across all syscalls;
an unresolved join does not close that endpoint. An exceptional detached thread
is reported as unfinished cleanup and keeps its resources until it exits.
Implicit Drop failures go to stderr. Peer endpoints are never closed or killed.

## Verification state, 2026-09-10

The first protocol/session run passed eight unit cases with Rust/Cargo 1.97.1:

```powershell
cargo test --locked --offline -j 1 -p harness-core --lib -- mcp_session::tests mcp_protocol::tests
```

`mcp-session-first.stdout` / `.stderr` are retained below the current local native
evidence root. They exercise fragmented Unicode, exact ID types, malformed and
oversized frames, lifecycle/capability errors, queue bounds, cancellation,
timeout and the cleanup gate. This does not establish pipe or process behavior.

The actual [CBM catalogue](rust-cbm-runtime.md) consumer has succeeded against the
audited package. Native [stdio process checks](../../crates/codex-harness/tests/mcp_stdio.rs)
use `harness-mcp-probe-fixture --stdio-session` inside an owned Windows Job.
All six cases passed: fragmented Unicode/exact identities, cancellation and the
cleanup gate, EOF during an active tool, truncated input, output backpressure
with both peer endpoints held open, and panic after cancellation. Timeout of the
outer Job fails acceptance; the tested server exits naturally. The last run took
5.98 seconds (`mcp-stdio-final-process.*`).

The scoped Astra review found that converting a worker panic into a tool result
could hide the failure after cancellation and permit queued work. The corrected
transport tears down the connection with a nonzero outcome; the process case
also proves the queued handler never starts. Review also corrected the expected
truncated-frame diagnostic without changing its nonzero-exit oracle.

The Grok pipe handoff had no passing test run. Parent integration corrected its
compile/test issues and reproduced an endpoint-lifetime defect with a controlled
pause immediately before `ReadFile`. The baseline failed because Drop closed the
still-used handle (`pipe-unresolved-baseline.*`, 0.25 seconds). The corrected case
passed (`pipe-unresolved-fixed.*`, 2.22 seconds); the fixture explicitly releases
and waits for its own paused worker even on the failing baseline. Retained
handoff sources are `pipe-middle-handoff.rs` and `pipe-middle-handoff-tests.rs`.

Ten real-pipe integration cases passed, including overlapped/message rejection,
pre-cancellation, observed active read/write cancellation with peers held open,
EOF, Unicode, input bounds and repeated drop (`pipe-handle-modes.*`, 0.48 seconds).
The final core subset passed nine tests (`mcp-stdio-final-core.*`, 2.22 seconds).
The unchanged dependency probe/catalogue suite passed 15 cases with its explicit
installed-package case ignored, and three CBM CLI cases passed
(`mcp-stdio-process-first.*`). These checks do not establish the remaining global
proxy/broker lifecycle or arbitrary foreign tool behavior.

Final core/CLI all-target Clippy with warnings denied and workspace formatting
passed (`mcp-stdio-clippy-fixed.*`, `mcp-stdio-format.*`). The current manager and
fixture were rebuilt, then all six process cases passed again from outside the
checkout (`mcp-stdio-outside-checkout.*`, 5.90 seconds). The test driver uses the
recorded compiled assertions against that rebuilt fixture; it is not an installed
MCP registration. The final actual catalogue read also passed, returning 15 tool
definitions including `check_index_coverage`, no tool calls, an empty Job and
removed private state (`cbm-catalogue-final-cli.*`). Manager SHA-256 is
`3e0983debb2f38a74a5a8e3ed297d9d20c1c2ab3b724e8c1e44a73c84df4cfd6`.
Input/build identities and run scope are retained in `mcp-stdio-checkpoint.json`.

## Explicit CBM connection integration

The later `codex-harness mcp codebase-memory --help` entry point connects
[cbm_stdio.rs](../../crates/harness-core/src/cbm_stdio.rs) to the native session
and Windows transport. It requires explicit executable/cache/runtime/account
paths, a saved `cbm-catalogue` JSON report and a connection lifetime. It does not
register or provision anything globally.

[cbm_catalogue.rs](../../crates/harness-core/src/cbm_catalogue.rs) reads up to
4 MiB through an ordinary-file guard. It checks the report's version labels,
the exact 15 tool names and descriptor shapes, preserving complete definitions.
This is explicit local configuration: its digest label is not proof of origin.
Each actual tool operation independently verifies the audited executable.
Initialize, ping and tools/list perform no foreign executable launch.

The fallible transport handler distinguishes successful tool results from
infrastructure failures. An error after cancellation or EOF still terminates
the connection and cannot advance the queue. CBM cancellation can release the
slot only when its typed native outcome confirms no worker started, or a zero
process Job plus joined output readers. Diagnostic strings are not used as
cleanup proof. Upstream `isError` tool results pass through normally.

Initial integration checks passed four catalogue cases, six CBM worker cases,
eight protocol/session cases, eight stdio cases and the CLI option check.
They include lazy handshake with a nonexistent CBM executable and a backend
cleanup error after both cancellation and EOF. Logs are `cbm-stdio-first-core.*`
and `cbm-stdio-first-cli.*` in the same local evidence directory. Actual package
acceptance and final build identity are separate from these fixture checks.

The actual native CLI integration then passed indexing, the expected `alpha`
query, an upstream Cypher tool error, cancellation after observing the daemon's
live operation-log handle, and another successful query on the same connection.
String/maximum unsigned numeric request IDs and structured results were checked.
The CLI process ran from a fresh owned directory outside the checkout and exited
normally after EOF. The outer test Job uses a stricter 512 MiB cap; this small
owned graph does not establish large-repository resource or timing behavior.

The complete actual case took 61.00 seconds (`cbm-stdio-actual-complete.*`). Its
explicit inputs were the audited CBM executable, saved catalogue and the
previously prepared `hmp-SLYzcO` test cache/repository; no installed cache was
selected. The earlier query/error/cancel-only case also passed in 57.11 seconds
(`cbm-stdio-actual-first.*`). Cancellation may retain the bounded private
diagnostic files defined by the CBM adapter; continued operation proves worker
cleanup, not removal of those retained diagnostics.

Core/CLI all-target Clippy with warnings denied and workspace formatting passed
(`cbm-stdio-clippy.*`, `cbm-stdio-format.*`). The final identity and applicable
checks are recorded in `cbm-stdio-checkpoint.json`. This explicit connection
still does not deliver the shared broker, ownership reconciliation, automatic
catalogue lifecycle or global registration; tasks 5.3 and 5.4 remain open.
