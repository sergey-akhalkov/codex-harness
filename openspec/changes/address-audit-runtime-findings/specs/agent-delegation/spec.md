## ADDED Requirements

### Requirement: Benefit ledger distinguishes recorded adoption from supported evidence

The existing feedback ledger SHALL preserve the latest recorded decision and separately report comparison consistency and evidence limitations. A record SHALL NOT be presented as a proven default solely because its outcome is `adopt`. Supported adoption SHALL require unchanged-or-improved quality, a positive matched-comparison count, valid finite comparison values, a declared tolerance and accounting basis, identifiable baseline/candidate arms and no contradictory timing arithmetic or unexplained regression beyond tolerance. Missing or contradictory evidence SHALL remain unproven without rewriting the original record. A record's consistency or evidence reference SHALL NOT be described as independent execution or proof of the underlying experiment. Existing explicit user acceptance of a trade-off SHALL remain distinguishable from a demonstrated improvement.

#### Scenario: Adoption contradicts measured quality
- **WHEN** the latest record for an item says `outcome=adopt quality=regressed` or `quality=unmeasurable`
- **THEN** the ledger retains that recorded decision but does not label it a proven default, and identifies the contradiction or missing quality evidence

#### Scenario: Comparison fields are absent or invalid
- **WHEN** an adoption omits matched observations or timing/accounting fields, or includes non-finite values, impossible counts or inconsistent regression arithmetic
- **THEN** it remains visible as unsupported with a useful reason rather than receiving fabricated defaults

#### Scenario: A newer invalid record follows an adoption
- **WHEN** a later benefit record attributable to the same item is incomplete, contradictory or malformed
- **THEN** the ledger exposes that newer unresolved record and does not silently restore the earlier adopted status

#### Scenario: A consistent comparison is recorded
- **WHEN** the latest adoption includes the required comparison data with unchanged-or-better quality and delivery-time results within its declared tolerance
- **THEN** the ledger reports a consistent recorded adoption with its existing evidence locator and makes clear that it has not rerun or independently certified the experiment

#### Scenario: Adoption is withdrawn
- **WHEN** a later rejection or inconclusive decision supersedes an adopted comparison
- **THEN** the item is no longer reported as a proven default and the recorded history remains available
