# Tool-workflow planning reconciliation

2026-09-09. OpenSpec task `improve-installed-tool-workflows` 5.1. Planning artifacts only; no runtime, skill, global-config, archive, commit or push changes.

Confirmed constraints reused from [project decisions](../project-decisions.md): ordinary diagnostic/context/Stop hooks remain OFF, with the accepted narrow RTK exception; CBM automatic index/watch remain OFF; native memories remain OFF and Git project memory is selected; Fast is excluded; all OpenAI assignments are Astra-only. User-stopped opencode-kit research was not resumed.

Current evidence reused, not re-run:

- [native-context-contracts.md](native-context-contracts.md): accepted Astra runtime pilot 2.1 and autonomous lifecycle probe 1.1. The probe proved current-session skill delivery through hook additionalContext, including compact and resume. Global ordinary hooks remain prohibited.
- [native-context-delivery.md](native-context-delivery.md): linked experimental flag, ordinary runtime, explicit false override and profile rollback passed task 2.2.
- [native-project-memory.md](native-project-memory.md): independent fixture, fresh clone and two-worktree conflict passed tasks 1.3/1.4; [global lifecycle](native-workflow-lifecycle.md) subsequently passed too.

Official Codex [hook documentation](https://learn.chatgpt.com/docs/hooks#sessionstart) lists SessionStart sources `startup`, `resume`, `clear` and `compact` and additionalContext delivery. The [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference) describes the context, Fast, hooks and native-memory settings. Those documented contracts do not authorize restoring ordinary hooks or enabling native memories.

## Contradictions corrected

- `autonomous-skill-evolution` treated `global/hooks.json` as an active diagnostic stack and assumed SessionStart/PostToolUse/Stop handlers plus harness-lsp. Those files are empty/retired under hooks-off. Compact/resume skill recovery remains required and is now an owning blocker until a supported hooks-off path exists.
- Parent follow-up also corrected tasks 5.3/5.4, which still prescribed SessionStart and retained diagnostic handlers. They now require the same compact/resume/child outcomes through an allowed mechanism and keep explicit checks and the RTK exception; unsupported delivery remains unfinished.
- `adopt-project-memory-and-native-workflows` still described experimental context as parser-only and all model-backed acceptance as unexecuted. Pilot 2.1 and Git-memory 1.3/1.4 are accepted; subsequent ordinary runtime/false override/profile rollback accepted task 2.2. Subsequent global skill lifecycle acceptance also passed; see native-workflows.md. Native memories and Fast stay excluded.
- `accelerate-verified-delivery` still made integrated acceptance depend on restoring diagnostic-hook repair and left opencode-kit quantitative work ambiguous. Historical hook comparisons stay historical; remaining open tasks 5.3/5.4/6.4 wait on the pending two-consumer decision and must not resume opencode-kit.
- `improve-installed-tool-workflows` now records those owners, evidence limits and the same hooks-off/RTK/CBM/Astra/Fast constraints without closing the other changes.

## Remaining decisions and blockers

- Unsupported same-session compact/resume skill delivery without ordinary hooks remains the owning blocker in `autonomous-skill-evolution`. The [pinned runtime investigation](skill-catalog-runtime.md) identifies the active-turn host snapshot; actual ordinary-TUI probes with experimental context false and true confirm the late skill is absent from the first post-compaction request and present in the next turn. Another supported extension route is not yet established.
- `adopt-project-memory-and-native-workflows` section 5 now has accepted owned lifecycle, parsed native discovery and global Check evidence; these base prerequisites do not close the additional tool-interface comparisons.
- Whether to retain a two-consumer quantitative benchmark after withdrawing opencode-kit support remains a user decision; `accelerate-verified-delivery` tasks 5.3, 5.4 and 6.4 stay open.

All four affected changes passed strict OpenSpec validation; 46 modified local
links resolved in the delegated check. Parent review checked the retained outcomes
and the current empty ordinary-hook configuration. Temporary out-of-scope Python
extraction helpers created during research were removed after a concrete scope
correction; they are not delivered or part of acceptance. No model/auth/quota
failure or reserve substitution occurred.

This planning reconciliation does not implement routes, restore hooks, run models or mark other changes complete.
