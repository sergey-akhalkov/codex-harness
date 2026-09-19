# Skill Session Awareness Specification

## Purpose

Make accepted skill revisions discoverable and usable during the current agent session, including continuation after context compaction, resumption and delegation to another agent.

## Requirements

### Requirement: Derived scoped skill catalogue

The system SHALL derive a compact catalogue from effective active skill sources for the current repository and user, identifying each skill's name, applicability, canonical path and revision. It SHALL preserve explicit disablement, distinguish conflicting sources from duplicate links to one source, and exclude candidates, retired packages and foreign project-only skills. The catalogue SHALL be regenerable without a second manually maintained registry. Partial discovery or context-budget truncation SHALL be explicit and retain a bounded retrieval route to omitted entries; it MUST NOT claim the complete active set is present.

#### Scenario: Same source appears through local and global links
- **WHEN** discovery reaches a managed skill by two paths to the same canonical source
- **THEN** the catalogue identifies one source revision without creating competing maintained copies

#### Scenario: Different sources declare the same name
- **WHEN** an active project skill and a different global source have the same name
- **THEN** the conflict is reported with distinct source identities and neither is silently assumed to override the other

#### Scenario: Catalogue exceeds the context allowance
- **WHEN** the active set cannot fit the compact delivery budget
- **THEN** the session receives an explicit incomplete-list indication and a usable route to discover the remaining relevant entries without loading every skill body

### Requirement: Measured catalogue admission budget

The workflow SHALL declare and measure a catalogue budget for its owned active skills in a stated scope and unit. Admission SHALL account for the complete effective metadata of those skills before presentation truncation, including names, descriptions, paths and required delivery overhead; body/reference loading SHALL be accounted for separately when it occurs. Duplicate source entries SHALL not inflate the count, and external or protected skills SHALL remain visible as outside the editable allocation. Automatic shortening, omitted entries or moving metadata into a second always-loaded list MUST NOT be used to hide growth. Changes to applicability or descriptions SHALL pass behavioral evaluation.

An addition or growth of the managed active catalogue SHALL require room in the budget or an evaluated replacement/consolidation that makes room. Catalogue pressure alone SHALL NOT authorize retirement, loss of required discovery or rewriting protected skills. If an existing baseline already exceeds the allocation, the workflow SHALL report overflow, preserve usable discovery through the bounded route, block further managed growth and permit evaluated changes that reduce the excess. Missing measurement or unsupported discovery control SHALL remain explicit and SHALL NOT count as successful bounded admission. Numeric settings SHALL be calibrated against actual delivery and task selection before automatic admission is enabled.

#### Scenario: A useful candidate does not fit
- **WHEN** a candidate passes behavioral evaluation but would grow the owned catalogue beyond its allowance
- **THEN** activation remains pending until an evaluated change creates capacity; the budget is not raised automatically and another skill is not silently removed

#### Scenario: A preexisting library exceeds the allocation
- **WHEN** the measured owned baseline is already over budget
- **THEN** normal skill access remains usable with explicit overflow, further growth is blocked, and only evaluated reductions can reduce the excess without destructive automatic pruning

### Requirement: Activation within the current session

After an accepted creation, update, consolidation or retirement, the workflow SHALL refresh the relevant catalogue and deliver the change to the current agent before its next applicable action. The agent SHALL read the accepted skill instructions before using them and observe the current revision after updates or rollback. Both an immediate continuation in the same turn and the next matching user task SHALL work without a user-supplied skill name or restarting the session. A file write, catalogue refresh or announcement alone SHALL not count as verified activation. A new process started as orchestrated instruction-refresh succession SHALL NOT be counted as this requirement's current-session activation.

#### Scenario: Skill is created and immediately needed
- **WHEN** an accepted new skill is relevant to work remaining in the current turn
- **THEN** the agent discovers and reads it, applies it to that work and produces the expected observable result without an explicit user invocation

#### Scenario: Next task uses an updated revision
- **WHEN** a skill is updated or rolled back and a later task in the same session matches its scope
- **THEN** the agent uses the accepted current revision and its resources rather than retained instructions from the superseded version

#### Scenario: Similar task is outside the skill's scope
- **WHEN** a new request resembles the skill's description but falls outside its declared use
- **THEN** the skill does not displace the applicable workflow merely because it was recently created

#### Scenario: Retirement takes effect in the current session
- **WHEN** an accepted retirement removes a skill previously loaded by a running agent
- **THEN** before the next relevant action the agent receives the retirement and uses the evaluated remaining workflow without reading retired resources or reviving the skill from remembered metadata
- **AND** a complete retirement claim requires observed future behavior as well as registration change; it does not claim that already loaded tokens were erased or refunded

### Requirement: Recovery after compaction and resume

The system SHALL restore skill awareness after manual compaction, automatic compaction during a turn and session resume, using current source identities rather than relying solely on the previous conversation summary. Restoration SHALL happen before the next relevant model continuation, including mid-turn automatic compaction. Deleted, disabled, retired or superseded skills SHALL not be revived from stale session metadata. The restored context SHALL be compact and load skill bodies only when relevant. Orchestrated process replacement through `codex resume` succession is owned by `orchestrate-feedback-and-pacing` and SHALL NOT satisfy this same-session compact or in-process activation requirement.

Owned CLI evidence established this outcome through SessionStart/UserPromptSubmit/PostToolUse additionalContext. Ordinary diagnostic, context and Stop hooks remain disabled, so that demonstrated path is not currently installable. A supported hooks-off replacement MUST be proven, or this requirement remains an owning blocker. Restoring ordinary hooks, substituting App Server-only success, or treating a new process as sufficient SHALL NOT close the requirement.

#### Scenario: Automatic compaction interrupts ongoing work
- **WHEN** automatic compaction occurs after a skill was created and before the current task's next use of it
- **THEN** the immediate continuation recovers the catalogue and applies the current skill without waiting for another user message

#### Scenario: Source changes while a session is suspended
- **WHEN** a resumed session's previous catalogue contains a skill that has since been replaced or disabled
- **THEN** restoration reconciles current sources and does not apply the stale or disabled revision

### Requirement: Fresh skill context for delegated work

Delegated work SHALL receive enough relevant skill identity and location information to apply the current accepted revision in the child's actual workspace. Children without inherited context SHALL discover or load that revision independently. Existing children SHALL reconcile an affected revision before their next use; results based on a prior revision SHALL retain that attribution rather than being relabeled as current. Delegation SHALL not inject unrelated project knowledge or the entire skill library.

#### Scenario: A fresh child receives a newly created skill
- **WHEN** the parent delegates a matching task after activation with no conversation fork
- **THEN** the child resolves the skill in its own project context, reads the accepted instructions and demonstrates their application

#### Scenario: Child is running during an update
- **WHEN** an affected skill changes while a child is active
- **THEN** the child checks or receives the new revision before its next use, and any earlier result remains attributed to the revision actually used

#### Scenario: A retired skill remains in a child's old context
- **WHEN** the parent retires a skill while an affected child is still running
- **THEN** the child reconciles the retirement before its next use and continues with the evaluated remaining workflow; prior results keep their original attribution

### Requirement: Bounded delivery and honest compatibility

Awareness delivery SHALL use supported mechanisms verified on the installed target runtime, keep per-event work bounded and avoid unconditional model calls. Repeated unchanged events SHALL not repeatedly inject the full catalogue. A failing delivery mechanism SHALL report its exact limit and use an available direct refresh/read path where possible; an unsupported required scenario SHALL remain incomplete. New-process discovery or App Server-only success MUST NOT substitute for required behavior in the ordinary installed CLI session.

Supported here means a currently allowed ordinary-CLI mechanism. Official Codex documentation still lists SessionStart sources `startup`, `resume`, `clear` and `compact`; that documented contract does not authorize restoring ordinary hooks under the durable hooks-off default.

#### Scenario: Native catalogue cache does not refresh
- **WHEN** a running CLI does not expose a newly accepted skill through its cached selector
- **THEN** the workflow uses a verified current-session delivery and direct-read path to meet the behavior requirement, or reports activation incomplete without silently substituting a restart

#### Scenario: Repeated unchanged hook events
- **WHEN** successive tool events observe the same scoped catalogue revision
- **THEN** they perform bounded freshness checks without repeated full-context injection or model-backed maintenance
- **AND** ordinary diagnostic, context and Stop hooks remain disabled; the accepted RTK exception is unchanged
