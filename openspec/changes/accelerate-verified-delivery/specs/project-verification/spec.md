## Purpose

Make project-native verification reusable across Codex sessions while preserving command provenance, actual execution evidence and the limits of each check.

## ADDED Requirements

### Requirement: Discover a bounded project verification path

The kit SHALL provide a globally discoverable `project-verification` skill for choosing and executing checks for a project change. The skill MUST honor applicable project instructions, inspect relevant native manifests, CI and existing validation documentation, and select the smallest useful first signal followed by all checks required for the agreed outcome. It MUST load detailed guidance only when relevant and MUST NOT require an expensive verification workflow for unrelated or documentation-only work.

#### Scenario: A project already owns validation commands

- **WHEN** a behavior change has documented project-native validation commands
- **THEN** the skill reuses those commands with their required working directory, preparation and scope instead of creating a competing validation catalogue
- **AND** required broader acceptance checks remain part of completion

#### Scenario: A trivial documentation edit does not require a model-backed test run

- **WHEN** the task only corrects documentation and has no executable behavior change
- **THEN** applicable documentation checks suffice and the skill does not trigger application tests, delegated agents or model-backed outcome evaluation merely because it is installed

### Requirement: Preserve command provenance and freshness

The workflow MUST distinguish `confirmed`, `docs-only`, `unknown` and `blocked` command knowledge. A confirmed record SHALL identify the command, working directory, relevant runtime/build identity, provenance, last successful execution and evidence scope. Relevant changes to commands, dependencies, build inputs or the executable MUST invalidate assumptions derived from earlier execution. Historical success MUST NOT be reported as verification of the current change. Durable command knowledge SHALL use the consuming project's existing documentation home where available; sensitive runtime output SHALL remain outside portable instructions.

#### Scenario: A documented command has not been executed

- **WHEN** the only available evidence is a manifest, README or CI definition
- **THEN** the command is labeled docs-only and no passing execution is claimed

#### Scenario: The runtime or build changed after confirmation

- **WHEN** relevant inputs differ from the last confirmed execution
- **THEN** the workflow rechecks preparation and entry-point identity before relying on the command
- **AND** retains the historical evidence without presenting it as current success

#### Scenario: Access or prerequisites are missing

- **WHEN** the required check cannot run with available access or prerequisites
- **THEN** the result identifies the blocker and any substitute evidence with its limits
- **AND** does not count an unexecuted check as passed

### Requirement: Exercise the actual changed behavior

Verification MUST identify the actual source or executable exercised, including required generated or bundled assets, and test representative behavior through its real entry point. Regression verification SHALL demonstrate the original failure on a baseline when available and the intended behavior on the candidate. Meaningful failure paths SHALL follow the specification and concrete risk. The workflow MUST NOT weaken expected behavior or substitute a different binary solely to make a check pass.

#### Scenario: A stale generated asset would hide the change

- **WHEN** the executable depends on generated or bundled assets that differ from edited source
- **THEN** verification rebuilds or selects the intended artifact and records its identity before claiming that the changed behavior was exercised

#### Scenario: A focused check passes but required integration fails

- **WHEN** the focused check passes and an applicable acceptance check fails
- **THEN** the task remains incomplete and the report preserves both results and the original failure

### Requirement: Report proportionate verification evidence

The workflow SHALL return a concise account of the behavior checked, commands and execution identity, outcomes, material failure evidence and remaining limitations. Detailed logs MAY be referenced rather than inserted into agent context. It MUST preserve incomplete and failed results through retries or handoff, and MUST NOT infer full product correctness from a lint check or isolated fixture.

#### Scenario: Only a lint check is available

- **WHEN** a real consuming project provides a runnable lint command but no exercised application acceptance path
- **THEN** the report claims the lint result only and explicitly leaves application behavior unverified

#### Scenario: Verification resumes after interruption

- **WHEN** another session continues the task from a saved evidence record
- **THEN** it can identify the last tested revision and unresolved checks without interpreting a previous partial pass as task completion
