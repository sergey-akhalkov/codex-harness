## MODIFIED Requirements

### Requirement: Existing dependency discovery and reuse

The kit SHALL discover existing installations before proposing installation or update. Its inventory SHALL identify the package or executable, actual version, resolved path, installation manager or owner, active consumer and health evidence. It SHALL reuse a working compatible installation and its applicable language-server cache instead of creating another permanent installation solely for Codex. Ambiguous command names and locally modified installations MUST NOT be selected or replaced silently.

#### Scenario: Tools are already installed for OpenCode
- **WHEN** discovery finds existing shared MCP and language-server installations
- **THEN** the proposed Codex connection uses those resolved dependencies and identifies which require an update or repair

#### Scenario: Two packages expose the same command name
- **WHEN** an unrelated package exposes the same command name as an intended MCP distribution
- **THEN** selection verifies the intended distribution and uses its resolved executable rather than accepting the first same-named command

#### Scenario: A dependency contains local modifications
- **WHEN** discovery detects a modified installation or cannot establish safe ownership for an update
- **THEN** it reports the uncertainty and preserves the installation until a compatible preservation or migration path is established
