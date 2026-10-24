# executor-mcp-plugin Specification

## Purpose
Provide executor delegation through one installed Codex MCP plugin, with native conversation visibility and communication, configured profiles and preserved worktree ownership.

## Requirements

### Requirement: Intent-based executor interface

The existing plugin SHALL accept Executor assignments from Directors and Leads and Lead assignments from Directors through MCP, and provide addressed messaging, observation, stop, continuation and release using the existing assignment and pool contracts. Existing Executor calls SHALL retain their compatible addressing and result fields. A Lead assignment SHALL carry a concrete OpenSpec change reference, its complete applicable planning inputs, accepted outcome, board reference, selected repositories and acceptance conditions. A task title or board card alone SHALL NOT satisfy Lead admission. Callers SHALL supply intent and necessary choices, without reconstructing runtime endpoints, checkout paths or transport files. Invalid input SHALL fail before allocation or model effects.

#### Scenario: Ordinary assignment
- **WHEN** an activated Director or verified delegated Lead submits an Executor assignment with an accepted committed base and optional configured profile
- **THEN** the interface validates the complete brief, selects and binds a shared pool slot, and returns the run reference, task title, actual checkout/base/profile and native conversation identity

#### Scenario: Ambiguous target
- **WHEN** an operation other than the default all-run `executor_wait` omits its target and several owned runs match
- **THEN** it returns a bounded list of matching runs and requests a run choice, without guessing from focus, cwd or recency
- **AND** exactly one verified matching run needs no manually copied address fields

#### Scenario: Default wait across owned runs
- **WHEN** the lead calls `executor_wait` without `run`
- **THEN** it observes eligible unreleased runs owned by that verified lead and returns an actionable question, an unacknowledged current terminal result, or an observation failure without requiring a run choice
- **AND** a question from one executor is not delayed until another executor completes

#### Scenario: Explicit wait target
- **WHEN** the lead supplies `run` to `executor_wait`
- **THEN** only that verified owned run is selected and its current result remains readable even if it was acknowledged in a previous all-run wait
- **AND** foreign or stale authority is rejected without retargeting to another run

#### Scenario: Invalid assignment
- **WHEN** required assignment content is empty, its inputs are absent, its base is unresolved or its selected profile is not allowed
- **THEN** dispatch reports the exact cause and remedy without starting model work or leaving a new live reservation

#### Scenario: Director dispatches a Lead for a prepared change
- **WHEN** a Director submits an authorized prepared OpenSpec change with valid committed planning and repository inputs
- **THEN** Lead dispatch validates role, inputs, direct-child and shared limits, binds its workspaces and return route, and reports the actual run, parent, role, profile and workspace identities

#### Scenario: Existing direct Executor caller
- **WHEN** an existing root session uses the existing Executor dispatch contract
- **THEN** it continues to receive a directly owned Executor with compatible result fields, without creating a Lead or rewriting the user's assignment

### Requirement: Native conversations in one CLI interface

Every delegated Lead and Executor SHALL have a distinct titled native Codex conversation accessible through the Director's existing CLI and native /agents interface, with its role and parent inspectable, with assignment, effective profile/model/effort, checkout, messages, tool activity and state inspectable. Dispatch SHALL establish this access before model work. A delegated Lead SHALL also have supported addressed access to its own Executors without another app-server, terminal or shared chat identity. No separate executor terminal, desktop rearrangement or native /subagents registration SHALL be required.

#### Scenario: Two concurrent executors
- **WHEN** the lead dispatches two assignments to different configured profiles and pool slots
- **THEN** both conversations appear distinctly in the same CLI interface and switching among them preserves the lead and both running assignments
- **AND** no additional terminal tab, pane or window is created

#### Scenario: View changes are not lifecycle events
- **WHEN** the user selects another conversation, closes a conversation view, or hides or archives a completed thread
- **THEN** ordinary navigation does not stop work, accept results, release slots or replay the assignment

#### Scenario: Visibility is unavailable
- **WHEN** dispatch cannot establish access through the native interface or the controlling CLI surface is lost
- **THEN** it reports the cause, prevents new dependent model dispatch and preserves in-flight work for supported restoration
- **AND** it does not substitute a hidden worker, transcript file, screenshot or separate executor terminal

#### Scenario: Whole hierarchy is visible
- **WHEN** a Director has two Leads, their Executors and a directly assigned Executor
- **THEN** the same native interface exposes each conversation and its parent, role, actual route, workspace and state without requiring a physically nested UI or additional windows
- **AND** switching views leaves ownership and active work unchanged

### Requirement: Verified caller and run authority

Every operation SHALL bind the actual calling Director, Lead or Executor to its recorded native runtime, thread, role, root, immediate parent, run generation and workspace leases. Only a verified root Director SHALL create Leads; a verified Director or Lead SHALL create Executors within its authorized scope. The Director SHALL be able to inspect and control its own tree, while a Lead SHALL control only its owned Executors and communicate upward to its Director. Caller-supplied identifiers, cwd, titles and session recency alone SHALL NOT establish authority. An Executor SHALL communicate only with its recorded immediate parent, whether Director or Lead, and SHALL NOT dispatch descendants. Role and relationship authority SHALL come from verified launch records rather than a model-supplied role, profile or parent. Missing or stale authority SHALL fail before delivery or model work.

#### Scenario: Origin is established before dispatch
- **WHEN** a lead starts an executor
- **THEN** its originating thread and usable return route are verified and recorded before the first model request
- **AND** a missing lead channel prevents dispatch rather than selecting another active conversation

#### Scenario: Forged or stale identity
- **WHEN** another lead, a sibling executor or an old run attempts to control a run using copied identifiers
- **THEN** the operation refuses without touching that run or the current occupant of a reused slot

#### Scenario: Executor changes directory
- **WHEN** an executor sends a question after changing its working directory
- **THEN** its verified launch binding still identifies the original lead without requiring recipient or installation paths

#### Scenario: Same profile does not imply the same authority
- **WHEN** an Executor using ds attempts Lead or Executor dispatch, or a Lead using ds attempts Lead dispatch
- **THEN** the operation refuses before changing reservations, workspaces, threads or model state, even if the caller knows the tool name and target identifiers

#### Scenario: Cross-team or cross-root control is refused
- **WHEN** a Lead addresses another Lead's Executor or a caller addresses a tree belonging to an unrelated Director
- **THEN** the server rejects the operation without delivering input or changing the target

#### Scenario: Director observes descendants
- **WHEN** the Director requests a descendant's status or native conversation within its verified tree
- **THEN** the operation succeeds without pretending the descendant is a directly owned Executor or transferring its assignment

### Requirement: Rebind an ended app-server without changing the run

When the originating lead thread is registered on a new app-server, the plugin
SHALL atomically update that run receipt's app-server lease only if the
previous process has provably ended and the live endpoint still names the same
lead thread. The run, executor thread, slot, checkout, base and profile SHALL
remain unchanged. A live or unverifiable previous process SHALL be refused
before effects.

#### Scenario: Resume a lead on a new app-server

- **WHEN** the same lead thread has a live registered endpoint on a new
  app-server and the process in its run receipt has ended
- **THEN** the next authorized operation records the new exact process in that
  receipt and can observe or control the unchanged run

#### Scenario: The old app-server is still live

- **WHEN** the run receipt's app-server process is still live but differs from
  the registered endpoint
- **THEN** the operation refuses without changing the receipt or addressing
  either runtime

#### Scenario: Process liveness cannot be proved

- **WHEN** identity inspection cannot determine whether the recorded
  app-server process ended
- **THEN** the operation refuses, preserves the lease and reports that the
  reconnect was not reconciled

### Requirement: Configured profiles without drift

Executor dispatch SHALL use the selected allowed profile or the configured Executor default, and Lead dispatch SHALL use the configured delegated Lead profile. Both SHALL preserve their effective model, provider, effort, authentication source, feature settings, instructions and execution policy. Role-specific tool masks and instructions SHALL be session-local overrides, leaving the shared profile and other sessions unchanged. Unsupported per-thread differences SHALL fail explicitly before model effects. Profile choice SHALL NOT silently alter provider, billing, model, permissions or the lead's configuration.

#### Scenario: Distinct providers share the interface
- **WHEN** two permitted profiles select different providers or feature settings
- **THEN** each executor actually consumes its own effective settings and provider route while the lead and other executor retain theirs
- **AND** observed binding is available to the lead without exposing credentials

#### Scenario: Invalid or unavailable route
- **WHEN** a profile cannot be resolved or its configured route fails
- **THEN** the result names the failing stage and remedy, preserves partial work when present, and does not substitute a model, provider or billing route

#### Scenario: Role overlays survive continuation
- **WHEN** ds Lead and Executor sessions start, reconnect or resume on the same app-server
- **THEN** each retains its actual configured provider, model, effort, execution policy and distinct role tool surface without changing the root Director or the shared ds profile

### Requirement: Preserve CLI execution privilege

Executors SHALL retain the originating CLI's ordinary or elevated execution identity, subject to its supported approval and sandbox policies. An elevated CLI SHALL NOT silently delegate through an ordinary daemon. Runtime attachment SHALL prevent ordinary clients from controlling elevated work and SHALL avoid altering unrelated sessions or requiring a general privileged service.

#### Scenario: Ordinary and administrator sessions
- **WHEN** the same installed workflow runs first in an ordinary CLI and then in an administrator CLI
- **THEN** executor-side commands observe the corresponding privilege level and both sessions retain the one-interface workflow

#### Scenario: Unsafe runtime attachment
- **WHEN** the available runtime would lower the caller's privilege or allow an ordinary client to control the elevated runtime
- **THEN** dispatch refuses before model work with the precise capability limitation and preserves existing sessions

### Requirement: Native bidirectional communication

The plugin SHALL use supported native Codex input, queue, turn and event facilities for parent-to-child steering, child-to-parent requests or notifications, and replies across Director-to-Lead, Lead-to-Executor and direct Director-to-Executor relationships. Messages SHALL reach the exact active or idle conversation without manual UI navigation, assignment replay or stopping unrelated work. No separate mailbox, transcript relay or model polling loop SHALL be required. A Lead's upward message SHALL be distinguishable from a message to one of its Executors; omitted addressing SHALL never guess between those directions. Executor parent addressing SHALL continue to come from its verified binding.

#### Scenario: Lead is busy
- **WHEN** an executor asks a question while the lead is generating or waiting in a tool
- **THEN** the native path surfaces the addressed request to that lead without requiring it to open /agents or finish unrelated work first

#### Scenario: Active and idle recipient
- **WHEN** a correction or reply targets an active turn or an idle conversation
- **THEN** supported native steering or queued input delivers it to that same thread
- **AND** delivery does not interrupt a running tool, create a replacement conversation or submit a second turn after the native queue already started one

#### Scenario: Direct human input
- **WHEN** the user sends input through an executor's native conversation view
- **THEN** it reaches the same managed thread once and participates in the current run lifecycle

#### Scenario: Lead works in both directions
- **WHEN** a Lead has a question for the Director and a correction for one Executor
- **THEN** each message reaches its intended relationship once, with independent correlated reply references and no recipient inferred from the currently focused conversation

### Requirement: Correlated questions remain actionable

Executor messages SHALL distinguish reply-required requests from notifications and carry trusted sender and reply references separate from literal content. Unresolved requests SHALL leave the executor reply-capable and its slot held, including after a turn ends. A correlated reply SHALL continue that conversation automatically; notification alone SHALL NOT hold completion.

#### Scenario: Two outstanding questions
- **WHEN** an executor asks two questions and receives a reply to one
- **THEN** only that request is resolved, the other remains actionable, and independent work may continue without a keepalive turn or status probe

#### Scenario: Trusted direct reply
- **WHEN** the lead replies using the returned request reference
- **THEN** sender, lead, run, thread, checkout/slot and available task metadata are verified, conflicting addressing is rejected, and the original requester receives the literal reply
- **AND** missing optional metadata is explicit and no secret is exposed

#### Scenario: Lead temporarily unavailable
- **WHEN** the originating lead disconnects while a question is pending
- **THEN** the request, partial work and lease remain recoverable without expiry, invented answers, automatic replay or reassignment to another lead

#### Scenario: Stop or replacement races reply
- **WHEN** a request's originating run stops or is replaced before its reply is delivered
- **THEN** the reply outcome is reconciled against that run and is not redirected to a replacement occupant

### Requirement: Honest delivery evidence

The interface SHALL distinguish native acceptance, observed delivery, recipient activity, indeterminate delivery and failure. It SHALL reconcile uncertain submissions using native identities before retrying and SHALL NOT claim that an acknowledgement proves recipient action. Different intentional messages with identical text SHALL remain distinct exchanges.

#### Scenario: Response lost after submission
- **WHEN** a native input request may have succeeded but its response is lost
- **THEN** the plugin reports indeterminate state and reconciles the original native operation before any retry, without silently duplicating input

#### Scenario: Delivery failure
- **WHEN** a native capability, connection or recipient is unavailable
- **THEN** the operation reports its original cause and supported next action, sends no replacement conversation input and records no false delivery

### Requirement: Literal input and large native records

Assignments and messages SHALL accept complete literal UTF-8 content through MCP without shell evaluation, truncation, command-line length dependence or caller-created transport files. Managed records SHALL have no harness-imposed fixed size ceiling. Native transport limits SHALL be reported accurately with no partial send or phantom request; operational waits and resource use SHALL remain bounded.

#### Scenario: Long multiline assignment or message
- **WHEN** a caller submits content beyond shell argument or former inline-message bounds
- **THEN** the recipient receives the complete unchanged content through the native path, including quotes and shell metacharacters, without a manual file handoff

#### Scenario: Large history and resume
- **WHEN** an exact-thread resume or final-message read exceeds one mebibyte
- **THEN** it uses the same native conversation without a harness size rejection or truncated result

#### Scenario: Native read or size failure
- **WHEN** a native limit prevents delivery or the authoritative final-message read fails
- **THEN** the actual failure remains visible and no partial message, successful result or substitute final text is invented

### Requirement: Observable run outcomes

Observation SHALL identify the current run and turn, expose actionable pending requests and distinguish working, waiting, completed, stopped, failed and output-defective outcomes. A bounded result SHALL link to native history and retained evidence. Completion SHALL require the actual current result, not process existence, an old event, server exit or a status label; acceptance and slot release remain separate.

#### Scenario: Question interrupts observation
- **WHEN** a lead is waiting for executor completion and that executor becomes waiting-for-reply
- **THEN** observation promptly returns the request and reply reference without marking completion or failure
- **AND** an established pending request is not hidden by a simultaneous observation timeout

#### Scenario: Completion reaches the lead
- **WHEN** an executor finishes its current work with a final result and no unresolved reply-required request
- **THEN** the owning lead receives the bounded result through the native conversation path without opening /agents or polling
- **AND** changed paths, checks, evidence and unresolved issues remain inspectable after navigating away

#### Scenario: Empty or stale completion
- **WHEN** a turn ends without a required usable result or only an earlier turn's completion is available
- **THEN** the run is not accepted as successful, partial work is retained and the evidence does not imply provider, authentication or quota failure

#### Scenario: Observation times out or disconnects
- **WHEN** a bounded wait ends without a terminal result or its connection fails
- **THEN** timeout remains distinct from connection failure and neither stops the executor, releases its slot or replays model/tool work

### Requirement: Urgent stop isolates one executor

Stop SHALL urgently interrupt the verified target thread and its owned work without waiting for a model answer or task completion. It SHALL preserve checkout contents, results, lease and continuation identity, prevent further pending delivery and leave the lead, other executors and shared runtime usable. Partial stop SHALL name surviving owned work and a next action; stop SHALL NOT imply rollback, acceptance or release.

#### Scenario: Stop during generation or a child command
- **WHEN** the lead stops one executor while another executor continues
- **THEN** the target's generation and owned child work end through a bounded native interruption/containment path and the neighbor and lead remain usable
- **AND** files, commits and untracked work survive without resetting or freeing the slot

#### Scenario: Repeated stop and completion race
- **WHEN** stop is repeated or races natural completion
- **THEN** it reports the observed stopped, completed, partial or error outcome, preserves an unknown exit code as unknown and does not act on a newer run

### Requirement: Preserve exact continuation and fresh recovery

Continuation SHALL retain the verified thread, slot, owner, profile and work without fetch, reset, clean or assignment replay. Fresh recovery SHALL retain the same worktree and predecessor evidence while creating a separately bound conversation with the original assignment and bounded progress. Stale or live conflicting ownership SHALL refuse before effects; release remains explicit after acceptance or discard.

#### Scenario: Recover interrupted work
- **WHEN** a stopped executor has edits, untracked files and an interrupted check
- **THEN** continuation or explicit fresh recovery preserves them, rechecks interrupted effects and reports only checks actually completed
- **AND** fresh recovery does not silently resume the predecessor's full context or automatically retry repeated failures

#### Scenario: Missing original input or native history
- **WHEN** fresh recovery lacks the original assignment or a failed startup never persisted native history
- **THEN** the interface names the missing evidence and needed input, retains the lease/work and does not claim that an empty thread is resumable

### Requirement: Existing pool and acceptance contracts remain authoritative

MCP dispatch SHALL use each selected repository's shared pool and existing role, assignment, resource and acceptance owners. Worktree capacity, direct-child role limits and installation-wide Executor concurrency SHALL remain independent and be enforced together. A delegated Lead's workspace leases SHALL consume their repositories' pools but SHALL NOT consume Executor concurrency positions. Terminal or stopped runs SHALL keep their worktrees until explicit safe release. MCP presentation, completion or archive SHALL NOT imply merge, discard or release. Cache-loss protection and other existing run safeguards SHALL remain isolated to the verified run; stopping a Lead SHALL reconcile and stop its owned descendants under the subtree-stop contract. Releasing a Lead's integration workspaces SHALL refuse while any child or unaccepted dependent result still needs them.

#### Scenario: Held slot and repeated use
- **WHEN** a slot contains live, suspended, reserved or unreviewed work
- **THEN** it remains occupied and is neither replaced nor supplemented by an ad-hoc worktree
- **AND** after authorized acceptance or discard, normal safe release reuses the existing slot and retains eligible ignored caches

#### Scenario: Executor role boundary
- **WHEN** an executor is initialized
- **THEN** it receives its complete role and assignment with native descendant spawning disabled while authenticated MCP communication with its own lead remains available

#### Scenario: A Lead cannot free an active team's base
- **WHEN** release is requested for a Lead whose children still own active or unaccepted work dependent on its workspaces
- **THEN** release names those dependencies and preserves the Lead and child workspaces without resetting or silently discarding them

### Requirement: Installed plugin lifecycle

The single executor plugin SHALL be installed, updated, disconnected and recovered through the kit's existing global lifecycle using supported Codex plugin facilities. Fresh consuming sessions SHALL discover it without copying sources or manually configuring per-task endpoints. Lifecycle changes SHALL preserve credentials, unrelated integrations, in-flight work and recoverable state.

#### Scenario: Fresh external consumer
- **WHEN** the plugin is installed and Codex starts outside the kit checkout
- **THEN** MCP dispatch, /agents visibility, both message directions, observation, stop and recovery work with configured profiles through the installed artifact

#### Scenario: Update or disconnect
- **WHEN** the plugin is updated or disconnected while recoverable assignments exist
- **THEN** the outcome identifies availability for new dispatch and preserves existing work and identity records without stopping unrelated sessions or exposing secrets

### Requirement: MCP-only active executor guidance

After verified installation, owned instructions, skills, generated briefs, help, examples and recovery hints SHALL teach the MCP executor workflow and native inspection interface. They SHALL contain no mentions of the migrated CLI operations, migration tables or required legacy references. Existing executable compatibility SHALL remain separately tested; unavailable MCP SHALL be reported honestly.

#### Scenario: Ordinary lead and executor context
- **WHEN** fresh lead and executor sessions load the installed guidance for dispatch, reply, stop or recovery
- **THEN** their actual required context names the available MCP tools and required decisions without old command examples, manual address bookkeeping or file-relay recipes

#### Scenario: Context reduction is measured
- **WHEN** the active guidance switches to MCP
- **THEN** the existing agent-delegation instruction-size and scenario-load acceptance is met across source and installed content with assignment payloads held constant

#### Scenario: Plugin unavailable
- **WHEN** installed guidance encounters a disconnected or unsupported plugin
- **THEN** it names the capability failure and supported restoration action without silently switching to a removed CLI recipe

### Requirement: Long waits remain quiet and interruptible

`executor_wait` SHALL default to a maximum wait of 1800 seconds, accept explicit shorter bounds, and keep one MCP operation pending while no actionable outcome exists. It SHALL return promptly for a question, current terminal result or observation failure. Cancellation or timeout SHALL affect observation only, preserving executor work and ownership.

#### Scenario: No event during a full wait
- **WHEN** eligible executors remain working for the full default interval
- **THEN** the installed MCP call remains pending until its 1800-second deadline and returns one timeout response
- **AND** no intermediate timeout, heartbeat or status response requires another model-issued wait call

#### Scenario: Result or question arrives during the wait
- **WHEN** a selected executor completes or records a reply-required question before the deadline
- **THEN** the same pending call returns the current result or exact request and reply reference on its next bounded observation cycle
- **AND** it does not wait for the rest of the interval or start a model turn to observe the event

#### Scenario: Caller cancels observation
- **WHEN** the user interrupts the pending wait or its MCP client disconnects
- **THEN** observation terminates through the existing bounded cancellation path without stopping an executor, releasing a slot, acknowledging an unseen result or replaying work
- **AND** a subsequent authorized wait can recover current outcomes

#### Scenario: Communication remains usable
- **WHEN** the lead has a long wait pending and an executor asks a question or the user sends native input
- **THEN** the ordinary communication path remains responsive and the wait does not hold a shared resource that prevents the event needed to wake it

#### Scenario: Unsupported or invalid wait budget
- **WHEN** the requested interval is invalid or the effective plugin client and server deadlines cannot support it
- **THEN** the operation reports the exact unsupported limit or setup cause and next action without silently shortening the wait or switching transports
- **AND** unrelated MCP tools retain their own bounded execution policy

### Requirement: Existing results survive late observation and retries

Waiting SHALL inspect retained current state before blocking and SHALL NOT depend on being present when completion occurs. Returned terminal results SHALL have stable identities. Receipt acknowledgement SHALL be explicit, retry-safe and scoped to the verified lead and result; sending a response alone SHALL NOT acknowledge it. Acknowledgement SHALL NOT imply work acceptance, release or resolution of a question.

#### Scenario: Completion precedes the first wait
- **WHEN** an executor completes before the lead calls `executor_wait`
- **THEN** the call returns its current result after the initial observation without waiting for a new event or the timeout
- **AND** completion is identified by the current native turn rather than process state or an older turn's result

#### Scenario: Completion occurs between waits
- **WHEN** a wait times out and an executor completes before the next call
- **THEN** the next call discovers the retained result immediately

#### Scenario: Several executors finish together
- **WHEN** multiple selected executors have unacknowledged current results
- **THEN** each remains retrievable until explicitly acknowledged, and acknowledging one lets subsequent all-run waits return the others without requiring acceptance or slot release
- **AND** a bounded response does not acknowledge omitted results

#### Scenario: A response is lost
- **WHEN** the server returns a terminal result but the client does not receive it
- **THEN** retrying observation can return the same result identity and evidence
- **AND** no send-side or connection-close action marks the unseen result as received

#### Scenario: Acknowledgement is retried
- **WHEN** the lead includes already received result identities in its next wait and retries that request after a connection failure
- **THEN** the acknowledgement is idempotent, acknowledged results stop waking default all-run waits, and any newly returned but unacknowledged result remains recoverable
- **AND** acknowledgement survives MCP reconnection and does not suppress a later turn or replacement run

#### Scenario: Invalid acknowledgement or pending question
- **WHEN** an acknowledgement has foreign, conflicting or unrecognized identity, or a result has an unresolved reply-required request
- **THEN** invalid acknowledgement is rejected before acknowledgement effects, and unresolved requests remain actionable until actually resolved through the existing reply contract

#### Scenario: State cannot be observed
- **WHEN** the native thread, accepted turn or retained authority cannot be read reliably
- **THEN** the wait reports an observation failure with available run identity and recovery guidance, without inventing completion, an empty queue or a normal timeout

### Requirement: Wait receipts minimize repeated context

Timeout and no-work responses SHALL carry only the wait state and necessary bounded scope or timing information. They SHALL NOT repeat run cards, paths, profiles, empty result placeholders or unchanged lifecycle records. Event responses SHALL identify the affected run and result or request, retain actionable errors and provide bounded evidence with access to full details through existing owners.

#### Scenario: Timeout response
- **WHEN** a valid wait expires with selected work still pending and no actionable outcome
- **THEN** its response identifies timeout and the waited scope or bound without repeating the dispatch/status payload
- **AND** the tool contract makes clear that timeout does not mean executor failure

#### Scenario: Nothing remains to wait for
- **WHEN** the default wait finds no eligible work, unanswered questions or unacknowledged current outcomes
- **THEN** it returns a compact no-work state immediately rather than spending the full interval or requesting an ambiguous target choice

#### Scenario: Diagnostic detail is needed
- **WHEN** a result is too large for the bounded inline response or observation fails
- **THEN** truncation or failure is explicit, necessary identity and actionable evidence remain present, and existing native history or status/evidence access retains the full details
- **AND** reducing timeout output does not weaken literal-message, native-record, authority or recovery guarantees

### Requirement: Role-specific tool visibility
The installed interface SHALL expose only orchestration operations permitted to the session's verified role. Directors SHALL receive Lead and Executor dispatch; Leads SHALL receive Executor dispatch, owned-child control and parent communication; Executors SHALL receive only parent communication and own-run observation. Leads and Executors SHALL have no native descendant-spawning tools. Server checks SHALL enforce the same boundaries.

#### Scenario: Lead and Executor catalogues differ
- **WHEN** a Lead and an Executor use the same ds profile
- **THEN** actual session and provider tool catalogues omit Lead dispatch from both and omit all descendant-dispatch tools from the Executor
- **AND** the Lead can dispatch an Executor and each can communicate with its parent

#### Scenario: A hidden tool is called directly
- **WHEN** a caller bypasses catalogue discovery and invokes an unauthorized operation
- **THEN** server-side role and ownership validation rejects it before effects

#### Scenario: Required role filtering is unavailable
- **WHEN** the installed native runtime cannot isolate the required role tool surface on start or resume
- **THEN** dispatch or continuation fails with the exact capability and restoration action before model work, preserving saved state and unrelated sessions

### Requirement: Retry-safe hierarchy dispatch
A dispatch request SHALL have a stable identity within its verified parent. Repeating the same request SHALL reconcile and return the original preparation or run without allocating another child. Reusing that identity with different intent SHALL fail before effects. Interrupted or uncertain startup SHALL retain enough evidence to reconcile native acceptance and reservations rather than guess success or start a duplicate.

#### Scenario: A start response is lost
- **WHEN** native thread or turn creation may have succeeded but the Director or Lead did not receive the dispatch response
- **THEN** retry resolves the original child's role, workspace and admission reservation before any additional creation

#### Scenario: Concurrent identical requests
- **WHEN** two clients repeat the same dispatch request for the same verified parent
- **THEN** at most one child is admitted and both receive its identity or the same unresolved preparation state

#### Scenario: A retry changes the assignment
- **WHEN** a request identity is reused with a different objective, role, repository set or committed inputs
- **THEN** the call rejects the conflict without mutating the original assignment

### Requirement: Hierarchy observation preserves event ownership
Status SHALL expose the Director's tree or a Lead's owned children with bounded detail and access to native evidence. Wait SHALL retain existing quiet, interruptible and acknowledged-result behavior for directly owned children. Descendant inspection SHALL not acknowledge a result on behalf of its parent, resolve its question or accept its work. Summaries SHALL distinguish stopped, waiting, failed, completed and unaccepted work.

#### Scenario: Director waits for either child role
- **WHEN** the Director waits with a Lead and a direct Executor active
- **THEN** an actionable question or current result from either wakes the same wait, with its role and exact parent relationship identified

#### Scenario: Director inspects an Executor's result
- **WHEN** the Director reads a grandchild result before its Lead receives it
- **THEN** the Lead's pending result and question delivery remain intact and the Director's inspection does not close or release that assignment

#### Scenario: Lead is unavailable with pending child work
- **WHEN** a Lead cannot respond while its Executors have questions or completed outputs
- **THEN** the Director's status exposes the affected subtree and recovery action without rerouting or inventing answers to those requests
