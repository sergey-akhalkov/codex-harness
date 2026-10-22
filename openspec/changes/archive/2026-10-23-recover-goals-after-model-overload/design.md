# Design

## Context

See [proposal.md](proposal.md) for motivation and the [goal recovery contract](specs/goal-recovery/spec.md) for observable requirements. This is a cross-cutting native launch, control-ordering and lifecycle change, so an explicit design is required.

The ordinary launcher currently starts upstream Codex once and waits in an owned Windows Job. Its registration has `task_control: false`. The separate task controller already hosts app-server/TUI connections, but also creates and arranges windows and coordinates handoffs. Enabling it wholesale would change everyday use and violate the current terminal constraints.

The installed Codex 0.163.0 protocol has `thread/goal/get`, `thread/goal/set`, turn notifications and the same remote TUI used by existing harness hosts. `thread/goal/set` can update only status with `origin: automatic`; it has no conditional expected-goal/turn revision field. Ordinary `Stop` hooks run on the successful sampling path, not on the terminal overload path. A `SessionStart` hook could launch a watcher but would not provide authoritative control ordering or lifecycle ownership.

### Discovery evidence and limits

On 2026-10-19 an isolated Rust probe used the installed harness launcher, its registered Codex 0.163.0, an owned local Responses fixture, two control connections and the native TUI in an owned ConPTY. It used synthetic credentials, fresh homes/workspaces and no external model requests. The fixture retained native protocol traffic, terminal transcripts, input/source identities and cleanup receipts privately through the existing process/BrokerRoot lifecycles.

| Observation | What it establishes |
| --- | --- |
| A completed native file-writing tool was followed by a synthetic overload without retry advice; `stream_max_retries` was 5. The turn failed, the goal became `blocked`, and the TUI showed `Goal stalled (/goal resume)`. | The reported failure is reproducible through installed native components. |
| An observer sent status-only `active` with automatic provenance. The same TUI/thread continued, the goal's creation time, objective and 10,000-token budget stayed unchanged, usage advanced from 20 to 40 synthetic tokens, and the file contained the effect once. The native goal tool completed the goal and the TUI showed achievement. | Native same-session recovery and retained accounting/tool state work. |
| A real `/goal pause` through the TUI produced `paused` and no spontaneous provider request. A deliberately stale automatic activation was then accepted and returned `active`. | The native API does not enforce the required pause-versus-resume ordering. |
| Owned process receipts reported zero remaining active processes after each run. | The discovery runs restored their owned runtime state. |

The recovery case completed inside a run whose later pause-input driver timed out; its intermediate evidence is retained. The corrected pause-only run exited successfully and exposed the race. Setup/driver failures are retained separately and are not counted as product failures or passes. This is evidence for the native primitives, not a delivered watchdog or the ordinary launch integration. Long-running tools, bounded exhaustion, ambiguous acknowledgements and the proposed ordering owner still require implementation acceptance.

Current contract references:

- [Codex goal management API](https://learn.chatgpt.com/docs/app-server#manage-a-thread-goal).
- [Codex 0.163.0 error retry classification](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/protocol/src/error.rs#L438).
- [Codex 0.163.0 Stop-hook call site](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/core/src/session/turn.rs#L718).
- [Codex 0.163.0 native goal error handling](https://github.com/openai/codex/blob/rust-v0.163.0/codex-rs/ext/goal/src/extension.rs#L423).

## Goals / Non-Goals

**Goals:** Keep recovery in the existing session owner, use native state transitions, make cancellation ordering enforceable, and preserve the caller's console and supported launch behavior. Provide a small real path first: overload, bounded wait, activation of the existing goal, native continuation.

**Non-Goals:** An upstream fork, model failover, general recovery from all errors, restarting processes to replay work, a second goal store, a global polling daemon, window management, quota handoff, or automatic attachment to arbitrary pre-existing sessions. Already-running ordinary sessions without the new owner acquire support on a later explicit reopen; installation must not disrupt them to retrofit it.

## Decisions

### 1. Own the native connection instead of attaching a free-running hook

For a new ordinary interactive invocation, extend the launcher with a minimal session-owned app-server/control relay and attach the native TUI to it in the **existing inherited terminal**. Keep one native thread and one backend for the session. Reuse already-hosted backends where a harness host exists; integrate recovery into that host's control owner rather than layering another host.

Reuse the native process/job primitives, authenticated local `ControlConnection`, argument separation and the established native TUI attach protocol. Relevant foundations are [native_launcher.rs](../../../crates/harness-core/src/native_launcher.rs), [task_control.rs](../../../crates/harness-core/src/task_control.rs), [task_runtime.rs](../../../crates/harness-core/src/task_runtime.rs), the relay in [executor_warning.rs](../../../crates/codex-harness/src/executor_warning.rs), and [task_control_contract.rs](../../../crates/codex-harness/tests/task_control_contract.rs). Extract only genuinely shared transport/attach behavior needed by existing and new callers. Do not invoke `task_runtime::run`, `task_view::layout`, quota transfer or child-window allocation for this feature.

The relay exposes the supported frontend/control endpoint; the backend's credential and endpoint remain private to the owner. All supported state-mutating clients use the ordered relay. Noninteractive commands and explicitly supplied foreign `--remote` endpoints retain native forwarding; if authoritative recovery cannot be provided for such a route, say so without retargeting it. No launch-time build, installation or second start after a payload has executed is allowed.

An external watcher using only logs or a status poll is rejected because it cannot close the observed cancellation race. A normal Stop hook misses the error path. An upstream retry patch is possible but adds a maintained CLI fork; the native control path already supplies the required operation. Injecting keystrokes or editing SQLite would bypass native ownership and is not a delivery mechanism.

### 2. Serialize recovery and user intent at one control boundary

Use the owner's existing event/control loop for scheduling. Associate a candidate with the owned native thread, latest failed turn, observed goal generation and a monotonically increasing control epoch. Increment/invalidate the epoch when a user control action is admitted; handle pending cancellation input before dispatching a due timer. A timestamp alone is not a generation identifier, since native goal timestamps have coarse resolution.

Immediately before dispatch, re-read the native goal/latest turn and verify the original conditions, the unchanged epoch, no newer pending user work, and an idle native turn. User pause/interruption, goal mutations, clear, new input, frontend closure and loss of authority invalidate the candidate. Events that merely update accounting do not create a new recovery episode. Internal model activity cannot authorize activation of a different goal.

Check-and-send is ordered against user writes **inside the same owner**. A final read performed by one process followed by an unrelated API write is explicitly insufficient. Supported clients must not bypass the relay. A command admitted before activation is forwarded wins; an action arriving after native activation is forwarded normally and cancels future retries. This is an observable ordering guarantee, not a claim to undo work that already began.

The discovery's stale automatic resume after a TUI pause is the mandatory regression case. Also test cancellation between the final read and send, not only cancellation before the timer fires. Do not mark a programmatic retry as an explicit user mutation to bypass provenance rules.

### 3. Keep the recovery state small and bounded

Use a per-session state machine:

```text
watching --eligible failed turn--> waiting --due + current--> activating
waiting --user action / lost authority--------------------> cancelled
activating --ack + new native turn------------------------> observing
observing --successful turn------------------------------> watching
observing --overload + attempts remain-------------------> waiting
observing --other stop / attempts exhausted--------------> stopped
```

Suggested defaults are five automatic resumes, with base waits of 5, 10, 20, 40 and 60 seconds plus 0-20% positive jitter. This is a bounded default policy, not a user/project/session lifetime limit. Use monotonic time for waiting. Honor retry deadlines when exposed by the native structured contract; do not infer hidden server headers from human-readable text. Native request retries finish before the owner receives a terminal failed turn; they remain separate from these goal-resume attempts.

Reserve an attempt when its activation is forwarded. Deduplicate by failed turn and retain remaining attempts across connection recovery. Intermediate tool progress does not replenish the episode; otherwise each failed turn could reset a permanent loop. A completed successful turn or an explicit user resume re-arms the episode. Exhaustion leaves the existing goal blocked with a clear manual action.

Retain the minimal candidate/attempt/dispatch state in the existing private session checkpoint lifecycle. Native goal state remains authoritative. An unacknowledged activation is uncertain: reconnect and reconcile goal/latest turn before doing anything else. Do not send it again to discover whether it succeeded. An owner/frontend exit cancels pending authority; stale checkpoints alone cannot restart model work.

### 4. Resume native state without reconstructing work

The recovery operation is status-only `thread/goal/set` with automatic provenance. Omit objective and budget. Let the native scheduler start the continuation; do not also send `turn/start`, resend the original prompt or create another goal/thread. Preserve current provider/model/effort settings rather than choosing them anew.

Keep existing tool sessions alive. Reusing the quota-handoff rule that waits for every terminal to disappear would incorrectly block recovery whenever a dev server is running. Test actual pending tool-result delivery and a long-lived background terminal separately from the completed-effect discovery case. Native turn admission, not a blanket empty-process inventory, governs continuation.

### 5. Expose policy through existing lifecycle and terminal surfaces

Add an optional versioned recovery policy to the installer-owned launch registration, generated by core install/update. Persist the enabled/disabled choice in existing installation settings and expose it through that lifecycle rather than introducing a separate configuration store or putting unsupported keys into Codex's TOML. New installations use the accepted bounded default; an explicit opt-out is retained on update. A live user's ordinary pause/interrupt cancels recovery immediately; changing installation policy controls subsequent launches without stopping current work.

Check reports enabled, disabled, supported or degraded based on current native capability/identity, separately from test evidence. Use the established frontend notification mechanism to show one scheduled-attempt notice, native continuation and a final cancellation/exhaustion/unavailable notice. Do not rely on periodic log spam or replace the native stalled label with a false active state.

## Risks / Trade-offs

- **Native API accepts stale resume:** enforce a single ordered mutation boundary and test the adversarial ordering. A watcher with only read access cannot claim this capability.
- **Hosted launch changes startup/exit behavior:** preserve all supported argument, cwd, profile, Unicode, configuration, terminal and cleanup contracts; compare ordinary and hosted routes through installed tests. Before payload start, degraded recovery may fall back once to native launch. After payload start, preserve the session and report the fault instead of launching another copy.
- **Extra native process/relay per ordinary session:** keep it in the existing session Job and reuse existing hosted owners. No always-on service or extra model request is needed for healthy goals. Measure startup/idle overhead during acceptance rather than claim savings.
- **Protocol/version drift:** check capabilities through installed schema/native initialization, keep unknown errors non-retryable, and report degraded recovery without disabling ordinary CLI use.
- **Synthetic evidence is narrower than subscribed work:** the probe exercises native code and tool execution, not service availability or real billing. Integrated lifecycle and in-flight work remain acceptance tasks; no paid-model stress test is necessary to induce overload.
- **Control-channel failure during an attempt:** retain ambiguous outcome and reconcile; never interpret missing observation as proof that no model work began.

## Migration Plan

1. Promote the synthetic overload/recovery and stale-pause cases into the existing native test ownership, with explicit installed CLI input and private retained evidence.
2. Implement the ordered recovery state in a small owner module and validate its cancellation, exhaustion and uncertain-acknowledgement boundaries.
3. Integrate with ordinary launch and existing hosted control owners, preserving the inherited terminal and current configuration. Keep the old task-control/window path disabled.
4. Deliver the registration policy through core lifecycle and exercise first use, repeat/resume, disable, rollback and failure recovery from an independent owned consumer outside this checkout.
5. Update the owning installation/native guide. Enable the delivered path only after the required ordinary-entry acceptance passes. Do not retrofit or restart existing user sessions.

Rollback restores the prior owned launch registration/build through the existing lifecycle. Saved native goals and their accounting remain untouched. No schema/template changes to external OpenSpec workflows are involved.
