# Spec Delta

## MODIFIED Requirements

### Requirement: Proportional isolation with explicit inputs

The global workflow SHALL use a separate pooled Git worktree when independent writes, experiments or review isolation justify its setup and integration cost. It SHALL identify the repository, exact committed base, task owner and required uncommitted inputs before acquisition. It MUST NOT silently substitute committed HEAD for a task that depends on local changes, or stash, reset, commit or discard unrelated user work. Acquisition SHALL NOT itself authorize committing inputs or copying mutable parent files into a live task checkout.

#### Scenario: Task depends on dirty source
- **WHEN** an isolated task requires uncommitted parent changes
- **THEN** the workflow uses an authorized committed snapshot containing the required inputs, or keeps the coupled work in the parent checkout and states the reason; it does not dispatch from an incomplete base or copy files into a live task checkout

#### Scenario: Small coupled edit
- **WHEN** worktree setup and integration would cost more than an ordinary bounded edit
- **THEN** the workflow supports direct work without requiring a new branch or worktree

### Requirement: Integration and recoverable cleanup

The integrating agent SHALL inspect the candidate diff, preserve unrelated changes, reconcile conflicts and run applicable checks on the integrated result. The workflow SHALL retain unfinished work and evidence until integration is accepted. It MUST NOT force-remove a dirty or unmerged worktree or delete a branch without establishing that its work is preserved and removal is authorized. Accepted tasks SHALL return their slot through the common release operation for reuse, preserving ignored build caches. Directory deletion SHALL remain a separately authorized retirement operation rather than normal task completion.

#### Scenario: Candidate passes but merged result fails
- **WHEN** checks pass in the task worktree but fail after integration
- **THEN** completion remains unaccepted and the source and evidence needed for correction remain available

#### Scenario: Global workflow in another project
- **WHEN** the installed workflow is used in an independent Git repository
- **THEN** it acquires an isolated task from explicit inputs, exercises preparation and verification, integrates its owned result, releases the slot safely and uses the same directory for a later task

## REMOVED Requirements

### Requirement: Ordinary Git worktrees, not Codex-managed checkouts

**Reason**: Separate allocation rules for ordinary tasks and executors conflict with one reusable pool for every task needing isolation.

**Migration**: Use the common harness pool for ordinary isolated tasks and executor dispatch. Existing non-pool Git or Codex-managed worktrees remain preserved and visible as foreign or legacy resources; they are not silently adopted, reset or deleted. Retire the standalone skill only after the installed common task entry point is available.

## ADDED Requirements

### Requirement: One pool for every isolated task in a repository

The harness SHALL provide one managed worktree pool per Git repository, shared by ordinary isolated tasks, experiments, reviews and executors. Entering through the main checkout or any linked worktree SHALL resolve the same pool identity and inventory. All managed allocation SHALL use this owner; task type, session restart or a different checkout path MUST NOT create another pool.

#### Scenario: Ordinary work and an executor share capacity
- **WHEN** an ordinary experiment occupies one slot and an executor requests isolation in the same repository
- **THEN** both allocations appear in one inventory and count toward the same capacity

#### Scenario: Acquisition from a linked worktree
- **WHEN** acquisition is requested from a linked worktree of a repository whose pool already exists
- **THEN** it resolves that existing pool and does not create nested slots or a second sibling pool

#### Scenario: Distinct repositories
- **WHEN** tasks target two independent Git repositories
- **THEN** each repository has its own pool and task ownership without treating a worktree of another repository as a reusable slot

### Requirement: Suppress the competing native worktree tool surface

Harness-managed ordinary, lead and executor sessions SHALL suppress the native `codex_worktrees` MCP surface through supported Codex configuration. Acceptance SHALL verify absence from effective tool catalogues and a working harness pool. Installation, update and recovery SHALL preserve existing worktrees and unrelated settings.

#### Scenario: Fresh ordinary and executor sessions expose one allocator
- **WHEN** the installed harness starts fresh ordinary/lead and executor sessions with the common pool selected
- **THEN** their effective tool catalogues omit the `codex_worktrees` tools and the installed harness acquisition and reuse path remains functional
- **AND** skill retirement, an unverified configuration value or instruction-only avoidance SHALL NOT count as disabling the tool surface

#### Scenario: An existing conversation retains native tools
- **WHEN** a saved or running conversation still exposes the native worktree tools after configuration changes
- **THEN** inspection reports the remaining tools and the supported restart or configuration remedy, without claiming that the current conversation has been updated
- **AND** an explicit override or host-injected surface that re-exposes the competing tools SHALL be reported as a configuration conflict rather than successful managed-pool delivery

#### Scenario: Disabling the surface preserves existing work
- **WHEN** the configuration is installed, updated or rolled back while foreign or Codex-managed worktrees exist
- **THEN** those directories, pending changes and recovery inputs remain intact, unrelated tools retain their settings, and any limitation on inspection or recovery is exposed before dependent use

### Requirement: Hard capacity with lazy reusable slots

The pool SHALL have a positive configured capacity N. It SHALL create stable slots lazily and MUST NOT create, register or replace a managed slot outside positions 1 through N. Existing suitable free slots SHALL be reused. Live, suspended, reserved and unreviewed tasks SHALL consume capacity. Exhaustion SHALL return the occupied slots and causes without allocating overflow worktrees or evicting held work.

#### Scenario: Sequential tasks do not accumulate directories
- **WHEN** twenty tasks are acquired, completed and released with pool capacity three
- **THEN** at most three managed worktrees exist, previously created eligible paths are reused and completion does not create task-specific replacement directories

#### Scenario: All slots are held
- **WHEN** capacity is four and the four slots contain any mixture of live, suspended, reserved or unreviewed tasks
- **THEN** another acquisition reports capacity exhaustion without creating a fifth slot, using the shared checkout as a substitute or discarding another task

#### Scenario: Missing or colliding slot
- **WHEN** a slot is registered but missing, or its expected path belongs to an unrelated directory or repository
- **THEN** acquisition preserves that state and reports the cause and recovery action without creating a substitute path beyond the configured pool

### Requirement: Independent pool capacity and executor concurrency

Orchestration configuration SHALL expose independent `worktree_pool_size` and `max_concurrent_executors` limits. The shipped defaults SHALL be four for each. Every held isolated task counts against pool capacity; only active executor runs count against executor concurrency. Changing one explicit setting MUST NOT silently change the other, and `worktree_limit` SHALL remain compatibility-only.

#### Scenario: Four directories and two executors
- **WHEN** pool capacity is four and executor concurrency is two
- **THEN** at most four managed slots exist and at most two executors run, while ordinary or suspended tasks can occupy the remaining slots

#### Scenario: A free directory does not grant executor concurrency
- **WHEN** the executor concurrency limit is reached and a pool slot is free
- **THEN** executor dispatch is refused with the concurrency cause without launching another executor

#### Scenario: Suspension preserves capacity
- **WHEN** an executor stops with its task suspended
- **THEN** its run ceases consuming executor concurrency but its retained slot continues consuming pool capacity until explicit release

### Requirement: Acquisition without launching an executor

The installed harness SHALL support acquiring and continuing an isolated task without starting an executor or another model conversation. Acquisition SHALL return the repository identity, stable slot path and index, exact base, task ownership handle and disposition. Repeating acquisition for the same held task SHALL recover its binding without resetting partial work or allocating another slot.

#### Scenario: Main-session experiment
- **WHEN** the main session requests a separate checkout for an experiment
- **THEN** the harness returns an owned pool slot without launching a model, and the session uses the returned checkout and establishes its tool context before edits

#### Scenario: Lost acquisition response
- **WHEN** a caller retries acquisition after losing the response for the same task
- **THEN** it receives the existing binding or a specific recoverable in-progress state rather than a second worktree

#### Scenario: Repeated task identity with a different base
- **WHEN** a held task is reacquired with a conflicting base request
- **THEN** the harness preserves its current files and binding and reports the conflict instead of silently synchronizing it to the new revision

### Requirement: Exact base without unnecessary network access

A newly acquired free slot SHALL reach and record the exact committed base before task work starts. An explicit locally available commit SHALL be usable without fetching a remote. A request requiring remote resolution or refresh SHALL report any failure before resetting the slot and MUST NOT silently substitute stale state. Reusing a held task SHALL preserve its current HEAD and partial work.

#### Scenario: Local commit while offline
- **WHEN** an explicit requested commit exists locally and the remote is unavailable
- **THEN** acquisition succeeds from that commit without requiring a fetch

#### Scenario: Requested remote refresh fails
- **WHEN** obtaining the requested base requires a remote operation that fails
- **THEN** acquisition reports that failure, preserves the previous checkout state and starts no task from a substituted base

#### Scenario: Tool context follows the returned checkout
- **WHEN** a task starts in a newly acquired slot
- **THEN** its observed HEAD equals the returned base and its code-navigation and verification context identify the returned checkout

### Requirement: Exclusive and recoverable task ownership

A slot SHALL have at most one task owner. Ownership SHALL span acquisition, executor startup when applicable, work, suspension and review, including periods without a live process. Competing callers MUST NOT replace a pending claim or assignment. An uncertain owner SHALL be preserved; elapsed time or an absent process alone SHALL NOT authorize reassignment.

#### Scenario: Competing acquisition of the last slot
- **WHEN** two different tasks concurrently request the last free slot
- **THEN** at most one acquires it, the other receives an actionable refusal and neither creates an overflow worktree

#### Scenario: Interruption before executor host handoff
- **WHEN** dispatch holds a slot but its executor host has not yet published a live binding
- **THEN** another request cannot overwrite its ownership, assignment or receipt; recovery distinguishes a confirmed abandoned unused reservation from unfinished work

#### Scenario: Host exits with a clean but held task
- **WHEN** a process exits before the task has been accepted or explicitly abandoned
- **THEN** its slot remains held even if the Git working tree is clean

### Requirement: Continuation respects current slot ownership

Harness continuation SHALL validate the task ownership handle and current session binding before returning or launching into a pooled checkout. Restarting a conversation for a held task SHALL retain its work and update the session binding explicitly. A stale binding from a released task MUST NOT resume through the harness into a slot assigned to another task.

#### Scenario: Interrupted task continues
- **WHEN** a held task is continued after interruption
- **THEN** it returns to the same path with tracked, untracked and task commits preserved and without reset or another allocation

#### Scenario: Old conversation targets a reused slot
- **WHEN** a previous task's continuation is requested after its slot has been released and reassigned
- **THEN** the harness refuses the stale binding before task execution and identifies the ownership conflict

### Requirement: Explicit release preserves work and caches

Release SHALL require the current task binding, no active owned operation and an explicit accepted-and-preserved or authorized-discard disposition. It SHALL verify reset to the selected committed base before making the slot free, retaining ignored build caches. Failure or uncertainty SHALL preserve the slot with its cause. A model reply, timeout, process exit or chat archive MUST NOT imply release.

#### Scenario: Accepted task becomes reusable
- **WHEN** the owned result is accepted and preserved, its processes have stopped and release succeeds
- **THEN** the same directory becomes available from the verified base with ignored build caches retained

#### Scenario: Clean tree still contains unreviewed commits
- **WHEN** a task's working tree is clean but its commits have not been accepted or explicitly discarded
- **THEN** the slot remains held and cannot be reset for another task

#### Scenario: Release fails partway through
- **WHEN** release cannot complete reset or verify the resulting checkout
- **THEN** the slot is not advertised as free, its surviving state and original error remain available and recovery does not create a replacement slot

#### Scenario: Archive is unrelated to release
- **WHEN** the task's chat is archived or removed from a conversation overview
- **THEN** its checkout and task ownership remain until an authorized release or retirement

### Requirement: Non-destructive capacity and state migration

Existing slot paths and unfinished task bindings SHALL survive migration to the common pool. Legacy capacity omission SHALL preserve the former executor-derived limit until installation materializes an explicit pool setting. Capacity reduction SHALL be rejected while owned slots outside the proposed range remain. Status and recovery SHALL remain available, and foreign worktrees MUST NOT be silently adopted or deleted.

#### Scenario: Existing installation lacks a pool setting
- **WHEN** an existing configuration has only its executor concurrency and compatibility `worktree_limit`
- **THEN** migration preserves its former effective capacity, writes that value as the explicit pool setting through the installation lifecycle and does not select the compatibility warning limit

#### Scenario: Requested smaller pool still has retained slots
- **WHEN** an operator requests capacity two while an owned slot three or four still exists
- **THEN** the smaller limit is rejected with the affected slots and required retirement actions, files remain intact, and status, release and authorized retirement remain usable

#### Scenario: Legacy mappings disagree
- **WHEN** migration discovers multiple existing mappings for the same Git repository or conflicting owners
- **THEN** it preserves their files and records, reports the conflict and refuses new allocation until reconciled without silently choosing an owner

### Requirement: One globally delivered isolation workflow

The installed kit SHALL deliver the common task lifecycle outside its source checkout. After that path is verified, installation SHALL retire the owned `isolated-worktree` skill and its disposable-tree recipe while preserving unrelated skills and settings. Current instructions and specifications SHALL direct managed isolation to the common pool and maintain one authoritative operational guide.

#### Scenario: Installed ordinary task and executor reuse
- **WHEN** the installed harness serves an authorized consumer outside its source checkout, including a separate checkout of this project with an unfinished development task
- **THEN** an ordinary task completes and releases a slot, an executor consumes that same pooled path through existing dispatch, and their actual checkout identities and preserved state satisfy the common contract

#### Scenario: Skill retirement is lifecycle-owned
- **WHEN** the updated kit is installed
- **THEN** the owned standalone skill is no longer advertised, its relevant safety guidance remains in the common workflow, and unrelated local skills and configuration are unchanged
