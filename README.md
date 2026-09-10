# coding-agents-harness-pack

A reusable public source of coding-agent instructions, skills, MCP connections
and installation tooling. The currently delivered automatic entry point is
Codex CLI on Windows. Compatible `codex-harness` command and package
identifiers remain until a separate migration changes them.

The checkout is the live source of the portable kit. Installation connects those
files through filesystem links. Machine-specific TUI settings, authentication,
session history, caches and recovery copies stay on the local machine. Shared
defaults continue to be read live from this repository.

On Windows, with PowerShell 7.4+, Codex CLI and OpenSpec installed:

```powershell
.\install.ps1 -WhatIf
.\install.ps1
```

Open a new PowerShell terminal and use `codex` from any project. The launcher
selects the shared `harness` profile automatically; an explicit `--profile` or
configuration override retains Codex's normal precedence.

```powershell
.\install.ps1 -Mode Check
.\install.ps1 -Mode Disconnect
```

- [Installation, updates and recovery](docs/installation.md)
- [Documentation map and official contracts](docs/README.md)
- [Confirmed product decisions](docs/project-decisions.md)
- [Full global instruction source](global/principles-of-work.md)
- [Shared configuration](global/harness.config.toml)
- [Managed source and prerequisite inventory](global/kit.psd1)
- [Native Rust commands](docs/rust-native.md)

Clients that start their own executable, such as IDE or app-server integrations,
need their own profile selection. This CLI registration does not configure those
clients. Additional agents or operating systems are not claimed by the current
delivery.
