## MODIFIED Requirements

### Requirement: Project-owned portable memory

The workflow SHALL store durable project knowledge as reviewable text files in the current project's Git working tree. It SHALL discover and reuse existing decision, context and verification records, provide a short entry index, and avoid maintaining duplicate authoritative copies. The global kit SHALL provide the workflow; project facts SHALL remain project-owned. Only knowledge needed to maintain the owning project and suitable for its publication boundary SHALL be retained. Private consumer identities, machine paths and operational evidence SHALL remain in that consumer's appropriate local storage, with reusable conclusions generalized in the kit. Native Codex memories, credentials, session transcripts and runtime databases MUST NOT be required or added to Git by this workflow.

#### Scenario: Existing decisions already have a home
- **WHEN** a project has an authoritative decision record and a new durable decision is confirmed
- **THEN** the workflow updates that record and references it from the memory index without copying the decision into a second authoritative store

#### Scenario: Fresh clone without personal history
- **WHEN** the project is cloned into another directory and Codex starts with the installed kit but without prior project sessions or native memories
- **THEN** the agent discovers and uses the checked-in memory through project instructions and the index

#### Scenario: An external consumer reveals a reusable constraint
- **WHEN** an agent records a finding learned from a private consumer
- **THEN** the kit records only the general constraint and its validation limits without copying the consumer's identity, domain data, logs or local paths

### Requirement: Bounded maintenance in the owning checkout

The agent SHALL record durable decisions during the task and verified reusable findings when established, without an unconditional extra model call or end-of-turn write. It SHALL preserve unrelated modifications, check current content before updating, and write only in the owning checkout. Maintenance SHALL replace superseded facts in their owning record, consolidate duplicated material, and remove transient or obsolete reports after necessary current facts and references are retained. Memory updates SHALL remain ordinary reviewable Git changes; automatic commit, push or cross-worktree propagation MUST NOT be implied.

#### Scenario: Concurrent worktrees update memory
- **WHEN** two tasks independently edit the same memory topic in separate worktrees
- **THEN** each task preserves its local evidence and Git exposes the integration conflict; the integrating agent resolves it from the current decisions and evidence instead of silently overwriting either task

#### Scenario: No durable knowledge was learned
- **WHEN** a routine task produces only transient progress or repeated information
- **THEN** the workflow finishes without adding a redundant memory entry

