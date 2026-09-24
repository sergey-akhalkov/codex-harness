## ADDED Requirements

### Requirement: Native TUI for managed executor runs

Ordinary pooled `executor spawn`, `executor resume` and `executor restart` SHALL present each managed conversation in the native Codex TUI on its dedicated titled terminal surface without requiring an additional caller option. The explicit TUI mode SHALL provide the same managed observation and control. The frontend SHALL display the exact bound conversation, assignment, actual model and supported effective effort, live messages, tool activity and state. It SHALL retain the recorded slot cwd, configured provider/model/effort and single-agent restrictions. Controller output SHALL NOT interleave with or corrupt the native TUI. Before the first model request, dispatch SHALL establish the frontend's attachment to that conversation and its live terminal surface; a process or tab existing alone SHALL NOT establish readiness. A failed or unsupported attachment SHALL report its cause and recovery action instead of silently substituting a text stream or an invisible conversation. Existing command spellings, including the assignment input named `--exec`, SHALL remain accepted; explicitly selected exec presentation SHALL retain its supported observation behavior.

The supported explicit exec presentation SHALL reuse a qualified native presentation wherever equivalent; retaining its spelling SHALL NOT require a second conversation or control lifecycle. Any residual compatibility adaptation SHALL name the observed missing native property and preserve the same observation and control contract.

#### Scenario: Default dispatch shows the native conversation

- **WHEN** the lead dispatches an assignment with the existing spawn command and no presentation option
- **THEN** a native TUI is attached to the bound slot and exact managed conversation before its first model request, showing its configured identity and subsequent messages and tools without mixed controller output or a duplicate assignment

#### Scenario: Concurrent executor views remain distinct

- **WHEN** two pooled runs are active in different slots
- **THEN** each TUI displays its own recorded conversation and each observer reports only its bound run, without switching either model or sharing a conversation identity

#### Scenario: Attachment fails before work starts

- **WHEN** the native frontend cannot attach or the target CLI lacks the required capability
- **THEN** no assignment model request is submitted, the failure and remedy are available through the dispatch observation, and no hidden or text-only substitute starts

#### Scenario: Continuation preserves the managed presentation

- **WHEN** the lead resumes the recorded exact session or restarts a fresh session in a preserved slot
- **THEN** the continued run has a native TUI and full observation/control, resume retains the original session, restart records its new session and predecessor, and neither operation resets partial work or replays the old assignment as a new request

### Requirement: Managed TUI lifetime follows the recorded run

Once a managed run reaches its terminal outcome, the host SHALL preserve its result or failure and then end that run's owned frontend/backend and finish its terminal surface automatically, without a manual quit step. This SHALL retain the terminal-host closure policy, real run outcome and exit code, durable detail locators, process-ownership guarantees and partial work. Closing SHALL affect no neighboring conversation or terminal window and SHALL NOT imply acceptance, merge or slot release. A cleanup failure SHALL name surviving owned resources and the recovery action, separately from the run outcome, rather than claiming that closure succeeded. If the only attached frontend is lost during active work, the controller SHALL suspend new model dispatch, interrupt or contain the owned run, preserve its work and record interruption or a more specific observed failure. It SHALL NOT allow unseen continuation or report success from frontend exit alone. Loss of focus or selecting another terminal tab SHALL NOT count as frontend loss.

A native turn ending with an unresolved required reply SHALL NOT establish a terminal run or trigger this cleanup. The existing reply workflow SHALL retain its live surface and resources until reply continuation or an actual failure/stop. Backend cleanup SHALL release only run-owned resources; a shared native server and neighboring sessions SHALL remain available.

#### Scenario: Successful work closes its TUI automatically

- **WHEN** the current run completes with a nonempty final answer and no unresolved reply hold
- **THEN** the answer and successful outcome remain available to watch after the owned frontend closes, run-owned backend resources are released and the executor tab closes, without a user quit command or effects on neighboring tabs or shared services

#### Scenario: An unsuccessful run retains its evidence after closure

- **WHEN** the current run records a failure, interruption or output defect and its host finishes
- **THEN** its TUI surface follows the terminal-host closure policy while watch retains the real unsuccessful outcome, cause and diagnostics instead of inferring success from the host exit used to close a tab

#### Scenario: The user closes an active TUI

- **WHEN** the only frontend disappears while an assignment or its tool is active
- **THEN** further model dispatch is suspended, the owned run is interrupted or contained with surviving processes reported honestly, its partial work and identity remain, and watch reports an unsuccessful outcome with the supported continuation route

#### Scenario: Completion races frontend exit

- **WHEN** a terminal backend event and frontend exit occur near the same time
- **THEN** the retained native evidence determines the outcome, a previously recorded result is not lost, and frontend exit alone never manufactures a successful completion

## MODIFIED Requirements

### Requirement: Control-backed executor conversations

Pooled executor dispatch SHALL run its managed conversations, including the default native TUI presentation and its pooled continuation, on a backend that actually accepts addressed input and urgent interruption for the live session. A command wrapper over an operation the backend does not support SHALL NOT be accepted as message or stop delivery, and absent capability SHALL NOT be masked by a successful response. The owning dispatch route SHALL provide the capability with the smallest necessary change while preserving the existing receipt, lease, slot, terminal, presentation, result-recording and process-ownership guarantees, and the change SHALL NOT alter the configured profile's model, provider or reasoning effort. Surfaces that remain unsupported for addressed input SHALL report that fact explicitly with the supported continuation path instead of pretending delivery. Attaching the native frontend SHALL NOT create a second conversation, duplicate an assignment or invalidate the existing message, stop, cache-loss protection or recovery contracts. Input accepted through the native TUI SHALL address the same managed conversation and participate in its observed lifecycle.

#### Scenario: A dispatched executor is addressable

- **WHEN** a pooled executor run is working in managed TUI or explicit exec presentation
- **THEN** its live conversation accepts an addressed message and an urgent stop through the kit commands without a new conversation or task re-send

#### Scenario: An unsupported surface is honest

- **WHEN** message addresses a legacy or unmanaged interactive executor run whose surface has no verified inbound channel
- **THEN** the command reports that surface as unsupported with its continuation remedy instead of reporting delivery

#### Scenario: Profile binding is preserved

- **WHEN** a pooled executor conversation runs on the control-backed route
- **THEN** its recorded and displayed model, provider and reasoning effort remain those of the configured executor profile

#### Scenario: TUI input belongs to the observed run

- **WHEN** a user submits input through the managed native frontend while its run accepts input
- **THEN** it reaches the same conversation once and its accepted work is reflected in the run lifecycle, without an older turn's completion prematurely finishing that work

#### Scenario: Stop retains exact ownership with a frontend attached

- **WHEN** the lead stops one of two active managed TUI runs
- **THEN** the existing urgent-stop contract interrupts and contains only that run, closes its owned surface, preserves its files and continuation identity, and leaves the other executor and lead running

### Requirement: Observable executor lifecycle and bounded result

The existing dispatch and receipt owners SHALL distinguish request acceptance, terminal creation, observed native start, running, completion, failure and lead acceptance. Native session identity, checkout/base, changed files, actual reported checks and outcomes, remaining work, limitations, required decision and a detail locator SHALL be available without manual rollout searches. Completion and errors SHALL reach the lead through the native observation path. Empty completion SHALL be an output defect, not evidence of model, authentication or quota unavailability. Every model conversation SHALL retain its distinct visible terminal surface.

For managed TUI runs, `executor watch` SHALL retain its existing receipt/slot addressing, options, blocking behavior, timeout behavior, bounded text/JSON review result and exit meanings: 0 for completion, 1 for an unsuccessful terminal outcome, and 2 for timeout or unavailable coverage. Native TUI presentation SHALL NOT by itself make coverage unavailable. Watch SHALL learn completion from the current run's correlated native outcome with the final result or output defect preserved, independently of the frontend's willingness to exit. Prior-session or prior-turn events, quiet output and frontend process termination alone SHALL NOT establish successful completion. A timeout SHALL NOT stop the run. No model-side status polling, periodic model messages, terminal scraping or manual rollout search SHALL be required to wait. A lost host or connection SHALL remain diagnosable and SHALL NOT be reported as success or leave waiting unbounded beyond the requested timeout. Historical receipts SHALL remain honestly readable without inventing missing coverage.

Additional reply-workflow observation SHALL extend this base contract without replacing it. A native turn ending while a required reply remains unresolved SHALL report the workflow's nonterminal waiting result rather than completion or empty-output defect. Its action-required result SHALL NOT change the meanings of completion, failure and timeout/unavailable results above or authorize slot release.

#### Scenario: Terminal opens but native start fails

- **WHEN** a dispatch creates its terminal and its child cannot start or exits unsuccessfully
- **THEN** the receipt and observer report the actual stage and failure, preserve diagnostics and do not report successful model execution

#### Scenario: Work completes

- **WHEN** an executor finishes its accepted work with no unresolved reply hold
- **THEN** a native observer emits a bounded completion or output-defect event with the exact session, slot, result and detail locator without model-side status polling

#### Scenario: Interrupted assignment resumes

- **WHEN** an original assignment stops with partial work
- **THEN** continuation uses its recorded exact session and checkout without reset, retains visible identity and preserves changes for correction and acceptance

#### Scenario: Accepted slot is released

- **WHEN** the lead accepts and integrates an outcome and records its disposition
- **THEN** the existing release operation records that decision before resetting the slot for reuse; unreviewed work remains preserved

#### Scenario: Watch completes while the frontend would otherwise await input

- **WHEN** the current assignment has a persisted terminal result with no unresolved reply hold but its native TUI would ordinarily remain at an input prompt
- **THEN** the same blocking watch call returns the bounded result and correct exit status without requiring the user or lead to close the frontend first

#### Scenario: Watch timeout leaves the executor working

- **WHEN** the requested watch timeout expires while the managed run is active
- **THEN** watch returns 2 with the ongoing state and detail locator, the TUI and assignment remain active, and a subsequent watch can observe the same run

#### Scenario: Completion carries no usable final answer

- **WHEN** the current turn reports completion, no unresolved reply hold remains and no nonempty final answer can be retained
- **THEN** watch reports an output defect with its diagnostics and no false success or claim that the model, authentication or quota was unavailable

#### Scenario: Old terminal events do not finish a continuation

- **WHEN** a resumed session or a newly accepted turn receives history or terminal events from earlier work
- **THEN** watch remains bound to the current run and accepted work until its own terminal outcome or an observed failure is established
