# Spec Delta

## MODIFIED Requirements

### Requirement: Global initial-context loading

After activation, the full shared portable core and the applicable responsibility role SHALL appear in the initial Codex instruction context of new managed sessions across repositories using the configured Codex home. Role-owned orchestration obligations SHALL be loaded for the lead and role-owned execution obligations for executors, without omitting common safeguards. Activation SHALL use documented native instruction discovery and preserve the ordinary instruction hierarchy. Role composition SHALL follow `agent-role-contexts`; a lead role SHALL NOT itself activate delegation or require dispatch for a small direct task.

#### Scenario: Codex starts outside the harness repository
- **WHEN** a new main session starts in a separate repository with the activated Codex home
- **THEN** its initial instruction context contains the shared core and lead role without requiring a skill invocation or a manual file read

#### Scenario: Project guidance is also present
- **WHEN** the target repository has its own AGENTS.md
- **THEN** the shared core, applicable role and project instructions are discoverable under Codex's normal precedence, without altering the project's source

#### Scenario: A new session delegates work
- **WHEN** a newly started session with the updated global source dispatches an executor
- **THEN** the shared core and executor role are available in the delegated session's initial context without copying common policy into the role definition, requiring a manual source read or carrying the kit-owned lead-only instructions

### Requirement: Bounded principles instruction floor

The common portable principles document SHALL remain at most 24 KiB. The shared core and separately owned role guidance together SHALL preserve every normative rule of this specification, including session-lifecycle economy guidance and the current managed-tool selection. Compression and partitioning SHALL NOT drop, weaken or redefine a requirement, and references to retired capabilities SHALL be removed. The document SHALL remain the single authoritative home of shared principles; each role obligation SHALL have one declared owner. Required instruction load SHALL be measured across composed surfaces under `agent-role-contexts`, so extracting role text cannot conceal growth.

#### Scenario: Size and content after compression
- **WHEN** the common document and composed role guidance are measured and mapped against this specification
- **THEN** the common document is at most 24 KiB, every normative obligation is retained in its applicable owner, and the complete required load satisfies the role-context and existing concise-guidance contracts

#### Scenario: Retired capability reference
- **WHEN** a capability leaves the managed selection
- **THEN** its applicable instruction owner stops naming it as a live selection without losing the general rule it illustrated

### Requirement: Native instruction invariant checks

The native source check SHALL enforce the 24 KiB (24,576-byte) common-principles limit, validate declared role instruction and managed skill owners, and verify the repository-relative documentation owners declared by token-audit. It SHALL name the violated invariant and fail without mutating source. Checks SHALL use the same owner declarations as the reports. Compression and partitioning MUST preserve normative requirements; passing size, membership or path checks does not establish semantic equivalence or actual session consumption.

#### Scenario: Instruction growth exceeds the accepted limit
- **WHEN** the common principles exceed 24,576 bytes
- **THEN** the source check fails with actual and allowed sizes

#### Scenario: A declared report owner disappears
- **WHEN** a declared documentation owner is missing or outside the source root
- **THEN** the source check fails with the invalid owner route

#### Scenario: A managed skill has no valid membership
- **WHEN** a kit-managed skill has no shared or role membership, or a declared role source is missing
- **THEN** the source check reports the unresolved owner without modifying sources or applying a fallback role
