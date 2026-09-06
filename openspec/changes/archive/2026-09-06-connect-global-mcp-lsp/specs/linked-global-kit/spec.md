## MODIFIED Requirements

### Requirement: Direct source consumption

The kit SHALL connect its configuration, portable instructions, skills, agent definitions, MCP connection definitions, language integration definitions and hooks by supported path references or filesystem links. Codex and the corresponding integration consumer SHALL read the repository source when consuming an artifact. The installer MUST NOT copy, render, merge or cache repository artifact contents into deployment files, including as a fallback. Small host-local path registrations and installation metadata SHALL contain references and ownership information rather than duplicated artifact bodies. External packages, language runtimes and generated runtime state SHALL be distinguished from repository-owned configuration and integration source; their installation SHALL not be used to smuggle deployed copies of those artifacts into a package cache.

#### Scenario: A clean host is connected
- **WHEN** the installer successfully connects a valid checkout
- **THEN** every managed artifact resolves to its source in that checkout and no deployed content copy is created

#### Scenario: Links or another required capability are unavailable
- **WHEN** a required direct connection cannot be established
- **THEN** installation fails with the specific cause and a corrective action, without substituting copied artifacts or reporting partial activation as success

#### Scenario: MCP or diagnostic integration source is updated
- **WHEN** a connected MCP definition, language integration or hook source changes and a new consumer starts
- **THEN** the consumer reads the updated checkout source through its existing connection without a generated deployment copy

### Requirement: Global capability discovery

The kit SHALL expose the existing portable principles, the repository's OpenSpec skills, repository-owned agent definitions and declared MCP/LSP/hook capabilities to sessions outside the harness while preserving applicable project instructions and unrelated user capabilities. Discovery SHALL refer to live source files. An empty agent source directory SHALL be reported accurately; supporting agent loading does not imply delivery of an unrequested agent catalogue. Duplicate discovery through repository and global paths SHALL not result in conflicting copies of the same managed source. Global MCP and automatic diagnostics activation SHALL not rely solely on a CLI-only profile when another supported installed local Codex consumer does not load it; native configuration consumption and explicit overrides SHALL be verified for each applicable entry point.

#### Scenario: Instructions and skills are consumed elsewhere
- **WHEN** Codex starts in an unrelated repository with its own AGENTS.md
- **THEN** the global principles and project instructions are in the initial context and the managed skills are discoverable from the checkout

#### Scenario: An agent definition is connected
- **WHEN** a valid repository-owned agent definition is registered
- **THEN** Codex discovers that definition globally and uses its repository source while unrelated personal agents remain discoverable

#### Scenario: Codex starts within the harness
- **WHEN** global registration and project discovery both reach a managed skill
- **THEN** there is one effective source identity or documented native deduplication, with no separately maintained content or ambiguous conflicting definition

#### Scenario: Global code tools are consumed elsewhere
- **WHEN** a new supported local Codex consumer starts in another project after global activation
- **THEN** the declared MCP/LSP capabilities and automatic edit diagnostics are available without project-local kit configuration, together with preserved unrelated capabilities
