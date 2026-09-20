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

- `--all` also chains the scoped component updates (code tools, token
  workflow, board, subscriptions) and reports each one.
- `--preview` plans without building links.
- `--build`/`--state` pin or place the candidate explicitly.

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
