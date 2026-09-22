## ADDED Requirements

### Requirement: Executor dispatch starts from a committed snapshot

The portable principles SHALL require the lead to fix a committed snapshot of the source checkout before dispatching an executor: assignment-relevant changes are committed locally (pushing remains a separate authorized step) and named as the assignment base, or committed HEAD is verified to contain every input and named instead. The principles SHALL state that copying files into a live executor checkout is not synchronization - changed tracked inputs travel as a new commit and a redispatch - and that slices depending on state the user forbade committing stay with the lead instead of being dispatched from a stale base. Executors SHALL verify their checkout is at the named base before substantive edits and report a mismatch instead of repairing it.

#### Scenario: The main worktree is committed before dispatch
- **WHEN** assignment inputs exist as uncommitted changes in the source checkout
- **THEN** the lead commits them locally, names that revision in the dispatch, and the executor slot starts exactly there without post-launch file copying

#### Scenario: The user forbids commits
- **WHEN** a slice depends on uncommitted state and commits are not authorized
- **THEN** the lead keeps the slice or asks for authorization rather than dispatching a stale base or hand-carrying files into the live checkout
