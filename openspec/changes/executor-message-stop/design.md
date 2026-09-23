## Context

Pooled exec dispatch currently runs `codex exec --json` behind the executor tab host (`executor run --file`): the host renders the event stream, records the lifecycle receipt beside the exact session identity, keeps the launcher tree inside a host-owned Windows Job, and records the slot lease for exactly the session's lifetime. Verified facts that constrain the design:

- `codex exec` has no inbound-context capability (prompt comes from argv/stdin at start only), so no wrapper can deliver a live correction to it.
- The native app-server route does: the kit's task-control work already verified `turn/start` from a second client on the same thread, `turn/interrupt`, thread naming/resume and event observation on real Codex CLI builds; `codex app-server` accepts `--listen ws://IP:PORT --ws-auth capability-token --ws-token-file PATH` and config overrides via `-c`, but has no `-p/--profile` flag.
- The dispatch receipt already records the resolved profile binding (model, provider, reasoning effort), slot binding, terminal window snapshot, host identity and lifecycle states; `executor steer` exists but requires a hand-supplied task-control state directory, which pooled exec sessions do not have.

### Verified baseline (2026-09-23)

Ran the opt-in contract checks against the installed native CLI (`codex-cli 0.156.1`, native binary sha256 `70bcb05f9bf1a4e7306edd0cd1b57d02af3267ad02a34b26f45c8c4bb20a3301`), model-free with the owned synthetic provider:

```powershell
cargo test --locked -p codex-harness --test task_control_contract native_two_clients_reconnect_tool_result_and_tui --jobs 1 -- --ignored --exact --nocapture --test-threads=1
cargo test --locked -p codex-harness --test task_control_contract native_executor_control --jobs 1 -- --ignored --nocapture --test-threads=1
```

The second command runs the two probes added with this change (`native_executor_control_active_turn_and_tool_interrupt`, `native_executor_control_generation_interrupt`); all three checks passed and retain raw responses, provider requests, terminal inventory and item records in their printed private evidence root. Findings:

- **Second-client `turn/start` on an active thread**: accepted without error, and the response carries the already-active turn (same turn id, status `inProgress`) rather than a new turn. When the turn continues, the input reaches the model in that turn's next provider request (observed). When the turn is interrupted first, the accepted input never reaches the model, no follow-up turn is created, and the sender still received a success response - transport acceptance is not delivery, so `message` must classify delivery from observed conversation evidence, not from the start response.
- **`turn/interrupt` during a tool call**: accepted; the turn completes with status `interrupted` (observed 195-439 ms after the request across probe runs). The running child command is not terminated: it stays listed by `thread/backgroundTerminals/list`, runs to natural completion (observed exit code 0 after the command's full duration, 3.1-5.5 s under concurrent load) and then completes its item against the already-interrupted turn, while the model-visible history records the interruption as `aborted by user` with the elapsed wall time. Ending owned work therefore remains the stop path's tree termination, not interrupt alone.
- **`turn/interrupt` during generation** (provider request in flight against a hung owned provider): accepted; the turn completes `interrupted` (observed 68-188 ms); no tool item is created.
- **`thread/start` config overrides**: explicit `model`/`modelProvider` and `config` map overrides are honored; `config: {"model_reasoning_effort": "high"}` is reflected in the start response (`reasoningEffort`) and in the first provider request, and the same map carries per-thread provider route overrides (already used by the existing checks).
- **Final message from thread items**: `thread/read` with `includeTurns` returns the completed turn's final assistant message (existing check and both probes).

Protocol differences from the documented 0.154.0 baseline observed on 0.156.1: full-history `thread/read includeTurns` now emits a deprecation notice pointing at paginated `thread/turns/list` and `thread/items/list` reads (it still returns the final message); the surface additionally exposes a per-turn `effort` field, `reasoningEffort` in the thread start response, a dedicated active-turn steering call (`turn/steer`, carrying `expectedTurnId`), and a thread input queue (`thread/queue/add|start|list|...`). Everything the 0.154.0 notes record for this route (second clients, reconnect, native child survival, route overrides, background terminal inventory, naming/resume, `-c` overrides, no `-p/--profile`) still holds in the re-run checks.

## Goals / Non-Goals

Goals:

- Make `executor message` and `executor stop` work end-to-end for real pooled exec runs, installed and verified through the real entry points.
- Preserve every existing executor guarantee: pool/slot isolation, synchronized base, lease, visible titled tab, readable rendering, honest lifecycle receipt, bounded result/detail/stderr records, resume-by-exact-session, explicit release.
- Keep using the existing owners: dispatch receipt, kit-local task state, task-control connection code, terminal-surface owner, process Job ownership, CLI usage/help, delegation guide and team-lead skill.

Non-Goals:

- No new executor-management system, board, report protocol or proof runner.
- No interactive-TUI message injection (TUI automation stays forbidden); `tui` mode and raw `executor run LAUNCHER` keep their current semantics and honest unsupported results.
- No change to profile selection, model/provider/effort routing, billing or quota handling; no activation of the unfinished lead task-control controller (view layouts, quota gateway, succession) for executors.
- No automatic slot release, base resynchronization or task restart after stop.

## Decisions

1. **Control-backed exec conversations.** The pooled exec-mode tab host starts one `codex app-server` child per run (loopback WebSocket, capability-token file under the run's kit-local state directory), initializes it with the experimental API, starts the conversation thread with `cwd` at the bound slot and config overrides derived from the already-resolved profile binding, names the thread with the assignment title, submits the assignment through `turn/start`, renders the thread events on the existing tab surface, and records completion/final message from the thread items. The endpoint (port, token, thread id) is recorded in kit-local state beside the dispatch receipt, and the existing receipt keeps recording session identity, model/provider/effort, states and result locators. The app-server child runs inside the same host-owned Job with the executor session environment (agent-tool disablement, prepared shell PATH), so process ownership is unchanged.
   - *Alternative: keep `codex exec`.* Rejected: verified absence of inbound input; a CLI wrapper would mask a missing capability, which the change explicitly forbids.
   - *Alternative: activate the lead task-control controller for executors.* Rejected: it couples executor dispatch to unfinished lead-oriented machinery (view placement, provider gateway, quota handoff) - a larger change than the capability requires.
   - *Alternative: write pending steering to a file for the next run.* Rejected: explicitly not model delivery.
2. **Message command.** `executor message` resolves the dispatch receipt from `--source/--codex-home/--slot/--owner` (with optional exact `--session`), verifies the recorded binding, live lease and run state, then connects to the run's recorded endpoint and issues `turn/start` on the same thread with the literal text from `--text` or a UTF-8 `--file`. The response is classified as queued, delivered or error and recorded with its request/turn identity in kit-local state so an indeterminate retry cannot silently double-deliver; the tab shows the message because the host renders all thread events. Completed/stopped/unavailable runs get an explicit state plus the exact-session resume remedy; no new conversation is started. The existing `executor steer` payload path is reused for the protocol call and retired or re-pointed only if the new addressing makes it redundant - no parallel command remains.
3. **Stop command.** `executor stop` verifies the same addressing and identity, then: (a) requests native interruption through the run's endpoint when one exists (`turn/interrupt`, bounded wait); (b) terminates the recorded host process tree - host death closes its Job and reaps the owned tree, and survivors are bounded-terminated and verified by recorded process identity, never by name or title; (c) verifies that exactly the stopped run's tab closed using the receipt's recorded window/tab identity through the terminal-surface owner; (d) writes a stop record to the receipt: stopped / already-completed / partial / error, timestamps, measured duration, unknown exit codes kept unknown, pending messages marked undelivered. No reset, clean, release or completion claim; a repeated stop reports current state; the race with natural completion is resolved by re-reading the receipt state at commit time under the existing receipt lock.
4. **Instructions and delivery.** CLI usage/help, `docs/rust-native.md`, `docs/agent-delegation.md` and the `team-lead` skill are updated in place as the owning documents, including the selection rules (message for concrete corrections of continuing work; no status-only nudges or repeats without new facts; a brief error alone does not justify stop; stop only for explicit cancellation or concrete necessity; waiting alone is not stop; standard commands before manual process killing, which itself requires an established cause; preserve partial work after stop). Delivery follows the existing immutable-build installation lifecycle; the installed launcher's help and version must agree with the source.

## Risks / Trade-offs

- [App-server contract drift between CLI builds] → Run the existing opt-in task-control contract check against the installed CLI before implementation lands, and keep message/stop fail-closed on protocol mismatch instead of improvising.
- [`thread/start` config may not reproduce the full profile binding] → Derive overrides from the resolved binding the receipt already records, verify the started thread's reported model/provider/effort against it, and fail dispatch on mismatch rather than silently changing routing.
- [Terminal tab may not close on host termination] → Verify in acceptance through the terminal-surface owner; if host exit alone leaves the tab, close that exact tab through the recorded window/tab identity, never the window.
- [External stop cannot close another process's Job handle] → Host termination must close the Job (kill-on-close); owned-tree survival is detected by recorded identity and reported as partial stop with next action, never as success.
- [Active-turn input semantics may queue, reject or differ by build] → Classify the actual response honestly (queued vs delivered vs error) and never present transport acceptance as the executor having applied the correction.
- [Larger exec-mode change surface] → The control backend keeps the existing receipt writer, renderer responsibilities, result/detail/stderr files, lease and Job structure; fixture-based tests cover identity, races, lifecycle and process-tree behavior before live acceptance.

## Migration Plan

1. Land fixture-covered implementation slices behind the existing executor CLI; keep legacy receipts readable (runs without a control endpoint report message as unsupported with the resume remedy, and stop still works through recorded host identity).
2. Run the opt-in contract check against the installed Codex CLI, then real dispatch acceptance on owned synthetic assignments with the configured executor profile: message delivered and used in the same session, stop during generation and during a child command with measured stop time and bounds, files preserved, lifecycle honest, repeated stop and completion races correct.
3. Update instruction owners, build and install through the normal immutable lifecycle without interrupting other active conversations, and verify installed help/version/instructions agreement. Rollback is the previous immutable build; pool state and receipts are untouched by rollback.

## Open Questions

- None blocking. Exact state-file names, event-rendering granularity and whether `executor steer` remains as a thin alias are implementation details settled by the slices against the verified protocol.
