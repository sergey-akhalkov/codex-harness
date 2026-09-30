## ADDED Requirements

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
