---
name: skill-evolution
description: Create, update, shorten, consolidate or retire owned skills from observed work. Use when maintaining the skill library after a verified procedure, failed skill use, or overlap. Skip facts, hypotheses, existing tools, and routine or read-only tasks.
---

# Skill evolution

Use the available built-in `skill-creator` to draft a candidate in an explicit staging path outside discovery. Do not overwrite `skill-creator`. Do not write the user's global link slot. Do not stage, commit or push.

## Modes

- `create` — a reusable owned procedure with a checkable result
- `update` / shorten — same identity, changed body or narrower applicability
- `consolidate` — replacement that must keep both original suites
- `retire` — compare keeping the skill with removing it; disable from discovery first

## What is not a skill

A project fact belongs in the owning record. A tentative idea stays tentative. A command that already has a tool or documented entry point is not a new skill. Routine success and read-only exploration are no-ops.

Read [routing examples](references/routing.md) when the observation is ambiguous. Read [ownership](references/ownership.md) before choosing a target.

## Gate

Candidates stay outside discovery. Activation requires the skill-evaluation decision (`accept` / `reject` / `inconclusive`) and the existing `Registration` publication primitive. Reject or inconclusive keeps the previous library. Provider unavailability is inconclusive; do not switch models silently.

After publish, read the accepted `SKILL.md` from its canonical path (or `codex-harness skills identity --path`) before the next use. Catalogue presence, a file write, or a compact continuation is not delivery. A skill added mid-turn may be missing until the next user turn on hooks-off CLI; do not restore ordinary hooks. Do not claim already loaded tokens were refunded.

On CLI 0.155.0 with ordinary hooks off: a new skill can appear on the next user turn, after idle `/compact`, on resume, and in a new child without fork. Same-turn continuation and an already-running child do not inject the add into the catalogue; they can still read the live `SKILL.md` or run `codex-harness skills identity --path`. Disablement and delete still leave the name in that session's catalogue. Observe disablement with `skills identity --path --codex-home` (`enabled` false, not `delivery_complete`) or `skills usage --codex-home` (`disabled`). A missing path fails identity and never refunds tokens. Custom descriptions are not in the injected catalogue.
