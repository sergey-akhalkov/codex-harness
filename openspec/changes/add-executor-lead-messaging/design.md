## Context

See [proposal.md](proposal.md) for motivation and scope. This change spans dispatch identity, native transport, executor lifetime and instructions, so a design artifact is required.

Current source observations:

- `executor_cli.rs` clears inherited `CODEX_SESSION_ID` and `CODEX_THREAD_ID` before launching a child. Its dispatch receipt records the slot, owner, native observation and endpoint, but no verified originating lead. Keep the environment clearing: a return address is distinct from the child's own native identity.
- `executor_message.rs` already validates an exact live executor and uses `turn/steer` for active turns and `turn/start` for idle threads. It distinguishes observed input from accepted or indeterminate delivery. Reuse that transport and evidence logic without weakening existing checks.
- `host_control_conversation` currently finishes the run and stops its app-server after a terminal turn. A pending question therefore needs a lifecycle hold beyond a single model turn.
- The installed core defaults `task_control` to false. The older task runtime also performs window layout and has argument bypasses, including explicit profiles. Neither its wholesale activation nor a claim that all native leads lack an inbound endpoint is justified.
- The generated structured brief already names the lead as consumer and defines escalation boundaries. The team-lead skill currently describes escalation by returning a result or using board feedback, which must be reconciled with a live exchange.

Read-only native inspection on 2026-09-24 found `codex-cli 0.156.1`, `codex queue --thread <UUID-or-name> --message <TEXT>` with optional remote endpoint/authentication arguments, and native app-server daemon commands. A local daemon version query failed to connect; this is evidence of an unavailable connection in that inspection, not proof that queue or daemon delivery is unsupported. No message was sent and no daemon was started during planning. Installed harness help was older than the source command surface.

The [official app-server documentation](https://learn.chatgpt.com/docs/app-server) establishes native thread identity, input injection, active-turn steering and event observation. The [official CLI documentation](https://learn.chatgpt.com/docs/developer-commands) distinguishes a local protocol endpoint from remote-control services. Public documentation inspected here did not establish the installed `queue` command's active/idle behavior; its help establishes syntax only. The existing `tests/task_control_contract.rs` and owned Responses fixture are the appropriate place to exercise that exact question without a model subscription.

The concurrent `add-observed-executor-tui` change owns native presentation and the full base `Observable executor lifecycle and bounded result` requirement. This change adds reply holds and a uniquely named waiting-observation requirement instead of replacing that block. The concurrent `close-failed-executor-tab` change owns closing a finished tab and preserving the real outcome separately from the terminal host exit. Keep the selected native frontend attached while a question is pending, close it only at an actual run end, and do not reintroduce a custom renderer or change terminal exit policy here. The [simplification design](../simplify-native-harness/design.md) owns cross-cutting consolidation and instruction reduction; consume its native owner rather than introducing another controller. These distinct delta operations must preserve each other's scenarios on archive.

## Goals / Non-Goals

**Goals:**

- One native conversation remains the authority for each lead and executor. Harness adds the relationship and lifecycle rules needed for their bounded exchanges.
- Mechanical decisions live in Rust: origin validation, exact addressing, metadata, reply correlation, delivery classification and keeping an unanswered request's run alive.
- Ordinary global use requires no endpoint discovery, new launcher ritual, recipient selection or manual resume to answer a waiting executor.

**Non-Goals:**

- No independent chat server, board mirror, transcript store, new model client, hidden helper model, native agent-tool enablement or desktop input automation.
- No automatic takeover by another lead, implicit fresh conversation, broader executor authority, new billing route or general-purpose hostile same-account isolation system.
- No historical receipt migration by guessing a parent from cwd, titles, rollout timestamps or session names.

## Decisions

### 1. Native session control first; one existing transport owner

Use official native operations on the already owning server and exact thread UUID. The initial real-path check compares installed `codex queue` with the existing app-server transport for a busy lead, a lead waiting in a tool and an idle lead. Use queue wherever it satisfies timely input, exact identity, visibility and observable outcomes. Where queue defers until the current turn ends, use the already supported native `turn/steer`; an idle loaded conversation receives `turn/start`. Do not maintain two delivery engines: the selected native invocation is an implementation detail of the existing transport owner.

Queue acceptance, history injection and model delivery are distinct. `thread/inject_items` alone does not wake an idle conversation. A second server that resumes a saved copy of a live lead is not delivery to that lead. Tests must observe the owning conversation's items and next provider input, including a lead blocked in `executor watch`, rather than accepting a successful RPC as proof.

Resolve an existing native endpoint when it is available. If an ordinary supported launch needs an endpoint, add only the minimal native app-server plus TUI attachment through the installed launcher, in the original terminal, reusing existing argument, process, authentication and connection code. Do not activate the old window-layout, quota or succession controller as a messaging prerequisite. Preserve profiles, user settings, cwd, sandbox, approval policy, native argument precedence and effective model/provider/effort. Explicit native remote sessions use their verified existing endpoint.

At spawn, establish the originating lead's exact native identity and delivery capability before an executor's first model request. If an older already-running session cannot be addressed natively, report that specific condition and the supported fresh-launch remedy; do not dispatch a falsely connected worker or restart the active lead. Optional harness failure still preserves ordinary native CLI availability under the existing launcher contract.

**Alternatives:** prompts cannot enforce ownership or keep a native server alive; native multi-agent tools would change the independently spawned executor architecture and its disabled-agent-tool contract; a new message broker duplicates native transport. None is selected. The native capability check is a bounded choice between existing interfaces, not permission to invent a parallel service.

### 2. Spawn owns an immutable originating relationship

Extend the existing local dispatch/run record with the originating lead's native thread identity, its verified endpoint reference, the executor run generation and sender process identity. Use the existing opaque capability/random identity and process validation facilities; inherited environment carries only the executor's per-run context, never the lead's `CODEX_THREAD_ID` as the child's identity.

`lead message` resolves that context and verifies the live run, session, lease and calling process lineage against recorded process creation identity before selecting the recorded lead. A copied marker or receipt reference from an unrelated process, sibling executor or later slot occupant must fail. Neither message text nor CLI options can choose a different parent. The check must survive a legitimate executor shell changing cwd and must not rely on a bare PID, executable name or window title.

The parent is the native lead thread, not whichever process most recently opened the repository. A legitimate reconnect to that same lead thread can refresh its verified endpoint. Resume/restart through existing authorized recovery preserves the originating relationship and creates a new run generation; stale senders and reply references are not silently redirected to it. A different lead does not acquire children through a matching checkout or label. Existing explicit authorized succession remains its own authority boundary.

This guards the supported harness command path within the existing local-user trust model. It does not claim an OS security boundary against a process that can modify the user's harness files or invoke upstream APIs directly. No endpoint bearer or sender capability appears in model-visible metadata, help output or tracked artifacts.

### 3. Request by default, optional notification, one-step reply

The ordinary commands are:

```powershell
codex-harness lead message --text 'Which input contract applies to sample-17?'
codex-harness executor message --reply-to MESSAGE_ID --text 'Use the versioned input contract.'
```

Both accept `--file FILE` instead of `--text`. `lead message` requests a reply by default. An optional `--notify` marks an exceptional notice that needs no answer and therefore creates no reply hold. This keeps the question path minimal and avoids retaining completed work solely because an informational notice was sent. No additional wait command or endpoint flags are required.

The send command returns a bounded transport receipt rather than blocking a model tool call until the human or lead answers. It records an unresolved request before attempting native delivery. The executor can continue independent authorized work; if it ends its turn with the request unresolved, the host enters waiting-for-reply. A definite rejected send removes that request's hold and reports failure; an indeterminate send remains unresolved with an honest delivery status and recovery locator.

Each envelope has a harness-owned header and a separate literal payload. The header contains message ID, request/notification kind, sender owner label, immutable run generation, native executor session, source checkout, worktree path and slot, originating lead identity, and explicitly registered assignment/board references when present. Do not infer board IDs from prose or parse beads into controller state. Omitted assignment metadata is reported as unavailable, never invented. The message ID is a local opaque reply reference, not an authority-bearing secret.

The lead sees a ready-to-use `executor message --reply-to ...` pattern. Reply resolution verifies the calling lead, the original sender generation and current native identity before reusing executor delivery. Text/file payloads remain literal; metadata cannot be overwritten by payload text. Legacy fully addressed executor messaging remains supported. Mixing `--reply-to` with a contradictory explicit address is rejected before sending. Ordinary unrelated steering does not acknowledge a pending request.

### 4. Waiting is a live run state, not model activity or completion

Keep native turn state distinct from the existing host's assignment lifetime:

```text
running -- send request --> running with unresolved request
                                |
                           native turn ends
                                v
                       waiting-for-reply
                                |
                      native reply is observed
                                v
                             running

running -- turn ends with no unresolved request --> existing completion path
running / waiting-for-reply -- explicit stop --> existing stopped path
```

The host retains the same server/thread, visible titled surface, receipt, process ownership and slot lease while an unresolved request exists. It waits on native events without periodic model turns, status nudges, transcript reloads or arbitrary expiry. Waiting consumes the existing slot and local process resources; this operating condition is confirmed. When the run is waiting-for-reply, an active or newly invoked `executor watch` returns promptly with exit 3 and bounded waiting/request/reply-reference data in text or JSON. This is action required, not completion, timeout, unavailable coverage or failure. Existing exits 0/1/2 retain their meanings. The lead answers and can invoke the same watch command again; no executor resume, new waiter protocol or model polling is needed. Waiting observation takes precedence over a simultaneous timeout once the verified waiting state is available.

An observed correlated reply resolves only its request. A reply received during the executor's current turn uses native steering; a reply received while its native thread is idle starts the next turn on that same thread. The sender, active turn and request status are checked again when transitions race. No question-completion race can stop the server between acceptance and reply, and no late reply can reach a reused slot. A mere transport acknowledgment does not clear the hold before delivery evidence exists.

A stopped or failed process is not called waiting. Explicit stop and lost-visibility handling retain their existing guarantees. Lead disconnection preserves request identity and partial work, reports availability truthfully and does not manufacture an answer or reparent the executor. When no native endpoint accepts delivery, return an explicit unavailable outcome; do not create a custom offline mailbox. If native queue accepted an item, reconcile that native item on reconnect to the same lead before retrying.

### 5. Keep existing records and honest delivery evidence

Use existing receipt/endpoint/lease locks and native history. A compact message-ID lookup is local addressing state owned by that lifecycle, not another task board or message transcript. It can point to the run/message record; it must not become an independent source of sender identity. Unknown and retired IDs fail closed. Follow existing record retention, preserving unresolved requests and preventing aliasing when resolved entries retire.

Reuse delivered/queued/error/indeterminate classification. Delivery means observed input in the addressed native conversation, not that the recipient agreed, acted or completed the task. Retries of an uncertain operation reconcile its recorded native request/item identity before sending again. Identical text in a later resolved exchange remains a new legitimate message; content-only deduplication must not suppress it forever. A deterministic refusal or bounded capacity error is preferable to silent loss, duplicate delivery or eviction of an unanswered request.

### 6. Instructions cover judgment; code covers mechanics

Inject a concise command and usage rule into both free-text and structured dispatch briefs, including supported continuation paths. Executors ask after inspecting available facts when a material ambiguity, authority/access boundary or unobtainable dependency requires the lead. They record durable blockers and decisions in the existing bd issue or appropriate feedback task. Ordinary errors remain their responsibility; status updates remain on bd. Messages must not become a polling channel.

Explain that a question opens a reply hold, the executor may finish its turn when blocked, independent work may continue, and `--notify` needs no answer. The lead replies using the supplied reference, records durable decisions on the board, and uses explicit stop when cancellation is appropriate. A watch result of 3 means answer the outstanding request, then continue observation; it does not mean resume or release. Instructions do not teach endpoint discovery, PID checks, receipt editing, ID lookup or manual keep-alive loops. Preserve agent-tool disablement and the prohibition on recursive delegation. Replace the obsolete end-to-escalate and manual addressing guidance instead of appending a second workflow; include generated briefs and mandatory references/help in the instruction accounting owned by `simplify-native-harness`.

## Risks / Trade-offs

- [Native queue help exceeds documented behavioral evidence] -> Verify its exact installed behavior with the existing owned native contract fixture; use the existing supported app-server operations for any demonstrated semantic gap. Record the selected route and evidence in this design during implementation.
- [Launcher integration changes normal use] -> Exercise the ordinary installed lead path, explicit profiles and an already configured remote endpoint; preserve the terminal and native settings. Do not accept a special test-only lead launcher as global delivery.
- [Question races turn completion, reply or stop] -> Register holds under the existing run lock, reconcile native events and retain immutable run identity through each transition; test both sides of each race.
- [Waiting occupies capacity] -> Show waiting and its request in native observation; no model polling or automatic expiry. The lead can reply or explicitly stop without losing partial work.
- [Misleading identity or delivery claims] -> Verify process lineage and native thread identity separately from payload metadata; distinguish confirmed input from queue acceptance and unknown outcomes.
- [Board/message divergence] -> Keep messages as bounded exchanges and bd as the durable record; do not automatically mirror or interpret board state.

## Migration Plan

1. Extend the existing model-free native contract test to settle queue/steer/idle delivery and native session discovery for the supported Windows entry point. Preserve useful partial findings and avoid model-backed probes for transport mechanics.
2. Implement the smallest native-backed request/reply path and run identity/hold changes in their existing owners. Keep old explicit executor addressing compatible; legacy unbound receipts report unavailability rather than fabricated parents.
3. Update source instructions/help, then deliver immutable binaries and skills with the normal harness deployment lifecycle. Verify fresh installed sessions outside the checkout; existing active sessions retain their old runtime until normal restart and are reported accurately.
4. Run the integrated two-lead round trip and real consumer acceptance, covering watch exit 3 for blocked waiting, reply, continued work, normal completion, isolation and stop/recovery on the current owned frontend, including native TUI when its change has integrated. Verify a pending request prevents TUI closure and the final resolved run closes normally. Retain exact private commands/evidence locally; keep only synthetic inputs and reusable conclusions in shared source.
5. Recover the prior installed build through the existing recovery owner if delivery fails. Do not overwrite live session state, reparent children, delete worktrees or claim migration of an active native conversation. Preserve waiting-run state before any necessary explicit stop/restart.
