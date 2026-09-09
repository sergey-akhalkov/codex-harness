# Native delegation usage

2026-09-09. The candidate `codex-harness delegation-usage [ROLLOUT ...]`
reads only the supplied local JSONL files. JSON is the default;
`--format markdown`, `--output PATH` and `--private-sources PATH` are explicit.
The [CLI and aggregation](../../crates/codex-harness/src/delegation_usage.rs),
[visible telemetry parser](../../crates/codex-harness/src/delegation_usage_rollout.rs)
and [Markdown projection](../../crates/codex-harness/src/delegation_usage_markdown.rs)
replace the corresponding behavior of the retained Python helper. This command
does not launch models, discover descendant files or decode opaque state.

The parser takes the last cumulative thread snapshot once and separately
deduplicates per-response usage. It retains missing, invalid, conflicting and
mixed-model attribution as unknown/partial. Compaction records do not add
usage. Hook text alone does not establish an actual continuation. The
JSON omits full source/workspace paths, raw messages and source hashes but keeps
legacy thread/response identifiers and project basenames. Keep that JSON in
private evidence. Optional source records contain input ordinal, SHA-256, bytes
and suffix. Markdown hashes project names, omits raw thread/response identifiers
and makes no billing or quota conversion. The Grok review identified this shared
legacy privacy boundary; an actual CLI case now verifies both projections.

Thirty-four legacy test cases were ported to the
[actual native CLI tests](../../crates/codex-harness/tests/delegation_usage.rs).
All 37 integration tests and four focused unit tests passed. Additional cases
cover invalid counters, overflow remaining unknown, a record exceeding the
32 MiB boundary followed by salvageable usage, timestamps, Unicode paths,
hard-link/symlink aliases, colliding output destinations and native option
parsing. Each CLI fixture runs from an owned directory outside this checkout.
Malformed telemetry produces a partial report; invalid options and output
failures exit 2 with a static error that does not expose input contents.

An owned regression first demonstrated source loss if an input was renamed to
the output path between parsing and opening the destination. The fix retains
source handles and compares actual file identities before truncating either
output; the same test then passed. Windows output handles also exclude
concurrent write/delete sharing. This protects source objects, including aliases;
it does not claim atomic publication of both report files or a frozen snapshot
of a concurrently written input.

The old helper and the native candidate were run on two closed, owned Astra/Grok
logs with no new model calls. Full JSON and private-source JSON were semantically
equal, and input hashes stayed unchanged. All 48 Markdown lines matched after
newline normalization: legacy Python emitted CRLF on Windows, Rust emits LF.
Byte-for-byte Markdown equivalence is therefore not claimed.

```text
cargo test -p codex-harness --bin codex-harness delegation_usage --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo test -p codex-harness --test delegation_usage --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo clippy -p codex-harness --all-targets --offline --locked --jobs 1 --target-dir <owned-check-target> -- -D warnings
```

Commands used the repository cwd and the existing private parent check target.
Final Clippy, scoped rustfmt and explicit Serena error/warning diagnostics passed.
Evidence, exact source/build identities and the real-log comparison are retained
under `%LOCALAPPDATA%/codex-harness-evidence/delegation-usage-e537f4953ab04598a13c10870a292244/`.
`unit.*`, `integration-final.*`, `clippy-final.*` and `comparison-accepted.json`
retain the accepted runs; the later outcome-run Clippy also includes this code.
`rename-before.*` retains the original failed regression.
The compatibility run used the installed Python 3.13.14 and the existing legacy
helper. All new maintained implementation and executable fixtures are Rust.

Task 7.3 remains open for outcome execution/oracles and their consumers. The old
usage helper remains for those callers. This is an explicit native candidate;
global lifecycle activation and retirement of the script entry point are pending.
