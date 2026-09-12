## RENAMED Requirements

- FROM: `### Requirement: Exact model assignment to named roles`
- TO: `### Requirement: Exact model and effort assignment`

## MODIFIED Requirements

### Requirement: Exact model and effort assignment

The kit SHALL provide native assignment parameters for an enabled provider/model and its supported reasoning effort, including Z.AI senior execution, Grok visual/general execution and Astra reserves, without a separate preset per level. Selection SHALL honor its explicit model and compatible reasoning while the lead normally remains on GPT. The supported heterogeneous mode SHALL deliver the assigned task and actual tool results. Each active conversation SHALL be simultaneously visible in its own window or pane. Autonomous recovery SHALL create a distinct visible task-level reassignment with a newly verified binding; an exact request SHALL NOT be silently relabeled or rerouted. Cross-provider continuation SHALL use sufficient visible task context and preserved artifacts, without assuming another provider can consume encrypted reasoning or compaction state.

#### Scenario: GPT delegates a review to Grok middle
- **WHEN** a GPT lead selects Grok and a supported effort for a review from outside the harness checkout
- **THEN** the child receives the task, uses the selected Grok model, exercises an appropriate local tool and returns its findings

#### Scenario: GPT delegates substantial implementation to Z.AI
- **WHEN** a GPT lead explicitly selects Z.AI and a supported effort for suitable text/code work from another project
- **THEN** the Z.AI subscription serves the exact assigned model and the executor performs actual tool work with evidence returned to the lead

#### Scenario: The role's model is unavailable
- **WHEN** an explicitly assigned model cannot serve a delegated request
- **THEN** the failure remains observable and any authorized fallback is a distinct, visible assignment to a capable model with preserved partial work

#### Scenario: A saved heterogeneous child needs recovery
- **WHEN** a restored task must continue a child whose provider binding cannot be verified
- **THEN** the runtime creates a fresh verified binding using the visible handoff and does not blindly resume it under an inherited model
