# token-efficient-agent-workflow Specification
## Purpose
Reduce avoidable input and output tokens and time to a verified result through selective compression, native orchestration, precise source work and task-appropriate reasoning, without weakening acceptance.
## Requirements

### Requirement: Transparent and bounded RTK compression

The kit SHALL make RTK available globally and automatically compress verified shell command forms through a narrow native hook. The supported boundary SHALL explicitly select the native runner when the hook cannot establish the actual per-call shell; ordinary shell commands and their aliases/functions SHALL remain unchanged. The adapter MUST preserve command intent, arguments, working directory, environment, exit status and applicable timeout semantics. Unsupported or ambiguous syntax, machine-readable output, exact source reads, already compact commands and explicit bypass SHALL pass unchanged. Hook processing SHALL be bounded and quiet when no transformation applies; it SHALL NOT start models or diagnostic scans.

#### Scenario: Supported command with verbose output
- **WHEN** a fresh consumer executes a supported command in another project
- **THEN** the operation executes once, useful output is compressed and its exit status and scope remain correct

#### Scenario: Ambiguous runner or shell syntax
- **WHEN** a command uses an unsupported runner, environment assignment, pipeline, redirection or control flow
- **THEN** the hook preserves the original command without a blocking suggestion or an extra model turn

#### Scenario: Raw evidence is needed
- **WHEN** compressed output omits detail needed for a decision
- **THEN** retained original output can be inspected without executing the operation again, and retention limits or truncation are explicit

#### Scenario: Adapter is unavailable or times out
- **WHEN** bounded hook processing cannot safely produce a rewrite
- **THEN** the original command remains usable, the adapter does not execute it, and any operational fault is distinguishable from a valid empty result

### Requirement: Native Code Mode with evidence-preserving results

The selected global configuration SHALL enable Code Mode through a supported native contract on the installed consumer. Agents SHALL use its orchestration for worthwhile groups of independent calls and return relevant aggregates to model context. Errors, incomplete results, provenance and recovery paths MUST survive aggregation; dependent edits and approvals MUST remain ordered.

#### Scenario: Independent retrieval batch
- **WHEN** independent tool results contain a large irrelevant payload and one failed call
- **THEN** Code Mode returns the necessary findings and the failed call explicitly while retaining access to details

### Requirement: Precise semantic retrieval and scoped editing

The delivered Serena and CodeGraph selection SHALL support targeted structure, definition, reference and impact queries with compact results. Consumers MUST verify repository identity and current coverage, reuse a verified unchanged index, and refresh or use fresh source on change or incomplete coverage. Serena SHALL be the starting choice for a known-file symbol, exact references and suitable edits; CodeGraph SHALL be selected for bounded repository discovery and relationships when it avoids several source reads. Literal text, configuration and documents SHALL retain scoped native retrieval. A task SHALL NOT require both code tools or graph setup when one already answers it. `apply_patch` SHALL be the preferred native path for suitable bounded text edits; semantic edits and deterministic generators SHALL remain available when they better preserve correctness.

#### Scenario: Retrieve and edit one symbol
- **WHEN** a task needs one definition and its callers in an indexed project
- **THEN** the workflow retrieves bounded relevant results through the suitable provider, applies a scoped edit, and verifies the changed behavior without whole-file regeneration or redundant retrieval

#### Scenario: Index or language coverage is incomplete
- **WHEN** graph results lack verified freshness or requested source scope
- **THEN** the consumer reports the limit and uses current semantic or direct source evidence without treating absence as proof

#### Scenario: Graph relationships are approximate
- **WHEN** an edge resolves to an ambiguous or unrelated same-name symbol
- **THEN** it remains a candidate until verified, and a refactor or exhaustive-reference claim uses current semantic or source evidence

### Requirement: Enforced code-tool response budgets

Managed CodeGraph responses SHALL be bounded before entering model context. The initial default SHALL be at most 4 KiB of serialized UTF-8 result data, including metadata; explicit larger requests SHALL remain at most 16 KiB. Search and one-hop relationship requests SHALL default to five results. Broad exploration SHALL require deliberate opt-in with a specific question and at most two requested source files by default; file count SHALL NOT substitute for the response-byte limit. Unbounded whole-file reads, unrestricted repository dumps and repeated exploratory fan-out SHALL NOT be default operations.

Responses SHALL retain project and generation identity, errors, warnings, coverage limits and explicit truncation. Oversized content SHALL produce a useful bounded partial answer or a bounded refusal with a local detail identifier and a narrowing hint. It SHALL NOT silently discard matches, label a partial result exhaustive, or return duplicate full payloads in text and structured content. Retained details and their retrieval SHALL be separately bounded; a detail request SHALL NOT rerun the backend. All number/range limits SHALL be validated at the managed boundary. Startup instructions and tool descriptions SHALL remain compact and match actual availability.

#### Scenario: One-file exploration returns a large payload
- **WHEN** upstream exploration returns more than the default response budget despite `maxFiles=1`
- **THEN** the managed response remains within budget, marks omitted material and offers bounded access or narrowing without silently increasing the limit

#### Scenario: A common name has many references
- **WHEN** a search, reference or impact response exceeds its count or byte allowance
- **THEN** the response explicitly states limited coverage and permits refinement by file or symbol instead of automatic pagination of the whole repository

#### Scenario: Upstream response is malformed or failed
- **WHEN** upstream returns a protocol error, oversized frame, invalid encoding or tool failure
- **THEN** a bounded explicit error reaches the consumer, original diagnostic details stay locally bounded, and no apparently successful empty answer replaces the failure

### Requirement: Measured selection without subscription overclaims

Acceptance SHALL compare the same questions on the pack and a locally selected large real repository: exact-symbol lookup/body, file overview, direct callers, cross-file flow, ambiguous name, absent symbol and high-fan-out retrieval. Current source or semantic evidence SHALL establish the answer oracle. Cold preparation, warm queries, schemas/initialization, follow-up reads, parent/child coordination and omitted-result recovery SHALL be accounted for separately. Measurements SHALL distinguish request bytes, upstream bytes, delivered bytes, elapsed time, tokenizer estimates or measurements, and actual subscription usage. Fewer tool calls SHALL NOT by itself prove lower cost.

Small exact tasks SHALL avoid broad exploration and remain within an initial 8 KiB cumulative retrieval budget unless a stated missing fact warrants expansion. Bounded graph tasks SHALL return enough verified information to answer the question; savings obtained by hiding necessary information SHALL fail acceptance. Compared with the selected raw baseline, the optimized path SHALL reduce unnecessary delivered bytes without losing the oracle answer. No fixed weekly-limit saving or overall speedup SHALL be claimed without direct measurement.

#### Scenario: A compact caller query answers the question
- **WHEN** a bounded graph query returns the needed direct callers with verified locations
- **THEN** the agent reuses that answer and does not also fetch full bodies or call Serena merely to complete a tool checklist

#### Scenario: Additional retrieval is needed
- **WHEN** the initial budget cannot answer a required question
- **THEN** the agent identifies the missing fact, narrows the next request, preserves earlier valid results and reports any remaining uncertainty instead of silently escalating breadth

#### Scenario: Updated instructions are delivered
- **WHEN** a new parent or tool-capable child starts outside the pack
- **THEN** it receives the current selection and budget rules through the existing global links, and already running sessions are identified as needing reload

### Requirement: Reuse valid work without suppressing required checks

Agents SHALL reuse inspected context and completed checks while their relevant input identity remains unchanged. Repeated calls, duplicate investigations, silent no-op updates and redundant status polling SHALL be avoided. A change, failure or unresolved risk MUST invalidate the affected evidence and trigger the necessary scoped follow-up.

#### Scenario: Unchanged evidence remains valid
- **WHEN** several related decisions use the same unchanged source and completed check
- **THEN** the workflow reuses that evidence and performs only missing work

#### Scenario: Inputs change after verification
- **WHEN** code or a relevant runtime input changes
- **THEN** stale success is not used to claim acceptance of the new state

### Requirement: Actual task-appropriate reasoning

The kit SHALL expose and use actual native reasoning settings for bounded task complexity and risk, with an explicit conservative default and escalation on demonstrated uncertainty. Selection MUST preserve Astra-only OpenAI assignments, the model-selection and recovery contract owned by [agent delegation](../agent-delegation/spec.md#requirements), and existing subscription routes. Instructions requesting brevity alone SHALL NOT count as lowering reasoning effort. Unsupported in-turn switching MUST be reported accurately; explicit user model and effort overrides MUST be respected.

#### Scenario: Routine bounded task
- **WHEN** a task has clear inputs, low risk and a deterministic acceptance check
- **THEN** its native task configuration can select a lower supported reasoning effort, with its effective effort observable

#### Scenario: Difficult or high-risk task
- **WHEN** a task requires substantial reasoning or exposes a correctness blocker
- **THEN** a higher supported effort is selected without silently changing model family or billing

### Requirement: Verified efficiency and global delivery

Acceptance SHALL exercise the installed entry points outside the harness and compare representative raw and optimized work with the same correctness oracle. Measurements SHALL distinguish output bytes, estimated or measured tokens, elapsed time and subscription usage. Supported cases MUST show useful reduction without material loss of correctness or speed; unsupported cases MUST preserve behavior. Total accepted-result cost, including coordination and recovery, SHALL determine workflow choices; no unmeasured whole-session or weekly-quota saving SHALL be claimed.

#### Scenario: Acceptance report
- **WHEN** the change is declared complete
- **THEN** all selected directions have implementation and applicable current checks, raw/compressed evidence and latency limits are recorded, and global lifecycle verification has passed

### Requirement: Prepared replay and restart compete on complete remaining cost

When repeated interaction or preparation dominates, the workflow SHALL compare continuing valid owned state, returning it to a known starting state, and restarting the minimum necessary component. It SHALL learn uncertain transitions through observed actions, exercise their replay through the existing automation owner and preserve the actual parent environment where its differences matter. A single persistent launch SHALL NOT be mandatory when a reliable restart is cheaper or necessary. Held or identity-bound artifacts SHALL NOT be changed underneath an active operation; tested candidate changes shall be integrated after the relevant release and revalidation.

#### Scenario: Several unknown interactions share an expensive startup
- **WHEN** the application is prepared and its relevant state and ownership remain valid
- **THEN** the agent observes individual transitions and return paths, tests the corresponding automation there, and completes the required integrated run rather than rebuilding the entire environment to discover each next control

#### Scenario: Reset is slower or less reliable than restart
- **WHEN** current evidence shows that returning to the starting state costs more or cannot restore necessary conditions
- **THEN** the agent restarts the necessary component, preserves reusable unaffected preparation and re-establishes invalidated state

#### Scenario: A separate diagnostic host omits a required condition
- **WHEN** the reduced environment lacks a configuration, observer, timing or recovery condition used by the real entry point
- **THEN** the agent does not treat its passing result as parent acceptance and restores the relevant condition before relying on the reduction

### Requirement: Parallel optimization investigations feed the next delivery decision

When delegation is available, authorized and useful, agents SHALL assign a bounded independent question or correction that can shorten the current path to acceptance. The brief SHALL identify the parent decision or integration point that will consume the result. Agents SHALL include coordination and integration costs, preserve visibility and aggregate resource limits, and keep one owner for shared runtime resources. They SHALL consume useful findings when available and reconsider assignments whose premises have become obsolete. Delegation SHALL NOT be a compulsory periodic activity or a reason to delay a cheaper direct task.

#### Scenario: A worker can resolve a missing observation path
- **WHEN** the parent can continue useful work while an independent worker checks an existing step or attach capability
- **THEN** the parent consumes the verified finding in its next relevant action without duplicating the investigation or sharing uncontrolled access to the same runtime

#### Scenario: Parallel activity does not advance acceptance
- **WHEN** a proposed worker would polish an unrelated component, duplicate current investigation or cost more than the remaining direct work
- **THEN** the agent leaves that work out and continues the accepted path without manufacturing a delegation requirement
