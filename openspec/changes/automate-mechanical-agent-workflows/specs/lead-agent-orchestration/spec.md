## ADDED Requirements

### Requirement: Validated structured executor assignment
Executor dispatch SHALL accept a structured assignment as an alternative to existing free text. It SHALL validate declared existing inputs and intended output paths against the allocated checkout, reject missing inputs and path escapes before a model request, and generate the brief with the actual checkout and full committed base. Newly created output files MUST NOT be required to exist. The assignment SHALL retain the agent-selected objective, scope, invariants and acceptance conditions. Resume SHALL preserve partial work and regenerate checkout context without resetting the slot. Existing free-text callers MUST remain compatible.

#### Scenario: Missing declared input
- **WHEN** a structured assignment names an input absent from the allocated checkout
- **THEN** dispatch fails before launching a model and reports the exact missing input

#### Scenario: Input escapes the checkout
- **WHEN** a declared path is absolute, traverses outside the checkout or resolves through an escaping link
- **THEN** the assignment is rejected without reading or writing the escaped target

#### Scenario: New output and existing input
- **WHEN** inputs exist and an owned output is new
- **THEN** dispatch produces a brief naming the actual checkout, base, inputs, outputs and acceptance

#### Scenario: Read-only rendering uses the actual committed base
- **WHEN** a caller supplies a base to the validation/render command
- **THEN** it resolves a full commit and refuses an invalid or mismatching base without changing the checkout
