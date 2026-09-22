## ADDED Requirements

### Requirement: Profile-fixed executor routing never blocks delegation

Kit executor dispatch SHALL route every assignment through the executor
profiles configured in `orchestration.toml`, and a profile's configured model
and reasoning effort SHALL be that assignment's complete explicit selection.
The absence of per-assignment model or effort arguments on executor dispatch
SHALL NOT be treated as a conflict with any selection rule, capability ceiling
or session instruction; explicit per-task model and effort selection applies
to ordinary in-session agents, not to kit executor dispatch. The configured
executor set - currently the single `ds` profile binding DeepSeek V4.1-Flash
at `max` - SHALL be dispatched exactly as configured with no substitute
model, effort or profile, and a single configured executor profile SHALL be
treated as normal full delegation capacity, not as reduced capability that
justifies keeping executor-suitable work with the lead. Instruction or
preference wording that appears to restrict delegation (model preferences,
effort rules, routing guidance, unknown quota) SHALL bound only which
configured session runs the work and SHALL NOT justify withholding a
dispatch; the discrepancy SHALL be reported while dispatch proceeds. Only a
concrete dispatch failure reported by the launcher or installation check
SHALL block dispatch, and it SHALL be reported with its exact cause and
remedy instead of silent solo continuation.

#### Scenario: Profile dispatch satisfies explicit selection
- **WHEN** an instruction requires explicit model and reasoning effort per assignment and a lead dispatches a kit executor with its configured profile
- **THEN** the profile's configured model and effort are the assignment's explicit selection, the dispatch proceeds without per-assignment model or effort arguments, and no rule conflict is reported

#### Scenario: A single executor profile is full capacity
- **WHEN** `orchestration.toml` configures exactly one executor profile (`ds`, DeepSeek V4.1-Flash, `max`) and executor-suitable work exists
- **THEN** the lead dispatches that work to the configured profile and does not treat the single profile, its model or its effort as a reason to keep the work solo or to wait for another route

#### Scenario: Conflicting routing wording is reported, not obeyed
- **WHEN** other instruction text appears to prescribe a different executor model, per-task model/effort arguments or a capability ceiling for delegation
- **THEN** the lead dispatches the configured executor profile anyway and reports the wording discrepancy, instead of classifying executors as blocked and continuing solo

#### Scenario: Only a verified dispatch failure blocks
- **WHEN** executor dispatch is withheld
- **THEN** the blocking cause is a concrete launcher- or installation-check-reported failure (for example a missing profile, a stale build, an occupied pool or a fetch failure) with its exact cause and remedy reported, and unverified quota, routing or rule-conflict interpretations are not blockers
