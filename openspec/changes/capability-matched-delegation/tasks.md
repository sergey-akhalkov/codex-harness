## 1. Guidance surfaces

- [x] 1.1 Extend the `Assign and brief` section of `.agents/skills/team-lead/SKILL.md` with the pre-dispatch analysis duty (requirement interpretation, risk and consequence decisions, approach direction, acceptance conditions), capability-matched slice sizing, and the over-difficulty rule (stay with the lead, split further, or bounded principal consultation - never dispatch unmodified); verify by rereading that executor-owned investigation remains intact and no other section contradicts the rule
- [x] 1.2 Update the selection guidance in `docs/agent-delegation.md` ("How selection works") with the same rule in document voice; verify the paragraph matches the spec delta's trigger, alternatives and executor-investigation preservation
- [x] 1.3 Check cross-file consistency between `openspec/changes/capability-matched-delegation/specs/agent-delegation/spec.md`, the skill section and the document; verify with a bounded review of the three changed passages that they state one rule, not three variants

## 2. Checks and delivery

- [x] 2.1 Run the documentation checks for the changed files: local links resolve, no private consumer data or machine paths, `git diff --check` reports no whitespace errors
- [x] 2.2 Validate the change with `openspec validate capability-matched-delegation --strict` and confirm it passes
- [x] 2.3 Refresh the installed `team-lead` skill copy through the supported kit update lifecycle; verify the installed `SKILL.md` contains the new rule by comparing its text against the checkout
