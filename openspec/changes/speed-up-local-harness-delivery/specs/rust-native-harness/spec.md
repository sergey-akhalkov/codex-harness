## MODIFIED Requirements

### Requirement: Explicit reproducible native build

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

### Requirement: Development feedback and publication retain separate evidence

The supported development route SHALL permit targeted native verification and
an explicit fast local delivery without requiring a cold release publication
when publication behavior is not under test. Publication SHALL retain
source/build identity, actual compiler-input coverage, compiler profile,
toolchain and target compatibility, integrity, installed-consumer acceptance
and recovery. Fast local reuse MAY use a guarded persistent compiler target and
compiled-input evidence, while ordinary publication SHALL retain fresh-target
release preparation. Build-key narrowing, reusable compilation state and
compiler-profile changes SHALL be evaluated separately from this route
separation; adoption SHALL require valid input coverage and measured benefit
without weakening runtime or delivery acceptance. Test or fixture paths MUST
NOT be excluded from build identity solely by their directory names. Unproven
optimization candidates SHALL leave the accepted publication route intact.

#### Scenario: A local formatter or parser change is checked

- **WHEN** relevant behavior can be exercised through the documented package or integration target
- **THEN** the development check can run without first publishing a release build, while completion still includes all applicable broader and delivery checks

#### Scenario: A fixture is compiled into a binary

- **WHEN** a file under a test-like or documentation-like path is a real compiler input
- **THEN** its content affects the appropriate build identity and a changed file cannot reuse an incompatible artifact

#### Scenario: A candidate compiler profile links faster

- **WHEN** a profile candidate improves build time
- **THEN** adoption also checks runtime operations, startup, artifact correctness, publication and recovery against predeclared criteria rather than inferring a total benefit from link time alone

#### Scenario: Test-only source changes

- **WHEN** only files outside every delivered binary's compiled-input evidence change
- **THEN** targeted development checks avoid a needless delivery build and a fast candidate may reuse matching compiled inputs with the relationship reported honestly

## ADDED Requirements

### Requirement: Fast and release native candidates remain distinguishable

Every native candidate SHALL record its compiler profile in its immutable
identity; every delivery receipt SHALL identify the selected profile. Fast
candidates MUST NOT be eligible for ordinary release selection, preview or
fresh-manager delivery unless fast mode is explicitly selected. A newer fast
build SHALL NOT cause release deployment to install non-release binaries. Both
profiles SHALL share ownership, integrity, immutable-file, installed-runtime,
rollback and recovery guarantees.

#### Scenario: Fast build exists before ordinary release deployment

- **WHEN** the owned state contains a newer verified fast candidate and the operator runs ordinary release deployment
- **THEN** release selection filters by release profile and does not silently install the fast candidate

#### Scenario: Explicit fast candidate is delivered

- **WHEN** the operator selects fast deployment and the verified candidate matches the requested source and profile
- **THEN** new sessions resolve that immutable candidate while running sessions keep their existing manager, and the receipt identifies the fast profile

#### Scenario: Fast candidate is altered

- **WHEN** a required binary or recorded identity in a fast candidate is missing or altered
- **THEN** the candidate is not delivered and the existing verified installation or explicit failure path remains intact

#### Scenario: Recovery after a fast delivery

- **WHEN** an explicit recovery or ordinary release deployment follows a fast delivery
- **THEN** it uses its own profile selection and preserves the prior immutable build and recovery evidence
