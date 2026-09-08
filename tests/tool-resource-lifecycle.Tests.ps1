#requires -Version 7.4
# Isolated lifecycle routing/ordering fixture. No native tool or service launches.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$root = Join-Path ([IO.Path]::GetTempPath()) ('harness-resource-routing-' + [guid]::NewGuid().ToString('N'))
$userDirectory = Join-Path $root 'user'
$codexDirectory = Join-Path $root 'codex'
$priorCodexHome = $env:CODEX_HOME
$assertions = 0
function Assert-True([bool]$Value, [string]$Reason) { if (-not $Value) { throw $Reason }; $script:assertions++ }
$module = Import-Module (Join-Path $repository 'tools/code-tools.psm1') -Force -PassThru
try {
    & $module {
        $script:calls = [Collections.Generic.List[object]]::new()
        $script:retireStatus = 'retired'
        function script:Get-HarnessCodeToolsRuntime($UserHome, $CodexHome, $CodexCommand) {
            @{ python = 'fixture-python'; lifecycle_python = 'fixture-python'; registry = (Join-Path $CodexHome 'harness/code-tools.json') }
        }
        function script:Invoke-CodeToolsPythonJson($Python, [string[]]$Arguments) {
            $script:calls.Add(@{ python = $Python; arguments = $Arguments; codex_home = $env:CODEX_HOME })
            if ($Arguments[-1] -eq '--retire') { return @{ status = $script:retireStatus } }
            @{ status = 'applied' }
        }
    }
    Write-CodeToolsJson (Join-Path $codexDirectory 'harness/code-tools.json') @{ mcp = @(@{ id = 'codebase-memory'; paths = @{ native_executable = 'never-launched' } }) }
    Invoke-CodeToolsResources $repository $userDirectory $codexDirectory 'fixture-codex' -Mode apply -Preview | Out-Null
    Assert-True ((& $module { $script:calls.Count }) -eq 0) 'Preview invoked configuration mutation.'
    Invoke-CodeToolsResources $repository $userDirectory $codexDirectory 'fixture-codex' -Mode apply -DeferCommit -TransactionId 'owned-transaction' | Out-Null
    $call = & $module { $script:calls[-1] }
    Assert-True ($call.arguments -contains '--defer-commit') 'Outer activation did not defer resource commit.'
    Assert-True ($call.arguments -contains (Join-Path $userDirectory '.cache/codebase-memory-mcp')) 'Fixture fell through to live account cache.'
    Assert-True ($call.arguments -contains (Join-Path $codexDirectory 'harness/tool-resources-pending.json')) 'Resource journal escaped activation home.'
    Assert-True ($call.arguments -contains 'owned-transaction') 'Outer transaction identity was omitted.'
    $null = New-Item -ItemType Directory -Path (Join-Path $codexDirectory 'harness/runtime/lsp-broker') -Force
    $null = New-Item -ItemType Directory -Path (Join-Path $codexDirectory 'harness/runtime/serena-broker') -Force
    Stop-CodeToolsServices $repository $userDirectory $codexDirectory 'fixture-codex'
    $calls = & $module { $script:calls.ToArray() }
    Assert-True ($calls.Count -eq 3) 'Both existing services were not retired.'
    Assert-True ($calls[1].arguments[0] -eq (Join-Path $repository 'tools/lsp/broker.py')) 'Incorrect LSP retirement entrypoint.'
    Assert-True ($calls[2].arguments[0] -eq (Join-Path $repository 'tools/code-tools/serena_broker.py')) 'Incorrect Serena retirement entrypoint.'
    Assert-True ($calls[1].codex_home -eq $codexDirectory -and $calls[2].codex_home -eq $codexDirectory) 'Retirement used another installation home.'
    Assert-True ($env:CODEX_HOME -eq $priorCodexHome) 'Retirement leaked CODEX_HOME.'
    & $module { $script:retireStatus = 'draining' }
    $failure = $null
    try { Stop-CodeToolsServices $repository $userDirectory $codexDirectory 'fixture-codex' } catch { $failure = $_.Exception.Message }
    Assert-True ($failure -match 'pending') 'Incomplete retirement was accepted as stopped.'
    Assert-True ($env:CODEX_HOME -eq $priorCodexHome) 'Failed retirement leaked CODEX_HOME.'
    Write-CodeToolsJson (Join-Path $codexDirectory 'harness/tool-resources-pending.json') @{ fixture = $true }
    Invoke-CodeToolsResources $repository $userDirectory $codexDirectory 'fixture-codex' -Mode recover | Out-Null
    Assert-True ((& $module { $script:calls[-1].arguments[1] }) -eq 'recover') 'Recover did not route to resource inverse.'
    Invoke-CodeToolsResources $repository $userDirectory $codexDirectory 'fixture-codex' -Mode commit -TransactionId 'owned-transaction' | Out-Null
    Assert-True ((& $module { $script:calls[-1].arguments[1] }) -eq 'commit') 'Committed activation did not finalize resource journal.'
    $activationModule = Import-Module (Join-Path $repository 'tools/activation.psm1') -Force -PassThru
    & $activationModule {
        $script:order = [Collections.Generic.List[string]]::new()
        function script:Stop-CodeToolsServices { $script:order.Add('retire') }
        function script:Invoke-CodeToolsResources {
            param($SourceRoot, $UserHome, $CodexHome, $CodexCommand, $Mode, $TransactionId, [switch]$Preview)
            $script:order.Add($Mode)
            if ($Mode -eq 'commit' -and (Read-CodeToolsJson (Join-Path $CodexHome 'harness/activation-pending.json')).phase -ne 'committed') { throw 'Resource journal committed before durable outer decision.' }
            Remove-CodeToolsFile (Join-Path $CodexHome 'harness/tool-resources-pending.json')
        }
        function script:Restore-HarnessSubscriptionRouting {}
        function script:Restore-CodeToolsRegistries {}
        function script:Restore-CodeToolsRegistration {}
        function script:Invoke-HarnessInstall {}
        function script:Restore-CodeToolsBootstrap { $script:order.Add('dependency-restore') }
        function script:Restore-BootstrapBase {}
        function script:Resume-HarnessSubscriptionRouting {}
    }
    $record = @{ schema_version = 1; owner = 'codex-harness-activation'; id = [guid]::NewGuid().ToString('N'); phase = 'prepared'; source_root = $repository; user_home = $userDirectory; dependency_user_home = $userDirectory; codex_home = $codexDirectory; codex_command = 'fixture-codex' }
    Write-CodeToolsJson (Join-Path $codexDirectory 'harness/activation-pending.json') $record
    Restore-HarnessActivation -SourceRoot $repository -UserHome $userDirectory -CodexHome $codexDirectory -CodexCommand 'fixture-codex' | Out-Null
    $order = & $activationModule { $script:order.ToArray() }
    Assert-True ($order[0] -eq 'retire' -and $order[1] -eq 'recover') 'Recovery changed resources/dependencies before retiring services.'
    Assert-True ($order[2] -eq 'dependency-restore') 'Resource recovery did not precede dependency rollback.'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $codexDirectory 'harness/activation-pending.json'))) 'Successful recovery left admission blocked.'
    $record.phase = 'committed'
    Write-CodeToolsJson (Join-Path $codexDirectory 'harness/activation-pending.json') $record
    Write-CodeToolsJson (Join-Path $codexDirectory 'harness/tool-resources-pending.json') @{ fixture = $true }
    Restore-HarnessActivation -SourceRoot $repository -UserHome $userDirectory -CodexHome $codexDirectory -CodexCommand 'fixture-codex' | Out-Null
    Assert-True ((& $activationModule { $script:order[-1] }) -eq 'commit') 'Committed Recover rolled back resource settings.'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $codexDirectory 'harness/tool-resources-pending.json'))) 'Committed cleanup left resource ownership pending.'
    Remove-Module $activationModule
    Write-Output "PASS: $assertions resource lifecycle routing assertions"
} finally {
    $env:CODEX_HOME = $priorCodexHome
    # One shell, verified exact owned temporary root, no link traversal.
    if ([IO.Path]::GetFullPath($root).StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase)) {
        Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-Module $module -ErrorAction SilentlyContinue
}
