# Frozen outcome inputs

Implementation started 2026-09-07. These criteria precede candidate model runs.
The owning [design](../../openspec/changes/accelerate-verified-delivery/design.md)
defines acceptance. This record is an experiment input, not evidence of benefit.

Private source snapshots: `%TEMP%/harness-outcomes-ehenes2t/sources/`.
`inputs.json` records every Git-listed source hash, including nonignored dirty
and untracked files. Snapshots are copies; the source checkouts are read-only.

| Consumer | HEAD | Snapshot SHA256 |
|---|---|---|
| opencode-kit | `0e75b94e28f8d860742ef4724f77297e6341630a` | `d6667605f610afc36abc3271f792b79b73846d5df6a43c2fdcaf439df3d7a966` |
| team-control | `f33d4db2148bf19a1993fd20df81c82997987825` | `925a3ca421afa698952181cdd2085471f3091631a6940976feb1956e5e02e5cb` |

Both arms use Windows, Node 24.18.1, native Codex 0.153.4, Astra xhigh,
the existing provider policy and the accepted stabilized hook sources recorded
in `inputs.json`. Role configuration is part of the frozen identity. During
implementation, `middle` was initially absent, later restored, then failed a
response decryption check; reserve work was explicitly reported. These are
implementation observations, not proof of availability in a native outcome run.
No acceptance case requires delegation. Other skills, tools and instructions
must match. Candidate skill availability is the treatment. Explicit native
discovery verifies both arms, including personal registrations.

Cold cases start from a new equivalent copy with no candidate-produced records;
the freshness case alone includes the same previous record in both arms.
Installed dependencies and filesystem caches are warm in both arms; no claim of
cold disk or uncached provider execution is made. Each native attempt gets 600
seconds and the existing 2 GiB Job boundary. All failures, corrections, waits,
acceptance commands and child attempts count in the accepted-result record.
Run baseline/candidate, then candidate/baseline for repeated benefit cases.

| Case | Input and task | Independent correctness oracle | Spec mapping |
|---|---|---|---|
| focused | Recorded opencode-kit; discover and run focused library validation | Actual native command, expected library summary/exit, accurate record in existing docs; immutable source hashes | project-verification: discovery/evidence |
| second | Recorded team-control; discover and run native lint | Actual `npm run lint`, exit 0, lint-only conclusion, immutable source | project-verification: provenance/lint limits |
| freshness | Same historical command record in both arms; fixture manifest changes from v1 to v2 after confirmation | Revalidation exercises v2 and retains prior v1 as historical, never current proof | project-verification: freshness/handoff |
| entrypoint | Source says v2; generated CLI still says v1 | Build from source, execute generated CLI returning v2, no weakened assertions | project-verification: actual behavior |
| reduction | opencode-kit focused wrapper runs a valid test writing 2 MiB on each stream | Direct Node exits 0; wrapper fails with ENOBUFS; accepted smaller input retains ENOBUFS and reference success; syntax-error reduction rejected | regression-reproduction: controlled case/reduction |
| process | Owned dual-stream, absent-readiness and hanging-child cases | Complete stream sizes, explicit readiness/timeout distinction, unrelated process/file survives, descendants reaped | regression-reproduction: isolation |
| missing | Recorded command requires a deliberately absent executable; installation forbidden for this case | Blocked result names missing prerequisite, no fabricated pass or substituted command | project-verification: blocked; outcome failure accounting |
| negative | Fix one typo in an owned Markdown file with local link | Exact text change and valid link, no application test/delegation/model-evaluation invocation or unrelated writes | project-verification: bounded activation |

Allowed writes are each attempt's owned source copy, its documentation/evidence,
generated fixture artifacts and private runtime logs. Production services, live
consumer checkouts, shared proxy control, credentials and unrelated processes
are outside the experiment. Prerequisite preparation occurs before comparisons.

The initial focused command failed for absent `jsonc-parser` in its copied state;
`focused-calibration.log` preserves it. The existing opencode-kit dependency tree
is copied into owned preparation state. Team-control's lint prerequisites are
installed into `lint-deps` with lifecycle scripts disabled and versions from its
manifest; its untouched source copy links to that owned directory. Neither
prerequisite correction counts as a candidate skill benefit.

The focused prerequisite was resolved before its paired model runs. Its immutable
`acceptance/` setup uses `focused-private-env-v1`: the recorded Node executable,
unchanged native wrapper and all 183 tests, with private runtime paths and native
temporary fixtures outside host skill ancestors. `focused-calibration.json`
records natural exit 0 and `OK: library tests=183`; `focused-check.ps1` retains
each request/result and raw streams. Both arms receive the same setup. The
controlled stale CLI fixtures use `controlled-v2` invocation auditing; those
oracles were checked before the first paired CLI run. These are preparation and
measurement corrections; the practical criterion below is unchanged.

The practical speed criterion is a reduction in total verified time of **both
15% and 15 seconds**, on each supporting real-consumer case across two attempts
per arm, with no correctness loss. The absolute floor represents a meaningful
discovery/check round trip rather than subsecond command noise. Baseline command
calibration is retained and full native baseline timing will be reported; the
threshold will not be lowered after candidate results. Alternatively, repeatable
correct completion or material defect detection gained within the same budget
supports a quality benefit. Report every case and variation. A small pilot
supports these cases only. Inconclusive evidence leaves acceptance open.

Hook comparisons keep skills fixed and use a separate controlled workload:
unchanged tree, one edited file, concurrent parent/child identities, finding
delivery, clearance and explicit incomplete state. The pre-repair snapshot is
copied from the diagnostic change's preserved rollback into `hooks-before`;
its hashes and current source hashes are in `inputs.json`. The diagnostic change
owns implementation and its [acceptance](diagnostic-reconciliation.md).
