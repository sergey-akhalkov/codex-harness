# Rust and Cargo efficiency guidance

## Purpose

Bounded kit guidance for modern Rust feature use, fast incremental Cargo
verification loops and safe toolchain/MSRV bumps, routing generic verification
policy to its existing owners.

## Requirements

### Requirement: Modern Rust feature routing

The kit SHALL provide a `rust-modern` skill that gates language and
standard-library feature use on the consuming project's `edition` and
`rust-version`, prefers current stable standard-library replacements over
hand-rolled code or new dependencies, and re-verifies uncertain
stabilizations against current official release notes or standard-library
documentation before relying on them.

#### Scenario: Feature is newer than the project MSRV

- WHEN a useful stable feature postdates the project's `rust-version`
- THEN the skill uses an MSRV-compatible fallback and, where the gain is
  material, proposes an explicit MSRV bump instead of committing unsupported
  code

#### Scenario: Uncertain stabilization claim

- WHEN the skill's dated snapshot disagrees with or omits the needed feature
- THEN guidance defers to the official release notes or std docs for the
  exact version before the feature is used

### Requirement: Fast Cargo verification workflow

The kit SHALL provide a `cargo-fast` skill that discovers the project's documented native checks first and chooses the narrowest sufficient Cargo command for the current question, using package and target selection without replacing completion gates. It SHALL diagnose build-directory size, effective profiles, target multiplicity, toolchain/configuration variation and cache reuse before recommending build/test cost changes. Guidance SHALL distinguish cold builds, warm reuse, edit feedback and test execution; explain debug-information and incremental trade-offs; preserve required coverage and operating limits; and compare total artifact/cache size and observed elapsed time before claiming an improvement. Current official documentation and the actual target platform SHALL govern optional tools and version-sensitive settings.

#### Scenario: Single-crate edit

- WHEN one workspace member changes
- THEN the loop uses the relevant package and target, avoids redundant preparatory commands, and reserves broader checks for their required boundary

#### Scenario: Unknown project

- WHEN no documented check commands are found
- THEN the skill states that absence instead of inventing authoritative gates and applies only standard Cargo commands

#### Scenario: Large target directory

- WHEN a project requests lower build storage and latency
- THEN the skill measures debug information, incremental state and duplicate target roots, distinguishes live reuse from inactive artifacts, and does not treat global Cargo cache cleanup as build-artifact collection

#### Scenario: Optional compiler cache or test runner

- WHEN sccache, an alternative linker or nextest is considered
- THEN the skill checks platform support, cache coverage, process/isolation behavior and required test coverage before adoption, including the added cache footprint

#### Scenario: Transfer outside the kit

- WHEN cargo-fast is invoked in another Rust checkout
- THEN its workflow uses that project's native commands and constraints without requiring harness-specific tools, copying private inputs, or imposing the kit's concurrency settings

### Requirement: Toolchain bump safety

Guidance for raising a toolchain or MSRV SHALL include reading compatibility
notes for every skipped minor release, expecting newly warn-by-default or
deny-by-default lints and behavior shifts, preferring the latest patch
release, and completing the project's warning-as-error check before the bump
is called done.

#### Scenario: Kit MSRV tracks current stable

- WHEN the kit accepts a new stable minor
- THEN `rust-version` and prerequisites documentation move together and the
  documented workspace checks run on the new toolchain

### Requirement: Bounded catalogue contribution

The two skills SHALL carry discriminating descriptions and Rust/Cargo
specific mechanics only. Generic verification policy, regression reproduction
and token discipline remain owned by their existing skills.

#### Scenario: Generic verification request

- WHEN a task asks about verification policy rather than Cargo mechanics
- THEN routing prefers `project-verification` and the two Rust skills stay
  out of scope
