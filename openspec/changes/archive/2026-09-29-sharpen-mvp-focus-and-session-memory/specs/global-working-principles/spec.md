## ADDED Requirements

### Requirement: MVP-priority effort gate

The portable principles SHALL require the agent to classify substantial effort
beyond the next required deliverable - additional research, checks beyond
applicable acceptance, refactoring, hardening or infrastructure - by its effect
on the earliest verified end-to-end MVP: accelerating it, required by agreed
acceptance, or deferrable. Deferrable effort SHALL be recorded in its existing
owner, such as a specification task, board item or decision record, with its
trigger and priority, and SHALL neither displace MVP work nor be lost silently.
Mandatory acceptance, correctness, data integrity, prevention of serious harm
and substantiated P0/P1 failure paths SHALL retain priority over raw speed.

#### Scenario: A useful improvement would delay the MVP
- **WHEN** an improvement is genuinely useful but is not required for the agreed MVP acceptance and would displace MVP work
- **THEN** the agent records it as deferred work with its trigger in the existing owner and continues the MVP path instead of implementing it immediately

#### Scenario: Required acceptance work retains priority
- **WHEN** effort is required by agreed acceptance or addresses a substantiated P0/P1 failure path
- **THEN** the agent executes it now even though it extends elapsed time

#### Scenario: The user sets the release priority
- **WHEN** the user confirms which milestone or release the current focus targets
- **THEN** subsequent effort classification follows that priority until the user changes it

### Requirement: Main-session focus with scarce lead context

The portable principles SHALL define the main session's posture for substantial
work: coordination, judgment, ideation, deep analysis, decomposition,
integration and acceptance remain in the main session, and the main session
treats its own context and token budget as the scarce resource that justifies
this division. Substantial transferable execution - research, writing or
implementation - SHALL go through the applicable delegation route, meaning
configured executors through the `team-lead` skill or ordinary child agents
where executors are unavailable or unsuitable, under the existing activation
and cost rules. Work cheaper than its own handoff, small tasks that no
decomposition repays, and work that genuinely exceeds delegate capability stay
direct in the main session; the posture SHALL NOT manufacture helpers, tasks or
board records for tiny work.

#### Scenario: Transferable implementation stays out of the main context
- **WHEN** substantial work contains slices an available executor or child agent can complete from a bounded brief
- **THEN** the main session keeps judgment, decomposition, integration and acceptance, and dispatches the slices instead of implementing them in its own context

#### Scenario: Small work stays direct
- **WHEN** a task is cheaper than its own handoff or no useful decomposition exists
- **THEN** the main session completes it directly without a delegation record or a manufactured helper

#### Scenario: Bulk retrieval does not fill the main context
- **WHEN** the main session needs source or documentation facts to decide or to brief a delegate
- **THEN** it uses bounded retrieval for what the decision needs and delegates bulk collection or verification instead of loading it into its own context

## MODIFIED Requirements

### Requirement: Native instruction invariant checks

The native source check SHALL enforce the 25,088-byte portable-principles
limit and verify the repository-relative documentation owners declared by
token-audit. It SHALL name the violated invariant and fail without mutating
source. Checks SHALL use the same owner declarations as the report.
Compression MUST preserve normative requirements; passing a size check does
not establish semantic equivalence.

#### Scenario: Instruction growth exceeds the accepted limit
- **WHEN** the principles exceed 25088 bytes
- **THEN** the source check fails with actual and allowed sizes

#### Scenario: A declared report owner disappears
- **WHEN** a declared documentation owner is missing or outside the source root
- **THEN** the source check fails with the invalid owner route
