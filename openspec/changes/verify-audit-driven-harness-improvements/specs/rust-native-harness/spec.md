## MODIFIED Requirements

### Requirement: Native build scratch reclamation

The explicit native build lifecycle SHALL reclaim only demonstrably owned abandoned scratch beneath a dedicated short harness scratch root. Deletion SHALL require validated ownership, confinement and evidence that no live operation holds the scratch lease; a matching prefix and age alone MUST NOT authorize removal. Active scratch SHALL remain intact regardless of age. Cleanup SHALL skip reparse points, preserve data outside the owned root, avoid following replacement links and remain best-effort so cleanup failures do not fail a build. Legacy process-temp entries without recoverable ownership evidence SHALL be left intact with bounded recovery guidance rather than swept by their names.

#### Scenario: Abandoned scratch is reclaimed
- **WHEN** an interrupted operation leaves validated owned scratch with no live lease and the documented retention condition is met
- **THEN** explicit management can reclaim it without affecting another operation

#### Scenario: Fresh and foreign entries survive
- **WHEN** process temp contains a stale foreign directory with a harness-like prefix, an active old directory, a fresh directory or a reparse point
- **THEN** none is removed merely because its name or timestamp matches the former sweep rule

#### Scenario: A junction targets outside data
- **WHEN** an owned-looking scratch entry or relevant path component resolves outside the verified root through a reparse point
- **THEN** reclamation refuses that target and outside sentinel data survives

#### Scenario: Interrupted owner and a reused process identifier
- **WHEN** recovery encounters a stale owner record or a reused process identifier
- **THEN** it establishes ownership and lease liveness without treating the numeric identifier or age alone as proof of abandonment

## ADDED Requirements

### Requirement: Ordinary launch separates runtime integrity from freshness

Ordinary installed-runtime selection SHALL establish required executable integrity and compatibility without traversing and hashing the checkout's compiled-source inputs solely to diagnose source freshness. Explicit check, diagnose, build and update operations SHALL retain accurate fresh, stale and unavailable-source states. Live linked configuration and other source-owned data SHALL continue to be consumed as required. Integrity failure SHALL prevent use of the invalid harness runtime while preserving the established native Codex fallback: the requested upstream payload starts at most once with native arguments and local settings. Per-launch reuse of verification SHALL NOT weaken integrity or introduce an indefinite timestamp-only trust cache.

#### Scenario: Valid runtime and modified source
- **WHEN** an ordinary session starts with an intact installed build and changed compilation sources
- **THEN** no compiled-source freshness scan is performed for launch selection, required live shared data is still read and the valid installed runtime remains usable

#### Scenario: Explicit diagnosis needs source status
- **WHEN** check or diagnose examines unchanged, changed or unavailable compilation sources
- **THEN** it distinguishes those conditions accurately and separately from executable integrity

#### Scenario: A dependent harness executable is altered
- **WHEN** a required runtime binary fails integrity verification
- **THEN** that harness build is not authorized and an otherwise valid upstream CLI remains available through the existing one-launch fallback

### Requirement: Build selection avoids irrelevant expensive validation

Build reuse selection SHALL reject candidates that cannot match the already established source/toolchain/target identity before invoking expensive consumer validation. A selected candidate SHALL still pass the required integrity and consumer checks. Duplicate hashing of the same unchanged executable within one verification decision SHALL be avoided where verification identity can safely be reused; changed or ambiguous state SHALL invalidate that reuse. Diagnostic evidence SHALL distinguish rejected candidates, performed validations and source reads.

#### Scenario: Many builds have different source identities
- **WHEN** reuse selection encounters clearly nonmatching records before a matching candidate
- **THEN** it avoids their consumer probes and still performs the complete required validation of the candidate it selects

#### Scenario: Verification inputs change
- **WHEN** an executable or relevant identity changes during or after a verification decision
- **THEN** earlier evidence is not reused to authorize the changed artifact

### Requirement: Development feedback and publication retain separate evidence

The supported development route SHALL permit targeted native verification of a change without requiring a cold release publication when publication behavior is not under test. Publication SHALL retain source/build identity, actual compiler-input coverage, toolchain and target compatibility, integrity, installed-consumer acceptance and recovery. Build-key narrowing, reusable compilation state and compiler-profile changes SHALL be evaluated separately from this route separation; adoption SHALL require valid input coverage and measured benefit without weakening runtime or delivery acceptance. Test or fixture paths MUST NOT be excluded from build identity solely by their directory names. Unproven optimization candidates SHALL leave the accepted publication route intact.

#### Scenario: A local formatter or parser change is checked
- **WHEN** relevant behavior can be exercised through the documented package or integration target
- **THEN** the development check can run without first publishing a release build, while completion still includes all applicable broader and delivery checks

#### Scenario: A fixture is compiled into a binary
- **WHEN** a file under a test-like or documentation-like path is a real compiler input
- **THEN** its content affects the appropriate build identity and a changed file cannot reuse an incompatible artifact

#### Scenario: A candidate compiler profile links faster
- **WHEN** a profile candidate improves build time
- **THEN** adoption also checks runtime operations, startup, artifact correctness, publication and recovery against predeclared criteria rather than inferring a total benefit from link time alone
