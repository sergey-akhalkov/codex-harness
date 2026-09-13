# Agent delegation

## Purpose

Increase completed, verified work from existing model subscriptions through reusable capability levels, economical delegation, useful parallelism and autonomous recovery, with evidence that includes coordination and rework.

## Requirements

This baseline records the earlier named-level contract. The accepted transition
to direct model/effort selection, GPT leadership, Z.AI text/code execution and
Grok visual/routine execution is specified in the active
[orchestration delta](../../changes/orchestrate-subscription-agents/specs/agent-delegation/spec.md).
Use the [operating guide](../../../docs/agent-delegation.md) for currently
supported entry points and limitations. Keeping the baseline here does not
override that transition or establish that its pending acceptance has passed;
its [tasks](../../changes/orchestrate-subscription-agents/tasks.md) remain open
until verified.

### Requirement: Universal capability levels

The kit SHALL expose middle, senior and principal capability levels globally, with explicit replaceable model and reasoning assignments. Preferred middle SHALL use the authenticated Grok 4.6 subscription at xhigh; middle reserve SHALL use GPT-6 Astra high on the existing ChatGPT subscription. Senior SHALL use GPT-6 Astra xhigh and principal SHALL use GPT-6 Astra max. All OpenAI model assignments, including reserves and auxiliary calls, SHALL use only the Astra family; GPT-5 models SHALL NOT be selected. Levels SHALL support tasks selected by the assignment and relevant available skills rather than a permanent coding/review profession. `grok_reviewer` SHALL be retired with documented migration.

#### Scenario: One level performs different activities
- **WHEN** the middle level receives a bounded implementation task and, separately, a bounded review task
- **THEN** it executes the requested activity and returns appropriate verification evidence without requiring a different profession-specific agent

#### Scenario: Native model selection is verified
- **WHEN** each supplied level is invoked through a new global Codex session outside the kit repository
- **THEN** native metadata identifies its assigned model and reasoning, and the assigned subscription serves an actual response

### Requirement: Economical delegation and Grok priority

The primary agent SHALL actively seek worthwhile bounded work to delegate and SHALL prefer available Grok middle over GPT middle for suitable delegation. It SHALL consider ambiguity, error consequences, verification cost, context transfer and expected rework. It SHALL perform work directly when delegation overhead outweighs the benefit or senior judgment is required. Completion and correctness SHALL take precedence over minimizing ChatGPT consumption; delegation count SHALL NOT be a success criterion.

#### Scenario: Independent routine work is available
- **WHEN** a task contains independent, sufficiently specified work that benefits from delegation and Grok is available
- **THEN** the primary agent selects Grok middle and continues useful independent work without duplicating the delegated investigation

#### Scenario: Delegation would add overhead
- **WHEN** a task is trivial, tightly coupled or requires senior judgment such that delegation is not worthwhile
- **THEN** the primary agent completes it directly without a mandatory decomposition or agent round trip

### Requirement: Autonomous recovery and escalation

The primary agent SHALL distinguish model unavailability from insufficient reasoning capability. Observed unavailable models, authentication failures and quota exhaustion SHALL permit autonomous use of GPT middle reserve, with the cause made visible and partial work preserved. A genuine difficult reasoning blocker SHALL permit a bounded principal consultation without additional user approval within the authorized task. Principal SHALL NOT be a routine reviewer, a mandatory pipeline stage or a remedy for missing credentials or unavailable infrastructure. Repeated attempts SHALL depend on evidence of progress, not a required number of failures. Requests SHALL retain explicit provider/model identity; switching SHALL be a visible orchestration decision, never a hidden transport fallback or paid API substitution.

#### Scenario: Grok cannot serve work
- **WHEN** Grok middle is absent or fails because its model, authentication or quota is unavailable
- **THEN** the primary agent briefly records the reason and continues suitable unfinished work using GPT middle reserve without asking the user to approve the already authorized work

#### Scenario: A hard reasoning blocker remains
- **WHEN** evidence supports a materially difficult reasoning problem beyond the current approach
- **THEN** the primary agent can consult principal with the problem, minimal reproduction, verified facts, attempted hypotheses and a specific desired result, then validate and integrate the answer

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

### Requirement: Explicit resource use

The preferred middle path SHALL avoid implicit OpenAI search/vision helper calls. Required unsupported capabilities SHALL produce a visible limitation or an explicit assignment to an appropriate level. The kit SHALL document how to inspect available usage information for both subscriptions and distinguish observed quota values, token accounting, missing telemetry and cost estimates. No new paid API credentials, top-ups or account changes SHALL be introduced.

#### Scenario: Grok needs an auxiliary capability
- **WHEN** a Grok task requires search or image processing
- **THEN** the configured route does not silently invoke an OpenAI helper; the task uses an available explicit capability or reports its limitation for reassignment

### Requirement: Evidence of efficiency and quality

Acceptance SHALL exercise the global policy on representative implementation, review, parallel, fallback, escalation and direct-work scenarios. A reproducible comparison with direct Astra execution SHALL record total elapsed time, parent and descendant token usage by provider, verification outcomes and rework for matched accepted tasks. Repeated cumulative usage records SHALL NOT be double counted. Missing telemetry or concurrent account activity SHALL be disclosed. Claims of improvement SHALL be limited to measured scenarios; token differences SHALL NOT be presented as exact weekly quota savings. Findings that show overhead or quality failures SHALL drive correction before completion.

#### Scenario: A comparative run finishes
- **WHEN** matched direct and delegated tasks have completed their acceptance checks
- **THEN** the report includes coordination and worker usage, both providers, elapsed time and quality evidence, and states whether each measured outcome improved or regressed

### Requirement: Global lifecycle and recoverability

Sources and setup SHALL remain in the kit and activate through its linked installation lifecycle for new Codex sessions in other projects. Grok availability SHALL follow the subscription integration; native GPT levels SHALL remain discoverable when that integration is disconnected. Existing credentials, unrelated work, MCP integrations and the running control channel SHALL be preserved. Updates and disconnection SHALL be verifiable without stopping the proxy serving the current session.

#### Scenario: A new external session starts
- **WHEN** the kit is installed and ordinary Codex starts outside this repository
- **THEN** the global delegation policy, available levels and relevant skills are discoverable without copying reusable sources into the target project

#### Scenario: Subscription integration is disconnected
- **WHEN** its lifecycle is exercised in an isolated installation
- **THEN** the Grok level disappears, native GPT levels remain available, and no global service or credentials are disrupted by that test


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
