# Agent delegation

## Purpose

Increase completed, verified work from existing model subscriptions through reusable capability levels, economical delegation, useful parallelism and autonomous recovery, with evidence that includes coordination and rework.

## Requirements

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

Delegated work SHALL include a concise objective, relevant context, ownership, constraints and acceptance criteria. Independent edits SHALL use disjoint write scopes or isolated worktrees. Agents SHALL perform applicable checks and report changed files, evidence and unresolved issues concisely; detailed logs SHALL remain available on demand. The primary agent SHALL retain responsibility for acceptance and integration. The default collaboration SHALL limit concurrency to two spawned agents and prevent unsolicited recursive delegation. The kit SHALL distinguish configured concurrency/tool controls from advisory time or token budgets and SHALL NOT claim an unsupported hard reasoning-token cap.

#### Scenario: Parallel work is integrated
- **WHEN** two middle agents handle independent assignments
- **THEN** their execution overlaps, changes remain within their ownership, their evidence is returned, and the primary agent verifies the combined outcome

#### Scenario: A worker discovers an out-of-scope issue
- **WHEN** a worker observes unrelated diagnostics or a change outside its assignment
- **THEN** it preserves unrelated work and reports only material information without expanding its assignment or creating additional agents

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
