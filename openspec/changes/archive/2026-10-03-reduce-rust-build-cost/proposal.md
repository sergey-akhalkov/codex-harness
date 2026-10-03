## Why

Repeated development and test builds retain substantial debug information and incremental artifacts, while separately linked integration targets and unnecessary publication binaries add compilation work. The owner wants smaller, faster Rust development with useful backtraces, an explicit full-debugging route, unchanged verification coverage, and a reusable procedure for other projects.

## What Changes

- Use compact debug information for ordinary development and test builds, with a separate full-debugging profile and retained incremental development reuse.
- Compile the declared delivery binaries for native publication instead of every binary target in the selected packages; preserve build identity, integrity, resource admission and installation recovery.
- Compare a bounded consolidation of integration targets and modest Cargo concurrency under the existing machine budget; adopt only improvements that preserve test identity, isolation and required coverage.
- Establish a measured cache lifecycle for active versus retired development targets, reclaim only proven inactive regenerable artifacts, and document the remaining ownership boundaries.
- Extend the existing `cargo-fast` skill with disk diagnosis, workload-specific trade-offs, cache lifecycle and attributable before/after checks; validate and deliver it through the existing skill and installation lifecycle.
- Measure cold compilation, warm reuse, edit feedback and required test execution separately. Preserve official-source rationale and accepted/rejected results in their existing documentation owners.

## Capabilities

### New Capabilities

- `rust-build-efficiency`: Compact development defaults, precise publication targets, measured build/test optimization and safe cache lifecycle in the kit.

### Modified Capabilities

- `rust-cargo-efficiency`: Extend `cargo-fast` from incremental command guidance to measured disk and build/test cost reduction in consuming Rust projects.

## Impact

Affected owners are the workspace Cargo manifest, native publication command construction and tests, selected integration-test manifests/entrypoints, native verification guidance, and the owned `cargo-fast` package. The published command names, seven delivery binaries, stable Rust/MSRV, mandatory tests and machine CPU/memory limits remain compatible. No new external dependency or background cleaner is required. Existing unrelated working-tree changes and unfinished executor work are preserved. Global delivery and an outside-checkout skill check are part of completion.
