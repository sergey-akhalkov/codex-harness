## 1. Contract and instruction owners

- [x] 1.1 Update the `team-lead` skill description, activation section, operating objective and utilization exception for main-session autonomous activation, the executor/helper prohibition and the trivial-correction exception; verify the changed phrases are present in `.agents/skills/team-lead/SKILL.md`
- [x] 1.2 Update the delegation paragraphs in `global/principles-of-work.md` and verify `rg -n "A user request for executors activates"` no longer matches the file
- [x] 1.3 Update the delegation-default delta for `global-working-principles` to match the principles and skill wording
- [x] 1.4 Update the activation sentences in `docs/global-instructions.md` and `docs/agent-delegation.md` and verify their existing relative links still resolve
- [x] 1.5 Record the confirmed activation decision in `docs/project-decisions.md`

## 2. Verification

- [x] 2.1 Verify `harness-source-check --root .` reports no principles-size finding for this change (24562 bytes, limit 24576) and no finding inside files this change edits; the remaining 44 findings pre-date this change and stay outside its scope
- [x] 2.2 Update the harness-core skill contract test to pin the new activation bounds and verify `cargo test -p harness-core team_lead_skill_bounds_activation board_and_lead_skills` passes
- [x] 2.3 Verify `openspec validate autonomous-lead-activation --strict` passes and no stale explicit-only activation wording remains in maintained sources
