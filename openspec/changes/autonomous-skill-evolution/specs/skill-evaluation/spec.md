## Purpose

Decide whether a skill change improves actual agent work using reproducible comparisons, protected acceptance conditions and proportionate resource use before activation.

## ADDED Requirements

### Requirement: Behavioral comparison with independent acceptance

Every behavioral library change SHALL have explicit success criteria established before acceptance. The comparison SHALL keep the surrounding applicable library fixed: addition against absence of the skill, update against its accepted version, consolidation against the old set, and retirement against retaining the skill. It SHALL preserve independent task requirements and common tools on both sides. Acceptance SHALL include at least one representative intended use, a similar request where the skill must not activate and a relevant boundary or failure case, including protected overlapping workflows. Cases used for final acceptance SHALL include a related case not used to refine the candidate. Task inclusion and weighting SHALL be declared before inspecting acceptance outcomes; the candidate MUST NOT select only its easiest successes or count its subtasks as extra completed user tasks. Outcome checks SHALL use deterministic project or artifact evidence where available; subjective assessment SHALL use a fixed rubric and distinguish judgment from execution evidence.

#### Scenario: New skill passes its example but fails transfer
- **WHEN** a candidate succeeds on the example used to write it but fails the independently held acceptance case
- **THEN** it is not activated as an improved skill and the failed transfer remains visible in the result

#### Scenario: A narrowly scoped correction
- **WHEN** an edit changes only non-behavioral wording or formatting
- **THEN** validation can reuse applicable prior behavioral evidence with an explicit justification and run only the checks required by the actual change; changed triggers, instructions, commands or resources are not classified as cosmetic

### Requirement: Isolated and attributable execution

Evaluation SHALL use owned test targets and fresh agent contexts for independent candidate and baseline executions, with matched inputs, provider/model, supported effort, common instructions, tools and relevant environment conditions. It SHALL verify the effective catalogue and package revisions used by each run, record execution order and relevant cache conditions, and prevent candidate instructions or prior solutions from leaking into the baseline. The candidate authoring conversation MUST NOT leak the solution into acceptance runs. Reports SHALL identify runtime, project and library/package revisions, test inputs, evaluator version and outcomes. Source inspection, model self-reports and discovery metadata alone MUST NOT count as proof of actual skill use or task success. Tests MUST NOT mutate real external production targets or the controlling session's services.

#### Scenario: Skill is visible but bypassed
- **WHEN** an intended-use case requires application of the candidate but the agent solves it without reading or applying the skill
- **THEN** selection/use acceptance fails or is explicitly inconclusive even if the final artifact is correct

#### Scenario: A removal or negative case avoids the skill
- **WHEN** a retirement variant or an out-of-scope request correctly runs without the skill
- **THEN** absence of invocation is expected behavior and its result and full cost are evaluated without misclassifying that absence as skill bypass

#### Scenario: Provider failure during comparison
- **WHEN** one side fails because its selected provider or quota is unavailable
- **THEN** the result is incomplete, no silent provider substitution occurs, and a comparison using an authorized reserve is rerun under matched conditions before attributing a difference to the skill

### Requirement: Benefit and regression decision

The decision SHALL be accept, reject or inconclusive. Accept SHALL require complete attributable evidence, all mandatory correctness and authority checks to pass, no unexplained regression in protected scenarios, demonstrated selection behavior, evidence for the declared benefit under the predeclared uncertainty policy, and admission within the applicable library and learning budgets. A demonstrated integrity or mandatory-behavior violation, or established regression against the declared claim, SHALL reject the candidate. Missing, stale, unavailable or statistically unresolved evidence SHALL be inconclusive and retain the active library. A rejected or inconclusive revision MUST NOT activate under a different name or weaker criterion.

The comparison policy SHALL distinguish a reproducible capability/correctness improvement from a claim about average savings or success probability. Statistical claims SHALL declare the task mix, meaningful effect, uncertainty method, sample or stopping plan and treatment of repeated candidate selection before acceptance; fixed-batch comparison is permitted and adaptive testing SHALL account for repeated looks. Failure to detect a difference MUST NOT establish equivalence or safe retirement. No mandatory requirement SHALL be relaxed to obtain a passing statistical result. Limited experiments SHALL NOT claim universal reliability, monotonic improvement or global optimality.

#### Scenario: Faster candidate skips a required check
- **WHEN** a candidate reduces execution time by omitting a mandatory validation step
- **THEN** it is rejected despite the speed improvement

#### Scenario: Broad description steals another skill's tasks
- **WHEN** a candidate improves its own examples but activates on protected requests belonging to an existing skill
- **THEN** coexistence acceptance fails and the prior active set is retained

#### Scenario: Small samples do not resolve a savings claim
- **WHEN** an observed cost reduction does not meet the predeclared evidence threshold before the budget ends
- **THEN** the outcome is inconclusive rather than accepted on the best run or retried until a favorable result appears

#### Scenario: A skill enables a required result at greater cost
- **WHEN** reproducible held-out evidence confirms a previously unmet required capability within its declared resource allowance
- **THEN** the candidate can satisfy that declared benefit while recording its additional cost; the result is not reported as resource savings

### Requirement: Full-cost measurement and net benefit

Evaluation SHALL measure accepted task outcomes, elapsed time, required user intervention and available model/tool consumption for both variants, including failed attempts, verification, correction and integration. It SHALL account for catalogue metadata on applicable and nonapplicable tasks, body/reference loading when selected, and work induced by the instructions. Creation, unsuccessful candidate revisions, evaluation, publication and maintenance SHALL be included in the change's learning cost without double counting. A savings claim SHALL state its horizon, task-frequency assumptions and expected net effect after this cost; measured, estimated and unknown quantities SHALL remain distinct. Existing logs SHALL support hypothesis selection, not fabricate the unexecuted alternative's outcome.

Model token categories, output bytes, provider quota windows and local resource measurements SHALL retain their units and attribution. Account-level quota changes SHALL account for resets, observation precision and unrelated concurrent consumption. Unattributable quota SHALL remain unknown; token or byte reductions SHALL NOT imply an exact subscription percentage or energy reduction. Unknown cost SHALL NOT be recorded as zero. A bounded pilot SHALL establish observable units and comparison feasibility before dependent automatic cost decisions or elaborate evaluation infrastructure.

#### Scenario: A long skill is rarely selected
- **WHEN** catalogue metadata is included but the skill body is not loaded on most representative tasks
- **THEN** the measurement distinguishes recurring catalogue cost from conditional body cost rather than charging the entire file on every task

#### Scenario: Evaluation costs exceed expected savings
- **WHEN** a candidate improves per-task cost but its supported use horizon does not offset creation and evaluation expense
- **THEN** it does not pass the declared net-savings claim merely because execution alone became cheaper

#### Scenario: Another task consumes the same subscription
- **WHEN** account-level allowance changes cannot be attributed to the compared runs
- **THEN** the report marks quota attribution unknown and limits any benefit conclusion to measurements that the comparison actually supports

### Requirement: Protected evaluator and evidence freshness

Acceptance criteria and their authoritative evidence SHALL remain outside the candidate's writable scope during evaluation. Candidate scripts MUST NOT alter expected outputs, evaluator code, baseline packages or historical results. Any change to the package, declared dependencies or baseline after evaluation SHALL invalidate the affected evidence before publication. Reusing unaffected checks SHALL be explicit; missing or corrupted evidence SHALL not be treated as success.

#### Scenario: Candidate changes a test expectation
- **WHEN** a trial modifies an oracle or attempts to rewrite its result record
- **THEN** the trial fails integrity checks and cannot authorize publication

#### Scenario: Resource changes after a passing run
- **WHEN** a referenced executable resource changes after the recorded candidate evaluation
- **THEN** the previous result cannot authorize publication of the changed package without applicable revalidation

### Requirement: Bounded learning cost and usable failure

Each nontrivial improvement episode SHALL have an explicit time/model-work budget and a stopping condition, selected for expected value and concrete risk. All episodes sharing a configured owner and accounting period SHALL also consume one aggregate learning budget, including authorship, both comparison variants, failures, maintenance and any model-backed judgment. A candidate, retry, rename, restart or new conversation SHALL NOT reset period expenditure or create extra allowance. Scheduling SHALL reserve measurable work before dispatch and retain unfinished reservations across interruption; limits lacking a reliable consumption bound SHALL be labeled estimates and supplemented by enforceable request/time limits. Unknown subscription telemetry SHALL NOT imply zero or unlimited remaining quota.

Routine no-signal tasks SHALL not incur a separate model-backed maintenance run. Exhausted episode or period budgets, repeated unchanged failures and unavailable evaluators SHALL stop new learning work while preserving useful partial evidence and the prior active skill. Another trial SHALL require changed relevant evidence or an unresolved decision that additional observations could justify within the remaining budget. Period exhaustion SHALL defer independent candidates without disabling ordinary task execution or silently increasing limits. Lifecycle hooks SHALL not initiate recursive learning or prevent ordinary task completion to force skill maintenance.

Ordinary diagnostic, context and Stop hooks remain disabled. Any remaining delivery path MUST NOT restore those hooks, create Stop continuations, or treat silence as successful diagnostics. The accepted RTK exception stays unrelated to skill learning.

#### Scenario: No new evidence after an unsuccessful attempt
- **WHEN** another refinement would repeat the same inputs and failed hypothesis
- **THEN** the workflow stops that episode with a concise pending or rejected status instead of continuing an unbounded self-improvement loop

#### Scenario: Many small episodes exhaust the period budget
- **WHEN** each candidate fits an episode budget but their combined expenditure and reservations reach the period allowance
- **THEN** no further learning call is admitted in that period, including after a restart or candidate rename, and normal task work remains available

#### Scenario: Remaining learning budget is insufficient for another pair
- **WHEN** the declared next baseline/candidate comparison cannot be reserved within the available allowance
- **THEN** the workflow preserves the existing evidence and active library without running only a favorable side or borrowing an undeclared budget

#### Scenario: Learning accounting is lost during a restart
- **WHEN** the controller cannot recover current-period expenditure or an interrupted reservation
- **THEN** it does not infer a fresh zero balance and admits no new learning work until the affected accounting is reconciled; ordinary task work remains available

#### Scenario: Learning hook failure
- **WHEN** metadata delivery times out during otherwise successful task work
- **THEN** the failure is reported as incomplete skill awareness and does not suppress existing diagnostics, terminate shared services or create repeated Stop continuation requests
- **AND** ordinary diagnostic/context/Stop hooks remain off; the incomplete awareness result does not authorize restoring them
