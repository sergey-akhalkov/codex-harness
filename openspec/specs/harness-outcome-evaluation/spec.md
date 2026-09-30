# harness-outcome-evaluation Specification

## Purpose

Measure whether reusable agent workflows improve accepted outcomes and elapsed time using comparable native executions, explicit correctness criteria and preserved failures. This change keeps that measurement capability and closes without a proven two-consumer speedup.

## Requirements

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

### Requirement: Complete audit coverage has explicit closure evidence

This change SHALL retain a traceable mapping from every enumerated supplied-audit point and accepted supplemental finding to its evidence, requirement, implementation or investigation task and observable closure criterion. Static reasoning, executed reproduction, historical evidence and hypotheses SHALL remain distinguishable. A mandatory correctness or delivery requirement SHALL close only on its required checks and actual integration. Optional optimization candidates SHALL close their investigation only with an evidenced adopt, reject or inconclusive decision; an inconclusive result MUST NOT be described as demonstrated improvement or satisfy a mandatory implementation requirement. Superseded claims SHALL be corrected rather than silently omitted. The mapping SHALL use the existing change artifacts and evidence owners, without a parallel tracker or mandatory new report protocol.

#### Scenario: A proposed optimization is rejected
- **WHEN** a compiler or prompt-layout candidate fails its declared benefit criterion
- **THEN** its investigation records that result and retains the previous default, while unrelated mandatory corrections remain open

#### Scenario: A task has only a source inspection
- **WHEN** an implementation task requires process or installed-consumer evidence but only a static analysis exists
- **THEN** the task remains unverified and cannot be checked off as completed behavior

### Requirement: Benefit decisions consume executed and independently accepted evidence

An automatic or recorded adoption claiming improvement SHALL be supported by actual comparable attempts, independent task acceptance and a positive result for a predeclared quality, time or resource objective. Agreement among report fields, a tolerance-compliant regression or a nonempty accounting label SHALL NOT by itself prove benefit. Required correctness SHALL be enforced independently of the candidate and SHALL NOT be relaxed for speed. Evidence identity SHALL bind the evaluated revision, configuration, actual model/effort/provider, relevant tools, consumed instructions and acceptance version. Missing, stale, changed or unverifiable evidence SHALL leave the claim unsupported. Existing historical exceptions that closed delivery without quantified benefit SHALL remain historical exceptions, not retroactive proof or authorization for new model calls.

#### Scenario: A consistent adoption record has no positive effect
- **WHEN** arithmetic and quality labels are internally consistent but no declared benefit is established
- **THEN** the record can be identified as consistent while the improvement claim and default adoption remain unsupported

#### Scenario: A real correctness fix costs more time
- **WHEN** a reproduced required correctness failure is eliminated and applicable acceptance passes
- **THEN** it can satisfy its declared quality objective with the cost disclosed, without being relabeled a speed or token saving

#### Scenario: A candidate modifies its oracle
- **WHEN** the candidate changes expected outcomes, acceptance inputs or historical evidence within its writable scope
- **THEN** the affected comparison fails integrity acceptance and cannot authorize adoption

### Requirement: Experimental units and complete task accounting remain valid

Comparisons SHALL identify independent tasks, declared pairs or experimental blocks; all baseline-candidate combinations from one case MUST NOT be treated as independent repetitions. Failed, cancelled, incomplete and retried attempts and attributable leader/worker verification and integration work SHALL remain accounted without double counting. Time to acceptance SHALL preserve causal and overlapping intervals rather than sum concurrent worker durations. Missing usage SHALL remain unknown. Usage categories SHALL retain their units and subset relationships, and provider cost or subscription allowance SHALL NOT be inferred from bytes or token totals. Cost per accepted task SHALL be accompanied by acceptance rate, case mix and coverage; a zero accepted-task denominator SHALL be undefined.

#### Scenario: Two baseline and three candidate attempts share a case
- **WHEN** a report forms their six possible cross-comparisons
- **THEN** those comparisons do not become six independent samples and inference uses the declared task/pair/block units

#### Scenario: A cheap final attempt hides expensive failures
- **WHEN** earlier attempts or workers consumed resources before a final accepted result
- **THEN** the complete task includes those attempts and corrections with attributable usage or explicit unknowns

#### Scenario: No task is accepted
- **WHEN** every attempted task fails independent acceptance
- **THEN** cost per accepted task is undefined, the failure rate is visible and no efficient-completion claim is produced

### Requirement: Optimization claims use predeclared scope and uncertainty

Before observing comparative outcomes, an optimization evaluation SHALL declare the task population, acceptance oracle, practically meaningful effect, allowed noncritical variation, nuisance controls, sampling or stopping policy and treatment of repeated candidate selection. Statistical claims SHALL use uncertainty appropriate to the declared independent units; lack of a detected difference MUST NOT imply equivalence. Deterministic counterexamples SHALL support only the properties they exercise. Adoption SHALL include total expected benefit over a stated use horizon after implementation, evaluation, maintenance and recovery costs where a net-saving claim is made. No finite evaluation SHALL be described as universal reliability or guaranteed future savings. Model-backed runs SHALL remain explicitly selected, visible and within the authorized provider and billing route.

#### Scenario: Only a byte reduction is observed
- **WHEN** retained output becomes smaller but end-to-end token or elapsed measurements are unavailable
- **THEN** the accepted claim is limited to that output property and does not imply subscription or general task savings

#### Scenario: Repeated trials stop on the best result
- **WHEN** a comparison changes its stopping rule after seeing favorable data
- **THEN** it cannot establish the original predeclared benefit without accounting for the changed selection procedure

#### Scenario: Small savings do not repay evaluation
- **WHEN** measured per-task savings do not offset the declared total cost over the intended use horizon
- **THEN** the candidate fails that net-saving claim even if its isolated execution is faster
