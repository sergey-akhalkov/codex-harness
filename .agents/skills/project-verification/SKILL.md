---
name: project-verification
description: Discover and reuse project-native checks when verification commands, build identity, or saved validation evidence need establishing for a change. Use for stale-build doubts or resuming incomplete verification; ordinary documentation edits need only their applicable checks.
---

# Project verification

Establish what the current change actually exercises, obtain a useful first signal, then complete the checks required by the agreed outcome.

## Find the verification path

Read applicable project instructions and any existing validation record. Bound discovery to the affected component's manifest, relevant CI job and documented test/build entrypoints. Use available repository navigation for code relationships and narrow native reads for manifests and documentation. Expand only to resolve a specific missing prerequisite, caller or acceptance requirement; do not inventory the whole repository to choose one check.

Reuse the project's runtime and commands, including cwd, setup, flags and supported test selectors. Inspect setup effects before execution: a CI command or saved record does not authorize live-service changes. If command sources disagree, compare their intended scopes and current definitions; label unresolved assumptions instead of guessing a passing command. Missing commands or prerequisites need a bounded next investigation or an explicit blocker.

## Establish execution identity

Identify the actual source entrypoint or resolved executable, runtime version, build configuration and relevant generated/bundled assets. Trace launch wrappers far enough to know which artifact they execute. A global binary on PATH or cached bundle may differ from edited source. Rebuild through the native preparation path when needed, then record the selected artifact and relevant inputs. Do not replace the intended binary or weaken expected behavior to obtain a pass.

Establish a coherent prepared path, including the test driver and its required support artifacts. A pin on one product artifact does not justify suppressing preparation of other required dependencies. Follow the current native dependency boundary, refresh affected outputs and reuse still-valid ones before running the consuming check; do not discover each stale prerequisite through another full run.

When recording commands or resuming saved work, read [command records](references/command-records.md). Use the consuming project's existing documentation home; create a small local validation document only if useful knowledge has no existing home. Keep detailed or sensitive logs in appropriate local evidence storage.

## Choose the feedback boundary

After a failure, establish whether preparation, the test driver, the product, or observation failed, and whether the intended product action occurred. A driver failure before that action leaves the product untested. When full runs repeatedly discover small unknowns in one layer, isolate that layer's question and meaningful check before another dependent full run, even if every error is different. One informative failure can expose enough unknowns to justify extraction.

Give the smaller task explicit inputs, necessary state and a result the parent path can consume. Preserve the original trigger, timing and integration conditions; reject a passing reduction that removed them. Correct an understood preparation mismatch through its existing refresh path; use `reproduce-regression` when a current intended artifact exhibits a CLI/MCP/process regression within that skill's scope. For unfamiliar interactions, verify the needed transitions and return paths in an owned prepared environment, then exercise the reusable mechanism through the actual parent entry point. An exploratory recipe is not product acceptance. Keep the required full check when no smaller scenario can answer the question; a cheap understood correction needs no new helper or worker.

## Execute and finish

Run the smallest native check that can distinguish the promised behavior from the defect, through the actual entrypoint. For a regression, exercise the original failure on an available baseline and the intended behavior on the candidate; state missing baseline evidence. Use representative inputs and meaningful failure paths selected by the specification and concrete risk.

Then run all applicable required integration/acceptance checks. A focused pass is progress while any required check fails, is blocked or remains unexecuted. After correction, rerun affected checks; repeat or broaden further only for new changes, failures or unresolved risks. Documentation-only work uses its link, syntax or factual checks; this skill does not itself require application suites, delegation or model-backed evaluation.

Report behavior and scope, exact commands/cwd, tested revision and execution identity, outcomes, evidence paths and unfinished checks. Separate command knowledge (`confirmed`, `docs-only`, `unknown`, `blocked`) from each run's result. Historical success is never current-change verification; lint or an isolated fixture proves only its exercised scope. Preserve failed, interrupted and partial attempts through retries and handoff.
