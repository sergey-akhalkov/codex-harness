# Spec Delta

## ADDED Requirements

### Requirement: Rebind an ended app-server without changing the run

When the originating lead thread is registered on a new app-server, the plugin
SHALL atomically update that run receipt's app-server lease only if the
previous process has provably ended and the live endpoint still names the same
lead thread. The run, executor thread, slot, checkout, base and profile SHALL
remain unchanged. A live or unverifiable previous process SHALL be refused
before effects.

#### Scenario: Resume a lead on a new app-server

- **WHEN** the same lead thread has a live registered endpoint on a new
  app-server and the process in its run receipt has ended
- **THEN** the next authorized operation records the new exact process in that
  receipt and can observe or control the unchanged run

#### Scenario: The old app-server is still live

- **WHEN** the run receipt's app-server process is still live but differs from
  the registered endpoint
- **THEN** the operation refuses without changing the receipt or addressing
  either runtime

#### Scenario: Process liveness cannot be proved

- **WHEN** identity inspection cannot determine whether the recorded
  app-server process ended
- **THEN** the operation refuses, preserves the lease and reports that the
  reconnect was not reconciled
