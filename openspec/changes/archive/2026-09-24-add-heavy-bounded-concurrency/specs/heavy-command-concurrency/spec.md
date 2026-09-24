## Purpose

Lets one heavy-command account run a bounded number of concurrent batch trees while keeping one aggregate memory envelope, one aggregate CPU ceiling and mechanical queue admission, release and diagnostics.

## ADDED Requirements

### Requirement: Bounded concurrent heavy-command admission

The account heavy-command queue SHALL admit up to a configured positive number of concurrent command trees per heavy-command account. Admission beyond the bound SHALL wait under the existing bounded queue wait and then fail without starting a payload. Each admitted tree SHALL hold exactly one slot for its lifetime, and a nested heavy command inside an admitted tree SHALL NOT acquire a second slot. A current caller SHALL acquire a shared lock on the account's legacy `heavy-command.lock` before taking a slot and SHALL hold that lock until the slot is released. An exclusive lock on that same file, including the first-byte exclusive lock of the previous build, SHALL block current admission even when every slot file is free. A current admission SHALL block a later exclusive lock on that file. Slot files alone SHALL NOT admit a caller. A slot count of 1 SHALL take the legacy file exclusively and SHALL NOT create a slot file. A slot SHALL be released on normal exit, payload failure, deadline expiry or cancellation, without deleting or altering another holder's admission state.

#### Scenario: Two concurrent trees within the bound
- **WHEN** two independent callers start heavy commands while free slots remain and the configured slot count is at least two
- **THEN** both command trees run concurrently, each holds exactly one slot, and neither caller waits behind the other

#### Scenario: Caller beyond the bound
- **WHEN** every configured slot is busy and another caller requests admission
- **THEN** that caller prints one bounded queue diagnostic naming the busy-slot count and available holder descriptions, waits within the configured queue wait without starting its payload, and proceeds only after a slot is released

#### Scenario: Queue wait expires
- **WHEN** no slot is released before the configured queue wait expires
- **THEN** the caller fails through the existing admission-failure exit path with no payload started and no holder disturbed

#### Scenario: Nested heavy command
- **WHEN** an admitted command tree invokes heavy again
- **THEN** the nested call runs without acquiring a second slot and reports that it inherited the outer admission

#### Scenario: Slot release on interruption
- **WHEN** an admitted command is cancelled, misses its deadline or its owner dies
- **THEN** its command tree is cleaned up, its slot becomes available to the next permitted caller, and other holders keep their slots and trees

#### Scenario: Legacy exclusive lock blocks a free slot
- **WHEN** an exclusive lock is held on the account `heavy-command.lock` and every current slot file is free
- **THEN** a current caller waits on the existing queue path, starts no payload, and proceeds only after that exclusive lock is released

#### Scenario: Current admission blocks a legacy exclusive lock
- **WHEN** a current caller holds one slot
- **THEN** an exclusive lock of that account's `heavy-command.lock` conflicts until the current admission releases it, and locking only a slot file never constitutes admission

### Requirement: Account aggregate memory envelope

All concurrently admitted heavy-command trees of one account SHALL be members of one account aggregate Job whose commit-memory limit is the account aggregate memory setting enforced by `JOB_OBJECT_LIMIT_JOB_MEMORY` (`JobMemoryLimit`). The aggregate Job SHALL NOT use `JOB_OBJECT_LIMIT_PROCESS_MEMORY` as that limit, SHALL NOT set a CPU rate, and SHALL NOT set kill-on-close. Each admitted tree SHALL also keep its own containment Job with its per-tree `JOB_OBJECT_LIMIT_JOB_MEMORY` limit, deadline and kill-on-close cleanup. The aggregate envelope SHALL NOT add a second CPU cap, SHALL NOT be applied to the shared CPU job, SHALL stay isolated per heavy-command account, and SHALL be created and joined by the callers themselves without a background owner process. A missing aggregate setting SHALL equal the effective per-tree memory limit. An explicit aggregate below that per-tree limit SHALL be refused before admission.

#### Scenario: Concurrent trees share one envelope
- **WHEN** two trees are admitted concurrently in one account
- **THEN** kernel readback shows both payload trees inside one account aggregate Job whose extended limits include `JOB_OBJECT_LIMIT_JOB_MEMORY`, whose `JobMemoryLimit` equals the configured aggregate, whose per-process memory limit and CPU rate are unset, and each tree inside its own containment Job with its per-tree settings

#### Scenario: Aggregate limit binds collectively
- **WHEN** the combined committed memory of admitted trees exceeds the aggregate limit
- **THEN** further allocations inside those trees fail inside the Job rather than escaping to the machine, while processes outside the account remain unaffected

#### Scenario: Lone tree keeps the full envelope
- **WHEN** exactly one tree is admitted in an account
- **THEN** its usable memory envelope is the full aggregate limit, matching a serialized account

#### Scenario: Per-account isolation
- **WHEN** two callers use different isolated heavy-command accounts
- **THEN** their slots and aggregate envelopes are independent, and one account's busy slots never block the other

### Requirement: Concurrency policy and reporting

The machine-local heavy-command policy SHALL define the concurrent slot count and the aggregate memory limit with defaults that preserve the aggregate guarantees. A missing slot count SHALL default to 2. A missing aggregate memory limit SHALL equal the effective per-tree memory limit. A policy without the new fields SHALL use those defaults without being rewritten, and an explicit slot count of 1 SHALL restore serialized admission. `heavy budget` SHALL report the effective slot count, aggregate memory limit, per-tree memory limit and their policy source in text and JSON without mutating policy or starting work.

#### Scenario: Legacy policy
- **WHEN** an existing heavy-command policy file lacks the new fields
- **THEN** heavy uses two slots and an aggregate limit equal to that file's effective per-tree memory limit, reports those as defaults, and leaves the file bytes unchanged

#### Scenario: Explicit serialization
- **WHEN** the policy sets the slot count to 1
- **THEN** concurrent callers serialize exactly as before this change, including queue diagnostics and release behavior

#### Scenario: Installed default
- **WHEN** no policy file exists on a fresh installation
- **THEN** the account admits two concurrent trees by default while the aggregate memory limit and the shared aggregate CPU ceiling remain at their documented single-account values

#### Scenario: Effective-value reporting
- **WHEN** `heavy budget` runs with mixed explicit and default policy fields
- **THEN** its text and JSON output report each effective value and whether it came from the policy file or the default
