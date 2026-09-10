# public-source-hygiene Specification

## Purpose

Keep the reusable agent pack's tracked sources, examples and public output suitable for publication, while retaining the knowledge and checks needed to maintain its supported capabilities.

## Requirements

### Requirement: Public tracked source boundary

All tracked source, configuration, instructions, skills, tests, fixtures, documents and OpenSpec artifacts SHALL exclude private consumer identities, private host or repository addresses, real machine-specific paths, credentials and session/runtime data. Examples SHALL use synthetic inputs or explicit local parameters. Public names needed for dependency identification and attribution SHALL remain accurate. A checked-in denylist SHALL NOT reproduce private identifiers. Private evidence and recovery copies SHALL remain outside the tracked repository.

#### Scenario: An integration is exercised in a private consumer
- **WHEN** the result is retained in the reusable kit
- **THEN** its reusable constraint and evidence limits are recorded without copying the consumer name, filesystem location or domain-specific data

#### Scenario: Generated files are staged for publication
- **WHEN** repository hygiene checks inspect the candidate source tree
- **THEN** tracked caches, logs, runtime databases and machine-specific configuration are reported as failures and ordinary generated caches are ignored

### Requirement: Safe public report projection

Public delegation reports SHALL anonymize arbitrary consumer labels without exceptions for real private projects and SHALL omit raw paths, transcripts and raw source identities. Private diagnostic output SHALL remain explicitly separate. Public projection SHALL retain meaningful accounting, attribution and missing-evidence status.

#### Scenario: An unknown consumer is reported
- **WHEN** the actual report command formats synthetic telemetry containing a consumer name and machine path
- **THEN** public output contains neither input identity while preserving the verified numeric totals and incomplete-data indicators

### Requirement: Documentation has a current maintenance purpose

Retained documentation SHALL support installation, configuration, development, verification, recovery, a current architectural constraint or an unfinished accepted requirement. Each fact SHALL have one authoritative home. Agents SHALL update that home instead of accumulating session diaries, copied upstream manuals, redundant verification reports or completed incident narratives. Before deleting a document, necessary current facts, active acceptance evidence and incoming references SHALL be preserved or retargeted. Cleanup SHALL NOT close, drop or weaken unfinished work.

#### Scenario: A completed investigation has a useful constraint
- **WHEN** its historical report is removed
- **THEN** the current operating restriction and relevant repeatable check remain in the owning maintained guide or test and links resolve

#### Scenario: An active migration still needs evidence
- **WHEN** documentation is consolidated
- **THEN** its accepted behavior, outstanding tasks, meaningful verification results and limitations remain available without private consumer identifiers

### Requirement: Reusable globally effective instructions

The installed global principles and memory workflow SHALL apply the publication and retention boundaries to the main agent and delegated work in consuming projects. Repository guidance SHALL describe the public pack direction while accurately distinguishing implemented Windows/Codex support from future platforms or agents. Existing public CLI entry points SHALL remain compatible. Verification SHALL include a fresh consumer outside the source checkout.

#### Scenario: The installed workflow is consumed elsewhere
- **WHEN** a fresh external session loads the installed instructions and memory skill
- **THEN** it receives the updated boundaries from their repository-owned sources without copying private facts into the kit

### Requirement: Proportionate repeatable hygiene verification

The repository SHALL provide a model-free check for supported public-source violations and local documentation links. Private terms supplied for a local audit SHALL NOT be persisted in the repository or echoed in public diagnostics. Synthetic failing and passing inputs SHALL verify the check. The check SHALL state its scope and SHALL NOT claim that current-tree cleanup removes information from Git history or previously published copies.

#### Scenario: A locally supplied private term is found
- **WHEN** an audit checks an owned test source containing that term
- **THEN** the command fails with the location and violation category without echoing the term or matched source line
