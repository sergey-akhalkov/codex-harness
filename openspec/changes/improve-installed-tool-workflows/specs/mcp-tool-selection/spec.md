## ADDED Requirements

### Requirement: Outcome-oriented installed tool routes

The global workflow SHALL provide concise, discoverable routes for change-impact analysis, scoped architecture discovery, value/call tracing, semantic discovery, precise symbol operations, browser sequences and matching connected-resource reads. Each route SHALL identify its prerequisites, relevant result boundaries, validation and fallback. The workflow MUST NOT require all routes or servers for every task, and MUST retain native tools when they meet the task more effectively.

#### Scenario: Change-impact question
- **WHEN** a task asks which consumers a diff affects
- **THEN** the agent uses the available impact capability with a verified repository, diff identity, fresh index and relevant coverage, inspects material consumers and runs required behavior checks
- **AND** missing graph relationships are not reported as proof that other consumers do not exist

#### Scenario: Architecture and value-flow questions
- **WHEN** a task needs the structure of a bounded subsystem or the source of a value
- **THEN** the agent requests the relevant architecture or trace view and only necessary source evidence, with explicit uncertainty for unsupported or partially covered paths

#### Scenario: Semantic discovery is unavailable
- **WHEN** the current generation cannot answer a meaning-based search
- **THEN** the agent reports that limitation and uses a verified structural or literal route without claiming that no implementation exists

#### Scenario: Selected connected resource is missing
- **WHEN** a matching connector is offered but its repository access or intended document session cannot be established
- **THEN** the agent records the discovery result and uses an authorized fallback without claiming a successful resource operation or enabling an unrelated integration

### Requirement: Qualified instructions and operation boundaries

Delivered tool guidance SHALL preserve current user authority, source-based uncertainty and applicable project verification. An edit success or empty diagnostic response MUST NOT establish behavioral correctness by itself. Any selected read-only MCP path SHALL have verified effective operation boundaries; a mode name or advertised configuration alone MUST NOT establish enforcement. MCP-specific controls MUST NOT be represented as restrictions on unrelated native tools.

#### Scenario: Successful edit introduces a behavior defect
- **WHEN** a symbol edit reports success but its caller produces an incorrect result
- **THEN** the workflow detects the defect through the applicable independent check and keeps the change unaccepted until corrected

#### Scenario: Read-only mode still exposes a mutation
- **WHEN** actual discovery or an owned negative test shows that a selected read-only mode permits an out-of-scope write
- **THEN** the mode is not advertised as enforced read-only, its boundary is corrected or a verified alternative is selected, and subsequent negative acceptance attempts leave the target data unchanged

#### Scenario: Active session differs from new configuration
- **WHEN** a configuration change applies only to a newly started consumer
- **THEN** the workflow distinguishes that consumer from existing sessions and does not claim unsupported in-turn reconfiguration

### Requirement: Evidence-based optional capability selection

Graphify-based mixed-source context and Serena-based cross-project querying SHALL receive bounded applicability evaluations with an independent expected answer. Selection SHALL include preparation, freshness, process resources and maintenance cost. A candidate SHALL be adopted only with demonstrated task benefit and a supported preservation/lifecycle path; otherwise the existing route SHALL be retained with an evidence-backed reason. Evaluation MUST NOT silently add model billing, downloads, automatic indexing or a permanent service.

#### Scenario: Applicability evaluation was not completed
- **WHEN** a candidate has neither an evidence-backed adoption nor a justified retention decision
- **THEN** its evaluation remains incomplete and conditional adoption does not permit closing the task without that decision

#### Scenario: Graph has no proven relationship to the task
- **WHEN** a graph is absent, stale or belongs to another project
- **THEN** the workflow uses current relevant sources and records the limitation without substituting an unrelated default graph

#### Scenario: Graph construction benefit is claimed
- **WHEN** the workflow claims that a generated mixed-source graph improves task completion
- **THEN** evidence includes actual construction and update from the fixture sources, correct answers and their total cost; a hand-authored graph establishes query behavior only

#### Scenario: Cross-project service adds cost without benefit
- **WHEN** the candidate requires additional processes and fails the predeclared usefulness or resource criteria
- **THEN** the existing verified route remains selected and any candidate-owned resources are cleaned up without disturbing other clients

### Requirement: Browser batching with observable partial outcomes

Browser routes SHALL use scoped observations and batch a known sequence only up to a decision point. They SHALL preserve per-step outcomes and verify the intended final effect. A failed batch MUST NOT cause unexamined repetition of previously successful effects. Acceptance mutations SHALL use owned targets.

#### Scenario: Error follows a successful browser action
- **WHEN** a later step fails after an earlier action changed the page
- **THEN** the agent inspects the observed state, identifies completed and incomplete effects, and resumes only the necessary authorized work

### Requirement: Existing contract preservation and global consumption

The change SHALL reconcile affected guidance with existing memory, skill-evolution, verification and installation owners. It MUST NOT silently remove another change's requirements, close its unverified tasks or restore ordinary diagnostic, context or Stop hooks. Accepted routes SHALL be globally discoverable and demonstrated through native consumers outside the kit, including a named tool-capable child. Changed lifecycle behavior SHALL preserve unrelated settings, shared installations and project data through update, relocation, disconnect and rollback.

#### Scenario: Existing plan assumes retired hooks
- **WHEN** related planning artifacts still assume a hook that current user policy disables
- **THEN** the assumption and supported replacement or unresolved blocker are recorded in the owning plan, while the required outcome remains intact and the hook remains disabled

#### Scenario: External parent and child use the route
- **WHEN** a new native session starts in either of two independent outside repositories and delegates a suitable bounded task
- **THEN** native prompt-input evidence confirms the entry instructions in both roots, including one with local project instructions, and the parent and named child apply the relevant route with actual tool results and correct project identity

#### Scenario: Kit update or rollback
- **WHEN** the selected workflow is updated, relocated, disconnected or rolled back
- **THEN** kit-owned changes are handled through the existing lifecycle and project records, foreign edits, shared tools and unrelated service state remain intact
