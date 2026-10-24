# Spec Delta

## MODIFIED Requirements

### Requirement: Intent-based executor interface

The plugin SHALL accept assignments through MCP and provide addressed messaging, observation, stop, continuation and release using the existing assignment and pool contracts. Callers SHALL supply intent and necessary choices, without reconstructing runtime endpoints, checkout paths or transport files. Invalid input SHALL fail before allocation or model effects.

#### Scenario: Ordinary assignment
- **WHEN** an activated lead submits an assignment with an accepted committed base and optional configured profile
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

## ADDED Requirements

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
