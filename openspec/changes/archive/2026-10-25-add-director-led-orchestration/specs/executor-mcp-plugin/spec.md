# Spec Delta

## MODIFIED Requirements

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

## ADDED Requirements

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

