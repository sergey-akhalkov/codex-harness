# Native launcher acceptance

2026-09-08, Windows x64, Rust/Cargo 1.97.1. The native ordinary entry point is
[codex.exe](../../crates/codex-harness/src/bin/codex.rs), backed by
[native_launcher.rs](../../crates/harness-core/src/native_launcher.rs) and the
existing native argument policy. The global installation still resolves the
transitional script launcher; native global registration belongs to the remaining
installation/cutover tasks.

## Runtime contract

An owned consumer home supplies `harness/native-launch.json`: schema 1, absolute
native build state, and an explicitly selected upstream executable with SHA-256.
An optional adopted package record includes its absolute root, manifest hash and
package manager. These are installation outputs, not runtime discovery guesses.
The launcher resolves the active immutable build and refuses an unfinished
selection, stale source, altered artifacts, mismatched launcher or changed
upstream. It does not compile, acquire dependencies, run hooks or rewrite state.

Arguments remain separate values, including Unicode, empty strings, quotes and
literal shell punctuation. The native profile/task-effort policy preserves
explicit settings. Cwd, ordinary stdin/EOF, stdout/stderr and upstream exit codes
are inherited. Package launches match the inspected installed upstream npm
entry point's `CODEX_MANAGED_PACKAGE_ROOT` and mutually exclusive manager hint.
The caller's process environment is not modified.

The wrapper and upstream share the console. The wrapper waits through Ctrl+C
and returns the child's actual termination code without rebroadcasting the
event. Ordinary upstream sessions own their persistent background processes;
the launcher imposes no kill-on-wrapper-close Job. Bounded helpers retain their
separate native Job policy. Tests establish both behaviors independently.

## Checks

```text
cargo test -p harness-core --lib -p codex-harness --test native_build --test native_launcher --test launcher --offline --locked --jobs 1 -- --test-threads=1
```

Result: 6 build, 5 native launcher, 9 core and 3 argument-policy tests passed.
The launcher tests exercise its actual executable in owned homes, streams and
ConPTY. Negative cases preserve installation state for source/binary/registration
damage, interrupted selection, changed upstream and direct self-recursion.
A compiled upstream fixture proves its bounded background worker can complete
after the wrapper exits. Scoped Clippy on the affected production and integration
targets passed with `-D warnings`.

## Actual installed CLI through a release candidate

Evidence is under
`%LOCALAPPDATA%/codex-harness-evidence/native-launch-1d8cb9f0d69a4578861e18a3a9d80855/`.
The parent froze 30 native inputs into `source/`, excluding the concurrent
regression helper's three partial source files. `snapshot.json` records that
boundary; it is not a complete migrated installation. A fresh release compile
under the manager's 2 GiB Job and one compiler succeeded with source identity
`2777da6748db823ce1df79586548dc6402a6595b308070c8de52387324ee3177`.
Compiler dep-info accepted that prototype's inputs and all staged binaries,
including `codex.exe` and `harness-inspect.exe`. That snapshot predates the
[subsequent schema correction](rust-structured.md): source-owned skill data now
remains a direct live input instead of being compiled into the helper.

The native manager selected that immutable candidate. An owned home registered
the exact existing official CLI binary and linked the current harness profile.
From the separate `consumer/` directory, its Rust launcher returned
`codex-cli 0.153.4`; `--harness-effort routine debug prompt-input` produced valid
native prompt JSON containing the linked Astra/Grok instructions, with empty
stderr. Native Check returned healthy with runtime and management allowed.
These probes made no model calls and did not restart the existing proxy.

The first shell invocation used a mixed-separator Windows extended path from the
receipt and failed before execution. Converting that local shell path to its
ordinary absolute form fixed invocation; the native receipt keeps its canonical
path. The global installation receipt and command registration were preserved.

This completes the ordinary-launch portion of foundation task 2.2. Full native
installation/upgrade, global command registration, broader legacy acceptance
migration and matched timing/resource comparisons remain open. In particular,
this candidate does not close migration task 4.4 or global cutover.
