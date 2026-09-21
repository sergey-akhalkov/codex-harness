# Token-audit notes

## Acceptance evidence (task 6.1)

Run on commit `ee92f29` in a clean detached worktree (the shared checkout held
unrelated parallel work), Windows x64, Rust stable:

```powershell
cargo test --locked -p token-audit --jobs 1 -- --test-threads=1
# 21 passed, 0 failed across report (9), findings (3), baseline (2), cli (7)

cargo test --locked -p codex-harness --test delegation_usage --jobs 1 -- --test-threads=1
# 38 passed, 0 failed - expectations unchanged by the reader extraction

cargo test --locked -p harness-core --lib rollout_reader -- --test-threads=1
# 8 passed, 0 failed - both usage formats, nested base instructions, tool bytes

cargo clippy --locked -p harness-core -p token-audit --all-targets -- -D warnings
# clean

cargo fmt --all -- --check
# clean

codex-harness ownership-check --source .
# 321 scanned Rust files, 0 findings

openspec validate token-audit --strict
# Change 'token-audit' is valid
```

Reader calibration from the local corpus (recorded in the slice-2 board
comment): 238 sessions / 988 MB streamed in 18 s (debug build), no failures;
`session_meta.base_instructions` arrives as a nested object, fixed and pinned
by reader fixtures. Tool output bytes are matched through call identity;
unmatched outputs land under `unmatched_call`.

## Deferred

Task 5.2 (delivery through the kit installation lifecycle against an installed
consumer outside this checkout) waits for the shared checkout's parallel work
to settle: the installed manager requires an explicit deploy after native
source changes, and deploying builds the working tree.
