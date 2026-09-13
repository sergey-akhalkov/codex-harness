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

## Astra instruction adaptation

The 2026-09-13 adaptation uses [Astra guidance](https://developers.openai.com/api/docs/guides/latest-model/gpt-6-astra),
[skill and prompt guidance](https://developers.openai.com/blog/rethinking-skills-and-prompts-for-gpt-6-astra)
and [skill discovery](https://learn.chatgpt.com/docs/build-skills). The
[owning change](../openspec/changes/archive/2026-09-13-align-instructions-with-astra-guidance/design.md)
records the broader source audit and observed behavioral acceptance. These
edits do not establish perfect obedience or a measured efficiency improvement.

| Standing principle owner | Disposition and preserved constraint |
| --- | --- |
| Opening delivery principle; Outcome and completion | Retained: an early usable result does not close unfinished requirements. |
| Language and shell defaults | Consolidated: Rust and PowerShell remain defaults for main and delegated work; concrete exceptions require explanation and do not authorize migration. |
| Mandatory skill use | Consolidated: every applicable nonredundant skill remains required, including named skills within scope; metadata routes selection and supporting detail loads only when needed. |
| MCP tool selection | Retained: actual tool context, source coverage, bounded retrieval, current evidence and fallback constraints remain necessary. |
| Everyday use and design discovery | Retained: establish ordinary use, distinguish requirements from mechanisms and resolve consequential unknowns without questionnaires for routine decisions. |
| Quality and evidence | Adapted: preserve required checks and real execution evidence; additional tests and review follow concrete risk, with no new test merely mirroring a minor wording edit. |
| Autonomy and authority | Adapted: action requests lead to completion, steering preserves unfinished work and pending clarification permits independent progress. Existing authority and external workflow boundaries remain binding. |
| Simplicity and reuse | Adapted: research remains required for substantive decisions; example tools, languages, approvals and test counts apply only under their documented conditions. Reuse, dependency trust and supported behavior remain binding. |
| Working environment before implementation | Adapted: verify setup before dependent code work; a documentation correction does not require unrelated language or graph preparation. Restore relevant broken setup and reuse valid checks. |
| Speed, feedback, and recovery | Retained: investigate the original failure, preserve recovery and complete parent acceptance; no arbitrary retry count or inferred performance gain. |
| Publication boundaries and documentation | Retained: public portability, one authoritative home and preservation of necessary unresolved evidence. |
| Context and collaboration | Adapted communication only: plain outcome-led prose and useful lists. Existing scoped retrieval, delegation ownership, durable memory, untrusted-input and evidence obligations remain. |

The repository's `AGENTS.md` routes documentation corrections to their applicable
checks and uses the documentation map only when route discovery is needed.
Externally maintained OpenSpec instructions are protected, including their
confirmation rules; this adaptation does not override them. Installed provider
routing and simultaneous conversation visibility remain separate binding
configuration contracts in the [delegation guide](agent-delegation.md).

## Completeness

All 38 named source principles were accounted for in the working text:
preserved, simplified, merged or adapted as above. Source process labels such as
Material, Value Ready, split-or-justify and Delivery Checkpoint State are not
standing product rules.

Loading checks confirm instruction transfer. Effect on later defects and speed
is evaluated in real project work; this adaptation does not claim a speed
percentage or an incident-free guarantee.
