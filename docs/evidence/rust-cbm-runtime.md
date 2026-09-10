# Native Codebase Memory runtime increment

Rust migration tasks 5.2, 5.3 and 5.4 remain open. This increment adds native
resource inspection, account admission, explicit audited indexing, catalogue
retrieval and isolated CLI tool calls. It does not replace the installed MCP
adapter, shared broker or configuration activation/rollback lifecycle.

The [configuration reader](../../crates/harness-core/src/cbm_configuration.rs)
reads the three CBM resource settings without executing CBM. The CLI consumer is
`dependencies resource-check --cache DIRECTORY`; exit 1 means the required
all-false policy is inactive, and an unavailable/incompatible input is an error.
Automatic-index/watch settings come from SQLite; UI settings come from JSON.
Duplicate JSON keys are rejected to prevent an ambiguous policy observation.
The audited binary embeds UI assets: absent `config.json` enables its UI, while
an existing empty JSON object uses the upstream disabled default. A SQLite
`ui_enabled` key cannot override that behavior. The earlier native reader copied
the transitional adapter's incorrect SQLite fallback; the real-daemon acceptance
below exposed and corrected it before global activation.

`dependencies cbm-catalogue --executable FILE --account DIRECTORY` reads complete
tool definitions from the audited executable under the account catalogue lease.
It negotiates MCP, collects bounded catalogue pages and closes stdin, without
calling tools or opening an installed graph. This reuses the isolated dependency
probe process and now validates envelopes through the native MCP protocol module.
It does not cache definitions or replace the globally installed proxy.
Its fresh private probe cache now receives native SQLite settings disabling
automatic indexing/watching plus JSON disabling UI. Creation refuses existing
inputs; it does not repair or overwrite an adopted cache.

SQLite operates on a private copy. Source copying holds the Windows SQLite
shared database lock and, for WAL, the SHM initialization, writer, checkpoint
and recovery locks. Main and WAL inputs are capped at 8 MiB each. Aliases,
conflicting locks, persistent/hot journals and WAL mode without both ordinary
sidecars fail closed. This assumes the standard local Windows SQLite locking
protocol; it is not a snapshot guarantee against arbitrary mapped writes that
ignore that protocol. The copy avoids writable SQLite SHM access to the source
cache, including redirected source sidecars. The selected System32 SQLite must
be 3.41 or newer. SQL/value/instruction limits and a progress callback bound SQL
execution; the callback alone is not an overall filesystem deadline.

The [account lease](../../crates/harness-core/src/resource_admission.rs) uses the
existing `cbm-index.lock` / `cbm-catalogue.lock` names in an explicitly supplied
account directory independent of CODEX_HOME. It interoperates with the legacy
first-byte Windows lock, never removes the file, preserves existing bytes, and
rejects aliases. Provisioning must create the account directory first.

The [index adapter](../../crates/harness-core/src/cbm_index.rs) accepts only the
audited CBM 0.10.8 executable SHA-256
`b4b403b1d7c4def3785f148b93f345ce8427858f4f5489ce28580c4387a336a6`.
Its explicit consumer is `dependencies cbm-index --help`. The caller selects the
repository arguments, cache, runtime and account directory. Indexing changes
the selected graph cache. Settings are checked after admission and before each
native operation; drift is rejected without repair. The worker receives two
threads, a 1 GiB internal budget, a 2 GiB / 25% CPU Windows Job, a 600-second
deadline and no automatic retry. Admission waits at most two seconds.

Stdout/stderr use bounded pipes. The upstream response-file protocol is polled
every 10 ms and capped again at 16 MiB after reclaiming the entire Job. Polling
does not impose an absolute disk-write quota. Tool errors remain tool errors;
foreign stderr is not exposed in a public error. Job containment and private
environment selection do not constitute a filesystem/network security sandbox.

The pinned [worker parser](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/src/mcp/index_supervisor.c)
accepts only `index_repository` with its exact internal argument order. Its blob
is `b5257dfc15f450a30b1cf409719a51c5e106b038`. The pinned
[dispatcher](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/src/main.c),
blob `288dec3577130c22cdbce166c4a5e6377df6ad21`, forwards ordinary CLI calls to the
account daemon. The worker entry cannot be reused for general queries.

## Verification, 2026-09-10

From the repository root, using Rust/Cargo 1.97.1 and the shared evidence target:

```powershell
cargo test --locked --offline -j 1 -p harness-core --lib -- cbm_configuration::tests resource_admission::tests cbm_index::tests
```

The first combined run passed 22 tests: 14 configuration cases, four account
lease cases and four native worker fixture cases. The configuration cases use
the actual System32 SQLite and include live committed WAL, source SHM/WAL
hardlink/symlink sentinels, recovery input bounds and actual SQLite writer/lock
conflicts. Worker cases use a Rust executable and cover Unicode, tool errors,
nonzero exit, malformed/duplicate responses, flood, deadline, cancellation and
a child surviving immediate parent exit. These are fixtures, not actual CBM
index acceptance. Logs are `cbm-native-first.stdout` / `.stderr` under the local
`core-lifecycle-29d511c364fb499286ae57b389e34c6a` evidence directory.

The corrected `resource-check` CLI also passed outside the checkout against the
installed cache. All settings were false and the main/JSON hashes stayed
unchanged. `cbm-configuration-corrected-actual-observation.json` binds that run
to manager SHA-256
`fb44c87f18c7b28ccea946be6826d45a43bdc1cd4ea260c430ac72dab357b036`.

Actual audited CBM acceptance initially failed on the test fixture's ordinary
Windows Temp cache. The diagnostic attempt exited naturally with code 1 because
CBM rejected that ancestor's DACL. Its owned job was empty after cleanup;
`hmp-0JP0M0/process.json` and private stderr preserve the cause. The initial
attempt lacked retained stderr, so its precise original cause is unconfirmed.
The fixture now uses the existing protected local-app-data allocator. No CBM
permission check was weakened. Native failures retain bounded private streams,
process evidence and a public locator.

The corrected actual worker test passed in 11.89 seconds, including rejection
of a separate drifted configuration without writes. The owned case is
`C:/Users/noilw/AppData/Local/hmp-22aX88`. The actual `dependencies cbm-index`
consumer then passed from outside the checkout in 7.89 seconds: the intended
Unicode-path repository produced 6 nodes, 6 edges and no unindexed files;
the Job was empty and temporary worker state was removed. Reports and exact
manager identity are `cbm-index-actual-cli.json` and
`cbm-index-actual-cli-observation.json`. These durations are acceptance observations,
not matched performance comparisons. General queries, graph coverage/navigation,
the installed MCP consumer and complete runtime lifecycle remain untested here.

Independent Grok review found no P0/P1 in the scoped index/admission orchestration
and identified three P2 diagnostic-retention issues. Corrections write diagnostics
only on failure, retain the private locator even when writing diagnostics fails,
save at most 16 MiB of the response and remove its original polled file. Cleanup
or diagnostic I/O failure is reported explicitly. Fixture failures inspect and
remove their own retained reports. A new actual Rust subprocess case verifies
successful work despite an unusable diagnostic destination, preservation of the
original worker error, and bounded retention of a 64 MiB response file.

The combined pre-review run passed 29 tests with two explicit package cases
ignored. After the corrections, six affected cases passed: five worker cases
and the Python reader-startup regression. The JSON override case also includes
duplicate-key rejection. Six focused CLI tests passed across resource inspection,
indexing arguments and Python staging arguments. Final source/check identity
is recorded in the local `cbm-python-checkpoint.json`; the broader migration and
actual installed MCP consumer remain open.

The final rebuilt CLI repeated resource inspection and owned indexing successfully,
with unchanged installed configuration hashes, 6 graph nodes and 6 edges.
`cbm-python-final-cli-observation.json` records manager SHA-256
`1eb19258b794c8e964dfdccb05219079cea0fc142e55e836035bd202359ea239`.
Final core/CLI all-target Clippy with warnings denied and workspace format checks
passed (`cbm-python-complete-*` logs). The four linked evidence/entrypoint documents
passed 75 local-target checks and `git diff --check`.

An additional actual CLI catalogue read from outside the checkout succeeded:
15 definitions, zero tool calls, empty owned Job and removed private state.
`cbm-catalogue-first-cli.json` / `.stderr` retain this observation; manager SHA-256
was `a1f2a798a4709fc1d875251bcd70ff24a06dd079e7d88738b7878b0f329bb6f0`.
That binary predates the later pipe/session integration and is not evidence for
the current stdio server. Its new fixture case initially failed because the
test process required four tool calls and assumed a 38-entry catalogue when
splitting pages; both fixture assumptions have been corrected. Subsequent
acceptance belongs to the [stdio record](rust-mcp-stdio.md).

## Explicit tool calls and actual daemon findings, 2026-09-10

`dependencies cbm-tool --executable FILE --cache DIRECTORY --tool NAME
--arguments-file JSON` accepts the audited non-index tools. It passes strict,
bounded UTF-8 object arguments through upstream `cli --json NAME --args-file`,
captures the complete `CallToolResult`, and preserves upstream exit 1 with
`isError: true`. An exit/result contradiction, empty startup failure, malformed
result, flood, deadline or cancellation fails with bounded private diagnostics.
No tool retry is automatic. Each call uses a fresh private `CBM_RUNTIME_DIR`,
two internal workers, a 1 GiB internal memory budget and a 2 GiB/25% CPU Job;
the operation deadline is 60 seconds. Indexing retains its separate admission
and internal-worker path.

The pinned [bootstrap](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/src/daemon/bootstrap.c)
uses that runtime parent for current and legacy Windows endpoint names. Empty
or oversized environment values can fall back to the platform directory, so
the adapter checks the actual selected private value before spawning. This
avoids contacting or upgrading an installed daemon; it does not permit two
daemons to share a graph cache.

Actual simultaneous-daemon acceptance exposed that distinction. The first
daemon kept its own MCP client open while the second CLI failed after the
upstream 30-second startup wait with empty stdout. Its Job was reclaimed; the
retained evidence is `hmp-ntmWCe/process.json` and private stderr. The pinned
[host](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/src/daemon/host.c#L123)
opens `cache/logs/cbm-daemon.log`, and the Windows
[log implementation](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/src/daemon/ipc.c#L4581)
holds it with `FILE_SHARE_READ`, excluding another writer. The native command
now rejects an observed busy log before spawning. This is an observation, not
a persistent lease; a later race still fails through the bounded upstream call.
The failed simultaneous-query case is not claimed as passing shared-worker
acceptance. That requirement remains open for the broker implementation.

The same real-daemon run exposed the UI default error in the earlier reader.
The pinned [UI loader](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/src/ui/config.c#L111)
enables embedded UI when JSON is absent and never reads SQLite's `ui_enabled`
key. The earlier report's all-false claim therefore did not prove disabled UI
for an absent JSON file. The reader, fixtures and fresh native probe-cache
initialization are corrected. No installed configuration was changed.

The final owned-package test in
[cbm_index_acceptance.rs](../../crates/harness-core/src/cbm_index_acceptance.rs)
passed in 50.00 seconds using the native initializer. It proved busy-cache
refusal, continued replies from the separate client, its normal EOF shutdown,
successful queries after that owner closed, preserved tool errors, and no UI
startup in the owned daemon log. `hmp-SLYzcO/report.json` retains actual results;
`cbm-tool-busy-cache-final.*` records the test invocation. This is acceptance
timing, not a matched performance comparison.

Earlier actual CLI calls outside the checkout returned the expected `alpha`
row, coverage data and a Cypher tool error, with empty Jobs and removed private
state (`cbm-tool-{query,coverage,tool-error}-actual.*`). Those executions predate
the UI fix. Coverage reported `missing` for an existing Unicode-path source;
the fresh ASCII case reported `metadata_changed`. Both retain upstream's
best-effort caveat and are not evidence of complete/current source coverage.
Grok traced the pinned
[freshness function](https://github.com/DeusData/codebase-memory-mcp/blob/v0.10.8/src/mcp/mcp.c#L4180),
which the parent checked: `missing` means the narrow `stat` failed before hash
lookup, while `metadata_changed` means stored size/mtime differed. Its Windows
mtime comparison uses whole seconds. The UTF-8 Windows path and timestamp
precision are plausible causes of these observations; exact errno and stored
timestamp values were not instrumented. The adapter does not rewrite the flags
or turn a successful graph query into a coverage guarantee.

Focused current checks passed 22 core cases, ten CLI cases and 15 protocol
probe cases; actual-package checks remain explicitly selected. CLI tests added
by Grok were inspected and executed by the parent. Global adapter activation,
shared-worker acceptance, complete provisioning and tasks 5.2–5.4 remain open.

Final Clippy for core/CLI all targets with warnings denied, workspace formatting
and native CLI build passed. The rebuilt CLI then read 15 catalogue definitions
without tool calls and returned the expected `alpha` query row outside the
checkout; both owned trees and private states were reclaimed. Manager SHA-256:
`dd566ae2bb7a55b0e833e64da4fbb291040edeb05d2b7c3e26e279aea866411a`.
`cbm-tool-final-{catalogue,query}-cli.*` retain those actual consumer results.
The local `cbm-tool-checkpoint.json` binds relevant dirty inputs, artifact hashes,
commands and limitations. Earlier `mcp-stdio-checkpoint.json` remains the
historical stdio checkpoint, not the identity of these later CBM changes.
