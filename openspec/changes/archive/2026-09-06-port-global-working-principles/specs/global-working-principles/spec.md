## Purpose

Provide portable working principles that guide Codex toward full specification completion, prevention of P0/P1 defects, and efficient development across the user's repositories.

## ADDED Requirements

### Requirement: Outcome and completion contract

The principles SHALL prioritize complete agreed outcomes, P0/P1 prevention, and time to a verified result. They SHALL use OpenSpec by default and define completion as fulfilling the entire agreed specification and closing every associated task based on completed work and applicable verification.

#### Scenario: A useful slice is finished but specification work remains
- **WHEN** a milestone is working and an associated requirement or task remains incomplete
- **THEN** the principles classify that milestone as progress and require continuing the remaining authorized work

### Requirement: Proportionate review and recovery

The principles SHALL determine additional review, verification, and recovery work from the agreed requirements and concrete risks. They SHALL avoid universal fresh-review gates, fixed re-review ceilings, and unconditional mechanism changes after a fixed retry count. Claims of production readiness SHALL have the relevant evidence, while production integration SHALL not silently expand an agreed task.

#### Scenario: A serious defect remains after review
- **WHEN** a review or correction exposes an unresolved P0/P1 risk
- **THEN** the principles require addressing it and verifying the changed behavior without imposing a fixed one-re-review limit

#### Scenario: A transient failure has an evidenced recovery path
- **WHEN** the same operation encounters a temporary failure with a justified bounded retry strategy
- **THEN** the principles permit that strategy and require changing approach when repetition provides no progress or useful evidence

### Requirement: Global initial-context loading

After activation, the full portable principles SHALL appear in the initial Codex instruction context of new sessions across repositories using the configured Codex home. Activation SHALL use documented native instruction discovery and preserve the ordinary instruction hierarchy.

#### Scenario: Codex starts outside the harness repository
- **WHEN** a new session starts in a separate repository with the activated Codex home
- **THEN** its initial instruction context contains the principles without requiring a skill invocation or a manual file read

#### Scenario: Project guidance is also present
- **WHEN** the target repository has its own AGENTS.md
- **THEN** the global principles and the applicable project instructions are both discoverable under Codex's normal precedence

### Requirement: Portable source and bounded activation

The reusable principles SHALL be self-contained and independent of a particular machine path, shell, project, or installed optional tool. Host activation SHALL preserve existing instructions and unrelated configuration, authentication, and session state. The source SHALL remain in this repository, with the relationship to the active global file documented.

#### Scenario: An existing global instruction file or override is discovered
- **WHEN** activation encounters an existing instruction source
- **THEN** it preserves that source and resolves composition before replacing or obscuring any existing rules

### Requirement: Traceable adaptation

Supporting documentation SHALL identify the source revision, account for every original principle through a retained, adapted, or consolidated disposition, and record the user's resolution of substantive tensions. Detailed comparison and verification notes SHALL remain outside the always-loaded principles.

#### Scenario: A maintainer asks why the fresh-review rule differs
- **WHEN** the maintainer reads the adaptation record
- **THEN** it explains the original rule, the concrete trade-off, the risk-based replacement, and the user's confirmation

### Requirement: Verified activation and recoverability

The change SHALL verify actual initial-prompt loading from at least two separate working directories, including a repository with project instructions. Documentation SHALL describe update and rollback behavior and distinguish new-session loading from already-running sessions or another Codex home.

#### Scenario: The user wants to disconnect the global principles
- **WHEN** the documented rollback is applied to the activation created by this change
- **THEN** future sessions stop loading that global source while the repository source and unrelated host state remain intact
