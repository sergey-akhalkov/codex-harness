## Context

See proposal.md. The observed executor binds its provider on an app-server thread and forwards profile configuration through `-c`; app-server has no `--profile` flag. Native interactive startup already owns shim readiness and build identity checks. The executor host starts its app-server inside an owned Windows Job.

## Goals / Non-Goals

**Goals:** Make executor transport preparation independent of an interactive xAI session and report preparation failure before a model turn starts. Preserve parallel sessions and OAuth helper ownership.

**Non-Goals:** Change token refresh policy without evidence, add model calls for warmup, or promise recovery from every external service/process termination.

## Decisions

- Prepare the shim from the executor's verified provider identity before spawning its app-server. Reuse native launcher lifecycle code. Parsing all Codex config arguments would duplicate configuration resolution and still start the shared shim inside the app-server's owned process tree; the host already knows the provider and can start it outside that tree.
- Replace ordinary child creation with `process_service::spawn`, already used for independent kit services. The shell preflight invokes the xAI launcher inside a temporary Job; starting before the launcher's own Job does not escape that inherited ancestor. Independent service bootstrap preserves the account CPU allowance and avoids inheriting caller pipes. The prior implementation fails a real-process regression when preflight cleanup destroys the shim.
- Validate the manager beside the canonical installed launcher before touching the listener. Use an explicit port argument in the existing shim lifecycle function so native tests can exercise real cold startup on an owned loopback port without touching live sessions.
- Keep local incident times and logs private. Observed network errors had no HTTP status, the replacement shim appeared after retries, and credential storage had not changed during recovery. This supports missing transport; the prior shim's exit cause is not retained and remains unknown.

## Risks / Trade-offs

- A shared service must outlive preflight and executor cleanup → use the existing independent bootstrap and test both temporary caller Job cleanup and peer reuse.
- Fixtures cannot prove historical process termination → retain that limitation separately from deterministic startup regression results.

## Migration Plan

Run focused baseline/candidate checks, native quality checks, then `codex-harness deploy`. Exercise the installed manager from an owned directory outside the checkout on an isolated port. Existing installation recovery remains the rollback path.
