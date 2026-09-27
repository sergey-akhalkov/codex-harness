## REMOVED Requirements

### Requirement: An oversized final-message read does not fail a completed turn

RATIONALE: The transport size limit this requirement worked around is removed;
a full-thread read no longer fails for record size, so both oversized-read
scenarios are unreachable and the delivered-message fallback would mask real
read failures.

## ADDED Requirements

### Requirement: Managed executor transports impose no harness-side record size cap

The host's control connection to its owned app-server child and the
frontend-facing relay SHALL NOT impose a harness-side WebSocket message, frame
or write-buffer size limit on managed conversation records. Time bounds -
connect, read poll, socket write and relay shutdown - SHALL remain bounded. An
exact-session resume or a full-thread final-message read whose records exceed
any fixed small bound SHALL complete through the same transport. A final
message SHALL come from the thread's own items; a full-thread read that fails
for its own reason SHALL fail the run, with no delivered-message fallback for
an oversized read.

#### Scenario: An exact-session resume exceeds one mebibyte
- **WHEN** a managed executor resumes an exact session whose resume state record is larger than one mebibyte
- **THEN** the resume completes on the managed backend, the assignment is submitted on that exact thread, and no new conversation is started

#### Scenario: A full-thread read exceeds one mebibyte
- **WHEN** a completed turn's full-thread read is larger than one mebibyte and its thread items carry a final assistant message
- **THEN** the run completes and records that message

#### Scenario: Another final-message read failure still fails the host
- **WHEN** a completed turn's final-message read fails for a reason other than record size
- **THEN** the host records the failure, terminates the owned child tree, and does not report a successful run
