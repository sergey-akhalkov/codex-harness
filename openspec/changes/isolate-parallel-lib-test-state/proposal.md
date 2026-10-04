## Why

Board card codex-harness-4q4.35 records a real defect: a full parallel `cargo test -p harness-core --lib` run showed 23 failures across unrelated modules (native_launcher, installation, legacy, rollout) while every sampled failing test passes in isolation and scoped suites pass. The suspected mechanism is shared ambient state between concurrently running lib tests (for example shared temp roots, ambient environment mutation or global registries), not per-test defects.

## Measurement

Observed problem: parallel lib-suite runs intermittently fail in unrelated modules while isolated reruns pass. Investigation scope: harness-core lib tests at the current base revision, default parallelism, repeated three times. Measurement question: which shared state crosses test boundaries, and does isolating it make the parallel run deterministic without serializing the whole suite? Workload: the existing harness-core lib suite linked from this change. Evidence: board codex-harness-4q4.35 with the exact filter, counts and isolation evidence. Limits: reproduction is load-sensitive; the mechanism is confirmed only when a failing cross-test dependency is demonstrated, not assumed.

## What Changes

- Identify the concrete shared state that crosses lib-test boundaries and isolate it per test (process-scoped temp roots, environment guards, or per-test registries as the evidence dictates).
- The full parallel lib suite passes repeatedly without `-j 1` and without narrowing test filters.

## Impact

Test infrastructure only. The task is workload B of the improvement-loop comparison recorded on its Beads card: its acceptance runs the real suites, and its completion time under baseline and candidate harnesses is the measured quantity.
