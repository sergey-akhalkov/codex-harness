## ADDED Requirements

### Requirement: Observation waits preserve the running conversation

Managed observation SHALL preserve a live conversation across idle intervals and
partial control messages, with bounded waits and ordered delivery. An observation
deadline SHALL NOT establish task failure or replay a model or tool request.
Actual connection closure, malformed messages and unrecoverable transport errors
SHALL remain visible failures with their original cause.

#### Scenario: Repeated empty and partial-message observations
- **WHEN** the local control server pauses repeatedly, including midway through a message
- **THEN** each observation remains bounded and the eventual complete messages arrive once in order on the same connection

#### Scenario: Closed or invalid control stream
- **WHEN** the control server closes the connection or supplies an invalid control message
- **THEN** observation reports the failure and does not treat it as an idle interval or successful completion
