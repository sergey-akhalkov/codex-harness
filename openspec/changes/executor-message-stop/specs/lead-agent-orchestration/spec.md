## MODIFIED Requirements

### Requirement: Lead steering and executor escalation without waste

The lead SHALL deliver steering to an executor through the kit's addressed executor message command into that executor's live conversation, and the delivered input SHALL appear in that executor's visible conversation. The message command SHALL address one existing run through the accepted checkout, slot, owner and exact recorded session identity, SHALL verify that identity against the actual run before delivery so a message cannot reach a later occupant of a reused slot, and SHALL accept a short literal text or a UTF-8 file containing multiline text, preserving real line breaks and literal content without shell evaluation. Delivery SHALL go to the same conversation with its context, model, provider, reasoning effort and completed work preserved, and SHALL NOT be implemented through hidden stop/resume, a new conversation, model replacement or re-sending the whole task. While an executor is working, delivery SHALL use the backend's real capability to accept additional input, including at the nearest supported point during a running tool call, and SHALL NOT interrupt that tool call. The command SHALL distinguish queued input, confirmed delivery and error, SHALL NOT present a local file write as delivery to the model, and a retry after an indeterminate result SHALL NOT silently deliver the same context twice. For a completed, stopped or unavailable run it SHALL return an explicit result with a supported next action and SHALL NOT start a new conversation. Waiting and routine event handling SHALL require no model calls, and no repeated status requests SHALL be sent to active executors. Steering SHALL add relevant facts, resolve a request, or correct an established mistake; status-only nudges, hurry demands and repeated messages without new facts SHALL NOT be sent. Executor reports and escalations SHALL reach the lead as board feedback tasks with bounded context rather than unbounded transcript copies.

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

## ADDED Requirements

### Requirement: Urgent executor stop through the kit command

The kit SHALL provide an executor stop command that urgently stops one exact executor run identified through the accepted checkout, slot, owner and exact recorded session identity. Stop SHALL NOT wait for task completion, a model answer or a long running command: it SHALL use the backend's native interruption signal where that provides urgent interruption, and SHALL boundedly terminate the remaining owned process tree when the signal is unavailable, insufficient or exceeded, without leaving a hung stop lacking a diagnosable result. It SHALL verify actual run and process ownership before acting - an old process id, program-name or window-title match SHALL NOT be sufficient. It SHALL close exactly the stopped run's terminal tab or pane through the existing terminal-surface owner and SHALL NOT close the lead's terminal, neighboring tabs or a window hosting other conversations. It SHALL stop further delivery of pending messages to that run and record their undelivered state honestly. It SHALL preserve changed and untracked files, the checkout, already received results and the information needed to continue: no reset, clean, worktree deletion or loss of partial work SHALL occur, the run SHALL NOT be marked complete, accepted or merged, the slot SHALL NOT be released or freed for another task, and continuation and release SHALL remain separate explicit actions. The lifecycle receipt SHALL record the observed outcome - stopped, already completed, partial stop or error - with an unknown exit code kept unknown, and stop SHALL NOT claim rollback of external actions or recovery of external systems. A repeated stop SHALL be safe and report the actual current state, a stop racing natural completion SHALL report which outcome won, and the result SHALL remain available to the lead after the tab closes. On partial failure the command SHALL name the surviving process or resource, the cause and the next action instead of reporting false success.

#### Scenario: Stop during generation
- **WHEN** the lead stops an executor while the model is generating
- **THEN** the run ends promptly, its owned processes are gone, only its terminal tab closes, the receipt records the stopped state, and its files remain for continuation

#### Scenario: Stop during a child command
- **WHEN** the lead stops an executor while a long child command is running
- **THEN** the child and its owned tree are terminated within the bounded stop path, the executor's tab closes, and the stop result reports actual termination rather than assuming the child ended with the host

#### Scenario: Stop races natural completion
- **WHEN** the run completes while stop is verifying identity or interrupting
- **THEN** the command reports the completed state and its recorded result instead of recording a stop that did not happen

#### Scenario: Stop is repeated
- **WHEN** stop is issued again for an already stopped run
- **THEN** it reports the stopped state without error, without touching another run and without releasing the slot

#### Scenario: Only the executor's tab closes
- **WHEN** the stopped executor shares a terminal window with the lead and other executors
- **THEN** exactly the stopped run's tab closes and every other conversation surface remains usable

#### Scenario: A partial stop is not success
- **WHEN** the host ends but an owned child survives the bounded termination
- **THEN** stop reports the surviving process, cause and next action, and the receipt records a partial stop instead of success

### Requirement: Control-backed executor conversations

Pooled executor dispatch SHALL run its non-interactive conversations on a backend that actually accepts addressed input and urgent interruption for the live session. A command wrapper over an operation the backend does not support SHALL NOT be accepted as message or stop delivery, and absent capability SHALL NOT be masked by a successful response. The owning dispatch route SHALL provide the capability with the smallest necessary change while preserving the existing receipt, lease, slot, terminal, rendering, result-recording and process-ownership guarantees, and the change SHALL NOT alter the configured profile's model, provider or reasoning effort. Surfaces that remain unsupported for addressed input SHALL report that fact explicitly with the supported continuation path instead of pretending delivery.

#### Scenario: A dispatched executor is addressable
- **WHEN** a pooled executor run is working in exec mode
- **THEN** its live conversation accepts an addressed message and an urgent stop through the kit commands without a new conversation or task re-send

#### Scenario: An unsupported surface is honest
- **WHEN** message addresses an interactive executor run whose surface has no verified inbound channel
- **THEN** the command reports that surface as unsupported with its continuation remedy instead of reporting delivery

#### Scenario: Profile binding is preserved
- **WHEN** a pooled executor conversation runs on the control-backed route
- **THEN** its recorded and displayed model, provider and reasoning effort remain those of the configured executor profile
