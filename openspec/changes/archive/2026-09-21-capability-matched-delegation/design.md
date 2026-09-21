## Context

See `proposal.md` for motivation. The delegation contract already lives in
`openspec/specs/agent-delegation/spec.md` ("Economical delegation through
configured roles"); the `team-lead` skill and `docs/agent-delegation.md` carry
its operational guidance, and `global/orchestration.toml` fixes the dispatch
profiles. The gap is wording: current keep-with-lead criteria are size and
coupling, while reasoning capability appears only as an assignment input
("according to reasoning needs") without a pre-dispatch analysis duty or an
over-difficulty rule.

## Goals / Non-Goals

**Goals:**

- Make lead-owned pre-dispatch analysis and capability-matched slice sizing an
  explicit, testable contract in the owning spec.
- Carry the same rule, briefly, in the `team-lead` skill's assignment guidance
  and the agent-selection document.

**Non-Goals:**

- No launcher, configuration, model-routing or board-schema changes; profiles
  stay exactly as configured and dispatch mechanics are untouched.
- No new measurement, gating or reporting workflow for judging slice
  difficulty.
- No weakening of executor-owned investigation or of existing acceptance
  duties.

## Decisions

1. **Instruction-level contract, not mechanical enforcement.** Capability
   matching is semantic judgment; a required spawn field would become ritual
   text and a formal gate. The board brief carries the analysis; acceptance
   review and the existing feedback loop expose violations. Alternative
   rejected: launcher-enforced difficulty metadata.
2. **Match by slice shaping, not model substitution.** When a slice is too
   hard, the lead keeps it, splits it, or uses the bounded principal
   consultation. This preserves the existing rule that dispatch uses exactly
   the configured profiles. Alternative rejected: routing hard slices to a
   stronger executor profile, which the configuration contract forbids and
   which would hide quota and identity attribution.
3. **Capability-matched wording instead of "the lead analyzes everything".**
   The trigger is a slice's reasoning demands versus the available profiles,
   and the requirement explicitly preserves executor investigation inside
   assignment boundaries. Alternatives rejected: a blanket lead-analysis
   monopoly, which serializes work and spends the scarcest turns on routine
   exploration the executors already own.
4. **One authoritative home.** The normative rule stays in the
   `agent-delegation` requirement; the skill and document state the
   operational rule and already point to the policy records instead of
   duplicating the full contract.

## Risks / Trade-offs

- [Lead over-hoards work] → The requirement and scenarios keep
  executor-owned investigation explicit; the overhead scenario still mandates
  direct completion only by cost, never by difficulty alone for
  capability-sized slices.
- [Capability estimates are subjective] → The observable contract is that an
  over-difficult slice is not dispatched unmodified; misestimates surface as
  steering churn or lead rework already visible on the board.
- [Guidance drift between spec, skill and document] → Tasks include a
  cross-file consistency check, and archive syncs the main spec from the
  delta.
- [Installed skill copy lags the checkout] → Delivery refreshes the installed
  skill through the kit lifecycle and verifies the installed text.

## Migration Plan

Documentation-only change: update the three guidance surfaces, validate the
OpenSpec change, then archive to apply the delta to the main spec. Rollback is
reverting those files; no state, configuration or installed launcher migration
is involved.
