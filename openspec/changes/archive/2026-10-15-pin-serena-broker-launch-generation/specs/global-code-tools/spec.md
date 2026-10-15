## ADDED Requirements

### Requirement: Open Serena sessions retain their manager generation

An open managed Serena session SHALL retain a consistent manager generation for broker identity and launch after an installation update. A subsequent broker restart SHALL preserve the session's selected project and response identity without launching a different build through a changed installation link. Existing integrity, ownership and bounded recovery checks SHALL remain effective.

#### Scenario: Installed manager link changes before broker restart
- **WHEN** the installed manager link changes while a Serena session remains open and its broker later exits
- **THEN** the next symbol query returns the selected project's symbol body with its original request id using the session's retained manager generation

#### Scenario: New session after delivery
- **WHEN** a new Serena session starts through the updated installation link
- **THEN** it uses the newly installed manager generation without replacing the older session's live broker
