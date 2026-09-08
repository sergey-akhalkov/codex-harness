## MODIFIED Requirements

### Requirement: Persistent global hook suspension

The kit SHALL support a recoverable suspension of all Codex lifecycle hooks across user/base configuration, the managed profile and installed hook definitions. Development without ordinary lifecycle hooks SHALL remain the reusable default after completion, update and archive. The specifically accepted RTK command-compression hook SHALL be the only newly enabled managed exception, with bounded execution and native trust. Already-running sessions retaining cached diagnostic harness handlers SHALL perform no automatic diagnostic analysis, fallback scan, context injection or completion continuation. A fresh consumer outside the checkout SHALL observe only the accepted selection, with base and managed profile agreement. Machine-local markers and backups SHALL remain outside reusable source. A fresh core-only installation without the accepted capability SHALL not enable hooks.

#### Scenario: A cached session invokes an old handler
- **WHEN** an old automatic diagnostic handler is invoked
- **THEN** it returns without diagnostic work or feedback

#### Scenario: Another project starts a new session
- **WHEN** it consumes the installed base or managed profile with RTK selected
- **THEN** only the trusted RTK exception is enabled among managed hooks, without project-local configuration

#### Scenario: The optimization change is completed
- **WHEN** its tasks are closed, archived or followed by a kit update
- **THEN** the consumer retains the accepted RTK selection and no backup or lifecycle label reactivates rejected hooks

#### Scenario: Explicit suspension or disconnection
- **WHEN** the RTK capability is disabled or disconnected
- **THEN** its hook is inactive, rollback remains recoverable and unrelated user configuration is preserved
