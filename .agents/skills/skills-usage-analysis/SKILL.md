---
name: skills-usage-analysis
description: Inspect skill-library growth, unused skills, last invocation, and disable or retirement candidates. Use only when the user asks about skill usage, unused skills, library growth, or what to disable. Skip ordinary implementation, debugging, OpenSpec, and documentation work.
---

# Skills usage analysis

Run only when the user asks about skill usage, growth, unused skills, last invocation, or what to disable or delete. Do not run at session start or after an ordinary coding task.

## Report

1. Run `codex-harness skills usage` from the current project directory.
2. Show the command output as-is: project-then-global table, then candidates, then the statement that no library changes were applied.
3. Do not apply disable, retire, delete or library edits unless the user confirms a specific owned non-protected skill.

Unknown last invocation is not a deletion reason. Do not propose system, third-party, foreign, explicitly disabled, OpenSpec-external, or required protected skills (`openspec-*`, `skill-evolution`, `skills-usage-analysis`, `project-verification`, `project-memory`).

If no owned skill qualifies, say so. Confirmed disable is reversible catalogue removal; physical deletion is a separate ownership check.
