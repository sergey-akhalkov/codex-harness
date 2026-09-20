## 1. Instruction and documentation surfaces

- [x] 1.1 Rewrite the visibility paragraph in `global/harness.config.toml` developer instructions: dedicated titled tab/pane/window per conversation, dispatch command establishes it, tabs sufficient, no simultaneous tiling requirement, explicit prohibition of resizing/moving/arranging desktop windows including the lead's own terminal; keep the no-screenshot/no-pixel-polling rule
- [x] 1.2 Update `.agents/skills/team-lead/SKILL.md` (`Assign and brief`): executor view is a titled terminal tab or window opened by `executor spawn`; add the no-window-management rule; verify no remaining section demands separate or simultaneously visible windows
- [x] 1.3 Update `docs/agent-delegation.md` visibility paragraph to the same rule and remove its duplicated view-loss sentence
- [x] 1.4 Revise the decision record in `docs/project-decisions.md` to the user's corrected rule and retarget its stale `orchestrate-subscription-agents` link to the archived change

## 2. Consistency and checks

- [x] 2.1 Grep instruction surfaces and main specs for `simultaneously visible`, `switchable list`, `own window` and `separate windows`; confirm remaining hits are historical evidence, change artifacts or unrelated, and that no delivered instruction or non-archived spec still forbids tabs or demands tiling
- [x] 2.2 Run documentation checks for changed files: local links resolve, no private consumer data or machine paths, `git diff --check` reports no whitespace errors
- [x] 2.3 Validate with `openspec validate tab-first-conversation-visibility --strict` and confirm it passes
- [x] 2.4 Verify live delivery needs no rebuild: the installed `team-lead` skill link targets this checkout and the installed launcher already reports tab-first `executor spawn` behavior
