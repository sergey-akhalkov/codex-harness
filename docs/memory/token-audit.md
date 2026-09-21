# Token audit loop

Measured token-usage analysis over local Codex rollouts, owned by the
`token-audit` crate and operated through the `tokenomics` skill. This record
owns the loop's decisions and limits; runtime discipline stays in
[token workflow](../token-workflow.md) and delegation accounting in
[agent delegation](../agent-delegation.md).

## Operating contract

- `token-audit report` aggregates recorded counters per session, project,
  model, effort and day with coverage warnings; identities are hashed unless
  `--private-sources` records them in an explicit local file.
- `token-audit findings` emits a versioned contract (id, basis
  measured/inferred/estimated, mass tokens, evidence locators, owner,
  validation plan); the default filter is measured-only and hidden bases are
  counted, never silently dropped.
- `token-audit baseline save|diff` keeps local aggregate snapshots under
  `CODEX_HOME/harness/token-audit/baselines` with a latest pointer; an
  incompatible snapshot is an explicit marker, not a best-effort comparison.
- One shared rollout reader (`harness-core::rollout_reader`) serves both the
  analyzer and `codex-harness delegation-usage`; format drift is a reader
  defect, not a per-consumer parser.

## Decisions

- 2026-09-21: tool-output findings carry basis `inferred` (any token figure
  derived from recorded bytes is an attribution, not a measurement) and stay
  hidden behind `--all-bases`; the declared bytes/4 estimate lives only in
  the finding's validation method. Evidence: change token-audit acceptance
  fixtures.
- 2026-09-21: the reader measures nested `session_meta.base_instructions`
  objects (current rollout shape) in addition to plain strings; per-turn
  developer instruction bytes were already recorded.

## Limits

No currency, quota or allowance attribution; no transcript content; session
corpus scans are streaming and unindexed - revisit only with a measured
latency complaint.
