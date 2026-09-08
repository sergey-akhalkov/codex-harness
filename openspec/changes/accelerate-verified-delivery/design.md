## Context

The user approved the research direction and requested this OpenSpec proposal on 2026-09-07. This is planning, not a claim that the proposed skills already work or improve outcomes. The target is the globally installed kit used across projects.

Observed integration points:

- The kit already links `.agents/skills/` through the lifecycle defined by [linked-global-kit](../../specs/linked-global-kit/spec.md) and `global/kit.psd1`. Skill resources must follow the same authoritative-source model.
- `tests/agent-delegation.py` and `tools/delegation-usage.py` provide native execution/evidence machinery. Existing [delegation evidence](../../../docs/evidence/agent-delegation.md) includes coordination overhead on small tasks and limits to provider accounting. Reuse the mechanisms; do not copy a claim that delegation always saves time.
- Automatic hooks in this research session repeatedly reported roughly 25 seconds of diagnostic work per tool. This is an observation of a bottleneck, not a controlled before/after measurement. [stabilize-diagnostic-reconciliation](../archive/2026-09-08-stabilize-diagnostic-reconciliation/design.md) already owns the repair and its correctness cases.
- The sibling `opencode-kit` has useful command knowledge states in `docs/adapters.md` and `templates/project/validation.md`, focused script tests, and a read-only Git snapshot helper. Its dirty checkout must be preserved. The snapshot helper needs its own exit-status and UTF-8 byte-limit review before reuse.
- Browser automation is already available through Nuphus. A new browser integration needs evidence of additional value; availability of Playwright alone is insufficient.

### Evidence informing the selection

Sources were inspected during the research on 2026-09-07. Repository `main` links are mutable: pin copied examples to a revision during implementation and preserve any required attribution. These sources motivate mechanisms; none establishes expected Astra/Grok speedup in our projects.

| Primary source | Supported observation | Application and limit |
| --- | --- | --- |
| [SkillsBench v4](https://arxiv.org/html/2602.12670v4) | Curated skills improved aggregate task success in the studied benchmark, but some tasks were harmed. | Try a small relevant set and measure outcomes; do not infer that adding arbitrary skills helps. |
| [SWE Skills](https://arxiv.org/html/2603.15401v1) | The studied repository-skill setup showed a small aggregate accuracy gain with additional token use. | Explicitly evaluate cost and marginal benefit; the tested model and tasks differ from this kit. |
| [Repository context-file study v2](https://arxiv.org/html/2602.11988v2) | Generated repository instructions increased costs without a clear overall success benefit in the studied setting. | Keep stable instructions concise and specialized contracts in their owning skill/project. |
| [Vercel agent evaluation](https://vercel.com/blog/agents-md-outperforms-skills-in-our-agent-evals) | Skills often failed to activate in a specific Next.js evaluation; a compact documentation index helped that setup. | Test discovery and activation separately from task correctness; do not generalize the vendor's scores. |
| [Bun verification skill](https://github.com/oven-sh/bun/blob/main/.claude/skills/verify/SKILL.md) and [review guidance](https://github.com/oven-sh/bun/blob/main/REVIEW.md) | Actual debug binaries, embedded asset freshness, reference behavior and correct subprocess tests matter to verification. | Put executable identity and failure-preserving checks in the workflows. Repository practice is engineering precedent, not causal benchmark evidence. |
| [Ruff/ty reduction skill](https://github.com/astral-sh/ruff/blob/main/.agents/skills/minimizing-ty-ecosystem-changes/SKILL.md) and [constraint-order skill](https://github.com/astral-sh/ruff/blob/main/.agents/skills/wobbling-ty-constraint-order/SKILL.md) | Frozen versions, verified minimization and invariant perturbations make subtle failures tractable. | Adopt controlled reproduction and use perturbation only where the actual defect warrants it. |
| [OpenAI skill evaluation guidance](https://developers.openai.com/blog/eval-skills) and [Anthropic skill creator](https://github.com/anthropics/skills/blob/main/skills/skill-creator/SKILL.md) | Native traces and with/without-skill comparisons separate activation, process, output and efficiency. | Reuse the existing runner for a small paired pilot; do not introduce another evaluation service. |
| [OpenAI harness engineering](https://openai.com/index/harness-engineering/) | Isolated application instances and observable behavior support agent verification. | Use isolated real project state and actual entry points; the article's productivity narrative is not an acceptance target. |

## Goals / Non-Goals

**Goals**

- Shorten time to the first useful validation signal and to the complete verified result.
- Prevent false confidence caused by stale builds, guessed commands, changed failure conditions, hidden timeouts or incomplete evidence.
- Deliver two focused, globally usable skills with demonstrated real consumers.
- Establish six to eight reusable outcome cases and honest local evidence of benefit, including failures and coordination costs.
- Verify diagnostic speed and correctness through the existing diagnostic change, with separate attribution.

**Non-goals**

- Changing models, billing, delegation policy, OpenSpec workflow or the global working philosophy.
- Installing a broad skill catalogue, a replacement orchestrator, mandatory extra review/TDD stages or an autonomous learning loop.
- Building a general benchmark platform, exhaustive repository index, process supervisor, fuzzing framework or universal test adapter.
- Delivering Playwright CLI, a Git snapshot helper or extra domain skills in this change.

## Decisions

### 1. Two small skills, project-owned command knowledge

Create `.agents/skills/project-verification/SKILL.md` and `.agents/skills/reproduce-regression/SKILL.md`. Keep their entry instructions compact; detailed examples and executable resources are loaded on demand. Skill metadata must describe both useful triggers and boundaries so ordinary tasks do not acquire unnecessary process.

`project-verification` first finds the project's existing instructions, relevant manifest/CI entry and validation record. It records only useful command knowledge in the existing project documentation home. If no home exists, a small project-local validation document suffices; there is no mandatory global database or large project template.

Command records carry command, cwd, preparation/runtime/build identity, provenance, knowledge state, last execution and evidence scope. `confirmed` means previously exercised under recorded conditions; it is not proof of the present change. Relevant manifest, lockfile, runtime or generated-input changes invalidate assumptions. A cheap relevant fingerprint is sufficient; hashing every file before every tool would recreate the observed bottleneck. Instructions in a discovered document do not authorize destructive or external effects.

Alternative rejected: always append a long verification checklist to global instructions. It creates irrelevant context and duplicates each project's contract. The local `opencode-kit` state vocabulary is reused as a concept, without importing its entire template system.

### 2. Preserve the failure and own the test resources

`reproduce-regression` uses this sequence: original trigger and contract; pinned failing/reference identities; real entry-point reproduction; independently verified reduction; regression check; scoped outcome report. A missing known-good version is explicit. The contract itself can be an oracle when no independent implementation exists, with correspondingly limited evidence.

Process examples cover concurrent stdout/stderr drains, observed readiness, bounded execution, unique temporary roots/ports, and ownership-aware cleanup. A runner timeout and a natural nonzero exit are different outcomes. Tests must never stop a live shared proxy or terminate processes merely by executable name. On Windows, cleanup verifies resolved absolute ownership paths and process identity before removal or termination.

Prefer the consuming project's runtime and existing harness test helpers. Extract a helper only when actual cases share the need; deliver a small reusable fixture, not an abstract process framework. Keep failure injections inside isolated test state. Perturbation or differential tests are selected by a concrete invariant, not required for every task.

### 3. Reuse native execution; keep comparison state explicit

Build the smallest outcome-case and reporting layer around the native execution patterns in `tests/agent-delegation.py` and the usage interpretation in `tools/delegation-usage.py`. Reuse safe functions where their contracts fit; avoid importing a test script with execution side effects or cloning its orchestration. A minimal extraction is preferable if reuse requires a stable boundary.

Each attempt records case/revision, arm, actual discovered skills, model/effort/provider, configuration and tool identities, hook revision, allowed effects, cache/preparation policy, start/first-useful-signal/end, correctness checks, interventions, retries/child references, available usage and evidence paths. Detailed transcripts stay in machine-local runtime storage. The tracked report contains reproducible inputs, concise results and limits, without credentials or full private traces.

Both arms start from equivalent isolated project state. Baseline does not see candidate skill registrations or validation records produced by candidate runs. Candidate receives the skills through source links in the normal lifecycle, not injected file paths in the prompt. Other existing project instructions and skills stay equivalent. Separate cold discovery from reuse of a previously confirmed record; do not mix cache policies within a comparison.

OpenAI assignments use the Astra family under current policy; the named middle remains the preferred Grok subscription when bounded delegation helps. Availability failures remain visible. No model substitution or paid fallback is part of the experiment. All relevant attempts and children contribute to the accepted-result account, but overlapping durations are not summed as elapsed time. Missing usage is unknown; tokens do not measure exact weekly quota savings.

### 4. Fixed small case catalogue and practical benefit criterion

The eight cases below record the original experiment. On 2026-09-08 the user withdrew ongoing opencode-kit support and further spending on it; case 1 and its completed runs are historical, with no further opencode-kit run required. This overrides the original consumer selection and repetition plan below. Whether to retain the two-consumer quantitative benchmark using another project is awaiting the user's decision; the practical threshold has not been lowered and benefit remains unproven.

Native positive/negative activation is part of the case evidence, but the acceptance oracle checks real outputs and behavior. Supporting deterministic fixture tests do not substitute for native outcome cases. The delivered skills and their resources use this harness without a dependency on the sibling opencode-kit checkout.

| Case | Task and correctness oracle | Evidence purpose |
| --- | --- | --- |
| 1. Native focused command | In isolated `opencode-kit`, discover and exercise its focused library validation entry, preserving documented prerequisites and a known expected result. | Real project discovery and first useful signal. |
| 2. Second project command | In a second real project, discover and exercise its native validation command with an intentionally bounded task. | Portability; a lint-only path supports only lint claims. |
| 3. Freshness | Change a relevant command/build input after a prior confirmed record; correctly invalidate and revalidate. | Avoid stale command knowledge. |
| 4. Actual entry point | A controlled fixture has source and generated/executable state that disagree; the agent identifies and exercises the intended state. | Catch false passing verification. |
| 5. Reference and reduction | Reproduce a real CLI/MCP/subprocess regression in an isolated external project and reduce it without losing its failure; compare a reference where applicable. | Real consumption of the regression skill. |
| 6. Process failure | A case stresses both output streams and a readiness/timeout failure; evidence distinguishes failure causes and cleanup preserves unrelated resources. | Material process correctness. |
| 7. Missing prerequisite | A required tool or reference is unavailable in controlled case state. | Explicit blocked evidence and no invented success. |
| 8. Negative activation | A trivial documentation task with ordinary link/syntax validation. | No automatic expensive workflow or model-evaluation recursion. |

The first consumer candidate is the sibling `opencode-kit`; `npm run test:focused:library` was identified in its package metadata, not executed during planning. Its dirty source is a reason for isolation, not for reset/cleanup. The second candidate is `team-control` through its native lint entry. Its role is deliberately limited to validation-command portability until an actual behavioral check is demonstrated. `time-report-generator` was not selected by the bounded feasibility inspection. Consumer commands remain docs-only until execution confirms them.

At implementation start, freeze exact project snapshots, each case's expected result, environment/budget and meaningful-effect threshold before seeing candidate results. A measured baseline determines a sensible absolute floor and relative threshold; record the rationale and never lower it after seeing results. This is calibration of the experiment, not deferral of scope or permission to drop difficult cases. If a proposed consumer is unavailable, select another existing project satisfying the same acceptance; a synthetic fixture cannot replace the two real consumers.

Run cheap deterministic checks first, then one paired attempt for all cases. Repeat the cases supporting a benefit claim at least twice per arm across both real consumers, alternating order. Report all outcomes and variation. Benefit requires either repeatably lower total verified completion time beyond the predeclared threshold, or improved correct completion/material-defect detection within the same budget, with required checks and safety invariants intact. Any quality loss prevents a speed-only success claim. A small pilot supports these cases, not population-level significance or a promised universal percentage.

If benefit is inconclusive or harmful, retain the results and leave benefit acceptance open. Correct the implementation within scope and rerun affected cases; do not silently remove a workflow, change an oracle or declare completion based on installation alone. A material scope change belongs to the user. Later routine changes select the affected deterministic checks and outcome cases; there is no always-on model benchmark hook.

### 5. Keep diagnostic ownership and attribution separate

The [diagnostic change tasks](../archive/2026-09-08-stabilize-diagnostic-reconciliation/tasks.md) remain authoritative for implementation and native diagnostic acceptance. This proposal adds integrated outcome evidence, not a second scan/cache/receipt implementation.

Before/after hook measurements pin the same project workload, skill configuration, tool calls and actual diagnostic assertions: unchanged tree, single changed file, concurrent parent/child work, findings delivered, resolved findings cleared, and incomplete work reported honestly. Preserve the old executable/source identity before it becomes unavailable. Historic session timings alone cannot establish a controlled speedup.

Skill comparisons run on the same accepted capability selection in both arms: the durable hooks-off default now owned by `reduce-subscription-waste`. Completed stabilized-hook comparisons remain historical evidence, not a restoration prerequisite. Any future candidate hook comparison is isolated and cannot reactivate the global configuration. A report must not attribute combined gains entirely to skills. Applicable consuming-task checks remain required; silence does not establish diagnostic correctness.

### 6. Global delivery is an actual consumer test

Use the existing linked installer discovery and ownership mechanisms. New registration reconciliation, resource resolution, source-body update visibility, paths with spaces/Unicode, foreign collisions, disconnect and reconnect are checked in isolated installation homes before normal global activation. Rollback removes only owned links and restores previous registrations; sources and unrelated user state survive.

Then start fresh ordinary native Codex sessions outside the harness in recorded isolated states of two real repositories. Verify `project-verification` in both and `reproduce-regression` in at least one through an actual task. Do not pass explicit skill source paths in prompts. A discovery listing, fixture-only app, copied skill body or lint pass alone cannot prove full reusable behavior. Live dirty checkouts, production systems and existing sessions remain untouched by acceptance fixtures.

### 7. Conditional follow-ups retain explicit entry criteria

| Candidate | Evidence needed before a separate proposal | Why it is outside this delivery |
| --- | --- | --- |
| [Playwright CLI and skills](https://github.com/microsoft/playwright-cli) | Repeated web tasks where CLI snapshots, traces or CI reproduction improve total verified outcome over the installed browser capability. | Nuphus already provides browser access; no local comparative bottleneck was established. |
| `opencode-kit` Git snapshot helper | Repeated measured Git context-gathering overhead, correct failure handling and actual UTF-8 output bounds, followed by an outcome comparison. | Useful candidate, but directly copying the current helper would import unverified edge cases. |
| More domain skills / a compact versioned docs index | A recurring task cluster, observed knowledge or activation failures, and outcome cases showing a focused intervention helps. | The current evidence does not support a large universal catalogue. |
| Property testing or deterministic simulation infrastructure | Repeated state/interleaving defects that smaller deterministic or differential cases cannot economically expose. | The focused fixtures and regression workflow provide a smaller current path. |

## Risks / Trade-offs

- **Guidance overhead exceeds benefit:** compact entry instructions, negative activation case, explicit total-time comparison, no mandatory additional agent.
- **False success from stale inputs or weaker tests:** identify the executable, preserve the original oracle, include generated-input and missing-prerequisite cases.
- **Damage to another process or dirty checkout:** isolated real source states, explicit resource ownership, bounded cleanup and no live-service control.
- **Noisy model and subscription data:** equivalent arms, repeated benefit cases, per-case variation and unknown usage; no fabricated quota conversion.
- **Contaminated comparisons:** inspect actual discovery, reset case-owned state and separate hook changes from skill treatment.
- **Second consumer is only lint-capable:** scope its evidence honestly; require real process regression consumption elsewhere and do not claim application correctness.
- **Dependency timing:** skills can progress independently, but integrated acceptance remains dependent on diagnostic repair; no competing edits to its files.

## Migration Plan

1. Freeze case inputs, diagnostic baseline identity and acceptance criteria; preserve current unrelated work.
2. Implement the focused skills/resources and deterministic fixture/report checks using existing integration points.
3. Complete isolated linked-lifecycle checks and consume accepted diagnostic-change evidence.
4. Run controlled native paired cases, integrate corrections and publish scoped evidence in repository documentation.
5. Activate the accepted source-linked skills globally and verify fresh outside-project sessions. If activation fails, use the existing owned-link rollback; if benefit remains unproven, keep the acceptance tasks open.

Planning creates no global registrations, runtime changes or model-backed evaluation runs. Implementation tasks in this change remain unchecked until the corresponding work and checks have actually completed.
