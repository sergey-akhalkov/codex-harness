# Stable native installation ownership

2026-09-09. [installation_metadata.rs](../../crates/harness-core/src/installation_metadata.rs)
consumes the [registration builder](rust-registration-metadata.md) to serialize
stable ownership into `harness/installation.json`, using document schema 2
(distinct from registration journal schema 8). The internal component is not yet
connected to the complete install/update/Check CLI.

The document records bounded machine-local settings, actual metadata-file ID,
link IDs/types/stored targets, ownership and a checksum. Destination kinds/names
must match the installer's fixed allowed paths. Sources may belong to a retained
old checkout, adopted literal alias or derived native build; this layer never
deletes a source target. It does not independently certify build provenance or
PATH settings: the lifecycle must validate those choices before commitment.

Fresh existing links are adopted, while newly staged links are owned. Native
updates compare guarded previous IDs, types and targets with prior ownership.
Legacy import carries the old owned/adopted claims only after matching captured
targets; the old format cannot retroactively prove original object IDs. Replacing
or removing adopted/fresh foreign links is refused. Component selection must
carry retained owned records; silently dropping an existing owned record fails.
Missing-owned-object repair is not implemented by this strict builder.

The prior metadata file is bound by its ID and whole-file SHA-256, including all
original bytes, through the builder's already held snapshot. The first attempt
reopened that file inside the callback and failed with Windows sharing error 32.
The corrected immutable witness avoids the redundant open; tests also reject a
stale semantic snapshot paired with a fresh destination snapshot, or a false
fresh-install claim over an existing metadata file.

Six actual NTFS/registration tests passed: fresh apply/finish/read, reuse/update
and owned removal with adopted/source preservation; refusal to modify fresh
foreign and recorded adopted links; schema/checksum/host/destination/content and
same-byte metadata replacement conflicts; legacy conversion preserving both
ownership categories and metadata ID; same-target foreign link replacement and
omitted-owned-entry refusal; stale prior-byte and false fresh-claim rejection.
No production/home registry or global installation writes occurred. Transitional
launcher kinds remain representable for migration and do not prove retirement.

```text
cargo test -p harness-core --lib installation_metadata --offline --locked --jobs 1 --target-dir <owned-check-target> -- --test-threads=1 --nocapture
cargo test -p harness-core --lib registration::metadata --offline --locked --jobs 1 --target-dir <owned-check-target> -- --test-threads=1 --nocapture
cargo clippy -p harness-core --lib --tests --offline --locked --jobs 1 --target-dir <owned-check-target> -- -D warnings
```

The final run also repeated all eight owned-key PATH tests after correcting two
malformed Unicode test inputs. Six ownership tests, ten builder tests and those
eight PATH tests passed sequentially; Clippy passed. The exact root and source
hashes are selected by `%TEMP%/harness-current-installation-metadata-evidence.txt`:
`%LOCALAPPDATA%/codex-harness-evidence/installation-metadata-e6708fc214e9463c994c320223b8657b/`.
`tests.*` and `recheck-cause.*` retain the sharing failures;
`witness-fixed.*` records the first correction; `final-*` identifies acceptance.
Full lifecycle/global activation and the remaining migration tasks remain open.
The later [PATH rename correction](rust-environment-path.md#rename-correction-and-review)
has its own twelve-test acceptance and source receipt.
