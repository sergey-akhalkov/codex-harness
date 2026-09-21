## MODIFIED Requirements

### Requirement: Economical delegation through configured roles

The lead SHALL assign worthwhile complete workstreams to configured executors according to reasoning needs, modality, tools, ambiguity, error consequences and verification cost. Before dispatching a slice, the lead SHALL perform the analysis that slice needs to become sufficiently specified for its executor profile: requirement interpretation, risk and consequence decisions, approach direction and acceptance conditions. Slice difficulty SHALL be matched to the configured executor profiles' reasoning capability: a slice whose reasoning demands exceed every available executor profile SHALL stay with the lead, be decomposed further into capability-sized slices, or use the bounded principal-consultation path, and SHALL NOT be delegated as-is. It SHALL retain requirements, consequential decisions, overall acceptance and merge responsibility, while executors own investigation, implementation, applicable checks and correction within their assignments and capabilities. The lead SHALL consider handoff, coordination, waiting, integration and rework, avoid solving delegated work in parallel, and perform a small or tightly coupled task directly when delegation would cost more. Completion and correctness SHALL take precedence over quota minimization; delegation count SHALL NOT be a success criterion.

#### Scenario: Independent routine work is available
- **WHEN** a task contains sufficiently specified independent work that benefits from delegation and a configured executor is available
- **THEN** the lead assigns the complete outcome to that executor in its own worktree and continues only non-overlapping useful work or waits without duplicating the investigation

#### Scenario: Delegation would add overhead
- **WHEN** a trivial or tightly coupled task costs more to brief and verify than to complete directly
- **THEN** the lead completes it directly without a mandatory agent round trip

#### Scenario: An over-difficult slice is not delegated as-is
- **WHEN** the lead's pre-dispatch analysis finds that a candidate slice's reasoning demands exceed every available executor profile
- **THEN** the slice stays with the lead, is decomposed into capability-sized slices, or goes through the bounded principal consultation, and no executor receives the unmodified slice

#### Scenario: A sufficiently specified slice is dispatched
- **WHEN** the lead's analysis has fixed a slice's requirement interpretation, approach direction, ownership and acceptance conditions within an executor profile's capability
- **THEN** the lead dispatches the complete outcome with those decisions carried by the brief instead of leaving them to the executor to derive

#### Scenario: Bounded investigation remains executor work
- **WHEN** a delegated slice needs exploration inside its boundaries and the configured executor profile can perform that exploration
- **THEN** the executor investigates within its assignment instead of the lead pre-solving every unknown before dispatch
