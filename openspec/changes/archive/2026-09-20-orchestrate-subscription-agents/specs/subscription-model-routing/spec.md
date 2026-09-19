## RENAMED Requirements

- FROM: `### Requirement: Exact model assignment to named roles`
- TO: `### Requirement: Exact model and effort assignment`

## MODIFIED Requirements

### Requirement: Exact model and effort assignment

The kit SHALL provide native assignment parameters for an enabled provider profile and its supported reasoning effort, with lead and executor bindings selected through kit orchestration configuration while explicit user profile selection retains its native precedence. The supported heterogeneous mode SHALL deliver the assigned task and actual tool results through the configured profiles. Each active conversation SHALL be simultaneously visible in its own window or pane with its effective identity. Autonomous recovery SHALL create a distinct visible task-level reassignment with a newly verified binding; an exact request SHALL NOT be silently relabeled or rerouted. Cross-provider continuation SHALL use sufficient visible task context and preserved artifacts, without assuming another provider can consume encrypted reasoning or compaction state.

#### Scenario: GPT delegates a review to Grok middle
- **WHEN** the lead assigns a review to the configured Grok executor profile from outside the harness checkout
- **THEN** the child receives the task, uses the selected profile's model, exercises an appropriate local tool and returns its findings

#### Scenario: The role's model is unavailable
- **WHEN** an explicitly assigned model cannot serve a delegated request
- **THEN** the failure remains observable and any authorized fallback is a distinct, visible assignment to a capable configured profile with preserved partial work

#### Scenario: A saved heterogeneous child needs recovery
- **WHEN** a restored task must continue a child whose provider binding cannot be verified
- **THEN** the runtime creates a fresh verified binding using the visible handoff and does not blindly resume it under an inherited model
