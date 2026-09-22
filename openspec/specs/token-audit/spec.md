# token-audit Specification

## Purpose
Measured local token-usage analysis and an optimization loop for Codex rollout
sessions: reports with context economics, evidence-ranked findings, baseline
diffing and a diagnostic skill consumer.

## Requirements

### Requirement: Measured session reports
The `token-audit report` command SHALL scan local Codex rollout session files
and produce aggregate token reports per session, project, model, reasoning
effort and day from recorded usage events. It SHALL accept both the
`event_msg` token-count format and the `token_usage_record` format, count each
model response once by stable response identity, and report coverage warnings
for malformed, unknown or missing records instead of failing the whole scan.
Reports MUST NOT include conversation transcript content, and project or
workspace paths SHALL be hashed unless the user explicitly supplies private
sources.

#### Scenario: Mixed event formats
- **WHEN** the scanned sessions contain both older token-count events and
  newer token usage records
- **THEN** both contribute once each to the same aggregates and the report
  states per-format coverage

#### Scenario: Malformed line tolerance
- **WHEN** a rollout file contains malformed or unknown event lines
- **THEN** the report completes, counts the affected records under explicit
  coverage warnings, and preserves the rest of the scan's results

#### Scenario: Private paths stay hashed
- **WHEN** a report is produced without explicit private-source input
- **THEN** project and workspace identities appear as stable hashes rather
  than raw local paths

### Requirement: Context economics metrics
Reports SHALL include cache efficiency (cached versus recorded input tokens),
per-session context repayment (the relationship between summed per-turn input
and the final turn's context size), and instruction-floor size (recorded base
and per-turn developer instruction bytes). Turn-level metrics SHALL be omitted
with an explicit unavailable marker when session records lack turn-identified
usage, rather than being synthesized.

#### Scenario: Marathon session repayment
- **WHEN** one session replays a long history across many turns
- **THEN** its report shows the summed per-turn input, the final turn's input
  and the resulting repayment multiplier for that session

#### Scenario: Missing turn identity
- **WHEN** usage records cannot be attributed to individual turns
- **THEN** the report marks turn-level metrics unavailable instead of
  estimating them

### Requirement: Evidence-ranked findings
The `token-audit findings` command SHALL rank optimization findings by
measured token mass and SHALL suppress any finding without measured mass.
Every returned finding SHALL carry a basis label of `measured`, `inferred` or
`estimated`, an evidence locator with session and turn identifiers, an owning
record or skill for remediation, and a validation plan referencing report or
baseline commands. By default only `measured` findings SHALL be returned;
other bases SHALL require an explicit request. Tool-level token attribution
MUST be labeled inferred, because rollout records do not attribute tokens to
individual tools.

#### Scenario: Default basis filter
- **WHEN** findings are requested without extra options
- **THEN** only findings whose basis is measured are returned, and hidden
  inferred or estimated findings appear as counts only

#### Scenario: Owner and validation present
- **WHEN** a finding is returned
- **THEN** it names an existing owning record or skill and a repeatable
  validation command, instead of embedding new instruction text as the fix

### Requirement: Baseline and diff loop
The `token-audit baseline save` command SHALL store report aggregates in
local state under the Codex home harness directory, outside tracked sources,
without transcript content or unhashed private paths. The
`token-audit baseline diff` command SHALL compare a fresh report against a
stored baseline and report metric movement per session, project, model and
day. Stored baselines SHALL either remain readable across analyzer upgrades
or the diff SHALL state the incompatibility explicitly.

#### Scenario: Adopted idea re-measured
- **WHEN** a baseline exists and a new report is diffed after an adopted
  change
- **THEN** the diff shows movement of the chosen validation metrics and names
  the compared baseline

#### Scenario: Baseline stays local
- **WHEN** a baseline is saved
- **THEN** it resides under `CODEX_HOME/harness/token-audit` and contains no
  transcript content

### Requirement: Preserved delegation accounting
Moving rollout event reading into a shared owner SHALL preserve the existing
`codex-harness delegation-usage` command contract: its inputs, outputs,
privacy handling and existing acceptance tests remain valid, and both
commands SHALL use one rollout reader implementation.

#### Scenario: Delegation usage unchanged
- **WHEN** the shared reader replaces the inline parser
- **THEN** the existing delegation-usage tests pass without modification of
  their expectations

### Requirement: Tokenomics skill consumer
The kit SHALL provide a `tokenomics` skill that runs the analyzer, interprets
findings against existing owners, records adoption or rejection in existing
project records, and does not create parallel efficiency instructions. The
skill SHALL be delivered through the kit's normal installation lifecycle and
verified against an installed consumer outside the source checkout.

#### Scenario: Diagnostic run
- **WHEN** the skill is invoked where recorded sessions exist
- **THEN** it produces the analyzer report, ranked findings mapped to owning
  records, and a validation step for each proposed idea

#### Scenario: Installed consumer
- **WHEN** the delivered kit is installed outside the checkout
- **THEN** the skill resolves and runs the analyzer from its installed
  location

### Requirement: No proxy pricing or quota claims
Analyzer outputs MUST NOT present currency costs or quota percentages. Token
counts, byte counts and explicit coverage SHALL be the only quantitative
currencies, and any estimate SHALL remain labeled as an estimate.

#### Scenario: Subscription session
- **WHEN** sessions were produced through subscription routing without usable
  rate-limit records
- **THEN** the report shows recorded tokens and coverage only, without
  invented cost or allowance fields

### Requirement: Bounded report presentation with complete detail
Interactive report and findings output SHALL present bounded ranked records, total and omitted counts, coverage warnings and the measurement limitations. Full existing JSON and baseline contracts MUST remain compatible. A compact observation SHALL retain its complete machine report in local evidence and provide detail access without scanning sessions again. Retention SHALL be bounded and expired detail SHALL produce an explicit error. Presentation MUST NOT alter aggregate counters or convert bytes into claimed subscription savings.

#### Scenario: Many recorded sessions
- **WHEN** the report contains more rows than the presentation limit
- **THEN** the summary reports omitted counts and a locator for the complete same-scan report
- **AND** retrieving details does not run a model or rescan sessions

#### Scenario: Incomplete or erroneous usage
- **WHEN** coverage is incomplete or some usage is unavailable
- **THEN** the summary retains that warning and does not present missing values as zero

#### Scenario: Existing machine consumer
- **WHEN** a caller explicitly requests full JSON or a baseline operation
- **THEN** its complete existing data contract is preserved

#### Scenario: Two scans finish with the same timestamp
- **WHEN** two reports share a generated timestamp but contain different observations
- **THEN** each returned locator retains its own complete scan without overwriting the other
