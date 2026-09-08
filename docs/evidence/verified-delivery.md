# Verified delivery: implementation evidence

Status on 2026-09-08: deterministic checks and ordinary global consumption passed.
The paired study was stopped after the user's 2026-09-08 scope clarification;
repeatable benefit has not been established.
The [fixed inputs and criterion](verified-delivery-inputs.md) precede model runs;
the [tasks](../../openspec/changes/accelerate-verified-delivery/tasks.md) own completion.

## Sources and bounded checks

The linked sources are [project-verification](../../.agents/skills/project-verification/SKILL.md)
and [reproduce-regression](../../.agents/skills/reproduce-regression/SKILL.md).
Both passed the installed skill-creator `quick_validate.py` check. Their references
cover command knowledge/freshness, historical records, wrong-failure reduction,
and incomplete/intermittent attempts. The process resource reuses the existing
Windows Job supervisor; it does not discover or kill unrelated processes by name.

Use the resolved Python executable; the host's plain `python` command currently
resolves to the Windows Store alias. The failed alias invocation is retained.

| Check | Observed result | Private evidence |
|---|---|---|
| `tests/outcome-process.py` | Six original scenarios passed; after adding the short-output-limit case, that case and infrastructure failure passed | `harness-outcomes-ehenes2t/process-checks-2.log`, `process-final-delta.log` |
| `tests/outcome-fixtures.py` | Actual stale CLI returned 1, rebuilt CLI returned 2; original and reduced valid Node inputs retained wrapper ENOBUFS; SyntaxError was rejected | `harness-outcomes-ehenes2t/oracles/report.json` |
| `tests/outcome-oracles.py` | 10 deterministic corruption tests passed, covering all eight cases, escaped native skill paths and activation evidence | `harness-outcomes-ehenes2t/oracle-unit-audit.log` |
| Process oracle with actual `check_process.py` | Four real subprocess cases, complete streams and ownership assertions passed; synthetic native receipt used only to exercise the oracle | `harness-outcomes-ehenes2t/process-oracle-native.log`, `harness-process-oracle-native-g0uafhl4/acceptance/oracle.json` |
| `tests/installer.Tests.ps1` | 23 scenarios, 323 assertions passed | `codex-installer-reserve-c8bc845df25a4dc08a82a8eb5316c9d6.log` |
| `tests/outcome-suite.py`, `tests/outcome-report.py` | 9 + 12 checks passed: explicit subsets, isolation/drift gates, calibration evidence, preserved failures/rework, overlapping time and unknown usage | `harness-outcomes-ehenes2t/suite-integration-audit.log`, `report-integration-audit.log` |
| Native discovery without models | Both arms exposed the intended enabled/disabled candidate registrations; no model call | `codex-outcome-suite-zuajf45j/suite.json` |

Evidence paths above are relative to `%TEMP%`. The process checks exercised
concurrent 2 MiB streams, readiness failure, natural nonzero exit, forced timeout,
owned descendant cleanup and an unrelated live sentinel/file. The first readiness
attempt failed because setup compilation consumed its readiness interval; the
observer now starts that interval when the child is running. Both attempts remain.

The installer checks read the actual nested references/scripts through source
links, observe updates before reinstall, preserve foreign directories and links,
restore previous links on injected failure, and exercise disconnect/reconnect in
owned paths containing spaces and Unicode. Runtime discovery is a separate check.

## Diagnostic dependency and separate experiment

The completed [diagnostic reconciliation evidence](diagnostic-reconciliation.md)
records installed and outside-project finding delivery, clearance and incomplete
coverage behavior. Its retained rollback supplied the pre-repair source.
`tests/outcome-hooks.py --inputs <private-input-root>` exercised 36 real hook
invocations, 18 per arm, with fixed skills and owned homes/workspaces.
Each arm's exact source hashes, individual timing, output and reports are retained
under `harness-outcomes-ehenes2t/hook-comparison/`.

Both versions delivered the injected JSON `Value expected` finding. The old
version returned an empty Stop with the error still present; the accepted version
blocked Stop with that exact finding, then returned an empty Stop after correction.
The unchanged-tree case stayed silent in the accepted version. An edited 9 MiB
JSON input was explicitly skipped as unresolved, with its discovered revision.
Concurrent parent/child hook identities completed with valid bounded responses.
The retained outputs were also reviewed after strengthening the exact-finding
assertion; no extra model run or fabricated result was used.

| Workload | Events per arm | Median event before, seconds | After, seconds |
|---|---:|---:|---:|
| Unchanged tree | 2 | 1.420 | 1.533 |
| Single-edit sequence | 10 | 2.291 | 2.093 |
| Concurrent identities | 6 | 1.565 | 1.629 |

These are per-event distributions, not sums of overlapping child wall times.

This small command-hook experiment establishes these correctness observations.
It does not establish a practical latency benefit and is not a skill-effect result.
Subsequent parallel work changed `journal.py` and `server.py`; current source must
not silently inherit acceptance of the saved version. Native skill comparisons
must use one fixed accepted source installation or newly verified replacement.

Dependency acceptance was reconciled with the owning change's completed tasks,
the actual 30-event installed report (`harness-installed-reconciliation-539ck550`),
and its native parent/child evidence. The installed report identifies this source
root and retains actual TS2322 delivery, clearance, unresolved large input,
single race delivery and read-only consumer results. The shared comparison
sources are preserved with hashes in `inputs.json` and the after-arm source
snapshot; the controlled hook checks above exercise those bytes. Subsequent
unrelated runtime revisions are not substituted into that accepted snapshot.

## Global activation and first ordinary consumption

The normal `install.ps1 -Mode Install -CoreOnly` lifecycle completed against the
existing global home: 13 links, eight skills and three core agents. Both skill
registrations resolve to this checkout. Rollback metadata and the installation
log are retained in `harness-verified-delivery-activation-82fcb8306f4e41c090f52673d2036168`.

A fresh ordinary session outside the checkout used the installed launcher and
existing global profile: Astra, xhigh, OpenAI. It discovered and read
`project-verification` and its command-record resource through global links,
rebuilt the stale CLI, and observed the output changing from 1 to 2. The native
thread is `01a07ffc-ddc3-7280-bd93-59cf95244b70`; its transcript, command evidence,
source hashes and result live in `harness-ordinary-verification-z2dpyes4`.

The initial automated verdict rejected its JSON-formatted subprocess output
because the checker expected a plain output line. That original verdict remains
preserved. The actual command transcript shows the build and before/after CLI
executions. Before paired model runs, the controlled fixture was revised to
record real build/CLI invocations; the oracle reads that audit before its own
independent rerun. A direct fixture check confirmed the sequence CLI 1, build 2,
CLI 2 in `harness-execution-audit-o7uzo_tf`. This ordinary fixture result does not
substitute for consumption in both real projects or for regression skill use.

A second ordinary global session, `01a08011-dc3f-7ef0-b30a-849d70ebec5b`,
read both installed skill files and their command-record, reduction and process
references without injected skill paths. It reproduced the external opencode-kit
wrapper failure and reduced the valid child to 1,048,577 stdout bytes; one byte
less passed. The independent oracle passed reference success, preserved ENOBUFS,
smaller input, wrong-failure rejection, source immutability and actual activation.
Evidence: `harness-ordinary-regression-rl_7dz3m/acceptance/oracle.json` and native
transcript. This completes linked resource discovery and global activation
acceptance; verification consumption in both real projects remains separate.

Ordinary global thread `01a0801a-0552-7e03-910b-3546608080f4` then exercised
project verification in both recorded real project copies. Opencode-kit returned
natural exit 0 with `OK: library tests=183`; team-control's native lint returned
exit 0, zero errors and two warnings. Each project retained commands, evidence
and scope in its README/outcome record. Both independent oracles passed,
including all immutable hashes and a separate lint rerun. The team-control
result makes no application-correctness claim. Evidence and actual Astra/xhigh/
OpenAI usage identity: `harness-ordinary-consumers-ks9lefd7`.

## Paired attempts and configuration correction

The first launch (`codex-outcome-suite-npefhss7`) stopped before a model call
because the owned frozen kit no longer matched its seed after the invocation
audit correction. A newly seeded entrypoint pair (`codex-outcome-suite-w4irb1zq`)
completed both native tasks and both independent oracles passed. Candidate
activation and baseline non-activation were observed.

The original candidate report excluded timing comparison after a concurrent
session changed the global donor config. The runner now checks that donor while
preparing each isolated home, then pins and records the actual consumed config,
profile, hooks and related inputs before/after execution. Sixteen suite tests
passed after this correction. The old pair remains correctness evidence; missing
contemporaneous post-run configuration hashes prevent claiming strict timing
comparability retroactively. Its original report is preserved.

The corrected study, `codex-outcome-suite-nmvg8vbl`, completed one pair in each
real consumer. All four independent outcome checks passed, with no excluded
comparison reasons and no observed changes to consumed execution inputs.

| Case / arm | Preparation (s) | Execution and acceptance (s) | Total verified (s) | First useful signal, including preparation (s) |
|---|---:|---:|---:|---:|
| opencode-kit / baseline | 149.9 | 582.5 | 732.4 | 402.4 |
| opencode-kit / candidate | 50.0 | 523.0 | 572.9 | 304.6 |
| team-control / baseline | 154.3 | 273.5 | 427.8 | 383.9 |
| team-control / candidate | 8.3 | 295.3 | 303.6 | 293.2 |

These are single pairs in baseline-first order. Preparation differs substantially;
team-control candidate execution and acceptance actually took longer. The lower
totals therefore do not establish a repeatable skill benefit. The originally
declared repeat count, alternating order and practical threshold are not met.
No causal or general speed claim follows from this pilot.

The user then withdrew ongoing opencode-kit support and further spending on it.
The owned study driver was stopped after the team-control pair, before a second
opencode-kit model call. Its next preparation had started but made no model call;
that incomplete attempt is retained. The original suite report, explicit
cancellation receipt and derived attempt summary are preserved in the study
directory. The later queued controlled-case study did not start. The skills and
their installed process resources depend on this harness, not the sibling
opencode-kit checkout; the external project was an experimental consumer only.

## Remaining acceptance

The opencode-kit wrapper initially timed out after 180 seconds without forwarding
its buffered output. A subsequent 600-second / 2 GiB run completed with one doctor
assertion failure: temporary fixtures inherited a host skill through ancestor
discovery. Private runtime paths and temporary fixtures outside that ancestor
chain resolved the prerequisite without changing product source or assertions.
The exact native wrapper then passed all 183 tests in 162.735 seconds, with a
peak Job memory of 806,166,528 bytes. All 4,063 frozen source hashes still matched.
The successful `focused-calibration.json`, rejected attempts and preparation
recipe remain under `harness-outcomes-ehenes2t`; native stdout/receipt are under
`C:/Windows/Temp/harness-focused-native-5633d628`. Team-control
lint passed with zero errors and two console warnings after preparing its recorded
ESLint dependencies in owned state; this is lint evidence only.

The remaining native cases and repeated benefit acceptance have not passed.
Whether the two-consumer quantitative comparison remains required after removing
opencode-kit is pending the user's scope decision. No further opencode-kit run
is required or queued. Final requirement reconciliation must use that decision;
installation and unit checks alone do not close the currently recorded benefit
requirement. No exact subscription saving is inferred from delegation counts or
tokens.
