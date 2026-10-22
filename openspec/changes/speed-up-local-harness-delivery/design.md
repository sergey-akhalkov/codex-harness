## Context

See [proposal.md](proposal.md) for the measured problem. The current native
publication path owns source identity, a fresh compiler target, release
profile, delivery selection, final dep-info coverage and immutable candidate
publication in `native_build.rs` and `native_handoff.rs`. The build record
requires the `release` profile, and ordinary deploy selection assumes that
profile. `linked-global-kit` owns the one-action deployment and preview
contract.

The release route intentionally uses a fresh target because Cargo freshness is
mtime-based. A changed input with a restored timestamp must not reuse a stale
artifact. Any fast cache therefore needs a separate freshness decision; it
must not relax release publication. The source-owned delivery set currently
comes from the freshly compiled manager, which is why the second Cargo phase
exists.

## Goals / Non-Goals

**Goals:**

- Make local fast delivery an explicit, measured option with a warm compiler
  target and a one-invocation delivery build.
- Keep the complete delivery set, immutable final artifacts, source identity,
  compiled-input coverage, installed-runtime checks and recovery.
- Make fast and release candidates impossible to confuse in selection,
  receipts, diagnostics and recovery.
- Avoid unnecessary delivery builds for test-only changes without path-based
  exclusion of real compiled inputs.

**Non-Goals:**

- Replacing fat LTO for public release or claiming seconds for a cold release.
- Adopting thin LTO without new evidence; the retained comparison found it
  slower and larger.
- Adding a background cache service, external compiler cache or new scripting
  runtime.
- Consolidating unrelated integration tests or changing test semantics without
  a separate measured decision.
- Reusing a candidate across changed lockfile, manifest, toolchain, Cargo
  configuration or compiled-input bytes.

## Decisions

### Explicit fast profile and selection

Add a maintained `delivery-fast` profile inheriting `release` with release
optimizations, `strip = true`, `lto = "off"` and `codegen-units = 16`. The exact
codegen and job values remain measurement-controlled; the profile values and
effective compiler invocation are recorded so a later change is visible.

Expose the mode as `deploy --fast`. Ordinary `deploy` continues to select and
prepare the release profile. Fast and release selection are separate:
`delivery_from`, preview, reuse candidates and `build_identity::check` receive
the requested profile instead of assuming `release`. Fast candidates are
stored immutably like release candidates but cannot be selected by ordinary
release deployment or preview. A missing fast profile is an error, not a
fallback.

Alternative considered: make no-LTO the default and add `--release`. Rejected
for now because it silently changes the runtime characteristics of the
everyday installation and weakens the public/release distinction.

### Persistent fast target with a conservative freshness guard

Fast builds use one persistent target under an owned short compiler-cache root,
with an ownership record, lease and last-successful-build record. The target
key includes the compiler identity, target triple, profile, lockfile, manifests,
Cargo host configuration and other declared build-affecting inputs. It does not
include ordinary workspace source bytes, so Cargo can rebuild changed crates.

Before reusing the target, compare the current source identity with the last
successful fast build. If any changed compiled input has an mtime that is not
newer than the recorded build epoch, or if the target key/ownership is
ambiguous, use the ordinary fresh target. The first implementation SHOULD use
the conservative fresh fallback rather than try to map every suspicious path to
a Cargo package; scoped cleaning can be added only with direct evidence. This
keeps the existing changed-bytes/restored-timestamp guarantee. The fast target
is never used by ordinary release publication.

Alternative considered: reuse one target for both profiles. Rejected because
release freshness, profile separation and cache retirement have different
consequences; a shared cache would make an accidental fast selection harder to
detect.

### Source-owned delivery declaration

Put the delivery program set in a single source-owned declaration in the root
Cargo manifest (for example `[workspace.metadata.harness] delivery = [...]`).
The pre-compilation manager reads that declaration and selects the full set in
one Cargo invocation. `harness-core` retains the runtime list used for build
identity and the delivery query, with a validation test that it matches the
manifest declaration. If the declaration is missing or unusable, the existing
two-phase source-manager query remains the compatibility fallback. This
preserves upgrades where a new source adds a delivery program without trusting
an older installed producer's list.

Alternative considered: infer delivery programs from `cargo metadata` target
names or a `fixture` naming convention. Rejected because Cargo exposes all
binaries and no reliable production/fixture marker; a naming heuristic could
silently publish the wrong set.

### Separate compiled-input evidence from checkout provenance

The build record continues to retain the full source identity for provenance
and release checks. Finalization also retains the union of compiled paths from
every delivered binary's dep-info, with hashes and the applicable build inputs.
A fast candidate may be reused when all compiled inputs and build-affecting
inputs match even if noncompiled test or documentation files changed. Reuse
does not rewrite the prior immutable build; it creates a new selection record
referencing the same verified binary hashes and the current checkout
relationship. Missing or ambiguous dep-info forces a rebuild.

This is safer than excluding paths such as `tests/` or `fixtures/` by name,
because those files can be real compiler inputs. It also avoids the earlier
objection that a dep-info-derived key is unavailable before a build: the first
build records the evidence, and later reuse is allowed only when that evidence
is complete and matching.

### Test and job measurement

Keep the existing test coverage and dev profile. Measure fast builds at two and
four Cargo jobs, plus queue wait and peak memory, before choosing the fast
default. The measured payload improved at four jobs, but an unrelated shared
heavy holder made total wall time worse; the fast default is chosen from
matched no-contention runs. Test execution improvements remain a separate
measured route; scoped serialization or a different runner cannot replace
coverage and is not required for delivery speed.

## Risks / Trade-offs

- **Stale artifact after a preserved timestamp** -> use the conservative fresh
  target on any suspicious changed input and retain a regression test.
- **Fast candidate accidentally selected by ordinary deploy** -> profile-aware
  selection, preview filtering, receipts and tests with both candidates
  present.
- **Fast runtime is slower or larger** -> explicit opt-in, size/startup and a
  representative runtime comparison before adoption; public release remains
  fat LTO.
- **Persistent target grows or becomes foreign** -> owned cache record, lease,
  inactivity check, scoped retirement and foreign-state preservation.
- **Long MSVC object paths** -> use the existing short owned compiler-root
  pattern rather than a deep state path.
- **Delivery declaration drifts** -> one manifest owner, runtime-list
  validation and two-phase fallback for legacy sources.
- **Shared heavy queue erases payload gains** -> report queue wait separately,
  keep the existing admission budget and adopt a job count only on matched
  evidence.

## Migration Plan

1. Extend the build record and candidate selection with a profile enum while
   treating a missing legacy profile as `release`.
2. Add the fast profile, owned target, freshness guard and one-invocation
   delivery declaration behind the explicit `--fast` selector.
3. Verify fast preparation, selection, installed delivery and recovery in an
   owned isolated state; verify ordinary release deployment with both profile
   candidates present and with a fast candidate newer than release.
4. Deliver the documentation and skills, then verify the installed fast and
   release paths outside the checkout.

Rollback is to stop selecting the fast profile and remove its target/records;
the release profile remains unchanged. Fast immutable builds left in owned
state are not selected by ordinary release deployment.
