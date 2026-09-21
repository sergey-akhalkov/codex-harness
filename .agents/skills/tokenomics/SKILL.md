---
name: tokenomics
description: Diagnose measured token usage across local Codex rollout sessions with the first-party token-audit analyzer, map findings to their owning records, and record accept/reject decisions in a baseline loop. Use when the user asks what consumed tokens across sessions or whether an optimization idea is worth adopting. Do not use for runtime output discipline (token-efficient-workflow owns that) or for subscription delegation accounting.
---

# Tokenomics

Run the analyzer from its installed location before reading any rollout file
by hand. Every number it reports is a recorded rollout value or carries an
explicit basis; never convert its output into costs, quota percentages or
subscription savings.

1. **Report**: `token-audit report --days N --format text` for the measured
   overview (sessions, projects, models, efforts, days, cache efficiency,
   context repayment, instruction floors, coverage warnings).
2. **Findings**: `token-audit findings --days N` ranks measured-mass findings
   with evidence locators and owning records; `--all-bases` also shows
   inferred or estimated findings, which the default filter hides on purpose.
3. **Map, do not duplicate**: each finding names its owner (for example the
   token workflow record, delegation guide or subscription models record).
   Apply remediation in that owner; never write parallel efficiency
   instructions here.
4. **Decide with a baseline**: before adopting an idea, run
   `token-audit baseline save`; after the trial period, run
   `token-audit baseline diff` and compare movement per session, project,
   model and day. Record the accept/reject decision and its evidence in the
   owning record, not in a new report.

Findings are eligibility for planning, not implementation authority: an
adopted change still needs its normal OpenSpec or owning-record route. The
analyzer stays local: it reads `CODEX_HOME/sessions`, hashes project
identities by default, and never emits transcript content.
