# Design

## Context

See [proposal.md](proposal.md) for the problem and scope. The existing [isolation specification](specs/isolated-worktree-workflow/spec.md) becomes the common contract; the two delegation deltas adapt its consumers.

The closest implementation is `crates/harness-core/src/task_worktree.rs`: it already owns stable slot paths, claims, startup reservations, records, synchronization, release and ignored-cache preservation. `create_slot` is a small wrapper around Git; the substantial behavior is the ownership and recovery surrounding it. `executor_cli.rs` currently combines that lifecycle with launching a visible executor. `resolve_source` canonicalizes a supplied path rather than identifying the shared Git repository, and `orchestration_config::load` reads configuration below its source root. Configuration origin and target repository therefore need explicit separation for use outside the kit checkout.

The standalone skill describes disposable task directories while executor guidance forbids that route. Existing `agent-delegation` wording also calls recorded executor trees Codex-managed. This change reconciles those contracts; it does not treat those competing descriptions as three supported allocators.

The inspected official `codex_worktrees` contract exposes creation, task-scoped listing and creation status. It does not establish caller-selected stable paths, attaching arbitrary existing pool slots, permanent retention, or a release/reset protocol. The [official worktree documentation](https://learn.chatgpt.com/docs/environments/git-worktrees) describes permanent worktrees and managed cleanup in the application, but those UI features are not proof of equivalent MCP operations. No native worktree creation or reset was exercised during planning.

## Goals / Non-Goals

**Goals:** use one resource owner and its existing state for every managed isolated task; make the hard capacity observable through the real installed commands; retain exact bases, ownership, work and useful caches through interruption and reuse.

**Non-Goals:** replace Git, build a general workspace-provider framework, introduce a daemon or queue service, redesign executor messaging or model routing, adopt foreign worktrees automatically, or treat file isolation as isolation of shared services and accounts. Small coupled edits can still use their current checkout.

## Decisions

### 1. Extend the existing pool owner and expose it independently

Refactor the common lifecycle within `task_worktree.rs` and its current record owner. Ordinary tasks and executor adapters call the same acquisition, continuation and release operations. Keep Git as the physical worktree mechanism. Do not create a second pool database, parallel ownership registry or provider-selection configuration.

The user-facing command family is `codex-harness worktree`, with `acquire`, `continue`, `list` and `release` operations. Acquisition takes an explicit target repository, task identity and base revision and returns the current slot receipt, including its path, exact base and ownership handle. It starts no conversation. Continuation returns the existing held binding without synchronization. The main session then uses that checkout for file operations, navigation and checks; acquiring a path does not silently change its original process cwd.

Existing `executor spawn`, `resume`, `restart`, `pool` and `release` remain supported adapters. Keep their profile, visibility and observation behavior. Add an optional `--repository` target selector while retaining `--source` as the kit/configuration source and its existing default target when the new selector is omitted. Generic worktree operations use the installed configuration source by default and take the target independently. A consuming project does not need a copied `global/orchestration.toml` or a kit-shaped directory layout.

A role handoff from an ordinary held task to an executor validates and transfers the current binding through the common owner before launch. It does not release the checkout into the free pool or allocate a second tree. An executor still receives and verifies the committed input base required by its assignment; role handoff does not bypass the prohibition on mutable input copying.

### 2. Identify the repository once and retain stable physical slots

Resolve the canonical Git common directory and registered worktrees through Git, using that shared identity for pool lookup regardless of the caller's checkout. Keep configuration-source identity separate. Record one stable slot root when the pool is first established; for a normal checkout use the existing sibling `<repository-name>-wt1..wtN` layout. Never derive a new slot root from a caller entering through an already linked worktree.

Preserve existing paths and import compatible existing slot records into this identity without moving files or resetting trees. Multiple legacy mappings or ambiguous ownership produce a visible reconciliation failure and stop new acquisitions. Foreign Git and Codex-managed trees appear separately in inventory and do not become managed merely because a path exists.

This extends the current path and inventory foundation. A separate pool for ordinary tasks would multiply capacity and duplicate the same preservation logic, so it is rejected.

### 3. Separate directory capacity from executor admission

Add `worktree_pool_size` to the existing orchestration configuration, shipped as four beside `max_concurrent_executors = 4`. Explicit settings are independent. Each held task consumes a pool position regardless of whether it has a live process. Active executor admission also observes the existing concurrency scope and limit; multiple callers or leadership changes cannot create extra allowance.

Allocation is lazy. A suitable free existing slot is preferred; an absent position is created only within the configured range. Capacity exhaustion returns a bounded inventory and cause. There is no automatic expansion, oldest-task eviction or new scheduler. Callers can wait through their existing orchestration path or finish and release held work.

Legacy configuration without the new field retains its previous executor-derived capacity. The supported installation/update path materializes that value as the explicit pool setting, preserving user choices and leaving `worktree_limit` compatibility-only. After migration, changing executor concurrency cannot resize the pool.

Reducing capacity is an explicit lifecycle change: existing slots above the proposed range must first be safely retired. A premature lower setting is rejected; it must not strand status or recovery commands. Manual creation of foreign worktrees is outside the harness capacity guarantee and is reported, never used as overflow.

### 4. Bind slots to tasks across conversation and process lifetimes

Use the existing task/owner identity as the durable owner, with a binding generation or equivalent non-reusable ownership handle in the same slot record. Store native conversation identity separately from that task binding. Starting a replacement conversation updates the session binding explicitly without freeing the task's checkout.

Hold exclusion during each short ownership transition, including the existing startup reservation through host handoff. Repeated acquisition by the same task returns its binding or the recoverable in-progress state. A different requested base for a held task is a conflict, not permission to reset. A competing task cannot replace a pending receipt, claim or assignment. Do not hold a filesystem lock for an entire model conversation.

Keep the existing states where their meaning suffices. The observable lifecycle is:

```text
free --> reserved --> held --> awaiting review --> released/free
                       |
                       +--> suspended --> held
```

Suspension and review retain ownership. A confirmed abandoned reservation that never admitted task work can be recovered using the existing startup evidence. Process disappearance, a clean working tree or elapsed time cannot establish that an admitted task has been accepted. Harness continuation rejects an old task or session binding after reassignment. This is a guarantee of the managed entry points, not a filesystem sandbox against arbitrary external Git or editor writes.

Coordinate with `serialize-executor-dispatch-claims` rather than implement a competing reservation fix. Reuse its accepted work and checks where present; code symbols alone do not establish that its pending acceptance is complete.

### 5. Prepare exact inputs only when acquiring a free slot

The common owner receives a selected base and resolves an exact commit before destructive preparation. A local explicit commit needs no fetch. The executor adapter retains its current upstream-default selection when no explicit base is supplied; that path still resolves and refreshes the requested upstream before preparation and fails if it cannot do so. A remote failure never authorizes a stale substitute.

Preparation verifies the resulting HEAD and clean tracked/untracked task state before returning acquisition or permitting the first executor model request. Continuation never invokes this preparation on a held slot. Required dirty parent inputs must be incorporated in an authorized committed snapshot, or their coupled work stays in the parent. No implicit commit, mutable file copy or unrelated reset is introduced.

This removes the current unconditional fetch for an explicit local base without weakening the existing upstream-default contract. It also removes the unsafe ambiguity between reacquiring the same task and resetting a free slot for a new task.

### 6. Release after preservation, retaining the directory

Release validates the current ownership handle and absence of active owned work, records the accepted-and-preserved or explicitly authorized discard decision, and prepares the selected committed base. The slot becomes free only after verification succeeds. An interrupted or failed release retains a recoverable disposition and the original stage error; it does not advertise success or allocate a replacement directory.

Reuse the existing `reset_for_reuse` behavior behind those ownership and acceptance checks. Preserve ignored build caches and keep task result commits or other accepted recovery evidence through their existing owners. Retained ignored files are not proof of a fresh runtime or passing verification; applicable project checks and shared-resource ownership remain required for the next task.

Routine task completion does not delete a directory. Capacity reduction and retirement use the established authorized Git removal path after preserving work and reconciling the inventory. Do not add periodic cleanup or a forced-delete fallback.

### 7. Retire the duplicate instructional route after delivery

Keep common isolation requirements in `isolated-worktree-workflow`, pool operation in the existing guide, and executor-specific use in `team-lead`. Update conflicting specification and guide terminology together. During implementation, update the main isolation Purpose to describe the common pool when the delta is synchronized; an ignored delta Purpose would not perform that change.

Remove `.agents/skills/isolated-worktree/` through the source kit and its installation lifecycle after the generic command is usable. Installation must remove only owned links/package entries and retain unrelated skills and configuration. Update existing references and ownership accounting where they actually point to the removed recipe. Do not author a new global skill or duplicate manual.

The official MCP remains a potential future physical backend only if an independently callable contract supports stable reuse, durable recovery and retention compatible with harness ownership, and an exercised integration reduces total maintenance. Do not add an unused abstraction or depend on a conversation-scoped tool merely to replace the small current Git creation wrapper.

## Risks / Trade-offs

- A suspended experiment occupies capacity and can prevent an executor from starting -> show its task and preservation reason; release requires an explicit outcome, not automatic eviction.
- Old conversations can refer to a reused directory -> validate the current task handle and session binding at managed continuation and reject stale requests before launch.
- Legacy path-keyed state can describe more than one pool for a repository -> preserve all records and files, refuse new allocation and reconcile under one identity without guessing ownership.
- A lower configured capacity cannot safely erase existing worktrees -> reject the decrease until authorized retirement; keep inventory and recovery usable throughout.
- Reused ignored files can influence later checks -> retain the existing cache benefit while requiring actual checkout identity and applicable project verification; do not inherit prior test success.
- An older executable may not understand task-owned slots -> use explicit record compatibility checks and fail closed on unsupported ownership; do not downgrade in-flight state into apparently free executor slots.

## Migration Plan

1. Extend the existing pool and configuration owners, then expose the generic installed CLI. Prove a small ordinary-task acquisition, result preservation, release and reuse path through the actual executable first.
2. Move executor allocation and continuation onto the common lifecycle; preserve existing command behavior and add the explicit consuming-repository selector. Reuse accepted claim-handoff work and test both allocation and executor admission boundaries.
3. Migrate configuration and compatible existing records without moving directories. Exercise held, interrupted, conflicting and invalid-capacity cases before enabling new acquisitions.
4. Reconcile specifications and current guides, then retire the owned skill through installation/update. Do not remove the old instructional entry before its replacement works outside this checkout.
5. Complete native model-free lifecycle checks and the existing real-consumer installed acceptance: actual authorized executor tool work in a locally supplied independent project, with the result consumed and applicable checks passing. Synthetic repositories alone prove mechanics, not global workflow delivery; public artifacts contain no private consumer identity or inputs.

Rollback uses the existing installation recovery path while preserving pool files and records. Re-enable an older executable only after all bindings it cannot safely understand have been explicitly resolved or admission has been disabled for that repository. Restoring binaries or a skill link does not authorize reset, eviction or loss of partial work.

The slot record discriminator is `ownershipSchema: 2`. Historical readers require a numeric `schema` field but ignore its value and unknown task fields; merely increasing that number would leave durable work vulnerable to their expired-session cleanup. The new discriminator makes those readers refuse the record before applying liveness rules. Current readers accept legacy `schema: 1` records without rewriting them during inspection and publish the new discriminator on the next authorized transition. Recovery retains these records, including released slots: an older host remains unable to admit work in an upgraded pool until a compatible harness is restored. Deleting ownership records is not a rollback procedure. Native old-host refusal and preservation checks remain required before accepting this migration.
