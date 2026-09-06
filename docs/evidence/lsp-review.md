# Independent diagnostic boundary review

Observed on Windows x64, 2026-09-06. This bounded review covers six concrete
failure paths in [journal.py](../../tools/lsp/journal.py),
[server.py](../../tools/lsp/server.py) and [backend.py](../../tools/lsp/backend.py).
The reviewer reproduced the failures independently; the implementation owner
made the corrections. The ordinary global language registry selected the real
installed backends in disposable projects outside this repository.

All six reproduced failure paths pass their correction checks. The dependency
case was rechecked independently for JSON, Python and XML; the additional
durable Python/XML regression passed actual error-to-clean transitions while
the consumer source bytes stayed unchanged.

This review is separate from the [language acceptance matrix](lsp-languages.md)
and from model-backed native Codex acceptance. It does not establish that all
possible races or language dependency graphs have been exhausted.

## Findings and correction checks

| Path | Observed failure and cause | Independent result after correction |
|---|---|---|
| Repeated Stop with a later source write | A previous native Stop completion matched the same turn identity for five seconds. After another write and native-service shutdown, command fallback returned `{}` without inspecting the newer bytes. | Corrected fallback compares current workspace bytes before reusing completion. The same scenario now reports actual JSON error 516 for the current revision; it accepts that revision into the journal. A deliberately short five-second budget reports pending rather than claiming clean. |
| Changes to dependencies of unchanged files | Only TypeScript/JavaScript enumerated dependent files. A JSON schema type change, Python exported return-type change, and XSD type change each produced automatic `clean` for the dependency while a fresh backend found an error in the unchanged consumer. | All three corrections independently pass: both dependency and consumer appear, with the actual consumer diagnostic, including Python `reportAssignmentType` and XML `XMLSchema`. The owner now rechecks same-language files from the bounded, hashed workspace snapshot; TS/JS continue to use actual projectInfo. JSON/JSONC and XSD/DTD are configuration inputs that invalidate the applicable snapshot. |
| Project-supplied Rust channel escaping installed toolchains | An absolute `rust-toolchain.toml` channel was appended as a path and selected an executable outside the installed rustup tree. The inert fixture executable was never run. | The same discovery call now raises a descriptive `ValueError` before selecting a command. The owner validates channel names and resolved executable containment; explicitly configured host-owned custom toolchains remain a separate registration path. |
| New source appearing during final Stop analysis | A controlled write created invalid `late.py` after the real JSON analyzer finished but before final report delivery. Only existing result files and configuration files were reconciled, so the batch said `clean`. | Final full source reconciliation now includes `late.py`, invalidates the older source generation and reports `unresolved`. The real Python backend independently confirms the late file is invalid. Repeated Stop uses a system message rather than another blocking loop; this check establishes honest unresolved feedback, not eventual analysis of an indefinitely changing workspace. |
| Pending analysis reused after a dependency changes | A retained pending job was keyed by the caller's bytes and configuration. Its unchanged caller could therefore reuse an old result after `lib.py` changed. The orchestrator reproduced a second batch reporting `clean` from a job that had observed the earlier library. | Job identity now includes the complete bounded source snapshot hash; jobs from older snapshots are removed from reuse. The regression independently passes: the pending first batch is unresolved, and the second batch starts analysis against the new dependency and reports its error. This uses a controlled asynchronous analyzer with real service/journal scheduling, not a real language-server timing probe. |
| Changed dependencies already open in a warm server | With both Python files already open, changing the caller's expected type and the library's return type together produced a false current `reportAssignmentType`: the caller was checked against the library's old open buffer. | Before querying, the client synchronizes changed open dependency buffers and closes deleted ones. An independent actual basedpyright rerun reports both files clean after the same batch; the durable regression also asserts that the already running client is reused. |

The new-source experiment wraps the actual analyzer only to schedule the
intervening filesystem write. Its diagnostic results are real. The Rust probe
uses an inert executable and discovery only; it is a path-selection check,
not execution of project code.

The fifth finding came from the orchestrator's related scheduling review. Its
controlled analyzer holds an old library observation across the first batch
deadline, then permits completion before the next batch. The correction check
asserts that caller analysis observes both generations and that only the new
generation supplies the final diagnostic result.

## Python dependency investigation

With the installed basedpyright 1.39.10 defaults, analyzing changed `lib.py`
publishes diagnostics only for that opened file. `main.py`, which imports it,
remains absent from the publication map. Opening and checking `main.py`
produces `reportAssignmentType` after the return type changes from `int` to
`str`. In workspace mode, the server initially publishes an unversioned empty
set for `main.py`, then its error; those notifications alone cannot certify a
current clean result. The documented default is `openFilesOnly`.
[Official language-server settings](https://docs.basedpyright.com/latest/configuration/language-server-settings/).

The exact installed-version source enables pull diagnostics only when the
client advertises dynamic registration. Its workspace diagnostic request is
a continuing stream, not a request with a normal final response; document
pulls operate on the selected file. These contracts informed the owning fix
without treating an elapsed delay or an initial empty publication as proof.
[basedpyright v1.39.10 source](https://github.com/DetachHead/basedpyright/blob/v1.39.10/packages/pyright-internal/src/languageServerBase.ts),
[dynamic diagnostic registration](https://github.com/DetachHead/basedpyright/blob/v1.39.10/packages/pyright-internal/src/languageService/pullDiagnosticsDynamicFeature.ts).

The selected fix keeps the existing per-document freshness protocol and checks
the same-language cohort from the already bounded source snapshot. It does
not rely on the speculative publication-based discovery path. Missing the
batch deadline remains an explicit unresolved result. This is conservative
within the known workspace; it does not claim discovery of arbitrary external
projects that are absent from the approved workspace roots.

## Retained evidence and limits

Host-local JSON records are under the verification account's temporary
directory. Original failing observations have the same basename plus
`-before.json`; the unsuffixed files contain the correction reruns:

- `harness-lsp-review-counterexamples.json`: repeated Stop and JSON schema.
- `harness-lsp-review-dependent.json`: Python and XML consumers.
- `harness-lsp-rust-channel-review.json`: inert Rust path escape.
- `harness-lsp-review-final-source.json`: controlled final-source race.
- `harness-python-published-review.json`: actual Python capabilities and
  publication ordering in default/workspace settings.
- `harness-pending-source-generation.json` and
  `harness-pending-source-generation-corrected.json`: original stale-job
  observation and independently executed regression, including source hashes.
- `harness-python-warm-batch.json` and
  `harness-python-warm-batch-corrected.json`: actual warm-server multi-file
  counterexample and independent correction, using the global Python backend.

The associated scripts remain alongside those records. Durable automated
regressions live in [lsp-adapter.py](../../tests/lsp-adapter.py), including
repeated Stop, escaped Rust channels, final source reconciliation and a real
JSON schema test. Its targeted
`test_unchanged_python_caller_and_xml_instance_follow_dependency_edits` also
passed with the installed servers, exercising error and clearance for both
unchanged consumers. Its
`test_pending_caller_job_cannot_survive_changed_dependency_generation` passed
with the explicitly substituted asynchronous analyzer. The final 33-test suite
also covers the actual warm-buffer multi-file correction. These fixtures do not replace native CLI hook registration,
fresh/resumed-session tool availability, dependency provisioning or the
separate fourteen-language acceptance checks.
