## Context

Activation semantics live in three places: the `lead-agent-orchestration`
specification (contract), the `team-lead` skill (workflow owner) and the
portable principles' delegation paragraphs (global instructions, delivered as
a live link from this checkout). Executor widening is already denied by the
existing isolation requirement and launcher environment marker; no controller
code changes are needed for it.

## Goals / Non-Goals

**Goals:**

- Bound autonomous activation to the main session and to tasks that repay
  orchestration, with the activation basis stated before spawning.
- Make lead token economy explicit, including the delegation-overhead
  exception for trivial corrections.

**Non-Goals:**

- No launcher, controller or executor-environment changes; existing guards
  already refuse nested executor and agent spawning.
- No new quota, pacing or board machinery.

## Decisions

- Modify the existing activation requirement (with a rename) instead of
  adding a capability: activation already has one owner and the change is a
  boundary shift, not a new workflow.
- Keep enforcement instruction-level for activation (model-facing wording,
  pinned by the harness-core contract test) and rely on the existing hard
  guards for executor widening; activation is a judgement call, not a
  sandbox boundary.
- Define the trivial-correction exception as "delegation overhead exceeds
  the work itself" rather than a size threshold, and exempt it from
  idle-capacity notes so the exception cannot breed bureaucracy.

## Risks / Trade-offs

- [Over-activation on modest tasks] -> gate on genuinely parallel,
  independently verifiable slices that repay orchestration, keep the small
  direct task exclusion, and require the stated basis.
- [Under-activation through ambiguity] -> keep all explicit triggers,
  including questions about why executors are unused.
- [Conflict with utilization accounting] -> trivial corrections are defined
  as not executor-suitable, so they create neither assignments nor
  idle-capacity records.
