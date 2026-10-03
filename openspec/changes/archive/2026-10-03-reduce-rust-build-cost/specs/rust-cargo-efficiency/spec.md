## MODIFIED Requirements

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
