# Proposal

## Why

Using Serena on another PC currently requires a live harness checkout, a locally built native manager and separately prepared dependencies. The owner wants ordinary installations to consume ready-to-use, versioned Codex plugins, with a live checkout reserved for development; Serena and its existing language integrations are the next delivery boundary.

## What Changes

- Deliver a standalone Serena plugin through the existing `coding-agents-harness` marketplace. Include a prebuilt Windows x64 Rust runtime, the required portable data and preparation guidance; ordinary use must not require the harness core, its launcher, a harness checkout, symlink privileges or compilation of harness source.
- Provide one explicit initial preparation operation reachable through the plugin. Its native entry point must work before Python or Serena is installed, reuse verified compatible dependencies, prepare missing distributable prerequisites, and report real readiness. Normal startup and read-only checks do not install or update packages.
- Preserve the current Serena tool selection, project isolation, resource limits, recovery, diagnostic uncertainty and accepted languages: Rust/Python, plus existing reuse-only C++, C# and Pascal/Delphi support. Project toolchains, licensed SDKs and project-specific build inputs remain project responsibilities.
- Make the native Codex plugin lifecycle own package installation, version selection and removal. Keep dependency/runtime state outside replaceable package files, preserve open sessions across updates, and migrate only owned legacy Serena connections without duplicate providers or lost recovery.
- Update the source-link contracts for this migrated capability: ordinary consumers use the selected released package; developers may explicitly use a source checkout. Other harness components retain their present delivery until separately migrated.
- Leave Nuphus on its existing lifecycle. Do not create a Nuphus plugin, alter executors or provider routing, expand the language set, enable automatic diagnostics, or build a general package manager.

## Capabilities

### New Capabilities

- `serena-plugin`: Independent packaged delivery, explicit preparation, ordinary project use, native package lifecycle, safe legacy migration and evidence from installed consumers.

### Modified Capabilities

- `global-code-tools`: Deliver Serena through its plugin while preserving Nuphus's existing integration, useful operations, startup isolation and runtime-generation guarantees.
- `linked-global-kit`: Permit versioned plugin packages for migrated capabilities and distinguish their update semantics from optional source-linked development.
- `rust-native-harness`: Keep reproducible Rust builds as a producer/developer responsibility while allowing ordinary consumers to install verified plugin artifacts without the harness build prerequisites.

## Impact

Reuse the existing Serena broker, stdio gateway, dependency discovery/planning/apply, resource ownership and verification code in `crates/harness-core`, and the native entry points in `crates/codex-harness`. Affected packaging owners include `.agents/plugins/marketplace.json`, a new `.agents/plugins/serena/` package, `global/code-tools.json`, release/build inputs and targeted CI. Installation/code-tools guidance and affected lifecycle tests must follow the new consumer path; external OpenSpec workflows remain untouched.

The first implementation boundary is an installed package whose native preparation entry point starts without core, checkout, Python or a harness compiler, followed by a real Serena operation in an outside project. Native package transport, package-relative execution and runtime provisioning are specific feasibility checks, not completed evidence. Broad migration waits for that usable path. This proposal authorizes no implementation or publication.
