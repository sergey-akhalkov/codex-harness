## ADDED Requirements

### Requirement: Quota-aware pacing across account windows

Dispatch and feedback cadence SHALL consider fresh available account-limit observations, their scope, reset times and observed use, prioritizing conservation of lead capacity for consequential decisions while following the orchestration configuration for routing. Account state SHALL be shared by the controller's tasks on that account while task work and authorization remain separate. Missing or stale telemetry SHALL remain unknown, not zero or unlimited; local request logs SHALL NOT be treated as an authoritative subscription remainder; no model call SHALL be made solely to estimate remaining quota. Pacing SHALL adjust new work allocation, supported concurrency, reasoning effort and feedback cadence without abandoning an active healthy executor, dropping accepted work or adding purchases. Reset-time bursts SHALL be avoided across tasks sharing an account.

#### Scenario: Lead capacity is depleting rapidly
- **WHEN** comparable current observations show the lead account window depleting quickly
- **THEN** new assignments and nonessential coordination are paced to preserve consequential decisions while healthy executors finish their owned work

#### Scenario: Several tasks share one account
- **WHEN** multiple tasks would retry or resume on the same account at its reset time
- **THEN** retry eligibility is distributed instead of issuing a synchronized burst
