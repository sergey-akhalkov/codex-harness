## 1. Portable principles

- [x] 1.1 Tighten existing wording in `global/principles-of-work.md` to free bytes for the additions without dropping any accepted constraint, and verify by diffing the touched sections against the previous revision
- [x] 1.2 Add the MVP-priority effort gate (accelerates / required by acceptance / deferrable, with deferred work recorded in its existing owner) and verify the section states all three classes plus the deferred-work record
- [x] 1.3 Add the main-session posture with scarce lead context (coordination, judgment, ideation, decomposition, integration, acceptance stay lead-side; substantial transferable execution is delegated; cheaper-than-handoff and undecomposable work stays direct) and verify it forbids manufactured helpers for tiny work
- [x] 1.4 Add one short pointer that cross-session decisions, agreements and deferred work are recorded in the owning memory record when decided, and verify it does not restate the `git-project-memory` contract
- [x] 1.5 Verify `(Get-Item global/principles-of-work.md).Length` stays within the enforced 25,088-byte limit
- [x] 1.6 Raise the enforced principles limit to 25,088 bytes in `crates/codex-harness/src/bin/harness-source-check.rs`, its `source_check` test expectation and the `Native instruction invariant checks` delta, and verify the native size-limit test passes at the new boundary

## 2. Skills

- [x] 2.1 Extend the operating objective in `.agents/skills/team-lead/SKILL.md` with the MVP gate and lead-context economy, and verify the change stays inside the existing objective wording and frontmatter
- [x] 2.2 Strengthen the recording trigger in `.agents/skills/project-memory/SKILL.md` for decisions, agreements and deferred work affecting future sessions, including explicit remember or roadmap requests, and verify transcripts, progress logs and unconditional end-of-turn writes remain excluded

## 3. Decision record

- [x] 3.1 Add one dated generalized decision to `docs/project-decisions.md` covering the MVP effort gate, main-session posture and decision-time memory persistence, and verify the entry contains no private consumer identities, paths or private roadmap items

## 4. Verification

- [x] 4.1 Run the installed source check (`codex-harness-check --source` route from `docs/source-diagnostics.md`) and verify links, principles size and source hygiene pass
- [x] 4.2 Run strict OpenSpec validation for this change and verify both deltas and all artifacts pass
- [x] 4.3 Run the model-free `codex debug prompt-input` loading check from an outside consumer directory and verify the new canonical text is served through the live global `AGENTS.md` link
- [x] 4.4 Verify existing documentation routes (`docs/memory/README.md`, `docs/agent-delegation.md`, `docs/global-instructions.md`) still point to valid owners and anchors after the edits
