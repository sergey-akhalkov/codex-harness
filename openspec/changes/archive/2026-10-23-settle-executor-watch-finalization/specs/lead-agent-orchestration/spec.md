## MODIFIED Requirements

### Requirement: Observable executor lifecycle and bounded result

The existing dispatch and receipt owners SHALL distinguish request acceptance, terminal creation, observed native start, running, completion, failure and lead acceptance. Native session identity, checkout/base, changed files, actual reported checks and outcomes, remaining work, limitations, required decision and a detail locator SHALL be available without manual rollout searches. Completion and errors SHALL reach the lead through the native observation path. Empty completion SHALL be an output defect, not evidence of model, authentication or quota unavailability. Every model conversation SHALL retain its distinct visible terminal surface.

For managed TUI runs, `executor watch` SHALL retain its existing receipt/slot addressing, options, blocking behavior, timeout behavior, bounded text/JSON review result and exit meanings: 0 for completion, 1 for an unsuccessful terminal outcome, and 2 for timeout or unavailable coverage. Native TUI presentation SHALL NOT by itself make coverage unavailable. Watch SHALL learn completion from the current run's correlated native outcome with the final result or output defect preserved, independently of the frontend's willingness to exit. Prior-session or prior-turn events, quiet output and frontend process termination alone SHALL NOT establish successful completion. A timeout SHALL NOT stop the run. No model-side status polling, periodic model messages, terminal scraping or manual rollout search SHALL be required to wait. A lost host or connection SHALL remain diagnosable and SHALL NOT be reported as success or leave waiting unbounded beyond the requested timeout. Historical receipts SHALL remain honestly readable without inventing missing coverage.

A native turn-completion observation SHALL NOT authorize a successful watch return while the current host outcome or retained final result is still pending. Watch SHALL continue observing that same run until its finalized success or output defect is available, an original failure is established, or the requested timeout expires. Waiting reports SHALL identify pending finalization without treating quiet output, a missing exit status or an absent result file as proof of success. Finalized success SHALL include the recorded successful exit status and readable bounded result; it SHALL NOT depend on the frontend closing.

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

#### Scenario: Turn completion precedes host finalization

- **WHEN** the current run reports native turn completion while its final exit status and result are not yet retained
- **THEN** the same watch call continues observing that run and does not return successful completion or an output defect merely because finalization is pending

#### Scenario: Finalization completes during watch

- **WHEN** that run subsequently retains its successful host outcome and nonempty final result before the requested timeout
- **THEN** watch returns success with the same run identity, actual exit status and retained bounded result in either text or JSON mode

#### Scenario: Finalization remains incomplete at the timeout

- **WHEN** the requested timeout expires while completion finalization remains unverified
- **THEN** watch returns the existing timeout or unavailable result with an explicit finalization cause, preserves the run and its original diagnostics, and neither reports success nor dispatches another model request

