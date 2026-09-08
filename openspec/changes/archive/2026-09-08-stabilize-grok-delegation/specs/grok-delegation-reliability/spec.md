## Purpose

Keep delegated Grok work progressing through delayed tools and distinguish task-output failures from actual subscription unavailability.

## ADDED Requirements

### Requirement: Delayed operations remain part of the assignment

The globally supplied Grok middle SHALL obtain pending tool results before dependent work or final delivery. A running operation SHALL NOT be reported as completed work. The agent SHALL return a nonempty final describing the actual result, applicable checks and remaining blockers.

#### Scenario: A tool yields before its result is ready
- **WHEN** an operation returns a running handle
- **THEN** middle retrieves its result with the matching continuation tool and completes the dependent task without a parent reminder

#### Scenario: A delayed operation fails
- **WHEN** a pending operation finishes with an error
- **THEN** middle reports or corrects that error within its assignment and does not invent successful output

### Requirement: Evidence determines coordination and reserve selection

The parent SHALL allow running children to work without repeated status-only messages. Wait intervals SHALL NOT act as task deadlines. Empty or status-only completion SHALL be treated as an output defect and inspected against current partial work and visible tool events. It SHALL NOT by itself justify middle_backup, which remains limited to absent middle or observed model, authentication or quota unavailability. A corrective continuation SHALL convey new actionable information; unchanged retries and repeated nudges SHALL be avoided.

#### Scenario: Output is missing but no provider failure is observed
- **WHEN** middle completes without an accepted result and no model, authentication or quota failure is established
- **THEN** the parent inspects available work, corrects an established cause or completes the task directly, without attributing a subscription outage or automatically switching to the reserve

#### Scenario: A child is still running
- **WHEN** a bounded status wait expires while a child is running
- **THEN** the parent continues independent work or waits appropriately without interrupting solely because of that interval

### Requirement: Opaque state is distinct from task evidence

Agents SHALL assess task results from visible messages, tool outcomes, artifacts and checks. Opaque reasoning or compaction fields SHALL NOT be treated as unreadable task files, completion evidence or proof of a provider outage. Actual decoding or verification errors SHALL retain their observed error classification.

#### Scenario: A transcript contains opaque reasoning state
- **WHEN** an otherwise usable transcript contains encrypted state
- **THEN** the parent preserves that state, uses visible task evidence and does not require decryption to accept work

### Requirement: Global regression acceptance

The kit SHALL retain failure-preserving checks for delayed continuation and result acceptance, and verify its installed Grok middle from an owned workspace outside this checkout. Model and subscription identity SHALL remain explicit; acceptance SHALL NOT stop the shared proxy or change billing paths.

#### Scenario: The installed middle executes a useful bounded task
- **WHEN** the updated role is loaded in a new outside session
- **THEN** Grok retrieves a delayed result, performs dependent work and returns verifiable evidence without parent nudges or a reserve agent
