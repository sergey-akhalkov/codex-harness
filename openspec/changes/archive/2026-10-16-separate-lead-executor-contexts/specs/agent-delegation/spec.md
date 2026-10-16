# Spec Delta

## MODIFIED Requirements

### Requirement: Universal capability levels

The kit SHALL support direct native selection of an explicit provider profile, model and compatible reasoning effort per assignment without requiring separate named agent definitions for each model or effort. Reusable `lead` and `executor` responsibility roles SHALL supply independently maintained instruction and skill contexts under `agent-role-contexts`; the existing orchestration configuration SHALL continue to own executor routing rather than fixed provider roles. Effective responsibility role and model identities SHALL be observable in each conversation view. Only efforts supported by the selected model SHALL be accepted; unsupported combinations SHALL NOT be silently substituted. Redundant supplied Grok middle and Astra middle reserve, senior and principal presets SHALL remain retired with the documented migration to model/effort arguments, preserving user-owned agents, skills and native GPT availability. All OpenAI assignments, including auxiliary calls and reserves, SHALL use only the Astra family, with GPT-5 excluded. Responsibilities and relevant skills SHALL select activities without profession-specific agents. The retired `grok_reviewer` SHALL remain retired.

#### Scenario: One level performs different activities
- **WHEN** a configured executor receives an implementation assignment and separately a review or research assignment within its capabilities
- **THEN** it performs each activity with the executor responsibility role, applicable skills and evidence without requiring a separate profession-specific agent

#### Scenario: Native model selection is verified
- **WHEN** an executor is launched with its configured profile and a supported effort from a fresh global Codex session outside the kit repository
- **THEN** native metadata identifies its effective model and reasoning, existing run inspection identifies its executor role context, and the profile's subscription serves an actual response

#### Scenario: A requested effort is unsupported
- **WHEN** an assignment requests an effort absent from the selected model's verified contract
- **THEN** dispatch reports the unsupported combination before a model request, without silently choosing another model or effort

#### Scenario: Redundant presets are removed
- **WHEN** the kit updates an installation that used supplied fixed agent profiles
- **THEN** its own obsolete definitions and references are retired, direct model/effort selection remains usable, unrelated user agents and `.agents/skills` remain intact, and responsibility-role delivery does not recreate the retired provider/effort presets

#### Scenario: Executor guidance changes independently
- **WHEN** the executor role is revised without changing model routing or the common core
- **THEN** new executor sessions load the new guidance while the lead role, shared safeguards and configured model/effort remain unchanged
