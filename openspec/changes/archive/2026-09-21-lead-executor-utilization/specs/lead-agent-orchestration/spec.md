## ADDED Requirements

### Requirement: Utilization checkpoints and observable occupancy

The lead workflow SHALL check executor utilization at session start, at stage or epic planning, after each dispatch decision, and after each acceptance or slot release. At each checkpoint the lead SHALL either dispatch the next worthwhile, capability-sized slice to available executor capacity or record the concrete reason the capacity stays idle. Lead progress reporting SHALL state executor occupancy against the configured concurrency limit and the recorded reason for every idle executor or free slot, derived from board and executor-pool records without window polling or model status requests to active executors. When a slot is released after acceptance or explicit discard and a worthwhile dispatchable slice exists, the lead SHALL backfill that capacity before starting unrelated implementation work itself. Sessions without lead activation SHALL remain unchanged.

#### Scenario: Progress reports occupancy
- **WHEN** the lead reports progress while orchestration is active
- **THEN** the report states how many configured executor slots are busy and the recorded reason for every idle executor or free slot, derived from board and pool records

#### Scenario: A released slot is backfilled
- **WHEN** the lead merges accepted work and releases a slot while a worthwhile, dispatchable slice exists within a configured executor profile's capability
- **THEN** the lead dispatches that slice into the freed capacity before starting unrelated implementation work itself

#### Scenario: An idle reason is refreshed
- **WHEN** the circumstance behind a recorded idle reason changes, such as a dependency resolving, a quota window resetting, or a preserved slot returning to the pool
- **THEN** the next utilization checkpoint re-evaluates the capacity and either dispatches a worthwhile slice or records the updated reason

#### Scenario: An ordinary session is unchanged
- **WHEN** a session without lead activation works a small direct task
- **THEN** no executor is spawned, no orchestration state is created, and no utilization reporting is required
