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
error before installation changes. The retained 2026-10-22 cold scope was a
617.8 s release baseline and 2m34s for the fast profile at two Cargo jobs;
the current native command uses one job, so these are not a matched
same-job promise. This revision has no persistent fast target or
one-invocation delivery. Preview and applying delivery keep their source,
build, profile and ownership checks; preview does not certify changed source.
For a test- or fixture-only change, run focused Cargo checks; deploy only when
delivery or installed behavior is under test.
Measured scope, cache lifecycle, limitations and the test-only no-deploy rule
are owned by the [installation guide](../../../docs/installation.md#fast-local-delivery).

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
