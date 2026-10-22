## MODIFIED Requirements

### Requirement: Publication builds only delivery targets

Native publication and fast local delivery SHALL compile the declared delivery
programs without selecting unrelated test helpers. The complete delivered
binary set, content identity, compiled-input coverage, fresh-input protection,
resource limits and recovery behavior SHALL remain intact. When the source
supplies a valid source-owned delivery declaration before compilation, fast
local delivery and ordinary publication SHALL build the complete delivery set
in one Cargo invocation. A legacy source or an unusable declaration SHALL
retain the existing two-phase source-manager query instead of guessing,
omitting a program or silently widening to every package binary. Ordinary
publication SHALL retain fresh-target fat-LTO preparation.

#### Scenario: Unrelated fixture binary

- **WHEN** a source package also contains a binary used only by tests
- **THEN** publication does not compile it and the normal test route can still build and use it

#### Scenario: Changed publication input

- **WHEN** source bytes change while timestamps are preserved
- **THEN** publication continues to refuse reuse of an artifact for a different source identity

#### Scenario: Source expands the delivery set

- **WHEN** a source revision declares an additional delivery program before its manager has been compiled
- **THEN** the source-owned declaration selects the complete new set in one Cargo invocation, and the resulting candidate is finalized through the existing explicit update path

#### Scenario: Legacy delivery declaration is unavailable

- **WHEN** the source cannot supply a valid pre-compilation delivery declaration but its manager supports the existing delivery query
- **THEN** the two-phase query builds the complete source-owned set and the fallback is visible in the retained build evidence

### Requirement: Measured optimization preserves test coverage and operating limits

Build/test optimization SHALL distinguish compilation, warm dependency reuse,
edited-source feedback and test execution. Consolidation or concurrency
changes SHALL preserve selected test coverage, ignored-test policy, failure
reporting and resource isolation. Fast local profile, persistent-target and
Cargo job-count candidates SHALL be measured separately from publication.
Adoption SHALL use a matched comparison under unchanged CPU/memory limits, with
no unexplained correctness regression. A rejected or inconclusive candidate
SHALL leave the accepted behavior intact and retain its reason. A local fast
profile SHALL also compare artifact size, startup and a representative runtime
path against release before adoption; build-time improvement alone is
insufficient.

#### Scenario: Integration target consolidation

- **WHEN** several integration targets are consolidated
- **THEN** the prior test cases remain discoverable and executable, required callers are updated, and comparison includes compilation/linking and execution

#### Scenario: Higher Cargo concurrency

- **WHEN** a larger job count is evaluated
- **THEN** it remains inside the existing heavy-command budget and is adopted only on measured benefit without failed or omitted required checks

#### Scenario: Fast profile candidate

- **WHEN** a release-optimized profile without fat LTO is evaluated for local delivery
- **THEN** cold build, warm reuse, edited-source feedback, artifact size, startup and a representative runtime path are retained and compared with the release profile

#### Scenario: Test-only feedback

- **WHEN** only test or fixture files change and no production behavior requires delivery
- **THEN** focused development tests can run on the warm development target without creating a release or fast delivery candidate

### Requirement: Reclaimable cache lifecycle

The development workflow SHALL distinguish active reusable build outputs,
inactive regenerable targets and retained diagnostic evidence. A persistent
fast compiler target SHALL be owned by the installation state, have a recorded
key and last successful build, and remain inactive until the explicit fast
delivery path uses it. Cleanup SHALL resolve owned paths, verify inactivity and
preserve unfinished work, live executables, unrelated projects and required
evidence. Foreign or ambiguous ownership SHALL be preserved and reported.
Reports SHALL distinguish logical file sizes from allocated disk space and
SHALL account for additional caches introduced by an optimization.

#### Scenario: Retired development target

- **WHEN** a target is confirmed inactive and contains only regenerable build outputs
- **THEN** scoped cleanup can reclaim it while recording the measured scope and keeping active development reuse

#### Scenario: Uncertain worktree ownership

- **WHEN** a worktree has unfinished work or its runtime ownership is uncertain
- **THEN** its artifacts are preserved and the unresolved ownership is reported

#### Scenario: Fast target is foreign or ambiguous

- **WHEN** the persistent fast target has a missing, foreign or unprovable ownership record
- **THEN** it is not reused or removed for this build, and an owned fresh target or the ordinary preparation error is reported

## ADDED Requirements

### Requirement: Explicit fast local delivery profile

The kit SHALL provide an explicitly selected local fast delivery mode backed
by a maintained profile that keeps release optimizations and stripping while
disabling fat LTO and enabling parallel codegen. The profile identity SHALL be
recorded in the build candidate and delivery receipt. The ordinary `deploy`
command and public publication SHALL retain the release profile with fat LTO.
Fast mode SHALL NOT be selected through an ambient compiler or profile
override, and a missing or incompatible fast profile SHALL fail before
installation changes with an actionable error.

#### Scenario: Fast deployment is explicitly requested

- **WHEN** the operator requests the documented fast local deployment mode
- **THEN** the candidate uses the fast profile, the receipt names that profile and its build identity, and installation proceeds only after the normal integrity, source and runtime checks

#### Scenario: Ordinary deployment remains release

- **WHEN** the operator runs ordinary `deploy` without the fast selector
- **THEN** the existing release profile, fat-LTO publication and fresh-target path remain in effect

#### Scenario: Fast profile is unavailable

- **WHEN** the source or toolchain cannot provide the declared fast profile
- **THEN** the command reports the missing profile before mutating installation and does not silently substitute release or an ambient override

### Requirement: Fast delivery reuses a guarded owned compiler target

Fast local delivery MAY reuse a persistent compiler target owned by the
installation state, keyed by the toolchain, target triple, profile, lockfile,
manifests, Cargo host configuration and other build-affecting inputs. Before
reuse, it SHALL compare the current source identity with the last successful
fast build. If a changed compiled input has a timestamp that is not newer than
the recorded build epoch, or ownership or key evidence is missing or
ambiguous, the build SHALL force a scoped rebuild of affected workspace
packages or use a fresh target. It MUST NOT trust Cargo's mtime cache for an
untrusted changed input. Final binaries SHALL still be copied into an immutable
integrity-verified candidate. The ordinary release path SHALL remain
fresh-target.

#### Scenario: Warm dependency reuse

- **WHEN** a fast build repeats with unchanged dependency inputs and a small workspace change
- **THEN** eligible dependency artifacts are reused and the changed workspace package is rebuilt without weakening the final identity or integrity checks

#### Scenario: Changed bytes have a preserved timestamp

- **WHEN** a compiled input changes while its timestamp is not newer than the recorded fast build epoch
- **THEN** the affected package is rebuilt or a fresh target is used, and the stale artifact is never accepted as the changed source

#### Scenario: Fast target has foreign ownership

- **WHEN** the persistent target directory or its ownership record does not belong to this installation state
- **THEN** it is preserved untouched and the build uses an owned target or reports the explicit preparation error

#### Scenario: Release publication remains fresh-target

- **WHEN** ordinary release publication runs after a fast local build
- **THEN** it does not inherit the fast target's freshness assumptions and retains the existing fresh-target publication guarantees

### Requirement: Compiled-input identity permits safe reuse

The build record SHALL retain the union of actual compiled inputs from the
dep-info evidence for every delivered binary, together with build-affecting
manifest, lockfile, toolchain and Cargo host-configuration identities. Fast
delivery MAY reuse verified delivered binaries when every recorded compiled
input and build-affecting input matches, even when only noncompiled test,
documentation or fixture files changed. A test-like or documentation-like path
that is a compiled input SHALL invalidate the matching artifact. Missing,
changed or ambiguous compiled-input evidence SHALL force a rebuild rather than
authorize reuse.

#### Scenario: Test-only change

- **WHEN** only a test file that is absent from every delivery binary's dep-info union changes
- **THEN** a verified fast candidate may be reused with the current checkout relationship reported honestly

#### Scenario: Fixture is compiled into a delivery binary

- **WHEN** a fixture or documentation-like file is present in a delivery binary's compiled-input evidence and its bytes change
- **THEN** the affected candidate is rebuilt and cannot be reused under the previous compiled-input identity

#### Scenario: Unrelated new file

- **WHEN** a new file appears outside every recorded compiled input and does not affect manifests, toolchain or Cargo configuration
- **THEN** it does not by itself force a delivery rebuild, and the receipt retains the relevant compiled-input identity

#### Scenario: Build-affecting input changes

- **WHEN** the lockfile, manifest, toolchain or Cargo host configuration changes
- **THEN** previously recorded compiled-input evidence cannot authorize the unchanged binary set without a new compatible build
