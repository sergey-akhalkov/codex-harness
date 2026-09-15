## Purpose

Evaluate and provide an optional tool interface to Git-owned project knowledge while preserving a single authoritative record, selective access, portable text and safe maintenance independent of a running MCP server.

## ADDED Requirements

### Requirement: Single-owner optional memory interface

The workflow SHALL compare the existing project text route with the available Serena memory interface without predetermining migration. Every durable fact SHALL retain one authoritative project-owned record; an interface entry SHALL reference an existing owner rather than maintain a second authoritative copy. Project knowledge MUST remain ordinary versionable text and readable through native tools without Serena or personal session state. No synchronization service or machine-global store SHALL be required.

#### Scenario: Fact already exists in project documentation
- **WHEN** the optional interface exposes a fact with an existing owning record
- **THEN** it directs the agent to that record and updates remain in the owning checkout without copying the fact into another authoritative memory

#### Scenario: New topic is stored through Serena
- **WHEN** the selected interface creates a new project topic
- **THEN** the topic has a single versionable text owner, is reachable through the project entry route and remains readable when the MCP is unavailable

### Requirement: Selective and current memory use

The selected route SHALL reveal relevant topics and load their bodies only when useful to the task. It SHALL preserve the distinction between decisions, verified observations and tentative information, verify stale technical claims against current inputs, and obey current user instructions. Onboarding or maintenance MUST NOT unconditionally reload existing knowledge, duplicate project policies or create an extra model call or write when no durable knowledge was gained.

#### Scenario: Stale command and unrelated history
- **WHEN** a relevant topic contains a command whose underlying configuration changed alongside unrelated memory topics
- **THEN** the agent skips unrelated bodies, checks the command against current evidence and updates or marks the owning record before relying on it

#### Scenario: User changes a decision
- **WHEN** a current user instruction supersedes a remembered decision
- **THEN** the new instruction governs and the owner is reconciled without retaining conflicting authoritative copies

#### Scenario: No useful new knowledge
- **WHEN** a task only confirms an existing fact or reports transient progress
- **THEN** the memory remains unchanged without an unconditional maintenance invocation

### Requirement: Recoverable memory operations and references

The optional interface SHALL preserve project ownership and unrelated edits during update, rename, failure and worktree integration. It SHALL distinguish supported memory-reference maintenance from ordinary document-link validation. Read-only and ignored memory behavior SHALL be checked against the actual interface. Partial operations MUST remain explicit and recoverable; successful rename MUST NOT be assumed to be atomic or to update unsupported link formats.

#### Scenario: Rename affects different link formats
- **WHEN** a topic is renamed and both tool-specific references and ordinary document links target it
- **THEN** all affected references are checked by an applicable method, and unresolved references are reported rather than claiming complete propagation from the rename result alone

#### Scenario: Protected or interrupted memory update
- **WHEN** a write is rejected by a selected protection rule or fails after partial progress
- **THEN** the rejection leaves protected content unchanged, any partial state is identified, and recovery preserves existing facts and unrelated edits

#### Scenario: Worktrees contain conflicting updates
- **WHEN** two worktrees change the same topic independently
- **THEN** both versions and their evidence survive until explicit integration resolves the conflict using current user decisions and source evidence

### Requirement: Compared selection and portable consumption

Acceptance SHALL compare native and candidate routes on the same retrieval, stale-information, decision-update, no-op and reference-maintenance tasks with independent correctness criteria. It SHALL exercise a fresh clone without prior session history or Serena and preserve project data through kit disconnect. The result SHALL record either adopted scope with evidence of benefit or retention of the native route with a concrete reason. Missing mandatory correctness or portability evidence MUST remain incomplete rather than become a successful candidate rejection.

#### Scenario: Memory is excluded from Git or still untracked
- **WHEN** a topic exists locally but is ignored or absent from the revision used for the clone check
- **THEN** its local readability is not reported as portability; acceptance verifies the actual tracked fixture revision and clone contents while keeping runtime caches out of Git

#### Scenario: Candidate has no established advantage
- **WHEN** correctness and portability checks pass but the interface fails the declared usefulness criterion
- **THEN** the native route remains selected, the comparison and reason are documented, and no duplicate store or new global dependency is installed

#### Scenario: Fresh clone and disconnected kit
- **WHEN** an independent project is cloned with its memory and the optional MCP or kit is unavailable
- **THEN** its ordinary text entry route still identifies the relevant authoritative records, and disconnect has not deleted or moved project knowledge
