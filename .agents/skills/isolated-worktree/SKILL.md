---
name: isolated-worktree
description: Run independent edits or experiments in a Git worktree when isolation justifies preparation and integration cost. Carry explicit base and dirty inputs, verify tool checkout identity, then integrate and check the combined result.
---

# Isolated worktree

Use an ordinary Git worktree for independent writes, risky experiments or isolated review. Keep small coupled edits in the current checkout when setup and integration cost outweigh the benefit. A conversation fork does not isolate files; a worktree does not isolate global configuration, credentials, services, ports or databases.

Before creating one, establish the repository root, explicit base revision, unique owned branch/path, ownership, invariants and acceptance checks. Inspect tracked and untracked dirty inputs. A new worktree contains the committed base only: supply a scoped reproducible snapshot of necessary tracked and untracked inputs and verify it, or keep coupled work in the parent. Never silently omit required inputs or stash/reset/commit unrelated work.

Use [the Windows CLI example](references/windows.md) for native Git creation, dirty-input transfer, integration and cleanup. Check both branch and path collisions; Git metadata may be a `.git` file. On failure retain the owned path and evidence and inspect the exact cause before retrying.

In the new checkout, read its instructions and relevant memory. Use `project-verification` when available to establish project-native preparation, runtime/build identity and checks; otherwise derive them from current manifests/CI and record their scope. Do not copy a parent's environment blindly. Allocate owned mutable test resources or serialize their use; never restart shared services merely because the test runs in a worktree.

Give an isolated worker its actual root, base, transferred inputs, file ownership, required skills, invariants and acceptance commands. Before using MCP navigation, verify its active checkout: select/index the intended root in Codebase Memory, activate it in Serena, and verify LSP or saved-graph identity. Refresh unknown or changed indexes before queries. If coverage or selection cannot be established, state the limitation and use fresh scoped source; a parent graph is not evidence about the worktree.

Inspect the candidate diff before integration. Transfer only owned changes, reconcile conflicts using both versions and their evidence, and run applicable checks on the combined tree. Passing candidate checks does not accept a failing merge. Preserve unfinished work and evidence until the integrated outcome passes.

Cleanup requires verified preservation of all work and authorization within the task. Check dirty/untracked/ignored files, unmerged commits and branch consumers before removal. Use `git worktree remove` without force; never delete an unsaved tree or use `git branch -D`. Do not clean up another task's resources. Keep the tree when preservation or integration is uncertain.
