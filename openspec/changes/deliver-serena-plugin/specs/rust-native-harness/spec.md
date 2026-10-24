# Spec Delta

## MODIFIED Requirements

### Requirement: Explicit reproducible native build

Native source builds SHALL remain the producer/developer route. Ordinary consumers of the released Serena plugin SHALL receive verified prebuilt artifacts and required portable resources through native plugin installation without installing harness build prerequisites or retaining its checkout. Plugin artifact integrity and compatibility SHALL be verifiable without a local source tree; source-linked freshness and live-data rules below apply only to the source-linked route. Project-language toolchains remain separate project prerequisites.

The kit SHALL document and verify a Rust-native bootstrap from the checkout
using declared build prerequisites and locked dependencies. Installed artifacts
SHALL carry the relevant source, compiler-profile and binary-integrity
evidence. A successful build SHALL be verified before replacing an active
installation. Ordinary runtime startup and read-only Check MUST NOT build code
or acquire dependencies. Stale, missing or altered required builds SHALL
produce an actionable status instead of silently executing an unverified
candidate. Source-linked configuration and resources SHALL retain their
authoritative checkout paths. A fast local candidate SHALL be an explicit,
separately identified profile candidate; ordinary release publication and
unchanged-build reuse SHALL retain their existing fat-LTO and fresh-target
requirements.

#### Scenario: Missing build prerequisite

- **WHEN** explicit bootstrap cannot find a compatible Rust toolchain or required native linker
- **THEN** it reports the missing prerequisite before global connection changes and preserves any existing working installation

#### Scenario: Source-stale artifact

- **WHEN** relevant executable source changes after a successful build
- **THEN** Check and the affected entry point identify the stale build and its explicit update action without compiling or downloading during the invocation

#### Scenario: Unchanged artifact and data update

- **WHEN** executable inputs are unchanged and a source-linked instruction or configuration file changes
- **THEN** the compatible verified binary is reused and the next consumer observes the updated authoritative data through the existing source connection

#### Scenario: Repair after source changes

- **WHEN** relevant source is newer than the installed manager or its dependent executable
- **THEN** an integrity-verified last accepted manager can perform Check and explicit update/recover/disconnect with compatible metadata without starting an obsolete ordinary runtime, or the documented Rust bootstrap provides the recovery path when that manager cannot safely be used

#### Scenario: Fast candidate is prepared

- **WHEN** the operator explicitly requests fast local delivery
- **THEN** the candidate records its fast profile, passes the same source, compiled-input, integrity and installed-consumer checks, and is distinguishable from a release candidate

#### Scenario: Released plugin consumer has no harness compiler

- **WHEN** a supported host installs and prepares the released Serena plugin without Rust/MSVC or Windows SDK installed solely for harness compilation
- **THEN** its native preparation and MCP entry points run from verified packaged artifacts with declared source/build provenance
- **AND** no ordinary installation, startup or Check compiles harness or requires its development checkout

