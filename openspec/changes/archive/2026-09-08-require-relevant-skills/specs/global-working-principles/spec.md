## ADDED Requirements

### Requirement: Mandatory use of relevant available skills

The global instructions SHALL require main agents and tool-capable subagents to assess available skills for their actual task and use each applicable, nonredundant workflow. Use SHALL include reading the skill instructions and following their relevant process, not merely naming the skill. Selection SHALL be reconsidered when task context changes. Task familiarity or the ability to use ordinary tools SHALL NOT justify skipping an applicable skill.

#### Scenario: Task matches an available skill
- **WHEN** the current task matches a skill's stated scope and invocation conditions
- **THEN** the agent reads and applies it without requiring a separate user reminder, announces first use, and loads only relevant supporting resources

#### Scenario: Several skills apply to different parts
- **WHEN** complementary skill workflows cover different parts of the accepted task
- **THEN** the agent applies those workflows while avoiding duplicate equivalent procedures

#### Scenario: Keyword overlap or explicit-only skill
- **WHEN** a skill only shares a keyword with the task or its explicit invocation condition has not been met
- **THEN** the agent does not activate it solely to satisfy the mandatory-use rule

#### Scenario: Skill conflicts with authorization or cannot be accessed
- **WHEN** skill guidance conflicts with a higher-priority instruction or a relevant skill cannot be read or used
- **THEN** the agent reports the concrete conflict or limitation, preserves the controlling instruction, and continues safe authorized work where possible without silently claiming skill use

#### Scenario: Ordinary session starts in another project
- **WHEN** a new session loads the linked global AGENTS.md outside the harness
- **THEN** the mandatory skill-use rule is present alongside the applicable local project instructions and covers child agents
