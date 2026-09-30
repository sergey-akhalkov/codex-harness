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

The evaluator SHALL accept an explicit local serving configuration and record actual model/weights identity, quantization, tokenizer and prompt template, server/backend identity, sampling settings, seed behavior, reasoning mode, context/cache settings and relevant execution environment. Before relying on repeated identical inputs, it SHALL perform a declared repeatability qualification through the actual agent and tools, not only a completion API. The required solution output SHALL repeat under identical controlled inputs; timing and explicitly identified nonsemantic metadata SHALL be excluded from output equality only by a rule fixed before qualification. Temperature or a fixed seed alone SHALL NOT establish qualification. Divergence, unknown material identity or configuration drift SHALL suspend dependent comparisons until corrected or a different repeatability policy is explicitly agreed. A changed model or billing route SHALL never be an implicit recovery action.

#### Scenario: Identical seeds produce different solutions
- **WHEN** repeated qualified-input attempts differ in required solution output despite identical sampling settings
- **THEN** the evaluator reports the divergence and does not run the strict loop as though deterministic behavior had been established

#### Scenario: The serving process changes during a comparison
- **WHEN** weights, backend, prompt template or a relevant cache/batching setting changes between arms
- **THEN** the affected pair is incomparable and qualification is repeated for the new identity before further comparisons

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
