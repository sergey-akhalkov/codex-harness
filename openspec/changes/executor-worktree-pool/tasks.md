## 1. Pool core in harness-core

- [x] 1.1 Implement deterministic slot resolution (sibling path, `<repo-name>-wt1..N` from `max_concurrent_executors`, collision refusal for existing non-pool paths) in `task_worktree.rs` and verify with unit tests covering naming, cap size and foreign-path refusal
- [x] 1.2 Implement the slot state machine (free, synchronizing, occupied, awaiting-review, released) with kit-local task-state recording and compare-and-set slot claims, and verify with unit tests that a claimed slot is not double-claimed and that liveness reconciliation frees only session-less slots
- [x] 1.3 Implement fail-closed upstream synchronization (fetch configured remote, resolve default base or explicit override, `reset --hard`, `clean -fd` keeping ignored caches, verify clean HEAD) and verify with unit tests using a local `file://` remote, including fetch-failure abort and base-override behavior
- [x] 1.4 Preserve unreviewed work: verify with unit tests that a dirty slot without a live session is reported as awaiting-review and never selected or reset, and that `reset_for_reuse` still refuses unresettable state
- [x] 1.5 Rework `audit` to classify pool slots versus foreign/legacy trees and enforce the pool invariant (no harness allocation beyond the pool) and verify with unit tests that legacy task-named trees are reported, not adopted or deleted

## 2. Executor CLI integration

- [x] 2.1 Change `executor spawn` to derive and synchronize a pool slot from `--source` before launch, make `--workspace` optional and restricted to the source checkout or a pool slot, and stop passing native `--worktree` isolation flags on the pooled path; verify with CLI tests using the existing mock-launcher fixtures
- [x] 2.2 Record the slot mapping (path, slot index, owner session identity, base revision) at dispatch and expose it through existing task-state load paths; verify with CLI tests that interruption preserves the mapping and reuses the same slot
- [x] 2.3 Add the explicit slot release path (record merged/discarded disposition with reason, then reset or preserve per `reset_for_reuse`); verify with CLI tests that released slots re-enter the pool and preserved slots report their limitation
- [x] 2.4 Replace the `worktree_limit` warning path with the pool-invariant refusal and keep the config field accepted for compatibility; verify with CLI tests that exhausted pools abort with a concrete cause and no new tree

## 3. Skill and documentation

- [x] 3.1 Update the `team-lead` skill text from task-named lanes to fixed pool slots, remove the executor-side synchronization obligation, and document the explicit release path; verify by re-reading the skill against the updated `lead-agent-orchestration` scenarios
- [x] 3.2 Update `docs/agent-delegation.md` (executor worktrees section: pool slots, sync, fail-closed refusal, superseded `worktree_limit`, legacy-tree migration) and `global/orchestration.toml` comments; verify local links and consistency with the installed CLI help

## 4. Native checks and hygiene

- [ ] 4.1 Run the full affected native suites (`cargo test -p harness-core -p codex-harness`) and the launcher/installation checks from `docs/rust-native.md`; record any unrelated pre-existing failures separately from this change
- [ ] 4.2 Run `codex-harness ownership-check` and the documentation source-hygiene checks for edited public files; fix only issues introduced by this change

## 5. Real-entry verification and migration

- [ ] 5.1 Rebuild and update the installed launcher through the installation lifecycle; verify `codex-harness executor --help` reflects the pooled spawn surface and the version identity matches this source
- [ ] 5.2 On a real consuming repository, review and retire legacy task-named worktrees through authorized lead merge/removal plus `git worktree prune`, then run repeated real dispatches through the installed entry point; verify privately that the same `<repo-name>-wtN` paths are reused, `git worktree list` shows the source plus exactly the pool slots, and private evidence stays outside the repository
