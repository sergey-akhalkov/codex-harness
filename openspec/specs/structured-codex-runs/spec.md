# structured-codex-runs Specification

## Purpose

Получать машинно проверяемый итог ограниченных задач Codex и принимать результат по фактическому завершению и проверкам, сохраняя выбранные подписки и модели.

## Requirements

### Requirement: Explicit bounded execution contract

The global workflow SHALL use native `codex exec --output-schema` for suitable repeated or machine-consumed bounded tasks. Every run SHALL identify its working directory, relevant input revision and dirty inputs, model/provider route, allowed effects, schema, independent acceptance checks and time/output limits. It SHALL reuse existing process-observation facilities where applicable and MUST NOT become mandatory for ordinary interactive work.

#### Scenario: Read-only repository inspection
- **WHEN** a structured run is assigned to inspect an owned repository fixture
- **THEN** it uses explicitly read-only effects despite inherited Full Access defaults, emits the contracted result, and leaves the source unchanged

### Requirement: Output validity is distinct from task success

A run SHALL be accepted only when it terminates naturally with a successful process and task outcome, produces a fresh final output conforming to its schema, and passes the task's independent acceptance checks. Event JSONL and final-result JSON SHALL remain distinct. The result SHALL report findings, evidence and unresolved issues without treating self-reported success as an oracle.

#### Scenario: Valid JSON describes a wrong result
- **WHEN** a schema-valid final response contradicts the fixture's independently known answer
- **THEN** the run is rejected as a task failure even if the CLI exited zero

#### Scenario: Timeout leaves a plausible output file
- **WHEN** execution times out or is forcibly terminated after writing output
- **THEN** the run remains timed out or terminated and the output is retained only as partial evidence

#### Scenario: Authentication, malformed output or stale artifact
- **WHEN** a run fails authentication, emits missing or malformed final JSON, or has only a previous run's output
- **THEN** the failure is surfaced distinctly and no earlier artifact or empty result is accepted as this run's success

### Requirement: Deliberate model routing

OpenAI assignments SHALL use the Astra family only. The workflow SHALL preserve Grok as the preferred middle for suitable delegated work, but MUST NOT assume a route supports native schema enforcement without evidence. An unsupported route SHALL be reported; reassignment SHALL follow the established capability policy with visible model and subscription identity and no silent billing fallback.

#### Scenario: External route does not honor the output contract
- **WHEN** the chosen external route rejects or ignores the schema contract
- **THEN** the workflow reports that incompatibility and does not claim structured-output support based solely on parseable text

### Requirement: Installed consumer and lifecycle

Verification SHALL exercise a real schema-constrained native run from outside the harness, verify an independent fixture oracle, and cover process and output failure cases using bounded deterministic fixtures where model calls add no evidence. Installation updates and disconnection SHALL preserve unrelated registrations, project sources and user output.

#### Scenario: Consumer uses the global skill and linked assets
- **WHEN** Codex starts in an independent repository after installation
- **THEN** it discovers the workflow and resolves its schema and helper assets through the installed kit without development-machine paths or repository-local-only setup
