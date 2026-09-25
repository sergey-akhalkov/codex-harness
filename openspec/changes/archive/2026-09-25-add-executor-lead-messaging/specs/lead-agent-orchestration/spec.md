## ADDED Requirements

### Requirement: Spawn-bound executor-to-lead messaging

The kit SHALL expose `codex-harness lead message --text TEXT` and an alternative `--file FILE` for literal UTF-8 multiline input. An executor SHALL need to supply only the payload, with no lead, source checkout, home, slot, owner, endpoint or session address. `executor spawn` SHALL establish the relationship between the exact executor run and the native lead session that originated it before the executor's first model request. The message command SHALL verify the caller's membership in that live spawned run and SHALL deliver only to its recorded originating lead. A marker, supplied identifier, current directory, matching label or most recently used session SHALL NOT alone establish authority. Caller-provided recipient overrides SHALL NOT be accepted. Missing, forged, stale or mismatched identity SHALL cause refusal before delivery, with a concrete reason and supported next action. Calling from another directory within the legitimate executor's process tree SHALL NOT change or lose the recipient. Existing executor restrictions on recursive delegation and native agent tools SHALL remain in effect.

#### Scenario: Minimal question from a spawned executor
- **WHEN** a live executor spawned by lead A invokes `codex-harness lead message --text 'Which contract applies?'`
- **THEN** harness validates its run and sends the question to lead A without requiring the executor to discover or supply addressing metadata

#### Scenario: Concurrent leads share a repository
- **WHEN** leads A and B each spawn executors from the same source checkout and both executors send messages
- **THEN** each message reaches only its own originating lead, independently of cwd, visible tab selection, labels or dispatch order

#### Scenario: An unrelated process copies the executor marker
- **WHEN** an ordinary shell, unrelated agent or sibling executor invokes lead message using another executor's marker or run reference
- **THEN** sender validation refuses the request before any lead receives it

#### Scenario: Sender changes working directory
- **WHEN** a legitimate executor runs the command from a subdirectory or another task-authorized directory
- **THEN** the originating lead and sender metadata remain those of its recorded spawned run

#### Scenario: A stale process outlives its run
- **WHEN** a stopped or replaced executor process attempts to send after its slot or run generation has changed
- **THEN** the command refuses the stale sender and does not use the new occupant's identity or lead relationship

#### Scenario: A spawn cannot establish the lead channel
- **WHEN** spawn cannot verify the originating native lead session or its supported delivery route
- **THEN** it reports the exact unavailable capability and recovery action before the executor's first model request, without guessing a parent, silently dispatching an isolated worker or restarting the active lead

### Requirement: Trusted sender metadata and direct replies

Every executor-to-lead message SHALL include harness-generated message identity, request or notification kind, originating lead identity, executor owner label, immutable run identity, exact native session, source checkout, worktree and slot, plus explicitly registered assignment or bd references when available. Absent assignment metadata SHALL remain explicitly unavailable rather than inferred from text. Payload content SHALL remain distinct from this metadata and SHALL NOT override it. Private authentication material SHALL NOT be included. The delivered envelope SHALL identify the sender without further lookup and supply a reply reference usable as `codex-harness executor message --reply-to MESSAGE_ID --text TEXT`, with the same `--file` alternative. Reply resolution SHALL verify the calling lead and exact original sender run before delivery; another lead, a retired reference or a reused slot SHALL NOT receive or authorize a redirected reply. Reply addressing SHALL be independent of the lead's current directory. Conflicting explicit addressing and a reply reference SHALL be rejected before delivery.

#### Scenario: Lead answers without reconstructing an address
- **WHEN** a lead receives an executor question with its injected metadata
- **THEN** it can identify the executor and worktree and answer using only the supplied reply reference and response text, without finding a receipt, slot, endpoint or session ID

#### Scenario: Another lead uses the reply reference
- **WHEN** lead B attempts to reply to a request sent to lead A
- **THEN** the command refuses the caller mismatch without delivering or transferring ownership

#### Scenario: Reply arrives after slot reuse
- **WHEN** the original executor ended and its slot now belongs to another run
- **THEN** a reply to the old message reports that original run's state and does not reach the new occupant or automatically resume an old session

#### Scenario: Payload imitates routing metadata
- **WHEN** the payload contains a fabricated sender header, shell syntax or a different session identifier
- **THEN** the literal text is preserved as payload, the trusted envelope still identifies the actual sender, and no payload text is evaluated or used to choose a recipient

#### Scenario: A continuation preserves its originating lead
- **WHEN** an authorized continuation of a spawned executor is created through the existing recovery path
- **THEN** it retains the original lead relationship with its new verified run identity, while stale sender contexts and old reply references cannot silently address that replacement run

### Requirement: Executor questions retain a reply-capable session

Lead message SHALL request a reply by default and SHALL return a bounded delivery receipt without requiring a separate wait command. An optional `--notify` SHALL send an exceptional notification without requesting a reply. An unresolved request SHALL keep the executor's exact conversation, native input capability, visible terminal surface, slot lease, partial work and run ownership available. The executor SHALL be able to continue independent authorized work while a request remains unresolved. When its native turn ends with an unresolved request, the run SHALL be reported as waiting-for-reply rather than completed or an empty-output defect. Waiting SHALL cause no model polling, repeated status requests, automatic task replay, hidden stop/resume or arbitrary expiry. The observation path SHALL expose the waiting state and outstanding request promptly, so a lead waiting for its executor can respond. A definite delivery refusal SHALL NOT create a phantom successful request or reply hold; an uncertain result SHALL retain its unresolved identity and honest delivery status.

An observed correlated reply SHALL resolve only its addressed request and continue the same executor conversation and worktree without a separate resume command or model/provider/effort change. Active executors SHALL receive native steering without interruption; idle waiting executors SHALL receive the next turn in their same native thread. Ordinary uncorrelated steering SHALL NOT silently resolve a request. Native turn completion, message acceptance, reply delivery and stop SHALL be reconciled so their races cannot prematurely close the conversation, duplicate a reply or target another run. Explicit stop, failure and visibility-loss handling SHALL retain their existing preservation and recovery guarantees; a dead process SHALL NOT be reported as waiting. Lead unavailability SHALL NOT silently complete the assignment, release the slot, invent an answer or transfer the executor to another lead.

#### Scenario: Blocked executor finishes its current turn
- **WHEN** an executor sends a clarification request, has no independent work and ends its native turn before the lead answers
- **THEN** its live session and visible surface remain available in waiting-for-reply, the slot remains occupied, and the lead can answer using one executor message command without resume

#### Scenario: Executor continues independent work
- **WHEN** a question is pending but the executor has authorized work independent of the answer
- **THEN** the send receipt allows that work to continue, and a correlated answer is delivered to the same active conversation without interruption or task replay

#### Scenario: Lead is waiting for executor completion
- **WHEN** the lead is observing an executor that transitions to waiting-for-reply
- **THEN** the native observation path exposes the request and waiting state without waiting for assignment completion or sending model status probes

#### Scenario: Reply races the executor's turn ending
- **WHEN** the reply arrives as the executor's active turn completes
- **THEN** the reply is delivered once into the same conversation using the appropriate active or idle native operation and no completion path destroys the reply channel first

#### Scenario: Multiple unresolved requests
- **WHEN** an executor has more than one accepted unresolved request and the lead answers one
- **THEN** only that request is resolved and the remaining request identities and reply hold are preserved

#### Scenario: Informational notice needs no answer
- **WHEN** the executor sends a notice with `--notify` and later finishes with no unresolved requests
- **THEN** the notice does not create a waiting state or prevent the existing completion path

#### Scenario: Lead disconnects while a request is pending
- **WHEN** the originating lead becomes unavailable before answering
- **THEN** the executor's request identity, worktree and partial work remain preserved, availability is reported honestly, and no other lead or new conversation is selected automatically

#### Scenario: Stop while waiting
- **WHEN** an explicit executor stop targets a waiting run
- **THEN** the existing urgent stop path ends that exact run, pending reply delivery is invalidated, and files and slot remain preserved for explicit recovery

### Requirement: Native session delivery with observable outcomes

The solution SHALL reuse official Codex session, queue, turn and event capabilities wherever they satisfy the required behavior. Harness-specific code SHALL provide the missing originating relationship, sender validation, metadata, reply resolution and lifecycle coordination rather than a parallel conversation runtime, model client, board store or delivery service. Stable mechanical behavior SHALL be enforced algorithmically instead of requiring agents to reconstruct addresses, keep sessions alive, inspect internal files or poll one another through prompts. Native-operation selection SHALL be verified against the installed CLI's actual behavior; help text alone SHALL NOT establish delivery. An ordinary globally installed lead session SHALL be able to receive and answer messages without a special per-task launcher, endpoint setup or terminal manipulation. Native argument semantics, profiles, local settings, sandbox/approval policy, model/provider/effort, visibility and process ownership SHALL remain preserved.

Delivery SHALL target the exact owning native conversation while busy or idle, including a lead waiting in a tool, and appear on its visible surface. The result SHALL distinguish accepted or queued input, observed delivery, definite refusal, unavailability and indeterminate outcome. Acceptance SHALL NOT be reported as recipient action, and writing a local file SHALL NOT count as model delivery. An uncertain attempt SHALL be reconciled before retrying; duplicate delivery SHALL NOT result from retry. Identical text in a later resolved exchange SHALL remain sendable as a new message. Missing native capability SHALL produce an explicit cause and supported next action, without hidden runtime replacement, a new lead conversation, routing to another lead or a custom offline mailbox. Existing ordinary native CLI fallback and installation recovery guarantees SHALL remain intact.

#### Scenario: Native queue satisfies the required delivery
- **WHEN** a supported native queue operation demonstrably reaches the exact active or idle lead with the required observation and visibility
- **THEN** the implementation reuses that capability instead of creating a second queue or conversation service

#### Scenario: Queue defers a blocking question
- **WHEN** the available native queue defers a question until a busy lead's current turn ends and therefore cannot satisfy timely delivery
- **THEN** the solution uses a supported native input operation that reaches the active conversation without interrupting its running tool or substituting another session

#### Scenario: Delivery acknowledgement is uncertain
- **WHEN** a send loses its response after the native endpoint may have accepted it
- **THEN** the command records and reports the indeterminate attempt and reconciles its identity before retrying, without claiming delivery or silently sending a second copy

#### Scenario: Same words in a later exchange
- **WHEN** an earlier exchange has been resolved and the executor legitimately sends the same text for another question
- **THEN** the new exchange receives its own message identity rather than being suppressed by permanent content-only deduplication

#### Scenario: Native delivery capability is missing
- **WHEN** a lead's existing session cannot accept the required native input
- **THEN** the command reports the specific unavailable capability and remedy without attaching a second server to a saved copy, launching another lead or claiming a local record was delivered

### Requirement: Actionable observation of pending executor requests

The existing watch, receipt and terminal observation owners SHALL expose waiting-for-reply with the exact run and bounded unresolved request/reply references. An active or newly invoked `executor watch` SHALL return promptly with exit 3 and an action-required waiting result in its supported text or JSON format when the verified run is waiting for a reply. This result SHALL preserve the live conversation, native frontend, lease, worktree and partial work; it SHALL NOT establish completion, failure, empty-output defect, unavailable coverage or permission to release the slot. Existing watch exit meanings 0 for completion, 1 for unsuccessful terminal outcome and 2 for timeout/unavailable coverage SHALL remain unchanged. A verified waiting state available at the timeout boundary SHALL yield the waiting result rather than a timeout. After the lead replies, the same watch interface SHALL observe continued work and eventual completion without a separate resume or new observation protocol.

This requirement SHALL extend the base `Observable executor lifecycle and bounded result` contract rather than replace its native-TUI, identity, result, failure and recovery guarantees. A dead or stopped run SHALL retain its actual failure/stop outcome instead of appearing reply-capable; accepted reply/completion/stop races SHALL be reconciled by the existing lifecycle owner.

#### Scenario: Turn finishes awaiting a decision
- **WHEN** an executor ends its native turn with an unresolved question to the lead
- **THEN** the observer reports waiting-for-reply with the exact run and message reference while the live reply channel, native surface and slot remain available

#### Scenario: Lead is already blocked in watch
- **WHEN** the observed executor reaches waiting-for-reply before the existing watch timeout
- **THEN** that watch returns 3 promptly with the bounded request and reply reference, without terminating the executor or requiring a model status probe

#### Scenario: Waiting is observed at the timeout boundary
- **WHEN** the watch deadline and an established waiting state are available in the same observation
- **THEN** watch returns the actionable waiting result rather than hiding the question behind timeout, and the run stays live

#### Scenario: Reply resumes ordinary observation
- **WHEN** the lead sends the correlated reply and invokes watch for the same run
- **THEN** watch observes that run's continued work through its existing lifecycle and later returns its true terminal result without reconstructing an address or resuming the executor

## MODIFIED Requirements

### Requirement: Board-based asynchronous assignment and feedback

Lead and executors SHALL coordinate routine assignments and feedback through the consuming project's task board using its non-interactive CLI: development stages as epics, specifications as features, executor feedback as feedback tasks, and lead-initiated improvements as tasks, or as OpenSpec changes when they alter accepted requirements. The board SHALL remain consuming-project state; the Rust controller SHALL NOT parse or duplicate it, because lead and executors read and update the board themselves. The kit SHALL deliver board-tool availability and workflow guidance through its installation lifecycle, verified from a fresh external session. Public kit sources SHALL contain only synthetic examples. When the board is unavailable, the orchestrator SHALL report the limitation explicitly and continue only work whose acceptance does not depend on the board. Exceptional questions and escalations SHALL be able to reach the originating lead through lead message while durable blockers, decisions, progress and results remain on the board; messaging SHALL NOT replace the board or introduce a parallel task record.

#### Scenario: An executor escalates a blocker
- **WHEN** an executor cannot proceed because of a missing dependency or an ambiguous requirement
- **THEN** it records the concrete blocker in the existing assignment or appropriate feedback task, can ask its own lead through lead message, and records the resulting durable decision on the board without duplicating task state in the controller

#### Scenario: A fresh session discovers the workflow
- **WHEN** an ordinary Codex session starts in a consuming project after kit installation
- **THEN** board commands and the lead/executor workflow guidance are available without copying kit sources or performing manual setup

#### Scenario: The board is unavailable
- **WHEN** the board tool or its project state is missing or broken
- **THEN** orchestration reports the limitation and affected assignments explicitly instead of silently losing acceptance records or inventing a replacement protocol

### Requirement: Lead steering and executor escalation without waste

The lead SHALL deliver steering to an executor through the kit's addressed executor message command into that executor's live conversation, and the delivered input SHALL appear in that executor's visible conversation. The message command SHALL address one existing run through either the accepted checkout, slot, owner and exact recorded session identity, or a verified reply reference that resolves to that same exact run. It SHALL verify that identity against the actual run before delivery so a message cannot reach a later occupant of a reused slot, and SHALL accept a short literal text or a UTF-8 file containing multiline text, preserving real line breaks and literal content without shell evaluation. Delivery SHALL go to the same conversation with its context, model, provider, reasoning effort and completed work preserved, and SHALL NOT be implemented through hidden stop/resume, a new conversation, model replacement or re-sending the whole task. While an executor is working, delivery SHALL use the backend's real capability to accept additional input, including at the nearest supported point during a running tool call, and SHALL NOT interrupt that tool call. The command SHALL distinguish queued input, confirmed delivery and error, SHALL NOT present a local file write as delivery to the model, and a retry after an indeterminate result SHALL NOT silently deliver the same context twice. For a completed, stopped or unavailable run it SHALL return an explicit result with a supported next action and SHALL NOT start a new conversation; a live waiting-for-reply run SHALL remain directly addressable. Waiting and routine event handling SHALL require no model calls, and no repeated status requests SHALL be sent to active executors. Steering SHALL add relevant facts, resolve a request, or correct an established mistake; status-only nudges, hurry demands and repeated messages without new facts SHALL NOT be sent. Executor reports and durable escalations SHALL remain bounded board records. An executor SHALL be able to use lead message for an exceptional clarification, missing authority or unavailable dependency after investigating what it can independently; the lead SHALL answer through the supplied reply reference, and durable decisions SHALL be reflected on the board rather than left only in conversation history.

#### Scenario: A mid-assignment correction arrives
- **WHEN** the lead sends a concrete correction while an executor is working
- **THEN** the input appears in that executor's conversation, the executor continues in the same session and worktree, and no duplicate session is started

#### Scenario: A path correction is delivered verbatim
- **WHEN** the lead sends a multiline UTF-8 correction from a file while the executor is inside a long tool call
- **THEN** the message is delivered at the nearest supported point without interrupting that tool call, its literal text appears in the executor's conversation, and the reported result distinguishes delivery from later application by the executor

#### Scenario: A stale identity is refused
- **WHEN** the addressed slot no longer runs the recorded session, or the slot was reused by another run
- **THEN** message refuses with the observed identity mismatch and names a supported next action instead of delivering to the current occupant

#### Scenario: A finished run cannot be messaged
- **WHEN** message addresses a run whose recorded lifecycle is completed, stopped or unavailable
- **THEN** the command returns that actual state with the continuation remedy and starts no new conversation

#### Scenario: A healthy executor is waiting
- **WHEN** an executor remains active beyond several observation waits without a demonstrated failure
- **THEN** waiting generates no model status requests and the executor's ownership is preserved

#### Scenario: A nudge without new facts is not sent
- **WHEN** the lead is tempted to ask for status or hurry an executor that has no new decision-relevant fact
- **THEN** no message is sent, and steering is reserved for concrete corrections, facts or requirement changes
