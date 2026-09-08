## Context

Implementation outcome and final accounting supersede the preliminary estimates
below; see [the verified selection](../../../../docs/evidence/subscription-efficiency.md)
and [deduplicated usage](../../../../docs/evidence/subscription-usage.md). Historical
planning observations remain here as context, not current acceptance evidence.

See [proposal.md](proposal.md) for motivation and accepted scope. Global configuration and hook definitions are live links into this checkout. CLI 0.153.4 retains previously loaded hook definitions; editing `hooks.json` and `features.hooks` did not stop this running session. The diagnostic command and native MCP paths share an authenticated broker, so a runtime gate is necessary for immediate suspension without restarting users' Codex sessions.

Observed evidence from 211 local rollouts (2026-09-04 through a partial 2026-09-08): this repository's September 7 sessions contained 1,252 automatic LSP messages / 9.75 million characters. Across projects there were 4,077 Astra response usage records that day; most input was cached. This is descriptive evidence, not an A/B comparison or a weekly-quota attribution. Model choice and long work already contributed before hooks appeared. The review also observed an actual Stop continuation after a read-only investigation.

Immediate suspension has already been authorized and applied separately from this planning work. [Current evidence](../../../../docs/evidence/hooks-suspension.md) records the actions and checks. Other agents are modifying this checkout; source identity and existing dirty work must be preserved.

## Goals / Non-Goals

**Goals:** select the smallest useful global capability set, reduce avoidable model/context/workflow cost, preserve the complete consuming-task acceptance criteria, and make strict automatic trigger boundaries executable as negative tests.

**Non-Goals:** rebuilding every LSP, retaining a language matrix at any cost, inventing a new orchestrator, repeating unrelated project tests, changing credentials/providers, Fast mode, lowering required quality, or converting raw tokens into claimed weekly savings. This planning turn does not implement the optimization programme beyond the explicitly requested emergency suspension.

## Decisions

### 1. Disabled is the baseline and the fallback

The user confirmed that development without hooks is the durable architectural
default, not a temporary inconvenience until every hook has a replacement.
Ordinary work uses suitable Serena tools, project-native checks on a completed
change and a concise result. No routine instruction reinjection, broad repeated
diagnostics or Stop-driven rechecking/continuation belongs in that workflow.

A hook exception needs a concrete important gap in existing tool permissions,
process lifecycle or user notification, plus the existing benefit evidence.
Prefer the owning tool's native mechanism. A precise pre-operation constraint,
cleanup of resources owned by the session, or a local completion notification
can be useful examples; none is a requested deliverable or an accepted exception
today. Do not build a hook framework or benchmark speculative hooks to justify
keeping hooks available.

Any accepted exception must be deterministic, narrowly triggered and bounded,
without a model call to dispatch or interpret routine events. Successful checks
are silent to the model; a necessary blocking/failure result gives only the
actionable reason. Cleanup and local notifications do not inject model context
or continue a turn. Silence alone does not establish efficiency: include process
startup, latency, resource use and maintenance in the decision.

The reusable hook file is empty and `[features].hooks = false` is set in the global profile and local base. A host-local `harness/hooks.suspended` marker makes cached command handlers and broker hook requests quiet. Only hook requests are gated; explicitly invoked diagnostics/navigation remain callable pending their own evaluation. A graceful broker retirement applies the gate to its live process; no proxy or Codex process is stopped.

Updating or moving the kit must preserve this selection. Do not restore backup contents automatically at archive/completion. Retaining the existing links with disabled source avoids foreign-file conflicts and lets the installation lifecycle own subsequent changes. The short-lived compatibility marker is a bridge for cached sessions, not another permanent source of truth: remove that bridge only after cached clients no longer need it, while native suspension remains enforced.

### 2. Reuse evidence before spending more

Extend or reuse `tools/delegation-usage.py`, `tools/outcome_report.py`, `tests/outcome-hooks.py` and the existing evidence formats. Add only missing attribution, robust record handling and deduplication; no always-on model-backed analytics. Group by real response identity and timestamp, model/provider, root session/child and project. Separate cached input, new input, output and reasoning (reasoning is included in output, not additional to it). Count automatic report text and continuation events separately; a timeout notice is not proof of another model request. Exclude duplicate/forked history and retain partial-work limitations.

Persist a compact report with source ranges/hashes and tool versions; keep raw logs and identities privately on the host. The initial exploratory scripts are not yet a reusable verified accounting tool.

### 3. Predeclare a bounded benefit comparison

Use the existing outcome harness where it matches current inputs. Compare hooks-off plus the same project-native acceptance checks against one candidate capability at a time in an owned neutral consumer. Include a clean edit, an introduced/corrected defect and representative ordinary work. Measure accepted task completion, detected/missed defects and false positives, total elapsed time including review/rework, model response/usage categories and hook/context volume.

A candidate can be restored only if it completes every fixed acceptance scenario, introduces no additional P0/P1 exposure, demonstrates a useful defect-prevention or rework benefit, and reduces total time to the verified outcome without moving unexplained subscription cost elsewhere. Define task-specific material differences and observed timing variability before running the comparison. Begin with deterministic checks and existing evidence. Where model-backed evidence is essential, allow at most three matched pairs per candidate, stop after two consistent outcomes, and keep an inconclusive candidate off. No huge language-wide benchmark is required to decide that an already costly feature should stay disabled.

Isolated evaluation can enable a candidate in an owned separate CODEX_HOME; it cannot lift the live global suspension. Total accepted-result cost includes parent and children. Metric improvements do not prove an exact percentage of the account's weekly limit.

### 4. Automatic trigger eligibility is a hard filter

For any retained automatic LSP:

`proven create/content change -> supported file -> accepted capability -> new file revision -> one bounded check`

Capture actual mutation evidence from the native operation or bounded before/after observations of its supported targets. A command name, broad matcher, nonempty Git diff or old pending row is not mutation evidence. Preserve creation of untracked files and writes before nonzero exits. Ambiguous concurrent changes cannot be attributed to a read-only operation. Existing detection mechanisms must pass these scenarios before reuse; do not add a watcher or global scan merely to retain the former architecture.

No automatic checks on reads, byte-identical writes, deletion-only operations, startup, Stop/SubagentStop or pending backlog. No whole-language/project cohorts and no treating every JSON change as permission to analyze all source. Backends may need dependency inputs to analyze a changed file, but the adapter must neither schedule nor deliver unchanged dependents. Full-project correctness remains covered by the consuming project's explicit compiler/tests. If a backend cannot satisfy the accepted boundaries economically, retire its automatic path.

Deduplicate per canonical workspace, owning mutation, file content revision and relevant backend inputs. Cancel or discard obsolete responses; never label uncertainty as clean. The proposed initial summary ceiling is 1000 characters including metadata, with full scoped detail available by explicit retrieval. A timeout/unavailable result is reported once with that edit and is not a retry trigger. There is no automatic Stop blocker.

### 5. Keep model/context changes evidence-led

Compare routine Astra `high` with the present `xhigh` only on accepted representative tasks; do not change the default on speculation. Preserve all Astra-only/OpenAI and preferred-Grok constraints. `stabilize-grok-delegation` owns yielded-tool/final-result recovery; consume its evidence and integrate its accepted resolution rather than creating another routing implementation. Inspect partial child work before repeats or reserve use.

Reduce context only where measured waste exists: repeated large reports, whole-document reads when a relevant section suffices, duplicated skill/tool instructions, and redundant setup after verified unchanged context. Preserve applicable mandatory skills, tool identity/freshness rules and correctness checks. Static prompt size and MCP count alone are not savings evidence.

### 6. Reconcile historical contracts without rewriting history

This delta changes unconditional language delivery, configuration/dependency fanout, pending reconciliation, completion hooks and installer restoration. Current-revision and non-destructive isolation guarantees remain applicable to retained checks. `bound-code-tools-resources` must not later reintroduce Stop/journal activity for a retired path; preserve shared-resource safety only for retained services. `accelerate-verified-delivery` provides reusable comparisons, not a requirement to keep the hooks it measured. `adopt-project-memory-and-native-workflows` owns memory/context pilots; reuse accepted results and keep unrelated work intact. Reconcile active artifact conflicts during implementation and final spec synchronization; historical evidence remains labeled historical.

### 7. Minimize all automated model input, not only LSP

The user's follow-up applies to all existing kit-owned automated interfaces. Make a bounded inventory from the installation manifest, registered adapters, hook definitions and agent/automation entry points. For each, inspect one ordinary success/no-op and one failure/change result, identify what the next decision actually needs, and remove duplicate or routine material at the owning producer. Reuse current protocol-specific fields rather than introducing a universal reporting framework.

No-op notifications send nothing. A result summary includes only necessary outcome/target/cause or changed finding, with a full-detail reference when useful. Keep raw reports outside the prompt until requested. Tool descriptions and persistent instructions must not repeat the same detailed policy in every entry; preserve the relevant tool contract and mandatory instruction semantics. Explicit status/search requests still receive enough information to satisfy their actual scope. Use interface-specific output assertions and comparative context measurements; the hook's 1000-character ceiling is not a universal schema constraint or an invitation to fill it.

### 8. Serena as the shared diagnostic provider: requested alternative

The user's confirmed architectural preference is to simplify around Serena when
its existing capabilities cover accepted needs. Begin with existing Serena tools
and explicit project-native checks while hooks remain off. This is the preferred
implementation direction, not a decision to restore automatic diagnostics.
Evaluate a ready-made inline diagnostic path only if it adds demonstrated value.
A separate provider or custom adapter requires a concrete important gap that
supported configuration and native checks cannot meet economically; do not build
parallel prototypes or benchmark every theoretical option first. Remove verified
overlap instead of maintaining two stacks by default.

Installed-source inspection on 2026-09-08 found `serena-agent` 1.7.0 with an exposed
`get_diagnostics_for_file(relative_path, start_line, end_line, min_severity,
max_answer_chars)` MCP tool, also listed in the
[official tool catalogue](https://oraios.github.io/serena/01-about/035_tools.html).
The host selects `language_backend: LSP`; this checkout's declared Serena language
selection is currently only `python`. The intended reduction is therefore one
owner of language-server processes and state, not the elimination of LSP inside
Serena. The separate harness `tools/lsp/server.py`/`backend.py` owns its own backend
pool and stdio servers. Duplicate live process counts have not been measured.

The inspected installed implementation adds four acceptance questions:

- `serena/tools/symbol_tools.py:GetDiagnosticsForFileTool.apply` synchronizes the
  project's filesystem changes before the file request and groups every finding
  by its owning symbol. `serena/ls_manager.py:LanguageServerFileChangeNotifier`
  polls tracked source files and can notify all project servers about changes
  outside the originating operation. A single file argument does not establish
  the strict automatic-work boundary or low latency.
- `solidlsp/ls.py:request_text_document_diagnostics` may fall back to cached
  published diagnostics and returns an empty list when no result was obtained.
  Verify current content revision, stale-error rejection and authoritative
  clearance; do not interpret every empty tool object as a successful check.
- `serena/tools/tools_base.py:_limit_length` can replace an oversized result with
  an answer-too-long notice. `max_answer_chars` alone does not implement a useful
  summary, deduplication, silent no-op delivery or on-demand full detail.
- Actual language/backend support, first-use cost and concurrent project identity
  need acceptance on retained scopes. The current connected Serena client failed
  `initial_instructions` with `Serena proxy source/runtime changed; restart this
  client`. This is an observed client lifecycle limitation, not evidence that its
  diagnostics API is absent. A successful live diagnostic call is still pending.

These are observations from installed code; the upstream
[tool implementation](https://github.com/oraios/serena/blob/main/src/serena/tools/symbol_tools.py)
and [LSP diagnostic implementation](https://github.com/oraios/serena/blob/main/src/solidlsp/ls.py)
provide reference contracts, not a substitute for pinned-version acceptance.

If a ready-made automatic path is accepted, its required behavior is `proven
mutation -> supported changed file -> existing project-scoped Serena service ->
minimal current finding`. This describes acceptance, not a mandate to implement
a new pipeline. Reuse the current owned transport and compatible worker without
a model round trip merely to dispatch automatic checks, a fresh Serena per edit,
or global project activation that interferes with other clients. The current
`tools/code-tools/serena_broker.py` is the existing service boundary to verify.

If the ready-made path cannot meet mutation isolation or truthful freshness,
retain explicit use where appropriate and leave automatic diagnostics off.
Do not introduce a lower-level integration, vendor patch, second Serena fork or
new universal wrapper merely to preserve automation. Do not bypass version or
ownership guards or weaken explicit-tool freshness. Retire overlapping harness
providers after checking their real retained consumers and global lifecycle.

### 9. Serena as the primary editor: preferred suitable workflow

The user prefers Serena as the primary environment where it solves the task,
including suitable symbol/text edits and batched replacements. A 99% share remains
a hypothesis about useful coverage,
not an accepted quota or a reason to route unsuitable work through more tools.
Measure required navigation/setup, mutation, diagnostics, review/rework and model
context together. Existing long sessions, model effort and repeated instruction
loading remain separate possible usage causes.

The installed 1.7.0 editing tools inherit
`serena/tools/tools_base.py:EditingToolWithDiagnostics`. It can attach a diagnostic
delta to the edit result, but its class-level `ENABLE_DIAGNOSTICS` is `False`.
The source comment explains that individual edits can intentionally introduce
temporary errors resolved by subsequent edits. This is not verified production
diagnostics and must not be enabled by flipping an internal flag globally. Its
`DiagnosticsDiff` path also needs freshness, unavailable-status, deduplication and
output-bound acceptance before reuse. Consider one diagnostic result for a
completed supported edit batch rather than interruption after every intermediate
step; the batch must still prove actual changed files and cannot defer work to a
later Stop/read hook.

The installed `codex` Serena context excludes `create_text_file`, `replace_content`,
`read_file` and shell operations. The connected tool catalogue exposes symbolic
edits and `replace_in_files`, but not file creation. Any expanded editing surface
needs deliberate supported context/tool selection and owned creation/modification
checks. Shell commands, formatters, generators, package managers and recovery
operations retain their appropriate native paths. Do not reimplement them in
Serena merely to raise its usage percentage.

Compare explicit diagnostics after a coherent edit batch with any accepted inline
edit-result diagnostics. The inline option may keep all native hooks off. Record
which languages and editing entry points it covers; non-selected write routes
remain covered by applicable explicit project checks, without pretending they
received automatic LSP. Preserve fallback on actual Serena/runtime unavailability
and verify no partially applied edit is repeated during recovery. A single provider
reduces duplicate ownership but also makes its availability more consequential.

## Risks / Trade-offs

- **Missed dependent error after removing automatic cohorts** -> preserve explicit project-native acceptance checks; document reduced automatic scope honestly.
- **Cached processes keep old hooks** -> native configuration off plus verified command/broker gate; distinguish old invocations from actual suppressed work.
- **Small or noisy performance samples** -> fixed oracles, matched inputs, recorded timing variability, bounded attempts and off on inconclusive evidence.
- **Overcomplicated optimization machinery** -> reuse existing reporters/checks and stop when sufficient evidence exists; no model-backed monitoring daemon.
- **Concurrent edits or shared packages** -> scoped changes, current identities, graceful owned retirement and registration-only removal for shared installations.

## Migration Plan

1. Preserve the applied suspension and its rollback information; verify native and cached consumers outside the checkout.
2. Establish trustworthy existing-log attribution and reuse comparable outcome evidence.
3. Evaluate candidates while global hooks remain off; reject or retire non-beneficial capabilities.
4. Implement only selected optimizations and, if justified, the strictly scoped retained automatic path; run its complete positive and negative contract.
5. Deliver the accepted selection through install/update/repair/recovery and verify unrelated-project consumers. Reconcile owning docs and active deltas before synchronization/archive.
6. Roll back a failing candidate to hooks-off with retained user data and project-native checks. Restoring the original noisy hook file is not the default recovery action.
