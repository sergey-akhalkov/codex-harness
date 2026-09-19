## Purpose

Make skill-library growth and each skill's observed invocations inspectable, so a person or agent can see project skills before globally available ones and receive disable or retirement candidates with reasons.

## ADDED Requirements

### Requirement: Local invocation ledger

The system SHALL persist an installation-owned invocation ledger outside Git and outside skill discovery. Each recorded invocation SHALL identify the skill name, canonical path, revision, scope, invocation kind, repository or worktree, session identity, parent or child agent role, and timestamp. Catalogue presentation without body load or explicit invocation MUST NOT be recorded as a call. The ledger SHALL NOT require a model call, MUST NOT store skill bodies, transcripts or secrets, and MUST NOT be treated as a second skill registry.

#### Scenario: Explicit invocation is recorded
- **WHEN** a user or client invokes an owned skill with `$name` or a skill input item
- **THEN** the ledger stores an explicit invocation with time, current project or global scope, and parent or child role

#### Scenario: Implicit body load is recorded
- **WHEN** Codex loads a skill's `SKILL.md` because the task matched its description
- **THEN** the ledger stores an implicit invocation distinct from catalogue presence

#### Scenario: Catalogue-only presence is not a call
- **WHEN** a skill's name and description appear in the compact catalogue and the body is not loaded
- **THEN** that event is available as catalogue cost and MUST NOT update last invocation

### Requirement: Honest last invocation and coverage

For each listed skill the command and analysis skill SHALL report last invocation time and age, or an explicit `unknown` / `not_observed` state. Last invocation SHALL be the later of the last explicit invocation and the last implicit body load under recorded coverage. Missing, truncated or pre-ledger history SHALL be `unknown`, not «never used». A skill with complete coverage in the declared window and no invocation MAY be `not_observed` for that window only.

#### Scenario: Ledger did not exist yet
- **WHEN** a skill predates the ledger and no covered invocation is stored
- **THEN** last invocation is `unknown` and the row MUST NOT claim the skill was never called

#### Scenario: Covered window has no call
- **WHEN** coverage for the declared window is complete and neither explicit nor implicit invocation occurred
- **THEN** last invocation is `not_observed` for that window, with the window bounds visible

### Requirement: Project-first usage command

The installed CLI SHALL provide `codex-harness skills usage` as a model-free report of the effective library for the current working directory. When that directory is a project, the report SHALL list that project's skills first, then only globally available skills. A single canonical source reached through both a project path and a global link SHALL appear once, in the project section. Globally available skills already shown as project skills MUST NOT be repeated. When there is no project scope, the report SHALL emit only the global section and say that project skills are absent. Each row SHALL include name, scope, enabled state, last invocation or coverage state, invocation kind of that last call when known, where it ran, and parent or child role. The command MUST NOT mutate the library.

#### Scenario: Command runs inside a repository
- **WHEN** `codex-harness skills usage` is invoked from a Git project that has repo skills and the session also has distinct global skills
- **THEN** the output lists the project skills first and the remaining globally available skills after them

#### Scenario: Same canonical skill is linked globally
- **WHEN** a project skill is the same canonical revision as a global registration
- **THEN** it appears only in the project section

#### Scenario: Command runs outside a project
- **WHEN** the working directory is not a project with repo skills
- **THEN** the output contains the global section only and states that no project skill section applies

### Requirement: On-demand analysis skill

The kit SHALL install a global `skills-usage-analysis` skill. It SHALL run when the user asks about skill usage, growth, unused skills, last invocation, or what to disable or delete. It SHALL invoke `codex-harness skills usage` for the current project and present that grouped report. It MUST NOT run at every session start or after every ordinary task. Its description MUST NOT match unrelated implementation work.

#### Scenario: User asks what is unused
- **WHEN** the user asks which skills are unused or whether the library is growing without calls
- **THEN** the agent reads `skills-usage-analysis`, runs the usage command, and shows the project-then-global report

#### Scenario: Ordinary coding task
- **WHEN** the user asks to implement or fix project code without asking about the skill library
- **THEN** `skills-usage-analysis` is not selected and no usage model-backed review starts

### Requirement: Immediate disable or retirement candidates

Every analysis presentation SHALL include a candidates section immediately after the usage table, grouped in the same project-then-global order. Each candidate SHALL name the owned skill, the proposed action (`disable` from discovery or `retire` after evaluation), and the observed reason: last invocation age or `not_observed` under complete coverage, catalogue cost without calls, or growth without matching use. Candidates MUST NOT be applied until the user confirms. Confirmed `disable` SHALL be reversible catalogue removal. Physical deletion SHALL remain a separate ownership- and consumer-checked operation. The section MAY be empty only when it states why no owned skill qualifies.

The workflow MUST NOT propose system, third-party, foreign, explicitly disabled, OpenSpec-external, or protected required skills. Incomplete coverage (`unknown` last invocation) MUST NOT by itself make a deletion candidate. Lack of recent use still MUST NOT auto-publish a retirement.

#### Scenario: Project skill sits in the catalogue without a covered call
- **WHEN** coverage for the window is complete, an owned project skill was catalogue-presented, and it has no explicit or implicit invocation
- **THEN** it appears as a project-section disable candidate with that reason before any global candidates

#### Scenario: Last invocation is unknown
- **WHEN** an owned skill has `unknown` last invocation because the ledger does not cover its history
- **THEN** it is not listed as a deletion candidate solely for that unknown

#### Scenario: User confirms a candidate
- **WHEN** the user accepts a proposed disable of an owned non-protected skill
- **THEN** the skill is removed from effective discovery reversibly and the package remains recoverable outside discovery

### Requirement: Uncontrolled growth report

The usage command and analysis skill SHALL report whether owned enabled-skill count or catalogue admission grew during the declared window while unused owned skills existed or catalogue overflow or omission occurred. A large library with observed invocations MUST NOT be labelled uncontrolled solely by size. Growth figures SHALL stay distinct from invocation counts.

#### Scenario: Adds without calls and catalogue pressure
- **WHEN** owned enabled skills were added in the window, at least one owned skill is `not_observed` under complete coverage, and the catalogue overflowed or omitted entries
- **THEN** the report states uncontrolled growth with those facts

#### Scenario: Library is large but used
- **WHEN** owned skill count is high and listed skills have observed invocations in the window
- **THEN** the report does not call that growth uncontrolled solely because the count is high
