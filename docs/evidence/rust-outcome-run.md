# Native outcome execution

The candidate `codex-harness outcome-run --request PATH --run-model-probes`
executes one explicitly selected native launcher. Without the opt-in flag it
returns `skipped` without reading the request or starting discovery or a model.
The [Rust executor](../../crates/codex-harness/src/outcome_run.rs) and
[incremental event observer](../../crates/codex-harness/src/outcome_events.rs)
replace the execution/observation part of the old outcome runner. Comparison
preparation, skill discovery/treatment, the suite and independent correctness
oracles remain unfinished; task 7.3 stays open.

The request is JSON with required `case_root`, `codex_home`, `launcher` and
`prompt` fields. Case and home must be separate existing directories beneath
the canonical temporary directory, outside this source checkout. The launcher
must be an explicit absolute `.exe`; the command does not search PATH or acquire
dependencies. The caller supplies the prepared native linked launcher and home.
The request can also set `timeout` (default 600 seconds), `output_limit` (default
128 MiB per watched file), `extra_config`, `useful_command_pattern` and
`cancel_file`. Unknown request fields fail. The useful-command expression uses
Rust regex syntax; unsupported expressions fail before execution.

The executor requests Astra/xhigh, rejects treatment keys that override model,
provider, profile, authentication or billing, and passes the original prompt
through stdin. It records the selected launcher hash, actual arguments and home
in private evidence. It uses the existing Windows Job implementation with a
2048 MiB limit and verifies membership before resuming the process. Timeout,
cancellation, stdout/stderr/final-file limits and root exit clean the owned tree.
This is a process ownership boundary, not a filesystem sandbox for the model.
The independent review caught the additional `openai_base_url` route override;
it and the ChatGPT/auth-store/local-provider keys are now refused before launch.
The endpoint meanings follow the [official configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference).
`observed_model_metadata_verified` checks model/effort labels and thread identity;
it does not establish the authentication route or endpoint. Prepared configuration
and actual live routing still require the suite/global acceptance.

Every allocated attempt retains `native-started.json`, final `native.json` and
private failure evidence even if validation or launch fails. Executed attempts
keep `request.json`, `started.json`, raw `events.jsonl`, `observed.jsonl`, stderr,
final message and the distinct `result.json` process receipt. The observer waits
for complete JSONL records, limits each polling batch, rejects malformed records
and retains later valid events. Signals require an actual completed command
with an integer exit code. A nonzero check can be useful; prose, a running command
or a null/boolean exit code cannot establish that signal.
Private `failure.json` retains the failure phase, I/O kind, original OS code and
message. CLI result errors stay sanitized. A real invalid-PE test confirms that
the original Windows error survives this boundary by comparing a direct failed
CreateProcess attempt with the recorded error. The first run exposed the lost
cause; the later test also corrected an assumed error 193 to the actually observed
error 216 for this fixture.

Zero process exit additionally requires a completed native turn. Error events
remain failures. `completed` is only execution status, never correctness or
comparison acceptance. Consumers must check `evidence_errors` and run their
independent oracle. Rollout discovery is confined to the isolated home's
sessions, skips links, rejects ambiguous matches and checks observed thread IDs
against event IDs. No rollout means unknown usage. A matching filename with a
different internal thread ID no longer contributes usage: the rejected report
stays in `rejected-usage.json`, while the attempt's usage is unknown. Missing
children, mixed/unverified route metadata and unexpected delegation are explicit.
Raw JSON and logs remain private; use the separate usage Markdown projection
when publishing a redacted account.

Verification on 2026-09-09 uses the real native command from owned directories
outside the checkout and a [Rust CLI double](../../crates/codex-harness/src/outcome_fixture.rs).
The [integration cases](../../crates/codex-harness/tests/outcome_run.rs) cover
opt-in, argv/Unicode/stdin/home, distinct evidence, event/process failures,
missing/ambiguous/wrong-thread usage, routing controls, unique failed attempts,
timeout/cancellation, child cleanup and output limits. Three observer tests cover
split UTF-8 records, invalid counter types and oversized/malformed-record salvage.
The wrong-thread test failed before the fix and passed afterwards.
All twelve executor integration tests, three observer tests and five existing
native launcher tests passed. Clippy, scoped formatting and explicit
Serena error/warning diagnostics passed; 54 local file links across the five
updated native evidence/entrypoint documents resolved.

```text
cargo test -p codex-harness --bin codex-harness outcome_run --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo test -p codex-harness --test outcome_run --test native_launcher --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo clippy -p codex-harness --all-targets --offline --locked --jobs 1 --target-dir <owned-check-target> -- -D warnings
```

Commands use the repository cwd and the existing private parent check target.
Evidence and source/executable hashes are retained under
`%LOCALAPPDATA%/codex-harness-evidence/outcome-run-eada8ed402ca4ae0ba1ce75e76f1714b/`.
`rollout-identity-before.*` preserves the failing counterexample and
`rollout-identity-after.*` the correction. `endpoint-before.*` and
`private-error-before.*` retain the two reproduced review findings;
`review-fixed.*` retains the first correction run (11 passed, one incorrect
expected OS code); `review-accepted.*` retains all twelve passing cases after
the independent OS-code comparison, and `review-clippy-final.*` the final lint.
The read-only Astra review found no additional concrete P0/P1 in its scoped
process/observation review; the parent reproduced both findings and verified fixes.
Installed Codex 0.153.4 accepted
`exec --strict-config --skip-git-repo-check --json --help`; its help confirms
stdin with `-`. This verifies the argument contract without a model call.
No real model, new opencode-kit evaluation or global activation ran in these
checks. Live native launcher/home integration remains part of migration delivery.

## Controlled outcome case targets

The [Rust case fixture](../../crates/codex-harness/src/outcome_case_fixture.rs)
adds `--outcome-case build|cli|flood|fail|no-ready|hang` to the existing test
executable. It must first be copied into an owned temporary case. Build/CLI
resolve inputs relative to that executable, preserving the old distinction
between source version 2 and stale generated version 1, and record real
executions in `execution-audit.jsonl`. Process targets preserve concurrent
2 MiB stdout/stderr, natural exit 7, absent readiness and a delayed descendant.
The suite preparer, model-facing case instructions and independent outcome
oracles still require integration; these targets alone do not complete task 7.3.

Two [actual Rust integration tests](../../crates/codex-harness/tests/outcome_case_fixture.rs)
passed, including a different invocation cwd, build/CLI audit order and bounded
Windows Job execution of all process modes. The existing native `harness-observe`
also exercised these copied targets: both flood streams matched every byte,
natural exit was 7, absent readiness produced `readiness-timeout`, hang produced
`timeout`, all jobs reported zero remaining processes, and the delayed marker
was absent after its deadline. Forced native codes 130/124 are recorded separately
from natural exit status; future oracle integration must preserve that distinction.
Ten discovery and five launcher integration tests passed alongside the new cases.
Clippy passed after a fixture-only conditional simplification. No models ran.

```text
cargo test -p codex-harness --test outcome_case_fixture --test outcome_discovery --test native_launcher --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
```

This increment's logs are in
`%LOCALAPPDATA%/codex-harness-evidence/outcome-arm-1757f8d3341e48279fed9884b87a95e3/`:
`fixture-integration.*`, `fixture-clippy-fixed.*`, `controlled-cli-smoke.json`,
`controlled-process-acceptance.json` and the individual native process receipts.
These target checks cover only the fixtures. [Arm preparation](rust-outcome-arm.md)
records the separate later consumer and installation guard acceptance in that directory.
