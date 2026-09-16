## MODIFIED Requirements

### Requirement: Evidence of efficiency and quality

Acceptance SHALL exercise the global policy on representative implementation, review, parallel, fallback, escalation, feedback triage and promotion, instruction-refresh succession and direct-work scenarios. A reproducible comparison with direct execution SHALL record total elapsed time, parent and descendant token usage by provider, verification outcomes, feedback-triage cost and rework for matched accepted tasks, both before and after a promoted improvement becomes a default. Repeated cumulative usage records SHALL NOT be double counted. Missing telemetry or concurrent account activity SHALL be disclosed. A feedback-driven improvement SHALL NOT become a default without unchanged-or-better quality and no material delivery-time regression beyond the declared tolerance; token differences SHALL NOT be presented as exact weekly quota savings. Findings that show overhead or quality failures SHALL drive correction before completion.

#### Scenario: A comparative run finishes
- **WHEN** matched direct and delegated tasks have completed their acceptance checks
- **THEN** the report includes coordination and worker usage, both providers, elapsed time and quality evidence, and states whether each measured outcome improved or regressed

#### Scenario: A promoted improvement would burn more tokens
- **WHEN** a promoted change improves quality claims but increases attributable token use or delivery time beyond tolerance
- **THEN** it is not adopted as a default until the regression is corrected or explicitly accepted by the user
