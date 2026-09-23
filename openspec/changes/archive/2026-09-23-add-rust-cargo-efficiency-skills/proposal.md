## Why

Rust 1.90-1.98 stabilized a large set of language and standard-library
features (`if let` match guards, `cfg_select!`, `Vec::push_mut`,
`str::substr_range`, allocation-free integer formatting, `assert_matches!`,
integer bit helpers, path/duration/atomic conveniences) plus Cargo workflow
features (`build.warnings`, config `include`, `build.build-dir`, the `-m`
shorthand). Agents default to older idioms and re-research release notes per
task, while the kit MSRV (1.89) blocks current stable features even though
the machine toolchain is already 1.98.1.

## What Changes

- Add two global kit skills with scoped triggers:
  - `rust-modern` - edition/MSRV-gated modern Rust usage, standard-library
    first replacements for hand-rolled patterns, and a toolchain-bump
    compatibility checklist.
  - `cargo-fast` - narrowest-sufficient Cargo verification loops, manifest
    and profile hygiene, dependency triage and quick experiment setups.
- Raise the workspace `rust-version` from 1.89 to the current stable minor
  (1.98) and update [Rust native](../../../../docs/rust-native.md)
  prerequisites. No code migration sweep: newer APIs are adopted
  opportunistically where code is already being changed.
- Record the current-stable MSRV policy in project decisions.
- Reconcile the global skill registration with the core lifecycle when the
  checkout is next in a shippable state; `update` requires an
  integrity-verified build matching the current source.

## Capability Impact

### Added Capabilities

- `rust-cargo-efficiency`: modern-feature routing with MSRV/edition gating,
  fast Cargo verification workflows, toolchain bump safety, and bounded
  catalogue contribution.

## Impact

Files: `.agents/skills/rust-modern/`, `.agents/skills/cargo-fast/`, root
`Cargo.toml`, `docs/rust-native.md`, `docs/project-decisions.md`, this
change. Consumers: Rust work in the kit and in projects using the global
skills. Risks: the catalogue grows by two entries (mitigated by narrow
descriptions and no overlap with `project-verification`,
`reproduce-regression` or `token-efficient-workflow`); MSRV 1.98 requires
current-stable toolchains from consumers (accepted kit policy). Binary
deployment stays with the existing deploy flow and is not part of this
change while the checkout carries an unrelated in-flight migration.
