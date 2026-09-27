## ADDED Requirements

### Requirement: Compact output covers documented Cargo verification

The installed native compression boundary SHALL support human-readable Cargo test, check, build and clippy invocations used in the kit's verification recipes, including package/workspace selection, locked dependencies, job limits and documented arguments after the Cargo separator. The documented invocation through the existing heavy-command resource owner SHALL reach that compression boundary without bypassing resource admission or changing the native Cargo argument vector, cwd, environment, stdin or child exit status. Cargo SHALL execute exactly once. The ordinary user path SHALL require no shell interception, manual log conversion or additional model turn to obtain a compact result.

#### Scenario: The documented serialized workspace test
- **WHEN** an installed consumer runs the documented compact heavy-command route for `cargo test --workspace --locked --jobs 1 -- --test-threads=1`
- **THEN** Cargo receives those arguments once under the existing resource ownership, recognized verbose output is compacted and the actual test exit status is preserved

#### Scenario: Package-scoped checks and lint failures
- **WHEN** the consumer selects a package with `-p`, uses `--locked` and a job limit for test/check/build/clippy, or passes `-D warnings` after the clippy separator
- **THEN** supported forms remain eligible for compression, and a compile or lint failure remains a failure even if no tests executed

#### Scenario: Raw output is the appropriate representation
- **WHEN** output is interactive, binary, machine-readable, unrecognized, explicitly bypassed, or would not become smaller after the complete compact presentation
- **THEN** original output remains available without fabricated success, lost records or command re-execution, and exact source reads and final review diffs stay raw

### Requirement: Verification diagnostics retain both streams and recovery

For selected compact Cargo execution, the adapter SHALL account for stdout and stderr without deadlock, preserve diagnostically relevant failures and unknown diagnostic blocks, and provide bounded access to retained originals with their stream identity. Output capture, retention and any loss of temporal interleaving fidelity SHALL be explicit. A nonzero child result SHALL NOT become success because filtering succeeded or a test summary is absent. Filter, archive and parser failures SHALL preserve a usable raw path without rerunning the child. Exceeding the capture limit SHALL preserve live raw output with an explicit retention limit. Long-running commands SHALL retain bounded progress visibility.

#### Scenario: Failure appears only on stderr
- **WHEN** Cargo emits a compiler error on stderr and no test summary on stdout
- **THEN** the result exposes the error and nonzero child outcome, and retained evidence identifies its original stream without inventing test counts

#### Scenario: Both pipes exceed their buffer capacity
- **WHEN** the native command writes large stdout and stderr concurrently
- **THEN** both streams are drained without a pipe deadlock, and capture limits do not silently discard diagnostics

#### Scenario: Formatting or retention fails
- **WHEN** the selected formatter fails, times out or produces unrecognized content, or raw retention cannot be completed
- **THEN** the original output remains usable, the presentation problem is distinguished from the child result, and the command is not executed a second time

#### Scenario: Detail is requested after completion
- **WHEN** a caller follows the compact result's raw locator or observation handle
- **THEN** available original evidence can be read without running Cargo again, and expired, truncated or unavailable evidence is reported explicitly

### Requirement: Compression coverage is observable without corrupting raw output

The existing token-workflow evidence surface SHALL make the compression decision, concrete bypass/fallback reason, measured raw bytes and delivered bytes available for an explicitly inspected invocation. Unsupported flags, unsupported commands, disabled/interactive execution, short or non-shrinking output and formatter failure SHALL be distinguishable. Missing measurements SHALL remain unavailable. Routine raw and machine-output calls SHALL NOT receive telemetry embedded in their payload, and byte measurements SHALL NOT be presented as token or subscription savings.

#### Scenario: Hook rewriting does not imply compression
- **WHEN** an explicit adapter invocation is rewritten by the hook but its actual command arguments are unsupported
- **THEN** diagnostic evidence identifies the bypass and reason instead of reporting that compression was applied

#### Scenario: Wrapper diagnostics and child diagnostics differ
- **WHEN** heavy-command admission fails before Cargo starts or the resource owner later terminates its process tree
- **THEN** that outcome remains visible as a wrapper/resource failure and is not mislabeled as a completed Cargo check or compression success
