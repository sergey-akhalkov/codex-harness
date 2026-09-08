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

The delivered Serena and Codebase Memory selection SHALL support targeted structure, definition, reference and impact queries with compact results. Consumers MUST verify repository identity and current coverage, reuse a verified unchanged index, and refresh or use fresh source on change or incomplete coverage. `apply_patch` SHALL be the preferred native path for suitable bounded text edits; semantic edits and deterministic generators SHALL remain available when they better preserve correctness.

#### Scenario: Retrieve and edit one symbol
- **WHEN** a task needs one definition and its callers in an indexed project
- **THEN** the workflow retrieves bounded relevant results, applies a scoped edit, and verifies the changed behavior without requiring whole-file regeneration

#### Scenario: Index or language coverage is incomplete
- **WHEN** graph results lack verified freshness or the requested source scope
- **THEN** the consumer reports the limit and uses current semantic or direct source evidence, without treating absence as proof

### Requirement: Reuse valid work without suppressing required checks

Agents SHALL reuse inspected context and completed checks while their relevant input identity remains unchanged. Repeated calls, duplicate investigations, silent no-op updates and redundant status polling SHALL be avoided. A change, failure or unresolved risk MUST invalidate the affected evidence and trigger the necessary scoped follow-up.

#### Scenario: Unchanged evidence remains valid
- **WHEN** several related decisions use the same unchanged source and completed check
- **THEN** the workflow reuses that evidence and performs only missing work

#### Scenario: Inputs change after verification
- **WHEN** code or a relevant runtime input changes
- **THEN** stale success is not used to claim acceptance of the new state

### Requirement: Actual task-appropriate reasoning

The kit SHALL expose and use actual native reasoning settings for bounded task complexity and risk, with an explicit conservative default and escalation on demonstrated uncertainty. Selection MUST preserve Astra-only OpenAI assignments, preferred Grok middle, reserve policy and existing subscription routes. Instructions requesting brevity alone SHALL NOT count as lowering reasoning effort. Unsupported in-turn switching MUST be reported accurately; explicit user model and effort overrides MUST be respected.

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
