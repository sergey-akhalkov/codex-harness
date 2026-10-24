# director-orchestration Specification

## Purpose
Coordinate substantial outcomes through one Director and independently responsible Leads, including changes spanning repositories, with explicit acceptance, resource limits and recoverable execution.

## Requirements

### Requirement: Director chooses the delegation level

The Director SHALL be able to work directly or assign either a Lead or an Executor whenever that fits the authorized outcome. Assigning a Lead SHALL require a well-described OpenSpec change under the specification-backed admission contract. The existence of a change SHALL NOT oblige the Director to delegate it, and a direct Executor assignment SHALL NOT require an intermediate Lead.

#### Scenario: Mixed delegation
- **WHEN** the Director assigns a prepared OpenSpec change to a Lead and a separate bounded task directly to an Executor
- **THEN** both run under the Director with their respective roles, and the direct Executor task requires no artificial specification or intermediate Lead

#### Scenario: Director retains a change
- **WHEN** the Director judges that directly implementing a change or using direct Executors is more appropriate
- **THEN** it can choose that route without a compulsory Lead or a fixed task-size threshold

### Requirement: Specification-backed Lead admission

A Lead assignment SHALL reference a concrete OpenSpec change and its committed planning inputs, with scope, requirements, testable acceptance conditions, implementation tasks and design when required by its workflow. The Director SHALL verify that these describe the intended outcome. Dispatch SHALL reject missing or incomplete required artifacts before allocation or model work; a title or beads card alone SHALL NOT substitute for the specification.

#### Scenario: Prepared change is assigned
- **WHEN** the selected change has complete applicable planning artifacts and the Director has established its outcome and acceptance
- **THEN** the Lead receives the exact change reference, artifact revisions, code repositories and board assignment, and owns implementation, executor coordination and integrated checks for that change

#### Scenario: A vague task is offered to a Lead
- **WHEN** Lead dispatch lacks a change reference, a required artifact or concrete requirements and acceptance
- **THEN** it reports the missing input without starting a Lead, and the Director can prepare the spec or choose a direct execution route within the existing authorization

#### Scenario: Specification changes during work
- **WHEN** requirements change after the Lead starts
- **THEN** the Director explicitly supplies the revised committed inputs and reconciles affected assignments and acceptance; an unannounced file change does not silently redefine the running scope

### Requirement: Independent direct-child limits

`orchestration.toml` SHALL configure `director_max_leads`, `director_max_executors` and `lead_max_executors` independently, defaulting to 2, 4 and 6. Admission SHALL count managed children by their immediate parent and role, including startup reservations, active runs and reply-waiting runs. A stopped or terminal run SHALL release concurrency only after its effects are reconciled; its worktree remains held.

#### Scenario: Director reaches each direct limit
- **WHEN** a Director has two admitted Leads and four admitted direct Executors under the default settings
- **THEN** another Lead or direct Executor is refused with the reached limit, occupying runs and next action before allocation or model work
- **AND** a Lead's Executors do not consume the Director's four direct-Executor positions

#### Scenario: Each Lead receives its own allowance
- **WHEN** two Leads under one Director each request six independent Executors and shared capacity is available
- **THEN** each can admit six Executors, and a seventh for either Lead is refused without reducing the other Lead's allowance

#### Scenario: Limits are configured differently
- **WHEN** the three parent limits are changed to valid supported values
- **THEN** new admission uses those values without hardcoded 2, 4 or 6 limits
- **AND** lowering a limit preserves existing work and blocks excess new starts instead of stopping or discarding admitted runs

#### Scenario: Concurrent or repeated admission
- **WHEN** requests from the same parent race for its final position, or a start response is lost
- **THEN** admission reserves the position atomically, reconciles retries against the original request and never exceeds the parent's allowance

### Requirement: Shared capacity remains explicit

Parent limits SHALL compose with the installation-wide executor limit, per-repository pool capacity and existing machine resource limits. Shipped defaults SHALL support two Leads and sixteen Executors in one repository: an executor limit of 16 and pool capacity of 18. Configured lower shared limits SHALL remain usable and be reported as effective constraints. Increasing a limit SHALL NOT eagerly create worktrees or authorize work.

#### Scenario: Full default hierarchy
- **WHEN** one Director starts two Leads with six Executors each and four direct Executors in one repository with an otherwise free default pool
- **THEN** admission supports eighteen child sessions and worktrees without an inherited six-Executor ceiling
- **AND** existing heavy-command limits still control simultaneous builds and tests

#### Scenario: Shared capacity is deliberately smaller
- **WHEN** local configuration preserves a smaller executor limit or pool than the parent limits permit
- **THEN** check and status identify the effective bottleneck and occupying work; dispatch reports that cause instead of claiming a role prohibition or allocating an extra worktree

#### Scenario: Repository or parent changes do not bypass shared limits
- **WHEN** several Leads or Directors dispatch across different repositories
- **THEN** the shared executor allowance is enforced across the installation and each repository's pool is counted once across its worktrees

### Requirement: Repository-scoped workspaces

A Lead assignment SHALL support one or several explicitly selected repositories, with an owned integration worktree in each. Each Executor SHALL receive a committed base and owned worktree for every repository its assignment needs. Repository membership SHALL not grant access to unrelated projects. Invalid inputs, unavailable repositories and partial preparation SHALL preserve existing work and report exact effects before model execution.

#### Scenario: One change crosses repositories
- **WHEN** a Lead owns a producer-and-consumer change in two selected repositories
- **THEN** it can dispatch implementation in both, inspect and integrate their outputs, and exercise the combined user path from its identified workspace

#### Scenario: Two Leads use the same repository
- **WHEN** independent Leads implement different blocks in one repository
- **THEN** their integration and Executor worktrees have separate owners in the common pool, with no writes into another assignment's checkout

#### Scenario: Preparation fails in a second repository
- **WHEN** a multi-repository dispatch cannot prepare one required workspace
- **THEN** no model work starts, the result identifies completed and failed preparation, and unused new reservations are released only when safe
- **AND** pre-existing work and any uncertain effects remain preserved and observable

### Requirement: Hierarchy worktree labels

Hierarchy worktrees SHALL use fixed role and ordinal labels in every selected repository pool: `de1..de4` for direct Director Executors, `l1..l2` for Leads, and `l1e1..l1e6` and `l2e1..l2e6` for Lead Executors. The same label SHALL be used in every repository selected by one assignment.

#### Scenario: Direct Executor and Lead allocations
- **WHEN** the Director dispatches a direct Executor, a Lead, and an Executor under that Lead
- **THEN** their worktrees are named with `deN`, `lN`, and `lNeM` respectively for the same ordinal in every selected repository

### Requirement: Legacy worktree labels

Legacy `{repo}-wtN` worktrees SHALL remain readable and SHALL NOT be renamed while held. A released position MAY migrate to its hierarchy label only when no owner or partial work can be lost. Compatibility CLI and ordinary tasks MAY keep legacy labels until migrated.

#### Scenario: Legacy slot reuse
- **WHEN** a held `{repo}-wtN` position is encountered during allocation
- **THEN** it remains readable and is not moved; after safe release and reuse the position takes its hierarchy label without losing retained work

### Requirement: Committed parent inputs determine child work

Child assignments SHALL resolve their required inputs to exact commits in the parent's declared repositories before dispatch. Children SHALL start from those snapshots and verify them. Integration targets and changed input revisions SHALL remain explicit. Uncommitted inputs SHALL not be copied into a running child checkout or silently replaced by an unrelated default branch.

#### Scenario: Lead prepares a shared contract
- **WHEN** a Lead commits the contract needed by its Executors
- **THEN** each dependent Executor starts from the named commit containing that contract, and its result reports the base actually used

#### Scenario: Required inputs change during work
- **WHEN** the parent changes an input after an Executor has started
- **THEN** the running assignment retains its recorded base and partial work until an explicit integration or recovery decision establishes the next committed input

### Requirement: Dependencies are separate from supervision

The Director SHALL coordinate dependencies and overlapping ownership between blocks independently of the parent-child tree. Leads SHALL coordinate dependencies within their own blocks. Shared contracts and runtime resources SHALL have an identified owner or serialized access. A satisfied Git merge SHALL not establish behavioral compatibility.

#### Scenario: Dependent blocks run selectively
- **WHEN** block B depends on a contract delivered by block A
- **THEN** the dependency is recorded in the owning board, independent work can proceed, and dependent implementation or acceptance waits for the required verified contract

#### Scenario: Separate checkouts share a service
- **WHEN** assignments need the same mutable installation, service, port or device
- **THEN** ownership or serialized access is established before conflicting use; worktree separation is not treated as runtime isolation

### Requirement: Acceptance follows the complete outcome

A Lead SHALL accept and integrate its Executors' work against its assigned block. The Director SHALL accept Lead outputs and direct Executor outputs against the overall outcome and their combined behavior. Multi-repository acceptance SHALL identify the tested revision set and any partial integration. Completion, result receipt, board closure and worktree release SHALL remain distinct.

#### Scenario: Individually passing changes conflict
- **WHEN** two accepted Lead outputs pass their local checks but fail the combined user path
- **THEN** overall acceptance remains pending, the Director assigns the concrete correction to its owner, and useful results and evidence are retained

#### Scenario: Cross-repository integration is incomplete
- **WHEN** one repository has received an accepted change but integration or checks in another fail
- **THEN** the outcome reports the applied and pending revisions and recovery action, without claiming atomic integration or complete delivery

### Requirement: Explicit hierarchy restoration

Closing the controlling CLI SHALL preserve the hierarchy, assignments, workspaces, questions and unaccepted results for explicit resumption. The workflow SHALL not depend on an unattended background service. Restoration SHALL verify current authority, route bindings, surviving processes and interrupted effects before continuing, and SHALL not duplicate dispatch or discard partial work.

#### Scenario: Close and resume
- **WHEN** the controlling CLI closes with one pending question, one completed unaccepted result and one partially edited worktree
- **THEN** these remain recoverable, no new dependent model work is dispatched after loss of control, and reopening alone does not resume the hierarchy
- **AND** an explicit resume reconciles surviving work and restores those items to their owners

#### Scenario: A resumed parent has surviving children
- **WHEN** explicit restoration finds an existing child thread or an uncertain launch outcome
- **THEN** it reconciles that recorded operation before replacement and preserves the selected profile, descendant identities and current checks
- **AND** inability to establish ownership is reported without guessing a replacement parent

### Requirement: Subtree stop preserves unrelated work

Stopping a Lead SHALL stop new dispatch and owned active work throughout its subtree while retaining every affected workspace and result. Stopping a direct Executor SHALL affect only that run. Partial interruption SHALL report surviving owned work. Stop, restart, navigation and archive SHALL not imply acceptance, release or automatic resumption.

#### Scenario: One team is stopped
- **WHEN** the Director stops one Lead while another Lead and a direct Executor are active
- **THEN** the selected Lead and its Executors are interrupted through verified ownership, pending delivery is reconciled, and the other branches remain usable

#### Scenario: Stop races a child launch
- **WHEN** a Lead is stopped while its child is being prepared or started
- **THEN** the launch is reconciled into the stopped subtree, no unmanaged child is left running, and uncertain or partial stop effects are reported with a recovery action
