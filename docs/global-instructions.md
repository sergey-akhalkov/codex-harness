# Global working principles

[Documentation map](README.md) · [Canonical text](../global/principles-of-work.md) ·
[Working-principles specification](../openspec/specs/global-working-principles/spec.md)

Principles enter the initial context of new Codex sessions through the host
global `AGENTS.md`. That file is a symbolic link to the repository-owned source.
A separate copied body is not maintained. `CODEX_HOME` stays the native Codex
home; this checkout is not used as the authentication or session store.
Connecting the link must not edit local `config.toml`, authorization or model
settings.

Official discovery: [AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md)
and [environment variables](https://learn.chatgpt.com/docs/config-file/environment-variables).

## Update and scope

Edit the canonical file in the repository. A new session reads the current text
through the live link; a separate install is not required after each edit. An
already running session must be started again to guarantee an updated initial
context. Creating a child agent from an already open parent does not reload the
parent's initial instructions.

A later global `AGENTS.override.md` would take precedence over global
`AGENTS.md`. Project instructions add to the hierarchy and may refine the
common rules. Another account or another `CODEX_HOME` needs its own connection.
The link depends on checkout location; after a move, reconnect it to the new
existing source. A Markdown link inside `AGENTS.md` does not include another
document: this connection uses a filesystem link.

The [kit installer](installation.md) recognizes a correct existing principles
link as already connected and preserves it on disconnect. Other platforms remain
separate work. If links are unavailable, fix the cause; copy fallback is
excluded.

## Language defaults and early delivery

Rust is the default programming language and PowerShell is the default shell in
every project, including delegated work. See
[project decisions](project-decisions.md#language-and-shell-defaults).
Model-free `codex debug prompt-input` confirmed that the current source loads
exactly once outside this checkout, with project `AGENTS.md` appearing after
the global text when present.

The early end-to-end delivery philosophy is in the same canonical source and
[project decisions](project-decisions.md#outcome-quality-and-speed). A fresh
child `middle` created from a new parent session can state those rules from
already loaded instructions without tools. That check confirms instruction
loading, not later product builds or unfinished Rust work.

## Reconnect

Create the link only when both global instruction files are absent. Existing
files need a separate composition decision.

```powershell
$principlesSource = Join-Path <checkout> 'global/principles-of-work.md'
$codexConfigRoot = Join-Path $env:USERPROFILE '.codex'
$globalInstructions = Join-Path $codexConfigRoot 'AGENTS.md'
$globalOverride = Join-Path $codexConfigRoot 'AGENTS.override.md'

if (-not (Test-Path -LiteralPath $principlesSource -PathType Leaf)) {
    throw 'Principles source is missing.'
}
foreach ($instructionPath in @($globalInstructions, $globalOverride)) {
    if (Get-Item -LiteralPath $instructionPath -Force -ErrorAction SilentlyContinue) {
        throw "Existing instructions require composition: $instructionPath"
    }
}
New-Item -ItemType SymbolicLink -Path $globalInstructions -Target $principlesSource
```

To roll back, confirm the active object is the expected symbolic link, then
remove only that link:

```powershell
$principlesSource = Join-Path <checkout> 'global/principles-of-work.md'
$globalInstructions = Join-Path $env:USERPROFILE '.codex\AGENTS.md'
$instructionLink = Get-Item -LiteralPath $globalInstructions -Force -ErrorAction Stop
if ($instructionLink.LinkType -ne 'SymbolicLink' -or
    [string]$instructionLink.Target -ne $principlesSource) {
    throw 'The active instructions differ from this activation; inspect before changing.'
}
Remove-Item -LiteralPath $globalInstructions
```

The operation removes the filesystem link; the repository source remains. Later
sessions stop receiving principles through that global file. The pack's own
root `AGENTS.md` still routes to the same source for work on this repository.

Loading checks use `codex debug prompt-input` without a model call. They
confirm initial context, not that a model will follow every rule in later tasks.
