# Stable native installation ownership

2026-09-09. [installation_metadata.rs](../../crates/harness-core/src/installation_metadata.rs)
consumes the [registration builder](rust-registration-metadata.md) to serialize
stable ownership into `harness/installation.json`, using document schema 2
(distinct from the registration journal's format). Its current core CLI
integration and remaining delivery work are recorded in [core installation](rust-core-installation.md).

The document records bounded machine-local settings, actual metadata-file ID,
link IDs/types/stored targets, ownership and a checksum. Destination kinds/names
must match the installer's fixed allowed paths. Sources may belong to a retained
old checkout, adopted literal alias or derived native build; this layer never
deletes a source target. It does not independently certify build provenance or
PATH settings: the lifecycle must validate those choices before commitment.

Skill destinations use the source directory's name, matching both inventories;
the capability name declared in `SKILL.md` may differ. Native schema-2 publication
and disconnection, and the actual schema-1 inspection CLI, accept this supported
legacy layout. A failing native publication counterexample and passing correction
are retained as `skill-folder-baseline.stdout`, `skill-folder-fixed.stdout` and
`skill-folder-legacy-cli.stdout` in the core installation evidence directory.

Fresh existing links are adopted, while newly staged links are owned. Native
updates compare guarded previous IDs, types and targets with prior ownership.
Legacy import carries the old owned/adopted claims only after matching captured
targets; the old format cannot retroactively prove original object IDs. Replacing
or removing adopted/fresh foreign links is refused. Component selection must
carry retained owned records; silently dropping an existing owned record fails.
Missing owned destinations can now be repaired through desired newly staged
objects, as described below. Silently omitting an owned record remains refused.

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

## Missing owned destination repair

The builder now accepts an absent owned destination when the guarded final view
contains a desired newly staged object at that path. Its actual new identity is
recorded as owned. Reused and replaced objects appear in the previous capture,
so a present foreign replacement still has to match the recorded identity and
is refused if it does not. The metadata ID/SHA witness, adopted-object checks
and omission refusal remain in force. This relies on the private registration
producer's guarded view; arbitrary serialized claims are not creation proof.

An independent read-only review traced that producer, staging, absence recheck,
publication and rollback. The implementation adds only the final-path membership
exception in the native and legacy ownership branches; it changes no journal
or metadata schema. Registration rechecks unguarded absence after the builder
returns and publishes without replacing an occupied destination.

The new acceptance first reproduced four failures in the old strict builder
(the omission counterexample already passed). Eleven metadata tests then passed,
including restoration of file and directory links with new IDs, legacy repair,
retained adopted/owned peers and untouched source contents. A foreign arrival
injected after metadata preparation prevented durable intent and survived.
An injected interruption after link/metadata publication, before completion,
was recovered through the actual registration API to original metadata and an
absent destination. This is injected failure evidence, not a killed-process run.
Present same-target foreign objects remain refused for both reuse and explicit
replacement; omission of an already-absent owned record remains refused too.

Ten existing metadata-builder tests, the existing native final-publication
foreign-arrival test, and six owned-link consumer tests also passed. One
standalone builder fixture remains intentionally ignored. Thus 28 applicable
checks passed, alongside Clippy, Rustfmt and empty explicit Serena diagnostics.
The final-publication race check exercises the unchanged primitive; it is not a
new full-installer race run. Logs and source identities are retained in
`%LOCALAPPDATA%/codex-harness-evidence/installation-repair-ca9b573415ca461f8119040c33933bd5/`:
`before.*`, `metadata.*`, `builder.*`, `foreign_boundary.*`, `owned_links.*`,
`clippy.*` and `acceptance.json`.

```text
cargo test -p harness-core --lib --offline --locked --jobs 1 --target-dir <owned-check-target> installation_metadata -- --nocapture --test-threads=1
cargo test -p harness-core --lib --offline --locked --jobs 1 --target-dir <owned-check-target> registration::metadata::tests -- --nocapture --test-threads=1
```

Intentional removal of an already-absent owned record now has a separate explicit
nomination from the core update plan. The default builder retains its omission
refusal; the explicit path rechecks absence and rejects adopted, unrelated,
duplicate and still-desired nominations. It removes only the old record and
grants no object deletion authority. See the
[current core acceptance](rust-core-installation.md#explicit-retirement-of-absent-obsolete-links)
for completion/rollback, foreign-arrival checks and consumer acceptance.
Full lifecycle orchestration, global activation and task 4.3 remain open.
