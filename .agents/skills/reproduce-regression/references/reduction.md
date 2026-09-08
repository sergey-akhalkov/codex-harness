# Reduction and attempt records

Read when minimization might change the failure, a reference has limited applicability, or reproduction is intermittent/blocked. Examples below are illustrative and do not claim observed results in any consuming project.

## Define the failure before reducing

Use an assertion that identifies the behavior and relevant cause/phase, not merely a nonzero exit or a substring shared by unrelated errors. Preserve an immutable original input and the last accepted input with their identities. Pin implementation/runtime, entrypoint, environment and oracle alongside them. A new candidate version is a separate comparison arm, not an unrecorded reduction.

Example contract: after a valid initialization and request over stdio, a server returns one response with matching request ID within two seconds. Original defect: after initialization completes, a valid request reaches the handler, stderr volume exceeds a pipe buffer, and the response never arrives. A runner kills it at the deadline. Expected failure evidence includes valid framing, completed initialization, handler entry, missing response and deadline termination; stderr text alone is insufficient.

| Proposed case | Observation | Decision |
| --- | --- | --- |
| Remove unrelated request fields; keep valid framing, initialization and stderr pressure | Same handler reached, response missing, deadline hit under pinned conditions | Accept after execution confirms the original condition; retain predecessor. |
| Truncate the JSON message | Immediate parser error, natural nonzero exit; handler never reached | Reject: smaller input fails before the original defect. |
| Remove stderr pressure and shorten the deadline to 1 ms | Startup killed before initialization | Reject: changed timing/trigger yields a different timeout. |
| Change runtime and remove fields together | Hang disappears | Uncontrolled comparison; freeze runtime and retry the reduction independently. |

The regression check must reject malformed input and startup timeout as evidence for this hang. On the candidate it must assert successful initialization, handler entry and matching response under the preserved pressure, as well as termination behavior required by the project. Keep assertions fixed between arms. If those milestones cannot be observed, label causal attribution uncertain and retain the larger case instead of claiming verified reduction.

## Reference limits

A known-good release should have a recorded immutable revision/artifact identity, runtime, build configuration and the same applicable contract. A different implementation may demonstrate expected protocol behavior without sharing internal diagnostics. Compare only shared observables. If a reference uses a different transport, lacks the feature, or cannot run with available prerequisites, record that limit; it cannot establish that the candidate fixed this regression.

Without an executable reference, cite the relevant project specification/test invariant, retain baseline evidence where available, and distinguish contract conformance from a demonstrated historical regression. A baseline failure alone does not prove which change introduced it.

## Attempts and handoff

Use the existing issue/evidence home for a compact record; keep large or sensitive transcripts outside portable sources. Each attempt needs case/input identity, baseline/candidate/reference identity, exact argv/cwd/runtime/build, environment differences, start and duration, readiness/protocol milestones, outcome and raw evidence location. Record assertion results separately from runner outcome and exit code. Keep rejected reductions and failed setup attempts.

Illustrative bounded batch: five attempts, two-second response deadline per attempt, thirty-second overall budget including setup and cleanup. All attempts use input `request-A`, baseline `failing-rev-A`, the same recorded runtime/build and seed; changing those conditions starts a new batch.

| Attempt | Observation | Classification |
| --- | --- | --- |
| 1 | Initialized, handler entered, no response; deadline cleanup | Original failure reproduced |
| 2 | Matching response in 0.08 s, natural exit 0 | Not reproduced |
| 3 | Initialized, handler entered, no response; deadline cleanup | Original failure reproduced |
| 4 | Runtime could not launch; no initialization | Blocked infrastructure; not a product-failure reproduction |
| 5 | Session interrupted while waiting; final receipt unavailable | Incomplete; termination and response unknown |

Report “2 of 3 completed, comparable product attempts reproduced; 5 total attempts, one blocked and one incomplete,” with the evidence for every row. Do not call it deterministic or silently treat unknown results as passes. Preserve the original and last verified reduced inputs and explain why any row is excluded from the comparable denominator.

For continuation, record unresolved conditions, remaining budget or the reason for a new bounded batch, last candidate checked, outstanding acceptance and owned-resource cleanup status. If progress stalls, choose the smallest discriminating observation (for example whether the handler was reached), or report what prerequisite/evidence is needed. More identical retries without new information are not a substitute for a diagnosis.
