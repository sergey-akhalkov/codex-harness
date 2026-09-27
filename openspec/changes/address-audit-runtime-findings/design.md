## Context

See [proposal.md](proposal.md) for scope. This assessment compares the supplied external audit of revision `24b68d4f30b1808122bf6eea0fbb4b3a6109c44c` with revision `48a9e44` on 2026-09-27. The starting tree was clean and had no active OpenSpec changes. The audit itself and local runtime data are not copied into the public source.

Evidence is current source inspection, semantic navigation and reference results, existing requirements/tests, installed CLI help, and the primary sources linked below. Rust navigation worked for both `tools/` and `crates/`. The installed RTK reports 0.48.0 and supports `pipe --filter`; safe handling by additional individual filters must be established with the command corpus before implementation relies on them. No compiler suite, concurrent live-worker experiment, paid model comparison or private rollout scan was run during exploration. Existing tests show intended coverage, not a fresh passing execution. Working-tree hygiene is separate from a Git-history audit.

The ordinary scenarios are already established by the kit: an agent runs Cargo checks under the installed resource wrapper; several sessions use different project roots while sharing bounded tooling; a lead reads the existing feedback ledger. Improving these paths requires neither additional agents nor a new user ritual.

## Goals / Non-Goals

**Goals:** make compact verification reachable from documented commands, remove unrelated-project serialization without weakening ownership, and prevent a misleading benefit status. Separate observed facts, source-derived consequences and hypotheses about savings.

**Non-Goals:** tune account resource limits; reduce executor concurrency; rewrite mandatory instructions or upstream skills; cache successful verification; rewrite active conversation history; train skills; add graph/index services; change model routing; promise a quota percentage. These exclusions reject unsupported implementation scope, not every underlying research idea.

## Decisions

### 1. RTK coverage: accept, with a smaller integration than a new runner

In [the adapter](../../../tools/rtk-adapter/src/main.rs), `filter_for()` accepts only a small Cargo-test allowlist. `--locked`, `--jobs`, `-p`, the test-argument separator and `--test-threads=1` fail selection; `check`, `build` and `clippy` are not selected. `run()` inherits stdout when no filter is selected and always inherits stderr. Consequently the native recipes in [rust-native.md](../../../docs/rust-native.md) and the package-scoped checks in [cargo-fast](../../../.agents/skills/cargo-fast/SKILL.md) bypass this compression. This is a source-level conclusion; the daily fraction of affected output is unmeasured.

The audit correctly rejects arbitrary PowerShell interception. A hook rewrite alone also does not prove compression: `hook()` rewrites the explicit adapter entry while `run()` makes the later argv-based selection. Preserve those separate decisions.

Use the existing adapter inside the existing resource owner:

```powershell
codex-harness heavy -- harness-rtk.exe compact cargo test --workspace --locked --jobs 1 -- --test-threads=1
```

This is the proposed documented composition, to be verified through the installed entry point. `heavy_command_cli::run` already forwards the selected native program and argument tail to its owned process tree. There is no reason to unwrap arbitrary nested commands or give `heavy` another output parser. Preserve its resource diagnostics, startup failures, timeout and cancellation semantics. Its own stderr remains distinct from Cargo diagnostics.

Extend structured argv recognition and reuse verified RTK filters. Human-output `test`, `check`, `build` and `clippy`, package/workspace selection, locked dependencies, jobs and the documented downstream test/lint flags form the initial corpus. Machine formats, exact reads, explicit disablement and interactive output keep raw behavior. `git diff` is deliberately excluded: the current skill requires an exact final review diff, and dropping the only changed condition could hide a defect. Lack of diff compression is therefore not automatically a bug.

For the selected Cargo path, drain stdout and stderr concurrently, preserve stream identity, and retain recoverable originals before emitting a compact result. Only compress recognized output; preserve unknown diagnostic blocks verbatim. A parser cannot infer success from missing test lines, and a failed compile with no test summary must remain a failure. Do not invent counts for build/check/clippy. If a pinned filter does not safely handle a form, use a narrowly tested Rust presentation in this adapter, not another runtime dependency. Keep raw fallback for unrecognized formats, filter failure and exceeded capture bounds. Preserve the child exit code and never rerun Cargo for presentation or recall.

Reuse the existing local raw/pack retention owner, extending stream metadata only as needed. Report `applied` or `bypassed`, the concrete reason and measured raw/visible bytes through a bounded adapter result or existing local evidence. Do not insert a footer into raw machine/bypass streams. An opt-in local diagnostic mode can expose otherwise silent bypass reasons. Byte counts are not measured tokens or allowance.

**Counterexample to universal benefit:** a warm `cargo check` printing a few lines is cheaper to leave raw; footer overhead can exceed compression. Long builds also need bounded progress visibility rather than minutes of silence. Neither situation justifies rejecting the command or losing diagnostics.

### 2. Serena: accept concurrency; preserve capacity and resource policy

[Backend::call](../../../crates/harness-core/src/serena_broker.rs) holds `self.pool.lock()` across `pool.rpc()`. [Pool::rpc](../../../crates/harness-core/src/serena_shared.rs) performs synchronous `worker.request()`; `worker_for()` also starts and closes workers under the same pool access. The [broker service](../../../crates/harness-core/src/broker_service.rs) already dispatches separate connection threads, so the pool lock is a real cross-project serialization point. A source diff from the audit revision changes the broker startup deadline, not this locking structure.

Use short pool-table coordination to reserve a worker generation, a per-worker serialized request path, and separate serialization for each client's mutable routing transitions. Execute worker startup, RPC and potentially slow cleanup outside the table lock. A generation/reservation remains counted against the cap until retirement has actually finished. Concurrent requests for one compatible key must share one startup, and callers must not route through a half-started worker. Preserve canonical project/configuration identity, activation/removal semantics, response IDs and the existing narrowly scoped fatal-language-server recovery. Do not expand retries to arbitrary writes whose outcome is unknown.

Idle eviction is allowed; in-flight or reserved work is not an eviction candidate. When all slots are busy, wait only within the incoming deadline, honor cancellation, and report capacity exhaustion without launching the request later. Warm requests to another worker must continue during a slow startup or retirement where resources permit. Maintain broker/process-tree ownership and bounded shutdown. Worker count remains at most the configured cap, including starting/retiring generations.

The settings in [orchestration.toml](../../../global/orchestration.toml) and [tool-resources.json](../../../global/tool-resources.json) are indeed four executor slots versus three Serena workers. With distinct keys, `A -> B -> C -> D -> A` causes LRU eviction and a new A startup in the present algorithm. But executors are not workers: agents may share roots, not use Serena simultaneously, or perform shell work. Today the global mutex prevents simultaneous eviction of an executing request; that race becomes a concern of the proposed concurrent design, not a demonstrated current race.

Do not automatically raise `max_projects` to five or couple executor dispatch to Serena admission. The current [resource contract](../../../docs/code-tools.md) permits a 4 GiB Job per worker; three to five increases the permitted aggregate worker envelope from 12 to 20 GiB, not necessarily actual working-set usage. Blocking whole tasks on IDE capacity also restricts useful independent work. Both require workload/resource evidence and, if ordinary operating conditions change, a separate user decision.

Expose hit/start/eviction/failure counts, active/reserved/idle worker counts and queue/start/RPC durations through the existing bounded local status owner. Keep restart/reset identity explicit. Record actual memory where existing process evidence provides it; otherwise mark it unavailable. No telemetry service or periodic model report is needed. These observations can later justify capacity tuning without making every caller perform diagnostics.

**Counterexample to universal benefit:** requests to one shared worker still serialize, and four alternating roots still outgrow a three-worker warm cache. The accepted improvement is independence across available workers, not unlimited parallelism or elimination of all cold starts.

### 3. Role-specific instructions: valid experiment, no policy rewrite yet

The historical 19–28k startup-token observation is in [token-workflow.md](../../../docs/token-workflow.md); it is not a current measurement of this session or one file. Lead procedures occur in the shared developer instructions, principles and selected skills. Role-specific presentation could remove redundant reads, but merely moving paragraphs into references can increase total reading and calls.

The [AGENTS.md study](https://arxiv.org/html/2602.11988v1) reports average LLM-generated-context cost increases of 20% and 23% in its two settings and small average success declines; developer-written context has different results. These are evaluated benchmark populations, not proof that this kit's authority, publication or Windows recovery rules are waste. Mandatory checks induced by instructions can be useful despite increasing cost.

Revisit with one bounded role-only candidate, measured effective instructions plus loaded references/briefs, unchanged mandatory constraints and matched outcomes. The existing [skill evaluation contract](../../specs/skill-evaluation/spec.md) already supplies comparison rules where a skill changes. No new prompt compiler is justified.

**Counterexample:** hiding cleanup rules from an executor because they appear in a lead section can leave shared processes running after its failed task. Shorter input with more recovery work is a loss. Safe role extraction needs semantic review and representative failure scenarios, not a word-count target.

### 4. Action Fusion: retain the native mechanism; require a repeated missing case

The current [Code Mode requirement](../../specs/token-efficient-agent-workflow/spec.md) and installed skill already cover independent batches, deterministic dependent sequences, error preservation and bounded results. There is no demonstrated missing native capability requiring a new engine. A known edit, formatting step and named test can be composed now; choosing a fix after a failed test cannot be predetermined safely.

[SoL-Pi's author report](https://nvlabs.github.io/SoL-Pi/) confirms 149 candidate transitions, projected reductions of 10.8% of model turns and 11.5% of tokens, and explicitly identifies that result as a trajectory counterfactual. [Anthropic's example](https://www.anthropic.com/engineering/code-execution-with-mcp) reduces 150,000 tokens to 2,000 through tool discovery and processing; this combines mechanisms and is not a universal edit/test speedup.

Use existing Code Mode for demonstrated repeated sequences. Revisit an owning-tool improvement only when an actual sequence repeatedly needs avoidable model turns and its next actions are known in advance. Preserve partial effects and stop dependent work after failure.

**Counterexample:** blindly running an expensive integration test after a failed symbol edit wastes time and can test the wrong state. A batch returning only `ok` can also conceal the failed prerequisite. Fewer calls alone do not establish improvement.

### 5. Verification reuse and handoff: good evidence discipline, unsafe general cache

[verification_record.rs](../../../crates/codex-harness/src/verification_record.rs) explicitly describes one run and declared inputs only; the [owning requirement](../../specs/project-verification/spec.md) forbids automatically reusing a previous pass. This is an intentional safety boundary, not an unfinished cache. The observer does not establish hermetic transitive inputs or all external state.

[Bazel's cache](https://bazel.build/remote/caching) associates results with declared action inputs, commands and environment. Borrowing that identity model is reasonable, but a file receipt is not equivalent to Bazel's action graph. Adding hashes for a few files cannot safely bridge the gap.

**Counterexample:** the same Git HEAD and test executable can produce different results after a DLL, compiler, environment variable, generated fixture, installed MCP registration or remote service changes. A pass for two independent branches also does not prove their merged state. Preserve previous receipts as evidence with their scope; rerun affected acceptance through its real entry point.

The useful handoff portion already fits current briefs: changed files, actual command/result identity, coverage and unresolved limits. Reuse that evidence in review when inputs and conditions are demonstrably unchanged, without manufacturing a new execution. A separately bounded pure check could eventually justify an explicit cache contract, but none is identified here. No generic pass cache is accepted.

[Aider's repository map](https://aider.chat/docs/repomap.html) ranks useful repository information within a token budget. For this kit a few Serena-retrieved signatures may help a concrete brief; a mandatory generated map for every assignment can duplicate cheap exact navigation. Revisit only after repeated rediscovery is observed, without reintroducing a graph service.

### 6. Lead, executor and cache economics: sound separation, partly stale diagnosis

The source still selects `executor_profiles = ["ds"]`, while current shared defaults describe direct ordinary-agent choices separately. A profile label is not proof of the actual model or billing on a particular installation; use the resolved dispatch receipt. Lower non-OpenAI usage does not by itself establish lower OpenAI allowance. Mechanical status/ledger work should remain native, as the current workflow already requires.

The audit's DeepSeek-only cache-guard description is stale. The current [guard guide](../../../docs/agent-delegation.md#provider-neutral-cache-loss-protection-and-recovery) describes provider-neutral runtime-proven support, exact-session recovery and warnings. This exploration reviewed the source change scope and documented contract; it did not rerun guard acceptance. Do not create a second guard or claim that the existing one fixes prefix churn.

[OpenAI's current caching documentation](https://developers.openai.com/api/docs/guides/prompt-caching) supports stable rendered prefixes and explains effects of tools, instructions and compaction. Effort changes can affect the prefix, but supported API configuration updates can preserve it; this is conditional, not a universal rule. An API feature is not automatically an installed Codex control. The local skill already distinguishes new CLI invocation effort from a running turn. Investigate a demonstrated cache-loss episode through the existing owner before adding prefix instrumentation.

[Codex usage documentation](https://learn.chatgpt.com/docs/pricing) says model, task complexity, context, reasoning, tools and caching affect usage; it supplies no universal tokens-to-weekly-percentage conversion. Reduced prompt bytes, cache ratios and API prices remain different measurements.

**Counterexample:** changing tools or restarting an otherwise reusable session to save a short instruction block can destroy a much longer cacheable prefix. Likewise, replacing the configured executor model can violate routing authority even if an estimate looks cheaper. No routing change is accepted here.

### 7. Observation lifetime: keep bounded recall; no speculative context service

Current RTK retains packed observations with a 64-entry/128 MiB cap and bounded digest-checked recall; raw captures have their own 32-entry/4 MiB-per-capture limits. Unknown/evicted handles fail explicitly. Preventing repeated retrieval before it reaches the model is useful and already supported. Retaining a file does not remove an old tool message from Codex's active context.

[The Complexity Trap](https://arxiv.org/abs/2508.21433) reports roughly half the cost of raw histories in its SWE-agent/SWE-bench Verified configurations, with masking competitive with model summarization. The comparison does not establish another halving for a Codex installation already using native context management and RTK.

**Counterexample:** pinning every still-mentioned handle in several long tasks eventually defeats a bounded cache, while blindly masking the only failure diagnostic makes recovery harder. A future pinning mechanism would need an actual eviction incident, explicit ownership/release and a byte cap even when every item is pinned. No such frequency or failure evidence was supplied or measured. Keep current raw recovery and explicit expiry; do not rewrite rollout files or add another summarizer.

### 8. Learned skills and benefit evidence: separate an existing evaluation rule from a reader defect

[The gskill author experiment](https://gepa-ai.github.io/gepa/blog/2026/02/18/automatically-learning-skills-for-coding-agents/) reports Mini-SWE-Agent test success improving from 55% to 82% for Jinja and 24% to 93% for Bleve, with separate training/validation/test data. It motivates targeted experiments, not a transferable guarantee for Rust/Windows or permission for a costly learning campaign. GEPA is not added as a dependency.

[The existing calibration record](../../../docs/memory/skill-evolution.md) confirms that compared arms read a live package rather than their isolated candidate, and correctly records an inconclusive result. The [current evaluation spec](../../specs/skill-evaluation/spec.md) already requires attributable package revisions and isolation. Before any future experiment, verify actual body selection, alternative live paths and frozen evaluation inputs. Adding the same requirement again would not fix a violated experimental setup. No learning benefit has been established by those inconclusive trials.

A distinct concrete defect is in [benefit_gate.rs](../../../crates/harness-core/src/benefit_gate.rs): `default_allowed()` checks only `outcome == "adopt"`, despite parsing quality. The synthetic record below is accepted by that predicate:

```text
benefit-gate v1 item=demo outcome=adopt quality=regressed
```

Exact references show one non-test consumer in [feedback_cli.rs](../../../crates/codex-harness/src/feedback_cli.rs): ledger presentation. This is a misleading adoption label, not evidence of an autonomous deployment or changed default. Existing tests cover latest valid accept/reject/inconclusive records but not this contradiction.

Keep the native board comments and ledger as owner. Parse comparison fields already documented by [board-workflow](../../../.agents/skills/board-workflow/SKILL.md), retain the raw recorded decision, and present consistency/coverage separately. Adoption requires unchanged/improved quality, a positive matched count, valid finite timing/tolerance values, declared arms/accounting and no arithmetic contradiction or unexplained regression beyond tolerance. Incomplete legacy records remain visible as unproven; never fabricate missing observations. A newer contradictory or malformed record attributable to an item must not expose an older adoption as if it were still the latest valid decision.

Do not resurrect a large evaluator or add a second comparison database. These checks validate record consistency, not independent truth of a benchmark. Preserve current comment/detail locators for the actual comparison evidence and do not describe a reference or self-reported number as verified execution. Explicit user acceptance of a trade-off can remain a recorded decision; it does not turn regressed quality into proof of improvement.

### 9. Measurement and priorities: accept the method, avoid a new analytics project

[token-audit](../../../docs/memory/token-audit.md) already has bounded reports, retained detail, measured/inferred bases, baselines and a shared rollout reader; the [delegation specification](../../specs/agent-delegation/spec.md) already requires total accepted-task accounting and avoids counting cumulative usage twice. Use those owners for any subsequent economic evaluation.

A task-to-session join could be useful when existing task/receipt identity permits reliable attribution. No missing measured join or dataset was established in this review. Do not infer ownership from similar titles, assign all concurrent account usage to one task, or add reasoning tokens twice when included in output. Add a join only against a concrete unsupported case, with explicit unknown coverage.

The audit's proposed ordering is broadly sound: model-free behavior checks first, then a small matched real-task pilot if making an economic claim, then more costly learning/context experiments. Its P0 labels indicate investment priority, not demonstrated emergency severity. RTK directly affects output bytes; Serena primarily affects overlapping latency; the benefit reader affects trust in status. None yields a measured weekly quota saving here.

[SoL-Pi's paper](https://arxiv.org/html/2609.20519v1) confirms the efficiency configuration's approximately 45–49% lower token traffic with about 94% of average score, and 15 versus 18 solved Terminal-Bench tasks. This supports preserving quality acceptance. [Google's controlled multi-agent study](https://research.google/blog/towards-a-science-of-scaling-agent-systems-when-and-why-agent-systems-work/) covers 180 configurations and supports matching coordination to task structure; its results are not a coding-harness worker-count formula. Retaining the existing rule against manufactured parallel work needs no new policy.

## Risks / Trade-offs

- **Diagnostic loss or pipe deadlock:** drain both Cargo streams concurrently, keep bounded byte-exact originals, preserve unknown blocks and test large simultaneous output, broken filters and nonzero exits. A compact summary never establishes success by itself.
- **Added output/progress cost:** skip non-shrinking results, preserve bounded progress and make routine bypass telemetry available on demand rather than bloating every response.
- **Worker races and deadlocks:** establish a lock order, reserve startup/retirement capacity, serialize per-client route mutation, and test synchronized concurrency/cancellation. Do not hold a table lock during child RPC or cleanup.
- **Resource multiplication:** retain the configured cap and existing Jobs. Measure actual usage before proposing new limits; warm-cache churn can remain under a deliberate cap.
- **Misleading validation labels:** distinguish record consistency from independently executed evidence, retain newer invalid records and never silently restore an older adoption.
- **Overgeneralized benchmark claims:** use the sources only for their evaluated scope. Real workload benefit and subscription attribution remain unmeasured.

## Migration Plan

Implement and test RTK, Serena and ledger slices in their existing owners. Their writes are separable except for final global delivery and documentation integration. Keep model-free fixtures in existing test targets; do not add production benchmark infrastructure.

For RTK, start with a small synthetic Rust project's passing and failing Cargo test through the real `heavy -> adapter -> cargo` route, then cover check/build/clippy and raw recovery. For Serena, use barriers to prove two independent workers can both enter before either is released, plus same-worker ordering and safe admission at capacity; then exercise two actual installed consumer roots with distinct symbols. For the ledger, test contradictory, incomplete, malformed-latest, adopted and withdrawn comments through the existing CLI fixtures.

After native checks, update through the supported kit lifecycle and verify from owned roots outside this checkout. Preserve unrelated active consumers; use the lifecycle's existing recovery/rollback if an update cannot safely activate. Include current source/build identities, status reset semantics and tested limitations in existing evidence owners. No live installation is changed by this planning task, and implementation tasks stay open.
