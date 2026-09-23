---
name: cargo-fast
description: Speed up safe Cargo build and verification cycles - targeted incremental check/clippy/test loops, manifest and profile hygiene, dependency duplicate triage and quick experiment setups. Use when building, testing or optimizing Rust compile/test feedback. Skip generic verification policy and non-Cargo ecosystems.
---

# Fast Cargo workflows

## Start from the project's own checks

Discover the documented or CI commands first (README, AGENTS.md, docs,
workflow files). They remain the completion gates; this skill only shortens
the path to them. When none exist, say so and use plain Cargo commands.

## Narrowest sufficient loop

- After an edit: `cargo fmt --all` (cheap, prevents diagnostic churn), then
  `cargo check -p CRATE`, then `cargo clippy -p CRATE --all-targets --
  -D warnings`, then a targeted `cargo test -p CRATE FILTER` (or `--test
  NAME`, `--exact`; `--no-run` to compile only). Reserve
  `--workspace --all-targets` runs for pre-completion verification.
- Package-scoped `-p` beats whole-workspace rebuilds after cross-crate edits
  touch one member; use `--workspace` checks when the edit spans members.
- Keep `--locked` for reproducibility when a lockfile exists; `-m PATH`
  (1.97) is the shorthand for `--manifest-path` in out-of-tree invocations.
- `cargo fix` and `cargo clippy --fix` apply only to selected targets since
  1.89; review their diffs like any edit.
- On Windows, retained test executables can lock the build; give parallel
  verification roots a dedicated `--target-dir` instead of an ambient
  `CARGO_TARGET_DIR`. When the project defines a build queue or wrapper, run
  heavy commands through it.

## Build-performance defaults

From the official
[build performance guide](https://doc.rust-lang.org/cargo/guide/build-performance.html):
measure against the workflows you care about, then consider

```toml
[profile.dev]
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false

[profile.debugging]
inherits = "dev"
debug = true
```

for faster builds and links with an opt-in debugging profile. Use
`cargo build --timings` to find slow crates; evaluate alternative linkers per
platform. Workspace-wide feature unification, Cranelift and `-Zthreads` are
nightly-only - keep them out of stable-MSRV projects. Cargo garbage-collects
its global cache automatically since 1.88.

## Manifest and config hygiene

- Centralize versions in `[workspace.dependencies]` with
  `dep = { workspace = true }`, and lints in `[workspace.lints]` with
  `[lints] workspace = true`.
- Use `resolver = "3"` with edition 2024 workspaces.
- Cargo config `include` (1.94) shares configuration files;
  `build.warnings` (1.97) enforces warning-free local packages from config;
  `build.build-dir` (1.91) relocates intermediate artifacts;
  `resolver.lockfile-path` (1.97) helps read-only source directories.

## Dependency triage

- `cargo tree -d` lists duplicate versions; `cargo tree -i CRATE` shows why a
  dependency is present; `cargo add`/`cargo remove` keep manifests tight;
  `cargo update --precise VER` makes minimal, reviewable bumps.
- Periodically review unused dependencies and features (nightly
  `-Zcargo-lints` or third-party scanners). Treat findings as evidence to
  check, not as automatic removals.

## Quick experiments without new projects

- Prefer an `examples/` target or a scratch test in the real crate: same
  toolchain, features and dependencies, no new manifest.
- Single-file `cargo script` remains nightly (`-Zscript`); stable
  alternatives are third-party (`rust-script`, `cargo-play`) - assess such a
  tool before installing it. See `rust-modern` for feature questions inside
  experiments.

The [workflows reference](references/workflows.md) expands these into
copy-ready command sequences and manifest snippets.
