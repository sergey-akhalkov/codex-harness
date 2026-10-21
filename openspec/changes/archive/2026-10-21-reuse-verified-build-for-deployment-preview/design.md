## Context

See [proposal.md](proposal.md). The observed source path in `install_cli::deploy` calls `native_build::prepare` before branching on `preview` when `--build` is absent. The explicit-build route already avoids that preparation. Existing installation/build owners distinguish artifact integrity, source freshness, selection and applying effects; this change must reuse those distinctions.

The ordinary scenario is reviewing an experimental candidate's deployment plan using an already prepared installation. The counterexample is a candidate that will immediately require a fresh build for application anyway: moving that work later may yield no end-to-end saving. A first installation or an unusable existing build also receives no promised saving.

## Goals / Non-Goals

The goal is a complete preview with attributable build identity and less unnecessary preparation in the supported review scenario. Changing model selection, build formats, applying deployment authority, ownership repair or independent acceptance is outside this treatment.

## Decisions

Reuse verified existing selection for the implicit-build preview, through the existing owner, and retain ordinary preparation when reuse is unavailable. Explicit `--build` already offers the lower-cost path; requiring callers to rediscover and spell that path on every review was considered. A bounded automatic preview branch makes the available behavior reliable while keeping the selected identity visible. Do not create a second build cache or selection record.

Integrity is mandatory for reuse; source freshness remains a separate reported fact. An older intact runtime can support planning only when the existing preview owner can produce the requested plan honestly. Never suppress a component or a missing prerequisite to claim a faster result. Applying deployment retains its stricter current source/build checks, and preview cannot grant removal or publication authority.

The user decision of 2026-10-21 removes the mandatory model-backed comparison for this change. Recover applicable retained implementation and integrate it against current source. Direct native acceptance and installed consumption establish the supported behavior; no comparison on `settle-executor-watch-finalization` or benefit-gate outcome is required for this delivery. This does not close that workload or the parent improvement-loop acceptance, alter other experiments, or establish end-to-end time/token savings.

## Acceptance

Exercise the real native deployment entry point in owned isolated installation state with a verified prepared build. A changed source plus a compilation marker must show that an eligible implicit preview does not invoke compilation, preserves installation state, and reports the actual selected build and source relationship. Explicit build selection, unavailable/altered build handling, `--all`/`--reset` preview and applying freshness/ownership guards must retain their declared behavior. Existing applicable deployment checks, formatting, scoped clippy and source/link hygiene must pass.

Deliver the checked implementation through the existing immutable-build installation lifecycle and verify the actual installed preview outside the source checkout. Preserve unrelated settings and installation ownership. Retain applicable prior evidence without treating it as current execution; record the current checks and delivery result in the existing task owner. Accept this bounded behavior without a claim about overall task efficiency.

## Risks / Trade-offs

- A stale runtime could be confused with fresh-source verification: report both identities and preserve the independent applying guard.
- First-use preparation or later applying work can erase the predicted saving: retain those costs and limit the claim to the exercised scenario.
- A check may call a working-tree binary rather than the installed runtime: verify the delivered identity and exercise the installed entry point outside the checkout.

## Migration Plan

Recover and integrate the retained implementation, run the affected native acceptance and hygiene checks, then deliver through the existing installer and verify installed consumption. Preserve retained experimental artifacts and any historical adverse evidence; do not rewrite an absent comparison into a successful result. Synchronize the accepted behavior through the normal OpenSpec lifecycle.
