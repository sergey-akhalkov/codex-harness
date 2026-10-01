## Why

An observed `deploy --preview` without `--build` entered immutable build preparation before producing a plan, even though a verified installed build was already available. Repeated candidate reviews may pay for compilation before deciding whether deployment is useful; the existing explicit-build preview already demonstrates a narrower path.

## What Changes

- Reuse an eligible existing verified build for an implicit-build deployment preview through the existing selection owner, without compiling merely to inspect the plan.
- Show the selected build identity and its relationship to the requested source so a preview cannot masquerade as verification or delivery of newly compiled code.
- Preserve explicit build selection, ordinary deployment checks, component previews, ownership guards and the existing preparation path when no eligible build is available.
- Measure the change on the separately specified executor-watch workload before deciding whether to adopt it. Include preparation and checking costs and retain an inconclusive or rejected result when reuse does not help.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `linked-global-kit`: allow planning-only deployment previews to reuse a verified existing build while retaining source/build identity and delivery safeguards.

## Impact

The affected owner is native deployment/build selection, principally `crates/codex-harness/src/install_cli.rs`, its existing native deployment tests, the `harness-deploy` skill and the installation guide. This adds no dependency or model/provider change. Experimental implementation and the benefit decision remain separate from mainline integration and live publication.
