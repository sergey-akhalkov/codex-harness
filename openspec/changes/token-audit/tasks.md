## 1. Shared rollout reader

- [x] 1.1 Extract a tolerant rollout event reader (both usage formats,
  response-identity deduplication, turn association, instruction byte
  capture, coverage warnings) into `harness-core` and make
  `delegation_usage` consume it; verify with new reader unit fixtures for
  both event formats
- [x] 1.2 Confirm the extraction is behavior-preserving by running
  `cargo test -p codex-harness --test delegation_usage` with unchanged
  expectations

## 2. Analyzer report

- [x] 2.1 Add the `crates/token-audit` workspace member (library plus thin
  binary) with `report`/`findings`/`baseline` command skeletons, JSON and
  text output, and `--days`/`--format` options; verify `cargo test -p
  token-audit` runs the skeleton CLI tests
- [x] 2.2 Implement sessions-directory discovery and streaming aggregation
  per session, project, model, effort and day with response deduplication
  and coverage warnings; verify with synthetic mixed-format fixtures whose
  golden aggregates are asserted
- [x] 2.3 Hash project/workspace identities by default and honor
  `--private-sources`; verify with a redaction test asserting that default
  output contains no raw local paths and no transcript content
- [x] 2.4 Add context economics metrics (cache efficiency, context
  repayment multiplier, instruction-floor bytes, unavailable marker for
  missing turn identity); verify with fixture tests including a marathon
  session and a session without turn-identified usage

## 3. Findings

- [ ] 3.1 Implement the versioned finding contract (id, basis, mass_tokens,
  evidence locators, owner, validation plan) with the measured-only default
  filter and hidden-basis counts; verify with contract tests
- [ ] 3.2 Implement the first detectors (context repayment, low-worth
  sessions, session outliers, tool output mass, effort mix) with per-detector
  fixtures; verify ranking by measured mass and suppression of zero-mass
  findings

## 4. Baseline loop

- [ ] 4.1 Implement `baseline save` under `CODEX_HOME/harness/token-audit`
  with a latest pointer; verify saved snapshots contain aggregates and
  hashed identities only
- [ ] 4.2 Implement `baseline diff` against latest or named baselines with
  movement per session, project, model and day, and an explicit marker for
  incompatible snapshots; verify with before/after fixture tests

## 5. Skill, delivery and documentation

- [ ] 5.1 Add the `tokenomics` diagnostic skill (analyzer invocation, owner
  mapping, decision recording in existing records, no parallel efficiency
  instructions); verify the kit skill inventory includes it and its
  description stays disjoint from `token-efficient-workflow`
- [ ] 5.2 Verify delivery through the kit installation lifecycle against an
  installed consumer outside the checkout, following the native check routes
  in `docs/rust-native.md`
- [ ] 5.3 Update executable-ownership accounting, `docs/rust-native.md`, the
  memory index and the owning audit-loop record; verify
  `codex-harness ownership-check` and local-link checks pass

## 6. Final acceptance

- [ ] 6.1 Run the applicable native check suite and `openspec validate
  --change token-audit --strict`; record the exercised commands and results
  in the change notes before archive
