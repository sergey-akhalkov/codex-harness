# Adaptation of source-kit principles

[Documentation map](README.md) · [Working principles](../global/principles-of-work.md) ·
[Working-principles specification](../openspec/specs/global-working-principles/spec.md)

The operational text is `global/principles-of-work.md`. It is self-contained
and intended for Codex initial context. This note records the agreed
adaptations; it is not a second policy manual.

**2026-09-10:** early verified end-to-end delivery, transparent operating
conditions and defenses chosen by concrete risk refine delivery priority while
preserving product correctness, required acceptance and completion of the whole
agreed task. Canonical wording is in the working principles and
[project decisions](project-decisions.md#outcome-quality-and-speed).

## Agreed adaptations

The source philosophy is compatible with this pack's goals. Three tensions were
resolved by adapting rules to the specification and concrete risk:

| Source rule | Why it was adapted | Agreed application |
| --- | --- | --- |
| Falsification Before Confidence: fresh context for a material decision and at most one repeat review | A mandatory extra pass may add no useful check; a hard maximum may ignore a remaining serious defect | Independent review from requirements and concrete risk. Repeat a check when changes or an unresolved issue justify it |
| Foundation Value Ready: production consumer and separate integration proof | Useful for a production-readiness claim, but universal application can exceed the agreed spec | Keep matching evidence for the claimed readiness and reuse; task completion is the whole agreed spec |
| Causally Different Retries: change mechanism after two similar failures | A transient failure sometimes has a understood wait-and-retry path | Bounded justified retries are allowed. Change hypothesis or approach when repetition adds no progress or new evidence |

Documentation, research or local infrastructure can be a complete result when
that is what was agreed. An unfinished feature remains unfinished even if a
useful intermediate stage works.

Official alignment used [model guidance](https://developers.openai.com/api/docs/guides/latest-model),
[best practices](https://learn.chatgpt.com/guides/best-practices) and
[subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents).
Detailed source-kit process labels were not copied into the standing text.

## Completeness

All 38 named source principles were accounted for in the working text:
preserved, simplified, merged or adapted as above. Source process labels such as
Material, Value Ready, split-or-justify and Delivery Checkpoint State are not
standing product rules.

Loading checks confirm instruction transfer. Effect on later defects and speed
is evaluated in real project work; this adaptation does not claim a speed
percentage or an incident-free guarantee.
