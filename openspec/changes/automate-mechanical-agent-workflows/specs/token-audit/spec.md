## ADDED Requirements

### Requirement: Bounded report presentation with complete detail
Interactive report and findings output SHALL present bounded ranked records, total and omitted counts, coverage warnings and the measurement limitations. Full existing JSON and baseline contracts MUST remain compatible. A compact observation SHALL retain its complete machine report in local evidence and provide detail access without scanning sessions again. Retention SHALL be bounded and expired detail SHALL produce an explicit error. Presentation MUST NOT alter aggregate counters or convert bytes into claimed subscription savings.

#### Scenario: Many recorded sessions
- **WHEN** the report contains more rows than the presentation limit
- **THEN** the summary reports omitted counts and a locator for the complete same-scan report
- **AND** retrieving details does not run a model or rescan sessions

#### Scenario: Incomplete or erroneous usage
- **WHEN** coverage is incomplete or some usage is unavailable
- **THEN** the summary retains that warning and does not present missing values as zero

#### Scenario: Existing machine consumer
- **WHEN** a caller explicitly requests full JSON or a baseline operation
- **THEN** its complete existing data contract is preserved

#### Scenario: Two scans finish with the same timestamp
- **WHEN** two reports share a generated timestamp but contain different observations
- **THEN** each returned locator retains its own complete scan without overwriting the other
