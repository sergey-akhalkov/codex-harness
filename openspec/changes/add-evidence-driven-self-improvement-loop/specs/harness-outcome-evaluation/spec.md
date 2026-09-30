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
