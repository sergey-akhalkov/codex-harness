#requires -Version 7.4
[CmdletBinding()]
param([Parameter(Mandatory)][ValidateSet('serena','codebase-memory','graphify','nuphus','harness-lsp')][string]$Server)
$ErrorActionPreference = 'Stop'
$hostRoot = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex' }
$registryPath = if ($env:HARNESS_CODE_TOOLS_REGISTRY) { $env:HARNESS_CODE_TOOLS_REGISTRY } else { Join-Path $hostRoot 'harness/code-tools.json' }
if (-not (Test-Path -LiteralPath $registryPath -PathType Leaf)) { throw 'Code tools are not resolved. Run the kit installation before starting this MCP.' }
$inventory = Get-Content -LiteralPath $registryPath -Raw | ConvertFrom-Json
$serena = @($inventory.mcp | Where-Object id -EQ 'serena')
if ($serena.Count -ne 1 -or -not (Test-Path -LiteralPath $serena[0].paths.python -PathType Leaf)) { throw 'The registered shared Serena Python is unavailable. Run install.ps1 -Mode Check.' }
& $serena[0].paths.python -B -u (Join-Path $PSScriptRoot 'code-tools/launch.py') $Server --registry $registryPath
exit $LASTEXITCODE
