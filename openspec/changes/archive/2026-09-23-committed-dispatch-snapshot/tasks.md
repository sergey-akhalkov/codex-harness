# Tasks: committed-dispatch-snapshot

## 1. Instructions

- [x] 1.1 `.agents/skills/team-lead/SKILL.md`: require a committed snapshot before every dispatch (`--base` naming the new local commit or verified committed HEAD), forbid live-slot file copying as synchronization, and add the executor base-verification duty to the brief contract.
- [x] 1.2 `docs/agent-delegation.md`: state the committed-snapshot base rule, the unauthorized-commit boundary and the redispatch path in the executor-worktree freshness contract.
- [x] 1.3 `global/principles-of-work.md`: add the portable committed-snapshot lead-handoff rule.

## 2. Records

- [x] 2.1 Record the confirmed decision in `docs/project-decisions.md` and refresh its update date.

## 3. Verification

- [x] 3.1 `openspec validate committed-dispatch-snapshot --strict` passes.
- [x] 3.2 Grep audit: no remaining instruction text presents post-launch file copying as synchronization, and the skill, guide and principles all name the committed-base duty.
- [x] 3.3 Local links in edited documents resolve, and the edited public files contain no private paths, tokens or consumer identity.
