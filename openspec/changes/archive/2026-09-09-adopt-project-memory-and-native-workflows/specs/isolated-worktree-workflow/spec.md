## Purpose

Использовать обычные Git worktrees для независимых задач Codex с явными исходными данными, корректным окружением и проверенной интеграцией результата.

## ADDED Requirements

### Requirement: Proportional isolation with explicit inputs

The global workflow SHALL use a separate Git worktree when independent writes, experiments or review isolation justify its setup and integration cost. It SHALL identify the repository, intended base revision, branch and required uncommitted inputs before dispatch. It MUST NOT silently substitute committed HEAD for a task that depends on local changes, or stash, reset, commit or discard unrelated user work.

#### Scenario: Task depends on dirty source
- **WHEN** a delegated task requires uncommitted parent changes
- **THEN** the workflow supplies a scoped reproducible snapshot of the required tracked and untracked inputs, or keeps the coupled work in the parent checkout and states the reason

#### Scenario: Small coupled edit
- **WHEN** worktree setup and integration would cost more than an ordinary bounded edit
- **THEN** the workflow supports direct work without requiring a new branch or worktree

### Requirement: Correct checkout and resources

Each isolated task SHALL establish the actual worktree root and project-native setup and verification commands before substantive edits. Any code-navigation tool used SHALL target that checkout and current source state. A worktree MUST NOT be treated as isolation of global configuration, accounts, services, ports or databases; conflicting mutable resources SHALL be allocated to the task or their use serialized.

#### Scenario: Linked Git metadata and stale tool context
- **WHEN** a worktree has a `.git` file and an available tool still identifies the main checkout
- **THEN** the agent selects and verifies the worktree context and refreshes its required index, or uses fresh scoped source with the limitation stated

#### Scenario: Test touches a shared service
- **WHEN** a worktree test would stop or mutate the service used by active Codex sessions
- **THEN** the test uses an owned isolated service target and never treats the worktree path alone as sufficient protection

### Requirement: Integration and recoverable cleanup

The integrating agent SHALL inspect the candidate diff, preserve unrelated changes, reconcile conflicts and run applicable checks on the integrated result. The workflow SHALL retain unfinished work and evidence until integration is accepted. It MUST NOT force-remove a dirty or unmerged worktree or delete a branch without establishing that its work is preserved and removal is authorized.

#### Scenario: Candidate passes but merged result fails
- **WHEN** checks pass in the task worktree but fail after integration
- **THEN** completion remains unaccepted and the source and evidence needed for correction remain available

#### Scenario: Global workflow in another project
- **WHEN** the installed workflow is used in an independent Git repository
- **THEN** it creates an isolated task from explicit inputs, exercises preparation and verification, integrates its owned result, and cleans up only resources proven safe to remove
