## ADDED Requirements

### Requirement: Semantic readiness is established through applicable operations

Managed code-tool health SHALL distinguish registration, process startup, project activation, backend initialization, usable semantic operations and current complete diagnostics. Activation or an empty diagnostic response SHALL NOT alone establish the later states. A failed backend SHALL produce a bounded cause-bearing result and evidence locator identifying the affected operation and coverage. Recovery SHALL respond to the observed cause, retain existing bounded retry behavior and avoid repeatedly restarting an unchanged fatal configuration. The kit SHALL restore the accepted useful source operations in the affected consumer or keep the failure explicitly open; it MUST NOT silently remove required language coverage, change resource policy or report degraded tools as fully delivered.

#### Scenario: Activation succeeds and the first symbol query fails
- **WHEN** project activation returns successfully but backend initialization subsequently fails
- **THEN** health reports semantic operations unavailable, retains the initialization failure and does not claim a working language from the activation response

#### Scenario: One backend exhausts resources
- **WHEN** a multi-language worker reports memory exhaustion or a failed native analysis process
- **THEN** diagnosis distinguishes observed failure from an unproven root cause, checks relevant coverage and resource inputs, and does not assume that another retry or a higher limit is a verified repair

#### Scenario: A repair is claimed
- **WHEN** a bounded correction is selected for the failed managed path
- **THEN** acceptance exercises representative retained navigation through the actual managed consumer and checks unaffected supported operations, with current diagnostics separately identified as complete or unavailable

#### Scenario: Optional backend removal is suggested
- **WHEN** a proposed shortcut would remove currently required language support or materially change everyday operating conditions
- **THEN** that shortcut cannot satisfy repair acceptance without the separate user-owned scope decision
