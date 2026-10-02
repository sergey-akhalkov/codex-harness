# Cargo workflow cookbook

Command examples use PowerShell; the Cargo flags are identical in other shells. Replace `CRATE`, `PATH` and `FILTER` with the project's real values. This file expands the diagnosis and decision procedure that [`SKILL.md`](../SKILL.md) summarizes.

## 1. Workloads and a matched baseline

```powershell
# effective target root first (honors config and environment)
cargo metadata --format-version 1 --no-deps | ConvertFrom-Json | Select-Object workspace_root, target_directory

# cold build of the workflow under test (fresh target root)
cargo build -p CRATE --locked --timings     # report under <target>/cargo-timings/
# warm reuse: rerun with no source change
cargo build -p CRATE --locked
# edit feedback: restore one small representative edit, then the narrowest command
cargo check -p CRATE --locked
# test execution: the project's own selected test command
cargo test -p CRATE FILTER --locked
```

Size breakdown of one root:

```powershell
$root = 'PATH\to\target'
Get-ChildItem $root -Recurse -File | Measure-Object -Property Length -Sum | Select-Object Count, Sum
Get-ChildItem $root -Directory | ForEach-Object {
  $s = (Get-ChildItem $_.FullName -Recurse -File | Measure-Object -Property Length -Sum).Sum
  [pscustomobject]@{ Dir = $_.Name; MB = [math]::Round($s / 1MB, 1) }
}
Get-ChildItem "$root\debug\deps" -Filter *.exe -ErrorAction SilentlyContinue | Measure-Object
```

- Report logical file length; NTFS allocated size can differ under compression or sparse files, so compare like with like.
- Keep raw numbers in local project storage; commit only the conclusions the project needs.
- One cold pair plus a few warm observations is a scoped measurement - say so instead of implying a population claim. An apparent regression worth acting on deserves one cause-driven confirmation before changing defaults.

## 2. Effective configuration: profiles, target dirs, toolchain, features

- The authoritative stable check is a verbose build: `cargo build -v -p CRATE` (or `cargo check -v`) prints each rustc invocation with the applied `-C debuginfo=...`, `-C incremental=...`, `--out-dir` and `--crate-name`. What appears there is what actually runs.
- `cargo config get` is nightly-only as of Cargo 1.98 - do not build a workflow on it. Read config files instead: Cargo searches `.cargo/config.toml` from the invocation directory upward, then `$CARGO_HOME/config.toml`; environment variables (`CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR`, `CARGO_BUILD_JOBS`, `CARGO_INCREMENTAL`, `RUSTFLAGS`) override files. Because discovery depends on the invocation directory, run from the workspace root for reproducible flags.
- Effective target root: `cargo metadata --format-version 1 --no-deps` reports `target_directory` after config and environment resolution.
- Toolchain identity: `rustc -vV` and `rustup show`. A different toolchain or target triple is a different build (separate fingerprints, often separate directories) - match the arms before comparing.
- Features: `cargo tree -e features` and `cargo tree -i CRATE` show which edges activate what; `--all-features`/`--no-default-features` change both time and artifact size. Compare arms with identical feature selection, and do not change resolver or edition settings as part of cost work - it invalidates comparisons and can change feature unification.

## 3. Debug information and the debugging profile

- Values: `false`/`0`/`"none"`; `"line-tables-only"` = minimal info for filename/line backtraces, no variables or parameters; `"line-directives-only"` is for specific targets (prefer line-tables-only elsewhere); `1`/`"limited"`; `2`/`true`/`"full"` (the dev default). String values require Rust 1.71+.
- Wildcard scoping: `[profile.<name>.package."*"]` applies to dependencies that are not workspace members. A path dependency inside the workspace directory becomes a workspace member automatically unless excluded - it keeps the member profile, not the wildcard. Registry dependencies and path dependencies outside the workspace take the wildcard.
- Inheritance: custom profiles inherit the parent's package overrides. With the standard dev recipe (`debug = "line-tables-only"` for members, `debug = false` for dependencies), `--profile debugging` keeps dependencies stripped unless the debugging profile re-enables them; that is why the recipe adds `[profile.debugging.package."*"] debug = true`. Verified on Cargo 1.98 with `cargo build -v --profile debugging`.
- Tests inherit `dev`; benches inherit `release`. `--profile debugging` output lives in `target/debugging` and needs a full rebuild the first time - use it deliberately when a debugger session needs variables and full dependency info.
- Build scripts and proc macros use `build-override` defaults (opt-level 0, debug off when possible); leave them alone unless you are debugging the build itself.
- Keep the dev profile honest: do not add `strip` or disable debug assertions/overflow checks for a quick win; backtraces, tests and runtime behavior must stay correct.

## 4. Incremental local reuse versus disposable builds

- dev/test enable incremental compilation for workspace members and path dependencies by default; search for `-C incremental=` in `-v` output to confirm.
- When the `CI` environment variable is set, Cargo defaults incremental off (absent other configuration); for packaging or cache experiments, `CARGO_INCREMENTAL=0` (or profile `incremental = false`) makes the build disposable and stops incremental state from growing.
- Compiler caches: sccache cannot cache incrementally compiled crates, so adopting it means turning incremental off - a real trade-off between local edit reuse and cache sharing, decided per project.
- Incremental state lives under `target/<profile>/incremental` and grows with edit churn; remove it by retiring the whole root or a scoped `cargo clean`, never by deleting inside it.

## 5. Target roots: inventory, footprint and safe retirement

- Enumerate the roots in play: effective `target_directory` (above), known `--target-dir`/`CARGO_TARGET_DIR` experiment roots, and per-worktree roots. Each root holds per-profile subdirectories (`debug`, `debugging`, `release`, ...) with `deps/`, `incremental/` and `build/`.
- What typically dominates on Windows MSVC debug trees: PDB files next to every test/binary artifact, incremental state, and the count of separately linked test/bench/example executables; variant roots duplicate all of it. Count executables and measure by subdirectory before acting.
- Retirement checklist for one root: resolve the full path; confirm ownership (your checkout/worktree, not another's) and inactivity (no running cargo/rustc/test process, no unfinished work that will reuse it); record size and shape; preview with `cargo clean --target-dir PATH --dry-run` (prints file count and total); then retire exactly that root. Keep the root serving the current worktree.
- Scoped cleanups when full retirement is too blunt: `cargo clean -p CRATE`, `--profile NAME`, `--target TRIPLE`, `--doc`, `--release`. `cargo clean` never touches the global cache.
- Concurrency: builds sharing one target dir contend and serialize; parallel checkouts are better isolated by separate `--target-dir` roots than by one ambient `CARGO_TARGET_DIR`. On Windows, a running test executable can lock outputs - stop it or build into a dedicated root.
- Two separate lifecycles: the global cache (`CARGO_HOME`) self-cleans with Cargo's automatic GC (since 1.88; frequency `cache.auto-clean-frequency`, default 1 day; skipped in offline/frozen mode; access tracked since 1.78) and never removes target outputs; target roots are removed only by explicit `cargo clean` or an owner-bound retirement. When reporting total footprint, include any sccache cache directory too.
- Retire caches at worktree retirement or demonstrated disk pressure, not before every build; no background cleaner is required.

## 6. Optional tools: decide from measurement, then check compatibility

For all three below: adopt only when the section 1 workloads show the remaining bottleneck (link time, repeated dependency recompiles, test orchestration), the project accepts the tool, and required coverage is unchanged. None of them fix missing basic profile and selection hygiene.

### sccache (compiler cache)

- Configure as a compiler wrapper: `build.rustc-wrapper = "sccache"` in `.cargo/config.toml` (equivalent environment: `RUSTC_WRAPPER`, `CARGO_BUILD_RUSTC_WRAPPER`). Prebuilt binaries exist for Windows, Linux and macOS.
- Documented cache gaps: incrementally compiled crates cannot be cached (so turn incremental off), and crates that invoke the system linker cannot be cached: `bin`, `dylib`, `cdylib`, `proc-macro`. Large binary crates and proc-macro-heavy graphs see less benefit; a thin `bin` wrapper over a `lib` changes what can be cached.
- Its on-disk cache is separate from Cargo's (`SCCACHE_DIR`; `SCCACHE_CACHE_SIZE` defaults to 10G) and counts toward total footprint.
- Verify on the target platform with one clean build and one repeated build: a nonzero hit rate plus improved wall time on the workload you measured, not on a benchmark you did not run.

### nextest (test runner)

- External runner; executes each test in its own process, which adds isolation and enables retries (`--retries`, `.config/nextest.toml`) but increases process spawns - measure execution time, especially on Windows where process creation is relatively expensive.
- `cargo nextest run` accepts the usual cargo test options (package/target selection); keep the project's selectors, ignored/platform conditions and required gates equivalent.
- Doctests are not supported on stable Rust: keep `cargo test --doc` as a separate, required step in the project's gates.
- Consider it when a large suite's execution time or retry/isolation behavior dominates; it does not reduce compilation.

### Alternative linkers

- The official build-performance guide suggests LLD, mold or wild (with Linux configuration examples for mold) and notes the default linker is often fast enough that switching may not pay off.
- Platform-gate hard: Linux recipes (`-fuse-ld=mold`, clang driver shims) do not transfer to Windows. There, check what the installed toolchain actually offers (for example a shipped `rust-lld`, or an LLVM `lld-link`), that every crate and build script still links, and that the override is scoped and reversible in `.cargo/config.toml` under the correct target.
- Measure matched link-heavy rebuilds before and after; link time is part of build wall time, and `--timings` shows where units spend time.

## 7. Dependency and manifest hygiene

- `cargo tree -d` lists duplicate versions; `cargo tree -i CRATE` shows why a dependency is present; `cargo add`/`cargo remove` keep manifests tight; `cargo update --precise VER -p CRATE` makes minimal, reviewable bumps.
- Centralize versions in `[workspace.dependencies]` with `dep = { workspace = true }`, and lints in `[workspace.lints]` with `[lints] workspace = true`. This is maintenance hygiene, not a measured build-cost improvement.
- Unused dependencies and features are evidence to check, not automatic removals; confirm the consumers still build before deleting.
- Quick experiments: an `examples/` target or a temporary filtered test inside the real crate keeps the same toolchain, features and dependencies; remove or promote it afterwards. Check the toolchain channel before relying on script/runner experiments.

## 8. Portability

- Use the target project's documented commands, toolchain and limits; do not require tools specific to the environment you were invoked from, and keep raw measurements in local project storage.
- State the platform your observations came from; platform support governs optional tools (linkers, process behavior).
- Keep changes scoped and reversible for the project that adopted them; never edit global configuration or impose concurrency/cleanup policy on other checkouts.

## Sources

- Build performance guide (profile recipe, linker advice): https://doc.rust-lang.org/cargo/guide/build-performance.html
- Profiles reference (debug values, wildcard scoping, inheritance, MSRV): https://doc.rust-lang.org/cargo/reference/profiles.html
- Cargo configuration (target dir, jobs, incremental, rustc-wrapper, cache GC frequency): https://doc.rust-lang.org/cargo/reference/config.html
- Global cache GC announcement: https://blog.rust-lang.org/2025/06/26/Rust-1.88.0/
- sccache cache gaps for Rust: https://github.com/mozilla/sccache#known-caveats
- nextest running and doctest policy: https://nexte.st/docs/running/
