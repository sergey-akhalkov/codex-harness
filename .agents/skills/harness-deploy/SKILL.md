---
name: harness-deploy
description: Deliver, update, verify or repair a global codex-harness installation from its kit checkout. Use when kit changes must reach consumer sessions, when the installed launcher is older than kit instructions, or when install/update refuses because recorded ownership no longer matches.
---

# Harness deployment

One action delivers the kit. The authoritative detail lives in the kit's
[installation guide](../../../docs/installation.md); this skill is the
operating procedure.

## Everyday delivery

From the kit checkout, after the applicable checks pass:

```powershell
codex-harness deploy --source <ABSOLUTE-KIT-CHECKOUT>
```

The command builds an immutable candidate (reusing an unchanged verified
build), installs or updates the core connection with user-default homes,
verifies the installed launcher, and prints one receipt. `--source` must be
absolute. Read the receipt: `status`, `build_identity`, and
`verification.executor_probe_ok` must reflect the new build before you call
the delivery done.

- Ordinary `deploy` is the release/publication route (fresh target, fat LTO).
  Never use its candidate for a `--fast` request or a fast candidate for
  publication.
- `--all` also chains the scoped component updates (code tools, token
  workflow, board, subscriptions) and reports each one.
- `--preview` plans without changing the installation. Without `--build`, it
  reuses an eligible verified build from the same checkout without compilation;
  otherwise normal preparation applies. Check `build_selection` and
  `build_check` in the receipt: an older build's plan does not verify new source.
- `--build`/`--state` pin or place the candidate explicitly.

## Local fast delivery

Use the explicit fast profile only for local iteration:

```powershell
codex-harness deploy --source <ABSOLUTE-KIT-CHECKOUT> --fast
```

The receipt's top-level `profile` must be `delivery-fast`; ordinary deployment
must report `release`. Fast and release candidates do not substitute for each
other, and an unavailable fast candidate or mismatched explicit build is an
error before installation changes. Fast candidates are local delivery output,
not publication candidates, and release deployment keeps its fresh-target
fat-LTO path. Both retain the normal immutable-build, ownership, integrity and
installed-runtime checks.

A valid source-owned delivery declaration compiles the exact declared set in
one Cargo invocation for either profile; a missing or invalid declaration uses
the two-phase manager query. Fast preparation may reuse one keyed,
installation-owned compiler target with a lease, conservative changed-input
timestamp fallback and seven-day inactivity retirement. Release never acquires
it. Do not hand-build, edit or delete fast-target entries; the full ownership,
foreign-state and retirement lifecycle belongs to the
[installation guide](../../../docs/installation.md#fast-local-delivery).

Both profiles now use four Cargo jobs after matched no-contention measurements
(release 1/2/4 jobs: 531/442/408 s phase sum, 3.10/2.54/2.62 GiB peak). Retained
2026-10-22 single runs: 4.2 s unchanged verified reuse without Cargo; 117.7 s
four-job fast cold matrix and 119.3 s real four-job fast deployment; 410.8 s
earlier one-job fast; 675.4 s two-phase fallback. The four-job samples meet the
360 s cold target but are not repeatability proof or evidence for the 120 s
leaf or 20-60 s single-binary targets. Fast compiled-input reuse for
noncompiled test/documentation changes is implemented; release still requires
the full source identity. Test-feedback measurement found no benefit from a
runner or scoped serialization and retained one deterministic pre-existing
token-audit failure. Applied delivery with both profiles present,
interruption/recovery and delivery outside the checkout remain unvalidated.

Preview and applying delivery keep their source, build, profile and ownership
checks; `--fast --all --preview` reports the fast core and component plans
without changing installation state. It reuses an eligible verified build
without Cargo and otherwise follows normal preparation. Preview does not
certify changed source or authorize the later apply.
For a test- or fixture-only change, run focused Cargo checks; deploy only when
delivery or installed behavior is under test. Read the full command, cache and
limitation detail in the
[installation guide](../../../docs/installation.md#fast-local-delivery).

## Repairing a blocked installation

When `install`/`update` refuse with ownership or identity conflicts, do not
edit links by hand and do not repoint binaries at working-tree builds - that
is exactly what breaks consumer sessions and blocks the lifecycle. Run:

```powershell
codex-harness deploy --source <ABSOLUTE-KIT-CHECKOUT> --reset [--all]
```

Reset removes only the recorded owned objects that are reparse points or
already missing, preserves and reports every regular file, writes a receipt
next to the installation metadata, then performs a normal fresh install.
Afterwards `update --preview` must validate without ownership conflicts.

## Rules

- Never launch consumers from `target/` builds or retarget `harness/bin`
  links out of band; immutable builds only.
- `install`/`update`/`check` never compile; only `deploy`/`build` run Cargo.
- On machines without MSI desktop PowerShell, tests need
  `HARNESS_ACCEPTANCE_POWERSHELL` pointing at the owner-designated pwsh.
- Recovery verbs stay available: `codex-harness recover`, `recover-build`,
  and the reset receipts under `<CODEX_HOME>/harness`.
