## 1. Preconditions

- [ ] 1.1 Confirm the migrate-harness-to-rust 8.2 batch that deletes the graphify-named sources has landed: `git ls-tree -r HEAD --name-only | rg -i graphify` returns no hits and the working tree shows no pending graphify deletions. If it has not landed, coordinate with that change's owner instead of re-deleting; record the pending state in the retirement evidence

## 2. Native dependency lifecycle removal

- [ ] 2.1 Remove the Graphify uv verification branch (module, console entry and package identities), the `("mcp","graphify")` identity mapping, the `@dreamtree-org/graphify` exclusion and the shared-service manifest read from `crates/harness-core/src/dependency_discovery.rs`, and drop the `graphify_manifest` request/discovery field; verify `rg -i graphify crates/harness-core/src/dependency_discovery.rs` returns no hits and `cargo test -p harness-core` passes
- [ ] 2.2 Remove the Graphify dispatch arms from `crates/harness-core/src/dependency_plan.rs` and `crates/harness-core/src/dependency_fetch.rs`, re-pointing the affected test fixtures to retained ids while keeping normalized-name coverage; verify `cargo test -p harness-core` passes and both files are graphify-free under `rg -i`
- [ ] 2.3 Remove the `--graphify-manifest` flag, its help text and the `PROGRAMDATA/OpenCodeWorkstation/manifest.json` default from `crates/codex-harness/src/dependency_cli.rs`, and the Graphify mention from the apply note in `crates/harness-core/src/dependency_apply.rs` and the apply help in `crates/codex-harness/src/dependency_apply_cli.rs`; verify the `dependencies` and `dependencies apply` CLI help output contains no graphify text and `cargo test -p codex-harness` passes
- [ ] 2.4 Delete the manifest-secret test in `crates/codex-harness/tests/dependency_discovery.rs` together with the removed feature; replace the `graphify` id in the `dependency_npm_install.rs` rejection test with a retained non-npm id; update the retired-tool comment in `crates/codex-harness/tests/support/installed_tool_workflows.rs`; verify both crates' tests pass and those files are graphify-free under `rg -i`

## 3. Retirement guard and host state

- [ ] 3.1 Keep the `GRAPHIFY` name in the owned-registration skip/conflict lists and retired projections (`crates/harness-core/src/codegraph_registration.rs`, `crates/harness-core/src/codegraph_integration.rs`) and add a short comment pointing to the retirement record and its removal condition; verify `cargo test -p codex-harness codegraph_install` and `cargo test -p codex-harness codegraph_consumers` pass unchanged, including the graphify-absence assertions
- [ ] 3.2 Remove `bootstrap-graphify-pending.json` from `unfinished_restart_blockers` in `crates/harness-core/src/subscription_lifecycle.rs`; verify `cargo test -p harness-core` passes and the file no longer mentions graphify
- [ ] 3.3 Record the guard, its rationale and the removal condition (no supported host state can predate the 2026-09-15 retirement) in `docs/evidence/legacy-mcp-retirement.json`; verify the file parses (`Get-Content -Raw | ConvertFrom-Json`) and the graphify entry describes the final action

## 4. Documentation and evidence

- [ ] 4.1 Remove the Graphify prerequisite sentence from `docs/installation.md`; verify the file contains no graphify reference
- [ ] 4.2 Update the external-tool list and the pending-status paragraph in `docs/rust-native.md`; verify the file contains no graphify reference
- [ ] 4.3 Reconcile `docs/code-tools.md` with the removed proxy/update sources (no rollback-source claim), fix the stale acceptance wording, and keep only a minimal retired-tool note; verify every remaining mention is that note
- [ ] 4.4 Update the note and the "four MCP interfaces" row in `docs/evidence/rust-requirement-checks.json`; verify the file parses and mentions graphify only as retired-and-removed
- [ ] 4.5 Confirm historical records stay untouched: `git diff --name-only` for this change touches no file under `openspec/changes/archive/`, no `docs/evidence/rust-migration-map.json` and no `docs/project-decisions.md`

## 5. Verification and handoff

- [ ] 5.1 Validate the change with `openspec validate remove-graphify-remnants`; verify it reports no errors
- [ ] 5.2 Run the native checks from docs/rust-native.md for both crates (fmt, clippy `-D warnings`, tests); verify clean
- [ ] 5.3 Run `codex-harness ownership-check --source` and verify it does not report a graphify-owned path
- [ ] 5.4 Record the reference sweep: `git grep -i graphify` and `git ls-files | rg -i graphify`; verify no graphify-named files and that every remaining hit falls into the declared allowlist (archives, dated inventory, decision records, retirement evidence, the registration guard and its tests, the minimal documentation note, and - until the cutover - the transitional `tools/` layer with its owning change named); record the result in the retirement evidence
- [ ] 5.5 After the migrate-harness-to-rust cutover (9.2) lands, re-run 5.4, update the residue list in the retirement evidence, and verify only historical records and the registration guard remain
