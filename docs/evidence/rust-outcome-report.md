# Native outcome accounting

2026-09-09. The candidate `codex-harness outcome-report --input PATH [--markdown]`
uses [Rust accounting](../../crates/harness-core/src/outcome_report.rs) and an
[explicit CLI consumer](../../crates/codex-harness/src/outcome_report_cli.rs).
It reads an attempt array or a previously generated report and writes JSON by
default. Markdown is explicit. It runs no model, project command, hook or child
process and leaves the input file unchanged.

The port follows [the maintained formatter](../../tools/outcome_report.py):
it retains failed attempts, checks and retry chains; uses the enclosing wall
span including gaps, verification and rework; and accounts for preparation
separately. It compares all required and caller-declared material fields.
Unknown usage stays null and per-run observations remain separate, without
summing potentially duplicated parent/child totals. A claimed `accepted` status
without executed acceptance remains incomplete. `benefit_status` remains
`not_evaluated`; no speedup, billing or quota saving is inferred.

The explicit input boundary is 8 MiB and 1024 attempts, with 16 MiB of generated
comparison records and 32 MiB of rendered output. Over-limit or malformed inputs
fail without dropping attempts or printing parser contents. Valid identifiers
are strings and check rounds/exit codes are integers. Invalid timestamps remain
unknown. Reports retain private input metadata; error redaction is not a claim
that report contents are suitable for public sharing.

Six deterministic accounting tests and two actual CLI tests passed. Cases cover
overlapping work, preparation, extended verification, failed/incomplete retries,
cycles/missing parents, changed retry identity, every matched field, custom
unknown fields, reloaded reports, absent and duplicate usage observations,
unsupported acceptance claims, duplicate IDs and private failures. The CLI ran
from owned temporary directories outside the checkout and exercised Unicode
paths, JSON/Markdown, over-size refusal and unchanged input bytes. These are
ported case oracles; no new Python execution or model-backed evaluation ran.

```text
cargo test -p harness-core --lib outcome_report --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo test -p codex-harness --test outcome_report --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo clippy -p harness-core -p codex-harness --all-targets --offline --locked --jobs 1 --target-dir <owned-check-target> -- -D warnings
```

Core/CLI Clippy and scoped formatting passed. Explicit Serena diagnostics for
the accounting and CLI modules were clean. Commands used the checkout cwd and
the existing private parent check target. Evidence, source and executable hashes:
`%LOCALAPPDATA%/codex-harness-evidence/outcome-report-b79ae54346314ed5b1c6600faadb4da4/`.
`accepted-core.*`, `accepted-cli.*` and `accepted-clippy.*` retain the final runs;
the earlier lint result remains available.

The [native usage parser](rust-delegation-usage.md) has separate acceptance.
Task 7.3 remains open: outcome execution/oracles and their remaining consumers
still need migration. The old formatter is retained for
those callers. Global installation and skill/lifecycle cutover remain unfinished;
the tested command is the explicit native candidate, not a new global alias.
