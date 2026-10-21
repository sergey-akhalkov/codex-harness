# Spec Delta

## MODIFIED Requirements

### Requirement: Complete portable kit source

The repository SHALL own the complete first-party source of its declared global toolkit: the full harness-owned global AGENTS.md content, shared configuration, owned skills and their referenced resources, managed agent definitions, connection/launch tools and dependency declarations. Ponytail instructions and skills SHALL remain in the original external package identified by an immutable declared upstream revision. The host-global AGENTS.md SHALL directly reference that full repository-owned supplemental instruction source. A fresh checkout with documented external prerequisites SHALL be sufficient to reproduce the declared kit without retrieving undeclared files from another PC. Connection and managed resource paths SHALL resolve on the current machine. A catalogue entry or dependency declaration SHALL not imply an unimplemented capability is delivered.

#### Scenario: The kit is deployed on a new PC
- **WHEN** a fresh checkout is connected on another Windows account with the documented prerequisites
- **THEN** every declared managed capability and its required resources are available from the checkout or an explicitly declared external dependency, without depending on files from the former PC; machine-specific settings remain local and the shared source contains only portable defaults

#### Scenario: The global instruction source is inspected
- **WHEN** the user follows the active global AGENTS.md connection
- **THEN** the complete harness-owned instruction content is stored under version control in this repository and appears in new sessions through that connection, while declared Ponytail guidance is consumed separately from its verified package without a second maintained instruction body

### Requirement: Direct source consumption

The kit SHALL connect its repository-owned configuration, supplemental portable instructions, skills, agent definitions, MCP connection definitions, language integration definitions and hooks by supported path references or filesystem links. Codex and the corresponding integration consumer SHALL read the repository source when consuming those data artifacts. The installer MUST NOT copy, render, merge or cache their contents into deployment files, including as a fallback. Small host-local path registrations and installation metadata SHALL contain references and ownership information rather than duplicated artifact bodies. Compiled first-party executables SHALL be distinguished from source-owned data: installation SHALL connect verified build artifacts with recorded source/build identity, while their complete source remains in the checkout. This executable-artifact allowance MUST NOT permit embedding deployed copies of source-owned configuration, instructions or skills. External packages, language runtimes and generated runtime state SHALL remain distinct from repository-owned configuration and integration source; their installation SHALL not be used to smuggle deployed copies of those artifacts into a package cache. The original Ponytail package SHALL be consumed through the native plugin lifecycle from its verified installed revision; this external-package allowance SHALL NOT permit copying harness or upstream instruction bodies into another policy owner.

#### Scenario: A clean host is connected
- **WHEN** the installer successfully connects a valid checkout
- **THEN** every managed first-party data artifact resolves to its source in that checkout without a deployed content copy, and every managed compiled executable resolves to a verified build identified with that checkout's relevant sources

#### Scenario: Links or another required capability are unavailable
- **WHEN** a required direct connection cannot be established
- **THEN** installation fails with the specific cause and a corrective action, without substituting copied artifacts or reporting partial activation as success

#### Scenario: MCP or diagnostic integration source is updated
- **WHEN** a connected MCP definition, language integration definition or hook definition changes and a new consumer starts
- **THEN** the consumer reads the updated checkout data through its existing connection without a generated deployment copy, while a changed compiled implementation follows the verified build/update contract

#### Scenario: An external plugin is installed
- **WHEN** ordinary installation provisions the declared Ponytail revision
- **THEN** Codex reads its upstream instructions, skills and hooks from the native installed package, installation records its actual identity, and harness-owned data remains directly connected to checkout source

### Requirement: Global capability discovery

The kit SHALL expose supplemental portable principles, the original Ponytail plugin and its skills, OpenSpec skills, managed agents and the accepted MCP/LSP/hook selection outside the harness while preserving project instructions and unrelated user capabilities. Discovery SHALL refer to live first-party source files or the verified external package and accurately distinguish installed, pending-trust, active, disabled, degraded and retired capabilities. An empty agent directory SHALL not imply an unrequested catalogue. Duplicate discovery SHALL not create conflicting copies. Base and profile consumers SHALL agree on hook suspension; retained global integrations SHALL not depend solely on a CLI-only profile when another supported installed consumer does not load it. Explicit native overrides SHALL remain visible.

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

A core install or update SHALL preserve the user's recorded hook selection, including disabled definitions and suspension. It SHALL update source targets on relocation, repair owned links, preserve foreign-file conflicts and transactional recovery, and SHALL NOT enable unrelated hooks on a fresh core-only installation. Ordinary core delivery SHALL include the default Ponytail selection; only its required hooks SHALL become eligible after native trust. Explicit prior disablement SHALL override this initial default, and repeated updates SHALL NOT reset that choice. Previously connected hooks SHALL NOT override a later suspension or retirement decision.

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
- **THEN** installation includes Ponytail and reports pending trust where required, without activating untrusted Ponytail hooks or enabling unrelated hooks

#### Scenario: Ponytail is disabled before an update
- **WHEN** a later core update repairs or relocates the plugin registration
- **THEN** explicit disablement remains effective and visible, and repair does not restore a default-enabled selection

### Requirement: Persistent global hook suspension

The kit SHALL support a recoverable suspension of all Codex lifecycle hooks across user/base configuration, the managed profile and installed hook definitions. Development without ordinary lifecycle hooks SHALL remain the reusable default after completion, update and archive. The specifically accepted RTK command-compression hook and the required Ponytail plugin hooks SHALL be the only kit-selected exceptions, with bounded execution and native trust. Ponytail plugin hooks SHALL retain their native non-managed trust classification; harness selection SHALL NOT bypass that review. Already-running sessions retaining cached diagnostic harness handlers SHALL perform no automatic diagnostic analysis, fallback scan, context injection or completion continuation. A fresh consumer outside the checkout SHALL observe only the accepted selection, with base and managed profile agreement. Machine-local markers and backups SHALL remain outside reusable source. A fresh core-only installation SHALL include Ponytail's default selection but SHALL NOT execute its hooks before native trust or reactivate retired diagnostic, context or Stop handlers.

#### Scenario: A cached session invokes an old handler
- **WHEN** an old automatic diagnostic handler is invoked
- **THEN** it returns without diagnostic work or feedback

#### Scenario: Another project starts a new session
- **WHEN** it consumes the installed base or managed profile with the accepted RTK and Ponytail selection
- **THEN** only those trusted selected exceptions are enabled by the kit, without project-local configuration or restored diagnostic and Stop handlers

#### Scenario: The optimization change is completed
- **WHEN** its tasks are closed, archived or followed by a kit update
- **THEN** the consumer retains the accepted RTK and Ponytail selections, including explicit disablement, and no backup or lifecycle label reactivates rejected hooks

#### Scenario: Explicit suspension or disconnection
- **WHEN** the RTK or Ponytail capability is explicitly disabled or disconnected
- **THEN** that capability's hooks are inactive, rollback remains recoverable, and the other accepted selection and unrelated user configuration are preserved

