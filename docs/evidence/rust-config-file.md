# Native existing configuration publication

2026-09-09. [`ConfigSnapshot`](../../crates/harness-core/src/config_file.rs)
captures an existing ordinary file's exact bytes and NTFS object identity, then
releases its handles. The [feature editor](rust-feature-edit.md) can prepare a
candidate in an owned home without holding the user's configuration locked.
Its explicit `publish_to` checks the preparation's private original bytes against
that snapshot. Public receipt hashes cannot authorize different input.

Publication acquires the existing native registration guard, with write access
requested only for this operation. It checks the file identity and original
bytes again, writes and truncates through the same transaction handle, flushes,
closes the data handle, and commits. A returned snapshot guards a subsequent
operation or explicit rollback. A competing replacement with identical contents
is rejected; rollback also refuses a later foreign change.

The underlying local NTFS protection rejects reparse ancestors, final symlinks,
multiple hard links and incompatible preexisting writers. Writes remain bounded
to 16 MiB; native feature input retains its stricter 1 MiB bound. Snapshot Debug
does not print file content. Exclusive creation of a previously absent config is
later recorded in [native configuration creation](rust-config-creation.md).
Installation metadata migration and full installer orchestration remain open.
This increment does not close tasks 4.1–4.3 or activate a global native installer.

Microsoft documents transactional behavior for both
[WriteFile](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-writefile)
and [SetEndOfFile](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setendoffile).
The [TxF programming contract](https://learn.microsoft.com/en-us/windows/win32/fileio/programming-considerations-for-transacted-fileio-)
describes rollback of content changes and closing file handles before commit.
As with native registration, TxF is deprecated and optional: unavailable support
fails closed, and these results do not establish support on other filesystems.

Actual Windows checks passed:

- 24 native protection tests, including new isolation and abrupt termination
  cases. The process test kills the owned Rust fixture both with its data handle
  open and after that handle closes, while its transaction remains live. The old
  bytes, length and identity survive without relying on Rust destructors.
- Four public configuration tests cover Unicode paths, empty/short/128 KiB
  publication, explicit rollback, preserved file attributes, changed bytes,
  same-content foreign replacement, symlinks, hard links, redirected parents,
  read-only files and size limits.
- The existing 18 registration integration tests passed after the shared
  regular-file reader changed, including interrupted registration recovery.
- The actual CLI 0.153.4 feature acceptance passed in 55.29 s: disabled Hooks,
  repeated application and Code Mode candidates are now published to an owned
  config and rolled back. A mismatching preparation is rejected. The malformed
  native input still retains its private failure without leaking its sentinel.
- Scoped core library/test Clippy passed. Separate-volume protection retains its
  previous explicit acceptance; it was not replayed for the shared reader change.

```text
cargo test -p harness-core --lib registration_native --offline --locked --jobs 1
cargo test -p harness-core --test config_file --offline --locked --jobs 1
cargo test -p harness-core --test registration --offline --locked --jobs 1
cargo clippy -p harness-core --lib --tests --offline --locked --jobs 1 -- -D warnings
```

The native feature invocation and executable identity are in the owning feature
record. Its retained evidence directory now includes `publication-native.stdout.txt`
and `publication-native.stderr.txt`; model-free preparation and publication used
only owned homes. This is process-interruption evidence, not a simulated power
failure or a full installation recovery result.

A separate Astra senior review inspected the publication path and changed shared
reader and found no concrete P0/P1 issue. It did not replay tests. Injected
write/truncate/flush failures, commit failures and power loss were not exercised;
the retained process tests establish completed staging followed by termination.

The subsequent configuration-aware registration journal is implemented in
[`Registration::apply_with_configs`](../../crates/harness-core/src/registration.rs).
`FeatureEdit::plan_for` supplies a change bound to its private preparation input.
Schema 4 covers original/candidate bytes, file identities and source-link
records in the same checksum; the completion marker binds the whole intent.
Repeated application retains the original rollback bytes. Disconnect explicitly
restores recorded configurations, so a caller chooses this rollback policy.
Older native intents are preserved with an error, not silently upgraded.

The registration test suite now passes 22 integration tests. Two additional
library cases exercise failure after the first configuration, a foreign change
before the next publication, and an actual killed Rust process between the two
files. Recovery preflights all recorded configurations before undoing any: a
foreign edit preserves the already published file, link and journal until the
owned test conflict is explicitly resolved. The combined library filter passes
26 tests; three ignored entries are explicit fixture/second-volume entry points.

Actual native feature preparation plus journaled publication/disconnect passed
in 55.36 s (`journal-native.stdout.txt` in the feature evidence directory).
`journal-protection.stdout.txt` retains the combined protection results. Scoped
Clippy and format checks passed. At this schema-4 journal boundary the
installation PATH, build selection, component lifecycle and script-layout
metadata migration still needed orchestration. Later exclusive creation of
previously absent files is owned by
[native configuration creation](rust-config-creation.md). Tasks 4.1–4.3 remain
incomplete.

The configuration-journal review then reproduced a P1 through the public API:
long and 8.3 spellings of one file passed the textual overlap check, the first
candidate was published, and the second record blocked both apply and recovery.
The original Rust reproduction is retained at
`%TEMP%/harness-config-alias-review-78f3a66d93e94d3a8f8679e05505f9d8/`.
Both planning and journal loading now reject duplicate retained file identities,
independently of path spelling. This invariant also rejects a validly checksummed
duplicate-identity intent before any recovery write.

The exact 8.3 public-API regression passed on this machine, preserving the
original bytes and creating neither intent nor a source link. It is explicit
because it needs an existing 8.3 alias; the test does not enable filesystem
features. Its evidence is `%TEMP%/harness-rust-registration-config-8dot3-t4tDY2/`
and `alias-fix.stdout.txt` in the feature evidence directory. The three affected
configuration library tests (including process interruption and foreign conflict)
and scoped Clippy passed after the correction.

```text
cargo test -p harness-core --test registration aliased_configuration_paths --offline --locked --jobs 1 -- --ignored --nocapture
cargo test -p harness-core --lib configuration_tests --offline --locked --jobs 1 -- --nocapture
```

The schema-4 journal and existing-file 8.3 identity checks above remain this
record's evidence. They are not a source receipt for later schema-5 files.
Follow-up destination-alias and recovery-state alias defects, with their
retained logs, are in [native registration](rust-registration.md). New-file
creation evidence lives only in the later creation record.
