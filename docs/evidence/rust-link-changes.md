# Native link replacement and removal

2026-09-09. [`LinkChange`](../../crates/harness-core/src/registration_changes.rs)
adds explicit replacement/removal of captured existing links to
[`Registration::apply_with_changes`](../../crates/harness-core/src/registration.rs).
The schema 6 increment bound those changes to the existing link/configuration
intent and its completion marker. The later [finish protocol](rust-registration-finish.md)
introduced schema 7; the current [metadata builder](rust-registration-metadata.md)
uses schema 8. Older journal schemas remain preserved and unsupported.

Capture verifies the current link's exact target, type and object identity.
It does not establish installation ownership: the installer must authorize that
link from its own metadata before requesting a change. The old target can be
unavailable after relocation; it is checked as a recorded name, without reading
or following the old checkout. A replacement target is checked as an available
source. No copy fallback is introduced.

Original-link guards remain held while replacement stages are prepared and the
intent is written. Publication moves the original object into a sibling rollback
name, then publishes the replacement if requested. Both renames preserve the
recorded identity using the [tunneling correction](rust-config-creation.md).
All configuration and link-change conflicts are checked before undo starts.
Rollback restores the original object, including dangling links, and refuses
to reconstruct a missing original merely from its target string.

Actual Windows checks passed:

- Public API: six file/directory replacement or removal combinations, repeated
  application and disconnect; capture of already dangling old links; same-target
  foreign replacement; preservation of configuration and all retained versions
  on an unresolved rollback conflict.
- Injected failure between moving the old object and publishing the new one,
  including a competing same-target link with a different identity.
- Owned Rust process termination at both publication boundaries, followed by
  fresh recovery of the exact original identity, no remaining stages and intact
  source data. This does not rely on Rust destructors in the killed process.
- Missing and replaced rollback objects refuse undo. Restoring the test actor's
  retained original permits recovery; recreating an equivalent link does not.
- An explicit 8.3 test rejects duplicate aliased changes and recovery-state
  overlap, then successfully replaces/restores a distinct requested alias.
- The combined run passed 32 registration/native library checks and 33 public
  integration checks (23 registration, four existing-config, four new-config,
  two link-change tests). The subsequently added rollback-object test and alias
  test passed separately. The prior five explicit alias tests also passed.
  After the target-comparison correction below, 33 library checks, three default
  public link-change tests and all six explicit alias tests passed. Scoped core
  Clippy passed on that corrected source.

```text
cargo test -p harness-core --lib registration --offline --locked --jobs 1 -- --nocapture
cargo test -p harness-core --test registration --test config_file --test config_creation --test link_changes --offline --locked --jobs 1 -- --nocapture
cargo test -p harness-core --test registration aliased_ --offline --locked --jobs 1 -- --ignored --nocapture
```

Evidence root:
`%LOCALAPPDATA%/codex-harness-evidence/link-change-c3c4c277b2044222878b60173208401f/`.
`public.*` retains the failed already-dangling capture, whose implementation
incorrectly required the old target to exist; `public-fixed.*` records the
correction. `recovery.*`, `backup-conflict.*`, `change-alias.*` and `combined-*`
record the other checks. `review-input.json` identifies the historical bounded
review input, before the later tests and target-comparison correction.

Independent review found a P1 rollback defect: an actor could retarget the old
backup object in place to an alias resolving to the original source. Canonical
target comparison accepted that alias during preflight; undo then removed the
candidate and a newly created configuration before exact native restoration
rejected the altered stored target. Both capture and rollback now use the same
non-resolving target comparison as the native link guard.

The default capture regression rejects a different stored target even when it
resolves to the same file. The explicit
`retargeted_backup_alias_is_rejected_before_any_configuration_or_link_undo`
regression passed in 0.12 seconds: it changes the backup reparse data without
changing its object identity and verifies that backup, candidate, configuration
and exact journal all survive rejection. Restoring the original stored target
then permits rollback. This test enables an already available symlink privilege
only in its short-lived Rust test process; it is explicitly opt-in because
[AdjustTokenPrivileges](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-adjusttokenprivileges)
cannot grant a privilege absent from the token. `target-fix.*`,
`target-alias-fixed.*` and `target-fixed-*` retain the corrected checks.
Independent review confirmed the P1 correction and preservation oracle.

Without an explicit finish, successful changes retain their old objects for
disconnect/recovery. The later [finish protocol](rust-registration-finish.md)
retires them after a durable decision bound to nominated metadata. Full installer
Check must still handle temporary names consistently with native capability
discovery. Stable metadata generation, global activation, old-layout migration
and lifecycle acceptance remain open.
