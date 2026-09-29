## Context

The canonical principles (`global/principles-of-work.md`, delivered through the
live global `AGENTS.md` link) already require an early verified MVP and
delegation inside the active `team-lead` role. Two gaps remain: no explicit
effort-classification gate against the earliest MVP, and a main-session posture
stated only for the active lead role. The memory workflow
(`.agents/skills/project-memory/SKILL.md`, `git-project-memory` specification)
records durable decisions passively, so confirmed priorities and deferred work
can vanish between sessions. The principles file is 24,496 bytes against the
enforced 24,576-byte limit, leaving 80 bytes of headroom.

## Goals / Non-Goals

**Goals:**

- Make MVP-priority classification an explicit, repeatable gate in the
  portable principles, with deferred work recorded rather than done or lost.
- Make the coordination/judgment/decomposition posture of the main session a
  general principle with its context and tokens treated as the scarce resource.
- Make persistence of cross-session decisions, agreements and deferred work a
  decision-time duty of the memory workflow, including explicit remember or
  roadmap requests.
- Keep every change inside existing owners and the enforced principles size
  limit.

**Non-Goals:**

- No scheduler, background writer, automatic end-of-turn memory write, or new
  enforcement tooling; prompt instructions do not guarantee compliance.
- No change to delegation activation rules, executor configuration, quotas or
  the solo default for small tasks.
- No private consumer identities, private roadmap items or session evidence in
  the shared kit; generalized rules only.
- No token or speed savings claims without measurement.

## Decisions

1. **Add rather than rewrite existing requirements.** The MVP gate and the
   main-session posture are new normative behaviors, so the
   `global-working-principles` delta uses ADDED requirements and leaves the
   existing outcome and delegation requirements intact. Alternative - editing
   the large existing requirements - risks losing accepted behavior at archive
   time and widens the size-constrained canonical text more than needed.
2. **One authoritative home for the memory duty.** The decision-time
   persistence trigger is specified once, in the modified
   `git-project-memory` requirement; the canonical principles carry only a
   short operational pointer so the global specification does not duplicate the
   memory contract. Alternative - mirroring the full trigger in
   `global-working-principles` - would create two spec homes for one fact.
3. **Modify the existing memory requirement in place.** The recording trigger
   already lives in `Bounded maintenance in the owning checkout`, so the delta
   copies that requirement fully and strengthens its trigger while preserving
   its boundaries (owning checkout, reviewable Git changes, no automatic
   propagation) and its no-noise scenarios.
4. **Compression plus one explicit, need-based limit raise.** Additions to
   `global/principles-of-work.md` are paid for first by tightening existing
   wording without dropping accepted constraints (-201 bytes, exact byte
   accounting). The remaining +541 bytes cannot fit the 24,576-byte limit, so
   on the user's 2026-09-28 decision the native limit rises by 512 bytes to
   25,088 (projected content 25,037) instead of weakening the new
   requirements; the raise travels with the check, its test and the spec
   scenario. A silent raise, or one larger than the recorded need, remains
   rejected.
5. **Skills only echo their owning specs.** `team-lead` gains a brief MVP and
   lead-context line in its operating objective; `project-memory` gains the
   decision-time trigger. No new sections or duplicated contracts, keeping the
   skills aligned with the specs they operationalize.
6. **Generalized decision record only.** `docs/project-decisions.md` records
   the confirmed user preference in generalized form; the user's concrete
   private roadmap items belong to the consuming project's own memory, written
   by a session in that project.

## Risks / Trade-offs

- [Instruction wording does not enforce behavior] -> Acceptance checks verify
  text, size, links and loading, and explicitly claim no measured compliance,
  token or speed gain; native session evidence remains the only behavioral
  proof.
- [Principles size limit leaves 51 bytes of headroom] -> The byte check gates
  the result at 25,088; further growth compresses first or returns to the user
  with exact byte accounting instead of weakening requirements.
- [Decision-time writes could add memory noise] -> The trigger names
  cross-session effect as the condition and keeps transcript, progress and
  transient exclusions with the existing no-redundant-entry scenario.
- [Posture could be read as mandatory orchestration of tiny tasks] -> The
  requirement keeps the cheaper-than-handoff and no-useful-decomposition
  exceptions and forbids manufactured helpers, tasks or records.

## Migration Plan

Edit the canonical files in this checkout; the live global `AGENTS.md` link
serves the new text to new sessions immediately, while already running
sessions must be restarted to reload initial instructions. Skills are live
links and need no separate install. Rollback is an ordinary Git revert of the
same files; no executable, configuration or consumer migration is involved.
