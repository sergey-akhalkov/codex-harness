# Installed worktree workflow acceptance

2026-09-08. The installed `isolated-worktree` skill and its Windows CLI reference
were applied to an independent owned Rust/Git fixture outside the harness.
This covers adoption tasks 3.2 and 3.3; it does not claim a speed comparison or
global Rust installer delivery.

Evidence root:
`%LOCALAPPDATA%/codex-harness-evidence/worktree-native-46f41708ca03490493da640af5795ab3/`.
The fixture's explicit base is `c851d7b5522cc56482f12798c5cbd99c1734be50`.
The owned branch is `task/render-checksum`, checkout `task-checkout/`; its Git
metadata is a `.git` file. Neither the harness's dirty inputs nor shared service
state was transferred or changed.

## Inputs, tools and integration

The parent had a tracked seed change from 0 to 17 and an untracked input line
`AB`. The scoped Git patch and the separately copied input had matching hashes
in the new tree (`input-receipt.json`). An unrelated uncommitted note stayed in
the parent with SHA-256
`79067B6007ED03405EF239FA4A975D753FFEB5ED59480314A1B0B0DC30822FB3`.
The worktree received its own Cargo target directory; offline, locked tests with
one compiler passed before the feature edit.

Codebase Memory indexed this exact checkout as `worktree-checksum-46f41708`:
17 nodes/20 edges initially, 21/28 after the primary edit, no reported skipped
or partial source. It returned the real `render_checksum` definition. Inert
fixture text and build output were excluded; no absence claim relies on them.
Serena activated the checkout with Rust, reported its LSP ready, and returned the
actual function body. `mcp-context.json` retains these tool results. The later
negative-case edits make that fixture graph stale until refreshed. Serena was
restored to the actual harness root after the experiment.

The independent oracle expected `sum=0094` for the transferred input/seed and
`sum=0011` for empty input. It first failed with observed `148`, then both tests
passed after the rendering change. A temporary Git index represented the exact
transferred baseline; `candidate.patch` contains only the task delta. After
inspection, that delta and the explicitly checked new test file were integrated.
Both combined-tree tests passed. Required dirty input and the unrelated note
remained uncommitted and preserved in the parent.

## Failure and cleanup cases

Branch collision returned 255 without creating another checkout. An occupied
path returned 128 and preserved its sentinel. Removing the dirty task worktree
without force returned 128 and preserved its source hash and untracked inputs.
These outcomes are in `collision-receipt.json` with separate stderr files.

A separate controlled experiment changed width to eight hex digits in one
version and the prefix to `checksum=` in the other. Each version passed its own
tests. Integration rejected the overlapping patch and retained both versions
and their hashes. The combined implementation initially failed a stale test;
it was not accepted at that point. Reconciling the independent requirements gave
`checksum=00000094` and `checksum=00000011`, and the combined tests passed.
Original, conflicting, intermediate and accepted sources/check output remain in
the evidence root. This was an owned fault-injection scenario, not a change to
the user's harness requirements.

An additional clean worktree was checked for dirty/untracked/ignored files,
unmerged commits and its absolute path inside the owned evidence root. Normal
`git worktree remove` and `git branch -d` removed only that proven-safe target.
The dirty task checkout remains for reproduction. After saving the negative
experiment, only its deliberately changed parent files were restored to the
accepted primary result; the two primary tests passed again and the unrelated
note's hash remained unchanged (`final-receipt.json`). No force removal, reset,
stash, real project cleanup or shared-service restart was used.
