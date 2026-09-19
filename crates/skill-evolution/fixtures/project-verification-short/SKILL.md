---
name: project-verification
description: Establish native checks and execution identity for behavior, build, test, CLI or process changes, or diagnose slow inconclusive verification of those changes. Skip documentation-only edits, link/syntax/factual checks, and unrelated tasks.
---

# Project verification

Skip this skill for documentation-only work. A spelling, link or factual edit uses the project's ordinary documentation checks.

Reuse the project's native commands, including cwd, setup, flags and supported test selectors. Identify the actual source entrypoint or resolved executable, runtime version and relevant generated assets before running a check. Do not replace the intended binary or weaken expected behavior to obtain a pass.

Run the smallest native check that can distinguish the promised behavior from the defect, through the actual entrypoint. Then run all applicable required acceptance checks. Report commands, cwd, tested revision, execution identity, outcomes and unfinished checks. Historical success is never current-change verification.
