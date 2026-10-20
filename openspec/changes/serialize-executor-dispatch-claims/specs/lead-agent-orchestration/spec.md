## ADDED Requirements

### Requirement: Executor startup retains exclusive assignment ownership

Executor dispatch SHALL retain exclusive slot, owner-generation and assignment ownership from claim through verified host handoff. A missing host lease SHALL NOT permit a competing dispatch or recovery to replace pending work. Startup exclusion SHALL preserve the shared pool's base-selection, held-task continuation, capacity and unreviewed-work contracts; it SHALL end at handoff rather than serialize model conversations.

#### Scenario: A pending dispatch has no host lease yet
- **WHEN** one dispatch has claimed a slot and is preparing synchronization, assignment or host startup while another dispatch selects a slot
- **THEN** the pending claim remains exclusively owned and the contender cannot replace its assignment or receipt merely because the first host lease does not yet exist

#### Scenario: Simultaneous callers share an owner identifier
- **WHEN** two dispatches for the same source and owner overlap before either host handoff completes
- **THEN** at most one owns that startup generation and the other waits within the configured bound or refuses without replacing work or creating another live conversation for the slot

#### Scenario: Startup ends before handoff
- **WHEN** the creating dispatch fails or its process exits before a host takes ownership
- **THEN** recovery resolves exact process and claim identities, preserves uncertain or unreviewed state and the original error, and reclaims only a safely resolved unused reservation

#### Scenario: Startup ownership becomes a live host lease
- **WHEN** the intended host accepts its exact assignment and publishes verified live ownership
- **THEN** startup exclusion is released through that handoff, the host lease protects the slot and independent conversations continue up to configured capacity

#### Scenario: Shared pool behavior remains intact
- **WHEN** dispatch continues a held task or uses an available explicit local base through the shared pool
- **THEN** startup protection introduces neither an extra fetch requirement nor permission to reset retained HEAD or partial work, and both pool and executor admission limits remain enforced

#### Scenario: A correction is checked against the demonstrated race
- **WHEN** a proposed correction is evaluated on the declared controlled overlapping startup schedules
- **THEN** the same independent checker exposes the violation on a qualified historical control and observes zero overwritten assignments, false accepted receipts, duplicate live bindings or capacity violations on the candidate, with original failure and recovery paths preserved
