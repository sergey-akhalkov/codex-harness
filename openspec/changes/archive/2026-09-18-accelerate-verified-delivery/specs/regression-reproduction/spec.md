## Purpose

Provide a focused, reusable method for reproducing and reducing CLI, MCP and subprocess regressions without losing the original failure or affecting unrelated processes.

## ADDED Requirements

### Requirement: Establish a controlled regression case

The kit SHALL provide a globally discoverable `reproduce-regression` skill for CLI, MCP and subprocess failures. Each reproduction MUST identify the original trigger, relevant versions and environment, expected behavior, observed failure and the entry point exercised. A reference implementation or known-good version SHALL be used when available and applicable; its absence MUST be explicit and replaced by a stated specification or invariant rather than an invented oracle.

#### Scenario: A known-good reference is available

- **WHEN** the regression can be compared against a known-good version under a shared behavior contract
- **THEN** the reproduction uses controlled inputs and records the baseline/candidate identities and their different outcomes

#### Scenario: No independent implementation exists

- **WHEN** only the failing implementation is available
- **THEN** the case states the expected contract and its source, identifies the missing reference and limits its conclusions accordingly

### Requirement: Reduce without changing the failure

The workflow MUST verify each accepted reduction against the original failure condition and preserve the last verified reproducer. Version drift, changed timing conditions, assertion changes or a different error message alone MUST NOT count as a successful reduction. If reproduction remains intermittent, the evidence SHALL include attempts and outcomes rather than label the case deterministic.

#### Scenario: A smaller input fails for another reason

- **WHEN** a proposed reduction produces a different failure cause
- **THEN** it is rejected as evidence for the original regression and the last verified reproducer remains available

#### Scenario: Only some attempts reproduce the failure

- **WHEN** equivalent attempts produce mixed outcomes
- **THEN** the workflow reports the observed frequency and unresolved conditions without hiding successful or failed attempts

### Requirement: Bound and isolate process fixtures

Reusable fixtures used by the workflow MUST isolate owned files, ports and processes, capture stdout and stderr without mutual blocking, use observable readiness and bound execution and cleanup. Cleanup MUST target only resources owned by the case. Natural termination, timeout, forced termination, infrastructure failure and assertion failure MUST remain distinguishable. Fixture creation SHALL reuse available native test capabilities and MUST have an actual consuming regression case.

#### Scenario: Both output streams exceed their pipe buffers

- **WHEN** a test child writes substantial stdout and stderr concurrently
- **THEN** both streams are drained without a capture deadlock and output limits are reported if reached

#### Scenario: A child never becomes ready

- **WHEN** the expected readiness signal does not arrive within the case's declared limit
- **THEN** the case reports a readiness failure, preserves diagnostic output and cleans up its owned resources
- **AND** unrelated user processes and live services remain untouched

#### Scenario: The harness terminates a hung child

- **WHEN** the case reaches its execution deadline and cleanup terminates the child
- **THEN** the result remains a timeout or forced termination rather than a successful natural exit

### Requirement: Keep reproduction focused and reusable

The skill SHALL provide a small reusable core with optional detailed references for applicable failure types. It MUST avoid mandatory broad fuzzing, simulation, model delegation or a new orchestration framework. The delivered resources SHALL support a real consuming CLI, MCP or subprocess regression outside the harness checkout and include an executable regression check or a documented, reproducible blocked result.

#### Scenario: A bounded subprocess regression needs no additional framework

- **WHEN** the project's existing test runtime can express the reproduction and relevant failure conditions
- **THEN** the workflow uses that runtime and adds only the fixture resources needed by the actual case

#### Scenario: A test cannot reproduce the original failure

- **WHEN** the available evidence is insufficient to reproduce the regression
- **THEN** the skill preserves the attempted inputs and blocker and does not claim a delivered regression fix or a successful benefit-evaluation case
