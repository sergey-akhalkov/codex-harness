# Linked Global Kit Specification

## Purpose

Make a checkout of codex-harness directly available to Codex CLI as a reusable global configuration and capability source, with reproducible host setup and no deployed copies of repository artifacts.

## Requirements

### Requirement: Complete portable kit source

The repository SHALL own the complete source of its declared global toolkit: the full global AGENTS.md content, shared configuration, managed skills and their referenced resources, managed agent definitions, connection/launch tools and dependency declarations. The host-global AGENTS.md SHALL directly reference that full repository-owned instruction source. A fresh checkout with documented external prerequisites SHALL be sufficient to reproduce the declared kit without retrieving undeclared files from another PC. Connection and managed resource paths SHALL resolve on the current machine. A catalogue entry or dependency declaration SHALL not imply an unimplemented capability is delivered.

#### Scenario: The kit is deployed on a new PC
- **WHEN** a fresh checkout is connected on another Windows account with the documented prerequisites
- **THEN** every declared managed capability and its required resources are available from the checkout or an explicitly declared external dependency, without depending on files from the former PC; machine-specific settings remain local and the shared source contains only portable defaults

#### Scenario: The global instruction source is inspected
- **WHEN** the user follows the active global AGENTS.md connection
- **THEN** the complete instruction content is stored under version control in this repository and appears in new sessions through that connection

### Requirement: Direct source consumption

The kit SHALL connect its configuration, portable instructions, skills, agent definitions, MCP connection definitions, language integration definitions and hooks by supported path references or filesystem links. Codex and the corresponding integration consumer SHALL read the repository source when consuming those data artifacts. The installer MUST NOT copy, render, merge or cache their contents into deployment files, including as a fallback. Small host-local path registrations and installation metadata SHALL contain references and ownership information rather than duplicated artifact bodies. Compiled first-party executables SHALL be distinguished from source-owned data: installation SHALL connect verified build artifacts with recorded source/build identity, while their complete source remains in the checkout. This executable-artifact allowance MUST NOT permit embedding deployed copies of source-owned configuration, instructions or skills. External packages, language runtimes and generated runtime state SHALL remain distinct from repository-owned configuration and integration source; their installation SHALL not be used to smuggle deployed copies of those artifacts into a package cache.

#### Scenario: A clean host is connected
- **WHEN** the installer successfully connects a valid checkout
- **THEN** every managed data artifact resolves to its source in that checkout without a deployed content copy, and every managed compiled executable resolves to a verified build identified with that checkout's relevant sources

#### Scenario: Links or another required capability are unavailable
- **WHEN** a required direct connection cannot be established
- **THEN** installation fails with the specific cause and a corrective action, without substituting copied artifacts or reporting partial activation as success

#### Scenario: MCP or diagnostic integration source is updated
- **WHEN** a connected MCP definition, language integration definition or hook definition changes and a new consumer starts
- **THEN** the consumer reads the updated checkout data through its existing connection without a generated deployment copy, while a changed compiled implementation follows the verified build/update contract

### Requirement: Reproducible global entry point

On the initial supported Windows native environment, `codex-harness.exe` SHALL provide native install, update, check, recover and disconnect operations and resolve its intended checkout from explicit source selection or verified registration independently of the caller's working directory. A documented Rust-native bootstrap with declared prerequisites SHALL connect an already installed supported Codex CLI. A new terminal's ordinary `codex` command SHALL select the repository configuration automatically for local session commands, without a manual permissions selection, profile flag, or harness working directory. Explicit invocation overrides SHALL remain effective under documented Codex precedence. The installer SHALL report the effective user directories, CLI version and supported native entry point. Upgrade SHALL migrate existing owned script-based connections and document equivalent native commands without requiring manual recreation of the kit.

#### Scenario: Codex starts in another repository
- **WHEN** a new terminal starts a normal local Codex session in a repository outside the harness
- **THEN** the repository configuration is loaded automatically together with applicable host and project configuration

#### Scenario: The checkout has a different path
- **WHEN** installation runs from a checkout with spaces or non-ASCII characters on another Windows account
- **THEN** it uses that checkout and account's resolved paths without references to the development machine

#### Scenario: The user deliberately overrides a default
- **WHEN** the user supplies an explicit profile or a supported session configuration override
- **THEN** the entry point preserves the user's choice and documents the resulting scope instead of silently forcing the kit default

#### Scenario: Existing script installation is upgraded
- **WHEN** the native installer upgrades an owned working installation created by `install.ps1`
- **THEN** native command connections replace its supported script entry points, ordinary `codex` remains available, and existing foreign state and recovery information are preserved

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

### Requirement: Source updates and checkout availability

A new Codex process SHALL consume edited contents of already connected source-owned data files without a second install or content synchronization. Changed first-party executable source SHALL require an explicit verified build/update before the affected native executable runs; stale, missing or altered builds SHALL be reported with the corrective action. Ordinary consumer startup MUST NOT invoke Cargo, compile or download dependencies. Adding or removing separately registered artifacts SHALL be reconciled by rerunning native installation without copying data artifacts. Checkout relocation SHALL support reconnection and build-identity reconciliation. Verification SHALL identify a moved, deleted or inaccessible source and MUST NOT treat a dangling connection or source-stale binary as healthy. Already running sessions are not required to reload their entire initial context; skills accepted and activated by the autonomous evolution workflow SHALL additionally satisfy current-session skill awareness and revision recovery without a user restart. Orchestrated worker succession through `codex resume` is a separate consumer of those updates and SHALL NOT satisfy the current-session awareness requirement.

These harness health checks SHALL NOT block ordinary upstream Codex CLI launch. An installed command bootstrap independent of checkout availability SHALL preserve access to the original CLI when harness source, shared configuration or its required build is unavailable. It SHALL report the degraded harness and use native arguments/local settings without running unverified harness extensions, rebuilding, downloading or rewriting user configuration. Missing upstream Codex itself and recursive command discovery SHALL remain explicit failures, not reasons to invoke an arbitrary replacement.

#### Scenario: A connected file changes
- **WHEN** a shared setting, principle, skill body or agent definition is edited in the checkout and a new process starts
- **THEN** Codex observes the new source content without a reinstall

#### Scenario: The registered set changes
- **WHEN** source artifacts are added or removed and installation is rerun
- **THEN** the live registrations are reconciled, removing only obsolete kit-owned registrations and preserving unrelated capabilities

#### Scenario: The checkout is moved
- **WHEN** verification encounters the old location and installation is subsequently run from the new checkout
- **THEN** verification reports the stale source and reconnection updates only kit-owned references and the applicable build identity to the new location

#### Scenario: Native implementation changes
- **WHEN** relevant Rust source or locked build inputs change after installation
- **THEN** Check and the affected invocation report the outdated build without automatic compilation, and successful explicit update activates a verified candidate with recoverable prior state

#### Scenario: Harness is unavailable during an ordinary Codex launch
- **WHEN** the installed launcher encounters stale or missing shared build inputs, an unavailable checkout/module, or missing or malformed harness registration while the original CLI remains installed
- **THEN** ordinary Codex launches once with the user's native arguments, local settings, cwd and streams preserved, reports degraded harness behavior, and does not require a successful harness update first

#### Scenario: A running process uses the previous build
- **WHEN** an explicit update validates a new native build while another session still uses the old executable
- **THEN** activation does not overwrite that running artifact and cleanup preserves it until owned use can safely end

#### Scenario: Stale manager needs to update itself
- **WHEN** source freshness differs but the last accepted manager still passes binary-integrity and metadata-compatibility checks
- **THEN** Check and explicit update/recover/disconnect remain usable without starting the obsolete ordinary runtime; an unavailable or altered manager instead requires the documented native bootstrap recovery path

#### Scenario: An accepted skill changes in a running session
- **WHEN** the autonomous workflow activates a created, updated or restored skill
- **THEN** the current session can apply the accepted revision before its next relevant action and recover it after compaction without requiring a new process
- **AND** recovery uses a currently allowed ordinary-CLI path; restoring ordinary diagnostic, context or Stop hooks, or substituting App Server-only success, does not satisfy this scenario

### Requirement: Bounded and repeatable installation

The installer SHALL provide a non-mutating preview and validate prerequisites, source files, target ownership and path boundaries before activation. Repeating a successful installation SHALL not duplicate links, configuration selection or path registration. An existing connection to the same source SHALL be recognized; a conflicting foreign file, link, capability name or higher-priority global instruction override SHALL be reported without replacement or concealment. Existing unrelated global capabilities SHALL remain usable.

#### Scenario: A compatible installation already exists
- **WHEN** installation runs again against the same checkout and host
- **THEN** it recognizes the current direct connections and leaves them effective without duplicate registrations

#### Scenario: A target is owned by another source
- **WHEN** preview or installation encounters a conflicting file, link or instruction override
- **THEN** it identifies the conflict and required resolution without overwriting or shadowing the existing content

### Requirement: Failure recovery and disconnect

The installer SHALL keep enough host-local ownership information to undo its own registrations. On a failed activation it SHALL restore the pre-activation connection state or report exactly which changes remain and how to recover. Disconnect SHALL remove only registrations still owned by this installation, preserve source files and unrelated local state, and leave the original Codex command usable. It MUST NOT recursively delete a source through a filesystem link.

#### Scenario: Activation fails after one mutation
- **WHEN** a later activation step fails
- **THEN** the completed mutations are rolled back and the original cause remains visible, with any rollback failure and outstanding change explicitly reported

#### Scenario: A managed target was replaced externally
- **WHEN** disconnect finds that a recorded destination now belongs to another file or link
- **THEN** it preserves that destination and reports the ownership mismatch

#### Scenario: The kit is disconnected
- **WHEN** disconnect succeeds
- **THEN** the original CLI and unrelated configuration/capabilities remain usable, kit-owned registrations are removed, and checkout/authentication/session files remain intact

### Requirement: Evidence from the actual consumer

Acceptance SHALL exercise the installed CLI entry point in both a neutral directory and a separate repository with project instructions. Checks SHALL prove effective shared and local configuration, live source updates, skill discovery, representative agent consumption, coexistence and recovery scenarios. File hashes, mocks or the existence of links alone SHALL not count as consumer evidence. Verification SHALL distinguish actual global activation from isolated tests and SHALL leave no test-only capability active.

#### Scenario: The installer reports successful activation
- **WHEN** activation is declared verified
- **THEN** evidence identifies the CLI version, effective source paths, representative observed behavior and tested limitations

#### Scenario: A disposable agent is used to verify loading
- **WHEN** a representative agent is exercised because no production role exists yet
- **THEN** the report identifies it as a fixture and removes its test registration without claiming a delivered production agent catalogue

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

### Requirement: Persistent global hook suspension

The kit SHALL support a recoverable suspension of all Codex lifecycle hooks across user/base configuration, the managed profile and installed hook definitions. Development without ordinary lifecycle hooks SHALL remain the reusable default after completion, update and archive. The specifically accepted RTK command-compression hook SHALL be the only newly enabled managed exception, with bounded execution and native trust. Already-running sessions retaining cached diagnostic harness handlers SHALL perform no automatic diagnostic analysis, fallback scan, context injection or completion continuation. A fresh consumer outside the checkout SHALL observe only the accepted selection, with base and managed profile agreement. Machine-local markers and backups SHALL remain outside reusable source. A fresh core-only installation without the accepted capability SHALL not enable hooks.

#### Scenario: A cached session invokes an old handler
- **WHEN** an old automatic diagnostic handler is invoked
- **THEN** it returns without diagnostic work or feedback

#### Scenario: Another project starts a new session
- **WHEN** it consumes the installed base or managed profile with RTK selected
- **THEN** only the trusted RTK exception is enabled among managed hooks, without project-local configuration

#### Scenario: The optimization change is completed
- **WHEN** its tasks are closed, archived or followed by a kit update
- **THEN** the consumer retains the accepted RTK selection and no backup or lifecycle label reactivates rejected hooks

#### Scenario: Explicit suspension or disconnection
- **WHEN** the RTK capability is disabled or disconnected
- **THEN** its hook is inactive, rollback remains recoverable and unrelated user configuration is preserved

### Requirement: Accepted capability selection survives lifecycle operations

Install, update, repair, source relocation, recovery and disconnect SHALL preserve explicit suspension and the accepted retained/retired capability selection. No lifecycle operation or implementation-complete label SHALL silently restore a rejected hook, LSP backend or LSP carrier. Shared external package removal SHALL require verified ownership and absence of other consumers; disconnecting only Codex registrations SHALL be sufficient when packages are shared. Recovery SHALL preserve user edits and credentials.

#### Scenario: Update repairs links after suspension
- **WHEN** a core or code-tools update repairs managed links
- **THEN** repaired links still consume the disabled selection and cannot revive previous automatic diagnostics

#### Scenario: Shared language installation has another consumer
- **WHEN** its Codex LSP integration is retired
- **THEN** that registration is removed while the shared installation and other application's configuration are preserved

### Requirement: Ordinary sessions receive live shared defaults

An ordinary local `codex` session SHALL receive the live checkout shared defaults that the kit declares for new tasks, including developer instructions and the accepted experimental context-management trial, together with applicable local overrides. A source-stale or otherwise non-runtime native build MUST NOT silently omit those defaults. If live injection cannot be established, the launcher SHALL report a degraded session without kit shared defaults, Check SHALL record the same failure, and that session MUST NOT be treated as a successful kit consumer. Explicit user profile or `-c` overrides remain effective. Management, help and version commands MAY keep their native invocation without session defaults.

#### Scenario: Native build identity is source-stale
- **WHEN** the recorded native manager no longer matches the current checkout source and an ordinary local session starts
- **THEN** the session still receives live shared defaults, or the launcher reports the degraded fallback and Check flags missing defaults

#### Scenario: Fresh consumer prompt is inspected
- **WHEN** `codex debug prompt-input` runs from an outside repository through the installed ordinary entry point
- **THEN** the dump includes the current developer-instruction marker from shared defaults, unless the degraded fallback was explicitly reported

### Requirement: Subscription profiles without OpenCodex
The linked kit SHALL connect Grok and Z.AI as native Codex profiles from checkout source without installing OpenCodex, copying a proxy config body, or injecting `openai_base_url` for ordinary sessions. Grok profile files, catalogs and helper registrations SHALL be references or generated host-local wiring that contain no secrets. Discovery SHALL report OpenCodex as retired after the Responses spike, not as a live capability. Ordinary `codex` SHALL keep live shared defaults and native GPT without a localhost model proxy.

#### Scenario: Fresh consumer uses GPT
- **WHEN** a new terminal starts ordinary `codex` in another repository after this connection
- **THEN** shared kit defaults load, traffic does not go to `127.0.0.1:10100`, and Grok is unused unless the xAI profile or an explicit override is selected

#### Scenario: Grok profile is selected
- **WHEN** the user starts `codex --profile xai` after login
- **THEN** the session uses the native xAI provider wiring from the kit without an OpenCodex process

#### Scenario: OpenCodex leftovers are checked
- **WHEN** Check runs after successful retirement
- **THEN** it reports the OpenCodex proxy/task/package as absent or retired and does not treat a leftover localhost injection as healthy kit routing

### Requirement: Codex CLI updates do not block ordinary launch

Harness compatibility and registration freshness checks SHALL NOT prevent the
installed Codex CLI from starting after that CLI is updated. A digest mismatch
for the registered upstream executable or its package metadata is not by itself
an incompatibility: ordinary launch MUST start the current CLI and MUST NOT
warn when harness enhancements still apply. A warning is allowed only when the
harness cannot apply its enhancements to that CLI, and MUST NOT refuse the
process. Missing Codex itself and recursive command discovery SHALL remain
explicit failures.

#### Scenario: User restarts Codex after its own updater
- **WHEN** Codex CLI has been updated in place and the user starts `codex` again through the installed harness command with a still-preparable harness session
- **THEN** the current Codex CLI starts without a launch blocker and without a compatibility warning; Check may still record stale registration

### Requirement: Deliver verification workflows through the global lifecycle

The installed skills and their resources MUST work without the original source-kit consumer
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

Delivery acceptance MUST exercise project verification in two existing projects outside the harness checkout and regression reproduction in at least one of them through new native Codex sessions. Isolated worktrees or snapshots of real projects MAY be used to preserve ongoing work, but manufactured fixture-only repositories MUST NOT substitute for the two real consumers. Evidence SHALL identify the source state, actual invoked command, skill use, observed outcome and limitations. A lint-only consumer MUST count only for its demonstrated validation scope. Both workflows MUST satisfy their qualitative consumption requirements before the reusable capability is declared complete. A two-consumer quantitative speed comparison is not an implementation-acceptance requirement of this change; benefit remains unproven.

#### Scenario: A consumer has unrelated uncommitted work

- **WHEN** its acceptance case is prepared
- **THEN** the case uses recorded isolated source state without modifying or cleaning the user's live checkout or controlling its running services

#### Scenario: Discovery succeeds without an actual task

- **WHEN** installation tests find the skills but no outside-project behavior has been exercised
- **THEN** discovery is recorded as passing while reusable-delivery acceptance remains incomplete

#### Scenario: Quantitative benefit stays unproven

- **WHEN** outside-project consumption has been exercised and the two-consumer speed comparison is not required
- **THEN** reusable-delivery acceptance may close while benefit remains unproven
- **AND** no acceleration claim is recorded

### Requirement: Autonomous managed skill registration

The kit SHALL expose the evolution workflow, the `skills-usage-analysis` skill, the usage command and their required runtime resources globally through its reproducible installation lifecycle. After accepted shared skill publication or retirement, the workflow SHALL reconcile only the affected kit-owned registrations and ownership state without requiring a user to rerun installation manually. This operation SHALL preserve direct source consumption, source ownership, rollback and disconnect semantics; it MUST NOT reinstall unrelated dependencies, rewrite unrelated configuration or restart shared services. Project-owned skill packages and their records SHALL survive kit disconnection.

#### Scenario: New common skill is accepted outside the harness
- **WHEN** an authorized evolution episode in another repository promotes a skill to the canonical kit source
- **THEN** its scoped global registration is reconciled automatically and the originating session and another project can use it through live source references

#### Scenario: Registration target has foreign ownership
- **WHEN** scoped registration encounters a conflicting foreign destination or an active installation transaction
- **THEN** it preserves the existing state, reports the conflict or busy condition and leaves publication pending or safely recovered without claiming complete activation

#### Scenario: Installation moves or disconnects
- **WHEN** the kit is reconnected at a new location or disconnected
- **THEN** evolution registrations follow the same ownership-aware lifecycle while project skills, knowledge records and unrelated capabilities remain intact

### Requirement: One-action deployment with explicit repair

The kit SHALL provide a single `codex-harness deploy` command that builds an immutable candidate from an absolute source checkout (or reuses an explicit build), installs or updates the core connection with user-default homes, verifies the installed launcher through its own version build identity and executor usage probe, re-reads the installation metadata, and reports one receipt. A relative source SHALL produce an explicit error naming the absolute-path requirement. The command SHALL offer an explicit `--reset` for installations whose recorded link ownership no longer matches reality: reset removes exactly the recorded owned objects that are reparse points or already missing, preserves and reports every regular file or foreign object, writes a receipt before removal, and then performs a normal installation. The ordinary verbs SHALL keep refusing silent repair. An `--all` option SHALL chain the existing scoped component updates after the core connection and report each component outcome in the same receipt.

The kit SHALL provide a `harness-deploy` skill, delivered with the kit's skills, that owns the deployment procedure: everyday one-action delivery, receipt verification, deliberate blocked-installation repair through `deploy --reset`, and the prohibition of out-of-band link retargeting to working-tree builds. The skill SHALL reference the installation guide as the authoritative detail home instead of duplicating it.

#### Scenario: Everyday delivery is one command
- **WHEN** a developer with a changed checkout runs `codex-harness deploy --source <absolute-checkout>`
- **THEN** a new immutable candidate is built and installed, and the receipt reports the installed launcher's build identity and a passing executor usage probe

#### Scenario: A blocked installation is repaired deliberately
- **WHEN** update refuses because an owned link's recorded identity no longer matches and the operator runs `deploy --source <absolute-checkout> --reset`
- **THEN** recorded link objects are removed only where they are reparse points, foreign regular files are preserved and reported, a reset receipt is written, the fresh installation succeeds, and a following `update --preview` validates without ownership conflicts

#### Scenario: The full delivery chain runs in one action
- **WHEN** the operator runs `deploy --source <absolute-checkout> --all`
- **THEN** the core connection is followed by the scoped component updates in order, and the receipt reports each component's outcome and stops at the first failure with prior work preserved
