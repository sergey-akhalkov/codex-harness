# Design

## Context

See [proposal.md](proposal.md) for motivation and the [delta](specs/executor-mcp-plugin/spec.md) for the behavior contract. This is planning only; implementation and installed acceptance remain undone.

Source inspection established the following reuse points:

- [executor_mcp.rs](../../../crates/codex-harness/src/executor_mcp.rs): `DEFAULT_WAIT_MS` is 30,000, `MAX_WAIT_MS` is 55,000, and `wait` observes retained state at bounded intervals. `observe_executor` checks the accepted turn and reads the current native result, so completion before a call already works for ordinary positive timeouts. `wait_base` embeds `run_summary`, causing unchanged timeout payloads to repeat the run card.
- [mcp_session.rs](../../../crates/harness-core/src/mcp_session.rs) creates operations with a 60-second deadline. [mcp_stdio.rs](../../../crates/harness-core/src/mcp_stdio.rs) owns operation execution, cancellation and connection cleanup. Their budgets must remain coherent with the longer observation.
- [executor_mcp tests](../../../crates/codex-harness/tests/executor_mcp.rs) already cover completion between calls, repeated result reads, stale turns, pending questions, observation failures and authority. Extend these fixtures instead of adding a second harness.
- [the plugin manifest](../../../.agents/plugins/executors/.mcp.json) currently declares only the command and arguments. [OpenAI MCP documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli) documents a 60-second default and `tool_timeout_sec` for configured servers. The installed plugin route is verified separately below for the target `codex-cli` 0.164.0.

The everyday sequence is dispatch, useful independent lead work, then one wait across owned runs. A worker can finish before that wait, several can finish together, and a question can arrive from a different worker. A connection may disappear after the server writes a result. These cases determine the design, not merely the nominal timeout.

## Goals / Non-Goals

**Goals:** extend the existing MCP operation; reuse native history, run receipts, authority checks and lifecycle owners; reduce model-visible empty exchanges while keeping result recovery and cancellation explicit.

**Non-Goals:** a new service, event bus, durable event journal, scheduler, CLI fallback, model change, provider change, broad MCP concurrency rewrite, or general acknowledgement framework. Replacing the existing internal polling with push notifications is unnecessary for the requested token-facing improvement. No token or quota percentage is an acceptance promise.

## Decisions

### Keep one MCP request pending

Set the ordinary wait bound to 1,800,000 ms and support shorter explicit waits. Coordinate three budgets: the observation bound, a harness operation deadline that also permits bounded setup/response work, and a client timeout with sufficient outer margin. Extend the operation policy for this tool without turning every MCP operation into a 30-minute request. Polling native state inside that operation does not call the model; unchanged observations stay inside the tool.

The isolated probe below established how the installed Codex plugin client obtains its effective timeout, including a call that crossed the former 60-second boundary; syntactic acceptance of a setting was not treated as sufficient. The full 1,800-second interval through the implemented, installed executor plugin remains task 3.3. If that later exercise cannot use the existing lifecycle, stop dependent work and report the capability blocker. This plan does not authorize replacing the plugin, patching Codex, adding a service or silently shortening waits; such a finding needs a separate scoped decision.

Client yields that retain the same operation are acceptable; repeated model-issued MCP calls to renew 55-second waits do not meet the contract. Progress notifications are not assumed to extend client deadlines. Keep existing read/cancellation bounds, and verify that neither a shared connection nor receipt ownership prevents questions or cancellation from being processed during a wait. Change scheduling only if that concrete path requires it.

### Verified installed plugin timeout (Codex 0.164.0)

The installed `codex-cli` 0.164.0 route takes the MCP call timeout from the plugin server's own `.mcp.json` entry: `mcpServers.<name>.tool_timeout_sec`. The ordinary `[mcp_servers.*]` configuration is not the owner for plugin servers, and app-server supplies no per-call timeout. In this version an omitted value defaults to 300 seconds (`DEFAULT_TOOL_TIMEOUT`), not 60 seconds. The inspected Codex path applies no cap to the configured value.

Source evidence from upstream tag `rust-v0.164.0` (commit `6aaac694803004a63c6f393d153c96d22521a6b9`), matching installed `@openai/codex` 0.164.0:

- `codex-rs/core-plugins/src/loader.rs` discovers the legacy plugin's `.mcp.json` (`DEFAULT_MCP_CONFIG_FILE`) and parses it with `parse_plugin_mcp_config`.
- `codex-rs/codex-mcp/src/plugin_config.rs` deserializes the server declaration into `McpServerConfig`; `codex-rs/config/src/mcp_types.rs` owns `tool_timeout_sec: Option<Duration>`.
- `codex-rs/codex-mcp/src/connection_manager.rs` resolves `tool_timeout_sec` with `DEFAULT_TOOL_TIMEOUT` and passes the effective value into `tools/call`; the default is 300 seconds in `codex-rs/codex-mcp/src/rmcp_client.rs`. `codex-rs/app-server/src/request_processors/mcp_processor.rs` routes `mcpServer/tool/call` through that path.

Observed through the installed app-server rather than a standalone client. A fresh `%TEMP%\codex-mcp-timeout-probe-<run>` root held an isolated `CODEX_HOME` and a local-marketplace plugin at `marketplace-root\plugins\probe`; the real `CODEX_HOME` and installed plugins were not used. The plugin declared `"tool_timeout_sec":90` and served one `sleep(seconds)` stdio tool. `codex mcp list --json` reported `"tool_timeout_sec": 90.0` for `probe_sleep`. The probe then ran `codex app-server --stdio` and this JSON-RPC sequence: `initialize` with experimental API, `initialized`, `thread/start` (ephemeral), `mcpServerStatus/list`, `mcpServer/tool/call` for `probe_sleep/sleep`.

- `sleep(65)` returned `isError:false` with `sleptSeconds:65` after 65,004 ms.
- `sleep(95)` returned `timed out awaiting tools/call after 90s` after 90,019 ms.

This verifies >60-second plugin calls and that the plugin-declared value is the effective client boundary. For the 1,800,000 ms wait, the executor plugin must set `tool_timeout_sec` above the harness operation deadline; the 300-second default cannot cover it. The exact outer margin belongs to task 1.2, and a full 1,800-second installed wait remains task 3.3, not established by this probe.

### Read current state before sleeping

Reuse the existing caller/run verification and current-turn observer. Omitted `run` selects that lead's unreleased runs; it does not infer ownership from cwd, focus or recency. Explicit `run` retains a repeatable targeted read. Refresh state before blocking and after each bounded observation interval. Apply this initial observation even to a zero-duration nonblocking request; the current code's zero-duration shortcut skips native reading and must not define the new contract.

Return a bounded batch of ready outcomes when several are available, using existing terminal classifications and exact question identities. Prioritize actionable questions while retaining simultaneously available terminal results. Unreturned results remain unacknowledged. When no work or unacknowledged outcomes remain, return `idle` immediately. A failed native read is an observation failure, never evidence that the set is empty.

A pending unanswered question remains actionable until a real reply resolves it. The lead must handle that question before resuming an otherwise empty wait; result acknowledgement cannot hide it. A result from an earlier turn must not settle newer work started by steering or continuation.

### Acknowledge receipt on the next request

Add an optional list of received result identifiers to `executor_wait`. Return the identifiers with terminal outcomes so callers can pass them back without reconstructing runtime addresses. This acknowledges receipt only, not code review, acceptance, release or question resolution.

Construct each stable identifier from the exact run/native thread/turn and terminal outcome revision using existing identity helpers. Store only the acknowledgement needed for the current result in the existing run receipt lifecycle, bound to the originating lead. Do not maintain a copied transcript or a separate completion queue. Reconcile with native current state on every read; a replacement run or later turn cannot inherit an old acknowledgement.

Validate all identifiers and caller authority before acknowledgement effects. Persist valid acknowledgements through the existing safe receipt-update owner before waiting for further results. Retrying an acknowledgement is harmless. Never acknowledge because a response was serialized, written or a connection closed. If response A was lost, its unacknowledged identity is returned again; if A was acknowledged and response B was lost, B remains recoverable. Explicit `run` permits rereading an acknowledged current result.

The rejected alternative is marking a result consumed while sending it: that loses information on transport failure. Keeping no receipt acknowledgement at all would make one finished worker repeatedly wake all-run waits until acceptance/release, coupling observation to unrelated review work. A global event log would duplicate native history without a required consumer.

### Keep quiet responses small

Build timeout and idle envelopes independently of `wait_base`/`run_summary`; retain only state and the necessary wait scope/bound. A default timeout can be as small as `{"state":"timeout","timeoutMs":1800000}`. A targeted timeout also identifies its run. Do not copy dispatch metadata, paths, profiles, null placeholders or lifecycle records into every timeout.

Event responses retain run and result/request identities, existing outcome distinctions, bounded result/error evidence and the native detail locator. Keep human-readable content and structured output semantically aligned; measure the actual model-visible representation rather than assuming either is invisible or counted twice. Preserve the existing native-record and full-detail access contracts.

### Update the ordinary lead path

Update the tool schema/descriptions and existing guidance in [global lead role](../../../global/roles/lead.md), [team-lead](../../../.agents/skills/team-lead/SKILL.md), [session reuse](../../../.agents/skills/token-efficient-workflow/references/session-reuse.md), [delegation guide](../../../docs/agent-delegation.md) and [token workflow](../../../docs/token-workflow.md) where they specify one exact run or short renewals. Retain independent-work-first guidance, event-driven intervention and the existing maximum supervision frequency; that frequency is not a mandatory timer that interrupts a healthy long wait. The user supplies intent, not recurring endpoint or receipt bookkeeping.

## Risks / Trade-offs

- Plugin timeout support is verified for 0.164.0 through an isolated fixture; the full 1,800-second installed exercise remains task 3.3. The plugin entry must set `tool_timeout_sec` above the operation bound because the 300-second default is too short.
- A long operation can expose serialized connection starvation: exercise executor questions and cancellation through the actual installed communication path before delivery.
- Delivery can be duplicated after a lost response: stable result identity and idempotent acknowledgement favor safe replay over loss. This is not exactly-once delivery.
- Default scope, timeout and response shape change: update owned callers, fixtures and guidance together, preserving explicit run authority and existing terminal/error distinctions.
- Native history or a receipt can become unavailable: report the observation failure and keep partial work, instead of returning an old success or normal timeout.
- Fewer calls do not quantify subscription savings: compare model-visible wait calls and serialized quiet-response sizes for equivalent scopes and durations. Use existing usage accounting only if making token claims.

## Migration Plan

1. Use the confirmed installed plugin timeout mechanism and settle the required deadline margins; preserve the previous supported installation and unresolved work.
2. Implement the scoped operation budget, current-state all-run observation, receipt acknowledgement and compact envelopes with existing native fixtures. Keep unrelated tool deadlines intact.
3. Update the installed plugin and owned guidance through the normal kit lifecycle. Start a fresh consumer session outside the checkout and verify the effective artifact, not just source configuration.
4. Exercise early/late completion, multiple results, a question during waiting, cancellation, reconnect/retry and one full 1800-second quiet wait. Use bounded owned fixture work for elapsed waiting rather than artificial model generation. Reconcile actual result identities and call counts; retain private raw evidence locally.
5. On delivery failure, use the existing rollback owner without stopping healthy executors or discarding receipts/checkouts. Preserve native results and work; an older observer may show an already received result again, which is safer than losing it. Check receipt compatibility before rollback. Do not mark acceptance tasks complete until the installed consumer path passes.
