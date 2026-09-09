## Purpose

Проверяемо применять экспериментальное управление контекстом Astra и поддерживаемый способ бокового вопроса, сохраняя работоспособность основной задачи и действующих моделей.

## ADDED Requirements

### Requirement: Reversible experimental context trial

The kit SHALL provide and exercise a reversible trial of the installed client's supported experimental context-management setting for new Astra tasks. Verification SHALL distinguish configuration parsing, effective activation, account/provider eligibility and observed task behavior. It MUST NOT claim runtime success from a feature-list or schema check alone. The chosen setting and rollback SHALL participate in the existing global installation lifecycle and respect explicit user overrides.

Native memories and Fast SHALL remain excluded.

#### Scenario: Eligible new task
- **WHEN** a new Astra task starts with the experimental setting enabled on an eligible route
- **THEN** evidence identifies the client, effective setting and model route, and a bounded continuation case verifies that an earlier constraint is retained and applied

#### Scenario: Runtime support is absent
- **WHEN** the client or account rejects or cannot establish experimental activation
- **THEN** the workflow records the concrete result, preserves a working ordinary context path, and leaves runtime acceptance incomplete without changing model provider or billing silently

#### Scenario: Explicit rollback
- **WHEN** the trial causes a verified regression or the setting is explicitly disabled
- **THEN** a fresh task uses the ordinary context path while existing project memory and unrelated global settings remain intact

### Requirement: Version-specific side-question guidance

The kit SHALL document the observed availability and state restrictions of `/btw` and `/side` on the installed CLI. Guidance SHALL reuse current reproduction evidence and the user's confirmation that the command now works, distinguish those evidence sources, and avoid asserting an unestablished original cause. The closed incident MUST NOT require further model calls or feature changes without a new observed problem.

#### Scenario: Command works with the existing feature configuration
- **WHEN** reproduction establishes native command availability with v2 disabled and the user confirms restored operation
- **THEN** guidance records those facts and prerequisites without prescribing v2 activation or leaving the user's incident open

### Requirement: Compatible side-question workflow

The delivered guidance SHALL describe the supported native side-question route, its parent-conversation prerequisite, restrictions and return controls, with the limits of local testing stated. Changes MUST preserve the accepted Astra-only OpenAI assignments, Grok middle routing and running shared services. An ordinary fork MUST NOT be described as filesystem isolation or as equivalent to ephemeral native side chat.

#### Scenario: User follows the native route
- **WHEN** the user consults the side-question guide
- **THEN** it explains using `/btw` or `/side` from a started main conversation, the native return controls, restrictions on nested side chats and review mode, and the difference between side chat and a Git worktree

### Requirement: Scope and cost preservation

The change SHALL preserve already-used Goals, exclude Fast-mode activation and exclude native global memory generation. Acceptance model calls SHALL use explicitly recorded existing subscription routes and bounded cases, and MUST NOT introduce an automatic additional call on every normal task.

#### Scenario: Trial is installed globally
- **WHEN** the selected context workflow is installed and checked from another repository
- **THEN** no Fast setting, native global memory generation or non-Astra OpenAI assignment is introduced by the change
