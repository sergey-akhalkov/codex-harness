## RENAMED Requirements

- FROM: `### Requirement: Economical delegation and Grok priority`
- TO: `### Requirement: Economical delegation through configured roles`

## MODIFIED Requirements

### Requirement: Universal capability levels

The kit SHALL support direct native selection of an explicit provider profile, model and compatible reasoning effort per assignment without requiring separate named agent definitions. Lead and executor responsibilities SHALL come from the kit orchestration configuration rather than fixed provider roles, and effective identities SHALL be observable in each conversation view. Only efforts supported by the selected model SHALL be accepted; unsupported combinations SHALL NOT be silently substituted. Redundant supplied Grok middle and Astra middle reserve, senior and principal presets SHALL remain retired with the documented migration to model/effort arguments, preserving user-owned agents, skills and native GPT availability. All OpenAI assignments, including auxiliary calls and reserves, SHALL use only the Astra family, with GPT-5 excluded. Responsibilities and relevant skills SHALL select activities without profession-specific agents. The retired `grok_reviewer` SHALL remain retired.

#### Scenario: One level performs different activities
- **WHEN** a configured executor receives an implementation assignment and separately a review or research assignment within its capabilities
- **THEN** it performs each activity with applicable skills and evidence without requiring a separate profession-specific agent

#### Scenario: Native model selection is verified
- **WHEN** an executor is launched with its configured profile and a supported effort from a fresh global Codex session outside the kit repository
- **THEN** native metadata identifies its effective model and reasoning, and the profile's subscription serves an actual response

#### Scenario: A requested effort is unsupported
- **WHEN** an assignment requests an effort absent from the selected model's verified contract
- **THEN** dispatch reports the unsupported combination before a model request, without silently choosing another model or effort

#### Scenario: Redundant presets are removed
- **WHEN** the kit updates an installation that used supplied fixed agent profiles
- **THEN** its own obsolete definitions and references are retired, direct model/effort selection remains usable, and unrelated user agents and `.agents/skills` remain intact

### Requirement: Economical delegation through configured roles

The lead SHALL assign worthwhile complete workstreams to configured executors according to reasoning needs, modality, tools, ambiguity, error consequences and verification cost. It SHALL retain requirements, consequential decisions, overall acceptance and merge responsibility, while executors own investigation, implementation, applicable checks and correction within their assignments. The lead SHALL consider handoff, coordination, waiting, integration and rework, avoid solving delegated work in parallel, and perform a small or tightly coupled task directly when delegation would cost more. Completion and correctness SHALL take precedence over quota minimization; delegation count SHALL NOT be a success criterion.

#### Scenario: Independent routine work is available
- **WHEN** a task contains sufficiently specified independent work that benefits from delegation and a configured executor is available
- **THEN** the lead assigns the complete outcome to that executor in its own worktree and continues only non-overlapping useful work or waits without duplicating the investigation

#### Scenario: Delegation would add overhead
- **WHEN** a trivial or tightly coupled task costs more to brief and verify than to complete directly
- **THEN** the lead completes it directly without a mandatory agent round trip

### Requirement: Autonomous recovery and escalation

The lead and task runtime SHALL distinguish unavailable access from insufficient reasoning capability and incomplete output. A confirmed model, authentication or quota failure SHALL permit visible reassignment to a capable configured executor within the existing authorization, preserving partial work; an unavailable profile SHALL NOT force unsuitable work onto another. Lead succession SHALL be governed by `lead-agent-orchestration`. A genuine hard reasoning blocker SHALL permit a bounded Astra principal consultation, with verified facts, attempted hypotheses and a precise question. Principal SHALL NOT be a routine review stage or an infrastructure/authentication remedy. Retries SHALL depend on their cause and evidence of progress. Provider/model identity SHALL remain explicit; no hidden transport fallback or paid substitution is permitted.

#### Scenario: Grok cannot serve work
- **WHEN** Grok's assigned model, authentication or quota is unavailable
- **THEN** suitable work continues on another capable configured executor, work requiring image input uses an available capable route, and any use of GPT reserve is explicitly attributed to the same GPT subscription as the lead

#### Scenario: A hard reasoning blocker remains
- **WHEN** evidence supports a materially difficult reasoning problem beyond the current executor's approach
- **THEN** the lead can consult principal on that bounded question and validate the answer while leaving the rest of the assignment with its executor

### Requirement: Bounded collaboration and verification

Each workstream SHALL receive a concise objective, sufficient inputs, dependencies, ownership of files and mutable runtime resources, constraints, acceptance checks, an integration consumer and a completion or return condition. Independent edits SHALL use disjoint scopes or Codex-managed worktrees recorded by the controller; ordinary Git worktrees from `isolated-worktree-workflow` SHALL NOT substitute for executor isolation. Shared desktop sessions, services, installed directories and devices SHALL have one interaction owner, isolated allocation or serialized use; a separate checkout SHALL NOT imply runtime isolation. Executors SHALL report changed files or other concrete results, relevant decisions, checks, validity conditions, restoration state and unresolved issues concisely, with details available on demand under the owning retention policy. The active lead SHALL verify important risks and the combined result without routinely repeating completed investigation or every worker check. A supporting result SHALL count as delivered only after its intended consumer uses it and applicable integrated acceptance passes. The task-wide concurrency limit SHALL come from the orchestration configuration; leadership changes SHALL NOT multiply that limit. Workers SHALL NOT create unsolicited recursive agent trees. Configured controls SHALL be distinguished from advisory time/token budgets; unsupported hard reasoning-token caps SHALL NOT be claimed.

#### Scenario: Parallel work is integrated
- **WHEN** two executors handle independent workstreams
- **THEN** execution overlaps without conflicting resource ownership, evidence is returned, and the active lead verifies the consumed combined outcome

#### Scenario: A worker discovers an out-of-scope issue
- **WHEN** an executor observes unrelated diagnostics or a change outside its assignment
- **THEN** it preserves unrelated work and reports material information without expanding scope or creating additional agents

#### Scenario: Two workstreams need the same interactive application
- **WHEN** separate agents would otherwise manipulate the same desktop or application session
- **THEN** one agent owns its interaction, conflicting operations are serialized, and independent work uses separate resources within existing aggregate limits

#### Scenario: An isolated investigation needs a parent dependency
- **WHEN** an executor finishes its investigation but cannot exercise integration because a named prerequisite is unavailable
- **THEN** it returns the verified result, unmet prerequisite and validity conditions, and the lead preserves pending integration without restarting the investigation or claiming full completion

### Requirement: Explicit resource use

Every role SHALL avoid implicit use of another subscription for search, vision or other auxiliary work. A capability unavailable on the selected route SHALL require an explicit capable assignment or an observable blocked dependency. The kit SHALL document available usage information for connected subscription profiles and distinguish observed account limits, task token accounting, missing telemetry and estimates. Native GPT descendants SHALL be attributed to the same ChatGPT subscription as their parent. No new paid API credentials, top-ups or account changes SHALL be introduced.

#### Scenario: An executor needs an auxiliary capability
- **WHEN** a selected route lacks image processing or search that an assignment needs
- **THEN** the missing capability is handled by an explicit available tool or assignment with visible subscription identity, without hidden GPT helper use

#### Scenario: Grok needs an auxiliary capability
- **WHEN** a Grok-profile task requires search or image processing
- **THEN** its route uses an available explicit capability or reports a limitation for reassignment without silently invoking an OpenAI helper

### Requirement: Global lifecycle and recoverability

Sources and setup SHALL activate through the existing linked global lifecycle for new Codex sessions in other projects. Available external models SHALL follow their subscription profile connections; direct native GPT assignment SHALL remain usable when external integration is disconnected. Dispatch SHALL NOT require redundant supplied agent presets. Existing credentials, unrelated user agents and skills, work, MCP integrations and active control channels SHALL be preserved. Updates and disconnection SHALL be verifiable in owned state without stopping a proxy serving other sessions.

#### Scenario: A new external session starts
- **WHEN** the kit is installed and ordinary Codex starts outside this repository
- **THEN** the delegation policy, the `team-lead` skill, orchestration role configuration, board availability, explicit model/effort selection, simultaneous conversation views and task recovery are usable without copying sources or repeatedly performing manual setup

#### Scenario: Subscription integration is disconnected
- **WHEN** its lifecycle is exercised in an isolated installation
- **THEN** removed routes are unavailable for new assignments, native GPT remains usable, recoverable task state is retained, and unrelated services and credentials are preserved

## ADDED Requirements

### Requirement: Patient ownership of delegated outcomes

Elapsed time, an expired observation wait, an intermediate answer or missing final prose SHALL NOT alone authorize takeover, duplicate execution, cancellation or reassignment. The lead SHALL wait without repeated model status calls when no independent useful work remains. Steering SHALL add relevant facts, resolve a request or correct an established mistake. Incomplete output SHALL first be reconciled with visible work and the owning result-delivery mechanism. A stopped or failed attempt SHALL be recovered or reassigned with its partial result and a concrete cause; it SHALL NOT silently become work for GPT. A verified reasoning limitation can justify bounded assistance, and user stop or redirection SHALL still take effect.

#### Scenario: A correct worker takes longer than an observation interval
- **WHEN** an executor remains active beyond several observation waits without a demonstrated failure
- **THEN** its ownership is preserved, no competing solution starts, and waiting generates no repeated model status requests

#### Scenario: A child returns only a progress message
- **WHEN** a child ends with an intermediate response and partial artifacts but no provider rejection
- **THEN** recovery examines its visible events and artifacts, retains completed work, and addresses continuation or delivery without treating the output as quota exhaustion

#### Scenario: Review finds a defect in the assigned work
- **WHEN** a concrete defect can be corrected within the executor's capabilities and scope
- **THEN** the lead returns the finding and acceptance condition to that executor rather than routinely rewriting its work
