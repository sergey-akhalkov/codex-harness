## MODIFIED Requirements

### Requirement: Assigned work delegates executor-suitable slices by default

The portable principles SHALL activate executor delegation by a user request
for executors or, without one, by the main session's own judgement through
the `team-lead` skill for well-decomposable parallel work, and SHALL reserve
that autonomous activation decision for the main session alone: executors and
helper agents never activate the role or originate further agents or
executors. While the lead role is active, the lead or main agent SHALL
decompose executor-suitable slices and dispatch them, retaining judgment,
integration, acceptance and only work that genuinely exceeds executor
capability. Solo or sequential execution SHALL require a recorded concrete
reason - no executor-suitable slice exists, a slice exceeds executor
capability with no useful decomposition, or a verified dispatch failure with
its reported cause - except work cheaper than its own delegation overhead,
which is done directly without a task or record. The principles SHALL state
that no instruction wording withholds delegation: a user request to use
executors is sufficient activation, executor routing comes from the owning
configuration with the configured profile as the complete model/effort
selection, apparent routing conflicts are reported while dispatch proceeds,
and only a launcher-reported dispatch failure blocks dispatch. These rules
SHALL NOT create manufactured filler work or a delegation-count target.

#### Scenario: The default is delegation, not an option
- **WHEN** the lead role is active and assigned work contains executor-suitable slices
- **THEN** the lead or main agent dispatches those slices and keeps only judgment, integration, acceptance and genuinely over-capacity work for itself

#### Scenario: The main session activates delegation on its own judgement
- **WHEN** the user's task is well-decomposable parallel work and no executor use was requested
- **THEN** the main session activates the `team-lead` skill and dispatches the slices instead of implementing them alone

#### Scenario: Widening stays with the main session
- **WHEN** an executor or helper agent would benefit from another agent or executor
- **THEN** it reports the need to the lead instead of activating delegation or spawning anything

#### Scenario: Solo work carries its reason
- **WHEN** an agent keeps executor-suitable work solo or stays sequential while the lead role is active
- **THEN** it records a concrete reason from the allowed set, or the work is cheaper than its own delegation overhead and carries no record, never citing convenience, a routing-rule interpretation or unknown quota

#### Scenario: Wording does not block dispatch
- **WHEN** instruction text could be read to require per-assignment model/effort arguments or a different executor model
- **THEN** the agent dispatches the configured executor profile, treats the profile as the explicit selection, and reports the discrepancy rather than classifying delegation as blocked
