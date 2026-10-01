## Context

See [proposal.md](proposal.md). The observed source path in `install_cli::deploy` calls `native_build::prepare` before branching on `preview` when `--build` is absent. The explicit-build route already avoids that preparation. Existing installation/build owners distinguish artifact integrity, source freshness, selection and applying effects; this change must reuse those distinctions.

The ordinary scenario is reviewing an experimental candidate's deployment plan using an already prepared installation. The counterexample is a candidate that will immediately require a fresh build for application anyway: moving that work later may yield no end-to-end saving. A first installation or an unusable existing build also receives no promised saving.

## Goals / Non-Goals

The goal is a complete preview with attributable build identity and less unnecessary preparation in the supported review scenario. Changing model selection, build formats, applying deployment authority, ownership repair or independent acceptance is outside this treatment.

## Decisions

Reuse verified existing selection for the implicit-build preview, through the existing owner, and retain ordinary preparation when reuse is unavailable. Explicit `--build` already offers the lower-cost path; requiring callers to rediscover and spell that path on every review was considered. A bounded automatic preview branch makes the available behavior reliable while keeping the selected identity visible. Do not create a second build cache or selection record.

Integrity is mandatory for reuse; source freshness remains a separate reported fact. An older intact runtime can support planning only when the existing preview owner can produce the requested plan honestly. Never suppress a component or a missing prerequisite to claim a faster result. Applying deployment retains its stricter current source/build checks, and preview cannot grant removal or publication authority.

Keep candidate A's implementation separate from the frozen source and specification of workload B (`settle-executor-watch-finalization`). Both arms receive the same task, independent native oracle and operating conditions. The immutable controller and checker remain outside candidate writes. The actual B assignment includes an owned installation preview; reuse must be demonstrated through the installed arm actually consumed, not inferred from a patch or a smaller file.

Declare the quantitative comparison policy in private run inputs before either measured arm, including meaningful effect, tolerated variation, coverage, stopping and repeated-selection rules. Record preparation, failed attempts, checking and shared costs once. Narrow claims to the observed planning/review scenario; adoption of a broader efficiency claim requires applicable corroboration. An inapplicable or neutral result remains inconclusive or rejected rather than becoming a synthetic saving.

## Acceptance

Exercise the real native deployment entry point in owned isolated installation state with a verified prepared build. A changed source plus a compilation marker must show that an eligible implicit preview does not invoke compilation, preserves installation state, and reports the actual selected build and source relationship. Explicit build selection, unavailable/altered build handling, `--all`/`--reset` preview and applying freshness/ownership guards must retain their declared behavior. Existing applicable deployment checks, formatting, scoped clippy and source/link hygiene must pass.

Complete the real A-on-B comparison with the qualified local agent/tool path and unchanged frozen workload oracle. Retain independently checked B solutions, original failures, actual metrics and the supported benefit decision. A correct preview implementation alone does not prove benefit, close the parent continuous-loop acceptance, or authorize syncing unadopted requirements into the main specification.

## Risks / Trade-offs

- A stale runtime could be confused with fresh-source verification: report both identities and preserve the independent applying guard.
- First-use preparation or later applying work can erase the predicted saving: retain those costs and limit the claim to the exercised scenario.
- A workload may call its own working-tree binary rather than the selected installed runtime: verify consumption and treat inapplicable treatment as unsupported evidence.

## Migration Plan

Implement and check the experimental branch, freeze its identity, and run the declared comparison before benefit-gated integration. The existing installer owns any later live publication and recovery. Retain rejected artifacts under their board owner without synchronizing the experimental delta.
