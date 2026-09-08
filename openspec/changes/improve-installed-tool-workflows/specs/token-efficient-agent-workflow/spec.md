## ADDED Requirements

### Requirement: Bounded tool results with accessible details

Installed routes SHALL scope tool requests before retrieval and expose only decision-relevant results to model context. They SHALL avoid duplicate representations and preserve rejected calls, tool errors, nonzero exits, partial outcomes, provenance, freshness and omission information. Necessary details SHALL remain accessible without repeating an already executed operation. Silent truncation, an empty response or a successful transport MUST NOT substitute for a complete result.

#### Scenario: Mixed independent batch
- **WHEN** a batch contains successful data, a rejected call, an MCP error and a process failure
- **THEN** the compact result preserves each outcome and its source and permits inspection of the original necessary details without rerunning the calls

#### Scenario: Result exceeds the selected scope budget
- **WHEN** even a bounded request returns more data than the selected output budget permits
- **THEN** the response identifies its omissions and a valid continuation or detail path, retaining any errors and coverage limitations

#### Scenario: Detail retention expired
- **WHEN** a temporary detail reference is no longer available
- **THEN** the workflow reports that absence and reassesses the need and authority for a new operation rather than presenting the earlier aggregate as complete evidence

### Requirement: Predeclared full-result comparisons

Workflow promotion SHALL use a predeclared task, input identity, independent oracle, allowed effects, materiality and regression tolerances, and a bounded comparison plan. Comparisons SHALL include necessary setup, execution, coordination, verification and correction cost while distinguishing cold and reused state. Quality criteria MUST NOT be traded for token savings, and tolerances MUST preserve the existing prohibition on material speed regression. Promotion on efficiency grounds or a recurring-benefit claim SHALL have repeated comparable observations beyond the observed variation; a single pair SHALL be labelled as such. Criteria MUST NOT be relaxed after observing results to manufacture a benefit.

#### Scenario: Smaller output makes the complete task worse
- **WHEN** a candidate emits fewer characters but omits a needed finding or increases complete-task cost beyond the predeclared tolerance
- **THEN** the candidate is corrected and rechecked or rejected, and the smaller response alone is not reported as an efficiency improvement

#### Scenario: Comparable native episodes
- **WHEN** a native baseline/candidate comparison supports promotion
- **THEN** both arms use the same substantive inputs, task, model/effort and correctness criteria, with all attempts and measured costs attributable to their respective arm

#### Scenario: Evidence remains inconclusive
- **WHEN** the bounded comparison cannot distinguish benefit from variation
- **THEN** the workflow retains the verified baseline, records the uncertainty and does not continue unchanged runs merely to obtain a favorable result

### Requirement: Scoped efficiency claims and retained selection

Reports SHALL distinguish measured tokens, estimated tokens, response size, elapsed time and subscription consumption. Unavailable measurements SHALL remain unknown. Completion SHALL include delivered required routes, their correctness evidence, the declared decisions for optional candidates and applicable global checks. Optional rejection MUST NOT close an unrelated mandatory requirement or transfer historical evidence to changed inputs. The selected hooks, resources, model families, provider routes and billing SHALL remain unchanged unless separately authorized.

#### Scenario: Only response-size evidence exists
- **WHEN** a comparison measures output bytes but cannot observe attributable native token or subscription use
- **THEN** the report describes the byte reduction and measurement limits without claiming a corresponding session or weekly-quota saving

#### Scenario: Optional capability is rejected
- **WHEN** a reviewed candidate is rejected under its declared conditions
- **THEN** its decision is recorded, the verified baseline remains usable, and all required routing, failure handling and global-consumption checks remain necessary
