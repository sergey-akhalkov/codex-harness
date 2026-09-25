## Context

See [proposal.md](proposal.md) for motivation and scope. The confirmed everyday path is dispatch, inspect a readable executor tab, block in `watch` when the lead has no other work, receive a result, and have the finished tab close automatically.

Current source has two different lifetimes. `executor_cli.rs` starts a control-backed app-server for pooled spawn/restart, drives its turn and records the outcome; `watch` reads that receipt and checks host liveness. Explicit TUI mode instead launches an interactive CLI with observation unavailable. Exact-session resume currently takes the observed `codex exec resume` route. Merely selecting TUI mode would therefore lose both observation and some live control coverage.

The installed CLI inspected during exploration was 0.156.1. Its help exposes `resume --remote` and bearer-token attachment. The official [CLI contract](https://learn.chatgpt.com/docs/developer-commands?surface=cli) documents these options; the [app-server protocol](https://learn.chatgpt.com/docs/app-server) documents thread subscriptions and terminal turn statuses. These are protocol capabilities, not proof of the proposed integrated executor path.

Existing `task_control_contract.rs` checks cover two-client native event delivery and attachment to a named empty thread with canned Responses. [Native verification guidance](../../../../docs/rust-native.md#native-task-control-contract) records historical success on 0.154.0, the need to prepare the named empty thread through resume by ID, and rejection of permission overrides on the attaching remote CLI. Those results must be requalified for the target executable; they do not establish current global delivery. The larger ordinary-launcher task controller is unfinished and is not a prerequisite to activate wholesale.

## Goals / Non-Goals

**Goals:** Keep one owner for execution and one native frontend for each managed conversation. Decouple assignment completion from frontend process exit while retaining the receipt as the lead's observation interface. Reuse the existing transport, terminal, Job and test facilities.

**Non-Goals:** A replacement TUI, a new general daemon or observer service, logfile-based completion inference, provider/model changes, automatic task retries, goal-mode semantics, new executor-to-lead messaging, or changes to slot release and lead acceptance.

## Decisions

### Attach the native frontend to the existing controlled conversation

The executor host remains the controller. Prepare the configured backend and exact thread without a model request, record identity, and launch a native `codex resume --remote ... <thread-id>` frontend in the executor's existing tab. Use the existing loopback capability-token mechanism without exposing the token in arguments, logs or receipts. Keep backend permission/profile settings on the backend; omit remote CLI overrides known to be rejected. Verify the resulting model/provider/effort, cwd and executor tool restrictions rather than trusting argv alone.

Before submitting the assignment, establish that the frontend has attached to this thread and owns a live terminal surface. A created window or process alone is insufficient. Reuse the named-empty-thread preparation demonstrated by the existing contract test. Submit the assignment exactly once through its owning controller, with both clients observing the same conversation. The native TUI owns terminal input and output while attached; controller rendering goes to existing bounded detail/error owners, not into the alternate screen. Pre-start errors and post-frontend diagnostics may still use the host terminal.

The default spawn, resume and restart routes use this managed presentation. Existing mode spellings remain accepted; qualify native inline presentation for the existing explicit `--mode exec` text/observation contract, while the default and `--mode tui` receive full observation/control. Remove replaced custom rendering after that qualification. If a documented compatibility property is missing, keep only the smallest adapter for that observed gap on the same lifecycle, not another controller or a permanent second presentation implementation by default. The existing `--exec` assignment-input argument is not a presentation selector. Record presentation truthfully without making callers add a flag or changing watch arguments. Old receipts remain readable with their honest coverage limitations. Unsupported native attachment fails with a remedy; it is not a reason to silently launch the default executor as a text stream.

This is smaller than writing another renderer and retains the user's familiar TUI. Watching rollouts would add history parsing, offset/correlation and crash ambiguities when the backend already publishes the needed events. A notification hook would add configuration, trust and failure-reporting work without replacing the existing control channel.

### Keep run outcome in the existing receipt

The controller consumes the backend's structured lifecycle. Match terminal events to the bound thread and accepted turn(s), not merely any event mentioning the thread or a previously resumed turn. Register work accepted through the existing message command or native TUI so an earlier terminal event cannot finish newer accepted work. Preserve the existing message delivery and pending/undelivered reporting contract; do not introduce a separate queue protocol.

On a terminal outcome, settle accepted work and retain its final message using the existing bounded result machinery before publishing the final receipt state. A successful turn without a nonempty final answer remains an output defect only when no unresolved reply request retains the run. Native turn completion does not release a reply hold or close a waiting TUI. Process exit, an idle frontend, a quiet connection and a stopped tool are not success signals. Retain host identity checks so a lost controller does not leave `watch` waiting indefinitely or invent an exit code.

`watch` keeps the current receipt polling implementation, CLI, default timeout and exit meanings: 0 completed, 1 unsuccessful terminal outcome, 2 timeout or unavailable coverage. Timeout does not stop the executor. It can return the persisted outcome independently of frontend shutdown; that preserves synchronous waiting without making the TUI exit itself after an answer. Completion still does not establish lead acceptance or release the slot. When the separate reply workflow is present, its additive waiting observation returns 3 for action required without a terminal outcome; this does not change meanings 0/1/2.

This change alone owns the full modified `Observable executor lifecycle and bounded result` requirement. The messaging change owns its additional waiting/request requirement, so either delta cannot overwrite the other's scenarios. The [simplification design](../../simplify-native-harness/design.md) owns later shared-daemon qualification and cross-cutting removal; no wholesale general-controller activation is needed for this TUI path.

### Finish the owned surface without affecting neighbors

Retain exact ownership of backend and frontend resources in the executor lifecycle. After preserving a terminal run result with no unresolved reply hold, close that frontend, release the run's backend resources through their owning mechanism and finish the tab host. An exclusively owned backend can exit; a shared native server must retain other sessions. Prefer the native disconnect/exit path where verified; use bounded owned-process cleanup when the frontend remains interactive. Do not synthesize `/quit`, send keystrokes, kill by title, close a terminal window or alter terminal settings. Distinguish the assignment outcome from cleanup failures and retain both in existing diagnostic owners.

The parallel `close-failed-executor-tab` change owns tab-host exit normalization and oversized final-message recovery. Consume that mechanism when integrating this presentation; do not add another terminal close policy. Preserve the run's actual state and exit code even if the terminal-facing host uses a different exit value to close its tab. A failed cleanup identifies the surviving owned resource and recovery action; it is not reported as successful closure.

If the only frontend disappears during work, stop further model dispatch, interrupt/contain the owned run using the existing stop and Job mechanisms, preserve partial work and record an unsuccessful outcome. Continuation is explicit through the pooled command with a restored TUI. Native cancellation alone does not prove that an external tool or process has stopped. Do not treat loss of focus, an unselected tab or lack of tiling as view loss.

### Include continuation and existing protections in the same delivery

Route exact-session resume through the managed backend while retaining its recorded slot and partial files; do not create another conversation as an accidental consequence of frontend attachment. Restart still creates a fresh session in the same preserved slot with the current bounded handoff. Rebind control endpoints and observation to each new run so a stale message, stop or completion cannot affect its successor.

Keep cache-loss monitoring, CPU/process admission, existing terminal targeting, single-agent restrictions and global installation recovery in their current owners. The frontend itself must not issue a duplicate assignment, request an unplanned model/provider switch or reactivate agent-spawning tools. A protocol incompatibility affecting these properties blocks this route with a concrete diagnostic rather than weakening the requirement.

### Verify the actual entrypoint with existing native fixtures

First requalify named-thread attachment and two-client delivery against the explicit target Codex binary, using the existing canned provider and owned state. Extend the existing executor tests with the smallest adapter that exercises actual spawn/run/watch plus native TUI and app-server. Assert one tool effect and one assignment, matching identity, readable TUI messages/tool activity, final result persistence and owned process exit. Check failure, output defect, manual frontend closure, control-message delivery, urgent stop, exact-session resume and neighboring-run isolation. Protocol fixtures remain useful for deterministic failure races but cannot substitute for the native TUI path.

Global acceptance uses the installed commands outside this checkout, in an owned consumer workspace and terminal, with the qualified native executable. Use the existing model-free provider fixture for deterministic scenarios. Configured subscription availability is a separate claim; reuse an authorized real assignment for that evidence rather than create paid diagnostic calls. Inspect the native terminal using existing text/terminal facilities or direct human observation; do not screenshot agent conversations. Keep raw evidence and local identities outside the tracked repository.

## Risks / Trade-offs

- [Remote attachment changes configuration or misses early events] -> Qualify exact-thread preparation and subscriptions before dispatch; assert binding, no duplicate request and result delivery from both clients.
- [The TUI and controller share stdout] -> Give terminal I/O exclusively to the frontend while retaining controller diagnostics in their existing files.
- [A stale terminal event races newer accepted input] -> Correlate turn identity and current run; test a correction at the completion boundary and reject or report undelivered input honestly after closure begins.
- [Closing the frontend leaves a backend or tool running invisibly] -> Retain process ownership and exercise manual closure, active tool interruption and surviving-resource diagnostics.
- [Automatic closure makes the final screen brief] -> This is the confirmed user choice; the durable result and details remain available through watch and its locators.
- [Another active change edits the same host and receipt paths] -> Integrate with `close-failed-executor-tab` and preserve any subsequently delivered executor-to-lead messaging contract; recheck affected boundaries without taking ownership of their unrelated features.

## Migration Plan

Complete the native contract check before wiring the default route. Implement and verify the executor path, then reconcile its guides with the actual backend instead of retaining the outdated claim that all pooled runs execute `codex exec --json`. Update the owning decision record to supersede the text-default choice while retaining automatic closure and blocking watch.

Deliver through the existing global install/update lifecycle and verify the installed commands in an independent workspace. Existing running executors keep their owning executable and receipt; do not retrofit or terminate them during deployment. Use the existing installation recovery route for rollback, preserving slots, session history, results and unrelated configuration.
