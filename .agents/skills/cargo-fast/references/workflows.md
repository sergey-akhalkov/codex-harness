# Cargo workflow cookbook

## Incremental verification loop (pwsh)

```powershell
cargo fmt --all
cargo check -p harness-core --locked
cargo clippy -p harness-core --all-targets --locked -- -D warnings
cargo test -p harness-core --locked dependency -- --test-threads=1
# pre-completion:
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Substitute the real crate and the project's documented gates. `--no-run`
compiles tests without executing them; `--test NAME` selects one integration
test binary; `--exact` matches a single test path.

## Centralized lint and dependency tables

```toml
[workspace.lints.rust]
unsafe_code = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
```

```toml
# member Cargo.toml
[lints]
workspace = true

[dependencies]
serde = { workspace = true }
```

## Finding what makes builds slow or big

```powershell
cargo build --timings          # per-crate time, parallelism
cargo tree -d                  # duplicate dependency versions
cargo tree -i serde            # why a crate is pulled in
cargo check --workspace --locked
```

Review duplicates by `cargo update --precise` on the offending edge, or by
removing the feature that activates the extra version. Confirm each removal
with the consumers that still build.

## Minimal dependency bumps

```powershell
cargo update --precise 1.0.210 -p serde
```

Prefer minimal precise bumps for security or bug fixes; full `cargo update`
runs are separate reviewable changes.

## Quick experiments

- Add `examples/probe.rs` and run `cargo run -p CRATE --example probe`.
- For a behavior check, add a temporary `#[test]` and run it with a filter;
  delete it or promote it into a real regression test afterwards.
- `cargo test --doc -p CRATE` checks examples in documentation; doctests
  also run when cross-compiling (1.89+).
