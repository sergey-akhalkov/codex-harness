---
name: rust-modern
description: Apply current stable Rust language and standard-library features when writing or reviewing Rust, gated by the project's edition and MSRV, including toolchain-bump compatibility checks. Use for Rust code changes, refactors, dependency-avoidance and MSRV decisions. Skip non-Rust languages and nightly-only features.
---

# Modern Rust usage

## Gate every feature on the project

1. Read the workspace `Cargo.toml` for `edition` and `rust-version`, member
   overrides, and `rust-toolchain.toml` when present; run `rustc --version`
   when the installed toolchain matters.
2. Use a stable feature only when the project's MSRV covers it. When a newer
   feature would clearly remove code, allocations or a dependency, propose an
   explicit MSRV bump as a separate decision instead of committing unsupported
   code, and offer the MSRV-compatible fallback meanwhile.
3. Verify uncertain stabilizations against the official
   [release notes](https://github.com/rust-lang/rust/blob/master/RELEASES.md)
   or [std docs](https://doc.rust-lang.org/std/) for the exact version. The
   snapshot in [stable features](references/stable-features.md) is dated
   2026-09-23 and covers 1.88-1.98; the official source wins on any doubt.

## Prefer the standard library

Before adding a dependency or hand-rolling logic, check the modern std form.
Highest-leverage examples: let chains instead of nested `if let`; `push_mut`
returning `&mut T` instead of push-then-index; `fmt::from_fn` for one-off
`Display`; `format_into` with `fmt::NumBuffer` for allocation-free integer
formatting; `substr_range` for parser spans; `assert_matches!` in tests;
`LazyLock::from`/`get` instead of hand-rolled singletons; integer `*_one` /
`bit_width` helpers instead of shift tricks; `RwLockWriteGuard::downgrade`,
`extract_if`, `array_windows`/`as_chunks`, path helpers and `Duration`
constructors. See the reference for the full version-gated table and what
each item replaces.

## Edition 2024 baseline

- Let chains in `if`/`while` (1.88, edition 2024) collapse nested binding
  checks; use them where they read better than early returns.
- `unsafe extern` blocks and `#[unsafe(...)]` attribute forms; no references
  to `static mut` - use atomics, `OnceLock`/`LazyLock` or `UnsafeCell`.
- `resolver = "3"` for edition 2024 workspaces.
- The [Rust book](https://doc.rust-lang.org/book/) assumes edition 2024;
  consult its closures/iterators, smart pointers and error-handling chapters
  for fundamentals rather than duplicating them here.

## Toolchain and MSRV bumps

- Raise `rust-version` deliberately, not as a side effect. Read the
  Compatibility Notes of every skipped minor; the reference lists the ones
  that repeatedly matter for `-D warnings` builds.
- Expect new warn/deny lints (for example `mismatched_lifetime_syntaxes` in
  1.89, `const_item_interior_mutations` in 1.93, `unused_visibilities` in
  1.94, must-use on `Result`/`ControlFlow` in 1.97, `c_void_returns` in
  1.98) and behavior shifts (prelude macro ambiguity in 1.94, the `pin!`
  coercion fix and v0 symbol mangling default in 1.97, `assert_eq` temporary
  scope and the derived `PartialOrd` fast path in 1.98, stricter
  `BTreeMap::append` on inconsistent `Ord` in 1.96).
- Stay on the latest patch release: 1.96.1 and 1.97.1 fixed compiler
  miscompilations, and Cargo patch releases carried CVE fixes.
- Finish a bump with the project's full warning-as-error check and native
  tests on the new toolchain. Adopt new APIs opportunistically in touched
  code; a repo-wide rewrite needs its own change and rationale.

For build and test loop mechanics, use the `cargo-fast` skill.
