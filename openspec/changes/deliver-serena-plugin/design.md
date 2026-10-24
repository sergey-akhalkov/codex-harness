# Design

## Context

See [proposal.md](proposal.md) for the motivation and [the plugin contract](specs/serena-plugin/spec.md) for observable acceptance. This is a cross-cutting packaging and ownership change, so a design is required.

The inspected installation currently requires a native manager under the core installation, live source data and previously prepared Serena dependencies. `crates/harness-core/src/mcp_preparation.rs` resolves a verified manager link and passes `--source-root` and a host registry into the Serena gateway. `crates/harness-core/src/code_tools_lifecycle.rs` adopts existing packages and refuses activation when Serena is unavailable. `crates/codex-harness/src/mcp_cli.rs` already owns the native serving and semantic-check entry points. These are reusable foundations; their installed-core assumptions are the gap.

The existing marketplace has Ponytail and executors. Executors demonstrates native plugin registration, but its MCP command depends on a global `codex-harness` and its dispatch depends on the harness launcher. Repeating that package shape would not provide independent Serena installation.

The observed host is Codex CLI 0.164.0. Native marketplace/add/list/remove commands were inspected, and the installed executors plugin was listed. No standalone Serena package, clean-host preparation or plugin-cache replacement was exercised during this planning change. The official [packaging guide](https://developers.openai.com/plugins/build/plugins) and [local MCP guide](https://developers.openai.com/plugins/deploy/desktop-only) document bundled resources and package-relative execution; they do not prove compatibility with this installed CLI. Public-directory submission is not required: reuse the existing repository marketplace.

## Goals / Non-Goals

**Goals:** Separate immutable plugin delivery from writable host state, retain one owner for each existing behavior, and make the first usable increment a real installed Serena operation without core or checkout. A source checkout is an explicit development route for this capability; it is not a consumer fallback.

**Non-Goals:** No new broker, language abstraction, package registry, generic installer framework, model call, provider change or background diagnostic workflow. Nuphus stays in its current owner. Other plugins and the remaining source-linked harness are not migrated by this change.

## Decisions

### 1. Publish one native package using the existing marketplace

Use `serena@coding-agents-harness` as the proposed package identity and `.agents/plugins/serena/` as its source-owned package definition. The produced release tree contains the native Windows runtime, manifest/MCP configuration, generated portable data and a small setup/recovery skill. It is delivered through a supported native marketplace source carrying those prebuilt files; the existing repository remains the source of truth. Extend the current build/release owner only enough to produce and verify that release tree, without a second package manager or maintained copy of the source configuration.

The native binary is built by the producer with declared Rust/locked inputs. Reuse the existing executable and library entry points where possible; add a narrower Rust entry point only when removing a core-only dependency requires it. Runtime resolution is package-relative, never a global `codex-harness` PATH lookup. Build identity must describe the packaged runtime and inputs without requiring the originating checkout on the consumer.

The first bounded implementation check installs the actual produced tree through native Codex, with no core in its environment, and invokes its native status/preparation entry point from an unrelated project. Exercise the selected package transport and command interpolation on the supported CLI before migration. Failure blocks dependent packaging claims; it does not authorize local compilation as a substitute. A manifest-only plugin and a ZIP that users must manually unpack/configure do not meet the contract.

### 2. Make explicit setup the only prerequisite-maintenance path

The packaged Rust entry point must start before Python, Serena or a backend exists. It owns setup, read-only Check and recovery, reusing the existing dependency discovery, plan/apply, integrity and transaction owners. The supplied skill resolves the installed executable and invokes this entry point for a request such as preparing code tools; users do not supply internal paths or reconstruct package-manager commands.

Select explicit preparation of missing third-party runtimes rather than unconditionally bundling a second Python/Node installation. Reuse compatible discovered installations first. Where the provisionable Python/Serena path needs missing runtime tooling, setup acquires declared upstream artifacts into its owned user-local lifecycle and verifies them; it must not stop at instructions to install uv/Python/Node manually. Implement only the missing paths for this selected dependency set. Keep the current dependency policy, including explicit compatibility holds, source provenance and shared-consumer preservation. An integrity observation alone is not an upstream authenticity guarantee.

Rust project toolchains remain project prerequisites; their adopted rust-analyzer follows the existing toolchain owner. C++, C# and Pascal remain reuse-only. Do not use this work to provision their compiler/SDK stacks or select additional languages. The current held BasedPyright staging path is a specific implementation gap for a clean Python installation, not grounds to omit Python acceptance.

Before setup, status is actionable without claiming a usable Serena worker. After successful setup, expose the accepted tool set through the next supported native catalogue refresh or fresh session. Check and normal startup never acquire packages. Avoid an additional control MCP/service if the packaged native entry point and skill are sufficient; any setup-status surface must remain callable without the semantic dependencies.

### 3. Keep package data and mutable state separate

Generate the Serena-relevant catalogue and portable defaults from existing repository owners, including `global/code-tools.json` and `global/tool-resources.json`; do not maintain a second language matrix. Packaging excludes Nuphus acquisition and registration while retaining compatible shared resource accounting. Adapt configuration resolution to accept the installed package resources and explicit host state instead of requiring `global/kit.json` or a core installation receipt.

Use the native writable plugin-data location where the supported host provides it, otherwise the existing owned host-local lifecycle location with an explicit plugin identity. Exact host paths remain local. Discovery, prepared dependency selection, ownership receipts, recovery inputs and runtime endpoints are not package contents. Retain existing bounded storage and process ownership; no new report protocol or unbounded version cache is needed.

### 4. Reuse the broker and language integrations unchanged in purpose

Carry forward `crates/harness-core/src/serena_stdio.rs`, `serena_broker.rs`, `serena_shared.rs`, `serena_configuration.rs` and `serena_acceptance.rs` through their existing owners. Preserve project binding, exclusion policy, request identity, truthful diagnostics, resource containment, idle retirement, bounded read retries and non-replay of uncertain edits.

Package version and prepared configuration participate in the existing generation identity. Verify that core and plugin consumers sharing a machine retain compatible account/resource ownership rather than accidentally starting unrelated resource pools. Reuse established runtime-generation retention so an open connection can restart its broker after native plugin cache changes. Retained native artifacts are bounded owned runtime state, not a second global harness installation.

### 5. Separate package updates from dependency activation

Native Codex owns package install/update/disable/remove. Explicit setup/update owns dependency changes and their existing transaction. Native package installation must not be treated as proof that dependencies are prepared or semantic acceptance passed. A new package reports compatible, preparation-required or failed state truthfully; the old usable generation and recovery inputs remain available where needed by open sessions or a failed activation.

Verify native cache replacement and removal against Windows executable locks, including a later broker restart. Prefer existing generation retention rather than preventing native removal or copying an entire mutable installation. Native package removal must not require an uninstall hook to avoid deleting user projects or adopted dependencies. Preserve local retained state and identify it as inactive when the plugin is removed.

### 6. Migrate only the legacy Serena connection

Candidate packaging and semantic acceptance precede cutover. Reuse the existing owned registration/activation journal to identify the old Serena command, preserve rollback, and activate one selected provider. A legacy foreign edit is a conflict, not permission to overwrite it. Avoid exposing two competing Serena providers during the transition; the native plugin enable/selection behavior must be checked with a real existing installation before cutover.

Keep Nuphus and its registry/package/transport state untouched. Retain compatible core/code-tools commands, but make them respect plugin ownership so later install/update/recover/disconnect does not resurrect or remove the migrated provider unexpectedly. A failed migration leaves a usable prior route or explicit retained recovery inputs. Update existing installation and code-tools guides in their owners when delivery is implemented; do not add a parallel installation manual.

## Risks / Trade-offs

- Native package-relative execution or cache behavior differs on the supported CLI: prove the installed first-use and update/remove paths before removing the working connection; record the actual minimum supported version.
- Clean-host runtime provisioning exposes gaps hidden by existing packages: finish the selected preparation path using the current dependency owner, with acquisition/verification failure checks. A manual prerequisite chain is not accepted as completion.
- Shared package updates interrupt another consumer: retain existing ownership, contention and rollback rules; report a held update rather than stopping unrelated work.
- Plugin and legacy services compete for identities or resource limits: exercise coexistence and migrate one owned connection at a time while preserving Nuphus.
- Native removal invalidates an open generation: verify retained runtime/resources and broker restart with real package replacement, and keep ownership-based bounded cleanup.
- Project-specific SDK/build configuration is absent: report the exact language limit; do not turn plugin preparation into a project toolchain manager.

## Migration Plan

1. Produce and install the standalone native package; demonstrate status without semantic dependencies, explicit preparation and a real Python operation outside the checkout. Add Rust verification with its project toolchain. Keep the legacy integration active for its existing consumers during this isolated work.
2. Complete retained language/process/recovery coverage, repeatability, package update/removal and generation retention through the independent consumer. Run existing real integration checks through the packaged path.
3. Verify owned legacy cutover and rollback in a separate consumer, including preservation of Nuphus, foreign settings and later core lifecycle operations.
4. Publish the verified consumer package through the existing marketplace's release route, update the owning guides and delivery decisions, and verify fresh native Codex consumption outside this checkout. Publication authority and credentials follow the existing release boundary; planning performs no publication.

Record source/package/backend identities, actual checks and environment limits in existing verification/release owners. Keep incomplete requirements open. No claim that another physical PC passed may be inferred from an isolated-home test on the development machine.
