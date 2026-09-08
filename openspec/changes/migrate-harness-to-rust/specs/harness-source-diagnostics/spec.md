## MODIFIED Requirements

### Requirement: Globally available focused Check

The kit SHALL provide `codex-harness.exe check --diagnose --project <directory>` and an equivalent globally connected `codex-harness-check.exe` command. Install and Update SHALL register the verified native diagnostic entry point while preserving direct source connections for its configuration and resources; Disconnect and recovery SHALL preserve the existing ownership and rollback guarantees. Check without Diagnose SHALL retain its behavior. A report SHALL identify the project, selected profile, observation scope and overall status as healthy, attention or incomplete. Upgrade SHALL migrate the owned `codex-harness-check.ps1` connection and document the native replacement for both legacy diagnostic command forms.

#### Scenario: Outside-repository consumer
- **WHEN** the installed global command runs in another project without a repository-relative command path
- **THEN** it inspects that project using the connected source and selected harness profile and returns a structured report

#### Scenario: Lifecycle and ownership
- **WHEN** a fixture installation is updated, disconnected or rolled back after an interrupted update
- **THEN** the diagnostic connection follows the same ownership rules as other managed links and unrelated files remain unchanged

#### Scenario: Legacy diagnostic connection
- **WHEN** native update encounters the owned script-based diagnostic entry point from an existing installation
- **THEN** it installs the verified native equivalent, preserves the model-free private bounded report contract, and leaves a conflicting foreign target intact with an actionable report
