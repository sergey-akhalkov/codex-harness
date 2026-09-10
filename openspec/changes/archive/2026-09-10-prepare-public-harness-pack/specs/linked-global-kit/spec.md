## MODIFIED Requirements

### Requirement: Complete portable kit source

The repository SHALL own the complete source of its declared global toolkit: the full global AGENTS.md content, shared configuration, managed skills and their referenced resources, managed agent definitions, connection/launch tools and dependency declarations. The host-global AGENTS.md SHALL directly reference that full repository-owned instruction source. A fresh checkout with documented external prerequisites SHALL be sufficient to reproduce the declared kit without retrieving undeclared files from another PC. Connection and managed resource paths SHALL resolve on the current machine. A catalogue entry or dependency declaration SHALL not imply an unimplemented capability is delivered.

#### Scenario: The kit is deployed on a new PC
- **WHEN** a fresh checkout is connected on another Windows account with the documented prerequisites
- **THEN** every declared managed capability and its required resources are available from the checkout or an explicitly declared external dependency, without depending on files from the former PC; machine-specific settings remain local and the shared source contains only portable defaults

#### Scenario: The global instruction source is inspected
- **WHEN** the user follows the active global AGENTS.md connection
- **THEN** the complete instruction content is stored under version control in this repository and appears in new sessions through that connection

### Requirement: Shared configuration and local state

The shared configuration SHALL provide `approval_policy = "never"` and `sandbox_mode = "danger-full-access"` as the previously selected Full Access default and SHALL record the declared reusable model, reasoning and tool preferences needed to reproduce the kit. Existing local configuration outside explicitly managed defaults SHALL remain active under native configuration precedence. Authentication, session history, caches and installer state SHALL remain outside tracked repository artifacts. The entire checkout MUST NOT become `CODEX_HOME`. Native configuration-management operations SHALL persist user-specific model preferences, trust paths and other machine state outside tracked source files. Shared defaults SHALL continue to be read live from the checkout. Installation and migration SHALL preserve local configuration, authentication and unrelated capabilities, retain a recoverable copy of relocated local settings, and report conflicting values rather than discard them.

#### Scenario: Existing machine configuration is present
- **WHEN** a host with provider/model preferences, trusted project paths and saved authentication is connected
- **THEN** those unrelated settings and data remain available and the shared Full Access defaults are effective for a new normal session

#### Scenario: A setting is persisted through its native writer
- **WHEN** a supported ordinary Codex operation persists a model preference or machine-specific setting after connection
- **THEN** the setting persists on the local machine, the shared source remains unchanged, and a subsequent session observes applicable local preferences together with the live portable defaults

#### Scenario: A previously connected shared profile contains machine state
- **WHEN** the installation is migrated to the public-source boundary
- **THEN** existing machine settings are preserved locally before their tracked source entries are removed, the installed entry point uses the corrected boundary, and failure leaves a documented recoverable state

