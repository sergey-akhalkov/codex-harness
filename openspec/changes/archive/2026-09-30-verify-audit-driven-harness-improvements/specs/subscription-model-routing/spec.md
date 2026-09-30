## ADDED Requirements

### Requirement: Owned xAI endpoint identity precedes secret-bearing traffic

The harness SHALL establish that the selected local xAI transport belongs to the intended live harness runtime before routing authorization or user payloads to it. A listener's self-reported executable path or schema marker alone SHALL NOT establish ownership. Unknown listeners, malformed or spoofed identity, unavailable probes and occupied ports SHALL remain distinguishable from a confirmed free port. An unverified listener MUST NOT be reused or terminated as a harness process. Failure SHALL name the blocked route and recovery action without exposing secrets, substituting a model or changing billing. A separately allocated endpoint SHALL be usable only after its own ownership and routing are verified.

#### Scenario: A foreign listener occupies the usual port
- **WHEN** the expected port is held by a listener whose ownership cannot be verified
- **THEN** it receives no secret-bearing model request, is not terminated, and the route either selects a verified owned endpoint or fails explicitly

#### Scenario: A listener echoes a plausible identity document
- **WHEN** a listener returns the expected schema and an executable path naming the selected build without the required ownership evidence
- **THEN** it is not classified as a trusted harness endpoint

#### Scenario: Identity probing times out
- **WHEN** a bounded identity probe times out or returns incomplete transport evidence
- **THEN** the result is unavailable or unknown rather than proof that the port is free

#### Scenario: A stale control request targets another generation
- **WHEN** retirement or another lifecycle request lacks the required generation ownership
- **THEN** it cannot retire unrelated active sessions or a foreign listener

### Requirement: xAI generations preserve existing sessions across delivery

New sessions SHALL bind to the verified selected transport generation while existing sessions retain a compatible owned endpoint for both active streams and subsequent requests. Ordinary update or launch SHALL NOT shorten an accepted request's configured deadline or require existing sessions to restart merely to free a shared port. A generation SHALL retire after its owners and accepted work release it, subject to existing explicit cancellation and recovery controls. Forced termination SHALL remain an explicitly selected operation with observable effects, not an implicit side effect of starting another generation. Generation state SHALL remain local, bounded through ownership-based reclamation and recoverable after interruption.

#### Scenario: A stream outlives the former drain grace
- **WHEN** an accepted request remains valid beyond the former 60-second retirement grace while a new build starts
- **THEN** it continues under its original deadline and a new session can use the new verified generation

#### Scenario: An older session sends its next request
- **WHEN** a session bound before delivery finishes one request and later submits another
- **THEN** its endpoint remains usable and compatible until that session releases ownership or explicitly chooses recovery

#### Scenario: Two launches and one owner exit overlap
- **WHEN** concurrent launches select a generation while an older owner exits
- **THEN** endpoint selection and retirement neither lose a live owner nor create a stale trusted route

#### Scenario: Recovery follows an unclean exit
- **WHEN** a generation's recorded owner has disappeared
- **THEN** recovery verifies actual ownership and liveness before reclaiming only abandoned resources
