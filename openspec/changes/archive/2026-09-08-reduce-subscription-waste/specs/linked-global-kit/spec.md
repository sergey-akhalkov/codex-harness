## ADDED Requirements

### Requirement: Persistent global hook suspension

The kit SHALL support a recoverable suspension of all Codex lifecycle hooks across user/base configuration, the managed profile and installed hook definitions. The recorded suspension SHALL remain effective except for a separately authorized and accepted narrow scope. Development without hooks SHALL remain the reusable default after completion, update and archive; only a specifically accepted narrow exception may change its selected scope. Already-running sessions retaining cached diagnostic harness handlers SHALL perform no automatic diagnostic analysis, fallback scan, context injection or completion continuation. A fresh consumer outside the checkout SHALL observe the disabled default or only the specifically accepted exception, with base and profile agreement. Machine-local markers and backups SHALL remain outside reusable source; the reusable disabled default SHALL be maintained in the kit.

#### Scenario: A cached session invokes an old handler
- **WHEN** global suspension is active
- **THEN** the handler returns without diagnostic work or feedback

#### Scenario: Another project starts a new session
- **WHEN** it consumes the installed base or managed profile
- **THEN** lifecycle hooks are disabled without project-local configuration unless a separately accepted narrow exception is selected; rejected diagnostic and Stop handlers remain inactive

#### Scenario: The optimization change is completed
- **WHEN** its tasks are closed, archived or followed by a kit update without a specifically accepted exception
- **THEN** the consumer still receives the hooks-disabled default and no backup or lifecycle label reactivates hooks

### Requirement: Accepted capability selection survives lifecycle operations

Install, update, repair, source relocation, recovery and disconnect SHALL preserve explicit suspension and the accepted retained/retired capability selection. No lifecycle operation or implementation-complete label SHALL silently restore a rejected hook, LSP backend or LSP carrier. Shared external package removal SHALL require verified ownership and absence of other consumers; disconnecting only Codex registrations SHALL be sufficient when packages are shared. Recovery SHALL preserve user edits and credentials.

#### Scenario: Update repairs links after suspension
- **WHEN** a core or code-tools update repairs managed links
- **THEN** repaired links still consume the disabled selection and cannot revive previous automatic diagnostics

#### Scenario: Shared language installation has another consumer
- **WHEN** its Codex LSP integration is retired
- **THEN** that registration is removed while the shared installation and other application's configuration are preserved

## MODIFIED Requirements

### Requirement: Global capability discovery

The kit SHALL expose portable principles, OpenSpec skills, managed agents and the accepted MCP/LSP/hook selection outside the harness while preserving project instructions and unrelated user capabilities. Discovery SHALL refer to live source files and accurately distinguish delivered, disabled and retired capabilities. An empty agent directory SHALL not imply an unrequested catalogue. Duplicate discovery SHALL not create conflicting copies. Base and profile consumers SHALL agree on hook suspension; retained global integrations SHALL not depend solely on a CLI-only profile when another supported installed consumer does not load it. Explicit native overrides SHALL remain visible.

#### Scenario: Instructions and skills are consumed elsewhere
- **WHEN** Codex starts in another repository with its own instructions
- **THEN** managed instructions and skills remain available from their owning source

#### Scenario: An agent definition is connected
- **WHEN** a managed agent is registered
- **THEN** it is discovered alongside preserved personal agents without conflicting source copies

#### Scenario: Global code tools are consumed elsewhere
- **WHEN** it starts after activation
- **THEN** retained capabilities are available and suspension or intentional retirement is reported without claiming automatic diagnostics

#### Scenario: Codex starts within the harness
- **WHEN** global and project discovery both reach a managed skill
- **THEN** there is one effective source identity or verified native deduplication without conflicting copies or duplicate instruction loading

### Requirement: Core updates preserve connected hooks

A core install or update SHALL preserve the user's recorded hook selection, including disabled definitions and suspension. It SHALL update source targets on relocation, repair owned links, preserve foreign-file conflicts and transactional recovery, and SHALL NOT enable hooks on a fresh core-only installation. Previously connected hooks SHALL NOT override a later suspension or retirement decision.

#### Scenario: Update an installation with diagnostics
- **WHEN** hooks are selected and have passed the benefit gate
- **THEN** their owned links remain usable and recorded through repeated updates

#### Scenario: Relocate or repair recorded connections
- **WHEN** a recorded hook link is missing or the checkout moves
- **THEN** repair preserves suspension instead of restoring previously active definitions

#### Scenario: Foreign replacement at a hook destination
- **WHEN** another file replaces a managed hook link
- **THEN** update reports the ownership conflict without overwriting it or losing state

#### Scenario: Fresh core installation
- **WHEN** only the core is installed
- **THEN** installation does not activate hooks

### Requirement: Hook and MCP diagnostic runtime agreement

When diagnostic transports are retained, command and native MCP handlers using the same installed registry SHALL share a compatible runtime identity and mutation delivery ownership. Suspension SHALL be honored by both, including previously loaded clients, without declaring that a skipped check passed. Runtime transition SHALL use owned graceful retirement and preserve unrelated services and in-flight explicit operations.

#### Scenario: Native runtime exists during suspension
- **WHEN** an old command or native hook calls it
- **THEN** no analysis or diagnostic output occurs

#### Scenario: Native diagnostics broker already running
- **WHEN** suspension has been lifted for a benefit-proven capability
- **THEN** the created/modified supported file receives one current scoped result without a false runtime mismatch
