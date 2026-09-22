## ADDED Requirements

### Requirement: Installed deterministic feedback operations
An installed command SHALL expose feedback recording, agent-selected triage, ledger inspection, promotion eligibility and explicit promotion using the existing bd-backed algorithms. It SHALL preserve distinct-reporter/episode vote rules, exclude diagnostic votes, honor configured batch and promotion limits, and return compact observable results. Semantic grouping and consequence decisions remain caller inputs. Repeated execution MUST NOT add duplicate counted votes or repeat completed promotions. A partial failure SHALL identify applied and failed operations and return nonzero; it SHALL NOT claim atomic success or discard retained board history. Read-only inspection MUST NOT mutate the board. No operation SHALL invoke a model, create another tracker or grant implementation authority.

#### Scenario: Repeated vote and diagnostic observation
- **WHEN** triage includes a repeated reporter/episode or a diagnostic observation
- **THEN** the counted total remains unchanged for that observation

#### Scenario: Failure after an applied action
- **WHEN** a later bd operation fails after an earlier action succeeded
- **THEN** the command returns the applied prefix, the failed action and a nonzero result suitable for recovery

#### Scenario: Real consumer board
- **WHEN** the globally delivered command is used on an isolated outside-checkout bd board
- **THEN** its observable ledger and issue state agree with the native command results

#### Scenario: Consumer uses customized installed limits
- **WHEN** a consumer has no kit configuration of its own and the caller supplies no source override
- **THEN** the command uses the installed kit's limits and identifies that configuration source
