## MODIFIED Requirements

### Requirement: Shared heavy-command admission

Executor heavy commands SHALL use the existing resource and process ownership mechanisms to admit a bounded number of concurrent command trees under one installed aggregate memory envelope and one aggregate CPU ceiling. Independent reads, analysis and edits SHALL remain concurrent. Executor-specific limits SHALL NOT multiply the aggregate memory, CPU or slot allowance. Queue admission and release SHALL be mechanical, with observable waiting, execution, exit and failure. Machine resource settings SHALL remain local.

#### Scenario: Two executors require heavy commands
- **WHEN** independently working executors request overlapping heavy checks and a free slot remains
- **THEN** both admitted command trees run concurrently under the shared aggregate memory envelope and CPU ceiling without lead-managed grants, while a caller beyond the slot bound waits observably

#### Scenario: Resource owner ends
- **WHEN** an admitted command completes, fails or is interrupted
- **THEN** its command tree and admission ownership are released appropriately and a subsequent permitted request can proceed without deleting a live owner's lock