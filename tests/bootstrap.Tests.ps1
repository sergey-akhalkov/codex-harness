#requires -Version 7.4
# Explicit opt-in: downloads official uv/CPython/Serena into a disposable user
# home, then uses native uv uninstall to recover those exact owned installations.
[CmdletBinding()]
param([switch]$RunProvisioning, [switch]$IncludeGraphify)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $RunProvisioning) { throw 'Use -RunProvisioning for the explicit disposable package-installation test.' }
$repository = Split-Path $PSScriptRoot -Parent
$state = Get-Content (Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex/harness/installation.json') -Raw | ConvertFrom-Json
$pwsh = (Get-Command pwsh).Source
$root = Join-Path ([IO.Path]::GetTempPath()) ('harness-bootstrap-' + [guid]::NewGuid().ToString('N'))
$fixtureUser = Join-Path $root 'user'
$fixtureCodex = Join-Path $root 'codex'
$priorPath = $env:Path
$assertions = 0
function Assert-True([bool]$Value, [string]$Message) { if (-not $Value) { throw $Message }; $script:assertions++ }
try {
    $env:Path = (Split-Path $pwsh) + ';' + (Join-Path $env:SystemRoot 'System32')
    $module = Import-Module (Join-Path $repository 'tools/activation.psm1') -Force -PassThru
    Import-Module (Join-Path $repository 'tools/code-tools.psm1')
    $runtime = Get-HarnessCodeToolsRuntime $fixtureUser $fixtureCodex $state.codexCommand
    Assert-True (-not $runtime.uv -and -not $runtime.python -and -not $runtime.lifecycle_python) 'Controlled fixture was not initially missing uv/Python.'
    $preview = Invoke-HarnessCodeTools -SourceRoot $repository -UserHome $fixtureUser -CodexHome $fixtureCodex -CodexCommand $state.codexCommand -Mode Install -Preview
    Assert-True ($preview.status -eq 'preview-bootstrap') 'Missing dependency did not produce bootstrap preview.'
    Assert-True (-not (Test-Path $root)) 'Bootstrap preview wrote fixture state.'
    $check = Invoke-HarnessCodeTools -SourceRoot $repository -UserHome $fixtureUser -CodexHome $fixtureCodex -CodexCommand $state.codexCommand -Mode Check
    Assert-True ($check.status -eq 'degraded' -and -not $check.callable) 'Missing Serena Check was not honestly degraded.'
    $runtime = & $module { param($UserPath,$ConfigPath,$Native) Initialize-CodeToolsRuntime $UserPath $ConfigPath $Native } $fixtureUser $fixtureCodex $state.codexCommand
    Assert-True ($runtime.python -and (Test-Path $runtime.python)) 'Explicit bootstrap did not produce Serena Python.'
    Assert-True ($runtime.uv.StartsWith($fixtureUser, [StringComparison]::OrdinalIgnoreCase)) 'Missing uv was not installed into the disposable shared user root.'
    $version = & $runtime.python -B -c 'import importlib.metadata; import mcp; print(importlib.metadata.version("serena-agent"))'
    Assert-True ($LASTEXITCODE -eq 0 -and $version -eq '1.7.0') 'Bootstrapped environment does not provide compatible Serena/MCP.'
    $again = & $module { param($UserPath,$ConfigPath,$Native) Initialize-CodeToolsRuntime $UserPath $ConfigPath $Native } $fixtureUser $fixtureCodex $state.codexCommand
    Assert-True ($again.python -eq $runtime.python -and $again.uv -eq $runtime.uv) 'Repeated bootstrap did not reuse the exact installation.'
    if ($IncludeGraphify) {
        $withGraphify = & $module { param($UserPath,$ConfigPath,$Native) Initialize-CodeToolsRuntime $UserPath $ConfigPath $Native -Tool graphify } $fixtureUser $fixtureCodex $state.codexCommand
        $graphifyVersion = & $withGraphify.graphify_python -B -c 'import importlib.metadata; import graphify.serve; print(importlib.metadata.version("graphifyy"))'
        Assert-True ($LASTEXITCODE -eq 0 -and $graphifyVersion -eq '0.9.55') 'Bootstrapped Graphify MCP environment is unavailable.'
        $reusedGraphify = & $module { param($UserPath,$ConfigPath,$Native) Initialize-CodeToolsRuntime $UserPath $ConfigPath $Native -Tool graphify } $fixtureUser $fixtureCodex $state.codexCommand
        Assert-True ($reusedGraphify.graphify_python -eq $withGraphify.graphify_python) 'Graphify bootstrap did not reuse its exact environment.'
    }
    $basePending = Read-CodeToolsJson (Join-Path $fixtureCodex 'harness/bootstrap-runtime-pending.json')
    $derived = Join-Path $basePending.python.target '__pycache__/harness_fixture.pyc'
    Write-CodeToolsBytes $derived ([byte[]]@(1,2,3))
    $foreign = Join-Path $basePending.python.target 'harness_foreign_source.py'
    Write-CodeToolsBytes $foreign ([Text.Encoding]::UTF8.GetBytes('# fixture user edit'))
    $conflict = $null
    try { Restore-HarnessActivation -SourceRoot $repository -UserHome $fixtureUser -CodexHome $fixtureCodex -CodexCommand $state.codexCommand -PathScope Process | Out-Null } catch { $conflict = $_.Exception.Message }
    Assert-True ($conflict -and $conflict.Contains('Bootstrap Python is incomplete or changed')) 'Modified source did not prevent base-runtime deletion.'
    Assert-True (Test-Path $foreign) 'Recovery removed modified base-runtime source.'
    Remove-CodeToolsFile $foreign
    Restore-HarnessActivation -SourceRoot $repository -UserHome $fixtureUser -CodexHome $fixtureCodex -CodexCommand $state.codexCommand -PathScope Process | Out-Null
    Assert-True (-not (Test-Path $runtime.python)) 'Owned Serena bootstrap did not roll back.'
    Assert-True (-not (Test-Path $runtime.uv)) 'Owned uv bootstrap did not roll back.'
    Assert-True (-not (Test-Path (Join-Path $fixtureCodex 'harness/bootstrap-pending.json'))) 'Serena rollback journal remains.'
    Assert-True (-not (Test-Path (Join-Path $fixtureCodex 'harness/bootstrap-runtime-pending.json'))) 'Base-runtime rollback journal remains.'
    if ($IncludeGraphify) { Assert-True (-not (Test-Path $withGraphify.graphify_python)) 'Owned Graphify environment did not roll back.' }
    Write-Output "Bootstrap checks passed: $assertions assertions. Retained download/evidence root: $root"
} catch { Write-Host "Bootstrap fixture retained: $root"; throw }
finally { $env:Path = $priorPath }
