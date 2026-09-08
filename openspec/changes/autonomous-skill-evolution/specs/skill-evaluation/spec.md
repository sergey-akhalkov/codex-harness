## Purpose

Decide whether a skill change improves actual agent work using reproducible comparisons, protected acceptance conditions and proportionate resource use before activation.

## ADDED Requirements

### Requirement: Behavioral comparison with independent acceptance

New skills and behavior-changing updates SHALL have explicit success criteria established before the acceptance run. New skills SHALL be compared with the same task without the skill; updates SHALL be compared with the previous accepted version. Acceptance SHALL include at least one representative intended use, a similar request where the skill must not activate and a relevant boundary or failure case. Cases used for final acceptance SHALL include a related case not used to refine the candidate. Outcome checks SHALL use deterministic project or artifact evidence where available; subjective assessment SHALL use a fixed rubric and distinguish judgment from execution evidence.

#### Scenario: New skill passes its example but fails transfer
- **WHEN** a candidate succeeds on the example used to write it but fails the independently held acceptance case
- **THEN** it is not activated as an improved skill and the failed transfer remains visible in the result

#### Scenario: A narrowly scoped correction
- **WHEN** an edit changes only non-behavioral wording or formatting
- **THEN** validation can reuse applicable prior behavioral evidence with an explicit justification and run only the checks required by the actual change; changed triggers, instructions, commands or resources are not classified as cosmetic

### Requirement: Isolated and attributable execution

Evaluation SHALL use owned test targets and fresh agent contexts for independent candidate and baseline executions, with matched inputs, model settings and relevant environment conditions. The candidate authoring conversation MUST NOT leak the solution into acceptance runs. Reports SHALL identify model/provider, runtime, project and package revisions, test inputs, evaluator version and outcomes. Source inspection, model self-reports and discovery metadata alone MUST NOT count as proof of actual skill use or task success. Tests MUST NOT mutate real external production targets or the controlling session's services.

#### Scenario: Skill is visible but bypassed
- **WHEN** the candidate appears in discovery and the agent solves the task without reading or applying it
- **THEN** selection/use acceptance fails or is explicitly inconclusive even if the final artifact is correct

#### Scenario: Provider failure during comparison
- **WHEN** one side fails because its selected provider or quota is unavailable
- **THEN** the result is incomplete, no silent provider substitution occurs, and a comparison using an authorized reserve is rerun under matched conditions before attributing a difference to the skill

### Requirement: Benefit and regression decision

Acceptance SHALL require all mandatory correctness and authority invariants to pass, no unexplained regression in protected existing scenarios, and evidence for the candidate's stated benefit. It SHALL assess both isolated behavior and relevant overlapping skills. The comparison method SHALL account for model variability with a declared repeat or uncertainty policy proportional to the claim. Inconclusive improvements SHALL remain candidates. Time, model/tool usage and maintenance/evaluation overhead SHALL be reported with their measured scope; skill count and token counts MUST NOT be equated with quality or exact subscription savings.

#### Scenario: Faster candidate skips a required check
- **WHEN** a candidate reduces execution time by omitting a mandatory validation step
- **THEN** it is rejected despite the speed improvement

#### Scenario: Broad description steals another skill's tasks
- **WHEN** a candidate improves its own examples but activates on protected requests belonging to an existing skill
- **THEN** coexistence acceptance fails and the prior active set is retained

### Requirement: Protected evaluator and evidence freshness

Acceptance criteria and their authoritative evidence SHALL remain outside the candidate's writable scope during evaluation. Candidate scripts MUST NOT alter expected outputs, evaluator code, baseline packages or historical results. Any change to the package, declared dependencies or baseline after evaluation SHALL invalidate the affected evidence before publication. Reusing unaffected checks SHALL be explicit; missing or corrupted evidence SHALL not be treated as success.

#### Scenario: Candidate changes a test expectation
- **WHEN** a trial modifies an oracle or attempts to rewrite its result record
- **THEN** the trial fails integrity checks and cannot authorize publication

#### Scenario: Resource changes after a passing run
- **WHEN** a referenced executable resource changes after the recorded candidate evaluation
- **THEN** the previous result cannot authorize publication of the changed package without applicable revalidation

### Requirement: Bounded learning cost and usable failure

Each nontrivial improvement episode SHALL have an explicit time/model-work budget and a stopping condition, selected for expected value and concrete risk. Routine no-signal tasks SHALL not incur a separate model-backed maintenance run. Exhausted budgets, repeated unchanged failures and unavailable evaluators SHALL stop the episode while preserving useful partial evidence and the prior active skill. Lifecycle hooks SHALL not initiate recursive learning or prevent ordinary task completion to force skill maintenance.

#### Scenario: No new evidence after an unsuccessful attempt
- **WHEN** another refinement would repeat the same inputs and failed hypothesis
- **THEN** the workflow stops that episode with a concise pending or rejected status instead of continuing an unbounded self-improvement loop

#### Scenario: Learning hook failure
- **WHEN** metadata delivery times out during otherwise successful task work
- **THEN** the failure is reported as incomplete skill awareness and does not suppress existing diagnostics, terminate shared services or create repeated Stop continuation requests
