#requires -Version 7.4
[CmdletBinding(SupportsShouldProcess)]
param(
    [ValidateSet('Install', 'Update', 'Check', 'Disconnect', 'Recover')]
    [string] $Mode = 'Install',
    [string] $CodexHome,
    [string] $UserHome = [Environment]::GetFolderPath('UserProfile'),
    [string] $DependencyUserHome,
    [string] $CodexCommand,
    [ValidateSet('User', 'Process')]
    [string] $PathScope = 'User',
    [switch] $CoreOnly
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'tools/kit.psm1') -Force
if (-not $CodexHome) {
    $CodexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $UserHome '.codex' }
}
$coreMode = if ($Mode -eq 'Update') { 'Install' } else { $Mode }
if (-not $DependencyUserHome) { $DependencyUserHome = $UserHome }
$common = @{ SourceRoot = $PSScriptRoot; CodexHome = $CodexHome; UserHome = $UserHome; DependencyUserHome = $DependencyUserHome; CodexCommand = $CodexCommand }
if (-not $CoreOnly -and -not $CodexCommand) {
    $priorStatePath = Join-Path $CodexHome 'harness/installation.json'
    if (Test-Path -LiteralPath $priorStatePath) { $common.CodexCommand = (Get-Content -LiteralPath $priorStatePath -Raw | ConvertFrom-Json).codexCommand }
}
$operationMutexes = [Collections.Generic.List[Threading.Mutex]]::new()
try {
    # Independent connection roots may share one explicitly selected dependency
    # owner. Serialize both identities in deterministic order, including bootstrap.
    foreach ($owner in @(@($UserHome,$DependencyUserHome) | ForEach-Object { [IO.Path]::GetFullPath($_).TrimEnd('\').ToLowerInvariant() } | Sort-Object -Unique)) {
        $lockIdentity = [Text.Encoding]::UTF8.GetBytes($owner)
        $mutex = [Threading.Mutex]::new($false, ('Local\CodexHarness-' + [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($lockIdentity))))
        try { $held = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $held = $true }
        if (-not $held) { $mutex.Dispose(); throw 'Another harness operation is active for this connection or dependency owner. Wait for it to finish.' }
        $operationMutexes.Add($mutex)
    }
    if ($CoreOnly) {
        if (Test-Path -LiteralPath (Join-Path $CodexHome 'harness/activation-pending.json')) { throw 'A combined activation is pending. Run Recover without -CoreOnly.' }
        Invoke-HarnessInstall @common -Mode $coreMode -PathScope $PathScope -Preview:$WhatIfPreference
    } else {
        Import-Module (Join-Path $PSScriptRoot 'tools/activation.psm1') -Force
        Invoke-HarnessActivation @common -Mode $Mode -PathScope $PathScope -Preview:$WhatIfPreference
    }
} finally {
    for ($index = $operationMutexes.Count - 1; $index -ge 0; $index--) { $operationMutexes[$index].ReleaseMutex(); $operationMutexes[$index].Dispose() }
}
