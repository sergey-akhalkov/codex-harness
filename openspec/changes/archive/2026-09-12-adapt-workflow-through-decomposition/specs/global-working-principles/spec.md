## ADDED Requirements

### Requirement: Workflow reassessment responds to the cost of learning

Main and delegated agents SHALL assess the next useful result, unresolved dependencies and cost of obtaining feedback when planning substantive work. They SHALL reassess the approach before consequential expensive repetition, after an unexpected failure, or when increasing scope, coupled fixes or recurring difficulty makes progress inefficient. Reassessment SHALL consider recurring classes of difficulty even when each attempt produces a different error or a new fact. Agents SHALL choose a smaller independently verifiable problem, optimize a justified repeated operation, or continue the existing path when current evidence supports it. They SHALL communicate material changes of approach concisely without requiring a fixed retry count, timer, per-tool report, additional approval or separate reflection agent.

#### Scenario: Different failures expose the same unlearned layer
- **WHEN** successive expensive full runs reveal different navigation, input or observation failures before reaching the product behavior being tested
- **THEN** the agent identifies the common unlearned layer and isolates its investigation before another dependent full run instead of treating each new error as sufficient justification to repeat the whole path

#### Scenario: The first expensive failure exposes several unknown prerequisites
- **WHEN** one attempt establishes that the remaining path depends on several unverified interactions that can be investigated together
- **THEN** the agent can extract that investigation immediately without waiting for a prescribed number of failures

#### Scenario: Entangled corrections make a large task hard to verify
- **WHEN** fixes across several concerns repeatedly invalidate one another or make failure attribution unclear
- **THEN** the agent separates the relevant hypotheses and dependencies into smaller verifiable results while preserving the accepted end-to-end outcome

#### Scenario: A long operation is still the appropriate next check
- **WHEN** the unresolved behavior requires a full integration run and no smaller check preserves its relevant conditions
- **THEN** the agent runs the required operation with its useful observation and limits identified rather than inventing a decomposition or skipping acceptance

### Requirement: Decomposition produces independently verifiable results

Agents SHALL prefer reducing an uncertain or costly problem into a smaller independently verifiable result before optimizing execution of the larger cycle. A workstream SHALL have a concrete result, relevant inputs and dependencies, a meaningful check, and an identified consumer in the parent task. Its reduced scenario SHALL preserve the original failure or question and necessary timing, state and integration conditions. Agents SHALL distinguish product defects, test-driving defects, preparation failures and inconclusive observations before selecting the next check. Decomposition SHALL retain the full accepted behavior and mandatory checks, and SHALL remain proportionate for small or tightly coupled work.

#### Scenario: The test driver fails before the product is exercised
- **WHEN** an automation or setup failure prevents the intended product action from occurring
- **THEN** the agent verifies the driver or preparation in a smaller owned scenario, identifies the product result as untested, and returns to product acceptance after the prerequisite is usable

#### Scenario: A reduced test loses the original defect
- **WHEN** a smaller reproduction passes only because it removed the triggering state, integration boundary or timing condition
- **THEN** the agent rejects that reduction as evidence for the original problem and retains or restores the last meaningful reproduction

#### Scenario: A routine task already has a short reliable check
- **WHEN** the next correction is bounded, understood and inexpensive to verify directly
- **THEN** the agent completes it without a mandatory workstream, helper, worker or new planning document

### Requirement: Repeated work is optimized within valid ownership and state

Agents SHALL evaluate reuse of expensive preparation, active sessions, verified intermediate results and deterministic automation against expected remaining repetitions and total completion cost. They SHALL reuse results only while the relevant inputs and conditions remain valid, invalidate affected evidence after changes, and automate demonstrated recurring mechanics in their existing owner. Exploration SHALL cover the actions needed by the accepted scenario, including relevant return and recovery paths, without requiring mastery of an entire interface. Agents SHALL preserve ownership, required restoration and the distinction between exploratory observations and accepted product evidence. Reusable knowledge SHALL be retained concisely in its owning project record with validity conditions and without private consumer data in shared sources.

#### Scenario: Several interactions can be learned in one prepared session
- **WHEN** preparing a test application is expensive and the needed interactions can be explored within one valid owned session
- **THEN** the agent reuses that session, verifies the effects and return paths, and produces a replayable result for the parent workflow without repeatedly preparing the whole environment for each unknown action

#### Scenario: A relevant input changes or a session ends
- **WHEN** a build, configuration, test input or runtime condition changes in a way that can invalidate retained state or observations
- **THEN** the agent re-establishes the affected preconditions and reruns the affected checks rather than treating the previous session or historical pass as current evidence

#### Scenario: Automation would cost more than the remaining task
- **WHEN** creating a general helper would add more implementation, integration and verification work than the demonstrated repetitions justify
- **THEN** the agent chooses a bounded direct or reusable existing path and preserves the required behavior without speculative infrastructure

#### Scenario: Work resumes after an interruption
- **WHEN** an agent returns to unfinished work with a partial result and recorded conditions
- **THEN** it checks those conditions, reuses still-valid knowledge and continues from the verified boundary rather than restarting discovery or assuming an old runtime still exists

### Requirement: Adaptive workflow delivery is verified through actual use

The updated workflow SHALL be delivered through the existing supported installation lifecycle and checked in a fresh ordinary consumer outside the source checkout. Acceptance SHALL distinguish instruction loading, observed agent decisions, integrated task correctness and measured efficiency. It SHALL exercise a costly repeated-work case, proportionate direct work, a necessary full check, resource ownership and invalidated reuse. At least one bounded real external development task SHALL demonstrate an integrated result and useful reduction in repeated work or feedback delay, with a comparable baseline and complete elapsed cost recorded. Preparation, delegation, waiting, verification, restoration and rework SHALL be included; overlapping execution SHALL NOT be double counted. Missing evidence, regressions and inconclusive comparisons SHALL remain explicit and SHALL NOT be converted into completion or unsupported general speed claims.

#### Scenario: A fresh session loads the policy but has not used it
- **WHEN** a fresh external session can see the updated instructions
- **THEN** loading is reported as verified while actual adaptive behavior and efficiency remain unverified until exercised

#### Scenario: A reusable mechanism succeeds only in its isolated test
- **WHEN** a workstream passes its local check but the parent task does not yet consume its result
- **THEN** implementation acceptance remains unfinished and the agent continues integration and required parent checks

#### Scenario: An optimization improves an intermediate metric but delays completion
- **WHEN** a method reduces full-run count but its preparation and integration introduce an unexplained increase in complete task time
- **THEN** the agent reports the trade-off, corrects unjustified overhead and does not claim the method is a verified overall speed improvement

#### Scenario: Repeated work has a measured useful improvement
- **WHEN** a comparable real task consumes the optimized result, retains its acceptance criteria and shows useful improvement with full costs included
- **THEN** the agent reports the measured scope and operating conditions without extrapolating to universal speed or subscription savings
