## MODIFIED Requirements

### Requirement: Evidence-based optional capability selection

Optional mixed-source or cross-project candidates SHALL receive bounded applicability evaluations with an independent expected answer. Selection SHALL include preparation, freshness, process resources and maintenance cost. A candidate SHALL be adopted only with demonstrated task benefit and a supported preservation/lifecycle path; otherwise the existing route SHALL be retained with an evidence-backed reason. Evaluation MUST NOT silently add model billing, downloads, automatic indexing or a permanent service.

#### Scenario: Applicability evaluation was not completed
- **WHEN** a candidate has neither an evidence-backed adoption nor a justified retention decision
- **THEN** its evaluation remains incomplete and conditional adoption does not permit closing the task without that decision

#### Scenario: Graph has no proven relationship to the task
- **WHEN** a graph is absent, stale or belongs to another project
- **THEN** the workflow uses current relevant sources and records the limitation without substituting an unrelated default graph

#### Scenario: Graph construction benefit is claimed
- **WHEN** the workflow claims that a generated mixed-source graph improves task completion
- **THEN** evidence includes actual construction and update from the fixture sources, correct answers and their total cost; a hand-authored graph establishes query behavior only

#### Scenario: Cross-project service adds cost without benefit
- **WHEN** the candidate requires additional processes and fails the predeclared usefulness or resource criteria
- **THEN** the existing verified route remains selected and any candidate-owned resources are cleaned up without disturbing other clients
