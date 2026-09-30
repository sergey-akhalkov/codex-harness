## ADDED Requirements

### Requirement: Serena sessions recover bounded transient transport failures

The managed Serena client SHALL retry a failed transport at most once within
the original request deadline for explicitly identified read operations. It
SHALL retain project selection and caller response identity, revalidate broker
ownership, and preserve unrelated live services. Cancellation, authorization
failure, invalid protocol data and unknown or mutating tools SHALL NOT authorize
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

### Requirement: Serena delivery includes real semantic acceptance

Delivery of a connected Serena integration SHALL exercise the candidate native
path with the adopted upstream Serena and retained language backends in owned
temporary projects. Missing prerequisites or failed semantic assertions SHALL
prevent a successful delivery receipt. Automatic credential-free CI SHALL run
real Serena integration tests explicitly, including tests otherwise ignored in
the default unit suite. Handshake-only checks SHALL NOT satisfy this acceptance.

#### Scenario: Normal delivery
- **WHEN** the kit delivers an installation with Serena connected
- **THEN** its receipt identifies semantic acceptance through the delivered
  native path, including navigation and an edit observed by a later read

#### Scenario: Broken backend
- **WHEN** an adopted backend starts but cannot answer semantic requests
- **THEN** delivery fails its Serena verification instead of reporting success

#### Scenario: Regression coverage
- **WHEN** automatic Serena integration runs
- **THEN** it exercises real MCP requests, transport reset, broker replacement,
  independent project routing and non-replayed edits with explicit assertions
