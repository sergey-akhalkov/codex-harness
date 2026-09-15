## MODIFIED Requirements

### Requirement: Reversible experimental context trial

The kit SHALL provide and exercise a reversible trial of the installed client's supported experimental context-management setting for new Astra tasks. Verification SHALL distinguish configuration parsing, effective activation through the ordinary local entry point, account/provider eligibility and observed task behavior. Presence of the setting only in checkout source or in an unused profile file MUST NOT count as activation. It MUST NOT claim runtime success from a feature-list or schema check alone. The chosen setting and rollback SHALL participate in the existing global installation lifecycle and respect explicit user overrides.

Native memories and Fast SHALL remain excluded.

#### Scenario: Eligible new task
- **WHEN** a new Astra task starts with the experimental setting enabled on an eligible route
- **THEN** evidence identifies the client, effective setting and model route, and a bounded continuation case verifies that an earlier constraint is retained and applied

#### Scenario: Runtime support is absent
- **WHEN** the client or account rejects or cannot establish experimental activation
- **THEN** the workflow records the concrete result, preserves a working ordinary context path, and leaves runtime acceptance incomplete without changing model provider or billing silently

#### Scenario: Shared defaults do not reach the session
- **WHEN** an ordinary local session starts without live shared-default injection
- **THEN** experimental context management is reported as not active for that session, and source-file presence alone is not treated as a passed trial

#### Scenario: Explicit rollback
- **WHEN** the trial causes a verified regression or the setting is explicitly disabled
- **THEN** a fresh task uses the ordinary context path while existing project memory and unrelated global settings remain intact
