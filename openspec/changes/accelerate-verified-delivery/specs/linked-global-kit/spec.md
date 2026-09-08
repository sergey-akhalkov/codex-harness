## ADDED Requirements

### Requirement: Deliver verification workflows through the global lifecycle

The installed skills and their resources MUST work without the opencode-kit
checkout. Previously recorded use of that project is historical acceptance
evidence, not a requirement for ongoing compatibility or additional test runs.

The linked kit SHALL deliver `project-verification`, `reproduce-regression` and their referenced reusable resources through its existing installation and discovery lifecycle. Skill bodies and resources MUST remain linked to authoritative reusable sources. Installation, source updates, reconciliation, rollback and disconnection MUST preserve existing ownership and collision guarantees. Machine-local case state and traces MUST remain outside tracked portable configuration.

#### Scenario: A new session starts outside the kit checkout

- **WHEN** the linked kit is activated and a new ordinary Codex session starts in another project
- **THEN** the session discovers both skills and can access their resources without being given absolute skill paths in the task prompt

#### Scenario: An installed source resource changes

- **WHEN** a referenced skill resource changes in its authoritative source
- **THEN** a new consuming session sees the source update through the existing link lifecycle without a copied deployment body

#### Scenario: Skill registration conflicts with a foreign installation

- **WHEN** a destination belongs to another installation
- **THEN** activation reports the collision and preserves that destination rather than overwriting it

#### Scenario: The kit is disconnected and reconnected

- **WHEN** the user exercises the installation lifecycle in an isolated acceptance environment
- **THEN** only kit-owned links are removed, source and unrelated user files survive, and reconnection restores discovery and resources

### Requirement: Prove actual outside-project consumption

Delivery acceptance MUST exercise project verification in two existing projects outside the harness checkout and regression reproduction in at least one of them through new native Codex sessions. Isolated worktrees or snapshots of real projects MAY be used to preserve ongoing work, but manufactured fixture-only repositories MUST NOT substitute for the two real consumers. Evidence SHALL identify the source state, actual invoked command, skill use, observed outcome and limitations. A lint-only consumer MUST count only for its demonstrated validation scope. Both workflows MUST satisfy their outcome requirements before the reusable capability is declared complete.

#### Scenario: A consumer has unrelated uncommitted work

- **WHEN** its acceptance case is prepared
- **THEN** the case uses recorded isolated source state without modifying or cleaning the user's live checkout or controlling its running services

#### Scenario: Discovery succeeds without an actual task

- **WHEN** installation tests find the skills but no outside-project behavior has been exercised
- **THEN** discovery is recorded as passing while reusable-delivery acceptance remains incomplete
