## Purpose

Measure whether reusable agent workflows improve accepted outcomes and elapsed time using comparable native executions, explicit correctness criteria and preserved failures. This change keeps that measurement capability and closes without a proven two-consumer speedup.

## ADDED Requirements

### Requirement: Compare representative native outcomes

The kit SHALL provide six to eight fixed outcome scenarios evaluated through the existing native Codex execution and usage facilities. Cases MUST cover project command discovery, actual entry-point identity, regression reproduction, a meaningful process failure and a negative activation control. Each case SHALL declare inputs, allowed effects, correctness criteria, stop conditions and a practically meaningful improvement criterion before comparison. Baseline and candidate MUST use equivalent source state, runtime, model and effort, provider, tool access, hook version and cache policy, with skill availability as the declared treatment for skill comparisons.

#### Scenario: Candidate guidance could leak into the baseline

- **WHEN** a baseline execution uses an isolated Codex environment
- **THEN** the runner verifies its actual skill discovery and configuration so linked candidate skills or project records produced by prior attempts do not contaminate the baseline

#### Scenario: A model or toolchain changes between attempts

- **WHEN** comparison inputs differ in a material uncontrolled way
- **THEN** the affected attempts are retained but excluded from like-for-like improvement claims with a stated reason

### Requirement: Account for the whole accepted result

Each attempt MUST record completion and correctness, time to first useful execution or validation signal, elapsed time through required verification and rework, interventions, failures and available usage. Delegated work and retries SHALL remain attributable to the parent attempt; elapsed wall time MUST NOT be calculated by summing overlapping child durations. Missing provider usage MUST be unknown rather than zero. Token counts MUST NOT be presented as exact subscription-quota savings.

#### Scenario: A fast initial answer needs correction

- **WHEN** an agent produces an answer quickly but later verification requires rework
- **THEN** reported completion time includes that rework and required verification

#### Scenario: A provider fails or omits usage

- **WHEN** an attempt fails authentication, quota or infrastructure checks, or lacks usage records
- **THEN** the failure and any partial evidence are preserved, unknown usage stays unknown, and no model or billing substitution occurs silently

### Requirement: Keep quantitative benefit unproven for this change

Implementation acceptance MUST NOT require repeated paired executions across two consuming projects. The 2026-09-18 user decision removes the two-consumer quantitative speed comparison from this change. Installation, skill discovery and use, outside-project qualitative consumption and local controlled native pairs MAY close without a proven time or correctness improvement. Benefit SHALL remain unproven: reports MUST NOT claim repeatable acceleration, subscription-quota savings or general proof from the local pairs or from unrun two-consumer comparisons. Further model-backed two-consumer comparison SHALL NOT run under this change. A later change MAY reopen measurement with criteria frozen before seeing results; this change MUST NOT lower the historical 15-second / 15% verified-time threshold retroactively or relabel inconclusive or missing comparisons as a speed win.

#### Scenario: A skill activates but outcomes do not improve

- **WHEN** skill discovery and invocation pass but the outcome comparison is inconclusive or the two-consumer quantitative comparison is not run
- **THEN** installation may be reported as working while benefit remains unproven
- **AND** this change's implementation acceptance may close on that honest status after the explicit scope decision

#### Scenario: Faster execution skips a required check

- **WHEN** a candidate is faster because it omits required validation or violates a case invariant
- **THEN** that attempt fails correctness acceptance and cannot support a speed improvement claim

### Requirement: Attribute diagnostic latency separately

The evaluation SHALL preserve completed `stabilize-diagnostic-reconciliation` comparisons as historical evidence and distinguish their diagnostic effect from skill effects. New comparisons SHALL use the same accepted capability selection in both arms, currently the hooks-off default owned by `reduce-subscription-waste`. Any isolated candidate hook comparison SHALL measure latency and correctness separately using unchanged-tree, edit and concurrent-work cases without restoring rejected global operations. Required consuming-task verification and honest incomplete states SHALL remain intact; silence SHALL NOT mean clean diagnostics. This change MUST NOT duplicate diagnostic implementation ownership.

The accepted capability selection also keeps Fast off, native memories off, CBM automatic index/watch off, and OpenAI assignments Astra-only. Withdrawn support for the original source-kit consumer SHALL NOT be resumed. Local controlled native pairs for the remaining cases MAY close their one-pair task without proving benefit. The 2026-09-18 decision closes quantitative benefit tasks without a two-consumer comparison. Further model-backed runs for that comparison SHALL NOT be spent under this change. A second real consumer SHALL NOT be chosen merely to complete this change.

#### Scenario: Both hooks and skills changed during the work

- **WHEN** integrated results are evaluated
- **THEN** isolated hook before/after cases use a fixed skill configuration, skill before/after cases use the same accepted capability selection, and their claimed effects are reported separately

#### Scenario: Diagnostics are faster because work was omitted

- **WHEN** a faster hook fails to deliver required findings or reports incomplete work as clean
- **THEN** it fails acceptance regardless of elapsed time

### Requirement: Keep evaluation economical and inspectable

The evaluation SHALL reuse existing runners and usage records, keep detailed runtime traces outside tracked reusable sources and provide a concise report linking to evidence. Deterministic checks SHALL validate reporting semantics before model-backed runs. Model-backed evaluation MUST be explicitly selected for relevant changes and MUST NOT run automatically for every tool call or documentation edit. Assignments SHALL preserve the authorized provider and capability-level policy.

#### Scenario: Only the result formatter changes

- **WHEN** reporting logic changes without changing skill behavior
- **THEN** deterministic fixtures verify failed, incomplete, overlapping-child and missing-usage cases before any decision to spend model-backed runs

#### Scenario: A later change affects one workflow

- **WHEN** only one existing workflow's behavior changes
- **THEN** the relevant outcome cases and required risk checks can run without an unconditional full-suite model evaluation
