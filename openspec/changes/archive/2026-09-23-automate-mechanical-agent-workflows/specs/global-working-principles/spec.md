## ADDED Requirements

### Requirement: Native instruction invariant checks
The native source check SHALL enforce the existing 24 KiB portable-principles limit and verify the repository-relative documentation owners declared by token-audit. It SHALL name the violated invariant and fail without mutating source. Checks SHALL use the same owner declarations as the report. Compression MUST preserve normative requirements; passing a size check does not establish semantic equivalence.

#### Scenario: Instruction growth exceeds the accepted limit
- **WHEN** the principles exceed 24576 bytes
- **THEN** the source check fails with actual and allowed sizes

#### Scenario: A declared report owner disappears
- **WHEN** a declared documentation owner is missing or outside the source root
- **THEN** the source check fails with the invalid owner route
