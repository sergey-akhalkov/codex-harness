## Purpose

Provide reproducible commit-bound Windows verification of the delivered harness while preserving private state, explicit model-use authority and the distinction between isolated checks and actual installed-consumer acceptance.

## ADDED Requirements

### Requirement: Deterministic Windows checks produce a commit-bound signal

Repository CI SHALL run the existing applicable formatting, warning-as-error lint, Rust tests, executable ownership and source/link hygiene checks on the supported Windows/MSVC target with an identified toolchain, locked dependencies and preserved command exit status. Results SHALL identify the evaluated revision, relevant inputs, executed check scope and skipped or unavailable checks. Deterministic regression cases from this audit SHALL enter the appropriate existing Rust targets. Filtering SHALL NOT omit required shared invariants or misrepresent an unrun check as passing. CI definition presence alone SHALL NOT count as a successful run.

#### Scenario: An audit regression is introduced
- **WHEN** a change loses an observation, corrupts baseline comparison or violates another covered deterministic invariant
- **THEN** the relevant CI check fails on that revision and the result names the failing check without manufacturing successful execution

#### Scenario: The runner lacks a required capability
- **WHEN** a required check cannot execute in its runner environment
- **THEN** its unavailable status and the required alternate execution route remain explicit and the overall required acceptance is not silently reduced

### Requirement: Lifecycle and model-backed checks retain their own authority

Integration and publication SHALL exercise actual native launcher, install, update, recovery, transport and affected managed-tool paths, including an installed consumer outside the source checkout where required by existing acceptance. Isolated deterministic fixtures SHALL NOT replace that evidence. Public automatic checks SHALL use owned synthetic state and SHALL NOT require production credentials, private transcripts or real model requests. Model-backed evaluation SHALL run only when explicitly selected with the authorized model, effort, provider, visibility and spending conditions. Existing CPU, heavy-command and runtime ownership contracts SHALL remain applicable.

#### Scenario: An isolated launcher test passes
- **WHEN** a synthetic forwarding fixture passes but the changed installed path has not been exercised
- **THEN** the fixture is reported as narrower evidence and required global-consumer acceptance remains open

#### Scenario: A pull request runs ordinary CI
- **WHEN** untrusted change content is tested automatically
- **THEN** it receives no production model credentials, does not change the owner's installation and starts no model-backed evaluation

#### Scenario: A model evaluation is deliberately selected
- **WHEN** an authorized operator selects the relevant model-backed case
- **THEN** actual assignment and consumption are observable, failures remain evidence and no substitute model or billing route is silently used

### Requirement: CI dependencies and public artifacts preserve integrity

CI SHALL use least necessary permissions, immutable verified action references and the existing dependency lifecycle. Public artifacts and diagnostics SHALL use synthetic inputs and exclude credentials, private identities, machine paths and transcript content. Result retention SHALL support investigation without publishing private raw evidence. Required remote repository settings or publication actions SHALL remain distinct from a local workflow-file change and require their applicable authority. A safe diagnostic failure SHALL preserve useful bounded evidence and a recovery route.

#### Scenario: A private-looking sentinel reaches a diagnostic
- **WHEN** an owned synthetic failure includes a marker representing private path or credential-shaped data
- **THEN** public output omits the protected value while retaining the error category and check identity needed to investigate

#### Scenario: Workflow files exist but no run has completed
- **WHEN** local workflow validation passes without an observed CI execution
- **THEN** the status reports configuration validation only and commit-bound execution acceptance stays unverified
