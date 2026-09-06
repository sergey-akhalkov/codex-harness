# Codex Harness

A portable configuration and capability kit for Codex CLI. Managed sources stay
in this checkout: the full global instructions, configuration, skills, agents and
connection tools are read through direct filesystem links.

The shared configuration is writable through its link: native TUI model changes
and machine-specific settings, including trusted project paths, may be saved
directly in this repository. This behavior is explicitly accepted.

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

Installation registers links and a user command-path entry. The local base
`config.toml`, authentication and session state stay on the machine. There is no
artifact-copy mode.

- [Installation, prerequisites, updates and recovery](docs/installation.md)
- [Documentation and project decisions](docs/README.md)
- [Full global instruction source](global/principles-of-work.md)
- [Shared configuration](global/harness.config.toml)
- [Managed source and prerequisite inventory](global/kit.psd1)
- [Verification evidence and supported limits](docs/linked-kit-verification.md)

The supported automatic entry point is the local Codex CLI command in Windows
PowerShell. Clients that start their own executable, such as IDE/app-server
integrations, need their own profile selection; this CLI registration does not
claim to configure those clients.
