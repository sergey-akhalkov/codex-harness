## MODIFIED Requirements

### Requirement: Bounded maintenance in the owning checkout

The agent SHALL record durable decisions during the task and verified reusable
findings when established. A confirmed decision, priority, agreement or piece
of deferred work whose effect can matter in a later session - scope, delivery
order, constraints, preferences or postponed improvements - SHALL be recorded
in its existing owning record when it is decided, before the work moves on; an
explicit user request to remember or roadmap an item SHALL cause an immediate
write to the right owner. This trigger creates no unconditional extra model
call, end-of-turn write, transcript or progress entry: transient observations
and facts no future session needs stay out of memory. The agent SHALL preserve
unrelated modifications, check current content before updating, and write only
in the owning checkout. Maintenance SHALL replace superseded facts in their
owning record, consolidate duplicated material, and remove transient or
obsolete reports after necessary current facts and references are retained.
Memory updates SHALL remain ordinary reviewable Git changes; automatic commit,
push or cross-worktree propagation MUST NOT be implied.

#### Scenario: A deferred improvement is agreed
- **WHEN** the user confirms that an improvement is postponed until after a release milestone
- **THEN** the workflow records the deferred item and its trigger in the owning record during the discussion, so a later session can retrieve it through the memory entry point

#### Scenario: The user asks to remember a decision
- **WHEN** the user explicitly asks that an agreement or priority be kept for future sessions
- **THEN** the workflow writes it to the existing owning record immediately instead of deferring the write to an unspecified later pass

#### Scenario: Concurrent worktrees update memory
- **WHEN** two tasks independently edit the same memory topic in separate worktrees
- **THEN** each task preserves its local evidence and Git exposes the integration conflict; the integrating agent resolves it from the current decisions and evidence instead of silently overwriting either task

#### Scenario: No durable knowledge was learned
- **WHEN** a task ends with only transient state, progress facts or knowledge already recorded in memory
- **THEN** the workflow finishes without adding a redundant memory entry
