## ADDED Requirements

### Requirement: Ordinary Codex starts reflection transport automatically

When the flag is true, the installed ordinary interactive `codex` entrypoint SHALL prepare the transport needed by reflection without a separate lead or enable command. It SHALL preserve the selected model, provider, effort, profile, configuration, working directory, native terminal and session selection. Internal and noninteractive commands SHALL retain their existing behavior without recursive hosting.

#### Scenario: Ordinary interactive launch
- **WHEN** the user starts ordinary `codex` with the flag true and normal default-provider authentication
- **THEN** the visible native conversation has its own verified lead registration, and its first eligible acceptance starts reflection without another setup command

#### Scenario: Explicit preferences and resumed conversation
- **WHEN** ordinary interactive startup selects a model, profile, configuration or existing conversation through native arguments
- **THEN** the same choices reach the managed transport and visible conversation, including native resume/fork selection, without silent replacement or dropped parameters

#### Scenario: Internal and noninteractive invocation
- **WHEN** Codex runs help, a noninteractive subcommand, an explicit remote attachment or an owned executor/reflection/backend invocation
- **THEN** it retains its appropriate native owner and does not recursively start an ordinary lead host

#### Scenario: Existing unmanaged frontend
- **WHEN** an unmanaged conversation was already open when the new launcher was installed
- **THEN** its work remains intact and the update applies on its next ordinary launch; status does not pretend that its thread id is an active transport

#### Scenario: Completed sessions remain dormant
- **WHEN** the flag becomes true or a new ordinary conversation starts while completed sessions exist
- **THEN** only the exact current foreground session can receive automatic reflection; the harness does not reopen completed sessions, backfill their acceptances or dispatch their retained pending reflection stages

#### Scenario: Explicitly resumed session
- **WHEN** the user explicitly resumes an existing conversation through the native frontend
- **THEN** that selected conversation becomes current, and only work accepted during its current activation is eligible; historical acceptances are not replayed

#### Scenario: Failed attachment or session exit
- **WHEN** the native frontend cannot attach or its owned session exits
- **THEN** the host retires only its own registrations and processes and retains any unfinished reflection work with truthful failure and recovery information

## MODIFIED Requirements

### Requirement: Explicit activation with a bounded mandate

The installed managed lead SHALL automatically enable the two-stage reflection chain for its project and authorized work when `self_reflection_enabled` is true. No separate enable command SHALL be required. Activation SHALL retain the existing bounded mandate, process ownership and recovery checks. It SHALL not authorize unrelated work, new providers, weakened acceptance or destructive actions. No persistent observer model SHALL be launched.

#### Scenario: A user enables improvement
- **WHEN** a supported lead session starts with the flag true, or observes it become true
- **THEN** the installed interface prepares event access, scope and recovery without another activation command, manual endpoints or task identifiers, and ordinary work continues

#### Scenario: Duplicate activation
- **WHEN** another activation addresses the same work
- **THEN** it identifies the existing owner, preserves checkpoints and retained records, and launches no duplicate consumer or reflection

#### Scenario: Mode is disabled
- **WHEN** ordinary development proceeds with the flag false
- **THEN** no new automatic reflection runs and ordinary lead, executor and feedback behavior remains available

#### Scenario: First acceptance arrives before the host poll
- **WHEN** the flag is true and the lead accepts its first eligible task before automatic host preparation has run
- **THEN** acceptance prepares the existing lifecycle before closing the task, and the resulting event remains eligible for exactly one reflection

#### Scenario: A new lead resumes the project
- **WHEN** the previous owner is verifiably inactive and a supported successor starts with the flag true
- **THEN** the harness transfers current control through the existing verified succession path without requiring a separate command or rewriting retained acceptance identities, and pending stages belonging to the finished predecessor remain dormant

#### Scenario: The retained owner is live or unverifiable
- **WHEN** automatic preparation encounters another live owner or cannot establish its exit
- **THEN** it preserves ownership and retained work and exposes the concrete conflict without launching duplicate model work

### Requirement: Configuration permission gates new reflection model work

The installed configuration SHALL define `self_reflection_enabled`, a boolean defaulting to true. True SHALL automatically prepare reflection in the supported managed lead. False SHALL prevent new executor and lead reflection model calls while ordinary work remains available. Each new stage SHALL re-read the value. Retained work, identities, usage, recovery and event boundaries SHALL be preserved.

#### Scenario: The permission is omitted
- **WHEN** an existing or new configuration omits `self_reflection_enabled`
- **THEN** it defaults to true and automatically prepares the supported managed lead without another activation step

#### Scenario: Model work is disabled
- **WHEN** `self_reflection_enabled` is false
- **THEN** no new executor or lead reflection model call starts from manual, automatic, continuation or internal dispatch, enable refuses before lifecycle mutation, and ordinary acceptance still completes

#### Scenario: The flag changes between stages
- **WHEN** a still-current host has retained an executor reflection and the flag changes from true to false before the lead stage
- **THEN** the executor result, identity, usage and pending lead stage remain retained, no lead model call starts, and a later true value resumes the retained lead stage without repeating the executor or requiring enable

#### Scenario: Status explains stopped model work
- **WHEN** the caller reads status while the flag is false or the lifecycle is not active
- **THEN** the result names the effective flag, configuration path and concrete reason no new model work may start, distinguishing absence of a supported host from an activation command requirement

#### Scenario: The permission has an invalid type
- **WHEN** the configuration supplies a string, integer or other non-boolean value
- **THEN** configuration loading fails before model work or lifecycle mutation
