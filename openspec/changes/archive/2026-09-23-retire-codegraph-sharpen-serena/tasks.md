## Tasks

- [x] 1.1 Remove `codegraph` from the managed MCP selection in
       `global/code-tools.json`; remove the Codebase Memory resource row from
       `global/tool-resources.json`.
- [x] 1.2 Delete first-party CodeGraph code: serving/broker/scheduler/
       observer/runtime/catalogue/response/generation/store/transport modules,
       `dependency_codegraph`, the fixture binary and their tests.
- [x] 1.3 Delete Codebase Memory rollback code (`cbm_*` modules, manual stdio
       route, probe kind, tests) and Graphify remnants.
- [x] 1.4 Rename the shared journal/planner to `mcp_registration` /
       `mcp_preparation` with `apply-registration` / `prepare-mcp` CLI names;
       keep retired-name removal for owned legacy registrations.
- [x] 1.5 Hide `search_for_pattern` in the Serena proxy catalogue with the
       documented reason and keep the unfiltered escape hatch.
- [x] 1.6 Rewrite the MCP routing instructions: portable principles, bounded
       retrieval recipes and the code-tools guide lose all CodeGraph routing,
       route literal text to `rg`, and add concrete Serena-first
       navigation/edit recipes.
- [x] 2.1 Update affected unit and integration tests (registration retirement
       over pre-retirement state, hidden `search_for_pattern`, discovery,
       planning, staging, probing).
- [x] 2.2 Run the full native workspace checks and documentation checks, and
       update the owning evidence records (retirement record, executable
       ownership, migration map/checks). Every codex-harness suite passes;
       the three remaining harness-core failures predate this change and are
       tracked by 2.3.
- [x] 2.3 Re-run the three pre-existing single-thread harness-core failures
       (`build_selection::activation_reuse_stale_repair_and_immutable_artifacts`,
       `core_install::preview_checks_build_and_foreign_destinations_without_creating_homes`,
       `native_launcher::run_reaps_detached_session_helper_and_returns_exit_code`)
       once machine state is clean; they reproduce on a clean HEAD checkout,
       so they are outside this change. They did not clear on their own: each
       was a stale expectation against a newer contract (stale checkouts keep
       the delivered build launchable, the preview link set grew with the
       binary set, and an ordinary launcher exit preserves upstream-managed
       background processes). The tests now assert those contracts, and the
       full single-thread harness-core suite passes 703/703.
- [x] 3.1 Deliver through the explicit code-tools lifecycle on this machine
       and verify a fresh process no longer discovers a managed CodeGraph
       registration while Serena and Nuphus remain callable.
       Evidence: `deploy --source <checkout> --all` reports `deployed` with
       `executor_probe_ok` and a healthy code-tools component (builds
       `eeed235…` then `9da1919…`); the installed `check --code-tools-only`
       reports inventory `[serena, nuphus]` and registration status
       `retired`; the live `config.toml` owns only `serena` and `nuphus`
       after removing the pre-retirement `codegraph` entry; and the real
       registry serena_stdio handshake check passes.
