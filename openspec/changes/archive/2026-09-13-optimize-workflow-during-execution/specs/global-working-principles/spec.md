## ADDED Requirements

### Requirement: Sustained work adapts at informative execution boundaries

During sustained work, agents SHALL reconsider their method when available evidence shows repeated expensive preparation, successive different failures in one unresolved stage, prolonged waits without useful observations, or supporting work that no longer advances the accepted result. This applies while an operation is running as well as after its terminal result. Agents SHALL select the next distinguishing observation and explain how its possible outcomes affect the next action before paying for another consequential repeat. An unfamiliar shorter route SHALL NOT be treated as evidence that a full run is necessary. Existing recovery, startup, timing and integration conditions remain binding.

#### Scenario: Different errors conceal the same expensive bottleneck
- **WHEN** successive attempts fail at different controls or observations before reaching the product action
- **THEN** the next action investigates that shared unresolved stage through a supported smaller boundary, or establishes the concrete condition requiring a full run, without waiting for another user correction

#### Scenario: An operation is still restoring state
- **WHEN** the primary failure is already observable while required restoration continues
- **THEN** the agent uses available evidence or independent authorized work to resolve the next decision, and does not interrupt recovery or reuse the resource before its state is known

### Requirement: Routine workflow investment happens inside the accepted task

Agents SHALL apply bounded, reversible improvements to their execution method when expected savings on the remaining work justify implementation, checking, integration and recovery costs. They SHALL measure the relevant costs when uncertainty can change that choice, distinguishing measured time from estimates and overlapping elapsed time from summed worker effort. Routine in-scope changes of method SHALL NOT require a separate specification, user intervention, fixed reflection interval, retry count, report protocol or permanent optimization agent. Material scope, authority or operating-condition changes still require the user's decision. Faster intermediate actions SHALL NOT replace final accepted behavior or its mandatory checks.

#### Scenario: An existing observation route removes repeated preparation
- **WHEN** a supported small change can expose the needed state within the current authorized task at lower expected remaining cost
- **THEN** the agent applies and exercises it in the owning workflow now, then completes the parent acceptance

#### Scenario: Measuring everything would cost more than the task
- **WHEN** a cheap correction has a clear verification path and no material repeated cost
- **THEN** the agent completes it directly without mandatory instrumentation, delegation or planning overhead

#### Scenario: An apparent optimization weakens the result
- **WHEN** a proposed shortcut removes a required startup condition, restoration control or acceptance check
- **THEN** the agent preserves that requirement and selects another improvement or the necessary full path

### Requirement: Execution adaptation is evidenced separately from instruction delivery

Changes to this behavior SHALL verify current global delivery and independently inspect an evaluator's actual tool actions and resulting artifacts on a bounded external task. Counterexamples SHALL cover differently named failures in one stage, valid and invalidated prepared state, necessary full acceptance, a cheap task and unjustified delegation. Merely reciting the policy or passing text validation SHALL NOT establish behavior. Scenario-only decisions SHALL remain distinct from executed application behavior, and no general speedup or reliable adoption across all future tasks SHALL be claimed from limited checks.

#### Scenario: Correct prose accompanies an unchanged uninformative repeat
- **WHEN** the evaluator describes reuse but its next tool actions repeat expensive preparation without a discriminating condition
- **THEN** behavioral acceptance fails and the guidance or execution path is corrected before completion

#### Scenario: External use and bounded counterexamples are checked
- **WHEN** final linked instructions are loaded outside the source checkout, an external task produces an independently checked result, and the counterexamples preserve all required boundaries
- **THEN** the change reports the observed decisions, actual costs and coverage limits without converting synthetic decisions into live UI or universal efficiency evidence
