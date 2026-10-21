## MODIFIED Requirements

### Requirement: Explicit activation with a bounded mandate

The installed managed lead SHALL automatically enable the two-stage reflection chain for its project and authorized work when `self_reflection_enabled` is true. No separate enable command SHALL be required. Activation SHALL retain the existing bounded mandate, process ownership and recovery checks. It SHALL not authorize unrelated work, new providers, weakened acceptance or destructive actions. No persistent observer model SHALL be launched.

#### Scenario: A user enables improvement
- **WHEN** a supported lead session starts with the flag true, or observes it become true
- **THEN** the installed interface prepares event access, scope and recovery without another activation command, manual endpoints or task identifiers, and ordinary work continues

#### Scenario: Duplicate activation
- **WHEN** another activation addresses the same work
- **THEN** it identifies the existing owner, preserves checkpoints and retained records, and launches no duplicate consumer or reflection

#### Scenario: Mode is disabled
- **WHEN** ordinary development proceeds with the flag false
- **THEN** no new automatic reflection runs and ordinary lead, executor and feedback behavior remains available

#### Scenario: First acceptance arrives before the host poll
- **WHEN** the flag is true and the lead accepts its first eligible task before automatic host preparation has run
- **THEN** acceptance prepares the existing lifecycle before closing the task, and the resulting event remains eligible for exactly one reflection

#### Scenario: A new lead resumes the project
- **WHEN** the previous owner is verifiably inactive and a supported successor starts with the flag true
- **THEN** the harness transfers current control through the existing verified succession path without requiring a separate command or rewriting retained acceptance identities

#### Scenario: The retained owner is live or unverifiable
- **WHEN** automatic preparation encounters another live owner or cannot establish its exit
- **THEN** it preserves ownership and retained work and exposes the concrete conflict without launching duplicate model work

### Requirement: Configuration permission gates new reflection model work

The installed configuration SHALL define `self_reflection_enabled`, a boolean defaulting to true. True SHALL automatically prepare reflection in the supported managed lead. False SHALL prevent new executor and lead reflection model calls while ordinary work remains available. Each new stage SHALL re-read the value. Retained work, identities, usage, recovery and event boundaries SHALL be preserved.

#### Scenario: The permission is omitted
- **WHEN** an existing or new configuration omits `self_reflection_enabled`
- **THEN** it defaults to true and automatically prepares the supported managed lead without another activation step

#### Scenario: Model work is disabled
- **WHEN** `self_reflection_enabled` is false
- **THEN** no new executor or lead reflection model call starts from manual, automatic, continuation or internal dispatch, enable refuses before lifecycle mutation, and ordinary acceptance still completes

#### Scenario: The flag changes between stages
- **WHEN** a host has retained an executor reflection and the flag changes from true to false before the lead stage
- **THEN** the executor result, identity, usage and pending lead stage remain retained, no lead model call starts, and a later true value resumes the retained lead stage without repeating the executor or requiring enable

#### Scenario: Status explains stopped model work
- **WHEN** the caller reads status while the flag is false or the lifecycle is not active
- **THEN** the result names the effective flag, configuration path and concrete reason no new model work may start, distinguishing absence of a supported host from an activation command requirement

#### Scenario: The permission has an invalid type
- **WHEN** the configuration supplies a string, integer or other non-boolean value
- **THEN** configuration loading fails before model work or lifecycle mutation
