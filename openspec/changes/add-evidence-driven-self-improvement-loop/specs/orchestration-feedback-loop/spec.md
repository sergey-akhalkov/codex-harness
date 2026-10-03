## ADDED Requirements

### Requirement: Beads owns the full hypothesis lifecycle

Each self-improvement hypothesis SHALL have one durable Beads `task` labeled `hypothesis`, retaining its initial measurement question and evidence basis, causal claim, applicability, selected experiment method and rationale, acceptance, specification reference, candidate branch/base/revision and local worktree ownership references, baseline and prepared runtime identities, experiment references, decisions and reconsideration conditions. Repeated experiments SHALL remain attributable to that hypothesis without requiring a duplicate task per trial. Work status and benefit outcome SHALL remain distinct: a checked implementation is not an adopted improvement, and a closed rejected experiment is not a failed board operation. Existing feedback intake, votes and skill-authoring ownership SHALL remain intact; measured hypothesis admission SHALL NOT manufacture votes or wait for unrelated vote thresholds.

#### Scenario: B is implemented while evaluating A
- **WHEN** B has a functionally accepted implementation but has not been evaluated for its own benefit
- **THEN** its card retains the implementation reference and pending benefit work without recording adoption or closing the hypothesis as completed

#### Scenario: A completed investigation rejects a candidate
- **WHEN** the required experiment is completed with an evidenced rejection
- **THEN** its card can close with outcome `reject`, retaining the cause and evidence rather than implying the improvement was installed

### Requirement: Every hypothesis requires its own OpenSpec change from initial measurement

Before its first targeted baseline measurement, every hypothesis SHALL reference its own OpenSpec change created through the installed CLI in the selected project or registered store. Existing routine observations SHALL be reusable as the reason for investigation without creating a change per observed event. The initial change SHALL state the observed problem, investigation scope, measurement question and tasks, evidence references and limits; it need not invent a solution before measurement. The same change SHALL be developed into proposal, requirements, design and tasks covering hypothesis formulation, implementation, predeclared experiment acceptance, remeasurement, decision and verified restoration or adoption, and SHALL pass applicable completeness and validation checks before candidate implementation. This rule SHALL cover small changes, instructions, skills, plugins, MCPs, other tools and configuration as well as executable code; it supersedes optional-spec routing for items labeled `hypothesis` only. Beads SHALL own identity, scheduling, status and decisions; OpenSpec SHALL own the measurement and experiment plan, planned behavior, design, acceptance and lifecycle checklist. Their references and actual completion state SHALL agree without duplicating the decision journal. Rejected or otherwise unadopted capability deltas SHALL NOT synchronize into main specifications; their planning and evidence SHALL remain referencable without representing the candidate as delivered. Native schemas, templates and external skill packages SHALL NOT be patched to satisfy this prerequisite.

#### Scenario: A grounded investigation begins with measurements
- **WHEN** routine evidence identifies a recurring cost but a specific solution has not yet been established
- **THEN** a linked change owns the targeted baseline measurement plan before execution and later owns the resulting hypothesis and decision path without a new change per phase

#### Scenario: A measurement workload is not another hypothesis
- **WHEN** an existing build, search or diagnostic operation is selected to evaluate a candidate
- **THEN** its contract and evidence are linked from that candidate's change without requiring a second hypothesis card or change unless it independently becomes an improvement hypothesis

#### Scenario: A one-line candidate is ready for coding
- **WHEN** its hypothesis card lacks requirements, design or experiment acceptance in the linked change
- **THEN** implementation remains undispatched with the exact missing artifacts reported, regardless of the small edit size

#### Scenario: A registered specification store is selected
- **WHEN** a hypothesis uses an OpenSpec store separate from its source checkout
- **THEN** the card identifies the resolved store/change and all planning and validation operations use that same store while executable edits target the owned checkout

#### Scenario: Experimental rejection completes the agreed investigation
- **WHEN** the change explicitly specifies an experiment with adopt/reject/inconclusive outcomes and its applicable tasks have been performed
- **THEN** the outcome can complete that investigation without declaring an unadopted feature delivered, while any genuinely unfinished required tasks remain open

#### Scenario: A rejected hypothesis is retained or archived
- **WHEN** a completed rejected experiment leaves an OpenSpec delta describing its candidate behavior
- **THEN** retention or supported archival preserves the change reference and evidence without applying that unadopted delta to the main specifications

### Requirement: Evaluation relationships do not force or block the hypothesis queue

A hypothesis card SHALL identify its evaluation workload and selected method through an explicit experiment reference. When that workload is another hypothesis, their relationship SHALL be nonblocking; ordinary operation or task workloads SHALL NOT require a fabricated hypothesis card. The next hypothesis SHALL be selected from current evidence rather than necessarily being the preceding workload. The decision about A SHALL be recorded on A when the workload is B. B's own benefit evaluation SHALL remain separate and SHALL NOT be a prerequisite for deciding A. True implementation dependencies SHALL remain distinguishable from these evaluation relationships, and a dependent task that changes paired inputs SHALL be ineligible for that comparison.

#### Scenario: A is evaluated on B and B will be evaluated on C
- **WHEN** both relationships are recorded
- **THEN** the queue can decide A from completed B trials without waiting for B's benefit decision or creating a blocking chain through C

### Requirement: Removal authorization is distinct from benefit outcome

The existing hypothesis card SHALL own the removal proposal reference and explicit user approval, refusal or withdrawal separately from `adopt`, `reject` or `inconclusive`. The authorization reference SHALL identify the user's decision, reviewed proposal/source basis, target and behavior scope, covered experiment/integration/publication actions and superseding decisions. An agent or evaluator SHALL NOT manufacture user consent from a benefit verdict or general run authority. The referenced proposal SHALL carry the informed-removal contract from `self-improvement-loop`; neither a new approval tracker nor duplicated private transcripts SHALL be required.

Status SHALL distinguish pending approval, user-declined removal, unsupported benefit and completed authorized application through the existing work statuses and decision details. A favorable benefit result without the authority for the next stage SHALL remain pending that stage and SHALL NOT imply activation, publication, main-spec synchronization or full completion. Before applying a covered action, including after recovery, the loop SHALL resolve the current decision and compare the actual scope with its authorization. An unchanged authorized action SHALL reuse its approval; a refusal SHALL be respected without repeated prompts absent new evidence or user instruction.

#### Scenario: Benefit passes while publication approval is absent
- **WHEN** an approved isolated removal experiment succeeds but installed publication was not covered
- **THEN** its card retains the favorable evidence and identifies the pending authority without reporting the feature retired from the live installation

#### Scenario: A decision changes while the loop is stopped
- **WHEN** the user withdraws approval after an experiment but before its integration and the controller resumes
- **THEN** the current withdrawal prevents new removal effects and baseline advancement, preserving completed evidence and identifying any already applied effects needing recovery

#### Scenario: Refusal is not an experimental failure
- **WHEN** the user declines a removal with otherwise promising evidence
- **THEN** the card records that decision separately from measured benefit and prior-result search prevents the same proposal from being repeatedly presented

### Requirement: Benefit decisions reuse the board evidence owner

Each comparison decision SHALL identify its claimed metric view and attribution-policy identity, reference both observed and adjusted evidence with exclusions and coverage, and retain material infrastructure uncertainty. Resume or repeated publication SHALL preserve this binding; a newer normalization rule SHALL NOT silently rewrite an earlier decision. Public board summaries SHALL contain scoped conclusions and evidence references rather than private queue-holder details or raw traces.

That binding SHALL also retain the nuisance-control plan, actual ordering and condition checks, calibration identity, selected experiment method and rationale, actual experimental-unit and applicable complete-pair counts, baseline reuse basis, measurement bounds and any separately supported statistical claim under `harness-outcome-evaluation`. Missing run-variability evidence SHALL remain visible rather than being replaced by a confidence percentage or zero variance. A changed relevant plan, analysis method or calibration SHALL NOT inherit an earlier adoption without renewed evaluation of the retained evidence and any newly required execution.

Hypothesis decisions SHALL reuse and, where needed, extend the existing benefit-gate/ledger contract with links to independently accepted experiment evidence. Each decision SHALL identify the hypothesis, experiment, evaluated revisions, scope, quality result, measured effects, coverage and reason, and reference the resulting adoption or accepted-baseline restoration state. Rejection SHALL preserve the failed candidate and measurements; recording a verdict SHALL NOT prove runtime restoration. Retried publication of the same decision SHALL be idempotent; contradictory or incomplete newer evidence SHALL not silently restore an older adoption. An `inconclusive` result SHALL identify the missing observation and next action or deferral condition rather than immediately requeue identical work. Closed and deferred hypotheses SHALL participate in prior-result search, and reconsideration SHALL preserve the earlier result and record a new evidential basis before another implementation attempt.

#### Scenario: Rejection is followed by autonomous continuation
- **WHEN** the loop has published a rejection and verified the accepted runtime is restored
- **THEN** the existing card retains the evidence and reason and another eligible hypothesis can proceed without deleting the rejected change or creating a second decision journal

#### Scenario: Process recovery repeats decision publication
- **WHEN** the same completed experiment is reconciled after an interruption
- **THEN** its existing decision is confirmed without adding another apparent experiment or counting its costs twice

#### Scenario: A previously rejected mechanism is proposed again
- **WHEN** search finds the same mechanism rejected under the same relevant conditions
- **THEN** the loop reuses that result rather than rerunning it; changed conditions require a recorded explanation linked to the prior evidence

#### Scenario: The board is unavailable
- **WHEN** hypothesis ownership or decision publication cannot be read or written through Beads
- **THEN** the loop preserves local execution artifacts and reports the board failure without replacing it with a file-based hypothesis journal or advancing an unrecorded adoption
