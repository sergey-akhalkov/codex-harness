## Context

See [proposal](proposal.md) for the replacement scope. The current globally managed provider remains CBM while the native CodeGraph candidate is implemented and verified. Automatic CodeGraph incremental/catch-up refresh is accepted, with the existing 2 GiB ceiling retained. The published server can therefore be used directly under kit supervision, with a native MCP boundary controlling its catalogue, responses, admission and lifetime. No upstream source patch is planned.

### Calibration

Observed on Windows on 2026-09-10 using the published [CodeGraph 1.6.0 release](https://github.com/colbymchenry/codegraph/releases/tag/v1.6.0), source tag `dfccdf62547fcd76d343344d823a0e1998d3a89f`. The Windows x64 archive matched the release SHA256SUMS: `cd76c3c3391f2d40abef12b142151950b6d77abc2d8429e648f89eaa90f5b68a`. No compiler/runtime development environment was installed for CodeGraph; the trial used its bundled Node executable. A small local Rust runner reused the pack's existing Windows Job implementation to enforce 2 GiB aggregate private memory, 25% CPU and finite deadlines. Raw responses and local input identities stay outside Git.

| Actual operation | Observation |
| --- | --- |
| Initial index, locally selected large repository | 973 files, 17,806 nodes, 78,264 edges; 23.820 s wall time; 726,573,056 B peak Job private memory (~693 MiB); exit 0, no surviving owned processes |
| Coverage of that index | 412 Rust, 204 XML, 260 YAML, 38 C, 25 Pascal, 20 C#, 12 JavaScript, one PHP and one Python file; stored file rows had no parse errors; no indexed file exceeded 1 MiB |
| Storage of that index | 87,531,520 B database (~83.5 MiB), zero WAL bytes at the measured closed checkpoint |
| Explicit unchanged incremental sync, same repository | 1.233 s wall time, 170,999,808 B peak private memory; already up to date |
| Initial index, pack checkout | 7.302 s, 543,756,288 B peak private memory; 286 files, 7,450 nodes, 32,852 edges; 20 missing archived metadata files were reported during concurrent cleanup, so this was not a clean whole-checkout coverage result |
| Connect-time update, owned fixture | A symbol added after initial indexing became searchable on first MCP query even with `--no-watch` and `CODEGRAPH_NO_DAEMON=1` |
| Live watcher, owned fixture | New symbol became searchable without manual sync after the debounce; server reported 124 ms sync, process peak 198,934,528 B (~190 MiB), exit 0 and zero surviving processes |

Coverage counts establish extraction accounting, not correct semantics for every language. Concurrent work continued in both roots: these measurements describe their inputs at each run, not the latest working tree at handoff. CBM's larger file count is not a like-for-like source oracle: document/configuration support differs. Markdown, JSON, PowerShell and custom formats continue through their appropriate native tools. The native 1 MiB extraction limit must be checked against actual maintained inputs during acceptance; do not delete or split input files to make a benchmark pass.

The following values are UTF-8 **tool text bytes**, excluding protocol/catalogue overhead. They are not tokenizer or subscription measurements. Exact queries used the same current symbols/files; large-consumer names and paths are private. The pack example was `resource_failure_message` in `crates/harness-core/src/cbm_index.rs`. Its current source and Serena references served as the oracle.

| Question | Serena | CodeGraph | Interpretation |
| --- | ---: | ---: | --- |
| Locate the pack's exact function | 173 B | 148 B search; 498 B node/signature | Both can locate it; do not add graph startup for a known-file task |
| Read that small function | 572 B | 752 B node/body | Serena includes just the requested symbol; CodeGraph adds trails |
| File overview in the pack | about 100 B | 658 B | Prefer Serena for one known file |
| Locate a function in the large repository | 161 B | 403 B search; 1,127 B node/signature | Scope and operation matter more than a universal tool preference |
| Direct callers of that function | 892 B references, including an import | 166 B callers, two source-verified call sites | Graph is useful for a compact call graph; it is not all language references |
| One-hop impact of that function | Not measured as an equivalent Serena operation | 177 B | Three relevant symbols returned; still a candidate set |
| Pack cross-file context, fresh session per variant | Not measured as one equivalent call | 15,679 / 19,920 / 21,911 B at `maxFiles` 1 / 2 / 12 | File count is not an answer budget |
| Large-repository cross-file context | Not measured as one equivalent call | 24,983 B at `maxFiles=2` | Broad exploration needs a separate delivered-byte limit |
| Common-name callers, `limit=5` | Not measured | 14,984 B | Per-name/overload fan-out bypasses the intended small answer |

CodeGraph initialization returned 6,087 B on the wire; tools/list returned 5,802 B on the pack and 10,979 B on the larger repository. Warm calls in the small exact pack sequence took 4–83 ms after startup; the three independent exploration calls took 249–293 ms. These are individual observations, not stable latency promises. A published CLI caller query produced the same two callers in 340 B JSON, 331 ms wall time and 118,710,272 B peak memory. CLI read-only lookup did not refresh an owned fixture until an explicit sync. That remains a fallback, not the default now that bounded automatic refresh is accepted.

The graph also produced an unrelated `Error` edge for the pack example. A wrong file qualifier on a caller request fell back to other definitions with a note. These are reasons to preserve ambiguity/coverage and use Serena or direct source for exact-reference claims. The measurements above calibrated routing; they are not current managed-runtime acceptance or weekly usage measurements.

### Managed candidate acceptance

The native entry and published package were exercised on both current working
trees through `tests/codegraph_mcp.rs` and its private source-oracle input. Full
indexing, direct lookup/body, overview, callers, impact/exploration, known false
edges, ambiguous/missing names and fan-out used the same questions in raw and
managed lanes. Native SQLite path inventory matched all eligible working files:

| Input | Full index | Peak worker Job memory | Source coverage |
| --- | --- | --- | --- |
| Pack working tree | 27.441 s; 8,187 nodes, 36,995 edges | 621,580,288 B | 315 eligible paths = 315 indexed paths |
| Locally selected large working tree | 56.658 s; 18,118 nodes, 80,473 edges | 714,379,264 B | 979 eligible paths = 979 indexed paths |

Both runs ended with zero owned worker processes, zero SQLite integrity errors
and zero pending resolution rows. Owned probes in each real root verified
automatic addition/change bursts/rename, an explicit incremental checkpoint and
deletion catch-up after disconnect. Product files and approved generated-log
exclusions were preserved. File accounting still does not certify semantic
coverage: extension-mismatched controller/include formats and file-level XML/YAML
need current source/text tools. Known false edges remain in upstream results.
File-qualified common constructors can still be missing; graph absence is not
source absence.

The comparison recorded serialized MCP tool-result bytes, including the managed
root/generation/freshness fields. All source oracles found in raw answers remained
recoverable in managed answers; no tokenizer or quota measurement was made.

| Input / calls | Raw results | Managed initial results | Exhaustive retained-detail recovery |
| --- | ---: | ---: | --- |
| Pack / 13 | 42,801 B | 17,211 B | additional 50,519 B / 13 calls |
| Large input / 14 | 60,854 B | 22,015 B | additional 70,894 B / 18 calls |

Initial delivery is smaller for this question set; fetching every detail costs
more bytes overall. Short answers gain metadata overhead, so known-file work
still starts with Serena. The pinned default catalogue in this run was only
1,808 B; the managed stable ten-tool catalogue was 4,548 B and also exposed the
exercised hidden handlers and explicit index/sync/detail operations. Debug-native
frontend startup was 11.324–11.472 s versus 270–840 ms for the raw initialized
worker. Managed query totals were 8.693/25.543 s versus 257/352 ms raw, excluding
detail paging and including first managed worker startup. These observations
include native identity/recovery/broker work and are not release-build latency
promises.

`codegraph_dependency` exercised official acquisition, selection, read-only Check
and rollback through the native CLI. Native failure tests wrote a partial active
database before Windows memory denial, deadline and cancellation; each preserved
the committed checkpoint, reclaimed the worker and refused automatic restart.
Generation tests cover interrupted checkpoint publication, capacity/reserve,
disappearing WAL files, bounded inventory and link-safe cleanup. Ordinary queries
do not snapshot the full database. Owned cache-local ignore files keep private
index data out of ordinary Git staging without changing project ignore rules.

Global installer/consumer acceptance remains separate and open until task 4 is
verified. The main global registration still selects CBM at this checkpoint.
These comparison measurements use the current observer, scheduler and generation
paths. The same test then keeps both real roots and a third owned Rust project
open concurrently, adds another same-root client, and verifies shared backend
identity and automatic addition, change bursts, rename and deletion in every
root. Committed SQLite file and symbol oracles are checked before graph queries;
manual sync also passes. The complete run takes 457.69 seconds. It supersedes
the earlier single-project comparison for concurrent runtime acceptance. Two
source oracles in each root remain absent upstream for ambiguous/common-name
follow-ups; their absence is preserved and does not establish missing source.
Current behavior is documented in the
[candidate guide](../../../docs/code-tools.md#native-codegraph-candidate).

The native concurrent entry-point check now exercises three owned indexed Rust
projects plus another client in the first project, each with a distinct Codex
home. Published 1.6.0 completes automatic add/change bursts/rename/delete in all
three roots, verified by reading committed SQLite generations without graph
queries triggering the updates. The same clients remain open beyond 610 seconds;
subsequent additions commit automatically in every root. Last-client disconnect,
continued updates in another root, reopening catch-up and confirmed broker
retirement pass. The complete test takes 686.27 seconds, with 642.14 seconds
between the first sharing comparison and final retirement. These are native MCP
connections; installed Codex conversation acceptance remains separate.

Read-only Windows process snapshots include explicit frontend owners, the broker
and their descendants. Counts include Windows console hosts. Samples are current
private memory, not peak memory or subscription measurements:

| Connected clients / projects | Owned processes | Node processes | Aggregate private bytes |
| --- | ---: | ---: | ---: |
| 1 / 1 | 8 | 3 | 141,619,200 |
| 2 / 2 | 10 | 3 | 146,567,168 |
| 3 / 3 | 12 | 3 | 149,946,368 |
| 4 / 3 | 14 | 3 | 138,948,608 |

The additional same-root client receives the same backend PID/creation identity.
Each extra frontend adds its own native process and console host without another
Node backend. Independent native fixtures also verify connection control while a
query blocks, EOF cancellation and subsequent reuse, failure isolation without an
automatic restart, bounded/error-preserving replies and 60-second idle retirement.
The source checks live in `codegraph_mcp`, `codegraph_failures` and
`codegraph_observer`; private detailed samples remain outside Git. Native fixtures
force automatic memory denial, last-client cancellation and a real 600-second
hung sync after partial writes. Each preserves the committed checkpoint, releases
admission for another active root, avoids automatic retries and recovers after
deliberate reconnect. The actual deadline check passed in 605.37 seconds. A
source edit during a query produces explicit pending freshness and current-source
fallback. Concurrent large-project acceptance and global installation/consumer
checks remain open.

## Goals / Non-Goals

**Goals:** globally usable graph discovery in several simultaneously open Codex CLI projects within the resource envelope; automatic updates for each active indexed project for the session's lifetime; shared processes and indexes where useful; compact answers with explicit recovery; exact semantic work retained in Serena; reversible CBM retirement.

**Non-Goals:** watching projects after all their sessions close, scanning the machine for projects, an upstream fork, a new parser or graph engine, automatic diagnostic hooks, a common Codex app-server, guaranteed graph completeness for unsupported/custom languages, or promised weekly subscription savings. Do not run the upstream interactive installer to rewrite agent instructions.

## Decisions

### Use the published MCP through the existing native ownership boundary

Adopt the pinned Windows release through dependency discovery and checksum validation. Use its bundled Node and published MCP/index/sync entry points, telemetry off, one parse worker and one resolution worker. Keep `CODEGRAPH_NO_DAEMON=1` when the native broker owns direct-mode processes; an independent detached upstream daemon must not escape the resource boundary. Explicitly remove conflicting inherited watch/debug/worker settings from the owned child environment; preserve unrelated user settings outside it.

Reuse the existing Rust MCP, Job, admission and broker primitives. This change owns the CodeGraph adapter, supervision and executable acceptance tests in Rust; the published CodeGraph implementation and bundled Node stay third-party. Rust migration task 5.4 adopts this implementation and evidence under the [shared ownership/order map](../migrate-harness-to-rust/design.md#graph-provider-ownership-and-order), without developing another CBM or CodeGraph adapter or waiting for complete native installer migration.

Separate project observation from expensive indexing admission. The account-wide native owner tracks connected clients by canonical project root, including across Codex homes, and keeps one observation/state owner for each active indexed root. Admission covers an individual bounded work episode, not a project's entire open session. Coalesce pending source changes per project and serve pending projects fairly; a continuous burst in one project must not repeatedly jump ahead of another. Queued/pending state remains visible until the corresponding update completes. Keep event storage bounded; overflow requires a bounded catch-up scan, not silent loss of changes. Index creation remains explicit, but connecting to an already indexed root registers its observation and catch-up without waiting for the first graph query.

Reuse the published same-project sharing behavior through the native ownership boundary, rather than launch a heavy backend for every CLI. The pinned package already represents daemons per canonical project and shares one writer/watcher among that project's clients; this does not impose a single project per account. Prefer existing native broker ownership and published entry points over another provider implementation. Use lightweight Rust project observation with queued published sync operations where the upstream automatic watcher cannot participate in account-wide admission. In that mode disable the upstream autonomous watcher to avoid duplicate or unscheduled writes, while preserving the required automatic behavior through the native observer. Retain one heavy indexing slot and the shared 2 GiB / 25% CPU allowance, so opening more sessions cannot multiply the indexing budget. Reuse backend processes only while their canonical root, generation and resource ownership remain valid; a process fixed to one root must drain and retire before replacement, never silently answer for another root. Do not require a permanently resident heavy process for each project. Measure the resulting process counts, aggregate memory and refresh/query latency before claiming reuse benefits.

Client disconnect releases only that client's reference. Keep a project's observation while any client remains; retire it and unneeded resources within 60 seconds after its last client closes. Other projects continue to run. Reopening triggers bounded catch-up from the preserved committed index. The owner reconciles dead clients and owned descendants using the existing broker primitives, without treating an ambiguous foreign process as its own. This scope does not introduce an always-on watcher for closed projects.

The [Rust ownership requirement](specs/global-code-tools/spec.md#requirement-rust-owned-codegraph-integration) also covers refresh coordination, resource/storage enforcement, catalogue/response shaping and new provider dependency/registration/recovery logic. Existing transitional lifecycle entry points may dispatch to native commands; they must not acquire new CodeGraph-specific logic in another language. Inspect generated/embedded programs and the actual process chain during acceptance so a Rust launcher around an owned script cannot satisfy this requirement. Declarative manifests and inert analysis samples remain data.

The 2026-09-11 implementation review verifies native ownership through the actual manager → Job-owned bundled Node → published CodeGraph 1.6.0 process chain. Native dependency discovery accepts the selected catalogue without duplicate providers; ten default discovery tests pass, including retained CBM compatibility. The [Rust ownership evidence](../../../docs/evidence/rust-migration.md#codegraph-ownership-review-replacement-26--41) records the native command/test owners and allowed transitional dispatch. Rust task 5.4 adopts that owner and remains open for its broader native lifecycle work.

A 600-second limit applies to an individual indexing episode, including automatic work; ordinary reads use a 30-second deadline and deliberate exploration at most 60 seconds. Prefer published finite sync processes with explicit completion, or verified pending/start/completion signals if reusing a resident backend. A finite process lease may bound ownership, but healthy retirement/replacement must be automatic and preserve queued changes, observation and continued service. Session age alone must not latch a failed state or require manual renewal. Never extend an in-flight operation's deadline on each new file event. Actual memory/time failures retire the affected work, preserve that project's committed generation and stop its automatic restart loop while releasing admission for other projects. A new deliberate request or verified changed condition can resume failed work.

### Keep the useful small tools visible and enforce budgets

Expose a small stable managed catalogue: project/index status, explicit index/sync, search, callers/callees, shallow impact, optional node detail and deliberate explore. The wrapper's names/schemas must describe actual supported arguments and limits. Upstream hides most tools below 500 files even with an allowlist; the owned fixture confirmed that a hidden caller handler still works. Validate each forwarded handler at installation/probe time rather than inventing tools from descriptions.

Default limits: five matches, depth one, symbol bodies off, 4 KiB serialized result including metadata. Expose `max_response_bytes` with a hard maximum of 16 KiB; reject invalid, non-finite or excessive inputs. Exploration requires a named question and explicit `maxFiles` (one or two initially). Source retrieval through node must distinguish symbol mode, file window and symbols-only mode. Prefer Serena for bodies and edits. Reject or explicitly mark qualifier fallback; never return another definition as the requested file.

Capture upstream results before returning them to a model. Preserve complete small records and required error/coverage fields; avoid arbitrary cuts inside code or JSON. When a result cannot fit, return a bounded partial/refusal with `truncated`, known omitted counts, root/generation, and an owned detail identifier. Retain at most 32 responses, 256 KiB each, 8 MiB total, with 30-minute expiry; detail pages use the same response cap. Stream capture must itself be bounded. Do not duplicate full source in text and structuredContent. Oversized warnings/errors are summarized with their original category and a detail identifier, never converted to success.

Replace the upstream initialization advice that recommends broad `explore` for every read with compact accurate instructions for the managed catalogue, refresh and candidate-edge limits. Disable cross-client explore deduplication (`CODEGRAPH_EXPLORE_DEDUP=0`) unless it is proven request/client scoped; source previously sent to another agent must not disappear from this agent's answer. Reuse results in the agent's actual context instead.

The published native-entry acceptance exercises unindexed status, deliberate
index/sync, search, callers, callees, impact, symbol body, file overview and
explicit exploration in a small owned Rust repository. Correct file qualifiers
are retained and a mismatched qualifier fails explicitly. Core catalogue/response
checks cover numeric ranges, Unicode, whole-record/source-line boundaries,
warnings/errors, capture truncation, duplicate payloads and retention expiry.
Managed stdio checks retain oversized original answers through bounded detail
pages, reject another client's identifier and prove pagination leaves the
upstream query counter unchanged. Protocol and ambiguity fixtures retain explicit
bounded failures. The real-root comparative and installed-consumer checks remain
separate acceptance requirements.

### Preserve indexes and disk space

Use an owned project data directory such as `.codegraph-harness`, with ownership metadata and native ignore support. The upstream `CODEGRAPH_DIR` accepts a directory name, not an arbitrary absolute cache path. Keep runtime/evidence placement explicit and configurable; a system drive low on space must not receive new trial build trees. Initial storage defaults: 1 GiB for active database/journals, 2 GiB total per-project including recovery/staging, and a 5 GiB free-space reserve. Those values are proposed bounds, not limits already provided by upstream.

Stage full replacement indexes and validate coverage before replacing a readable generation; native `index` otherwise recreates the database destructively. For automatic incremental failure, verify actual transaction/recovery behavior and retain a usable committed generation or bounded recovery copy. A partial result may remain diagnostically readable but cannot be relabelled a completed refresh. Copying the entire database per query is excluded. Reclaim only owned obsolete generations, and bound WAL/log growth. Retained CBM indexes are preserved separately for rollback.

The published 1.6.0 extraction implementation deletes prior file data before storing the replacement and uses multiple transactions across a refresh; database consistency alone therefore cannot prove preservation of the previous graph. The native candidate uses the Windows-provided WinSQLite3 backup API for consistent recovery snapshots, without installing another runtime or modifying CodeGraph. Ten `codegraph_generation` checks verify failed full staging, partial active writes, interrupted publication, bounded restore, storage pressure and link-safe cleanup. The native transport and automatic-refresh failure checks verify reclamation and preserved committed bytes after memory denial, deadline and cancellation. The published native-entry check also verifies repeated warm queries reuse the worker, leave active/committed database modification times unchanged and create no rebuild stage. Limits are exercised with small owned allowances rather than filling a drive. Private response storage has bounded count, bytes, page size and expiry checks in `codegraph_response` and `codegraph_mcp`; global installation remains separately required.

### Use source coverage and question-specific oracles

The canonical recipes live in [code retrieval](../../../.agents/skills/token-efficient-workflow/references/code-retrieval.md). Global principles and the installed skill now route there and distinguish planned CodeGraph behavior from live CBM configuration. Auto-refresh does not justify repeated status/sync calls when a completed relevant update is already established. During debounce, failed refresh or unsupported coverage, use scoped current source.

Compare exact-location/body tasks, direct calls, impact candidates and full language references as different questions. Do not certify a graph edge from name matching alone. For an exhaustive or edit-sensitive conclusion, include necessary source/Serena follow-ups in the cost and correctness check. Status file counts and successful CLI exit are insufficient coverage oracles.

## Risks / Trade-offs

- Broad responses and overloaded names → hard delivered-byte caps, explicit incompleteness and scoped follow-up; instructions alone are not enforcement.
- Approximate graph edges and file-qualifier fallback → verify consequential relationships and preserve an ambiguous/unsupported result.
- Automatic sync races or resource failure → one owned indexing slot, bounded work episodes, per-project pending/failure state and crash/recovery checks; no restart loop or global failure latch for one project's error.
- Several projects compete for memory and service → shared same-project resources, lightweight observation, fair bounded scheduling and actual process/memory measurements; do not reserve admission for the first client's lifetime or multiply heavy workers per agent.
- Observation and worker replacement can lose changes → retain coalesced pending state through healthy handoff, perform bounded catch-up on overflow/reopen, and test edits across the former 600-second lease and disconnect boundaries.
- Different parsing coverage and the upstream 1 MiB file ceiling → inventory relied-on maintained files, inspect representative source/query results and retain appropriate text/semantic fallback.
- Disk growth and destructive upstream rebuild → staged full index, bounded recovery/storage and explicit ownership cleanup.
- Concurrent public-pack maintenance → use fresh owning files, keep private evidence outside Git and preserve unrelated edits.

## Migration Plan

1. Complete the bounded native boundary and reusable dependency/registration sources in an isolated accepted implementation change. Keep the current provider while checks run.
2. Exercise the real large repository plus owned edit/crash/cancel/high-fan-out fixtures. Keep at least three distinct indexed projects open concurrently, add a second client to one project, and include different Codex homes. Verify automatic refresh in every root, no duplicate heavy same-root worker, fair service under bursts, failure isolation, independent disconnect/reopen and continued automatic work after 600 seconds through the actual managed MCP. Reuse earlier source-oracle results only where the changed scheduling/generation inputs do not invalidate them.
3. Apply the already reconciled Rust dependency/ownership map and recheck it against changed runtime inputs. Carry retained CBM source oracles into CodeGraph checks and verify selected dependency/registration state; preserve unresolved acceptance and avoid rewriting closed archives as if they had used CodeGraph. Broader workflow comparisons stay in their owning change.
4. Activate through the existing recoverable global lifecycle; remove only the owned CBM registration once the replacement passes. Preserve packages/indexes and later user edits. Verify outside the checkout and document the new-session boundary.
5. On failed activation, Recover restores the coherent prior selection. Automatic refresh remains off for restored CBM. Disconnect retires only owned CodeGraph workers and registrations.

## Primary Contracts

Pin source review to the tested release: [tool schemas, handlers and budgets](https://github.com/colbymchenry/codegraph/blob/dfccdf62547fcd76d343344d823a0e1998d3a89f/src/mcp/tools.ts), [MCP session and catch-up](https://github.com/colbymchenry/codegraph/blob/dfccdf62547fcd76d343344d823a0e1998d3a89f/src/mcp/session.ts), [watch policy](https://github.com/colbymchenry/codegraph/blob/dfccdf62547fcd76d343344d823a0e1998d3a89f/src/sync/watch-policy.ts), [initialization guidance](https://github.com/colbymchenry/codegraph/blob/dfccdf62547fcd76d343344d823a0e1998d3a89f/src/mcp/server-instructions.ts), [directory contract](https://github.com/colbymchenry/codegraph/blob/dfccdf62547fcd76d343344d823a0e1998d3a89f/src/directory.ts), [extraction](https://github.com/colbymchenry/codegraph/blob/dfccdf62547fcd76d343344d823a0e1998d3a89f/src/extraction/index.ts). The observed executable takes precedence over stale documentation defaults.
