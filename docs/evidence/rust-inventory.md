# Native core data inventory

2026-09-08. [inventory.rs](../../crates/harness-core/src/inventory.rs) reads the
live declarative [native manifest](../../global/kit.json) through
`codex-harness inventory --source CHECKOUT --codex-home DIRECTORY --user-home DIRECTORY`.
This is the first part of migration task 4.1. It does not install, repair,
disconnect, change PATH, inspect dependency compatibility or replace the current
PowerShell lifecycle. `global/kit.psd1` remains that transitional lifecycle's
inventory until its consumers migrate.

The native report resolves direct instructions/profile/skill/agent data sources,
checks portable unique names, lists build-owned executables, and distinguishes
missing, correctly linked and conflicting targets. Skill names come from bounded
frontmatter, excluding nested metadata and body text. Source traversal and
reparse parents are rejected; existing foreign files and dangling links are
reported as conflicts. Diagnostics do not echo descriptor bodies. Fresh target
roots remain absent after inspection. Parser/input errors retain the manager's
existing exit-2 contract.

```text
cargo test -p codex-harness --test inventory --offline --locked --jobs 1 -- --test-threads=1
cargo clippy -p harness-core -p codex-harness --bin codex-harness --test inventory --offline --locked --jobs 1 -- -D warnings
```

The initial five integration tests passed through the manager executable; Clippy passed.
Coverage includes live descriptor updates without compilation, missing targets,
foreign files/dangling links, duplicate or malformed names, bounded private
errors, traversal, unsupported schemas and source/destination reparse parents.
Initial negative tests expected exit 1; inspection confirmed the unchanged
manager reports input errors with exit 2, and the tests were corrected to that
existing contract while retaining all preservation and error-output assertions.

The actual connected user/home inventory returned 15 matching source links and
three agents. Evidence:
`%LOCALAPPDATA%/codex-harness-evidence/native-inventory-029efc7bef694b6082f2617665d71241/`.
This read-only observation excludes current script launcher/hook registrations
and does not certify complete native global activation. Native primitives now
have separate [registration](rust-registration.md), [lock](rust-installation-lock.md)
and [legacy metadata](rust-installation-state.md) evidence. Their full installer
integration remains open.

## Global name preflight, 2026-09-09

The current manager checks foreign global skill/agent names before reporting
inventory. Managed destinations retain their reuse/conflict result. Read-only
foreign discovery follows normal symbolic-link roots and descriptor files;
source traversal and managed destination-parent checks remain strict.
There is a 4096-entry operation budget, eight directory levels and a 256 KiB
descriptor bound. Revisited resolved names, cycles, dangling/unreadable links and
unsupported reparse objects refuse a clean report. These are bounded discovery
checks, not object-identity ownership evidence or an atomic filesystem snapshot.

The first global attempt exposed rejection of the normal `agents/codex-harness`
link. Moving the managed-destination check before foreign traversal corrected
that case. A second actual run found the separate linked subscription group,
which also must participate in name discovery. The completed traversal scans it
and correctly distinguishes its `middle` from core `middle_backup`, `senior` and
`principal`; there is no hardcoded subscription exclusion.

Agent descriptors now use the [native-compatible TOML parser](rust-agent-config.md).
This supports quoted keys, escapes, comments and multiline values while rejecting
malformed TOML privately. Skill frontmatter retains its bounded portable-name
contract. The main entry point also checks base `config.toml` role collisions.

All 16 inventory CLI tests passed, including linked external unique/colliding
names, linked descriptor files, managed wrong-target conflicts, cyclic/dangling
inputs and complete agent TOML parsing. The final outside-checkout command again
returned 15 linked data entries and three core agents. The worker's scoped
before/after home snapshot was unchanged; later parser verification is a separate
read-only observation. Full native global activation is still unfinished.

Evidence root:
`%LOCALAPPDATA%/codex-harness-evidence/inventory-preflight-46a794334d114947a639e428551c08c5/`.
`actual-before.*` and `actual-after.*` retain both failures;
`linked-foreign-*` records the traversal correction; `toml-complete.*` records
the final 16-test run and `toml-global.*` the current global read.
