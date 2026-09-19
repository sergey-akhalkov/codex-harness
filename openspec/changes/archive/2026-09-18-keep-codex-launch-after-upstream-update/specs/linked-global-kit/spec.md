## ADDED Requirements

### Requirement: Codex CLI updates do not block ordinary launch

Harness compatibility and registration freshness checks SHALL NOT prevent the
installed Codex CLI from starting after that CLI is updated. A digest mismatch
for the registered upstream executable or its package metadata is not by itself
an incompatibility: ordinary launch MUST start the current CLI and MUST NOT
warn when harness enhancements still apply. A warning is allowed only when the
harness cannot apply its enhancements to that CLI, and MUST NOT refuse the
process. Missing Codex itself and recursive command discovery SHALL remain
explicit failures.

#### Scenario: User restarts Codex after its own updater
- **WHEN** Codex CLI has been updated in place and the user starts `codex` again through the installed harness command with a still-preparable harness session
- **THEN** the current Codex CLI starts without a launch blocker and without a compatibility warning; Check may still record stale registration
