# executor-mcp-plugin Specification

## Purpose
Provide executor delegation through one installed Codex MCP plugin, with native conversation visibility and communication, configured profiles and preserved worktree ownership.

## Requirements

### Requirement: Intent-based executor interface

The plugin SHALL accept assignments through MCP and provide addressed messaging, observation, stop, continuation and release using the existing assignment and pool contracts. Callers SHALL supply intent and necessary choices, without reconstructing runtime endpoints, checkout paths or transport files. Invalid input SHALL fail before allocation or model effects.

#### Scenario: Ordinary assignment
- **WHEN** an activated lead submits an assignment with an accepted committed base and optional configured profile
- **THEN** the interface validates the complete brief, selects and binds a shared pool slot, and returns the run reference, task title, actual checkout/base/profile and native conversation identity

#### Scenario: Ambiguous target
- **WHEN** an operation omits its target and several owned runs match
- **THEN** it returns a bounded list of matching runs and requests a run choice, without guessing from focus, cwd or recency
- **AND** exactly one verified matching run needs no manually copied address fields

#### Scenario: Invalid assignment
- **WHEN** required assignment content is empty, its inputs are absent, its base is unresolved or its selected profile is not allowed
- **THEN** dispatch reports the exact cause and remedy without starting model work or leaving a new live reservation

### Requirement: Native conversations in one CLI interface

Each executor SHALL have a distinct titled native Codex conversation accessible through the lead's existing CLI and native /agents interface, with assignment, effective profile/model/effort, checkout, messages, tool activity and state inspectable. Dispatch SHALL establish this access before model work. No separate executor terminal, desktop rearrangement or native /subagents registration SHALL be required.

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

### Requirement: Verified caller and run authority

Every operation SHALL bind the actual calling lead or executor to its recorded native runtime, thread, run and lease. Caller-supplied identifiers, cwd, titles and session recency alone SHALL NOT establish authority. An executor SHALL communicate only with its originating lead and SHALL NOT dispatch descendants. Missing or stale authority SHALL fail before delivery or model work.

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

### Requirement: Configured profiles without drift

Dispatch SHALL use the selected allowed profile or the configured executor default and preserve its effective model, provider, effort, authentication source, feature settings, instructions and execution policy. Unsupported per-thread differences SHALL fail explicitly before model effects. Profile choice SHALL NOT silently alter provider, billing, model, permissions or the lead's configuration.

#### Scenario: Distinct providers share the interface
- **WHEN** two permitted profiles select different providers or feature settings
- **THEN** each executor actually consumes its own effective settings and provider route while the lead and other executor retain theirs
- **AND** observed binding is available to the lead without exposing credentials

#### Scenario: Invalid or unavailable route
- **WHEN** a profile cannot be resolved or its configured route fails
- **THEN** the result names the failing stage and remedy, preserves partial work when present, and does not substitute a model, provider or billing route

### Requirement: Preserve CLI execution privilege

Executors SHALL retain the originating CLI's ordinary or elevated execution identity, subject to its supported approval and sandbox policies. An elevated CLI SHALL NOT silently delegate through an ordinary daemon. Runtime attachment SHALL prevent ordinary clients from controlling elevated work and SHALL avoid altering unrelated sessions or requiring a general privileged service.

#### Scenario: Ordinary and administrator sessions
- **WHEN** the same installed workflow runs first in an ordinary CLI and then in an administrator CLI
- **THEN** executor-side commands observe the corresponding privilege level and both sessions retain the one-interface workflow

#### Scenario: Unsafe runtime attachment
- **WHEN** the available runtime would lower the caller's privilege or allow an ordinary client to control the elevated runtime
- **THEN** dispatch refuses before model work with the precise capability limitation and preserves existing sessions

### Requirement: Native bidirectional communication

The plugin SHALL use supported native Codex input, queue, turn and event facilities for lead-to-executor steering, executor-to-lead requests or notifications, and replies. Messages SHALL reach the exact active or idle conversation without manual UI navigation, assignment replay or stopping unrelated work. No separate mailbox, transcript relay or model polling loop SHALL be required.

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

MCP dispatch SHALL use the repository's shared pool and existing role, assignment, resource and acceptance owners. Worktree capacity and executor concurrency SHALL remain independent. MCP presentation, completion or archive SHALL NOT imply merge, discard or release. Cache-loss protection and other existing run safeguards SHALL remain isolated to the verified run.

#### Scenario: Held slot and repeated use
- **WHEN** a slot contains live, suspended, reserved or unreviewed work
- **THEN** it remains occupied and is neither replaced nor supplemented by an ad-hoc worktree
- **AND** after authorized acceptance or discard, normal safe release reuses the existing slot and retains eligible ignored caches

#### Scenario: Executor role boundary
- **WHEN** an executor is initialized
- **THEN** it receives its complete role and assignment with native descendant spawning disabled while authenticated MCP communication with its own lead remains available

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
