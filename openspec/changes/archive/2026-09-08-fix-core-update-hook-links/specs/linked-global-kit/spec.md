## ADDED Requirements

### Requirement: Core updates preserve connected hooks

A core install or update SHALL retain previously connected hook definitions and launcher links in the managed installation. It SHALL update their source targets when the checkout moves, repair missing managed links, preserve ownership conflicts and transactional recovery, and SHALL NOT enable hooks on a fresh core-only installation.

#### Scenario: Update an installation with diagnostics
- **WHEN** a user updates only the core of an installation with recorded hook connections
- **THEN** both hook connections remain usable and recorded, including on a repeated update

#### Scenario: Relocate or repair recorded connections
- **WHEN** a core update selects another checkout or a recorded hook link is missing
- **THEN** the hook links point directly to the selected source and missing links are recreated

#### Scenario: Foreign replacement at a hook destination
- **WHEN** another file replaces a managed hook link before a core update
- **THEN** the update reports the ownership conflict without replacing the file or losing installation state

#### Scenario: Fresh core installation
- **WHEN** a user installs only the core without previous hook connections
- **THEN** the installation does not add hook definitions or a hook launcher

### Requirement: Hook and MCP diagnostic runtime agreement

Command hooks and native MCP diagnostics using the same installed registry SHALL share a compatible diagnostic runtime identity and deliver real findings and their clearance.

#### Scenario: Native diagnostics broker already running
- **WHEN** the native MCP launcher has started the installed diagnostics runtime and a command hook observes a source edit
- **THEN** the hook can use that runtime without a false source-version mismatch and reports the edited file's diagnostic state
