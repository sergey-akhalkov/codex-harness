## ADDED Requirements

### Requirement: Rolling comparisons freeze the task separately from the runtime

A real-task comparison SHALL bind the same task prompt, source snapshot, task specification, allowed edits, acceptance version and limits to both arms, while identifying the baseline and candidate harness runtimes separately. Both arms SHALL begin from equivalent owned task state and fresh executor contexts without the earlier solution, sibling traces, researcher memory or mutable shared candidate installation. Consumed instructions, tools, plugins and skills SHALL be attributable to the selected arm. Provider/model and non-treatment environment settings SHALL remain common. Only the declared treatment SHALL vary; changed task inputs or accidental sharing SHALL invalidate the comparison.

#### Scenario: A harness-code task is used as the workload
- **WHEN** candidate A changes the harness repository that also supplies task B
- **THEN** both B executors receive the same pre-solution source snapshot, while their executing harnesses differ by A in separately owned runtimes

#### Scenario: The second executor finds the first solution
- **WHEN** an arm can consume the other arm's patch, branch, conversation or solution artifact
- **THEN** that pair cannot support a benefit decision and the contamination is retained as an explicit failure of comparison setup

#### Scenario: Linked worktrees expose sibling Git history
- **WHEN** candidate retention uses worktrees sharing a Git object database and references
- **THEN** measured task inputs use isolated snapshots or an equivalently verified boundary that does not expose sibling solutions through Git, rather than treating separate working directories as sufficient isolation

### Requirement: Local model comparisons require repeatability qualification

The evaluator SHALL accept an explicit local serving configuration and an explicit identity policy. It SHALL record actual available model/deployment metadata, quantization, tokenizer and prompt template, server/backend identity, sampling settings, seed behavior, reasoning mode, context/cache settings, effective client profile/catalogue and relevant execution-environment facts, with unavailable facts identified rather than guessed. Under the user-selected API-observed policy, required observable fields and their provenance SHALL be declared before qualification; endpoint/model and effective client configuration remain required, while unavailable weight hashes and hardware details SHALL be retained as limits rather than prerequisites. Full-material identity checks SHALL remain enforced when that stronger policy is selected. The policy itself SHALL be bound to the qualification; neither a field nor the policy may silently be dropped to obtain a pass.

Before relying on repeated identical inputs, the evaluator SHALL perform a declared repeatability qualification through the actual agent and tools, not only a completion API. The required solution output SHALL repeat under identical controlled inputs; timing and explicitly identified nonsemantic metadata SHALL be excluded from output equality only by a rule fixed before qualification. Temperature or a fixed seed alone SHALL NOT establish qualification. Divergence, an unavailable required observation, disappearance or drift of a declared observed field, or unknown identity required by the selected policy SHALL suspend dependent comparisons until corrected and requalified or a different policy is explicitly agreed. API-observed qualification SHALL NOT claim to detect unobservable changes to weights or hardware. A changed model or billing route SHALL never be an implicit recovery action.

#### Scenario: Identical seeds produce different solutions
- **WHEN** repeated qualified-input attempts differ in required solution output despite identical sampling settings
- **THEN** the evaluator reports the divergence and does not run the strict loop as though deterministic behavior had been established

#### Scenario: The serving process changes during a comparison
- **WHEN** a required observed model/deployment field, backend, prompt template, client configuration or relevant observed cache/batching setting changes between arms
- **THEN** the affected pair is incomparable and qualification is repeated for the new identity before further comparisons

#### Scenario: The API does not expose weight hashes or hardware
- **WHEN** API-observed identity is explicitly selected and the declared server/client observations and repeated agent/tool output checks pass
- **THEN** unavailable weight hashes or hardware details remain disclosed limitations without blocking that policy or being presented as verified full-material identity

#### Scenario: An advertised observation disappears
- **WHEN** a field required by the declared API-observed policy is absent or cannot be read before an arm begins
- **THEN** that arm is suspended until observation and qualification are restored, without dropping the field or replacing its value with a guessed default

#### Scenario: The requested model has no usable tool execution route
- **WHEN** the configured agent cannot execute and observe the required tools through that local serving configuration
- **THEN** qualification fails with the unsupported capability identified, rather than replacing real task execution with an unverified text-only simulation

### Requirement: Measurements distinguish useful work from experiment overhead

Each attempt SHALL preserve task acceptance, elapsed time through independent checking and corrections, model requests, sequential interaction rounds, tool operations and failures, and measured token categories with their subset relationships and coverage. Repeated input processing, cached input and reasoning/output categories SHALL not be double counted or converted from bytes into claimed measurements. Failed, cancelled and retried attempts SHALL remain accounted. Hypothesis investigation, planning, candidate preparation, evaluation, integration and recovery SHALL have separately attributable overhead without charging the same shared work twice. Time comparisons SHALL use actual elapsed intervals; token comparisons SHALL identify the model/tokenizer, and currency or local compute cost SHALL require an explicit measured basis.

#### Scenario: A batched tool call replaces several operations
- **WHEN** an arm uses one outer tool call to perform multiple operations
- **THEN** call and operation counts remain distinguishable and the lower outer-call count alone is not treated as a cost reduction

#### Scenario: A candidate saves inference but requires expensive setup
- **WHEN** candidate preparation or recurring startup is material to intended use
- **THEN** the report identifies its one-time and recurring costs, compares the declared warm/cold operating condition fairly, and limits any net-saving claim to the recorded use horizon

### Requirement: Infrastructure noise is separated through attributable lifecycle evidence

The evaluator SHALL retain observed end-to-end time and resource accounting and SHALL separately derive metrics adjusted for verified external infrastructure blocking. Neither view SHALL erase failed, cancelled, retried or waiting work. Native admission/process evidence SHALL correlate each wait with its attempt, tool call, command, resource, admission and parent identities, verified blocking ownership, and monotonic start/end cause. The evaluator SHALL distinguish unrelated contention, work belonging to the same attempt or experiment, and unknown ownership. An agent statement, queue diagnostic without complete boundaries or an unverified timestamp SHALL NOT establish a measured deductible interval.

The algorithm SHALL clip eligible waits to the attempt, union overlapping and nested intervals, retain only intervals of verified task blocking, and exclude concurrent useful activity. It SHALL subtract that duration once from observed elapsed time without counting queue time as command execution or producing negative time. Model activity not independently established as wait-only SHALL NOT be assumed idle. Cross-process timestamps SHALL have verified clock alignment. Coverage gaps, ambiguous ownership and execution slowdown without a measured idle boundary SHALL remain explicit uncertainty, not estimated zero or guessed deductions. Existing local evidence/process/admission and rollout owners SHALL own the trace; no independent monitoring service or second accounting journal is required.

Waiting-related model requests, tool operations and measured usage SHALL be reported separately from passive wait duration. Only whole requests independently correlated with a wait and structurally verified to perform waiting alone MAY be excluded from adjusted usage. Mixed task/status requests SHALL remain included with incomplete attribution; token categories SHALL preserve their subset relationships. Duration, bytes or prose SHALL NOT be converted into token or currency measurements. Candidate-controlled polling, redundant work, self-contention and execution after admission SHALL retain attributable cost. Changes in build demand, queue exposure or waiting strategy SHALL remain visible in the benefit analysis even where an adjusted view excludes verified external waiting.

#### Scenario: One arm encounters an unrelated occupied build slot
- **WHEN** equivalent task work differs only by a verified external queue delay under the same predeclared attribution rule
- **THEN** observed elapsed times differ while adjusted work metrics do not report a model or harness improvement solely from that delay, and the waiting cost remains in the operational view

#### Scenario: Queued work overlaps useful activity
- **WHEN** a 30-minute attempt includes a 10-minute external queue interval and 4 minutes of useful work overlap that interval
- **THEN** only the 6 minutes of verified blocking are deducted, adjusted elapsed time is 24 minutes, and observed elapsed time remains 30 minutes

#### Scenario: Multiple waits and inherited admission overlap
- **WHEN** two blocking intervals overlap or a nested command inherits an already held admission
- **THEN** the union is counted once and inherited admission does not create an additional wait charge or deduction

#### Scenario: Waiting generates model requests
- **WHEN** a blocked attempt has passive waiting, tool-only status polling, measured wait-only model requests and mixed task/status requests
- **THEN** each category retains its real operations and usage; only independently verified wait-only request usage is eligible for adjustment, and mixed requests and token-attribution uncertainty remain visible

#### Scenario: The treatment changes builds or polling
- **WHEN** the candidate avoids builds, submits redundant builds or changes polling frequency
- **THEN** actual build work, request/operation changes, self-contention and changed queue exposure remain attributable effects rather than being discarded as external noise

#### Scenario: Timing or ownership evidence is incomplete
- **WHEN** a wait end is missing, a blocking owner is unknown, clocks cannot be aligned or contention slows running work without a measurable idle interval
- **THEN** the evaluator retains known raw observations and verified deductions, labels the unresolved portion and makes no unsupported claim of fully corrected work time or usage

#### Scenario: Zero queue delay is distinguished from missing telemetry
- **WHEN** one attempt records immediate admission completely and another has no usable admission trace
- **THEN** only the first has proven zero queue delay; neither the absent trace nor the adjusted metric establishes a counterfactual completion time on an unloaded machine

#### Scenario: Admission expires before a command starts
- **WHEN** a task attempt times out in the infrastructure queue
- **THEN** its failed attempt, elapsed time and actual usage remain accounted, and the absence of a produced solution is not classified as evidence of an incorrect model solution

### Requirement: Benefit policy fixes the metric view and handles infrastructure uncertainty

Before observing comparative results, the policy SHALL bind the claim to work-efficiency or operational metrics, the attribution rule/version, eligible causes, coverage requirements and a material-uncertainty rule. The same rule SHALL apply to both arms and remain bound to retained evidence and decisions through resume. Work-efficiency analysis SHALL use adjusted metrics with sufficient comparable coverage; the operational view SHALL retain actual waiting and cost. A treatment targeting admission, scheduling or waiting SHALL be evaluated under controlled relevant load without excluding its own mechanism. Acceptance and cost/time trade-off requirements SHALL remain in force. A favorable adjusted result alone SHALL NOT establish net savings or justify a treatment-induced operational regression hidden in excluded categories.

If unresolved infrastructure effects could change whether the declared meaningful effect or regression threshold is met, the evaluator SHALL record `inconclusive`, identify the missing evidence and the smallest relevant controlled check, and SHALL NOT infer adoption or model/harness regression from the contaminated pair. A conservative bound that demonstrates the decision is unchanged MAY support the scoped conclusion with its limits. Any repeat SHALL follow the predeclared stopping rule, retain earlier attempts and costs, and SHALL NOT select away unfavorable load after seeing results.

#### Scenario: Infrastructure imbalance can reverse the verdict
- **WHEN** raw times suggest benefit or regression but attribution gaps could move the effect across a decision threshold
- **THEN** the result is inconclusive rather than a causal adoption or rejection, with retained raw/adjusted evidence and an explicit next check

#### Scenario: Waiting itself is the declared treatment
- **WHEN** the hypothesis changes admission or waiting policy
- **THEN** the experiment controls relevant contention and evaluates that operational effect instead of subtracting the changed behavior from both arms

#### Scenario: A report is resumed or the attribution rule changes
- **WHEN** an experiment is recovered or a newer analysis rule becomes available
- **THEN** its original policy and evidence remain identifiable, and a changed rule cannot silently replace the measured view or inherit an earlier benefit decision

### Requirement: Rolling evidence retains applicability and exact candidate lineage

The evaluation policy SHALL declare the meaningful effect, quality constraints, tolerable noncritical variation, repetition/stopping rule and treatment of repeated candidate selection before observing comparative results. Attempts SHALL be paired by task and operating block, with order and cache/load effects controlled. A result on B SHALL support only its exercised scope; task difficulty changes from B to C SHALL not be interpreted as a longitudinal speed improvement. The evaluator SHALL retain replayable completed real-task inputs and independent checks linked through their Beads owners, and use applicable prior real tasks for corroboration when the declared adoption scope requires it. A task that does not exercise the proposed mechanism SHALL not be treated as evidence of general uselessness. Rebased, edited, substituted or combined candidates SHALL require acceptance of their new exact identity rather than inherit another patch's benefit result.

#### Scenario: A is faster on B but B is easier than C
- **WHEN** the next loop step has a slower absolute duration on C
- **THEN** it compares H against H+B on C and does not infer a regression from the difference between B's and C's absolute durations

#### Scenario: A workload never invokes the optimized tool
- **WHEN** neither arm exercises the mechanism named by the hypothesis
- **THEN** the report identifies lack of applicability and leaves broader usefulness unresolved instead of recording a general rejection

#### Scenario: Two implementations of B differ
- **WHEN** baseline and candidate arms produce different valid B patches
- **THEN** the loop records which exact patch becomes candidate B, checks it against the chosen next baseline, and does not attribute a benefit measured for another revision to it

#### Scenario: Repeated search finds one apparently favorable pair
- **WHEN** a candidate is selected from many attempts or stopping was influenced by observed gains
- **THEN** the decision applies the predeclared selection treatment and required corroboration rather than treating the best pair as independent confirmation

### Requirement: Subtractive treatments prove useful effects and retained behavior

A simplification comparison SHALL name the removed burden and the intended-use conditions in which it incurs cost. It SHALL measure accepted-task elapsed time, interaction/tool steps and token/resource categories through the same accounting as additive candidates, including applicable startup, discovery, fallback, manual work, verification, maintenance and recovery overhead over the declared use horizon. Measurements SHALL distinguish invocation from exposure: a skill or tool need not be invoked to incur catalogue, instruction or initialization cost, but that cost and its removal SHALL require evidence of actual consumption in each arm. Missing telemetry SHALL remain unknown; smaller source files or zero invocations SHALL NOT be reported as measured context or token savings.

Both arms SHALL retain the same frozen task acceptance, including required correctness, diagnostics, supported compatibility and recovery. For any explicitly approved capability retirement, the changed product scope and lost scenarios SHALL be declared before comparison; the workload SHALL exercise the retained contract and SHALL NOT be rewritten after seeing failures to conceal a regression. Applicable caller/configuration/dependency checks and an exercised restoration path SHALL accompany the benefit evidence. Removing tests or checks SHALL NOT remove the independent oracle or prove retained behavior merely because fewer checks now run.

The default efficiency adoption policy SHALL require a meaningful measured benefit with required quality preserved; fewer steps alone SHALL be insufficient when work is merely moved or total time/resources materially worsen. No detected gain or regression SHALL NOT prove equivalence or uselessness. A maintainability-only or exposure-only claim SHALL remain separately identified and SHALL require an explicit predeclared user-agreed decision basis if it does not meet the efficiency policy; reduced size alone SHALL NOT justify adoption. Experiments SHALL NOT authorize retirement independently of the user's scoped removal decision.

#### Scenario: A skill is absent on disk but still appears in the executor context
- **WHEN** an approved treatment removes a skill yet the actual candidate conversation consumes the old catalogue
- **THEN** the intended context treatment is not established, and source deletion or a lower file count cannot support a token-saving claim

#### Scenario: A dormant feature has no observed overhead
- **WHEN** the selected workloads neither invoke the feature nor show attributable discovery, startup or other costs
- **THEN** the experiment cannot establish useful savings from removing it, and unexercised usefulness remains unresolved

#### Scenario: Fewer automated steps shift work to the user
- **WHEN** a simplification removes tool operations but introduces recurring manual setup or slower fallback
- **THEN** the comparison includes that burden and cannot claim net benefit from the lower operation count alone

#### Scenario: A removed wrapper still has a supported indirect caller
- **WHEN** a scoped consumer or recovery check fails after removal
- **THEN** the candidate fails retained-behavior acceptance despite a faster ordinary workload, and any expanded retirement scope requires an informed user decision before further dependent changes

#### Scenario: A candidate deletes the check that exposes its regression
- **WHEN** an apparent saving depends on removing required acceptance coverage
- **THEN** the unchanged independent oracle rejects it and the lower check cost cannot support adoption
