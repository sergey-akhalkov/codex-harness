## MODIFIED Requirements

### Requirement: Bounded collaboration and verification

Delegated work SHALL include a concise objective, relevant context, ownership, constraints and acceptance criteria. Assignments SHALL identify the independently verifiable result, required inputs and dependencies, integration consumer, and completion or return condition. Independent edits SHALL use disjoint write scopes or isolated worktrees. Mutable runtime resources such as a desktop session, service, installed directory or test device SHALL have an explicit owner, isolated allocation or serialized use; separate checkouts SHALL NOT imply runtime isolation. Agents SHALL perform applicable checks and report changed files or other concrete results, evidence, validity conditions, restoration state and unresolved issues concisely; detailed logs SHALL remain available on demand within the owning project's retention policy. The primary agent SHALL retain responsibility for acceptance and integration, continue useful independent work when available, and avoid duplicating the delegated investigation. The default collaboration SHALL limit concurrency to two spawned agents and prevent unsolicited recursive delegation. The kit SHALL distinguish configured concurrency/tool controls from advisory time or token budgets and SHALL NOT claim an unsupported hard reasoning-token cap.

#### Scenario: Parallel work is integrated
- **WHEN** two middle agents handle independent assignments
- **THEN** their execution overlaps, changes remain within their ownership, their evidence is returned, and the primary agent verifies the combined outcome

#### Scenario: A worker discovers an out-of-scope issue
- **WHEN** a worker observes unrelated diagnostics or a change outside its assignment
- **THEN** it preserves unrelated work and reports only material information without expanding its assignment or creating additional agents

#### Scenario: Two workstreams need the same interactive application
- **WHEN** separate agents would otherwise manipulate the same desktop or application session
- **THEN** one agent owns its interaction, conflicting operations are serialized, and independent analysis or implementation proceeds only within separate resources and the existing aggregate resource limits

#### Scenario: An isolated investigation needs a parent dependency
- **WHEN** a worker can finish its own investigation but cannot yet exercise integration because a named prerequisite is unavailable
- **THEN** it returns the verified result, unmet prerequisite and validity conditions, and the parent preserves the pending integration rather than restarting the investigation or marking the entire task complete

## ADDED Requirements

### Requirement: Workstream completion includes consumption and recovery

The primary agent SHALL verify that a delegated or directly isolated supporting result is consumed by its intended parent path before treating its delivery as complete. An executable recipe SHALL identify its preconditions, checked effects and return or recovery behavior. A research-only result SHALL answer its bounded question with evidence and limits sufficient for the dependent decision. The parent SHALL distinguish exploratory findings, reproducible mechanisms and accepted product results, preserve useful partial work on interruption or reassignment, and finish required integration and restoration. Delegation SHALL remain an implementation choice rather than a mandatory consequence of decomposition.

#### Scenario: A discovery report still requires production integration
- **WHEN** a worker delivers observed interactions and an experimental script
- **THEN** the parent verifies and incorporates the needed mechanisms into the existing automation owner, exercises that owner from the actual parent entry point and preserves unfinished product checks until they pass

#### Scenario: The result is a bounded technical decision
- **WHEN** a worker was assigned to determine a dependency behavior rather than implement a feature
- **THEN** its evidence and limits resolve that decision, and the parent applies the finding without demanding an unrelated executable artifact

#### Scenario: Restoration remains uncertain after useful exploration
- **WHEN** an investigation produces useful findings but leaves an owned process or changed installation state unresolved
- **THEN** that uncertainty is reported and required recovery is completed before conflicting dependent work, while the findings remain available for independent use within their validity limits

#### Scenario: Decomposition is useful but delegation is not
- **WHEN** a smaller verification boundary simplifies the task but transferring context to a worker would add unnecessary cost
- **THEN** the primary agent executes that bounded subtask directly and still verifies its integration into the parent result
