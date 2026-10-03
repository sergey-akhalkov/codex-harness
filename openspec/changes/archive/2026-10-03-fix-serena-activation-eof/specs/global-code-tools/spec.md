## MODIFIED Requirements

### Requirement: Serena sessions recover bounded transient transport failures

The managed Serena client SHALL retry a failed transport at most once within
the original request deadline for explicitly identified read operations and idempotent project activation. It
SHALL retain project selection and caller response identity, revalidate broker
ownership, and preserve unrelated live services. Cancellation, authorization
failure, invalid protocol data and unknown or source-mutating tools SHALL NOT authorize
transport replay. An uncertain mutation SHALL return a cause-bearing error
without being replayed, and later reads SHALL remain usable.

#### Scenario: Connection reset during symbol navigation
- **WHEN** a symbol query loses its broker connection with a transient reset
- **THEN** the client reconnects and retries once in the same session, returning
  the selected project's symbols with the original request identifier

#### Scenario: Persistent failure
- **WHEN** the retry also fails or the original deadline expires
- **THEN** the client reports failure with the original cause and recovery
  outcome, without an unbounded restart loop

#### Scenario: Edit response is lost
- **WHEN** a transport failure leaves an edit's completion uncertain
- **THEN** the edit is not replayed and a subsequent read can inspect its effect


#### Scenario: Activation loses its response
- **WHEN** project activation encounters a transient transport failure before or after the worker accepts it
- **THEN** the client retries at most once, updates its route only from a successful response and subsequent navigation uses the requested project

#### Scenario: Worker exits during a request
- **WHEN** a worker transport closes unexpectedly
- **THEN** the failure retains the original transport classification, stderr locator and available native exit and job-memory observations without claiming that a configured limit is observed usage

## ADDED Requirements

### Requirement: Serena aggregate containment includes admitted workers

The broker's aggregate committed-memory allowance SHALL include the service allowance and all worker slots permitted by its validated resource policy. It SHALL retain finite containment, per-worker memory limits, CPU settings and ownership cleanup. The installed broker SHALL retain a native kernel readback of its aggregate allowance so configuration mistakes can be distinguished from measured memory consumption.

#### Scenario: Multiple workers share a broker
- **WHEN** a broker starts with multiple configured worker slots
- **THEN** its parent Job does not impose the service-only memory ceiling on the combined workers, and each worker remains subject to its own limit

#### Scenario: Resource bound is updated
- **WHEN** the broker configures its aggregate allowance before starting workers
- **THEN** the actual native limit matches the computed allowance and CPU, kill-on-close and handle-inheritance settings remain intact

