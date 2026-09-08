## Purpose

Make accepted skill revisions discoverable and usable during the current agent session, including continuation after context compaction, resumption and delegation to another agent.

## ADDED Requirements

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

### Requirement: Activation within the current session

After an accepted creation, update, consolidation or retirement, the workflow SHALL refresh the relevant catalogue and deliver the change to the current agent before its next applicable action. The agent SHALL read the accepted skill instructions before using them and observe the current revision after updates or rollback. Both an immediate continuation in the same turn and the next matching user task SHALL work without a user-supplied skill name or restarting the session. A file write, catalogue refresh or announcement alone SHALL not count as verified activation.

#### Scenario: Skill is created and immediately needed
- **WHEN** an accepted new skill is relevant to work remaining in the current turn
- **THEN** the agent discovers and reads it, applies it to that work and produces the expected observable result without an explicit user invocation

#### Scenario: Next task uses an updated revision
- **WHEN** a skill is updated or rolled back and a later task in the same session matches its scope
- **THEN** the agent uses the accepted current revision and its resources rather than retained instructions from the superseded version

#### Scenario: Similar task is outside the skill's scope
- **WHEN** a new request resembles the skill's description but falls outside its declared use
- **THEN** the skill does not displace the applicable workflow merely because it was recently created

### Requirement: Recovery after compaction and resume

The system SHALL restore skill awareness after manual compaction, automatic compaction during a turn and session resume, using current source identities rather than relying solely on the previous conversation summary. Restoration SHALL happen before the next relevant model continuation, including mid-turn automatic compaction. Deleted, disabled, retired or superseded skills SHALL not be revived from stale session metadata. The restored context SHALL be compact and load skill bodies only when relevant.

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

### Requirement: Bounded delivery and honest compatibility

Awareness delivery SHALL use supported mechanisms verified on the installed target runtime, keep per-event work bounded and avoid unconditional model calls. Repeated unchanged events SHALL not repeatedly inject the full catalogue. A failing delivery mechanism SHALL report its exact limit and use an available direct refresh/read path where possible; an unsupported required scenario SHALL remain incomplete. New-process discovery or App Server-only success MUST NOT substitute for required behavior in the ordinary installed CLI session.

#### Scenario: Native catalogue cache does not refresh
- **WHEN** a running CLI does not expose a newly accepted skill through its cached selector
- **THEN** the workflow uses a verified current-session delivery and direct-read path to meet the behavior requirement, or reports activation incomplete without silently substituting a restart

#### Scenario: Repeated unchanged hook events
- **WHEN** successive tool events observe the same scoped catalogue revision
- **THEN** they perform bounded freshness checks without repeated full-context injection or model-backed maintenance
