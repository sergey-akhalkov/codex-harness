## 1. Portable sources and first consumer

Accepted clarification (2026-09-06): the user permits machine-specific settings
in the shared repository configuration. Native `/model` and TUI project-trust
writes through the linked profile are accepted behavior. Authentication,
history, caches and installer state remain local. See
[verification evidence](../../../../docs/linked-kit-verification.md).

- [x] 1.1 Add the repository-owned source inventory and shared `global/harness.config.toml`, preserving current reusable preferences and Full Access while excluding secrets/runtime state; verify each selected key against the installed CLI and record the managed-key/dependency inventory. The initial profile need not migrate existing trust paths; subsequent native machine-specific settings are permitted.
- [x] 1.2 Include the full global AGENTS source, all six existing OpenSpec skills and their resources, the agent source location and all install/launch tools in the declared kit; verify every source/resource reference resolves from a fresh checkout with only documented external prerequisites.
- [x] 1.3 Prove native profile and base-config coexistence with the new shared source through a disposable file link and the actual `codex --profile harness debug prompt-input`; verify source edits appear on the next launch, and document native write targets for ordinary configuration writers/model selection/project trust, including accepted writes into the repository, while preserving the link and local authentication/history/cache state before host activation.

## 2. Default CLI entry point

- [x] 2.1 Implement a repository-owned transparent launcher with automatic profile selection for supported session commands, explicit user-profile precedence and native management-command delegation; verify representative argv boundaries, flags before subcommands, spaces, non-ASCII paths, stdin/stdout/stderr, exit codes, cancellation and recursion prevention.
- [x] 2.2 Implement linked user-command registration and recovery of the original CLI identity without copying launcher source; verify ordinary `codex` resolution in a fresh PowerShell process, default profile behavior for TUI/exec/review/resume/fork, and unchanged help/version/management dispatch.

## 3. Connection lifecycle

- [x] 3.1 Implement installer path resolution, source inventory validation and non-mutating preview; verify alternate accounts/checkout paths, missing prerequisites, unsupported CLI contracts, profile-name conflicts and unavailable link privilege fail clearly without copies or changes to user state.
- [x] 3.2 Connect the full global instruction source, profile, skill folders and namespaced agent directory through direct links; verify the existing matching principles link is recognized, unrelated global capabilities remain discoverable, and conflicting files/links/names or AGENTS overrides are preserved and reported.
- [x] 3.3 Add local ownership records and idempotent reconciliation; verify a second install creates no duplicate registration/PATH entry and adding/removing source artifacts updates only matching kit-owned links.
- [x] 3.4 Add bounded rollback and disconnect; inject a failure after an activation mutation and verify restoration, original error reporting, preservation of externally changed targets, original CLI usability and no traversal/deletion of source targets.
- [x] 3.5 Add check and reconnection for inaccessible/moved checkout or moved original CLI; verify stale paths are reported, repair from a new location preserves unrelated state and ownership conflicts prevent unsafe replacement.

## 4. Integration and portability evidence

- [x] 4.1 Exercise the installed entry point in a neutral directory and a separate repository with project AGENTS.md; verify effective shared/local/project configuration, Full Access, full global instructions and managed skill discovery from repository paths, including the harness's own duplicate-discovery case.
- [x] 4.2 Exercise a harmless repository agent fixture through actual Codex discovery and consumption alongside an unrelated user agent; verify edits and referenced resources are read from the live source, then remove the fixture and report the empty/real agent catalogue accurately.
- [x] 4.3 Exercise a clean-checkout installation and lifecycle matrix in disposable host paths with spaces/non-ASCII characters; verify live content updates without reinstall, explicit profile/config overrides, conflict behavior, failed activation, relocation, disconnect and preservation of local configuration/authentication/session data.
- [x] 4.4 Run a bounded review of actual global mutation and CLI forwarding paths for concrete data-loss, state-leakage and command-execution failures; resolve material findings and rerun the affected checks before activation.

## 5. Real activation and documentation

- [x] 5.1 Preview and activate the verified kit on the current host without replacing unrelated state, then verify the real user's ordinary `codex` entry point outside this repository; record source paths, effective defaults, global instruction/skill loading and actual CLI/PowerShell versions, without treating disposable probes as this activation.
- [x] 5.2 Document install, prerequisites, direct-source inventory, new-terminal behavior, update/reconnect/disconnect and CLI-only entry-point limits; update owning project records and check local links plus attribution to current official contracts.
- [x] 5.3 Reconcile evidence against every requirement/scenario, confirm all implementation checks and cleanup outcomes are recorded, and run `openspec validate --all --strict`; close tasks only for completed work and leave any unmet requirement explicitly unfinished.
