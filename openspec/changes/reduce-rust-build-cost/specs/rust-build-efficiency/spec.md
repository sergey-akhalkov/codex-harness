## Purpose

Reduce recurring Rust development disk usage and build/test latency while preserving debuggability on demand, verification coverage and trustworthy global delivery.

## ADDED Requirements

### Requirement: Compact ordinary development with full debugging on demand

Ordinary development and test builds SHALL retain source-location backtraces with reduced debug information. An explicitly selected debugging profile SHALL provide full debug information for workspace code and dependencies. Ordinary development SHALL preserve incremental reuse. Assertions, overflow checks and the required tests SHALL remain enabled under their existing semantics.

#### Scenario: Daily edit and test
- **WHEN** a developer builds or tests without selecting a profile
- **THEN** the compact development settings apply and an unchanged repeated invocation reuses eligible artifacts

#### Scenario: Debugger investigation
- **WHEN** the developer selects the documented debugging profile
- **THEN** a separate artifact set includes full debug information without requiring a manifest edit

### Requirement: Publication builds only delivery targets

Native publication SHALL compile the declared delivery programs without selecting unrelated test helpers. The complete delivered binary set, content identity, fresh-input protection, resource limits and recovery behavior SHALL remain intact.

#### Scenario: Unrelated fixture binary
- **WHEN** a source package also contains a binary used only by tests
- **THEN** publication does not compile it and the normal test route can still build and use it

#### Scenario: Changed publication input
- **WHEN** source bytes change while timestamps are preserved
- **THEN** publication continues to refuse reuse of an artifact for a different source identity

### Requirement: Measured optimization preserves test coverage and operating limits

Build/test optimization SHALL distinguish compilation, warm reuse, edited-code feedback and test execution. Consolidation or concurrency changes SHALL preserve selected test coverage, ignored-test policy, failure reporting and resource isolation. Adoption SHALL use a matched comparison under unchanged CPU/memory limits, with no unexplained correctness regression. A rejected or inconclusive candidate SHALL leave the accepted behavior intact and retain its reason.

#### Scenario: Integration target consolidation
- **WHEN** several integration targets are consolidated
- **THEN** the prior test cases remain discoverable and executable, required callers are updated, and comparison includes compilation/linking and execution

#### Scenario: Higher Cargo concurrency
- **WHEN** a larger job count is evaluated
- **THEN** it remains inside the existing heavy-command budget and is adopted only on measured benefit without failed or omitted required checks

### Requirement: Reclaimable cache lifecycle

The development workflow SHALL distinguish active reusable build outputs, inactive regenerable targets and retained diagnostic evidence. Cleanup SHALL resolve owned paths, verify inactivity and preserve unfinished work, live executables, unrelated projects and required evidence. Reports SHALL distinguish logical file sizes from allocated disk space and SHALL account for additional caches introduced by an optimization.

#### Scenario: Retired development target
- **WHEN** a target is confirmed inactive and contains only regenerable build outputs
- **THEN** scoped cleanup can reclaim it while recording the measured scope and keeping active development reuse

#### Scenario: Uncertain worktree ownership
- **WHEN** a worktree has unfinished work or its runtime ownership is uncertain
- **THEN** its artifacts are preserved and the unresolved ownership is reported
