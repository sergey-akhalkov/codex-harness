# Spec Delta

## Purpose

Deliver the original Ponytail package through the harness lifecycle so ordinary Codex work receives its guidance by default, with reproducible versions, visible activation and recoverable host changes.

## ADDED Requirements

### Requirement: Ponytail is part of ordinary delivery

Ordinary harness installation and deployment SHALL include the original Ponytail Codex plugin and its supplied skills without a separate opt-in component. After native trust setup, new ordinary sessions SHALL receive Ponytail automatically in its supported default mode. Installation SHALL preserve upstream licensing and attribution and SHALL NOT create a separately maintained copy of its rules or skills.

#### Scenario: Installation on a new PC
- **WHEN** the user installs the harness on a supported clean Windows target and completes native hook trust setup
- **THEN** an ordinary Codex session in another repository receives Ponytail without manual file copying, project configuration, a profile flag or a Ponytail invocation
- **AND** the upstream skills and their required resources are discoverable from their installed package

#### Scenario: Existing harness is upgraded
- **WHEN** an existing installation without Ponytail is updated
- **THEN** the normal lifecycle includes Ponytail and reports the remaining native trust step without requiring the user to assemble a second installation procedure

### Requirement: Prerequisites and trust are explicit

Installation SHALL check the selected Codex and hook runtime before activation and report missing prerequisites with a supported next action. It SHALL preserve native hook trust review and SHALL NOT grant trust by rewriting approval records. A package awaiting trust SHALL remain visibly pending; installation, trust and observed context loading SHALL be reported as distinct facts.

#### Scenario: Hook trust is not granted
- **WHEN** the plugin is installed but the current hook definitions have not been trusted
- **THEN** installation and Check identify pending activation and direct the user to the native review path
- **AND** neither a successful package download nor its presence in the catalogue is reported as active Ponytail guidance

#### Scenario: A required runtime or CLI capability is unavailable
- **WHEN** a target lacks a compatible Node runtime or required native plugin or hook support
- **THEN** preflight identifies the missing capability before committing activation, preserves the previous usable installation and names the supported provisioning or upgrade action

#### Scenario: Changed hooks require renewed trust
- **WHEN** an accepted package update changes a hook definition that Codex requires the user to review again
- **THEN** the lifecycle reports pending native review and does not reuse an obsolete trust decision or report the updated hooks as active

### Requirement: Versions are reproducible and updates deliberate

The portable declaration SHALL identify the authoritative upstream and an immutable package revision. Installation SHALL verify the resolved identity. Explicit update SHALL activate only the declared candidate after compatibility and instruction-coverage checks. Ordinary session startup and Check SHALL NOT download or advance Ponytail, and a failed update SHALL preserve a recoverable previously accepted selection.

#### Scenario: The upstream branch advances
- **WHEN** upstream publishes new commits without a change to the accepted harness declaration
- **THEN** repeat installation, startup and Check retain the declared revision instead of silently selecting the new branch head

#### Scenario: A candidate is incompatible
- **WHEN** update cannot establish the candidate's identity, native compatibility or retained instruction coverage
- **THEN** it reports the failed stage, preserves the prior selection and recovery inputs, and does not claim that the candidate was activated

#### Scenario: A prepared installation starts offline
- **WHEN** the selected package and required runtime are already installed and the network is unavailable
- **THEN** normal startup uses those local resources without an update request

### Requirement: Ownership and explicit choices survive repetition

Repeated lifecycle operations SHALL preserve user disablement, unrelated plugins, project instructions and local settings. An existing compatible Ponytail installation SHALL be recognized without duplicate effective discovery; conflicting package or skill ownership SHALL be reported without replacement. Check SHALL distinguish deliberate disablement from an unexpected loading failure.

#### Scenario: Compatible Ponytail already exists
- **WHEN** installation finds an existing Ponytail package matching the selected source and revision
- **THEN** it reuses that identity where supported, records that it was not created by the harness and avoids a second effective plugin or skill source

#### Scenario: A copied skill or other package conflicts
- **WHEN** a foreign installation would shadow or duplicate a selected Ponytail skill
- **THEN** preflight reports the conflict and its supported resolution without deleting or silently replacing the foreign source

#### Scenario: The user has deliberately disabled Ponytail
- **WHEN** installation, update, repair or reconnection is repeated
- **THEN** that choice remains effective and visible rather than being reset by the default-on policy

### Requirement: Context survives the ordinary session lifecycle

Ponytail guidance SHALL reach supported lead and executor sessions before dependent work, including fresh starts and applicable resume or compaction boundaries. The integration SHALL preserve original upstream mode behavior and document its observed scope and limitations in `docs/installation.md`. It SHALL NOT promise session-isolated modes or mode preservation beyond what the pinned upstream provides.

#### Scenario: Resume or compaction occurs
- **WHEN** an active managed session resumes or crosses its supported compaction boundary
- **THEN** subsequent work receives Ponytail guidance together with harness supplements and applicable project instructions; resume uses the upstream default mode and compaction uses its current stored mode
- **AND** recovery does not require manually pasting upstream rules

#### Scenario: Work runs concurrently
- **WHEN** a lead and executor, or two ordinary sessions, work concurrently in the same or different repositories
- **THEN** all receive the original plugin and harness supplements; documentation explains that same-project sessions share mode state and that hosts without project context share a fallback state
- **AND** documentation warns that another session's startup or resume can reset that shared mode; independent mode isolation is not claimed

#### Scenario: A mode is changed explicitly
- **WHEN** the user uses a supported Ponytail mode control during concurrent work
- **THEN** the observed upstream effect and shared-state scope are documented, including reset on resume and possible effects on concurrent sessions; installation-level plugin disablement remains distinct from a temporary upstream mode change

### Requirement: Loading failures remain visible and recoverable

Unexpected package, runtime, trust or hook failures SHALL identify their cause, activation limit and next recovery action. Ordinary Codex launch SHALL remain available under the existing degraded-launch contract. The harness SHALL NOT conceal missing Ponytail with duplicated fallback rules, treat degraded context as successful integration, or claim that running sessions have reloaded an update without evidence.

#### Scenario: Package files or hook execution are unavailable
- **WHEN** an otherwise usable Codex starts with missing Ponytail resources or a failed required hook
- **THEN** the session or native startup output exposes the failed activation, Check reports the same condition where observable, and ordinary Codex remains usable
- **AND** unobservable current loading is reported as unverified rather than inferred from an earlier successful check

#### Scenario: A running session predates an update
- **WHEN** a package update completes while another session is active
- **THEN** the lifecycle identifies which subsequent sessions use the new revision and does not assert that the active session has already consumed it

### Requirement: Recovery and disconnect preserve foreign state

The lifecycle SHALL retain host-local ownership and recovery information for its Ponytail effects. Recovery SHALL distinguish the original failure from restoration failure. Disconnect SHALL undo only still-owned registrations and preserve pre-existing packages, shared runtimes, unrelated settings and user session data. It SHALL leave the original Codex entry point usable.

#### Scenario: Installation is interrupted after effects
- **WHEN** an interrupted operation has changed plugin selection or registration
- **THEN** recovery restores the prior owned state or identifies each remaining effect and safe next action without claiming a complete rollback

#### Scenario: The harness adopted an existing package
- **WHEN** the harness is disconnected
- **THEN** the pre-existing Ponytail installation and its unrelated consumers remain available

#### Scenario: A managed registration was changed externally
- **WHEN** recovery or disconnect encounters an ownership mismatch
- **THEN** it preserves the changed object and reports the conflict instead of removing it by name alone

### Requirement: Acceptance proves installed consumption

Acceptance SHALL exercise locally isolated installation and actual use outside the harness checkout, covering required lifecycle scenarios. Evidence SHALL distinguish discovery, context loading and observed behavior without exposing private inputs or claiming unmeasured savings. `docs/installation.md` SHALL provide the other-PC handoff; preparing another Windows environment SHALL NOT be required for implementation acceptance.

#### Scenario: Another PC receives the installation handoff
- **WHEN** the receiving agent installs the harness on another supported Windows PC
- **THEN** `docs/installation.md` gives the prerequisites, normal install, native trust step, fresh-session and repeat-install checks, Check and recovery path without relying on the development account's package cache, trust state or hidden files
- **AND** the implementation provides locally verified command guidance without claiming that the receiving agent's target-machine checks have already run

#### Scenario: A real external task consumes the integration
- **WHEN** a fresh installed Codex session performs a bounded task in an existing external project with its own instructions
- **THEN** retained native evidence identifies the selected package, effective guidance and relevant skill use, and independent checks establish the task result
- **AND** a neutral working directory verifies global loading while disposable fixtures remain supplemental rather than replacing the real consumer

#### Scenario: Only discovery or text checks have passed
- **WHEN** no actual external task or required lifecycle case has been exercised
- **THEN** those checks remain explicitly unfinished and the integration is not declared delivered
