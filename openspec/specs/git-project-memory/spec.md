# git-project-memory Specification

## Purpose

Сохранять полезные знания в Git самого проекта и применять их в новых задачах, clone и worktrees без зависимости от личной истории Codex или отдельного сервера памяти.

## Requirements

### Requirement: Project-owned portable memory

The workflow SHALL store durable project knowledge as reviewable text files in the current project's Git working tree. It SHALL discover and reuse existing decision, context and verification records, provide a short entry index, and avoid maintaining duplicate authoritative copies. The global kit SHALL provide the workflow; project facts SHALL remain project-owned. Native Codex memories, credentials, session transcripts and runtime databases MUST NOT be required or added to Git by this workflow.

#### Scenario: Existing decisions already have a home
- **WHEN** a project has an authoritative decision record and a new durable decision is confirmed
- **THEN** the workflow updates that record and references it from the memory index without copying the decision into a second authoritative store

#### Scenario: Fresh clone without personal history
- **WHEN** the project is cloned into another directory and Codex starts with the installed kit but without prior project sessions or native memories
- **THEN** the agent discovers and uses the checked-in memory through project instructions and the index

### Requirement: Relevant retrieval and evidence

At the start of substantive project work and after a context reset, the agent SHALL consult the memory entry point and retrieve only knowledge relevant to the task. Entries SHALL distinguish confirmed user decisions, verified technical observations and tentative ideas; they SHALL identify scope, evidence and the date or source revision needed to reassess technical validity. Memory MUST NOT override current instructions or fresh source evidence.

#### Scenario: Irrelevant and stale entries coexist
- **WHEN** the index contains unrelated history and a relevant command whose referenced configuration has changed
- **THEN** the agent skips unrelated bodies, verifies the relevant command against current project sources, and updates or marks the stale entry before presenting it as current

#### Scenario: New instruction contradicts remembered preference
- **WHEN** the user explicitly changes a recorded preference
- **THEN** the agent follows the new instruction and reconciles the owning record, retaining any useful explanation of the superseded decision

### Requirement: Bounded maintenance in the owning checkout

The agent SHALL record durable decisions during the task and verified reusable findings when established, without an unconditional extra model call or end-of-turn write. It SHALL preserve unrelated modifications, check current content before updating, and write only in the owning checkout. Memory updates SHALL remain ordinary reviewable Git changes; automatic commit, push or cross-worktree propagation MUST NOT be implied.

#### Scenario: Concurrent worktrees update memory
- **WHEN** two tasks independently edit the same memory topic in separate worktrees
- **THEN** each task preserves its local evidence and Git exposes the integration conflict; the integrating agent resolves it from the current decisions and evidence instead of silently overwriting either task

#### Scenario: No durable knowledge was learned
- **WHEN** a routine task produces only transient progress or repeated information
- **THEN** the workflow finishes without adding a redundant memory entry

### Requirement: Global discovery with local adoption

The installed kit SHALL expose the memory workflow in repositories outside the harness and support project adoption through a concise project entry route. Verification SHALL exercise retrieval and an authorized update in an independent repository and retrieval from its fresh clone. Updating or disconnecting the global kit SHALL preserve the project's memory files and unrelated instructions.

#### Scenario: Kit is disconnected
- **WHEN** the user disconnects the kit after a project has adopted memory
- **THEN** the project's text records remain readable and versionable, and only kit-owned global registrations are removed
