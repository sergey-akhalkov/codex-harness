## ADDED Requirements

### Requirement: Interrupted pooled executors resume through the pooled command

The kit SHALL provide an executor dispatch that continues one exact
interrupted pooled session on its recorded pool slot without resynchronizing
that slot: no fetch, reset, clean or base change SHALL occur between the
interruption and the resumed session. The command SHALL require the exact
session id, the explicit slot index and the owner id; it SHALL refuse a live
owner and any slot claimed by a different owner, and it SHALL adopt a slot
whose owner was cleared by reconciliation only for the explicitly named slot
and owner. The resumed session SHALL run through the same receipt, lease,
terminal and console hosts as a fresh dispatch, and its child invocation
SHALL use the verified non-interactive resume argument order.

#### Scenario: Partial work survives the resume
- **WHEN** an interrupted executor slot holds uncommitted partial work and the lead resumes its exact session on that slot
- **THEN** the slot is rebound without reset or clean, the partial work is still present when the resumed session starts, and the recorded base is unchanged

#### Scenario: A reconciled slot is adopted explicitly
- **WHEN** the interrupted session's host is gone and reconciliation cleared the slot record's owner
- **THEN** resume with that slot index and owner id rebinds the slot and records the lease, instead of failing an ownership comparison

#### Scenario: Another owner's claim is refused
- **WHEN** the named slot is bound to a different owner or the requested owner is already live
- **THEN** resume refuses with both identities named and no tree state changes

#### Scenario: Hand-edited receipts fail with a remedy
- **WHEN** `executor run --file` receives a receipt whose owner differs from an unowned slot record
- **THEN** the failure names the pooled spawn or resume command for that slot instead of only the owner mismatch
