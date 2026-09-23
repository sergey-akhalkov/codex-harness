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

The kit SHALL provide a `cargo-fast` skill that discovers the project's
documented native checks first and then applies the narrowest sufficient
incremental Cargo loop - package-scoped `check`, `clippy` and targeted `test`
before any full-suite run - without replacing the project's completion gates.

#### Scenario: Single-crate edit

- WHEN one workspace member changes
- THEN the loop uses `-p` scoped commands and reserves `--workspace` runs
  for pre-completion verification

#### Scenario: Unknown project

- WHEN no documented check commands are found
- THEN the skill states that absence instead of inventing authoritative
  gates and applies only standard Cargo commands

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
