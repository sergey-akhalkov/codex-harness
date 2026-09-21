## Why

Token-saving decisions currently rest on scattered manual observations. The kit
has runtime discipline (`token-efficient-agent-workflow`) and delegation
accounting (`codex-harness delegation-usage`), but no measured answer to "what
consumed tokens across my Codex sessions, and which optimization is worth
adopting". Local rollout files already contain the needed evidence; third-party
analyzers add a trust and lifecycle burden and price subscriptions in proxy USD.

## What Changes

- Add a first-party Rust crate `token-audit` that reads local Codex rollout
  sessions and produces measured token-usage reports (per session, project,
  model, effort and day) with cache efficiency, context repayment and
  instruction-floor size.
- Add an optimization layer: ranked findings where every number carries a basis
  (`measured` / `inferred` / `estimated`), a measured token mass at stake, an
  owning record or skill for remediation, and a validation plan; no costs,
  quota percentages or auto-applied fixes.
- Add a local baseline/diff loop (`baseline save`, `baseline diff`) so an
  adopted idea can be re-measured against the prior audit.
- Extract the existing rollout JSONL event reading from
  `codex-harness delegation-usage` into a single shared owner reused by both
  the existing accounting command and the new analyzer; the
  `delegation-usage` CLI contract stays unchanged.
- Add a `tokenomics` diagnostic skill that invokes the analyzer, maps findings
  to existing owners instead of creating parallel instructions, and records
  accept/reject decisions in existing records; delivered through the normal kit
  installation lifecycle and verified outside the checkout.

## Capabilities

### New Capabilities

- `token-audit`: measured local token-usage analysis and optimization loop for
  Codex rollouts — reports, findings with basis labels and owner pointers,
  baseline diffing, and the `tokenomics` skill consumer.

### Modified Capabilities

None. `subscription-efficiency` keeps owning delegation usage accounting and
`token-efficient-agent-workflow` keeps owning runtime discipline; the reader
extraction is an implementation detail with unchanged behavior.

## Impact

- New workspace crate `crates/token-audit` (library plus thin binary) and a
  small shared rollout-reader owner replacing the inline modules in
  `crates/codex-harness/src/delegation_usage*.rs` (CLI behavior and existing
  tests preserved).
- New first-party executable entry in `docs/evidence/executable-ownership.json`
  and the native-check routes in `docs/rust-native.md`.
- Local private state under `CODEX_HOME/harness/token-audit` for baselines;
  reports hash project paths by default, following the existing
  `delegation-usage --private-sources` convention.
- Owning documentation: a new audit-loop record linked from
  `docs/memory/README.md`; `docs/token-workflow.md` keeps runtime discipline
  and links to the analyzer without duplicating facts.
