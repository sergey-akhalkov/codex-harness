#requires -Version 7.4
[CmdletBinding(SupportsShouldProcess)]
param(
    [ValidateSet('Install', 'Update', 'Check', 'Disconnect', 'Recover', 'ConfigureRestart')]
    [string] $Mode = 'Install',
    [string] $CodexHome,
    [string] $UserHome = [Environment]::GetFolderPath('UserProfile'),
    [string] $DependencyUserHome,
    [string] $CodexCommand,
    [ValidateSet('User', 'Process')]
    [string] $PathScope = 'User',
    [switch] $CoreOnly,
    [switch] $SubscriptionsOnly,
    [switch] $CodeToolsOnly,
    [switch] $TokenWorkflowOnly,
    [switch] $Diagnose,
    [switch] $Detailed,
    [string] $ProjectPath = (Get-Location).Path
)
$ErrorActionPreference = 'Stop'
if (@($CoreOnly,$SubscriptionsOnly,$CodeToolsOnly,$TokenWorkflowOnly | Where-Object { $_ }).Count -gt 1) { throw 'Component selectors are mutually exclusive.' }
if ($Mode -eq 'ConfigureRestart' -and -not $SubscriptionsOnly) { throw 'ConfigureRestart requires -SubscriptionsOnly.' }
Import-Module (Join-Path $PSScriptRoot 'tools/kit.psm1') -Force
if (-not $CodexHome) {
    $CodexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $UserHome '.codex' }
}
if ($Diagnose) {
    if ($Mode -ne 'Check' -or $CoreOnly -or $SubscriptionsOnly -or $CodeToolsOnly -or $TokenWorkflowOnly) { throw '-Diagnose requires -Mode Check without component selectors.' }
    Import-Module (Join-Path $PSScriptRoot 'tools/source-diagnostics.psm1') -Force
    Invoke-HarnessSourceDiagnostics -SourceRoot $PSScriptRoot -CodexHome $CodexHome -UserHome $UserHome -ProjectPath $ProjectPath -CodexCommand $CodexCommand
    return
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
    if ((Test-Path -LiteralPath (Join-Path $CodexHome 'harness/subscription-restart-policy-pending.json')) -and
        -not $CodeToolsOnly -and -not ($SubscriptionsOnly -and $Mode -eq 'Recover')) {
        throw 'An interrupted restart-policy update requires -SubscriptionsOnly -Mode Recover before another operation.'
    }
    $useTokenWorkflow = $TokenWorkflowOnly -or (-not $CoreOnly -and -not $SubscriptionsOnly -and -not $CodeToolsOnly)
    $tokenResult = $null
    if ($useTokenWorkflow) {
        Import-Module (Join-Path $PSScriptRoot 'tools/token-workflow.psm1') -Force
        $tokenArgs = @{SourceRoot=$PSScriptRoot;CodexHome=$CodexHome;UserHome=$UserHome;CodexCommand=$common.CodexCommand;Mode=$Mode;Preview=$WhatIfPreference}
        # Detach before the core removes its registration; recover this owned
        # component first when a previous token activation was interrupted.
        if ($Mode -in @('Disconnect','Recover') -or $TokenWorkflowOnly) { $tokenResult = Invoke-HarnessTokenWorkflow @tokenArgs }
    }
    $result = if ($TokenWorkflowOnly) {
        $tokenResult
    } elseif ($CoreOnly) {
        if (Test-Path -LiteralPath (Join-Path $CodexHome 'harness/activation-pending.json')) { throw 'A combined activation is pending. Run Recover without -CoreOnly.' }
        Invoke-HarnessInstall @common -Mode $coreMode -PathScope $PathScope -Preview:$WhatIfPreference
    } elseif ($SubscriptionsOnly) {
        if (Test-Path -LiteralPath (Join-Path $CodexHome 'harness/activation-pending.json')) { throw 'A combined activation is pending. Run Recover without -SubscriptionsOnly.' }
        Import-Module (Join-Path $PSScriptRoot 'tools/subscription-routing.psm1') -Force
        Invoke-HarnessSubscriptionRouting @common -Mode $Mode -Preview:$WhatIfPreference
    } else {
        Import-Module (Join-Path $PSScriptRoot 'tools/activation.psm1') -Force
        Invoke-HarnessActivation @common -Mode $Mode -PathScope $PathScope -Preview:$WhatIfPreference -CodeToolsOnly:$CodeToolsOnly
    }
    if ($useTokenWorkflow -and -not $TokenWorkflowOnly -and $Mode -notin @('Disconnect','Recover')) { $tokenResult = Invoke-HarnessTokenWorkflow @tokenArgs }
    if ($Detailed -or $CoreOnly -or $SubscriptionsOnly -or $TokenWorkflowOnly -or $WhatIfPreference) {
        $result
        if ($tokenResult -and -not $TokenWorkflowOnly) { $tokenResult }
    } else {
        # Full discovery and dependency records are for explicit inspection.
        # Normal lifecycle output must not dump inventories into model context.
        $field = { param($value, $name)
            if ($value -is [Collections.IDictionary]) { return $value[$name] }
            if ($null -ne $value -and $value.PSObject.Properties[$name]) { return $value.$name }
        }
        $codeResult = if ($CodeToolsOnly) { $result } else { & $field $result 'codeTools' }
        $summary = [ordered]@{ status = (& $field $result 'status') }
        if ($tokenResult) { $summary.tokenWorkflow = $tokenResult.status }
        if ($codeResult) {
            $summary.codeTools = & $field $codeResult 'status'
            $registration = & $field $codeResult 'registration'
            $health = & $field $codeResult 'health'
            $resources = & $field $codeResult 'resources'
            if ($registration) { $summary.registrations = $registration.status }
            if ($health) {
                $summary.failures = @($health.servers | Where-Object status -eq 'failed' | ForEach-Object { "$($_.id): $($_.reason)" })
            }
            $reason = & $field $resources 'reason'
            if ($reason) { $summary.resourceProblem = $reason }
        }
        $summary.details = 'Use the same command with -Detailed for full discovery, dependency and recovery evidence.'
        [pscustomobject]$summary
    }
} finally {
    for ($index = $operationMutexes.Count - 1; $index -ge 0; $index--) { $operationMutexes[$index].ReleaseMutex(); $operationMutexes[$index].Dispose() }
}
