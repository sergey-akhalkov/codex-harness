## ADDED Requirements

### Requirement: Native verification execution record
The installed process observer SHALL optionally record the selected verification scope, actual executable and arguments, cwd, Git identity when available, and content identities of explicitly declared source, provenance and build inputs. It SHALL preserve before/after identities and actual process outcomes in its existing local evidence lifecycle. Missing inputs SHALL fail before command execution; an interrupted, timed-out or failed run MUST NOT become a pass. Records SHALL state that unlisted inputs and external runtime state are not covered, and SHALL NOT automatically accept a task or reuse a previous pass.

#### Scenario: Successful command with changed inputs
- **WHEN** a command exits zero but a recorded input changes during execution
- **THEN** the receipt separately records natural exit and changed input identity without claiming unchanged verification

#### Scenario: Failure or interruption
- **WHEN** execution fails or times out
- **THEN** the original failure and captured identity remain available without being overwritten by a subsequent attempt

#### Scenario: Verification outside a Git repository
- **WHEN** a caller runs a valid verification command outside Git
- **THEN** Git identity is explicitly unavailable while executable, inputs and process results remain recorded

#### Scenario: An explicitly selected input uses a link
- **WHEN** a declared input resolves through an existing link or junction to a regular file
- **THEN** capture records its requested and resolved identity, including a target change during execution
