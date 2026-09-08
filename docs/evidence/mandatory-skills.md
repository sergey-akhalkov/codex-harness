# Mandatory relevant skills

User decision, 2026-09-07: relevant available skills must be used in every project,
including by subagents. The [global source](../../global/principles-of-work.md#mandatory-skill-use)
now requires selection before choosing a workflow, actual reading and application,
reassessment when context changes, first-use disclosure and complementary workflows.
Task familiarity and access to ordinary tools do not excuse skipping a relevant skill.

The earlier context paragraph now references this obligation. The accepted MCP
section and all unrelated instructions remain intact. This is an instruction
change; it does not install skills or alter their invocation metadata.

## Contract and scenario review

The official [Build skills documentation](https://learn.chatgpt.com/docs/build-skills)
describes matching by skill description, progressive loading of the full SKILL.md,
and the explicit-only `allow_implicit_invocation: false` setting. The user's new
rule requires use when the skill applies under those conditions; it preserves
existing disabled/explicit-only settings and instruction precedence.

The following are manual policy checks, not independent model evaluations:

| Context | Required outcome under the new rule |
| --- | --- |
| Codex configuration work | Apply OpenAI Docs and its relevant reference/source workflow. |
| Implement an OpenSpec change involving Codex configuration | Apply the implementation skill and OpenAI Docs to their respective portions; do not duplicate equivalent procedures. This task uses both. |
| Update an existing skill's content | Apply Skill Creator even if the edit appears easy. This instruction-policy change does not itself modify a skill. |
| Generic application work that happens to mention Codex | Do not select OpenAI Docs from that keyword alone. |
| Ordinary research without a Deep research request | Preserve that skill's explicit invocation restriction. |
| Relevant skill cannot be read, or conflicts with explicit user scope | Report the specific limitation/conflict, preserve controlling instructions, continue authorized work where possible. |
| Child receives a bounded task | The child must assess, read and apply relevant skills; naming them in the brief is not sufficient. |

## Activation and verification

The existing `C:\Users\noilw\.codex\AGENTS.md` symbolic link targets the repository
source. Changes therefore ship through the established kit lifecycle and load in
new ordinary sessions. No source copies, service restarts or model-routing changes
are required. Codex CLI 0.153.4 `debug prompt-input` loaded the full updated source
from two outside Git repositories in the retained TEMP fixture. The second also
loaded its local AGENTS.md marker. No instruction/profile override was supplied.
The linked file and source hashes matched, and comparison with `skills-before.md`
confirmed the entire MCP section was unchanged.

Seven documents passed 41 local link checks and the new skill-anchor checks.
OpenSpec strict validation and scoped `git diff --check` passed; automatic current
Markdown diagnostics reported the source, decisions, evidence and four change
artifacts clean. The old optional-sounding sentence is replaced, with one
authoritative mandatory-skill section. All four change tasks are complete.
These checks establish instruction delivery, not universal future model
compliance or measured token savings. No additional model-backed evaluation was
needed for this bounded instruction change.

## Recovery

The local pre-change backup is `skills-before.md` in TEMP
`harness-mcp-routing-7765f8171af94add821625e0a756f729`.
Rollback removes only the `Mandatory skill use` section and restores the prior
single context sentence. Preserve the MCP section and any later unrelated edits.
The live global link stays in place.

Archive audit 2026-09-08: current source and global link hashes matched; native prompt-input from both external repositories loaded the complete source and the second local AGENTS.md. Primary receipts are retained in TEMP harness-spec-audit-8ae152f990de442da00b60c3a6fc9466. Main requirements were synchronized and strict validation passed before archival. The first audit assertion expected an absent phrase; it was corrected to compare the complete source without changing product instructions.
