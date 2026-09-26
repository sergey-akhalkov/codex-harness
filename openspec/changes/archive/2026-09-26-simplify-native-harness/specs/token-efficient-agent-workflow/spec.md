## MODIFIED Requirements

### Requirement: Actual task-appropriate reasoning

The kit SHALL expose and use actual native reasoning settings for bounded task complexity and risk, with an explicit conservative default and escalation on demonstrated uncertainty. Selection MUST preserve Astra-only OpenAI assignments, the model-selection and recovery contract owned by agent delegation, and existing subscription routes. Instructions requesting brevity alone SHALL NOT count as lowering reasoning effort. Unsupported in-turn switching MUST be reported accurately; explicit user model and effort overrides MUST be respected.

Owned guidance SHALL use native `model_reasoning_effort` settings or the supported native per-agent effort parameter as the canonical interface, without requiring another task-level vocabulary. Existing `--harness-effort` invocations SHALL remain compatible while they are supported: translation SHALL preserve their documented mapping, invalid-input behavior and explicit native/profile precedence without becoming a separate model-selection policy. This change SHALL retain that public spelling as compatibility input rather than silently remove it or require existing callers to migrate before they can work.

#### Scenario: Routine bounded task
- **WHEN** a task has clear inputs, low risk and a deterministic acceptance check
- **THEN** its native task configuration can select a lower supported reasoning effort, with its effective effort observable

#### Scenario: Difficult or high-risk task
- **WHEN** a task requires substantial reasoning or exposes a correctness blocker
- **THEN** a higher supported effort is selected without silently changing model family or billing

#### Scenario: Existing effort selector is still used
- **WHEN** a caller uses a supported legacy selector together with an explicit native effort or profile
- **THEN** the existing precedence and effective effort are preserved, while new guidance teaches only the native selection interface
