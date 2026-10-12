## ADDED Requirements

### Requirement: Serena connection lifetime follows its client

Managed Serena STDIO connections SHALL remain available while their client keeps the transport open, regardless of connection age. Request deadlines, worker idle retirement, resource limits and client routing isolation SHALL remain effective. Closing the client input SHALL terminate its proxy without retaining an orphan client process.

#### Scenario: A conversation remains open beyond one day
- **WHEN** a connected client sends an activation or semantic request after the former 24-hour connection expiry
- **THEN** the proxy serves the request normally instead of closing the transport because of its age

#### Scenario: A client returns after worker retirement
- **WHEN** a connected client sends another request after its idle worker has been retired
- **THEN** the proxy obtains a compatible worker and preserves that client's project selection

#### Scenario: A request stalls
- **WHEN** a request exceeds its operation deadline in a long-lived connection
- **THEN** the operation remains bounded without converting connection age into a deadline for unrelated future requests

#### Scenario: A client closes normally
- **WHEN** the client closes its input after using a long-lived connection
- **THEN** its proxy exits and releases its owned connection resources while unrelated clients remain unaffected
