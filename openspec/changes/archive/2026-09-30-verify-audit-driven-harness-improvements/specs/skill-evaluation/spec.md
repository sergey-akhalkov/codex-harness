## ADDED Requirements

### Requirement: Actual consumed skill identity determines comparability

Skill evaluation SHALL verify the identity and availability of catalogue metadata, the selected body and all consumed references or executable resources in each arm, not merely the existence of an isolated directory. Isolation SHALL preserve common task capabilities while preventing or detecting fallback reads from a live or different library revision. An intended-case comparison without attributable treatment consumption SHALL be inconclusive. A negative or absence arm SHALL demonstrate the expected lack of activation without being required to read the absent skill. Existing intended, negative, boundary, held-out, protected-workflow, net-cost and uncertainty acceptance SHALL remain in force. External OpenSpec workflows SHALL remain outside this kit's mutation ownership.

#### Scenario: A candidate reads the live library
- **WHEN** the execution opens a live skill or reference outside its frozen compared package
- **THEN** the attempt is retained with contamination evidence and excluded from a causal claim about the compared skill revision

#### Scenario: Only the body matches
- **WHEN** the selected body matches the candidate but a consumed reference or helper differs from the frozen package
- **THEN** that attempt does not count as a comparable execution of the declared candidate

#### Scenario: A negative case correctly avoids activation
- **WHEN** a protected or similar-unsuitable request completes without reading the tested skill
- **THEN** absence is valid negative evidence rather than missing intended-case consumption

#### Scenario: A shorter skill passes only tuned cases
- **WHEN** the candidate passes its development examples but fails independent held-out or mandatory-behavior acceptance
- **THEN** it is not adopted and reduced instruction bytes do not override the failed outcome
