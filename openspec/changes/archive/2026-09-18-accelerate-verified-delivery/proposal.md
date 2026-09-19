## Why

The kit already provides code navigation, diagnostics and delegation, but agents still rediscover project commands, risk exercising the wrong build, and incur slow or verbose feedback without a repeatable measure of the accepted result. The approved research identifies a smaller, evidence-backed next increment: make real verification and regression reproduction reusable across projects, and measure benefit honestly rather than assuming that more instructions or tools help. This change delivers the workflows and closes without a claimed speedup.

## What Changes

- Deliver a globally connected `project-verification` skill that discovers and reuses project-native entry points, focused checks and meaningful failure checks. Keep command provenance and freshness in the target project's existing documentation; distinguish executed evidence from documentation-only suggestions.
- Deliver one focused `reproduce-regression` skill for CLI, MCP and subprocess behavior: freeze relevant versions, reproduce the original trigger, compare an independent reference when available, and minimize the case without changing the failure being investigated.
- Supply small reusable process fixtures where needed for these workflows: isolated resources, concurrent output capture, observable readiness, bounded cleanup and explicit incomplete results. Prefer existing test/runtime capabilities over a new execution framework.
- Add six to eight paired outcome scenarios using the existing native Codex runners and usage accounting. Measure completion, correctness, time to first useful signal, total time through verification/rework, interventions and observed usage; preserve failed and incomplete runs.
- Verify the latency and correctness contribution of `stabilize-diagnostic-reconciliation` through its existing ownership and acceptance. This change depends on that fix for integrated acceptance and does not create a competing hook implementation. Keep hook comparisons separate from skill comparisons.
- Activate both skills and their resources through the linked installation lifecycle and demonstrate actual use in two existing projects outside this checkout, with isolated test state and scoped evidence.
- Close implementation without a two-consumer quantitative speed comparison; keep benefit unproven rather than claiming acceleration.

Selection reconciliation, 2026-09-08: `reduce-subscription-waste` makes hooks-off
the durable default. Existing diagnostic comparisons remain historical evidence;
new skill comparisons pin the same accepted hooks-off selection in both arms.
An isolated candidate comparison cannot reactivate global hooks or retired LSP.

User clarification, 2026-09-08: the delivered capabilities must not depend on
the original source-kit consumer. Ongoing support and additional test spending for that project are
not required. Its completed runs remain historical evidence. No repeatable
acceleration is claimed.

User decision, 2026-09-18: two-consumer quantitative speed comparison is not
required for implementation acceptance. Installation, outside-project
consumption and local controlled pairs may close; benefit remains unproven.
Further model-backed runs for that comparison are out of scope for this change.
Do not lower the historical 15-second / 15% threshold retroactively or relabel
inconclusive or unrun comparisons as a speed win.

This reconciliation does not resume the original source-kit consumer work or restore ordinary hooks. Task 5.3 completed the five local controlled native pairs on hooks-off Astra; they do not prove benefit. Tasks 5.4 and 6.4 close on this explicit scope decision, not on a demonstrated speedup. Fast remains excluded; OpenAI assignments remain Astra-only.

Playwright CLI, a Git snapshot helper, additional domain skills and larger property/simulation frameworks remain conditional follow-ups. Their adoption criteria and research sources are captured in the design; they are not unconditional deliverables. No new model provider, orchestrator, approval layer, global instruction catalogue or autonomous learning loop is introduced.

## Capabilities

### New Capabilities

- `project-verification`: trustworthy project command discovery, freshness, actual entry-point verification and scoped validation evidence.
- `regression-reproduction`: controlled CLI/MCP/subprocess reproduction, reference comparison, reduction and isolated failure fixtures.
- `harness-outcome-evaluation`: comparable native-agent cases, diagnostic latency measurements, honest benefit decisions and economical regression selection.

### Modified Capabilities

- `linked-global-kit`: source-linked delivery, lifecycle and outside-project acceptance of the two verification skills and their resources.

## Impact

Planned sources: `.agents/skills/project-verification/`, `.agents/skills/reproduce-regression/`, narrowly scoped test fixtures and outcome tooling alongside `tests/agent-delegation.py` and `tools/delegation-usage.py`, installer discovery/tests, and documentation. Project-specific records belong to their consuming projects, not the portable global policy. Runtime logs and model traces remain outside tracked reusable sources.

Prerequisite: [stabilize-diagnostic-reconciliation](../../specs/automatic-lsp-diagnostics/spec.md), the current contract for the implemented diagnostic repairs and their conditional activation. Planning this proposal does not authorize implementation, publication, production changes or interference with existing sessions. Quantitative gains from external studies are supporting evidence, not acceptance targets or promises for Astra/Grok.
