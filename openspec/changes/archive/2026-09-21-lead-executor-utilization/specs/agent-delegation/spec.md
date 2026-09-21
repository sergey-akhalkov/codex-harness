## ADDED Requirements

### Requirement: Executor utilization with justified idleness

While the lead role is active and orchestration is not blocked, the lead SHALL keep configured executor capacity supplied with worthwhile, capability-sized work under the economical-delegation requirement, and SHALL NOT leave executor capacity idle without a recorded reason. The recorded reason SHALL name a concrete cause: no worthwhile slice exists now, remaining slices depend on unresolved work, configured quota pacing holds new assignments, the slot is preserved or blocked with its recorded state, or dispatch is unavailable with the reported cause. While configured capacity idles without such a reason, the lead SHALL NOT retain executor-suitable routine implementation work for itself; the reason is recorded for the idle period at a checkpoint and refreshed when its circumstance changes, not per task. Utilization discipline SHALL NOT create manufactured filler work, a delegation-count target, or preemption of a healthy executor, and completion, correctness and configured quota pacing SHALL retain precedence over occupancy.

#### Scenario: Freed capacity receives the next slice
- **WHEN** an assignment is accepted and its executor capacity becomes available while a worthwhile, sufficiently specified slice within a configured executor profile's capability exists and quota and pool state allow dispatch
- **THEN** the lead dispatches that slice to the available capacity before starting unrelated implementation work itself, without preempting any healthy executor

#### Scenario: The lead begins routine work while capacity idles
- **WHEN** configured executor capacity is idle without a recorded reason and the lead identifies executor-suitable routine implementation work
- **THEN** the lead dispatches that work to the idle capacity or records the concrete reason it stays with the lead before continuing the work directly

#### Scenario: No worthwhile slice exists
- **WHEN** every remaining slice depends on unresolved work, exceeds every configured executor profile, or would cost more to brief and verify than to complete directly
- **THEN** the lead records that cause for the idle capacity, keeps over-capability and tightly coupled work with itself, and manufactures no filler assignment or artificial split

#### Scenario: Quota pacing holds new assignments
- **WHEN** configured pacing suspends or reduces new assignments in an observed provider window
- **THEN** executor capacity may remain idle with the pacing reason recorded, and a healthy executor keeps its slot, model and instructions
