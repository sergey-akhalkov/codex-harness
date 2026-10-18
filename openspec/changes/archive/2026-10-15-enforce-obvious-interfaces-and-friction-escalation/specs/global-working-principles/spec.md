# Spec Delta

## ADDED Requirements

### Requirement: All interfaces follow don't make me think

The principles SHALL make "don't make me think" mandatory for UI/UX, CLI, APIs, interfaces, methods, functions, libraries, configuration, tools, instructions and agent workflows. Design SHALL minimize avoidable decisions, remembered mechanics and knowledge of internals for human and programmatic consumers. Required correctness, integrity, explicit effects, control and acceptance SHALL remain intact; an agent's ability to overcome difficulty SHALL NOT establish usability.

#### Scenario: A programmatic consumer performs an ordinary operation
- **WHEN** a supported task uses an API, library, method or function
- **THEN** names, inputs, results, errors and lifecycle make the intended operation understandable without reconstructing undocumented call order or implementation details

#### Scenario: Related options have invalid combinations
- **WHEN** a caller must repeatedly coordinate related options or flags to express one ordinary intent
- **THEN** the design provides a coherent contract for that intent, makes invalid combinations difficult to express or rejects them clearly before effects, and retains justified advanced control without imposing it on the ordinary path

#### Scenario: Defaults disagree with the supported environment
- **WHEN** an interface's default selects an obsolete or unauthorized environment while current ordinary work requires another
- **THEN** the agent identifies the mismatch as a design defect and proposes a supported explicit selection or an unambiguous authorized default; it does not silently choose a different target or credit a refusal as a usable normal path

#### Scenario: Instructions themselves become an interface burden
- **WHEN** applying a rule requires repeated reconstruction from overlapping instructions
- **THEN** the agent consolidates the owned contract and its entry point within authorized scope, preserves normative requirements and protected external instructions, and avoids adding another overlapping manual

### Requirement: Ordinary workflows own routine mechanics

An interface SHALL own routine preparation, coordination, validation and completion that it can reliably perform within its authority at proportionate lifecycle cost. Consumers SHALL supply meaningful intent and necessary choices rather than repeatedly reconstructing derivable inputs or orchestration. Automation SHALL preserve observable state, errors, cancellation and recovery. Command count alone SHALL NOT define simplicity.

#### Scenario: One command conceals manual preparation
- **WHEN** a command requires callers to repeatedly prepare interdependent files, copy transient identifiers or coordinate helper processes before invoking it
- **THEN** usability assessment includes that preparation and recommends moving reliable routine mechanics into their existing owner instead of declaring the command simple because it has one entry point

#### Scenario: Required identity can be discovered
- **WHEN** the selected environment needs a fresh process identity, component version or derived path
- **THEN** the interface obtains and validates that fact against the selected authorized configuration where feasible, refuses ambiguous or incompatible state, and does not ask the caller to shuttle the same fact between stages or treat discovery as authorization

#### Scenario: Components exist but the supported path is incomplete
- **WHEN** individual operations exist but the normal entry point cannot perform the supported scenario and directs callers to assemble lower-level operations
- **THEN** the agent reports the integrated path as incomplete, identifies the missing composition and recommends completing its existing owner while preserving the working components and checks

#### Scenario: A manual condition is proportionate
- **WHEN** a bounded manual step is cheaper over actual use than automation and preserves the accepted outcome
- **THEN** the agent assesses repetition, recovery and user burden under the existing everyday-use contract, discloses the condition and obtains a decision before dependent work if it materially affects ordinary use; zero configuration and universal automation are not required

### Requirement: Interface acceptance exercises the everyday lifecycle

Acceptance for interface work SHALL cover the intended ordinary path, relevant first use, repeat use, interruption and recovery through the actual consumer boundary. Checks SHALL expose avoidable decisions, prerequisite handoffs and error discovery, alongside required correctness. Advanced controls SHALL remain available where needed. Static review and simulated decisions SHALL remain distinct from observed use; a successful isolated component SHALL NOT prove the integrated path.

#### Scenario: A repeat operation should reuse valid setup
- **WHEN** the normal task is repeated with unchanged valid configuration
- **THEN** the interface reuses appropriate setup and requires only meaningful changed inputs while retaining fresh checks for facts whose validity can change

#### Scenario: A prerequisite is missing
- **WHEN** an ordinary invocation lacks a prerequisite discoverable before consequential effects
- **THEN** the interface checks it early, supplies it when authorized and reliable, or identifies the missing fact and a supported next action without requiring an expensive downstream failure to reveal it

#### Scenario: An operation fails after possible effects
- **WHEN** execution is interrupted or its outcome is uncertain
- **THEN** the interface distinguishes completed work, failure and recovery state, preserves required recovery inputs and authority, and explains the safe next action without presenting partial completion or a blind retry as success

#### Scenario: Required complexity cannot be removed
- **WHEN** an explicit choice, full integration check or recovery step is necessary for the promised result
- **THEN** it remains present and understandable, and acceptance rejects a shorter route that hides effects, drops checks or transfers an unaccepted recurring burden to the consumer

### Requirement: Material friction produces timely improvement feedback

Agents SHALL promptly surface newly evidenced material friction and concrete opportunities to improve speed, clarity, convenience or reliability, even when work can continue. Feedback SHALL state the observed burden, consequence, recommended alternative and uncertainty. Bounded worthwhile corrections within scope SHALL be performed and checked without renewed permission. Reports SHALL arrive while they can change the work and SHALL NOT become repetitive noise or a suggestion quota.

#### Scenario: A workaround succeeds but recurs
- **WHEN** the agent repeatedly reconstructs arguments, coordinates preparation or recovery, or re-derives context that the owning interface could reliably manage
- **THEN** it identifies the recurring mechanics as an interface or tooling defect and proposes the concrete owning improvement without waiting for total blockage, task completion or a user complaint

#### Scenario: A small correction is already authorized
- **WHEN** a reversible bounded improvement fits the accepted task and its expected benefit justifies implementation and verification
- **THEN** the agent makes and checks it, explains the effect briefly and completes the parent task without creating an approval ceremony or a separate project

#### Scenario: A useful systemic improvement is outside current scope
- **WHEN** observed friction warrants a larger refactor but the accepted task does not authorize it
- **THEN** the agent reports the problem and recommendation promptly, explains the cost or risk of the available workaround, and requests a scope decision only if needed for dependent work; a proposal does not authorize implementation

#### Scenario: A known issue has already been discussed
- **WHEN** no new evidence changes the consequence or agreed response to a reported issue
- **THEN** the agent follows the decision and avoids repeated suggestions, status-only objections, timers or manufactured feedback; useful deferred work remains in its existing owner with its trigger

### Requirement: Material workflow defects trigger actionable escalation

When continuing a defective workflow would materially compound waste or unreliability, agents SHALL stop dependent repetition and escalate the evidenced cause, consequence, recommended remedy, feasible interim route and needed decision or capability. They SHALL preserve partial work, complete required recovery and continue independent authorized work. Escalation SHALL NOT grant refactoring authority, relax acceptance or override an informed user choice except for serious harm.

#### Scenario: Preparation repeatedly prevents the intended experiment
- **WHEN** costly attempts keep failing in setup or observation before reaching the intended product behavior
- **THEN** the agent identifies the shared defective boundary, stops another unsupported full attempt and either applies an authorized bounded correction or presents the concrete refactor or capability decision before resuming dependent work

#### Scenario: The only supported environment lacks a complete launcher
- **WHEN** the common entry point rejects the only currently supported environment and recurring work would require manual assembly of its lifecycle
- **THEN** the agent explicitly escalates the incomplete ordinary path and its recurring burden, proposes completing the supported composition, and does not silently substitute a legacy environment or an unreported manual procedure

#### Scenario: Recovery is still active when escalation occurs
- **WHEN** a material workflow defect is found while cleanup or restoration owns shared state
- **THEN** the agent reports the original problem without waiting to hide it behind final cleanup, preserves the recovery owner and performs no conflicting operation while pursuing independent useful work

#### Scenario: The user selects the interim route
- **WHEN** the user understands the consequence and chooses the original or interim workflow
- **THEN** the agent implements that decision faithfully within authority and unchanged acceptance, reports its actual limits and does not reopen the same objection without new evidence, except where serious harm requires the existing dissent rule

#### Scenario: The task is exploration only
- **WHEN** analysis reveals a material interface defect during a read-only or planning task
- **THEN** the agent reports and proposes the remedy within that boundary without modifying implementation, the external workflow or persistent settings merely to avoid the defect
