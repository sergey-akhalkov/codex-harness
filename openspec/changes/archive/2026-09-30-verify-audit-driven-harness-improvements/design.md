## Context

See [proposal.md](proposal.md) for motivation and [tasks.md](tasks.md) for execution. This document is the corrected analysis of the supplied audit and the decision record for the resulting change. It deliberately keeps the original twelve sections and their subsidiary recommendations traceable instead of treating the audit as twelve indivisible tickets.

The supplied audit and the inspected checkout both identify public revision `4b2f958b88d6667b9eff71c25254cf42834ecdb7`. Source observations below were rechecked on 2026-09-29. The planning investigation inspected the relevant implementations and existing acceptance contracts; it did not run their regression suites, reproduce destructive races, install a candidate or spend model-backed evaluation calls. Static counterexamples establish plausible or direct control-flow failures, not completed Windows acceptance. A read-only source/link check passed on the original tree; it establishes source hygiene only.

The delivered product remains the Windows Codex entry point and installed reusable kit. Ordinary use includes repeated launches from unrelated repositories, several sessions sharing one local state root, long-running model requests, concurrent tool output, interrupted updates, and reports spanning old and active sessions. Existing decisions require live shared data, local private state, native launch availability, exact model/provider authority and outside-checkout delivery. These are constraints on the solution, not variables to relax to improve a benchmark.

The inspected consumer reports Codex CLI 0.157.1 and OpenSpec 1.12.0. The workspace uses Rust edition 2024, MSRV 1.98 and Windows/MSVC native build acceptance. Runtime contracts must be qualified again against the actual implementation consumer; current web documentation is not a substitute for its installed behavior. No new language runtime or library is necessary merely to define these fixes.

Evidence labels used below:

| Label | Meaning | Permitted conclusion |
| --- | --- | --- |
| C | Current code or specification inspected | The stated branch, data loss or missing guard exists in the inspected revision; execution still needs its check |
| O | Operation observed during this investigation | The reported operation happened in this environment; generality and root cause can remain unknown |
| H | Existing historical evidence inspected | The old result has its recorded limits and is not fresh acceptance for changed inputs |
| P | Opened primary external source | The foreign mechanism exists as documented; its measured benefit is not transferred to this product |
| I | Investigation or optimization hypothesis | No adoption or quantitative benefit is established yet |

## Goals / Non-Goals

The design aims for complete coverage of the enumerated audit, small independently useful corrections, and an evidence path capable of rejecting a bad optimization. A correctness repair can improve quality without saving tokens; that distinction is intentional. An optimization can improve a scoped mechanical property without establishing cheaper accepted tasks. Both must state what was actually proved.

The design does not promise that every future workload becomes faster or cheaper, that a finite test suite finds every defect, or that every optimization candidate will be adopted. Those guarantees cannot be established by this audit. It also does not introduce a new agent framework, a second event store, automatic model-backed maintenance, a provider migration, a generic transaction framework or a rewrite of externally maintained OpenSpec instructions.

Complete coverage means every row in the matrix has a requirement and a task. Completed implementation means every mandatory row has its required current evidence and integration. Optional investigations can finish with rejection or an explicit inconclusive verdict, but neither verdict is an improvement and neither closes a mandatory correction. No required feature below is made optional by labeling its implementation difficult.

## Decisions

### 1. Repair RTK's publication boundary before optimizing its storage

**Finding and evidence.** In [RTK main](../../../../crates/harness-rtk/src/main.rs), `pack_store` creates an observation before reading the shared index, `retain` deletes log files absent from that in-memory index, and `write_pack_index` uses a common `index.json.tmp`. The following is a real static counterexample: A creates its file; B creates its file; A reads the old index and adds A; A retention removes B's unindexed file; B later publishes metadata for its now-missing file. Independent unique observation names do not serialize the store transaction. A second race can lose one writer's index update or conflict on the temporary index. The audit is correct about the missing transaction boundary (C).

**Additional defect.** `read_pack_index` does not validate its schema and drops malformed records, while `PackEntry::from_value` accepts any string as a handle. Both recall and eviction construct paths from that value. A crafted or corrupt record containing a parent-relative handle can direct an eviction outside the intended pack directory. The relevant claim is that this data is not validated before a destructive path is constructed; no exploitation or actual deletion was observed. Dropping damaged metadata before orphan cleanup can also turn corruption into loss of recoverable evidence (C).

**Chosen boundary.** Preserve the existing file store and retention contract. Use one short store transaction with process-death-safe ownership, isolated staging and a defined commit point. The lock covers publication and retention, not Cargo execution, filtering or a session lifetime. Readers must acquire a consistent committed observation or explicitly report its legitimate expiry. Validate the entire destructive decision before cleanup; unknown schema, invalid handle grammar, digest or size arithmetic prevents destructive retention. Preserve old committed evidence on failure. Do not solve this by lengthening retention, serializing all tools, silently rerunning a command or replacing the store with a database.

The standard library and existing Windows ownership primitives are the first implementation candidates. The existing installation lock proves reusable locking experience but its user-home-wide scope must not be imposed on every RTK invocation. Reuse a primitive only if its lifetime, alias resolution, abandonment and thread constraints fit this store. Windows rename behavior and the interruption boundary require real tests; a rename API call alone does not prove durable transactional publication. Power-loss durability beyond the documented filesystem contract is not claimed without corresponding evidence.

**Acceptance.** Use two real processes and explicit barriers at allocation, read, retention and publication. Below the limits, both successful handles recall byte-identical digest-checked content. Exercise termination before/after commit, a concurrent reader, retention at its normal limit, write/lock failures and a malformed-index outside sentinel. Command execution stays exactly once and failure still exposes usable original output. Existing raw-file retention and pack retention have different windows; combining them is not part of this repair.

### 2. Decide RTK presentation before publishing it

**Finding and evidence.** Both `present_stream` and the non-Cargo `filter` accept a shorter body before adding raw paths and optional pack handles. `run_cargo` adds both stream footers and a failure notice later. Checking only `result.len() < raw.len()` permits a longer delivered result (C). The defect is broader than fixing only `present_stream`. Existing Cargo requirements already say a full non-shrinking presentation should remain raw; this is an implementation gap, not a reason to weaken that requirement.

**Chosen boundary.** Prepare the complete candidate, determine the evidence it can truthfully advertise, account for already emitted progress and final notices, select the representation, then publish it. Keep source stdout and stderr identities and report the same accounting basis used for selection. A local prepared value is appropriate; a general presentation framework is not needed. If evidence publication fails, recompute the available candidate or deliver raw output, retaining the failure and child status. An operation must never be re-executed just to regenerate a cleaner presentation.

**Correction to the audit's guarantee.** A required error or progress message can make a raw fallback larger than the child's raw output. Therefore the strict smaller-than-original guarantee applies to invocations labeled `applied`, whose full model-visible byte ledger must justify it. Raw/fault/bypass paths preserve truthful messages and do not promise universal byte monotonicity. This avoids satisfying a size metric by suppressing a failure. Wrapper progress and notices are measured explicitly; they are not hidden in an uncounted channel. Bytes still do not prove token or quota reduction.

`cargo_status_line` currently checks an indentation range and a word, not the asserted status-field width. The minimal correction checks the exact recognized layout and preserves negative examples. Even perfect alignment is syntactic evidence only: user text can imitate Cargo. Original recovery and explicit raw operation remain necessary, so the specification cannot honestly call the filter universally semantically lossless.

**Acceptance.** Exercise nearly incompressible output above the 500-byte threshold, long Windows paths, both streams, diagnostics on stderr only, nonzero exit, progress already emitted, binary/oversize/machine-output bypass, filter failures and all supported non-Cargo compact routes. Compare bytes actually emitted through the real adapter, not only the filter's returned vector. Preserve unknown lines and the child result.

### 3. Resolve native preference precedence before injecting defaults

**Finding and evidence.** [Portable configuration](../../../../crates/harness-core/src/portable_config.rs) avoids replacing a local `model_reasoning_effort`, but [native launcher](../../../../crates/harness-core/src/native_launcher.rs) passes only the effective default model to [launcher](../../../../crates/harness-core/src/launcher.rs) `per_model_effort`. That function considers arguments and its mapping, then can inject an effort with stronger CLI precedence. The local model/low-effort example in the audit is supported by this control flow (C). Native configuration documentation confirms that CLI overrides outrank configuration files (P; source list below).

**Specification conflict.** The existing per-model scenario intentionally replaces an unrelated machine effort when selecting Grok, while the project decision record says saved local model/reasoning preferences win. Treating all local effort as absent reproduces the defect; changing only the implementation would leave contradictory acceptance. This delta makes an applicable native choice authoritative and retains the mapping only as a fallback. It does not choose a new model or change the provider policy.

**Additional boundary.** Current portable resolution inspects shared and user files, while native configuration also has trusted project layers. A fix that recognizes only the user file can still defeat project configuration by promoting shared defaults to CLI overrides. This is a required installed-consumer test, not a claim that a current project-setting reproduction was executed. Native trust rules, profile selection, flags and compatibility selectors must be treated consistently.

**Chosen route.** Qualify the installed native configuration-resolution/inspection contract and reuse it or a supported lower-precedence default mechanism. Preserve value provenance rather than reconstructing a second full loader. If resolution is unavailable, do not guess and promote a fallback over an unknown native choice. Maintain upstream launch availability and bounded diagnosis. Expose effective model/effort and source through an existing diagnostic owner, with secrets excluded.

**Acceptance.** Test configuration through final argv and effective native consumer values: absent effort, saved model/effort pair, trusted/untrusted project layer, nested applicable project settings, CLI override, profile, remote, compatibility selector, unmapped model, TUI and exec, plus malformed or unavailable optional defaults. Use the installed consumer's supported behavior; documentation alone does not settle the test.

### 4. Make baseline comparison trustworthy before using it to select optimizations

**Confirmed audit findings.** [Baseline implementation](../../../../crates/token-audit/src/baseline.rs) returns a name already ending in `.json`, while explicit resolve appends `.json` again. Compatibility is set from a schema number before deserialization, whose failure is suppressed. The default directory omits `.codex` when only the user profile is available. Writers share staging names and `latest.tmp`. Snapshot scope is stored but not used to distinguish comparable populations. [CLI rendering](../../../../crates/token-audit/src/main.rs) prints every session and bucket movement (C).

**Additional findings.** [Report generation](../../../../crates/token-audit/src/report.rs) serializes `generated_at` to seconds; baseline save derives its final name from it. Unique temporary names alone would still let concurrent same-second saves overwrite the same final snapshot. Further, snapshots keep some bucket-level partial metadata, but session digests drop warnings and usage basis, full scan coverage is absent, and diff/rendering discards coverage from the movement view. A reduced known subtotal can then look like a complete decrease. It is inaccurate to say the parser has no coverage accounting; the loss occurs while projecting the comparison (C).

**Chosen representation.** Publish immutable uniquely identified snapshots and an atomically updated pointer under the existing local state owner. Define latest by publication order, not by an assumed monotonic clock. Validate JSON/schema/required fields and integrity of comparison inputs before producing a valid snapshot value. Represent format validity and population comparability separately. Carry observation mode, interval, parser semantics, usage basis and coverage through snapshot and diff. Legacy data with missing metadata remains inspectable with a weaker explicit status, never upgraded by guessing.

Preserve the existing bounded-report/detail pattern for diff. A concise result must contain complete aggregate totals and coverage, selected significant movements, hidden counts and stable access to complete comparison data. Default sorting and ties are deterministic. Explicit full JSON remains an exhaustive opt-in. Relative baseline names and pointer contents must not escape the local baseline owner. The design does not add transcript storage or a second analyzer.

**Acceptance.** Real CLI `save -> diff` with the returned name; same-second concurrent saves; kill during publication; valid-schema/missing-fields JSON; unknown schema; different roots/windows/modes; mixed or reduced coverage; missing usage; legacy snapshot; thousands of changes; complete paging and matching aggregate totals. An invalid baseline cannot acquire fabricated zero totals or a successful compatibility flag.

### 5. Separate temporal accounting from incremental execution

**Finding.** `report::scan` fully reads files, filters by last activity and aggregates lifetime session counters; its day key uses the first timestamp. This implements active-session analysis, not exact interval usage. Existing documentation partly acknowledges it, so it is a semantics limitation that must be made explicit rather than relabeling all historical reports as corrupt (C).

**Temporal design.** Keep the old activity view with an explicit label and add a separately selected interval view. Extend the existing [shared rollout reader](../../../../crates/harness-core/src/rollout_reader.rs), preserving the delegation consumer. Declare UTC half-open bounds and whether the evidence timestamps represent a response/usage record. Token generation instants that the provider never exposed cannot be recovered from a timestamped final counter. Deduplicate stable responses, retain distinct attempts/retries, detect conflicts and counter resets, and leave boundary-crossing deltas unknown when they cannot be allocated. Mixed-model sessions need attributable events; do not guess a single model. Check the event format actually recorded by the target native CLI.

**Incremental design.** Reuse parser state only across a proven unchanged prefix and a stable semantic version; checkpoint the last complete event and preserve coverage for incomplete tails. A disposable local cache holds derived data, not authority. Full parsing remains the reference and recovery path. Source bytes read and events parsed are observable mechanical metrics, separate from wall time.

**Important correction.** File identity, length, timestamp and an offset are not sufficient to prove the old prefix unchanged. An in-place edit can keep identity and size; timestamps can also be preserved. Sampling only the tail misses earlier mutation. A supported change-generation signal or a genuinely controlled immutable snapshot can establish eligibility; otherwise the safe route is full parsing. Do not hide an append-only operating restriction from the user. The invalidation mechanism is a bounded technical qualification task before dependent cache implementation. The required capability permits safe full-scan fallback; it does not demand a fast path for inputs whose stability cannot be established.

**Acceptance.** Equivalence with full parsing on append, partial tail, truncate, replace, same-size/preserved-time mutation, duplicate/conflicting events, counter reset, model change, missing timestamps, parser change and concurrent snapshot boundaries. Repeated unchanged eligible inputs must demonstrate fewer source bytes read; actual latency is reported separately. No improvement claim uses the old activity report as though it measured seven days of spending.

### 6. Remove launch-only freshness work while retaining integrity

**Finding.** [Build identity](../../../../crates/harness-core/src/build_identity.rs) hashes the manager, hashes it again in the binary loop, and traverses compilation sources. Healthy, source-stale and source-unavailable states all permit use of an intact runtime. [Build selection](../../../../crates/harness-core/src/build_selection.rs) calls that check on ordinary selection. This is avoidable work for the launch decision (C).

**Chosen route.** Split an integrity/compatibility result from an optional source-freshness diagnosis and reuse the integrity operation already present where applicable. Preserve each caller's required level of evidence. Reuse a verified executable identity only within the same coherent operation; do not replace hashing with an indefinite mtime cache. A different or ambiguous artifact invalidates the result.

Two audit formulations need correction. First, ordinary launch cannot perform zero checkout reads: live manifest, defaults and other linked data still come from the source owner. The objective is zero traversal of compilation inputs solely for freshness. Second, an altered harness build must not block the separately valid upstream Codex CLI. It disables use of that harness runtime and preserves the existing one-launch fallback. These constraints appear in the delta explicitly.

In [native build](../../../../crates/harness-core/src/native_build.rs), `find_reusable` filters root/toolchain/target but performs consumer validation before rejecting a different source digest. Pass the already known expected identity into selection and reject obvious mismatches early. Still perform final consumer and integrity validation on the selected artifact. This is not authorization to reuse stale binaries.

**Acceptance.** Instrument the owning read/check boundary with native test evidence: no compiled-source freshness scan on ordinary launch, correct explicit fresh/stale/unavailable diagnosis, one upstream launch when optional harness integrity fails, live data still applied, no consumer validation for definitely mismatched candidates, and unchanged validation for the selected candidate. Measure launch I/O and elapsed time separately.

### 7. Preserve the reasons for cold publication builds

**Finding.** Native publication creates a fresh short target directory and builds release with `lto = true`, `codegen-units = 1`. Source identity includes most of the crate tree and can be invalidated by non-production test changes. These costs exist, but their measured share in everyday work is not yet known (C/I).

The code documents two reasons for fresh targets: avoid trusting stale content through Cargo's timestamp cache and keep MSVC paths short. Removing this guard without a replacement would trade correctness for speed. Official Cargo documentation describes fat/thin LTO and codegen trade-offs, but does not prove the best setting for these binaries.

**Chosen sequence.** First establish that the existing targeted dev/test route is accessible without publishing a release. Preserve the full publication route for changes that actually exercise it. Then evaluate build identity against real compiler inputs and existing dep-info coverage. A file named `tests` or a Markdown fixture can be compiled via an include and cannot be excluded by category alone. Compiler flags, build scripts, environment, target and toolchain remain identity inputs where relevant.

Only after those boundaries are established compare optional cache or compiler-profile candidates. Use the actual changed packages and entry points, with the existing heavy-command/resource owner and isolated targets where concurrent verification needs them. Consider cold build, small-edit warm feedback, publication, executable size, startup and representative runtime operations separately. Keep the accepted flags/cache if net benefit or input correctness is unresolved. A database, remote build service, compiler migration or unrelated dependency cleanup has no demonstrated need here.

### 8. Replace unsafe TEMP ownership, including its current specification

**Finding.** `sweep_stale_scratch` enumerates the process TEMP root and recursively deletes old ordinary directories matching `hcb-`, `hcc-` or `hca-`. Name and last-write age do not establish ownership or lack of a live user (C). The existing `Native build scratch reclamation` requirement explicitly prescribes this behavior. Adding a new safety sentence elsewhere would leave contradictory requirements; the delta replaces that entire block.

Use a dedicated short owned root, validated owner metadata and a live operation lease. Age can decide retention after abandonment is established, not establish abandonment itself. Preserve reparse confinement through the actual deletion boundary. Reuse existing native state/lease mechanics where their scope fits. A PID alone is insufficient because it can be reused. Best-effort cleanup failures remain nonfatal to build correctness.

Legacy prefixed directories without recoverable ownership remain untouched. This may leave historical disk use; the bounded operating consequence is preferable to guessing that old data is disposable. It is not a permanent requirement for users to perform manual cleanup on every build. Existing interrupted owned operations can be recovered once ownership is established. Test a stale foreign prefix, active old owner, abandoned owned entry, junction/outside sentinel and ownership recovery.

### 9. Authenticate and version xAI transport without interrupting sessions

**Audit findings.** `ensure_xai_shim` reuses `Unknown` after warning. A changed build retires the old listener; the server stops accepting immediately and drains at most 60 seconds, despite a 30-minute request deadline. These are distinct confidentiality/routing and continuity problems (C); no actual credential leak was observed.

**Additional identity gap.** `shim_probe` calls a public loopback identity endpoint and trusts a matching JSON marker/schema and self-reported executable path. `same_executable` compares that supplied path; it does not prove which process owns the listener. Also, all control-request errors become `Free`, conflating unavailable evidence with an unused port. Merely rejecting `Unknown` would therefore leave a spoofed `Ours` case and misclassified errors (C).

**Chosen route.** Reuse the existing local process/generation ownership infrastructure to bind endpoint, verified build and live owner before credential-bearing traffic. Control-plane retirement needs appropriate ownership too. Unknown or foreign listeners are preserved, not killed. A verified alternate endpoint is permissible; failing the requested xAI route with an actionable message is preferable to sending secrets to an unverified process. This does not switch providers or authorize billing changes.

Give each supported live session a generation binding. A new build obtains a verified new generation; existing sessions keep both active streams and subsequent requests on their compatible endpoint. Raising the 60-second drain constant is insufficient because the old listener would still disappear before the old session's next request. Retire on owner/work release, not simply on discovery of a newer build. Separate explicit forced recovery from ordinary launch/update. Avoid a fixed session-lifetime restriction or an arbitrary new user/project cap.

**Acceptance.** An owned loopback fixture proves no credentials reach an unknown or spoofed listener, unavailable probing is distinct from free, and foreign processes survive. Real process integration then keeps a stream valid beyond the old grace while the new generation serves a new session, sends a subsequent request from the old session, overlaps two launchers, and recovers stale owner state. These transport tests need synthetic payloads, not provider credentials or model calls.

### 10. Make evidence, adoption and statistics different concepts

**Finding.** [Benefit gate](../../../../crates/harness-core/src/benefit_gate.rs) checks quality labels, matched counts, numeric consistency, tolerance, distinct arm names and an accounting label. This is useful validation of a report, not independent proof of execution or benefit. [Outcome report](../../../../crates/harness-core/src/outcome_report.rs) already retains unsuccessful attempts, comparability reasons and `benefit_status: not_evaluated`, and accounts for overlapping spans. The audit correctly avoids claiming those capabilities are absent (C).

Connect the existing experiment/attempt/oracle/report/decision owners rather than creating another report protocol. Distinguish input validity, evidence provenance, acceptance, comparability and positive effect. Small local types can prevent publication of an unchecked value in each owner; a generalized state-machine framework adds no demonstrated benefit. An internally consistent adoption with unchanged time and quality does not prove an improvement merely because it stays within tolerance. Conversely, a reproduced mandatory data-integrity fix can satisfy a declared quality objective even if it costs more CPU; disclose that cost.

Current report construction pairs every baseline and candidate attempt of the same case. Those edges can describe comparisons, but six edges from two baseline and three candidate runs are not six independent samples. Introduce explicit experiment, case, pair/block, attempt and retry identities where the existing evidence schema lacks them. Preserve raw attempts and independent task acceptance outside candidate control. Do not let an agent-authored assertion, a lifecycle label or a hash substitute for executed checks.

For stochastic changes, freeze the task mix, independent units, meaningful effect, stopping/uncertainty policy, repeated-selection treatment, cache policy and relevant conditions before measuring. Randomized or counterbalanced order is appropriate where order can affect cache or infrastructure load. Report effect and uncertainty at the declared unit; do not pick the best seed or silently exclude failures. The design deliberately does not impose one universal sample count or significance threshold on correctness fixes and model evaluations alike.

The complete metric vector contains acceptance rate, time to first useful signal, wall time through acceptance/rework, interventions, per-provider usage categories with coverage, and relevant I/O/output/build metrics. Cost per accepted task includes all attempted tasks and attributable children/retries, with no double counting of parent summaries, cached-input subsets or overlapping wall time. Zero accepted tasks makes the ratio undefined. Costs of implementation, unsuccessful experiments, evaluation, delivery and maintenance belong in a net-saving claim over a stated use horizon. API token counts are not exact account allowance, money or energy measurements.

Historical outcome acceptance explicitly left quantified benefit unproven and stopped a particular two-consumer evaluation. This new change may define an explicitly selected new experiment; it cannot rewrite that historical verdict or automatically spend model calls under old authorization. Required deterministic correctness checks need no speculative model benchmark.

### 11. Repair skill-experiment attribution before optimizing instructions

[Historical skill evidence](../../../../docs/memory/skill-evolution.md) reports an intended-case change from 335945 to 565755 tokens, but executions read the live skill and its references rather than the isolated revision. The historical verdict is inconclusive. Neither the increased count nor a shorter body establishes the causal effect of shortening (H).

Existing [skill evaluation](../../../../openspec/specs/skill-evaluation/spec.md) already requires protected acceptance, held-out cases, negative activation, uncertainty, full costs and bounded learning. Repeating that framework in another skill would increase instruction cost. The missing implementation evidence is the identity of the package actually consumed: catalogue metadata, body, references, helpers and any live-library fallback. Freeze these inputs, deny or detect contamination and report intended-case nonconsumption. A negative case is correctly allowed to avoid the skill.

Instruction shortening is a candidate, not a required blanket edit. First identify recurring loaded cost and mechanical procedures that have an existing deterministic owner. Preserve authority, publication boundaries, recovery and completion rules. Compare intended, similar-unsuitable, failure/boundary and independent held-out tasks with the surrounding library fixed. Metadata cost on nonapplicable tasks is distinct from body/reference cost after selection. Do not change externally maintained OpenSpec files or count their forced duplication as an owned optimization target.

### 12. Borrow mechanisms from primary sources, not their advertised outcome

Opened primary sources support the following limited design inputs:

| Source | Mechanism relevant here | Application and limit |
| --- | --- | --- |
| [DeepSeek session](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/session/README.md) | Model-visible facts derive from an event log; failed attempts have separate records | Attribute actual delivered presentation and attempts through existing native logs. A hard loss before settlement can still leave no durable attempt; do not promise perfect reconstruction where native evidence is absent. |
| [DeepSeek token meter](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/llm/token-meter/README.md) | Usage, context pressure and estimated breakdown are distinct; some pricing is heuristic | Keep measured usage separate from context estimates and bytes; do not import a heuristic as an exact tokenizer. |
| [DeepSeek system prompt](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/core/system-prompt/README.md) | Stable rendering and schema order affect prefix reuse | Evaluate stable sections with actual native consumption and cache evidence. Do not reorder authoritative instructions just to preserve a prefix. |
| [Claude Code MCP](https://code.claude.com/docs/en/mcp) | Definitions can be deferred and discovered on demand; behavior depends on version and route support | Qualify the installed Codex/native tool surface before proposing an equivalent configuration. Tool-search overhead and missed discovery belong in the comparison. |
| [Anthropic programmatic tool calling](https://www.anthropic.com/engineering/advanced-tool-use) | Intermediate results can be processed outside model context | Reuse Code Mode/native aggregation for measured deterministic chains. Their reported 43588 to 27297 tokens on research tasks is their benchmark, not a forecast for this repository. |
| [DeepSeek result pruner](https://github.com/deepseek-ai/deepseek-harness/blob/master/packages/compaction/compaction-tool-result-pruner/README.md) | Syntactic head/tail trimming retains originals but can remove useful middle content and invalidate earlier cache reuse | Reject blind pruning as a default quality-preserving promise; retain bounded semantic results and exact recovery. |

The [DeepSeek repository](https://github.com/deepseek-ai/deepseek-harness) identifies developer-preview compatibility risk. A runtime migration has no demonstrated local benefit and is not selected. Existing Code Mode, isolated executors, bounded output, raw retention and native memory records are existing strengths, not new feature proposals. No plugin is installed merely because it is popular. Primary-source behavior is inspiration; only this project's scoped acceptance can authorize adoption.

Two other consulted primary contracts guide concrete fixes: [native Codex precedence](https://learn.chatgpt.com/docs/config-file/config-basic) for model/effort preservation and [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) for build-profile trade-offs. Their current descriptions must be checked against the target installed version before implementation decisions depend on them.

### 13. Close the observed semantic-tool gap without diagnosing from a symptom alone

During this inspection, Serena activation reported Rust and Python, but the first symbol overview returned an uninitialized language-server-manager error. Its local log contained Python `JavaScript heap out of memory` and failed Rust analysis subprocess evidence (O). This proves unavailable semantic operations in that invocation; it does not establish whether heap configuration, job pressure, project coverage or another input caused the failure.

[Serena startup](../../../../crates/harness-core/src/serena.rs) already sets a 4 GiB worker job budget, and [shared routing](../../../../crates/harness-core/src/serena_shared.rs) already retries the specific fatal initialization marker once. Proposing another generic retry or simply increasing that limit ignores inspected behavior. The investigation used scoped native source reads after the failure and made no claim of working semantic coverage.

The implementation task preserves the failure evidence locally, checks intended source coverage/exclusions, relevant backend configuration and actual resource observations, and reproduces the smallest applicable managed operation. Correct the cause in its existing lifecycle owner, then exercise representative retained Rust navigation and affected multi-language operations through the installed consumer. A capability can remain explicitly unavailable during diagnosis; it cannot be marked delivered from activation or empty diagnostics. Removing a required language, changing everyday resource conditions or reducing accepted coverage is not an automatic repair. The new requirement concerns health truthfulness and cause-directed recovery, not a speculative rewrite of Serena.

### 14. Run existing checks continuously and keep their scope honest

The inspected tracked tree contains no `.github` workflow directory (C). The supplied audit reported zero remote Actions workflows; the repeat unauthenticated API request was rate-limited, so the current remote count is unknown (O). This does not mean there are no tests: the repository already has substantial lifecycle, transport, build, executor, RTK and analyzer tests.

Use those owners on an identified Windows/MSVC toolchain with locked dependencies. Deterministic checks include formatting, lint, native tests, ownership and source/link hygiene. Controlled process interleavings and malformed-state tests exercise the important failures. Do not replace them with assertions that the new documentation contains a word. Keep appropriate targeted feedback early and required broader checks before integration.

Separate automatic synthetic checks from actual install/update/recovery and installed-consumer acceptance, and both from explicitly selected model-backed evaluations. Public untrusted PR execution must not access production model credentials or the owner's installation. Action revisions are immutable verified references and permissions are minimal, following [GitHub's secure-use guidance](https://docs.github.com/en/actions/reference/security/secure-use). Synthetic privacy sentinels exercise public output. Workflow files and successful YAML parsing alone do not establish a green CI run; publishing and enabling remote checks remain separate authorized actions.

## Audit coverage matrix

Requirement references below resolve within this change. Task IDs are in [tasks.md](tasks.md). Evidence and acceptance details are in the numbered decisions above; the table is an index, not a second implementation owner.

| Alias | Requirement in delta specification |
| --- | --- |
| W1 | [Transactional observation publication and recovery](specs/token-efficient-agent-workflow/spec.md#requirement-transactional-observation-publication-and-recovery) |
| W2 | [Validated observation metadata confines retention](specs/token-efficient-agent-workflow/spec.md#requirement-validated-observation-metadata-confines-retention) |
| W3 | [Compression selection accounts for the delivered result](specs/token-efficient-agent-workflow/spec.md#requirement-compression-selection-accounts-for-the-delivered-result) |
| W4 | [Conservative Cargo status recognition](specs/token-efficient-agent-workflow/spec.md#requirement-conservative-cargo-status-recognition) |
| W5 | [Per-model default reasoning effort](specs/token-efficient-agent-workflow/spec.md#requirement-per-model-default-reasoning-effort) |
| W6 | [Evidence-gated context optimization through existing owners](specs/token-efficient-agent-workflow/spec.md#requirement-evidence-gated-context-optimization-through-existing-owners) |
| T1 | [Immutable uniquely named baseline publication](specs/token-audit/spec.md#requirement-immutable-uniquely-named-baseline-publication) |
| T2 | [Baseline validity comparability and coverage remain distinct](specs/token-audit/spec.md#requirement-baseline-validity-comparability-and-coverage-remain-distinct) |
| T3 | [Bounded baseline presentation retains complete detail](specs/token-audit/spec.md#requirement-bounded-baseline-presentation-retains-complete-detail) |
| T4 | [Explicit session activity and interval usage accounting](specs/token-audit/spec.md#requirement-explicit-session-activity-and-interval-usage-accounting) |
| T5 | [Incremental analysis preserves full-scan meaning](specs/token-audit/spec.md#requirement-incremental-analysis-preserves-full-scan-meaning) |
| R1 | [Native build scratch reclamation](specs/rust-native-harness/spec.md#requirement-native-build-scratch-reclamation) |
| R2 | [Ordinary launch separates runtime integrity from freshness](specs/rust-native-harness/spec.md#requirement-ordinary-launch-separates-runtime-integrity-from-freshness) |
| R3 | [Build selection avoids irrelevant expensive validation](specs/rust-native-harness/spec.md#requirement-build-selection-avoids-irrelevant-expensive-validation) |
| R4 | [Development feedback and publication retain separate evidence](specs/rust-native-harness/spec.md#requirement-development-feedback-and-publication-retain-separate-evidence) |
| X1 | [Owned xAI endpoint identity precedes secret-bearing traffic](specs/subscription-model-routing/spec.md#requirement-owned-xai-endpoint-identity-precedes-secret-bearing-traffic) |
| X2 | [xAI generations preserve existing sessions across delivery](specs/subscription-model-routing/spec.md#requirement-xai-generations-preserve-existing-sessions-across-delivery) |
| E1 | [Complete audit coverage has explicit closure evidence](specs/harness-outcome-evaluation/spec.md#requirement-complete-audit-coverage-has-explicit-closure-evidence) |
| E2 | [Benefit decisions consume executed and independently accepted evidence](specs/harness-outcome-evaluation/spec.md#requirement-benefit-decisions-consume-executed-and-independently-accepted-evidence) |
| E3 | [Experimental units and complete task accounting remain valid](specs/harness-outcome-evaluation/spec.md#requirement-experimental-units-and-complete-task-accounting-remain-valid) |
| E4 | [Optimization claims use predeclared scope and uncertainty](specs/harness-outcome-evaluation/spec.md#requirement-optimization-claims-use-predeclared-scope-and-uncertainty) |
| S1 | [Actual consumed skill identity determines comparability](specs/skill-evaluation/spec.md#requirement-actual-consumed-skill-identity-determines-comparability) |
| G1 | [Semantic readiness is established through applicable operations](specs/global-code-tools/spec.md#requirement-semantic-readiness-is-established-through-applicable-operations) |
| C1 | [Deterministic Windows checks produce a commit-bound signal](specs/harness-continuous-verification/spec.md#requirement-deterministic-windows-checks-produce-a-commit-bound-signal) |
| C2 | [Lifecycle and model-backed checks retain their own authority](specs/harness-continuous-verification/spec.md#requirement-lifecycle-and-model-backed-checks-retain-their-own-authority) |
| C3 | [CI dependencies and public artifacts preserve integrity](specs/harness-continuous-verification/spec.md#requirement-ci-dependencies-and-public-artifacts-preserve-integrity) |

The following 39 rows cover the supplied audit's twelve numbered sections, external comparisons, architectural recommendation, ordering and measurement guidance. Mandatory fixes are marked **required**; candidate adoption remains conditional, while its investigation and explicit decision are required.

| ID | Supplied audit location and point | Assessment | Requirement | Task | Closure evidence |
| --- | --- | --- | --- | --- | --- |
| A01 | Section 1: shared pack transaction and crash recovery | C, required | W1 | 1.1, 1.2 | Real interleavings preserve successful handles and prior committed content |
| A02 | Section 2: full compact size, both streams and exit text | C, required | W3 | 1.4 | Actual complete applied output is smaller; correct raw fallback |
| A03 | Section 2: Cargo status alignment and heuristic limits | C, required | W4 | 1.5 | Negative lines survive; originals and limitations remain |
| A04 | Section 3: saved effort, precedence and source | C, required | W5 | 2.1, 2.2, 2.3 | Effective native selection and final argv agree across the matrix |
| A05 | Section 4: save/resolve suffix mismatch | C, required | T1 | 3.1 | Returned identity works unchanged through CLI |
| A06 | Section 4: schema number is not successful validation | C, required | T2 | 3.3 | Malformed valid-version snapshot is invalid |
| A07 | Section 4: scope and format compatibility differ | C, required | T2 | 3.3 | Root/window/mode mismatch remains explicit |
| A08 | Section 4: default Codex home | C, required | T1 | 3.1 | Native user-profile fallback location matches |
| A09 | Section 4: concurrent baseline and latest publication | C, required | T1 | 3.2 | Distinct successful saves survive concurrent/interrupted publication |
| A10 | Section 4: unbounded diff | C, required | T3 | 3.5 | Bounded output, exact totals and exhaustive stable detail |
| A11 | Section 5: active-session and daily-usage semantics | C, required | T4 | 4.1, 4.2, 4.3 | Old/new response, boundary and missing-data cases report correctly |
| A12 | Section 5: incremental reader | I, required capability with safe fallback | T5 | 4.4, 4.5, 4.6 | Full-scan equivalence and fewer reads for proven eligible inputs |
| A13 | Section 6: launch integrity versus source freshness | C, required | R2 | 5.1, 5.4 | No launch-only freshness traversal; diagnosis still accurate |
| A14 | Section 6: repeated manager hashing | C, required | R3 | 5.2 | Duplicate unchanged per-decision hashing removed without stale trust |
| A15 | Section 6: reject nonmatching candidates early | C, required | R3 | 5.3 | Nonmatching candidates avoid consumer probes; selected one is verified |
| A16 | Section 7: development checks versus publication | C/I, required route | R4 | 8.1 | A real small edit is checked through the appropriate native path |
| A17 | Section 7: compilation input identity | C/I, investigation required | R4 | 8.2 | Real compiler-input coverage; narrowed key only if safe |
| A18 | Section 7: LTO/codegen/cache trade-offs | P/I, candidate decision | R4, E4 | 8.3, 8.4 | Scoped measured adopt/reject/inconclusive; no flag folklore |
| A19 | Section 8: TEMP ownership and active owner | C, required | R1 | 6.1, 6.2, 6.3 | Foreign/live/junction data survives; owned abandonment reclaimed |
| A20 | Section 9.1: unknown xAI endpoint | C, required | X1 | 7.1, 7.2 | Unknown route receives no secrets and foreign process survives |
| A21 | Section 9.2: update interrupts old xAI work | C, required | X2 | 7.3, 7.4, 7.5 | Long stream and old session's next request survive delivery |
| A22 | Section 10: reproducible Windows CI | C; remote count unverified, required | C1, C2, C3 | 12.1, 12.2, 12.3, 12.4 | Actual commit-bound checks; lifecycle/model boundaries explicit |
| A23 | Section 11: evidence-to-adoption chain | C, required | E2 | 9.1, 9.3 | Unsupported or stale evidence cannot authorize an improvement |
| A24 | Section 11: nonregression is not a positive effect | C, required | E2, E4 | 9.1, 9.4 | Tolerance-only record cannot establish benefit |
| A25 | Section 11: all attempts, children and rework | C, required | E3 | 9.2 | Complete costs, honest unknowns, no overlap double count |
| A26 | Section 11: cross-pairs are not independent samples | C, required | E3 | 9.2 | Explicit pair/block units and correct sample accounting |
| A27 | Section 12: consumed skill and references | H/C, required | S1 | 10.1, 10.2 | Live-library contamination detected and excluded |
| A28 | Section 12: intended/negative/held-out acceptance | Existing contract, required preservation | S1, E2 | 10.2, 10.4 | Independent oracle and protected-workflow results |
| A29 | Section 12: mechanical rules and conditional instructions | I, candidate decision | W6, S1 | 10.3, 10.4, 10.5 | Frozen-package comparison preserves core obligations |
| A30 | External comparison: model-visible replay | P/I, scoped integration | W6, E2 | 9.3, 10.3 | Delivered evidence identity or explicit unavailable reconstruction |
| A31 | External comparison: usage/context/heuristics | P, required distinction | T2, T4, E3 | 3.4, 4.1, 9.2 | Units and measurement basis remain distinguishable |
| A32 | External comparison: stable prompt prefix | P/I, candidate decision | W6, E4 | 10.3, 10.4, 10.5 | Native consumed prefix/cache/task evidence |
| A33 | External comparison: deferred MCP discovery | P/I, candidate decision | W6, G1 | 10.3, 10.4, 10.5 | Supported route discovers needed tools; negative cases pass |
| A34 | External comparison: programmatic mechanical work | Existing capability/I, candidate decision | W6, E4 | 10.3, 10.4, 10.5 | Existing owner reduces measured work without hiding errors |
| A35 | External comparison: no unproved runtime migration | P/I, retain selection | E1, E4 | 9.5 | Evidence-based no-migration decision; no extra runtime installed |
| A36 | External comparison: no blind head/tail default | P/C, retain selection | W6, E4 | 9.5 | Mid-result diagnostic counterexample and quality-preserving decision |
| A37 | Architecture: separate validation, selection and publication | C, required local boundaries | W1, W3, T2, R2, E2 | 9.6 | Owning corrections prevent the specific invalid state transitions |
| A38 | Delivery ordering: small verified increments | Required integration discipline | E1, C2 | 13.1, 13.3 | Independent phases integrate; full outstanding scope remains visible |
| A39 | Final metrics: accepted outcome, time, quality, tokens and quota | Required claim boundary | E3, E4 | 9.2, 9.4, 13.3 | Complete metric vector and scoped claims with coverage |

Supplemental rows sharpen the audit or add verified gaps; they do not inflate hypotheses into discovered runtime defects.

| ID | Addition or correction | Evidence | Requirement | Task | Closure evidence |
| --- | --- | --- | --- | --- | --- |
| N01 | Pack metadata can construct an outside retention path; schema/corrupt-record handling is unsafe | C, RTK metadata and retention, decision 1 | W2 | 1.3 | Invalid path/schema/size rejected before destructive use; sentinel survives |
| N02 | Same-second saves collide on the final snapshot name | C, seconds in report and naming, decision 4 | T1 | 3.2 | Simultaneous identical timestamps preserve both immutable snapshots |
| N03 | Diff loses coverage/basis and can present a partial subtotal without its limitation | C, snapshot/diff projection, decision 4 | T2 | 3.4 | Missing-usage change is visible and cannot establish savings |
| N04 | Effort fix must cover project-native layers and reconcile existing conflicting requirements | C/P, decision 3 | W5 | 2.1, 2.3 | Actual installed precedence/trust matrix and coherent owning docs |
| N05 | Self-reported shim identity is not listener ownership; probe failure is not a free port | C, decision 9 | X1 | 7.1, 7.2 | Spoof and timeout fail safely; valid owned route still works |
| N06 | Semantic activation was followed by a backend OOM and unavailable navigation | O, cause unresolved, decision 13 | G1 | 11.1, 11.2, 11.3 | Cause-directed repair and real retained operation evidence |
| N07 | RTK no-growth promise must account for every route and preserve mandatory fault/progress text | C, decision 2 | W3 | 1.4 | Applied ledger includes overhead; fallback claims stay honest |
| N08 | Zero checkout reads and blocking all Codex on altered harness conflict with live-data/fallback contracts | C, decision 6 | R2 | 5.1, 5.4 | Zero freshness traversal with live data and native fallback preserved |
| N09 | Metadata-only incremental cache misses same-size preserved-time mutation | Logical counterexample, decision 5 | T5 | 4.4, 4.5 | Independent validity proof or full parse; no stale result |
| N10 | Original citation placeholders and universal-benefit wording are not verifiable evidence | Supplied audit and primary-source review | E1, E4 | 13.3 | Actual source links, explicit proof levels and no universal savings claim |

## Risks / Trade-offs

- Store serialization adds a short critical section. Keep command execution outside it, measure contention and test abandonment; removing the lock to recover throughput is not acceptable.
- More conservative filtering can return more raw output. This is an explicit quality trade-off; the complete result still decides whether compression is useful.
- Native configuration contracts can change. Qualify the installed consumer and preserve native selection on uncertainty instead of guessing or silently upgrading it.
- Incremental correctness may require more validation I/O than a timestamp cache. Report the remaining net reduction; use full parsing where reliable reuse is unavailable.
- Per-generation xAI coexistence uses temporary resources while old sessions remain alive. Reclaim by verified ownership without imposing an unapproved maximum session lifetime.
- Cold publication builds may remain necessary. Development-path improvements can still be useful without changing that guard; compiler/caching experiments may legitimately be rejected.
- CI cannot exercise every private environment or prove absence of defects. Keep real installed-consumer requirements explicit and privacy-safe; a synthetic fixture is narrower evidence.
- Statistical experiments can be expensive and inconclusive. Use predeclared relevant cases and stopping policy, retain the old default, and account for evaluation cost rather than testing until favorable.
- The semantic-tool root cause is unresolved. A failed operation justifies diagnosis, not disabling languages or unlimited memory growth.

## Migration Plan

1. Freeze synthetic reproductions and acceptance inputs in existing test owners, then fix independently verifiable correctness slices: RTK, baseline, unsafe cleanup and endpoint identity. Build the narrow required Windows checks alongside these repairs.
2. Deliver model/effort precedence and xAI generation continuity with their actual installed-consumer paths. Preserve old sessions and unrelated configuration; recovery must target only owned state.
3. Extend analyzer semantics and evidence accounting before using their numbers for optimization decisions. Version local formats; preserve old files and clearly distinguish weaker legacy comparability. Cached parser state is disposable, authoritative evidence is not.
4. Split launch freshness and development/publication checks. Measure the scoped effect; evaluate optional compiler/input/cache candidates only after their correctness boundary holds.
5. Repair actual semantic-tool readiness. Qualify the retained installed operations before using them as evidence for instruction or tool-surface experiments.
6. Execute explicitly selected optimization comparisons and record their decisions in existing owners. No model-backed work is automatically authorized by completing these planning artifacts.
7. Complete global delivery, outside-checkout consumption, interruption/recovery and applicable CI evidence. Reconcile all matrix rows, current docs and tasks. Planning validation does not close implementation tasks.

Rollback preserves a previously verified native build, recoverable local configuration and versioned original baseline/pack evidence. New writers must not silently make state unreadable to supported older readers; where backward read compatibility is impossible, use a versioned separate state location or a verified explicit migration with recovery. Endpoint rollback must not kill another generation's live sessions. No cleanup removes private evidence merely to produce a clean demonstration.

## Verification boundaries

Use [native checks](../../../../docs/rust-native.md) as the command authority. The applicable developer loop starts with targeted package/integration checks; completion includes the required formatting, warning-as-error lint, workspace/native regression, ownership and source/link checks and the affected real installed paths. Keep exact commands, input revision, toolchain, outcome, scope and local detail locators in existing evidence records. Tests of process interleavings use deterministic synchronization rather than sleep-dependent success. No model-backed calls are needed to prove a baseline filename bug, a failed publication or safe local endpoint ownership.

Before archive, matrix/task/spec coverage must be mechanically checked, mandatory tasks must have current acceptance evidence, and optional decisions must state their claim limits. Implementation checkboxes remain open in this planning delivery. No runtime acceptance, broad security certification, exact quota benefit or exhaustive audit of every repository line is claimed by the existence of this design.
