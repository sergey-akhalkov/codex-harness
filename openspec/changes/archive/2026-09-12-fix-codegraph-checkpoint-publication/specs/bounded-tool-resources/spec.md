## ADDED Requirements

### Requirement: Bounded checkpoint publication with concurrent readers

CodeGraph SHALL tolerate a transient Windows sharing conflict from a concurrent
reader while publishing a refreshed checkpoint. Publication SHALL remain bounded
by its operation deadline and cancellation, retain a recoverable committed
checkpoint on failure, and report persistent failures without presenting stale
data as current. Ownership and source/build integrity checks SHALL remain active.

#### Scenario: Reader releases a checkpoint during publication

- **WHEN** a short-lived reader overlaps publication and then releases its handle
- **THEN** refresh completes within its deadline and subsequent queries observe
  the saved source change without manual restart or reindexing

#### Scenario: Reader outlives the allowed publication wait

- **WHEN** a sharing conflict persists until cancellation or the publication bound
- **THEN** the operation fails explicitly within that bound and the last committed
  checkpoint remains recoverable
