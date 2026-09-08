## ADDED Requirements

### Requirement: Bounded diagnostic reconciliation makes progress

When an edit requires more work than one synchronous diagnostic budget, subsequent automatic checks SHALL continue remaining work for the same source and configuration contents. They MUST NOT repeatedly analyze already completed current results merely because another affected file is pending. The existing finite delivery bound and current-revision guarantees SHALL remain intact, including verification of source contents before delivery. A new relevant edit SHALL invalidate affected results and unfinished work from older inputs.

#### Scenario: A cohort spans multiple hook budgets
- **WHEN** finite analyses for an unchanged affected cohort require several hook invocations
- **THEN** successive checks complete that cohort and a following unchanged tool returns without a new diagnostic run

#### Scenario: Source changes during a partial check
- **WHEN** source or configuration contents change before delivery or before reconciliation finishes
- **THEN** results from the old inputs cannot establish current clearance and the affected work remains scheduled

### Requirement: Pre-edit work shares one finite deadline

The independent pre-edit hook SHALL keep filesystem traversal, root bookkeeping and database lock waits within a shared budget below its ten-second native timeout. Large empty directory trees and concurrent diagnostic writers MUST NOT silently exceed that budget or produce an apparently complete baseline after incomplete work. Long source scans MUST NOT retain a database write lock.

#### Scenario: A journal writer holds its transaction
- **WHEN** the pre-edit hook encounters a contended diagnostic journal
- **THEN** it returns within its finite budget with either a valid baseline or explicit unavailability while preserving existing edits and baseline state

#### Scenario: Traversal exhausts its budget before finding files
- **WHEN** scanning a directory tree uses the available time even without source files
- **THEN** the baseline is explicitly incomplete and does not establish a successful empty check
