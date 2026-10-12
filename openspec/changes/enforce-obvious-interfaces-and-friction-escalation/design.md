# Design

## Context

See [proposal.md](proposal.md) for motivation and the [delta specification](specs/global-working-principles/spec.md) for the behavioral contract. The user confirmed this direction on 2026-10-12 and requested planning, not implementation or changes to a consuming project.

The current [principles](../../../global/principles-of-work.md) already cover developer experience, everyday-use discovery, dissent, bounded improvements and reassessment of costly repetition. The gap is explicit coverage of every interface and the response to a workflow that can be forced through but remains materially burdensome. These additions therefore belong in the existing `global-working-principles` capability.

The [delivery guide](../../../docs/global-instructions.md) identifies a live global instruction link; fresh sessions consume the source and running sessions are not proven to reload it. External OpenSpec skills and workflow configuration are protected. The principles have effectively no headroom under the native 25,088-byte check; the existing specification separately requires at most 24 KiB. Meeting the stricter 24 KiB bound satisfies both without changing either limit.

## Goals / Non-Goals

**Goals:** express the accepted obligations in their existing owners, make the fix/report/escalate distinction operational, preserve proportionality and demonstrate behavior separately from successful instruction loading.

**Non-Goals:** add another policy manual, skill, evaluator framework, mandatory feedback log, scheduled review, numeric ergonomics score or consumer implementation. A private motivating example is not an acceptance dependency or publishable fixture.

## Decisions

### Keep one policy owner and consolidate overlapping text

Broaden `Developer experience` into a general interface-design contract. Reconcile `Partnership and dissent`, `Working environment before implementation`, and `Speed, feedback, and recovery` around the new escalation behavior. Keep scope, user authority and protection of required checks in their existing owners rather than copying those rules into every section.

Update `docs/project-decisions.md` with the confirmed durable preference and a route to its owner; update `docs/global-instructions.md` only where operation or verification needs clarification. Do not replicate normative prose in skills or external OpenSpec workflows. This is preferable to a new usability skill because the obligation applies without a special invocation.

### Treat ease of use as a complete consumer contract

Review the intended task at the boundary used by its real consumer: operator, developer, programmatic caller or agent. Inspect necessary decisions, independently maintained inputs, coordination, error discovery and recovery. A single command is useful only if preparation and completion are also owned; a large parameter bag or a hidden manual setup guide does not satisfy that goal.

Favor an ordinary path expressed in domain intent, with explicit advanced controls when needed. Required choices stay visible. Discoverable facts are obtained and checked against the selected authorized configuration, never used to invent authority or silently select a different environment. Internal modularity remains compatible with a convenient public composition; no universal facade or architectural rewrite is prescribed.

Steve Krug's [own explanation](https://sensible.com/dont-make-me-think/#:~:text=Is%20this%20just%20about%20Web%20sites) extends the usability principle beyond websites. Applying it explicitly to APIs, lifecycle contracts and agent tooling is the accepted project policy, not a claim that the book prescribes these implementation choices.

### Use observable friction to choose the next action

| Observed situation | Required response |
| --- | --- |
| A bounded, worthwhile correction is within the task | Correct it, check it and briefly explain the effect; no renewed permission. |
| A systemic improvement is justified but the current safe path is still proportionate | Report it early with a concrete recommendation and the interim cost; preserve useful deferred work in its existing owner. |
| Another dependent attempt would materially compound waste or unreliability | Stop that repetition, complete necessary recovery and present the remedy, interim option and missing decision or capability; continue independent authorized work. |
| The informed user chooses the interim path | Follow that choice with unchanged acceptance and authority, reopening the objection only for new evidence or the existing serious-harm exception. |

Materiality follows consequence, likelihood, recurrence and total completion cost. It is not a fixed retry count, elapsed-time trigger or ban on manual work. Distinguish measured costs from estimates. Suggestions should accompany new decision-relevant findings while action can still help, not accumulate until the final response or repeat on a timer.

### Verify source, delivery and behavior separately

Use existing OpenSpec validation and `harness-source-check --root <checkout>` for document integrity and links. Review preservation of all affected existing obligations and measure the final principles against 24 KiB; a successful size check is not semantic acceptance.

For implementation acceptance, use the existing global link and fresh external consumer verification route. Observe at least one bounded real task using an actual installed tool on owned inputs, with unchanged independent acceptance and an observable preparation or coordination burden. Reuse applicable valid evidence and existing sessions or checks only when their relevant inputs and loaded instructions still match. Evaluate the new scenarios alongside applicable existing external-use obligations; do not substitute synthetic role-play for the real task.

Use bounded counterexamples to check a programmatic interface with coupled options, a cheap authorized correction, an incomplete supported entry point, a material scope decision, an informed interim choice and a necessary full check. Prompts state the outcome and boundaries without prescribing the expected feedback. Inspect actual messages, next actions, effects and recovery. Keep scenario reasoning, observed decisions, completed task behavior and measured efficiency distinct. No new model route, hidden conversation or benchmark infrastructure is authorized by this design.

## Risks / Trade-offs

- **Convenience conceals dangerous or ambiguous effects** -> Preserve explicit meaningful choices, identity validation, authority and recovery; automate dependable mechanics only.
- **Every imperfection becomes a detour or approval request** -> Use the existing proportionality and scope rules, allow justified minor operating conditions and require no suggestion quota.
- **An agent reports a defect but continues the same wasteful cycle** -> Judge the next dependent action and resulting task state, not the wording of its explanation.
- **More policy increases the burden it addresses** -> Consolidate existing sections, retain one owner per rule and meet the stricter existing size bound without weakening unrelated requirements.
- **Static checks are mistaken for adoption** -> Leave behavioral tasks open until fresh applicable external use is observed; report limits without universal compliance or speed claims.

## Migration Plan

Implement only after a separate apply request. Update the owned source and its documentation, validate the delta and affected invariants, then verify the existing global link and fresh external consumption. A correct existing link needs no reinstall; use the normal installation lifecycle only if delivery actually needs repair, preserving unrelated state.

Rollback restores only this change's owned instruction and documentation edits through the existing recovery route. It does not reset unrelated work or consumer state. Running sessions retain their original loading uncertainty. Keep implementation and behavioral acceptance tasks open until actually completed; synchronize and archive through the supported OpenSpec workflow when requested.
