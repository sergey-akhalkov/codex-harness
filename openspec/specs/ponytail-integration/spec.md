# Ponytail Integration Specification

## Purpose

Deliver the original Ponytail plugin through native Codex setup and provide instructions another agent can follow when installing the harness on another PC.

## Requirements

### Requirement: Normal setup includes Ponytail

The ordinary setup guide SHALL include native installation of the original Ponytail plugin and supplied skills. Ponytail SHALL be enabled by default after native trust without project-local setup, copied policy or a per-session invocation. Existing deliberate disablement and unrelated settings SHALL be preserved.

#### Scenario: Another PC follows the guide
- **WHEN** the receiving agent follows `docs/installation.md` with its declared prerequisites and native hook review
- **THEN** a fresh ordinary Codex session receives Ponytail and discovers its skills without development-account files

#### Scenario: Ponytail is already installed
- **WHEN** setup finds an existing native installation
- **THEN** it checks that identity and selection instead of blindly adding a duplicate or resetting disablement

### Requirement: Native package and trust mechanisms remain authoritative

The integration SHALL use existing Codex installation and trust mechanisms and preserve upstream licensing. The guide SHALL identify the tested source and runtimes, commands and distinction between installed, pending trust and observed loading. No harness-specific package update, rollback, removal or ownership subsystem SHALL be added.

#### Scenario: Trust is incomplete
- **WHEN** the installed plugin awaits native review
- **THEN** the guide directs the user to that step without calling package presence active guidance

#### Scenario: A prerequisite is missing
- **WHEN** the receiving agent lacks a documented prerequisite
- **THEN** the guide identifies it and its supported setup action before claiming success

### Requirement: Original mode limitations are explicit

The original Ponytail behavior SHALL remain intact. The guide SHALL describe the observed resume reset and shared-mode state and distinguish temporary modes from persistent plugin disablement. Independent session modes SHALL NOT be promised.

#### Scenario: Sessions resume or run concurrently
- **WHEN** the user consults the operating guidance
- **THEN** the accepted upstream limitations are explicit without a fork or new mode controller

### Requirement: Acceptance observes the installed composition

Acceptance SHALL reuse valid qualification and verify final guidance outside the harness, including the selected external project read-only. Relevant source, link and strict change checks SHALL pass. A separate Windows environment or expanded lifecycle campaign SHALL NOT be a prerequisite.

#### Scenario: A fresh consumer loads the integration
- **WHEN** a fresh ordinary session starts outside the harness, including the selected existing project
- **THEN** evidence shows Ponytail, harness supplements, applicable project instructions and relevant skill resources without project tasks or file changes

#### Scenario: Target-machine checks have not run
- **WHEN** the locally verified guide is delivered
- **THEN** the other PC's execution remains the receiving agent's responsibility and is not reported as already observed
