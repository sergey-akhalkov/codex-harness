## Why

The harness maintains Python, PowerShell, JavaScript, C# and Rust across its runtime, installer, integrations and tests. The user selected Rust for all code owned by this repository to reduce implementation and maintenance fragmentation and support efficient execution; third-party tools may retain their own languages and runtimes.

## What Changes

- Complete the harness Rust migration covering maintained runtime code, installation and recovery tools, tests, executable fixtures, build helpers and skill helpers. This full migration commitment applies to `codex-harness`; the separately confirmed global Rust programming and PowerShell shell defaults apply in other projects without requiring unrelated rewrites.
- Consolidate active first-party functionality into a Cargo workspace, reusing the existing RTK adapter and exposing native CLI entry points with shared ownership, process and configuration primitives.
- **BREAKING**: Replace `install.ps1`, PowerShell launch/check entry points and script-based helper commands with documented native executable commands. Migrate existing installations transactionally; retain ordinary `codex` usage, accepted capabilities, explicit overrides and recoverability.
- Preserve MCP tools, subscription routing, source diagnostics, bounded resource ownership, explicit dependency provisioning, RTK and skill workflows. Replace integrations with foreign language internals through verified external interfaces; generating scripts or moving harness-owned code into a nominal dependency does not satisfy the migration.
- Port behavioral acceptance coverage to Rust, retire obsolete code only after consumer checks, remove supported first-party non-Rust execution paths, and reconcile manifests and documentation.
- Deliver and verify the migrated kit globally from outside this checkout, including upgrade from the existing installation, repair, relocation, disconnect and rollback. Measure runtime effects against the same scenarios; a language change alone is not evidence of acceleration.

## Capabilities

### New Capabilities

- `rust-native-harness`: First-party Rust ownership boundary, native workspace and commands, compatibility-preserving migration, foreign-tool integration boundaries and complete acceptance.

### Modified Capabilities

- `linked-global-kit`: Native bootstrap/entry points, explicit build freshness for compiled code, continued direct source configuration and reversible global migration.
- `harness-source-diagnostics`: Native installer/check commands replace PowerShell command names while preserving bounded, private, model-free diagnostic behavior.

## Impact

Affected surfaces include `install.ps1`, `tools/`, executable helpers under `.agents/skills/`, `tests/`, the kit manifest and global command registrations, local repository guidance and installation/verification documentation. Rust build outputs and machine-local state remain outside tracked reusable configuration. Third-party packages, their own runtimes, native Codex/OpenSpec, credentials, model selection, other projects' language stacks and unrelated OpenSpec work remain separate concerns; their accepted behavior must be preserved. Initial platform support remains native Windows. Implementation must account for the current dirty tree and concurrent changes rather than assuming the exploration snapshot is frozen.
