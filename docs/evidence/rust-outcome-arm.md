# Native outcome arm preparation

2026-09-09. `codex-harness outcome-arm --request PATH` prepares `baseline` or
`candidate` skill configuration and verifies it through native discovery. The
[command](../../crates/codex-harness/src/outcome_arm.rs) consumes the existing
[registration/configuration publication](rust-config-file.md) and a new
[read-only installation link guard](../../crates/harness-core/src/installation_links.rs).
It introduces no installation registry. This completes another part of task 7.3;
suite preparation, independent oracles, remaining consumers and global migration
are still unfinished.

The strict JSON request requires `arm`, `case_root`, `codex_home`, `user_home`,
`dependency_user_home`, `source_root` and the original native `upstream` executable.
Optional `timeout` defaults to 30 seconds per discovery (1–300 allowed). Case,
Codex home and user home must be existing temporary directories outside the
checkout; the case and private evidence must be outside the installation homes.
The source and installation homes use explicit ordinary Windows drive paths,
not `\\?\` request spellings. `dependency_user_home` is a read-only metadata
identity input. No prompt, model call, profile or routing override is exposed.
The selected file profile must satisfy the [native base discovery boundary](rust-outcome-discovery.md).

Both skills must first have installer records marked owned and live directory
links to their expected kit sources. The caller acquires the existing installation
mutex for the selected user home and transfers it to `OwnedSkillLinks::read`.
The public API requires that lock to match the home; it does not infer homes.
The private receipt includes skill names, source/destination paths and assurance:

| Metadata | Verified link identity |
| --- | --- |
| Schema 1 | Owned record plus current live capture; no historical object ID existed |
| Schema 2 | Stored target, directory type and object ID match the live link |

The guard rejects absent/unowned/adopted records, mismatching homes/source and
legacy `harness/pending.json`. It holds the actual link objects against replacement
and rechecks metadata bytes/object identity. Native metadata fields remain
crate-private; only a read-only accessor was added. This is not a snapshot of all
kit inputs or a completed installer lifecycle.

Preparation snapshots the ordinary base config, profile and discovered candidate
skill documents. Every discovered registration of `project-verification` and
`reproduce-regression` must receive the selected enabled state, including duplicate
names at different locations. Each recorded source must appear in native discovery.
The command appends native `[[skills.config]]` entries, preserves the original text,
and refuses an existing opposite override rather than rewriting a previously used
arm. Case-insensitive Windows path comparison handles ordinary/canonical inventory
spellings. Repeating the same preparation does not rewrite the config.

Publication uses the existing exact-object transaction/journal, including exclusive
creation when the base config was absent. Acceptance requires a second successful
native discovery: all selected registrations have the intended state, every other
skill is unchanged, and native configuration outside `skills.config` is unchanged.
The executable hash, profile, candidate skill documents and installation guard
are rechecked. Any subsequent verification failure requests exact guarded rollback;
later foreign data is preserved and reported as a recovery conflict.

JSON and evidence remain private because they include full paths and native config.
`started.json`, before/after discovery, publication state, `arm.json` and any private
failure/rollback failure are retained outside the case. Successful preparation keeps
its reversible publication journal for the caller. An interrupted process can leave
a prepared temporary case; an absent success receipt is not acceptance. These tests
do not claim complete case recovery after process termination or power loss.

Verification passed on the current Windows/NTFS host:

- Six installation guard tests use actual schema 1 records and schema 2 metadata
  created through the existing production registration/metadata builder. They cover
  both assurance levels, absent/unowned/wrong source, same-target object replacement,
  active link protection, changed metadata and bounded/duplicate skill names.
- Four configuration tests cover all registrations, preserved text, repeat/conflict,
  Windows path spellings, invalid inputs and changed unrelated discovery/config.
- Five actual CLI integration tests cover both arms, repeated application, absent
  config, failed native verification, unrelated skill/provider changes, preserved
  foreign config, unowned/missing links and a valid same-name skill from another
  source. Failed preparation restores exact original bytes or absence when safe.
- An explicit model-free installed Codex 0.153.4 test passed for both arms with the
  real guard held, preserving installation metadata. It copied no authentication.
  The two-arm test took 58.89 seconds; this is not a speed comparison.
- Core/CLI Clippy, scoped formatting and explicit Serena error/warning diagnostics
  passed. These checks do not activate the global native installation.

```text
cargo test -p harness-core --lib installation_links --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo test -p codex-harness --bin codex-harness outcome_arm --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo test -p codex-harness --test outcome_arm --offline --locked --jobs 1 --target-dir <owned-check-target> -- --nocapture --test-threads=1
cargo test -p codex-harness --test outcome_arm installed_native_confirms_baseline_and_candidate_in_owned_homes --offline --locked --jobs 1 --target-dir <owned-check-target> -- --ignored --nocapture --test-threads=1
cargo clippy -p harness-core -p codex-harness --all-targets --offline --locked --jobs 1 --target-dir <owned-check-target> -- -D warnings
```

The native test requires `HARNESS_NATIVE_CODEX` set to the explicit original
executable identified in the discovery record. Commands run from this checkout.
Private evidence is retained in
`%LOCALAPPDATA%/codex-harness-evidence/outcome-arm-1757f8d3341e48279fed9884b87a95e3/`.
`config-unit.*` preserves the initially wrong TOML value parser and its failing
positive case; `config-unit-fixed.*` passes using document deserialization.
`guard-initial.*` and `guard-fixed.*` retain corrected test expectations about
Windows spellings and exclusive link handles. `guard-accepted.*`, `arm-accepted.*`,
`arm-native.*` and `clippy-final.*` contain accepted results. Native raw arm roots
are `%TEMP%/codex-outcome-arm-u5FNfm/` and `%TEMP%/codex-outcome-arm-fvuY37/`.
