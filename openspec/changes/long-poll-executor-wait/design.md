# Design

## Context

See [proposal.md](proposal.md) for motivation and the [delta](specs/executor-mcp-plugin/spec.md) for the behavior contract. This is planning only; implementation and installed acceptance remain undone.

Source inspection established the following reuse points:

- [executor_mcp.rs](../../../crates/codex-harness/src/executor_mcp.rs): `DEFAULT_WAIT_MS` is 30,000, `MAX_WAIT_MS` is 55,000, and `wait` observes retained state at bounded intervals. `observe_executor` checks the accepted turn and reads the current native result, so completion before a call already works for ordinary positive timeouts. `wait_base` embeds `run_summary`, causing unchanged timeout payloads to repeat the run card.
- [mcp_session.rs](../../../crates/harness-core/src/mcp_session.rs) creates operations with a 60-second deadline. [mcp_stdio.rs](../../../crates/harness-core/src/mcp_stdio.rs) owns operation execution, cancellation and connection cleanup. Their budgets must remain coherent with the longer observation.
- [executor_mcp tests](../../../crates/codex-harness/tests/executor_mcp.rs) already cover completion between calls, repeated result reads, stale turns, pending questions, observation failures and authority. Extend these fixtures instead of adding a second harness.
- [the plugin manifest](../../../.agents/plugins/executors/.mcp.json) currently declares only the command and arguments. [OpenAI MCP documentation](https://learn.chatgpt.com/docs/extend/mcp?surface=cli) documents a 60-second default and `tool_timeout_sec` for configured servers, but does not establish that the installed plugin route accepts the same setting. The inspected CLI is 0.164.0; verify the effective target version during implementation.

The everyday sequence is dispatch, useful independent lead work, then one wait across owned runs. A worker can finish before that wait, several can finish together, and a question can arrive from a different worker. A connection may disappear after the server writes a result. These cases determine the design, not merely the nominal timeout.

## Goals / Non-Goals

**Goals:** extend the existing MCP operation; reuse native history, run receipts, authority checks and lifecycle owners; reduce model-visible empty exchanges while keeping result recovery and cancellation explicit.

**Non-Goals:** a new service, event bus, durable event journal, scheduler, CLI fallback, model change, provider change, broad MCP concurrency rewrite, or general acknowledgement framework. Replacing the existing internal polling with push notifications is unnecessary for the requested token-facing improvement. No token or quota percentage is an acceptance promise.

## Decisions

### Keep one MCP request pending

Set the ordinary wait bound to 1,800,000 ms and support shorter explicit waits. Coordinate three budgets: the observation bound, a harness operation deadline that also permits bounded setup/response work, and a client timeout with sufficient outer margin. Extend the operation policy for this tool without turning every MCP operation into a 30-minute request. Polling native state inside that operation does not call the model; unchanged observations stay inside the tool.

First establish how the actual Codex plugin client obtains its effective timeout, using the installed schema/source and a focused call that crosses the current 60-second boundary. Use an isolated owned fixture plugin through the same installed discovery/transport route for this probe, adapting the existing native MCP fixture only as needed: the unchanged executor wait itself rejects bounds above 55 seconds. A configuration value being accepted syntactically is insufficient. Then exercise the full default interval through the implemented, installed executor plugin. The implementation must stop dependent work and report a capability blocker if the plugin client cannot support it through the existing lifecycle. This plan does not authorize replacing the plugin, patching Codex, adding a service or silently shortening waits. Such a finding needs a separate scoped decision.

Client yields that retain the same operation are acceptable; repeated model-issued MCP calls to renew 55-second waits do not meet the contract. Progress notifications are not assumed to extend client deadlines. Keep existing read/cancellation bounds, and verify that neither a shared connection nor receipt ownership prevents questions or cancellation from being processed during a wait. Change scheduling only if that concrete path requires it.

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

- Plugin timeout support is unverified: settle the exact configured runtime boundary first; refuse an unsupported route rather than hiding 55-second renewal loops.
- A long operation can expose serialized connection starvation: exercise executor questions and cancellation through the actual installed communication path before delivery.
- Delivery can be duplicated after a lost response: stable result identity and idempotent acknowledgement favor safe replay over loss. This is not exactly-once delivery.
- Default scope, timeout and response shape change: update owned callers, fixtures and guidance together, preserving explicit run authority and existing terminal/error distinctions.
- Native history or a receipt can become unavailable: report the observation failure and keep partial work, instead of returning an old success or normal timeout.
- Fewer calls do not quantify subscription savings: compare model-visible wait calls and serialized quiet-response sizes for equivalent scopes and durations. Use existing usage accounting only if making token claims.

## Migration Plan

1. Confirm the installed plugin timeout mechanism and the required deadline margins; preserve the previous supported installation and unresolved work.
2. Implement the scoped operation budget, current-state all-run observation, receipt acknowledgement and compact envelopes with existing native fixtures. Keep unrelated tool deadlines intact.
3. Update the installed plugin and owned guidance through the normal kit lifecycle. Start a fresh consumer session outside the checkout and verify the effective artifact, not just source configuration.
4. Exercise early/late completion, multiple results, a question during waiting, cancellation, reconnect/retry and one full 1800-second quiet wait. Use bounded owned fixture work for elapsed waiting rather than artificial model generation. Reconcile actual result identities and call counts; retain private raw evidence locally.
5. On delivery failure, use the existing rollback owner without stopping healthy executors or discarding receipts/checkouts. Preserve native results and work; an older observer may show an already received result again, which is safer than losing it. Check receipt compatibility before rollback. Do not mark acceptance tasks complete until the installed consumer path passes.
