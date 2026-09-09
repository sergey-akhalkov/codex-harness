# Git memory and native workflows acceptance

This record maps `adopt-project-memory-and-native-workflows` to its owning
evidence. Skill lifecycle and global discovery passed as described below.
It does not close the separate Rust migration,
tool-interface comparison or autonomous skill-evolution changes.

| Requirement and scenarios | Implementation and observed acceptance |
| --- | --- |
| Project-owned portable memory; existing owners and fresh clone | Globally linked `project-memory`; [independent Git consumer and fresh clone](native-project-memory.md) use a short index pointing to the existing decisions/check records, with no personal-history dependency |
| Relevant retrieval; stale technical entry and changed user instruction | Same consumer reads relevant owners, checks the stale CLI command against source and actual tests, and records the verified correction; the later controlled user decision wins the worktree conflict |
| Bounded maintenance; concurrent worktrees and no new knowledge | Actual Git conflict retains both versions before integration; fresh clone produces no needless memory diff; no separate model call is added to normal maintenance |
| Global discovery and disconnection of memory | Source-linked skill and project-local records; [final common lifecycle](native-workflow-lifecycle.md) preserves memory and unrelated files through disconnect/reinstall |
| Reversible context trial; eligibility, absent support and explicit rollback | [Pilot](native-context-contracts.md) separates parse/eligibility/runtime and preserves the early constraint after compaction. [Ordinary delivery](native-context-delivery.md) proves source-linked enabled, explicit false and profile-removal paths; failed setup stays evidence rather than success |
| Version-specific side questions and compatible native route | [Side-question guide](../native-side-questions.md) records the user's working `/btw`, aliases, prerequisites/return controls, existing feature selection and ConPTY limits; no repeat model probe was made for the closed incident |
| Scope and cost preservation | Actual context requests use Astra and the existing subscription route; Goals remain enabled, Fast and native memories are not activated, ordinary hooks remain off, shared proxy is preserved |
| Proportional isolation and explicit dirty inputs | Globally linked `isolated-worktree`; [owned Rust consumer](native-worktrees.md) transfers required tracked/untracked inputs, preserves unrelated work and avoids imposing isolation on small coupled edits |
| Correct checkout/resources; stale tool context and shared services | Actual fresh Codebase Memory identity and Serena activation/navigation in the task worktree; source coverage and check results are recorded; acceptance owns its processes and leaves shared services intact |
| Integration and recoverable cleanup | Real collision, conflict, dirty-removal refusal and initially failing combined test; corrected combined tests pass, both original versions and unsaved work remain, only an independently checked clean owned worktree is removed |
| Bounded structured execution and read-only effects | Globally linked `structured-codex-run` and schema; [actual Astra consumer](rust-structured.md) reads an outside-checkout fixture under explicit read-only sandbox with recorded inputs, limits and independent Rust oracle |
| Output validity versus success; wrong, missing, malformed, stale and timed-out output | Deterministic [structured acceptance](rust-structured.md) distinguishes process/event/output/oracle failure; plausible output after forced stop never passes, and schema-valid wrong findings fail the independent oracle |
| Deliberate model routing; unsupported external schema route | Actual consumer records Astra/openai/existing subscription. The workflow requires reporting rejected/ignored external schema contracts and does not claim Grok schema enforcement without an actual proof |
| Installed assets and lifecycle | Native schema candidate uses the directly linked source asset with before/after integrity checks; the existing installed helper remains transitional until the separately scoped native installer migration |

The native-context Rust helper's three ordinary requests took 20.66, 20.63 and
21.41 seconds; three preceding trust setup failures made no model requests.
The memory consumer and fresh clone took 88.71 and 58.09 seconds. The structured
consumer took 24.154 seconds after an initial provider-configuration rejection.
These are individual acceptance runs with their retained failures/corrections,
not matched performance comparisons or a measured percentage of quota saved.

Full commands, fixture identities, detailed receipts, meaningful failure oracles
and limits belong to the linked records. Reuse those checked inputs; repeat a
model-backed run only for a new relevant change or unresolved concern.

## Final common lifecycle

The final global `install.ps1 -Mode Check -CoreOnly -CodexCommand <original>`
from outside this repository returned Connected: 19 source links, 12 skills and
three agents. The global base configuration hash remained unchanged. The
`final-global-check.json` receipt is in the [context delivery](native-context-delivery.md)
evidence root. [Owned lifecycle and native discovery](native-workflow-lifecycle.md)
passed preview, install/update/reinstall, checks, disconnect/reconnect, direct
resource identity and exact preservation checks. The parsed native catalogues
from the independent Git workspace confirm all three skills and local AGENTS.

Strict validation of this change passed. The final documentation pass resolved
125 local file links in 11 applicable documents (existing anchors were not
revalidated). Earlier installer acceptance covered 24 scenarios and 356 assertions;
the final owned lifecycle adds the new skills/resources and context-preservation
checks. Rust context/structured checks and their actual runtime consumers are
recorded above. Source scopes unchanged by this delivery reuse their applicable
checks; the unfinished native installer requires its own later migration suite.

All 16 tasks are complete. The change was
[archived on 2026-09-09](../../openspec/changes/archive/2026-09-09-adopt-project-memory-and-native-workflows/tasks.md)
after all four main specs exactly matched their accepted deltas and strict
validation passed. [Closure audit](spec-closure.md#native-workflows--2026-09-09)
records the move and the remaining separate changes.
