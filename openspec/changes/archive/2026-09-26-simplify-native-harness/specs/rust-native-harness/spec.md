## ADDED Requirements

### Requirement: Native substitution preserves accepted outcomes

A replacement by a native capability SHALL preserve the affected accepted outcomes, business rules, authorization, concurrent use, recovery and installed entrypoint behavior. Help text, a successful API acknowledgment or a unit-test-only consumer SHALL NOT establish equivalence. An unsupported version or missing native guarantee SHALL retain the smallest working adapter and report the concrete limitation rather than silently drop a feature, add routine manual steps, change a provider or weaken acceptance. Required unfinished work SHALL remain explicit after removal of an unused implementation; removal SHALL NOT count as completing that work.

#### Scenario: Native operation lacks an ownership guarantee
- **WHEN** a native command can start or stop a session but cannot enforce the kit's exact-run ownership and neighboring-run isolation
- **THEN** the retained adapter enforces those rules through the supported entrypoint, and the replacement is not described as a complete native substitution

#### Scenario: A helper has no established runtime consumer
- **WHEN** a candidate is referenced only by tests or inactive scaffolding
- **THEN** its removal requires checked callers, supported external entrypoints and a disposition for every accepted rule it represents, without deleting those rules or declaring their unfinished acceptance complete

#### Scenario: Substitution reaches ordinary global use
- **WHEN** a native-backed replacement is delivered
- **THEN** the installed outside-checkout workflow preserves its arguments, results, failure meanings, live source ownership, active work and recovery without a new endpoint, configuration or manual-check ritual

### Requirement: Simplification reduces maintained work rather than relocating it

The completed simplification SHALL demonstrate a net reduction of first-party production code and affected owned instruction content against an identified source baseline, with tests, fixtures, planning artifacts and operational documentation reported separately. It SHALL preserve required checks and readable implementation; minification, moving code to another maintained component, deleting tests, hiding instructions in mandatory references or substituting repeated agent reasoning for deterministic checks SHALL NOT establish reduction. Every assessed replacement SHALL have a recorded removal or an evidence-based retention decision in its existing owner. No percentage, token, speed or subscription benefit SHALL be claimed beyond the actual measurement.

#### Scenario: Code is moved into another package
- **WHEN** a refactor removes a module but introduces an equivalent maintained helper or dependency adapter elsewhere
- **THEN** acceptance includes the added code and operating burden rather than counting only the deleted module

#### Scenario: Tests dominate a reported line reduction
- **WHEN** obsolete implementation-specific tests are removed after equivalent behavior coverage is retained
- **THEN** production and test changes are reported separately and the production reduction claim does not include test deletion
