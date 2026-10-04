## ADDED Requirements

### Requirement: Comparison suite targets fit the heavy deadline

The `improvement_comparison` integration coverage SHALL be packaged as more than one cargo test target such that each target's serialized execution completes inside the shared heavy-command deadline with margin, while every pre-split test appears exactly once across the target set.

#### Scenario: Full suite runs without deadline overrun
- **WHEN** each new target runs serialized through `codex-harness heavy`
- **THEN** every test executes and exits 0 without the 1800 s deadline terminating the command

#### Scenario: Coverage and CI parity are preserved
- **WHEN** the installed workflow target list and the `ci_workflow` equality contract are updated
- **THEN** they name exactly the resulting target set and the contract check passes
