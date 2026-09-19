# Subscription Efficiency Specification

## Purpose

Reduce avoidable subscription use while preserving complete accepted outcomes, correctness and fast delivery, using measured evidence rather than tool or delegation counts.

## Requirements

The active [routing and efficiency delta](../../changes/orchestrate-subscription-agents/specs/subscription-efficiency/spec.md)
owns the accepted transition from the earlier Grok-middle preference below.
Current selection and migration status follow the
[delegation contract](../agent-delegation/spec.md#requirements); this baseline
does not certify the active change's unfinished benefit or orchestration checks.

### Requirement: Attributable usage evidence

The kit SHALL provide a bounded report from existing local evidence that separates model/provider, parent and child work, cached and uncached input, output including reasoning, hook context, repeated delivery, continuation and elapsed time. Model responses SHALL be deduplicated by stable response identity; cumulative totals MUST NOT be added as independent usage. Missing records, cross-session concurrency and incomparable quota-reset windows SHALL be explicit. Raw tokens or character counts MUST NOT be presented as exact weekly quota percentages. Source transcripts and credentials SHALL remain outside tracked reports.

#### Scenario: Repeated and incomplete usage records
- **WHEN** the same response appears in multiple logs and one child has incomplete usage
- **THEN** it is counted once and the incomplete child's contribution remains explicitly partial

#### Scenario: A weekly allowance changes during parallel work
- **WHEN** quota snapshots span different reset windows or concurrent projects
- **THEN** the report does not attribute their difference solely to the inspected task

### Requirement: Development without hooks is the durable default

Ordinary development SHALL use suitable explicit tools and project-native checks
without lifecycle hooks by default, including after this change is completed.
The kit SHALL NOT routinely reinject instructions, repeatedly analyze unrelated
files, or use Stop to request further checking or automatic continuation.

A hook exception SHALL require a concrete important unmet need, evaluation of a
simpler native mechanism, narrowly defined events and inputs, and the existing
benefit acceptance. It SHALL be deterministic and bounded, without a model call
to dispatch or interpret routine events. Successful checks SHALL add no model
context; necessary blocking/failure feedback SHALL contain only an actionable
reason and essential uncertainty. Resource cleanup or local user notifications
SHALL NOT inject model context or continue an agent turn. Silent output alone
SHALL NOT establish efficiency: execution latency, resource use and maintenance
remain part of the assessment.

Examples such as a precise pre-operation constraint, owned-resource cleanup and
a local completion notification SHALL NOT create implementation requirements or
implicitly authorize activation. This change SHALL select no hook exception of
its own. A separately authorized narrow exception SHALL retain its owning
acceptance and SHALL NOT restore diagnostic, context or Stop hooks. The change
MAY complete with every hook disabled and no replacement hook system.

#### Scenario: Existing tools cover ordinary development
- **WHEN** Serena and applicable native project checks satisfy the consuming task
- **THEN** the workflow completes with hooks disabled and no substitute hook infrastructure

#### Scenario: A silent hook still slows work
- **WHEN** a candidate emits no model context but adds unjustified startup, waiting or maintenance cost
- **THEN** silence is not treated as benefit evidence and the candidate remains disabled

#### Scenario: A specific hook exception is justified
- **WHEN** a concrete need lacks an adequate simpler native mechanism and passes the benefit gate
- **THEN** only its narrow deterministic scope is selected, with bounded execution, silent success and no routine model invocation or completion loop

#### Scenario: A useful example has no current consumer
- **WHEN** planning lists a guard, cleanup or notification example without an actual unmet need
- **THEN** implementation neither builds nor enables it merely to close this change

### Requirement: Capability restoration requires demonstrated benefit

All hooks and automatic LSP SHALL remain disabled during normal work until a bounded comparison demonstrates their benefit for task completion, defect prevention and total time to a verified result, including setup, waiting, review and rework. Existing explicitly invoked LSP tools may remain callable during evaluation, but their final retention SHALL require the same benefit assessment. Comparisons SHALL hold task inputs and acceptance checks constant and include subscription usage. A capability SHALL NOT be restored based only on installed status, passing its own tests, speed of an individual call or historical investment. Inconclusive, harmful or redundant capabilities SHALL remain disabled or be retired; retiring all LSP integrations SHALL be an acceptable outcome. Isolated owned evaluation targets SHALL be the only place candidate hooks are temporarily enabled before this decision.

#### Scenario: A hook passes tests but slows accepted work
- **WHEN** its native integration tests pass but comparison finds no outcome benefit or longer completion time
- **THEN** it remains disabled and the decision records the observed trade-off

#### Scenario: No LSP candidate demonstrates benefit
- **WHEN** the bounded evaluation finishes without sufficient evidence for any LSP capability
- **THEN** the delivered selection contains no LSP and retains appropriate project-native acceptance checks

### Requirement: Evaluate shared Serena diagnostics before duplicate providers

The capability assessment SHALL prefer existing Serena navigation, editing and
file-diagnostics capabilities when they meet accepted needs. It SHALL start with
explicit tools and project-native checks while hooks remain off, and assess
ready-made automatic behavior only when useful within the existing bounded
experiment budget. A custom adapter, vendor patch or separate overlapping provider
SHALL require a demonstrated important gap after supported configuration and
native checks have been evaluated; preserving automation alone is not such a gap.
Reusing Serena SHALL
mean reuse of a compatible project-scoped service and its language-server state,
not a new Serena process or a model turn for every hook. A separate overlapping
provider SHALL require a demonstrated useful gap and accepted total-result cost.

An API name, file argument or output limit SHALL NOT establish current diagnostics,
strict mutation isolation or compact meaningful delivery. Acceptance SHALL verify
actual installed language support, project identity, scan/notification scope,
current revision, error/clearance, unavailable versus clean status, output size
and full-detail access, concurrent clients and bounded cold/warm behavior. If a
small maintained integration cannot meet these contracts, the candidate SHALL
remain explicit-only or disabled. User data and unrelated explicit tool semantics
SHALL be preserved when duplicate providers are retired.

The comparison SHALL also evaluate suitable edits performed through Serena,
including explicit batch-end diagnostics and any supported inline diagnostic
result, without requiring an extra model dispatch solely for automatic checks.
Tool-use percentages SHALL NOT replace accepted-result measurements. Actual
exposed editing/creation capabilities, atomic/no-op/partial-failure behavior and
recovery SHALL be verified before promoting this workflow. Native generators,
formatters, unsupported operations and recovery SHALL retain appropriate fallback
paths; their automatic coverage or deliberate explicit verification SHALL be stated.

#### Scenario: Existing Serena covers the accepted need
- **WHEN** its supported tools and project-native checks satisfy the task's acceptance criteria
- **THEN** the kit uses that simpler path and retires verified redundant managed diagnostics instead of adding or retaining an equivalent custom stack

#### Scenario: Automatic integration would require a bespoke workaround
- **WHEN** the ready-made path fails strict trigger, freshness or output requirements and no important unmet product need justifies custom work
- **THEN** automatic diagnostics remain off and the task uses suitable explicit tools and project-native checks

#### Scenario: Existing Serena can serve an eligible diagnostic request
- **WHEN** an accepted changed file has a compatible Serena service already running
- **THEN** the candidate reuses that service without a duplicate language-server instance for the same purpose, a per-hook Serena startup or an additional model request to dispatch diagnostics

#### Scenario: A file API synchronizes unrelated project changes
- **WHEN** a single-file call polls or notifies other changed files outside the originating mutation
- **THEN** its name does not qualify it for automatic use; the ready-made path must demonstrate scoped behavior or remain explicit-only/disabled unless a separately proven important need justifies custom work

#### Scenario: Empty or length-limited output lacks usable evidence
- **WHEN** the API returns no findings without authoritative current completion, stale cached findings, or an answer-too-long notice
- **THEN** the integration does not claim clean diagnostics or complete delivery and must pass the existing uncertainty/minimal-output contract before automatic acceptance

#### Scenario: Intermediate edits temporarily break a declaration
- **WHEN** a coherent sequence changes a declaration and its references
- **THEN** the candidate compares verification of the completed changed-file batch with per-edit delivery, preserves final project checks, and does not inject repeated intermediate findings merely to keep an automatic feature active

#### Scenario: A required edit cannot use the active Serena surface
- **WHEN** creation is excluded, an external generator must run or the Serena client is unavailable
- **THEN** the agent uses an appropriate native path, verifies any partial mutation before retry, and does not spend additional setup or tool calls solely to satisfy a target Serena usage percentage

### Requirement: Bounded optimization experiments

Existing logs, checks and comparable evidence SHALL be reused before new model-backed experiments. Each new comparison SHALL define its task, acceptance oracle, identities, maximum attempts and stopping rule before execution. Deterministic checks SHALL precede model-backed checks. The comparison SHALL stop after sufficient evidence or when repeated attempts add no information; inconclusive results SHALL NOT require unbounded reruns. No further runs of the original source-kit consumer SHALL be required.

#### Scenario: Comparable evidence already exists
- **WHEN** an existing outcome or delegation run covers the same current inputs and contract
- **THEN** it is reused with its limitations instead of repeating the model work

### Requirement: Model and context efficiency within authorized routing

Optimization SHALL evaluate lead reasoning effort, unnecessary context, redundant preparation, waiting, worker execution and total delegation cost. Provider preference SHALL follow the kit orchestration configuration rather than fixed provider roles, while preserving acceptance. All OpenAI assignments SHALL remain in the Astra family without hidden provider, billing or Fast-mode changes. Reasoning effort SHALL be selected at supported task or turn boundaries according to uncertainty and risk, without restarting ongoing work solely to change effort. Changed effort defaults SHALL require comparable accepted-result evidence; token savings SHALL NOT justify a quality regression. Lost child results SHALL be distinguished from model/auth/quota failure and recovered through their owning mechanism before completed work is duplicated. Quota pacing across account windows and token-benefit comparisons are owned by the separate follow-up change and SHALL NOT be claimed by this change.

#### Scenario: A child ends without a final artifact
- **WHEN** evidence shows partial work without a model/auth/quota rejection
- **THEN** its visible state and delivery mechanism are reconciled without inferring provider unavailability or automatically transferring the work to GPT

#### Scenario: A lower effort candidate saves tokens but misses a defect
- **WHEN** that candidate fails unchanged acceptance
- **THEN** it does not become the global default

#### Scenario: Effort is selected without a fixed agent profile
- **WHEN** an assignment can use a supported lower effort without changing its required outcome
- **THEN** the lead selects that model/effort combination directly, the effective values are visible in its conversation pane, and no separate agent definition is required

### Requirement: Complete accepted scope remains the measure

The final report SHALL distinguish completed optimization, observed savings, unresolved attribution and rejected candidates. It SHALL record the selected global capability set and all required checks. Turning hooks off SHALL NOT imply that unperformed language checks passed, nor waive the consuming project's applicable tests.

#### Scenario: Work completes with hooks off
- **WHEN** the product's actual acceptance checks pass without automatic LSP
- **THEN** completion cites those checks and does not invent an LSP-clean result

### Requirement: Necessary minimum information across automated interfaces

Every automated interface and model-input path owned by the kit SHALL expose only information necessary to understand the current result or make the next task decision. This applies to hooks, LSP/MCP adapters, lifecycle and status notifications, diagnostic/fallback output, tool-discovery descriptions, automation wrappers and delegated-result handoffs. No-change automatic events SHALL be silent; explicitly requested status SHALL return only the requested facts. New findings and failures SHALL preserve actionable identity, original cause, required uncertainty and relevant severity without routine metadata dumps, repeated policies, entire logs or unchanged result inventories. Detailed evidence SHALL be available on demand with a concise reference when useful. Repeated semantic outcomes SHALL be deduplicated within their applicable scope; delivery errors SHALL NOT create feedback loops.

Each interface SHALL have a justified summary bound and representative output checks. The bound SHALL be a ceiling, never a desired message size. Minimization SHALL NOT conceal material errors, truncate required protocol fields, claim an unperformed check passed or remove information necessary for the consuming task. Existing native protocols SHALL remain valid.

#### Scenario: Routine operation adds no actionable information
- **WHEN** an automatic status or diagnostic event contains no new task-relevant result
- **THEN** it injects no model context instead of emitting a clean-file list or routine status JSON

#### Scenario: A failure has a large detailed log
- **WHEN** an automated interface reports that failure
- **THEN** its summary contains the necessary cause, affected target and next useful evidence reference while the full log stays available on demand

#### Scenario: Several transports report the same outcome
- **WHEN** a hook, fallback or wrapper repeats an already delivered semantic result
- **THEN** only the first necessary summary reaches the model and newer material findings remain deliverable

#### Scenario: Minimum output is validated
- **WHEN** an interface's concise mode is accepted
- **THEN** checks prove quiet no-ops, actionable error/changed-result summaries, detail retrieval, deduplication and preserved protocol semantics

### Requirement: Quota-aware pacing across account windows

Dispatch and feedback cadence SHALL consider fresh available account-limit observations, their scope, reset times and observed use, prioritizing conservation of lead capacity for consequential decisions while following the orchestration configuration for routing. Account state SHALL be shared by the controller's tasks on that account while task work and authorization remain separate. Missing or stale telemetry SHALL remain unknown, not zero or unlimited; local request logs SHALL NOT be treated as an authoritative subscription remainder; no model call SHALL be made solely to estimate remaining quota. Pacing SHALL adjust new work allocation, supported concurrency, reasoning effort and feedback cadence without abandoning an active healthy executor, dropping accepted work or adding purchases. Reset-time bursts SHALL be avoided across tasks sharing an account. This requirement paces orchestration account windows; it SHALL NOT replace skill-evolution episode or period learning budgets.

#### Scenario: Lead capacity is depleting rapidly
- **WHEN** comparable current observations show the lead account window depleting quickly
- **THEN** new assignments and nonessential coordination are paced to preserve consequential decisions while healthy executors finish their owned work

#### Scenario: Several tasks share one account
- **WHEN** multiple tasks would retry or resume on the same account at its reset time
- **THEN** retry eligibility is distributed instead of issuing a synchronized burst
