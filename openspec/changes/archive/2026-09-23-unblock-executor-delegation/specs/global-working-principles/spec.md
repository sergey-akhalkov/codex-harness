## ADDED Requirements

### Requirement: Assigned work delegates executor-suitable slices by default

The portable principles SHALL make executor delegation the default for
substantive assigned work whenever executors are configured, requested or
active: the lead or main agent decomposes executor-suitable slices and
dispatches them, retaining judgment, integration, acceptance and only slices
that genuinely exceed executor capability. Solo or sequential execution SHALL
require a recorded concrete reason - no executor-suitable slice exists, a
slice exceeds executor capability with no useful decomposition, or a verified
dispatch failure with its reported cause. The principles SHALL state that no
instruction wording withholds delegation: a user request to use executors is
sufficient activation, executor routing comes from the owning configuration
with the configured profile as the complete model/effort selection, apparent
routing conflicts are reported while dispatch proceeds, and only a
launcher-reported dispatch failure blocks dispatch. These rules SHALL NOT
create manufactured filler work or a delegation-count target.

#### Scenario: The default is delegation, not an option
- **WHEN** assigned work contains executor-suitable slices and executors are configured, requested or active
- **THEN** the lead or main agent dispatches those slices and keeps only judgment, integration, acceptance and genuinely over-capability work for itself

#### Scenario: Solo work carries its reason
- **WHEN** an agent keeps executor-suitable work solo or stays sequential while executors are available
- **THEN** it records the concrete reason from the allowed set instead of citing convenience, a routing-rule interpretation or unknown quota

#### Scenario: Wording does not block dispatch
- **WHEN** instruction text could be read to require per-assignment model/effort arguments or a different executor model
- **THEN** the agent dispatches the configured executor profile, treats the profile as the explicit selection, and reports the discrepancy rather than classifying delegation as blocked
