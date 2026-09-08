#requires -Version 7.4
# Actual registration/registry coordinator; package and native resource effects
# are isolated fixtures. No model probes, package installs or global mutations.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$root = Join-Path ([IO.Path]::GetTempPath()) ('harness-scoped-activation-' + [guid]::NewGuid().ToString('N'))
$fixtureHome = Join-Path $root 'codex'
$fixtureUser = Join-Path $root 'user'
$actualUser = [Environment]::GetFolderPath('UserProfile')
$installation = Get-Content (Join-Path $actualUser '.codex/harness/installation.json') -Raw | ConvertFrom-Json
$code = Import-Module (Join-Path $repository 'tools/code-tools.psm1') -Force -PassThru
$runtime = Get-HarnessCodeToolsRuntime $actualUser $fixtureHome $installation.codexCommand
$activation = Import-Module (Join-Path $repository 'tools/activation.psm1') -Force -PassThru
$assertions = 0
function Assert-True([bool]$Value, [string]$Reason) { if (-not $Value) { throw $Reason }; $script:assertions++ }
function Snapshot {
    $value = @{}
    foreach ($name in @('config.toml','harness/code-tools.json','harness/lsp-servers.json','harness/resource-fixture.json','harness/subscription-routing-pending.json','harness/pending.json','harness/bootstrap-pending.json')) {
        $value[$name] = [Convert]::ToBase64String((Get-CodeToolsBytes (Join-Path $fixtureHome $name)))
    }
    $value
}
try {
    & $code {
        param($Runtime)
        $script:fixtureRuntime = $Runtime
        $script:realJson = (Get-Command Invoke-CodeToolsPythonJson).ScriptBlock
        function script:Get-HarnessCodeToolsRuntime { $script:fixtureRuntime }
        function script:Invoke-CodeToolsPythonJson($Python, [string[]]$Arguments) {
            switch ([IO.Path]::GetFileName($Arguments[0])) {
                'dependencies.py' { throw 'Scoped activation attempted dependency mutation.' }
                'discovery.py' { return @{ mcp = @(@{ id = 'codebase-memory'; paths = @{ native_executable = 'fixture-only' } }); languages = @() } }
                'registry.py' { return @{ schema_version = 1; servers = @{} } }
                'check.py' { return @{ status = 'protocol-ready' } }
                'resources.py' {
                    $pending = $Arguments[[Array]::IndexOf($Arguments, '--pending') + 1]
                    $state = Join-Path (Split-Path $pending) 'resource-fixture.json'
                    switch ($Arguments[1]) {
                        'apply' { Write-CodeToolsJson $pending @{ before = (Read-CodeToolsJson $state) }; Write-CodeToolsJson $state @{ active = $true } }
                        'recover' { $record = Read-CodeToolsJson $pending; if ($record) { Write-CodeToolsJson $state $record.before; Remove-CodeToolsFile $pending } }
                        'commit' { Remove-CodeToolsFile $pending }
                        'check' {}
                    }
                    return @{ status = 'active' }
                }
            }
            & $script:realJson $Python $Arguments
        }
    } $runtime
    & $activation {
        function script:Initialize-CodeToolsRuntime { throw 'Scoped activation attempted bootstrap.' }
        function script:Invoke-HarnessInstall { throw 'Scoped activation touched core state.' }
        function script:Invoke-HarnessSubscriptionRouting { throw 'Scoped activation touched subscriptions.' }
        function script:Restore-HarnessSubscriptionRouting { throw 'Scoped recovery touched subscriptions.' }
        function script:Restore-CodeToolsBootstrap { throw 'Scoped recovery touched packages.' }
    }
    Write-CodeToolsBytes (Join-Path $fixtureHome 'config.toml') ([Text.Encoding]::UTF8.GetBytes("# foreign configuration`nmodel = 'gpt-6-astra'`n"))
    Write-CodeToolsJson (Join-Path $fixtureHome 'harness/code-tools.json') @{ mcp = @(); old = $true }
    Write-CodeToolsJson (Join-Path $fixtureHome 'harness/lsp-servers.json') @{ old = $true }
    Write-CodeToolsJson (Join-Path $fixtureHome 'harness/resource-fixture.json') @{ active = $false }
    foreach ($name in @('subscription-routing-pending.json','pending.json','bootstrap-pending.json')) {
        Write-CodeToolsJson (Join-Path $fixtureHome ('harness/' + $name)) @{ unrelated = $name }
    }
    $common = @{ SourceRoot = $repository; UserHome = $fixtureUser; CodexHome = $fixtureHome; CodexCommand = $installation.codexCommand; CodeToolsOnly = $true }
    $before = Snapshot
    Invoke-HarnessActivation @common -Mode Install -Preview | Out-Null
    foreach ($key in $before.Keys) { Assert-True ($before[$key] -ceq (Snapshot)[$key]) "Preview changed $key" }
    foreach ($phase in @('registration','registry:lsp-servers.json','resources','before-commit')) {
        $failure = $null
        try { Invoke-HarnessActivation @common -Mode Install -Checkpoint { param($value) if ($value -eq $phase) { throw "injected $phase" } } | Out-Null }
        catch { $failure = $_.Exception.Message }
        Assert-True ($failure -match 'injected') "Checkpoint not reached: $failure"
        foreach ($key in $before.Keys) { Assert-True ($before[$key] -ceq (Snapshot)[$key]) "Rollback after $phase changed $key" }
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixtureHome 'harness/activation-pending.json'))) 'Scoped rollback remained pending.'
    }
    $failure = $null
    try { Invoke-HarnessActivation @common -Mode Install -Checkpoint { param($phase) if ($phase -eq 'committed') { throw 'simulated crash after commit marker' } } | Out-Null }
    catch { $failure = $_.Exception.Message }
    Assert-True ($failure -match 'committed') 'Committed checkpoint was not retained.'
    $pending = Read-CodeToolsJson (Join-Path $fixtureHome 'harness/activation-pending.json')
    Assert-True ($pending.scope -eq 'code-tools' -and $pending.phase -eq 'committed') 'Durable marker lost scope/commit.'
    Invoke-HarnessActivation @common -Mode Recover | Out-Null
    Assert-True ((Read-CodeToolsJson (Join-Path $fixtureHome 'harness/resource-fixture.json')).active) 'Committed Recover rolled back resources.'
    foreach ($key in @('harness/subscription-routing-pending.json','harness/pending.json','harness/bootstrap-pending.json')) {
        Assert-True ($before[$key] -ceq (Snapshot)[$key]) "Commit cleanup removed unrelated $key"
    }
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixtureHome 'harness/activation-pending.json'))) 'Committed cleanup retained admission marker.'
    Invoke-HarnessActivation @common -Mode Recover | Out-Null
    foreach ($key in @('harness/subscription-routing-pending.json','harness/pending.json','harness/bootstrap-pending.json')) {
        Assert-True ($before[$key] -ceq (Snapshot)[$key]) "Repeated scoped Recover touched unrelated $key"
    }
    # Exercise the shipped parameter routing after all module-mocked cases.
    & (Join-Path $repository 'install.ps1') -CodeToolsOnly -Mode Recover -UserHome $fixtureUser -CodexHome $fixtureHome -CodexCommand $installation.codexCommand | Out-Null
    foreach ($key in @('harness/subscription-routing-pending.json','harness/pending.json','harness/bootstrap-pending.json')) {
        Assert-True ($before[$key] -ceq (Snapshot)[$key]) "CLI scoped Recover touched unrelated $key"
    }
    Write-Output "PASS: $assertions scoped activation assertions"
} finally {
    Remove-Module $activation, $code -ErrorAction SilentlyContinue
    if ([IO.Path]::GetFullPath($root).StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase)) {
        Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
    }
}
