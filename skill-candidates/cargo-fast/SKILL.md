---
name: cargo-fast
description: Diagnose and reduce Cargo build, test and disk cost - the narrowest sufficient command for the current question, debug-info and incremental trade-offs, target/test multiplicity, safe owner-bound cleanup, workload-specific measurement, and platform-gated optional caches, runners and linkers. Use when building, testing or optimizing Rust compile/test feedback or reclaiming build storage. Skip generic verification policy and non-Cargo ecosystems.
---

# Fast Cargo workflows

## Start from the project's own checks

Discover the documented or CI commands first (README, AGENTS.md, contributor docs, workflow files). They remain the completion gates; this skill shortens the path to them and never replaces or weakens them. When none are documented, state that absence instead of inventing gates, and use plain Cargo commands.

## Choose the narrowest sufficient command

Match the command to the current question; do not run a fixed fmt/check/clippy/test sequence after every edit.

- Does it compile? `cargo check -p CRATE` (no codegen, no link).
- Does one behavior hold? `cargo test -p CRATE FILTER` (or `--test NAME`, `--exact`); `--no-run` compiles tests without executing them.
- Lint or API risk the project gates on? `cargo clippy -p CRATE --all-targets -- -D warnings`.
- Do edits span members or a shared type? Widen `-p` to `--workspace` for that step.
- At the completion boundary? Run the project's full gates (workspace tests, doctests, everything the project requires).

Formatting belongs in the loop only where the project gates on it. Keep `--locked` when a lockfile exists, and `-m PATH` (short for `--manifest-path`) for invocations outside the workspace root. Package-scoped runs beat whole-workspace rebuilds; reserve the broad runs for their required boundary.

## Measure before claiming an improvement

Cold compilation, warm reuse (unchanged sources), edit feedback (one small source change) and test execution are separate workloads with separate bottlenecks. Measure them separately, before and after, with matched inputs (toolchain, features, profile, target root). Compare wall or compiler time (`cargo build --timings`, shell timing) and logical bytes of the target roots you own. A scoped pair supports a scoped claim, never a universal number.

## Profiles: compact development, explicit full debugging

The official build-performance recipe, with one addition for dependency backtraces:

```toml
[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false

[profile.debugging]
inherits = "dev"
debug = true

# custom profiles inherit the dev wildcard, so re-enable dependency debug info
[profile.debugging.package."*"]
debug = true
```

`line-tables-only` keeps filename/line backtraces (panic messages stay useful) without variable or parameter info. `--profile debugging` is the explicit full-debug route for workspace members and dependencies; it builds into its own `target/debugging` directory, so it costs one full rebuild - keep it out of routine loops. `cargo test` inherits dev, so tests get compact info too. String debug values need Rust >= 1.71; on older MSRV use a numeric form (`debug = 1`) or leave the default. Never weaken debug assertions, overflow checks or test coverage to save time.

## Reuse and storage lifecycle

- Keep incremental compilation for local edit loops (dev/test enable it for workspace members and path dependencies). Cargo defaults it off when the `CI` environment variable is set; set `CARGO_INCREMENTAL=0` explicitly for disposable or reproducible builds.
- Target roots multiply: the default `target`, `--target-dir`/`CARGO_TARGET_DIR` experiment roots, per-profile directories, and one linked executable per test/bench/example target. Count before concluding what dominates.
- Cargo's global cache self-cleans (automatic GC since 1.88; `cache.auto-clean-frequency` defaults to 1 day; not run in offline mode) and it never removes target directories. Target cleanup is explicit: `cargo clean` scoped by `-p`, `--profile`, `--target` or `--target-dir`, with `--dry-run` to preview. Retire only whole roots you own and have verified inactive; keep the active root warm and never hand-delete fingerprint or incremental internals.

## Optional tools are gated

sccache, nextest and alternative linkers come only after measurement shows the remaining bottleneck and platform/compatibility checks pass. Decision-relevant limits: sccache cannot cache incremental crates (so it forces incremental off) or crates that link (bin, dylib, cdylib, proc-macro) and adds its own disk cache; nextest runs each test in a separate process and does not run doctests, so keep `cargo test --doc` in the gates; the official linker suggestions are Linux-specific - do not transplant them to Windows. Commands and checks for all three: [references/workflows.md](references/workflows.md).

## Manifest and feature hygiene

Fix cost in the manifest where possible: workspace dependency and lint tables, duplicate versions via `cargo tree -d`, unused dependencies and features treated as evidence rather than automatic removals, minimal `cargo update --precise` bumps. Verify experiments with an `examples/` target or a scratch test in the real crate instead of a new project. Nightly-only options (unstable `-Z` flags, Cranelift backend) stay out of stable projects. Command details: [references/workflows.md](references/workflows.md).
