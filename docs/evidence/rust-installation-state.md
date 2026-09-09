# Native legacy installation inspection

2026-09-09. [LegacyInstallation](../../crates/harness-core/src/installation_state.rs)
reads schema 1 `harness/installation.json` through an exact ordinary-file
snapshot. The native `inspect-installation` command holds the
[installation lock](rust-installation-lock.md), validates the requested Codex,
user and dependency homes, then reports only installation paths and ownership
counts. An absent file returns JSON `null` without creating homes.

The parser preserves the distinction between `owned: true` links and adopted
links. It validates supported destination/kind pairs, source containment in the
recorded checkout, duplicate destinations, PATH scope and bounded metadata.
The old checkout/upstream can already be absent after relocation; neither is
executed or followed during inspection. Unrecognized schemas/fields, malformed
JSON, host/dependency mismatch, reparse metadata/parents and any existing legacy
pending transaction fail closed with the input preserved. Version strings and
metadata bodies are omitted from ordinary output and Debug.

`verify_unchanged` rechecks both the original file identity and exact bytes.
Legacy metadata is a recorded ownership claim, not an object identity receipt:
the later installer must still verify/capture each live link before mutation.
This implementation does not yet translate pending legacy transactions, migrate
metadata into stable native ownership, modify PATH or install/disconnect links.
The subsequent [native ownership consumer](rust-installation-metadata.md) now
verifies an isolated legacy conversion; complete migration orchestration remains open.

Three real CLI integration tests passed: absent homes and relocated inputs;
unsupported/foreign/duplicate/oversized/private malformed records; pending
recovery, reparse objects and concurrent identity/byte changes. The tests verify
that retained originals and foreign targets survive every rejected operation.
Scoped Clippy passed; explicit Serena diagnostics returned no errors/warnings.

```text
cargo test -p codex-harness --test installation_state --offline --locked --jobs 1 -- --nocapture
cargo clippy -p harness-core -p codex-harness --lib --bin codex-harness --test installation_state --offline --locked --jobs 1 -- -D warnings
```

The actual read-only command was also run from `%TEMP%` against the connected
home. It reported 19 legacy links: 18 owned and one adopted, with User PATH scope
and the existing dependency owner. The exact installation metadata SHA-256 was
unchanged before/after. This excludes global activation and does not certify the
contents or health of those linked capabilities.

Evidence:
`%LOCALAPPDATA%/codex-harness-evidence/installation-state-496851aee64247f58e09005bfd7196b0/`.
The local pointer `%TEMP%/harness-current-installation-state-evidence.txt` selects
the concrete run containing `actual.*`, `actual-receipt.json`, `tests.*` and
`clippy.*`; the final source receipt identifies the inspected native increment.

## PATH planning prerequisite

[path_plan.rs](../../crates/harness-core/src/path_plan.rs) now computes insertion
and removal of the managed bin entry, preserving unrelated text and empty entries.
It uses Windows
[environment expansion](https://learn.microsoft.com/en-us/windows/win32/api/processenv/nf-processenv-expandenvironmentstringsw)
and [ordinal comparison](https://learn.microsoft.com/en-us/windows/win32/api/stringapiset/nf-stringapiset-comparestringordinal).
The read-only precedence check rejects an earlier `codex.ps1`, `.exe`, `.cmd`,
`.bat` or `.com` without executing it. Inputs/expansion are bounded; unresolved
relative entries before the bin and explicit remote paths refuse a clean result.
This covers PATH order, not shell functions/aliases or current-directory search.

Two native tests passed with case/quoted/slash/Unicode spellings, `%SystemRoot%`,
empty/unrelated entries, inert competing files, a linked external directory and
a directory named `codex.exe`. The proposed bin was never created. The final
run used an independent Cargo test output directory after a retained shared-target
test executable caused Windows linker error LNK1104; that failure was preserved.

```text
cargo test -p harness-core --lib path_plan --offline --locked --jobs 1 --target-dir <owned-verification-directory> -- --nocapture
```

Evidence is selected by `%TEMP%/harness-current-path-planning-evidence.txt`, with
the original `tests.*` failure and passing `isolated.*` logs. These functions do
not change registry/process environment values. [Native PATH publication](rust-environment-path.md)
now provides a separately verified exact-value transaction; full journal and
installer consumption remain unfinished.
