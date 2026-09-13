## ADDED Requirements

### Requirement: Official Astra guidance coverage

The pack SHALL maintain a traceable audit of its instructions and documentation against available, applicable official OpenAI guidance for GPT-6 Astra. The audit SHALL identify source URLs, retrieval dates, model and product applicability, recommendations and affected repository surfaces. Discovery SHALL cover official documentation indexes, model guidance, developer articles and relevant linked guides rather than one article or search snippets. Unavailable sources and conflicting or uncertain claims SHALL remain explicit. API-specific advice SHALL NOT be represented as observed Codex CLI behavior without verification.

#### Scenario: Maintainer reviews completeness
- **WHEN** the adaptation is presented as complete
- **THEN** the audit identifies the searched official entry points, fetched relevant sources and their coverage, with no unresolved applicable source gap silently treated as satisfied

#### Scenario: A guide uses an unrelated product example
- **WHEN** an official article demonstrates a domain-specific tool or an API-only feature
- **THEN** its applicability is recorded and only supported relevant guidance is adapted, without imposing the example's tools, language or endpoint on every consumer

### Requirement: Whole-repository adaptation accountability

The audit SHALL account for every instruction and documentation surface in the current repository, including hidden skill directories, supporting resources, agent prompts, source-embedded instructions, examples, tests and project-owned OpenSpec material including archives. Each surface SHALL have a retained, adapted, consolidated, or protected disposition with a reason and relevant source mapping. Repeated rules SHALL have an authoritative owner; retired documents SHALL preserve required facts and working references. Historical evidence SHALL NOT be rewritten to imply current validation. Existing user decisions, required acceptance and unfinished tasks SHALL remain intact unless the user explicitly changes them.

#### Scenario: A document needs no edit
- **WHEN** current guidance already satisfies the applicable recommendations
- **THEN** the audit records it as retained with its rationale rather than making cosmetic edits or omitting it from coverage

#### Scenario: Another active change owns related instructions
- **WHEN** adaptation intersects an unfinished change or pre-existing working-tree edits
- **THEN** the agent reconciles affected project-owned artifacts while preserving unrelated edits and all unfinished acceptance obligations

### Requirement: Protected OpenSpec ownership during adaptation

The adaptation SHALL NOT modify externally maintained OpenSpec instructions, skills, schemas, templates, workflow configuration, generated instruction copies, or their installed equivalents. This includes `.agents/skills/openspec-*/` and `openspec/config.yaml`. The prohibition SHALL NOT be circumvented by wrappers, alternate copies, or pack guidance intended to override those workflows. Project-owned specifications and change artifacts SHALL remain editable within the authorized change. External conflicts SHALL be reported with their concrete source and practical effect, distinct from pack-owned defects.

#### Scenario: External skill requires an extra confirmation
- **WHEN** an OpenSpec skill contains a confirmation requirement at odds with desired Astra autonomy
- **THEN** that skill remains unchanged and the audit records the limitation without claiming unconditional conformity across protected external instructions

#### Scenario: Ownership boundary is checked
- **WHEN** the adaptation is validated
- **THEN** comparison against the recorded starting state shows no task-authored changes to protected OpenSpec surfaces, including installed copies

### Requirement: Task-appropriate Astra instruction behavior

Pack-owned instructions SHALL provide precise skill triggers and task-scoped context routes, preserve relevant mandatory skill use, distinguish requirements from optional guidance, and respect controlling instructions and existing user authorization. They SHALL direct agents to finish the complete authorized outcome, incorporate mid-task steering without losing unfinished work, ask about consequential unknowns, communicate clearly, and delegate when supported and worthwhile. They SHALL NOT introduce repeated approval of authorized routine actions, unconditional context inventories, or broad repeated verification without a task-specific reason. Real authority boundaries and required acceptance checks SHALL remain enforced.

#### Scenario: Small authorized documentation fix
- **WHEN** an agent receives a reversible documentation correction with adequate context
- **THEN** it reads relevant material, makes the correction and runs applicable document checks without unrelated setup, application suites or repeated approval

#### Scenario: Continued implementation with a side question
- **WHEN** a user asks a side question or supplies a correction during authorized implementation
- **THEN** the agent incorporates or answers it and continues remaining authorized work and checks until completion or a concrete blocker

#### Scenario: Material scope or authority is missing
- **WHEN** the next action would materially change the promised outcome or exceed authorization
- **THEN** the agent asks a focused question while continuing independent authorized work and identifies any instruction that caused the pause

### Requirement: Evidence for delivered Astra adaptation

Acceptance SHALL distinguish source consistency, actual instruction loading and observed model behavior. Updated global instructions SHALL be verified in fresh sessions from at least two working directories, including a consumer repository with project instructions, using the existing installation lifecycle. Representative owned scenarios SHALL exercise relevant skill selection, routine completion without redundant approval, continuation after steering, justified clarification and proportionate verification on GPT-6 Astra. Evidence SHALL retain reproducible inputs and observed outcomes without publishing private runtime data. Unexecuted or failed checks SHALL remain unfinished; static checks or agent-written success statements SHALL NOT substitute for behavioral evidence. Changed behavior SHALL preserve documented update and rollback operation and unrelated local settings.

#### Scenario: Static checks pass but model probes did not run
- **WHEN** document and loading checks succeed but behavioral scenarios are unavailable or unexecuted
- **THEN** the result reports those exact limits and leaves behavioral acceptance open

#### Scenario: New sessions consume updated instructions
- **WHEN** the existing global connection exposes revised source to fresh consumer sessions
- **THEN** retained native evidence identifies the effective instructions and model while existing sessions are not claimed to have reloaded automatically
