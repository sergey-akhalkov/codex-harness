## MODIFIED Requirements

### Requirement: Model and context efficiency within authorized routing

Optimization SHALL evaluate lead reasoning effort, unnecessary context, redundant preparation, waiting, worker execution and total delegation cost. Provider preference SHALL follow the kit orchestration configuration rather than fixed provider roles, while preserving acceptance. All OpenAI assignments SHALL remain in the Astra family without hidden provider, billing or Fast-mode changes. Reasoning effort SHALL be selected at supported task or turn boundaries according to uncertainty and risk, without restarting ongoing work solely to change effort. Changed effort defaults SHALL require comparable accepted-result evidence; token savings SHALL NOT justify a quality regression. Lost child results SHALL be distinguished from model/auth/quota failure and recovered through their owning mechanism before completed work is duplicated. Quota pacing across account windows and token-benefit comparisons are owned by the separate follow-up change and SHALL NOT be claimed by this change.

#### Scenario: A child ends without a final artifact
- **WHEN** evidence shows partial work without a model/auth/quota rejection
- **THEN** its visible state and delivery mechanism are reconciled without inferring provider unavailability or automatically transferring the work to GPT

#### Scenario: A lower effort candidate saves tokens but misses a defect
- **WHEN** that candidate fails unchanged acceptance
- **THEN** it does not become the global default

#### Scenario: Effort is selected without a fixed agent profile
- **WHEN** an assignment can use a supported lower effort without changing its required outcome
- **THEN** the lead selects that model/effort combination directly, the effective values are visible in its conversation pane, and no separate agent definition is required
