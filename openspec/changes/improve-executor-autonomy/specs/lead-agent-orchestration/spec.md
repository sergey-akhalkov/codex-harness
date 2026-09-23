## ADDED Requirements

### Requirement: Observable executor lifecycle and bounded result

The existing dispatch and receipt owners SHALL distinguish request acceptance, terminal creation, observed native start, running, completion, failure and lead acceptance. Native session identity, checkout/base, changed files, actual reported checks and outcomes, remaining work, limitations, required decision and a detail locator SHALL be available without manual rollout searches. Completion and errors SHALL reach the lead through the native observation path. Empty completion SHALL be an output defect, not evidence of model, authentication or quota unavailability. Every model conversation SHALL retain its distinct visible terminal surface.

#### Scenario: Terminal opens but native start fails
- **WHEN** a dispatch creates its terminal and its child cannot start or exits unsuccessfully
- **THEN** the receipt and observer report the actual stage and failure, preserve diagnostics and do not report successful model execution

#### Scenario: Work completes
- **WHEN** an executor finishes its turn
- **THEN** a native observer emits a bounded completion or output-defect event with the exact session, slot, result and detail locator without model-side status polling

#### Scenario: Interrupted assignment resumes
- **WHEN** an original assignment stops with partial work
- **THEN** continuation uses its recorded exact session and checkout without reset, retains visible identity and preserves changes for correction and acceptance

#### Scenario: Accepted slot is released
- **WHEN** the lead accepts and integrates an outcome and records its disposition
- **THEN** the existing release operation records that decision before resetting the slot for reuse; unreviewed work remains preserved

### Requirement: Shared heavy-command admission

Executor heavy commands SHALL use the existing resource and process ownership mechanisms to serialize admission under one installed aggregate budget. Independent reads, analysis and edits SHALL remain concurrent. Executor-specific limits SHALL NOT multiply the aggregate allowance. Queue admission and release SHALL be mechanical, with observable waiting, execution, exit and failure. Machine resource settings SHALL remain local.

#### Scenario: Two executors require heavy commands
- **WHEN** independently working executors request overlapping heavy checks
- **THEN** only the admitted command tree consumes the shared allowance while the other waits without lead-managed grants

#### Scenario: Resource owner ends
- **WHEN** an admitted command completes, fails or is interrupted
- **THEN** its command tree and admission ownership are released appropriately and a subsequent permitted request can proceed without deleting a live owner's lock

### Requirement: Installed autonomy acceptance

Delivery SHALL exercise a small real Rust repair through installed components first, then independent executors, aggregate command admission, clear startup failure, partial-work preservation, continuation and release. Checks SHALL use synthetic projects or owned fixtures and keep private inputs outside the public kit. Global delivery SHALL use the supported lifecycle and preserve unrelated active consumers; a conflicting activation SHALL retain the ready candidate and name the remaining dependency.

#### Scenario: Activation conflicts with another consumer
- **WHEN** the normal installation lifecycle refuses activation because an unrelated active consumer owns affected state
- **THEN** the candidate and its evidence remain available and the conflict is reported without stopping that consumer or replacing links out of band
