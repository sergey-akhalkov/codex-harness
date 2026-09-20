## MODIFIED Requirements

### Requirement: Ordinary Git worktrees, not Codex-managed checkouts

This workflow SHALL isolate work with ordinary Git worktrees created for the task. It MUST NOT use Codex CLI `--worktree`, TUI `/worktree`, or checkouts in the Codex-managed worktree pool (`$CODEX_HOME/worktrees` or a configured Desktop worktree root) as its isolation mechanism. An executor slot worktree owned by the orchestration harness pool is outside this capability. This workflow is not the allocator for executor isolation: a lead/executor assignment obtains its isolated checkout from the orchestration harness pool through the dispatch command, and this workflow MUST NOT create a substitute ordinary Git worktree for that session.

#### Scenario: A bounded independent edit needs file isolation
- **WHEN** an ordinary session needs a separate checkout and Codex-managed worktrees are available
- **THEN** the workflow still creates an ordinary Git worktree from explicit inputs rather than allocating a Codex-managed checkout

#### Scenario: An executor session needs isolation
- **WHEN** a lead/executor assignment requires an isolated checkout for a native Codex session
- **THEN** the checkout comes from the orchestration harness worktree pool through the dispatch command, and this workflow neither allocates it nor creates a substitute tree for that session
