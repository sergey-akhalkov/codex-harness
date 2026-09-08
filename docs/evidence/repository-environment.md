# Repository working environment

Verified 2026-09-08 in `codex-harness`, on the current dirty source based on
`dfc2d6af49f2da949b2b1b0ed549a33c62d1eb4b`. This records preparation for the
unfinished OpenSpec work, not completion of the Rust migration.

## Instructions

The user's preparation-before-implementation decision is implemented in
[portable principles](../../global/principles-of-work.md#working-environment-before-implementation),
[repository guidance](../../AGENTS.md) and the
[main requirement](../../openspec/specs/global-working-principles/spec.md).
The installed `%USERPROFILE%/.codex/AGENTS.md` resolves to the canonical source;
the contents and SHA-256 matched after the edit, and the new section was read
through that global path. Strict validation of `global-working-principles` and
the scoped `git diff --check` passed. Existing sessions retain their initial
instructions until their next load; the current task follows the user's request.

## Serena Rust and source coverage

`.serena/project.yml` selects Python and Rust, with the root workspace and no
additional ignored paths. Rust was absent from the managed language catalogue
after the earlier subscription-efficiency selection. Adding the project language
alone initially failed the runtime guard with `unmapped`; that was a real
dependency-selection gap, not a missing executable.

Rust was restored to the explicit catalogue and reconciled with
`./install.ps1 -Mode Install -CodeToolsOnly` (exit 0). This adopted the existing
rustup `stable-x86_64-pc-windows-msvc` rust-analyzer 1.97.1 and preserved the
empty standalone harness-LSP registry. No dependency download, toolchain change
or subscription service stop was needed. Language selection is enabled for this
project; other projects' Serena configuration was not edited.

The old session MCP client then correctly reported changed source/runtime and
requested a restart. A fresh proxy process using the installed Serena 1.7.0
Python and global dependency registry passed actual calls for:

- Rust symbol overview, including `filter_for`, in `tools/rtk-adapter/src/main.rs`.
- Rust `source_identity` definition discovery in `crates/harness-core/src/build_identity.rs`.
- Python symbol overview, including `command_for`, in `tools/code-tools/cbm_proxy.py`.

Native MCP receipt: `%TEMP%/harness-rust-serena-zxg3tmci/report.json`.
Configuration follows [Serena's project/global layers](https://oraios.github.io/serena/02-usage/050_configuration.html);
the adopted component follows [rust-analyzer installation](https://rust-analyzer.github.io/book/installation.html).
These calls verify navigation, not comprehensive Rust diagnostic or edit coverage.

Codebase Memory was explicitly refreshed successfully. The summarized
`excluded.dirs` contained `tools`, but actual graph queries returned both files
above, including with `file_pattern: "tools/*"`. The initial interpretation that
the entire directory was excluded was incorrect. No whole-directory ignore rule
was found or removed. The index reports 18 partially parsed PowerShell files;
those ranges still require direct source checks. Latest preparation coverage log:
`%USERPROFILE%/.cache/codebase-memory-mcp/logs/D-home-sergey-akhalkov-codex-harness-1788880971.log`.

The updated `tests/subscription-efficiency.py` passed with the adopted Serena
Python: `%TEMP%/harness-subscription-efficiency-4o5knl61/report.json`.
Ordinary diagnostic/Stop hooks remain off; the accepted RTK exception is retained.

## Native migration handoff

New Cargo workspace members: `crates/harness-core`, `crates/codex-harness`, and
the existing `tools/rtk-adapter`. Root lockfile generated with Cargo 1.97.1;
Rust 1.97.1 and the native Windows linker compiled the initial workspace.
The core now computes native source identities and checks binary hashes,
source freshness and whether management/runtime are allowed. The initial CLI
exposes only `check --build DIRECTORY [--source CHECKOUT]`; it is not a delivered
replacement for global installation or launch. No global native cutover occurred.

From the repository root, `cargo check --workspace --locked` passed after fixing
an initial `Result` comparison compile error. `cargo test -p harness-core --locked`
passed two tests: changed/added/removed/locked source versus documentation, and
stale-source management versus altered-binary rejection. Full format/lint,
explicit build/bootstrap, installer/launcher and all migration/global acceptance
remain unfinished. The root workspace means the legacy RTK build identity must
be reconciled with root Cargo inputs before migration acceptance.

All nine changes observed at the start remain in the accepted overall scope.
No new change was declared complete or archived by this parent turn, and nothing
was committed or pushed. Continue from current files and OpenSpec tasks after
the requested session restart; inspect any worker artifacts and use fresh Grok
children rather than resuming saved encrypted child state.

The stopped archive sidecar made no spec/archive changes. Its read-only comparison
found `agent-delegation`, `subscription-model-routing` and
`subscription-runtime-recovery` mains already present; the routing main carries
the newer Grok-middle wording and must not be overwritten by an older delta.
`grok-delegation-reliability` and `bounded-tool-resources` mains still need sync.
Historical receipts referenced in their owning evidence documents remain on disk;
the autorestart routing-module hash differs and needs impact assessment before
historical checks are reused. No final implementation-acceptance audit passed.
The process-ownership sidecar produced only an investigation handoff and no code;
task 2.4 remains open. Both children stopped without live process handles and were
closed for the requested restart; their three temporary audit scripts were removed.
