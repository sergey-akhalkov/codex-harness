## MODIFIED Requirements

### Requirement: Bounded collaboration and verification

Each workstream SHALL receive a concise objective, sufficient inputs, dependencies, ownership of files and mutable runtime resources, constraints, acceptance checks, an integration consumer, a completion or return condition, and the synchronized base revision of its checkout. Independent edits SHALL use disjoint scopes or Codex-managed worktrees recorded by the controller; ordinary Git worktrees from `isolated-worktree-workflow` SHALL NOT substitute for executor isolation. Shared desktop sessions, services, installed directories and devices SHALL have one interaction owner, isolated allocation or serialized use; a separate checkout SHALL NOT imply runtime isolation. An executor SHALL verify that its checkout HEAD equals the base named in its brief before substantive edits and SHALL stop and report a mismatch instead of repairing synchronization, copying files from another checkout or creating a substitute worktree; changed tracked inputs reach an assignment through a new committed base and a redispatch. Executors SHALL report changed files or other concrete results, relevant decisions, checks, validity conditions, restoration state and unresolved issues concisely, with details available on demand under the owning retention policy. The active lead SHALL verify important risks and the combined result without routinely repeating completed investigation or every worker check. A supporting result SHALL count as delivered only after its intended consumer uses it and applicable integrated acceptance passes. The task-wide concurrency limit SHALL come from the orchestration configuration; leadership changes SHALL NOT multiply that limit. Workers SHALL NOT create unsolicited recursive agent trees. Configured controls SHALL be distinguished from advisory time/token budgets; unsupported hard reasoning-token caps SHALL NOT be claimed.

#### Scenario: Parallel work is integrated
- **WHEN** two executors handle independent workstreams
- **THEN** execution overlaps without conflicting resource ownership, evidence is returned, and the active lead verifies the consumed combined outcome

#### Scenario: An executor starts from the named base
- **WHEN** an executor receives a brief that names the synchronized base revision of its pool slot
- **THEN** it verifies its checkout HEAD equals that revision before substantive edits, and on mismatch returns the exact observed and expected revisions to the lead instead of editing

#### Scenario: A worker discovers an out-of-scope issue
- **WHEN** an executor observes unrelated diagnostics or a change outside its assignment
- **THEN** it preserves unrelated work and reports material information without expanding scope or creating additional agents

#### Scenario: Two workstreams need the same interactive application
- **WHEN** separate agents would otherwise manipulate the same desktop or application session
- **THEN** one agent owns its interaction, conflicting operations are serialized, and independent work uses separate resources within existing aggregate limits

#### Scenario: An isolated investigation needs a parent dependency
- **WHEN** an executor finishes its investigation but cannot exercise integration because a named prerequisite is unavailable
- **THEN** it returns the verified result, unmet prerequisite and validity conditions, and the lead preserves pending integration without restarting the investigation or claiming full completion
