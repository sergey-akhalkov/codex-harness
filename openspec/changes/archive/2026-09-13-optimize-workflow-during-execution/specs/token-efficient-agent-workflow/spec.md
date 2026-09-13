## ADDED Requirements

### Requirement: Prepared replay and restart compete on complete remaining cost

When repeated interaction or preparation dominates, the workflow SHALL compare continuing valid owned state, returning it to a known starting state, and restarting the minimum necessary component. It SHALL learn uncertain transitions through observed actions, exercise their replay through the existing automation owner and preserve the actual parent environment where its differences matter. A single persistent launch SHALL NOT be mandatory when a reliable restart is cheaper or necessary. Held or identity-bound artifacts SHALL NOT be changed underneath an active operation; tested candidate changes shall be integrated after the relevant release and revalidation.

#### Scenario: Several unknown interactions share an expensive startup
- **WHEN** the application is prepared and its relevant state and ownership remain valid
- **THEN** the agent observes individual transitions and return paths, tests the corresponding automation there, and completes the required integrated run rather than rebuilding the entire environment to discover each next control

#### Scenario: Reset is slower or less reliable than restart
- **WHEN** current evidence shows that returning to the starting state costs more or cannot restore necessary conditions
- **THEN** the agent restarts the necessary component, preserves reusable unaffected preparation and re-establishes invalidated state

#### Scenario: A separate diagnostic host omits a required condition
- **WHEN** the reduced environment lacks a configuration, observer, timing or recovery condition used by the real entry point
- **THEN** the agent does not treat its passing result as parent acceptance and restores the relevant condition before relying on the reduction

### Requirement: Parallel optimization investigations feed the next delivery decision

When delegation is available, authorized and useful, agents SHALL assign a bounded independent question or correction that can shorten the current path to acceptance. The brief SHALL identify the parent decision or integration point that will consume the result. Agents SHALL include coordination and integration costs, preserve visibility and aggregate resource limits, and keep one owner for shared runtime resources. They SHALL consume useful findings when available and reconsider assignments whose premises have become obsolete. Delegation SHALL NOT be a compulsory periodic activity or a reason to delay a cheaper direct task.

#### Scenario: A worker can resolve a missing observation path
- **WHEN** the parent can continue useful work while an independent worker checks an existing step or attach capability
- **THEN** the parent consumes the verified finding in its next relevant action without duplicating the investigation or sharing uncontrolled access to the same runtime

#### Scenario: Parallel activity does not advance acceptance
- **WHEN** a proposed worker would polish an unrelated component, duplicate current investigation or cost more than the remaining direct work
- **THEN** the agent leaves that work out and continues the accepted path without manufacturing a delegation requirement
